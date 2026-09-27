//! The WebView2 engine: one shared `CoreWebView2Environment` with browser extensions enabled,
//! and the engine side of extensions (load an unpacked folder, list, enable, remove).
//!
//! Every web view in the process must use this environment: WebView2 refuses a second
//! environment on the same user data folder with different options (ERROR_INVALID_STATE).

use std::path::Path;

use windows_core::imp::IGenericFactory;
use windows_core::{HSTRING, IInspectable, Interface, PCSTR, Result, s, w};
use windows_future::IAsyncOperation;

use crate::bindings::*;

pub(crate) struct Engine {
    environment: CoreWebView2Environment,
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
}

impl EngineExtension {
    fn new(handle: &CoreWebView2BrowserExtension) -> Result<Self> {
        Ok(Self {
            id: handle.Id()?,
            name: handle.Name()?,
            enabled: handle.IsEnabled()?,
        })
    }

    /// Extensions WebView2 installs on its own; the shell hides them.
    pub fn is_builtin(&self) -> bool {
        matches!(
            self.name.as_str(),
            "Microsoft Clipboard Extension" | "Microsoft Edge PDF Viewer"
        )
    }
}

/// Loads an unpacked extension folder. WebView2 persists it in the profile, keeps the folder in
/// place, and drops the extension if the folder's files change afterwards.
pub(crate) async fn add_extension(
    profile: &CoreWebView2Profile,
    dir: &Path,
) -> Result<EngineExtension> {
    let added = profile
        .cast::<ICoreWebView2Profile7>()?
        .AddBrowserExtensionAsync(&dir.to_string_lossy())?
        .await?;
    EngineExtension::new(&added)
}

pub(crate) async fn extensions(profile: &CoreWebView2Profile) -> Result<Vec<EngineExtension>> {
    let list = profile
        .cast::<CoreWebView2Profile_Manual3>()?
        .GetBrowserExtensionsAsync()?
        .await?;
    let mut out = Vec::new();
    for handle in &list {
        out.push(EngineExtension::new(&handle)?);
    }
    Ok(out)
}
