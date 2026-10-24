//! Owned by W1.3.
//! The CEF glue: implements the `vsesvit_engine` contract over CEF 154 (design sections 1.2 and 3). It
//! never names a GPUI type. Every module is declared here, so the units that fill the handler files
//! never edit this one.
//!
//! Every entry point runs [`run_subprocess`] first (boot step 0): in a CEF sub-process it returns the
//! exit code. The browser process then builds the engine with [`CefEngineFactory`] (boot step 9).

use std::process::ExitCode;
use std::rc::Rc;

use futures::channel::mpsc::unbounded;
use vsesvit_engine::{EngineConfig, EngineInitError, EngineParts};

pub mod app;
pub mod client;
pub mod contexts;
pub mod devtools;
pub mod driver;
pub mod engine;
pub mod events;
pub mod ext;
pub mod handlers {
    pub mod auth;
    pub mod command;
    pub mod context_menu;
    pub mod cookie_access;
    pub mod dialog;
    pub mod display;
    pub mod download;
    pub mod favicon;
    pub mod find;
    pub mod focus;
    pub mod jsdialog;
    pub mod keyboard;
    pub mod life_span;
    pub mod load;
    pub mod permission;
    pub mod request;
    pub mod resource_request;
}
pub mod hidden;
pub mod idc;
pub mod lifecycle;
pub mod native;
pub mod prefs;
pub mod prompts;
pub mod registry;
pub mod schemes;
pub mod settings;
pub mod site_data;
pub mod strings;
pub mod teardown;

/// The Windows sandbox information `RunWinMain` receives (design section 1.1); none in a dev build and
/// off Windows.
pub struct Sandbox {
    info: *mut std::ffi::c_void,
}

impl Sandbox {
    /// No sandbox information: Linux, and Windows `cargo run`.
    pub fn none() -> Sandbox {
        Sandbox { info: std::ptr::null_mut() }
    }

    /// The sandbox information CEF's `bootstrap.exe` passed to `RunWinMain`.
    #[cfg(windows)]
    pub fn from_bootstrap(info: *mut std::ffi::c_void) -> Sandbox {
        Sandbox { info }
    }

    /// Whether the process runs under the bootstrap's sandbox.
    pub fn sandboxed(&self) -> bool {
        !self.info.is_null()
    }

    fn info(&self) -> *mut u8 {
        self.info.cast()
    }
}

/// The leading fields of CEF's `cef_version_info_t` (`include/cef_version_info.h`), which every version
/// of the struct starts with; `RunWinMain` receives the bootstrap's.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VersionInfoPrefix {
    /// Size of the whole structure.
    pub size: usize,
    /// CEF major.
    pub cef_version_major: i32,
    /// CEF minor.
    pub cef_version_minor: i32,
    /// CEF patch.
    pub cef_version_patch: i32,
    /// CEF commit number.
    pub cef_commit_number: i32,
    /// Chromium major.
    pub chrome_version_major: i32,
    /// Chromium minor.
    pub chrome_version_minor: i32,
    /// Chromium build.
    pub chrome_version_build: i32,
    /// Chromium patch.
    pub chrome_version_patch: i32,
}

/// Boot step 0a (Windows release): the bootstrap's CEF must be the one this build was made for.
pub fn check_bootstrap_version(info: Option<&VersionInfoPrefix>) -> Result<(), String> {
    use cef::sys;
    let Some(info) = info else { return Err("the bootstrap passed no version information".into()) };
    if info.size < std::mem::size_of::<VersionInfoPrefix>() {
        return Err(format!("the bootstrap's version information is too short ({} bytes)", info.size));
    }
    let ours = (
        sys::CEF_VERSION_MAJOR,
        sys::CEF_VERSION_MINOR,
        sys::CEF_VERSION_PATCH,
        sys::CHROME_VERSION_MAJOR,
        sys::CHROME_VERSION_MINOR,
        sys::CHROME_VERSION_BUILD,
        sys::CHROME_VERSION_PATCH,
    );
    let theirs = (
        info.cef_version_major,
        info.cef_version_minor,
        info.cef_version_patch,
        info.chrome_version_major,
        info.chrome_version_minor,
        info.chrome_version_build,
        info.chrome_version_patch,
    );
    if ours != theirs {
        return Err(format!(
            "the bootstrap runs CEF {}.{}.{} (Chromium {}.{}.{}.{}), this build needs {}.{}.{} (Chromium {}.{}.{}.{})",
            theirs.0, theirs.1, theirs.2, theirs.3, theirs.4, theirs.5, theirs.6, ours.0, ours.1, ours.2, ours.3, ours.4,
            ours.5, ours.6
        ));
    }
    Ok(())
}

