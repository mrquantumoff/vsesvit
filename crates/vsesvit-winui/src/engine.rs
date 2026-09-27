//! The WebView2 engine: one shared `CoreWebView2Environment` with browser extensions enabled,
//! and the engine side of extensions (load a folder, list, enable, remove).
//!
//! Every web view in the process must use this environment: WebView2 refuses a second
//! environment on the same user data folder with different options (ERROR_INVALID_STATE).

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_core::imp::IGenericFactory;
use windows_core::{HSTRING, IInspectable, Interface, PCSTR, Result, s, w};
use windows_future::IAsyncOperation;

use crate::bindings::*;

pub(crate) struct Engine {
    environment: CoreWebView2Environment,
}

/// The installed WebView2 runtime's version, as `CoreWebView2Environment` reports it
/// (`"154.0.4258.37"`, possibly followed by a channel name).
pub(crate) fn available_version() -> Result<String> {
    let statics = app_local_factory::<ICoreWebView2EnvironmentStatics>(
        "Microsoft.Web.WebView2.Core.CoreWebView2Environment",
    )?;
    unsafe {
        let mut version = std::ptr::null_mut();
        (Interface::vtable(&statics).GetAvailableBrowserVersionString)(
            Interface::as_raw(&statics),
            &mut version,
        )
        .ok()?;
        let version: HSTRING = std::mem::transmute(version);
        Ok(version.to_string_lossy())
    }
}

/// The Chromium version inside a WebView2 version string: its first word when that is four
/// dot-separated numbers.
pub(crate) fn chromium_version(version: &str) -> Option<String> {
    let first = version.split_whitespace().next()?;
    let parts: Vec<&str> = first.split('.').collect();
    let numeric = |p: &&str| !p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit());
    (parts.len() == 4 && parts.iter().all(numeric)).then(|| first.to_owned())
}

impl Engine {
    pub async fn create(user_data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(user_data_dir).map_err(|e| {
            windows_core::Error::new(E_FAIL, format!("{}: {e}", user_data_dir.display()))
        })?;
        let options: CoreWebView2EnvironmentOptions = app_local_factory::<IGenericFactory>(
            "Microsoft.Web.WebView2.Core.CoreWebView2EnvironmentOptions",
        )?
        .ActivateInstance()?;
        options
            .cast::<ICoreWebView2EnvironmentOptions6>()?
            .SetAreBrowserExtensionsEnabled(true)?;
        let statics = app_local_factory::<ICoreWebView2EnvironmentStatics>(
            "Microsoft.Web.WebView2.Core.CoreWebView2Environment",
        )?;
        let browser_folder = HSTRING::new();
        let user_data = HSTRING::from(user_data_dir.as_os_str());
        // Minimal bindings give activation-factory interfaces no methods, so call the vtable.
        let operation: IAsyncOperation<CoreWebView2Environment> = unsafe {
            let mut result = std::ptr::null_mut();
            (Interface::vtable(&statics).CreateWithOptionsAsync)(
                Interface::as_raw(&statics),
                std::mem::transmute_copy(&browser_folder),
                std::mem::transmute_copy(&user_data),
                Interface::as_raw(&options),
                &mut result,
            )
            .ok()?;
            IAsyncOperation::from_raw(result)
        };
        let environment = operation.await?;
        log::info!(
            "WebView2 runtime {} with user data in {}",
            environment.BrowserVersionString().unwrap_or_default(),
            user_data_dir.display()
        );
        Ok(Self { environment })
    }

    pub fn environment(&self) -> &CoreWebView2Environment {
        &self.environment
    }

    pub fn browser_version(&self) -> String {
        self.environment.BrowserVersionString().unwrap_or_default()
    }

    pub fn find_options(&self, term: &str) -> Result<CoreWebView2FindOptions> {
        let options = self
            .environment
            .cast::<ICoreWebView2Environment15>()?
            .CreateFindOptions()?;
        options.SetFindTerm(term)?;
        options.SetSuppressDefaultFindDialog(false)?;
        Ok(options)
    }
}

/// The Windows App Runtime registers `Microsoft.Web.WebView2.Core.*` but does not ship the DLL,
/// so `RoGetActivationFactory` fails with 0x8007007E. The DLL next to the exe (placed there by
/// build.rs) provides the factories.
fn app_local_factory<I: Interface>(class: &str) -> Result<I> {
    type DllGetActivationFactory = unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *mut *mut std::ffi::c_void,
    ) -> windows_core::HRESULT;
    const LOAD_LIBRARY_SEARCH_APPLICATION_DIR: u32 = 0x200;
    unsafe {
        let module = LoadLibraryExW(
            w!("Microsoft.Web.WebView2.Core.dll"),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_APPLICATION_DIR,
        );
        if module.is_null() {
            return Err(windows_core::Error::from_thread());
        }
        let entry: PCSTR = s!("DllGetActivationFactory");
        let Some(proc) = GetProcAddress(module, entry) else {
            return Err(windows_core::Error::from_thread());
        };
        let get: DllGetActivationFactory = std::mem::transmute(proc);
        let name = HSTRING::from(class);
        let mut factory = std::ptr::null_mut();
        get(std::mem::transmute_copy(&name), &mut factory).ok()?;
        IInspectable::from_raw(factory).cast()
    }
}

