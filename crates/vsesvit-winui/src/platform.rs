//! Process setup that has to happen before XAML starts, and the few Win32 calls the shell needs.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use windows_core::{GUID, HRESULT, HSTRING, IUnknown, Interface, PCWSTR, PWSTR, w};

use crate::bindings::*;

const RUNTIME_FAMILY: PCWSTR = w!("Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe");
/// Windows App Runtime 2.5.1.0, the runtime of Windows App SDK 2.5.1.
const RUNTIME_MIN_VERSION: u64 = (2 << 48) | (5 << 32) | (1 << 16);
const RUNTIME_DOWNLOAD: &str = "https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads";

#[derive(Debug)]
pub(crate) enum StartupError {
    Com(windows_core::Error),
    RuntimeMissing,
    Runtime(windows_core::Error),
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Com(e) => write!(f, "COM initialization failed: {e}"),
            Self::RuntimeMissing => write!(
                f,
                "the Windows App Runtime 2.x (version 2.5.1 or newer, {}) is not installed",
                arch_name()
            ),
            Self::Runtime(e) => write!(f, "could not load the Windows App Runtime 2.x: {e}"),
        }
    }
}

impl std::error::Error for StartupError {}

pub(crate) fn init() -> Result<(), StartupError> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32)
            .ok()
            .map_err(StartupError::Com)?;
    }
    add_runtime_dependency()
}

/// The OS dynamic-dependency API (Windows 11) puts the framework package into the process
/// package graph, so no bootstrapper DLL ships with the app.
fn add_runtime_dependency() -> Result<(), StartupError> {
    let classify = |hr: HRESULT| {
        if hr == STATEREPOSITORY_E_DEPENDENCY_NOT_RESOLVED {
            StartupError::RuntimeMissing
        } else {
            StartupError::Runtime(hr.into())
        }
    };
    unsafe {
        let mut id = PWSTR::null();
        TryCreatePackageDependency(
            std::ptr::null_mut(),
            RUNTIME_FAMILY,
            PACKAGE_VERSION {
                Anonymous: PACKAGE_VERSION_0 {
                    Version: RUNTIME_MIN_VERSION,
                },
            },
            arch_flag() | PackageDependencyProcessorArchitectures_Neutral,
            PackageDependencyLifetimeKind_Process,
            PCWSTR::null(),
            CreatePackageDependencyOptions_None,
            &mut id,
        )
        .ok()
        .map_err(|e| classify(e.code()))?;
        let mut context = std::ptr::null_mut();
        let mut full_name = PWSTR::null();
        let added = AddPackageDependency(
            PCWSTR(id.0),
            0,
            AddPackageDependencyOptions_None,
            &mut context,
            &mut full_name,
        );
        if added.is_ok() {
            log::info!(
                "Windows App Runtime: {}",
                full_name.to_string().unwrap_or_default()
            );
        }
        let heap = GetProcessHeap();
        let _ = HeapFree(heap, 0, id.0.cast());
        let _ = HeapFree(heap, 0, full_name.0.cast());
        added.ok().map_err(|e| classify(e.code()))
    }
}

fn arch_flag() -> PackageDependencyProcessorArchitectures {
    if cfg!(target_arch = "aarch64") {
        PackageDependencyProcessorArchitectures_Arm64
    } else if cfg!(target_arch = "x86") {
        PackageDependencyProcessorArchitectures_X86
    } else {
        PackageDependencyProcessorArchitectures_X64
    }
}

fn arch_name() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else {
        "x64"
    }
}