/// Boot step 0: `api_hash`, then `execute_process`; `Some(code)` in a CEF sub-process.
pub fn run_subprocess(sandbox: &Sandbox) -> Option<ExitCode> {
    subprocess_exit_code(sandbox).map(|code| ExitCode::from(code.clamp(0, 255) as u8))
}

/// [`run_subprocess`] with CEF's own exit code, for `RunWinMain`, which returns an `int`.
pub fn subprocess_exit_code(sandbox: &Sandbox) -> Option<i32> {
    vsesvit_cef_child::execute_subprocess(sandbox.info())
}

/// How many threads this process runs (Linux; `None` elsewhere). The Linux sandbox wants one while CEF
/// initializes (design section 3.1).
pub fn thread_count() -> Option<usize> {
    #[cfg(target_os = "linux")]
    return std::fs::read_dir("/proc/self/task").ok().map(|d| d.count());
    #[cfg(not(target_os = "linux"))]
    return None;
}

/// Builds the CEF engine for the browser process.
pub struct CefEngineFactory {
    sandbox: Sandbox,
}

impl CefEngineFactory {
    /// The factory; `sandbox` is `Sandbox::none()` except under `RunWinMain`.
    pub fn new(sandbox: Sandbox) -> CefEngineFactory {
        CefEngineFactory { sandbox }
    }
}

impl vsesvit_engine::EngineFactory for CefEngineFactory {
    fn chromium_version(&self) -> &'static str {
        "154.0.8037.98"
    }

    /// Boot step 9: `cef::initialize` with design sections 3.2 and 3.3. Installs the registry on this
    /// thread, which becomes the engine's main thread.
    fn initialize(&self, config: EngineConfig) -> Result<EngineParts, EngineInitError> {
        if let Some(threads) = thread_count().filter(|n| *n > 1) {
            log::error!("vsesvit-cef: {threads} threads run before cef::initialize; the sandbox wants one (design section 3.1)");
        }
        let paths = settings::Paths::of(&config);
        if let Err(e) = std::fs::create_dir_all(&paths.root_cache_path) {
            return Err(EngineInitError::Failed(format!("{}: {e}", paths.root_cache_path.display())));
        }
        let exe = std::env::current_exe().map_err(|e| EngineInitError::Failed(format!("the executable's path: {e}")))?;
        let image = settings::subprocess_image(settings::Os::CURRENT, &exe, |p| p.is_file(), !cfg!(debug_assertions))
            .map_err(EngineInitError::Failed)?;
        let (sink, events) = events::Sink::new();
        let (wake, wakes) = unbounded();
        let mut app = app::browser_app(&config, wake.clone(), sink.clone());
        let cef_settings = settings::cef_settings(&config, &paths, &image, self.sandbox.sandboxed());
        let backend = Rc::new(client::CefBackend::new(sink.clone(), config.background_color));
        registry::install(registry::Registry::new(backend, sink));
        let args = vsesvit_cef_child::OwnedArgs::get();
        let started = cef::initialize(Some(args.main_args()), Some(&cef_settings), Some(&mut app), self.sandbox.info());
        drop(app);
        if started != 1 {
            registry::take();
            let code = cef::get_exit_code();
            if code == 0 {
                return Err(EngineInitError::EarlyExit(ExitCode::SUCCESS));
            }
            return Err(EngineInitError::Failed(format!("CEF did not start (exit code {code}); see {}", paths.log_file.display())));
        }
        Ok(EngineParts {
            engine: engine::CefEngine::new(),
            driver: Box::new(driver::CefDriver::new(wake, wakes)),
            events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ours() -> VersionInfoPrefix {
        use cef::sys;
        VersionInfoPrefix {
            size: 128,
            cef_version_major: sys::CEF_VERSION_MAJOR,
            cef_version_minor: sys::CEF_VERSION_MINOR,
            cef_version_patch: sys::CEF_VERSION_PATCH,
            cef_commit_number: 0,
            chrome_version_major: sys::CHROME_VERSION_MAJOR,
            chrome_version_minor: sys::CHROME_VERSION_MINOR,
            chrome_version_build: sys::CHROME_VERSION_BUILD,
            chrome_version_patch: sys::CHROME_VERSION_PATCH,
        }
    }

    #[test]
    fn the_bootstrap_must_run_our_cef() {
        assert_eq!(check_bootstrap_version(Some(&ours())), Ok(()));
        assert!(check_bootstrap_version(None).is_err());
        let old = VersionInfoPrefix { cef_version_patch: ours().cef_version_patch - 1, ..ours() };
        assert!(check_bootstrap_version(Some(&old)).unwrap_err().contains("this build needs 154.0.34"));
        let short = VersionInfoPrefix { size: 8, ..ours() };
        assert!(check_bootstrap_version(Some(&short)).is_err());
    }

    #[test]
    fn no_sandbox_by_default() {
        assert!(!Sandbox::none().sandboxed());
    }
}