/// An extension as the engine sees it.
pub(crate) struct EngineExtension {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    handle: CoreWebView2BrowserExtension,
}

/// Extensions WebView2 installs on its own. The shell neither lists nor removes them.
const BUILTIN_EXTENSIONS: [&str; 2] =
    ["Microsoft Clipboard Extension", "Microsoft Edge PDF Viewer"];

impl EngineExtension {
    fn new(handle: CoreWebView2BrowserExtension) -> Result<Self> {
        Ok(Self {
            id: handle.Id()?,
            name: handle.Name()?,
            enabled: handle.IsEnabled()?,
            handle,
        })
    }

    pub fn is_builtin(&self) -> bool {
        BUILTIN_EXTENSIONS.contains(&self.name.as_str())
    }

    pub async fn remove(&self) -> Result<()> {
        self.handle.RemoveAsync()?.await
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<()> {
        self.handle.EnableAsync(enabled)?.await
    }
}

/// Loads an unpacked extension folder. WebView2 persists it in the profile, keeps the folder in
/// place, and drops the extension if the folder's files change afterwards.
pub(crate) async fn add_extension(
    profile: &CoreWebView2Profile,
    dir: &Path,
) -> Result<EngineExtension> {
    let walked = dir.to_owned();
    let longest = crate::exec::background(move || longest_relative_path(&walked)).await;
    let path = engine_path(dir, longest);
    if path != dir.to_string_lossy() {
        log::info!("{} is long; the engine loads it as {path}", dir.display());
    }
    let added = profile
        .cast::<ICoreWebView2Profile7>()?
        .AddBrowserExtensionAsync(&path)?
        .await?;
    EngineExtension::new(added)
}

/// Room for what the engine writes into the folder itself, such as
/// `_metadata\generated_indexed_rulesets\_ruleset1`.
const ENGINE_METADATA_CHARS: usize = 64;
/// `MAX_PATH` without the terminating NUL.
const MAX_PATH_CHARS: usize = 259;

/// The path the engine gets for `dir`, whose longest entry has `longest_relative` characters.
/// The engine fails with E_FAIL when a file in the folder would pass `MAX_PATH`, and accepts the
/// same folder in extended-length form (up to a limit of its own, see
/// `extensions::DEEP_FOLDER_CHARS`). A folder that fits keeps its plain path, from which
/// Chromium derives the id of an extension without a `key`.
fn engine_path(dir: &Path, longest_relative: usize) -> String {
    let plain = dir.to_string_lossy();
    let longest = plain.encode_utf16().count() + 1 + longest_relative.max(ENGINE_METADATA_CHARS);
    match extended_length(dir) {
        Some(extended) if longest > MAX_PATH_CHARS => extended,
        _ => plain.into_owned(),
    }
}

/// The longest path of anything below `dir`, relative to it, in UTF-16 units.
fn longest_relative_path(dir: &Path) -> usize {
    let mut longest = 0;
    let mut pending = vec![dir.to_owned()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(relative) = path.strip_prefix(dir) {
                longest = longest.max(relative.as_os_str().encode_wide().count());
            }
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                pending.push(path);
            }
        }
    }
    longest
}

/// `C:\dir` as `\\?\C:\dir`; `None` for paths that are not absolute drive paths.
fn extended_length(dir: &Path) -> Option<String> {
    let text = dir.to_str()?;
    let bytes = text.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    drive.then(|| format!(r"\\?\{text}"))
}

pub(crate) async fn extensions(profile: &CoreWebView2Profile) -> Result<Vec<EngineExtension>> {
    let list = profile
        .cast::<CoreWebView2Profile_Manual3>()?
        .GetBrowserExtensionsAsync()?
        .await?;
    let mut out = Vec::new();
    for handle in &list {
        out.push(EngineExtension::new(handle)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{chromium_version, engine_path, extended_length};

    #[test]
    fn long_folders_get_the_extended_form() {
        assert_eq!(engine_path(Path::new(r"C:\p\ext"), 20), r"C:\p\ext");
        let deep = format!(r"C:\{}", "d".repeat(180));
        let deep = Path::new(&deep);
        assert_eq!(
            engine_path(deep, 20),
            deep.to_str().unwrap(),
            "the engine's own files still fit"
        );
        assert_eq!(engine_path(deep, 90), format!(r"\\?\{}", deep.display()));
    }

    #[test]
    fn extended_length_paths_only_for_drive_paths() {
        assert_eq!(
            extended_length(Path::new(r"C:\p\ext")).as_deref(),
            Some(r"\\?\C:\p\ext")
        );
        assert_eq!(extended_length(Path::new(r"\\?\C:\p")), None);
        assert_eq!(extended_length(Path::new(r"\\server\share\ext")), None);
        assert_eq!(extended_length(Path::new("ext")), None);
    }

    #[test]
    fn chromium_version_is_the_numeric_first_word() {
        assert_eq!(
            chromium_version("154.0.4258.37").as_deref(),
            Some("154.0.4258.37")
        );
        assert_eq!(
            chromium_version("156.0.4300.0 canary").as_deref(),
            Some("156.0.4300.0")
        );
        assert_eq!(chromium_version(""), None);
        assert_eq!(chromium_version("154.0.4258"), None);
        assert_eq!(chromium_version("a.b.c.d"), None);
    }
}