pub(crate) fn report_startup_failure(error: &StartupError) {
    match error {
        StartupError::RuntimeMissing => {
            let text = format!(
                "Vsesvit needs the Windows App Runtime 2.x (version 2.5.1 or newer, {}).\n\n\
                 Open the download page now?",
                arch_name()
            );
            if message_box(&text, MB_YESNO | MB_ICONERROR) == IDYES {
                open_in_shell(RUNTIME_DOWNLOAD.as_ref());
            }
        }
        other => {
            message_box(
                &format!("Vsesvit could not start: {other}"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

/// Release builds are GUI-subsystem programs with no console, so text printed for a terminal
/// is lost unless the process joins the console of the shell that started it. Redirected
/// output keeps its handles either way.
pub(crate) fn attach_parent_console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

pub(crate) fn message_box(text: &str, style: i32) -> i32 {
    let text = windows_core::HSTRING::from(text);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            PCWSTR(text.as_ptr()),
            w!("Vsesvit"),
            style as u32,
        )
    }
}

/// Opens a URL, file or folder with its default program.
pub(crate) fn open_in_shell(target: &OsStr) {
    let target = HSTRING::from(target);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            w!("open"),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Where Vsesvit stands as the browser Windows opens links with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DefaultBrowser {
    Vsesvit,
    Other,
    /// Windows does not know Vsesvit as a browser: this copy was not installed by the
    /// installer, which registers it.
    Unregistered,
}

/// The names `packaging/windows/installer.nsi` registers Vsesvit under.
const REGISTERED_NAME: &str = "Vsesvit";
const PROG_ID: &str = "VsesvitHTML";

pub(crate) fn default_browser() -> DefaultBrowser {
    if user_registry_string(r"Software\RegisteredApplications", REGISTERED_NAME).is_none() {
        return DefaultBrowser::Unregistered;
    }
    let choice = user_registry_string(
        r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
        "ProgId",
    );
    if choice.as_deref() == Some(PROG_ID) {
        DefaultBrowser::Vsesvit
    } else {
        DefaultBrowser::Other
    }
}

/// Windows Settings' page for Vsesvit's default apps. Windows keeps the choice itself to the
/// user; no app may make itself the default.
pub(crate) fn open_default_apps_settings() {
    open_in_shell(format!("ms-settings:defaultapps?registeredAppUser={REGISTERED_NAME}").as_ref());
}

/// A string value under `HKEY_CURRENT_USER`.
fn user_registry_string(key: &str, value: &str) -> Option<String> {
    let (key, value) = (HSTRING::from(key), HSTRING::from(value));
    let read = |data: *mut u16, bytes: &mut u32| unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ as u32,
            std::ptr::null_mut(),
            data.cast(),
            bytes,
        )
    };
    let mut bytes = 0;
    if read(std::ptr::null_mut(), &mut bytes) != 0 {
        return None;
    }
    let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
    if read(buffer.as_mut_ptr(), &mut bytes) != 0 {
        return None;
    }
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

/// Opens File Explorer on the folder that holds `file`, with `file` selected.
pub(crate) fn show_in_folder(file: &Path) {
    // Explorer reads its own command line: the path is quoted inside the one argument.
    let mut select = OsString::from("/select,\"");
    select.push(file);
    select.push("\"");
    if let Err(e) = std::process::Command::new("explorer.exe")
        .raw_arg(select)
        .spawn()
    {
        log::warn!("show {} in its folder: {e}", file.display());
    }
}

/// The user's Downloads folder, wherever they moved it.
pub(crate) fn downloads_folder() -> windows_core::Result<PathBuf> {
    const FOLDERID_DOWNLOADS: GUID = GUID::from_u128(0x374de290_123f_4565_9164_39c4925e467b);
    unsafe {
        let mut path = PWSTR::null();
        let found = SHGetKnownFolderPath(
            &FOLDERID_DOWNLOADS,
            KF_FLAG_DEFAULT,
            std::ptr::null_mut(),
            &mut path,
        )
        .ok()
        .map(|()| PathBuf::from(OsString::from_wide(path.as_wide())));
        CoTaskMemFree(path.0.cast());
        found
    }
}

#[windows_core::interface("eecdbf0e-bae9-4cb6-a68e-9598e1cb57bb")]
unsafe trait IWindowNative: IUnknown {
    fn window_handle(&self, hwnd: *mut HWND) -> HRESULT;
}

/// The Win32 window behind a XAML `Window` (`microsoft.ui.xaml.window.h`).
pub(crate) fn window_handle(window: &Window) -> windows_core::Result<HWND> {
    let native: IWindowNative = window.cast()?;
    let mut hwnd: HWND = std::ptr::null_mut();
    unsafe { native.window_handle(&mut hwnd).ok()? };
    Ok(hwnd)
}

/// Gives the window the exe's icon, which WinUI leaves unset: the taskbar button reads the exe,
/// but its hover thumbnail and Alt+Tab draw the window's own icons.
pub(crate) fn set_window_icon(hwnd: HWND) {
    /// `winresource`'s ID for the icon `vsesvit`'s build script embeds, as `MAKEINTRESOURCE`.
    const APP_ICON: PCWSTR = PCWSTR(std::ptr::without_provenance(1));
    unsafe {
        let exe = GetModuleHandleW(PCWSTR::null());
        let load = |cx, cy| {
            // Shared icons at system sizes are cached by the OS and never freed.
            LoadImageW(
                exe,
                APP_ICON,
                IMAGE_ICON as u32,
                cx,
                cy,
                (LR_DEFAULTSIZE | LR_SHARED) as u32,
            )
        };
        let small = load(GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
        let big = load(0, 0);
        if small.is_null() || big.is_null() {
            log::warn!(
                "loading the app icon: {}",
                windows_core::Error::from_thread()
            );
        }
        SendMessageW(hwnd, WM_SETICON as u32, ICON_SMALL as usize, small as isize);
        SendMessageW(hwnd, WM_SETICON as u32, ICON_BIG as usize, big as isize);
    }
}

/// Puts `text` on the clipboard, where it stays after Vsesvit exits.
pub(crate) fn copy_text(text: &str) -> windows_core::Result<()> {
    let package = DataPackage::new()?;
    package.SetText(text)?;
    Clipboard::SetContent(&package)?;
    Clipboard::Flush()
}
