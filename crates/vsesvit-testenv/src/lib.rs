//! Owned by W0.5.
//! The test-environment guard (design section 7.3). Every self-test, example and window-opening test
//! binary calls `require()` before it opens a window; it exits 3 unless the process runs on the private
//! display or desktop that `xtask`'s launcher created (`cargo xtask probe`, `cargo xtask selftest`).
//! There is no override. Input for tests goes through `PrivateInput`, which only a passed guard builds.

// The writer's half (`Token::from_bytes`, `EnvFile::to_json`) is used by xtask, which includes this
// file by path.
#[allow(dead_code)]
mod envfile;
mod guard;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

use std::path::Path;
use std::process;

use envfile::{EnvFile, Place};
use guard::{FileFacts, Machine, Os};

#[cfg(target_os = "linux")]
pub use linux::PrivateInput;
#[cfg(windows)]
pub use windows::PrivateInput;

/// A validated test environment. Only `require` builds one.
pub struct TestEnv {
    file: EnvFile,
}

impl TestEnv {
    /// For logs: where the run is, by the names the guard checked.
    pub fn describe(&self) -> String {
        match &self.file.place {
            Place::Xvfb { display, xvfb_pid, .. } => format!("Xvfb {display} (pid {xvfb_pid})"),
            Place::Desktop { name } => format!("desktop {name}"),
        }
    }

    /// Input into this environment only: XTEST into the private X server, or messages posted to
    /// this process's own windows.
    #[cfg(target_os = "linux")]
    pub fn input(&self) -> Result<PrivateInput, String> {
        match &self.file.place {
            Place::Xvfb { display, .. } => PrivateInput::open(display),
            Place::Desktop { .. } => Err("a desktop environment on Linux".into()),
        }
    }

    /// Input into this environment only: XTEST into the private X server, or messages posted to
    /// this process's own windows.
    #[cfg(windows)]
    pub fn input(&self) -> Result<PrivateInput, String> {
        Ok(PrivateInput::new())
    }
}

/// Exits 3, with the reason on stderr, unless this process runs in a launcher-made test
/// environment. Call it before anything that can open a window.
pub fn require() -> TestEnv {
    match guard::check(&Real, Os::CURRENT) {
        Ok(file) => TestEnv { file },
        Err(reason) => {
            eprintln!("vsesvit-testenv: refusing to run outside the launcher's test environment: {reason}");
            process::exit(3)
        }
    }
}

/// The machine this process runs on.
struct Real;

impl Machine for Real {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
    }

    fn env_file(&self, path: &Path) -> Result<FileFacts, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
            // `/proc/self` belongs to the process's effective user.
            let me = std::fs::metadata("/proc/self").map_err(|e| format!("/proc/self: {e}"))?.uid();
            Ok(FileFacts { text, owned_by_me: meta.uid() == me, mode: Some(meta.mode()) })
        }
        #[cfg(not(unix))]
        Ok(FileFacts { text, owned_by_me: true, mode: None })
    }

    #[cfg(target_os = "linux")]
    fn lock_pid(&self, display: &str) -> Result<u32, String> {
        linux::lock_pid(display)
    }

    #[cfg(target_os = "linux")]
    fn exe_name(&self, pid: u32) -> Result<String, String> {
        linux::exe_name(pid)
    }

    #[cfg(target_os = "linux")]
    fn root_token(&self, display: &str) -> Result<Option<String>, String> {
        linux::root_token(display)
    }

    #[cfg(not(target_os = "linux"))]
    fn lock_pid(&self, _: &str) -> Result<u32, String> {
        Err("no X lock files on this OS".into())
    }

    #[cfg(not(target_os = "linux"))]
    fn exe_name(&self, _: u32) -> Result<String, String> {
        Err("no /proc on this OS".into())
    }

    #[cfg(not(target_os = "linux"))]
    fn root_token(&self, _: &str) -> Result<Option<String>, String> {
        Err("no X server on this OS".into())
    }

    #[cfg(windows)]
    fn thread_desktop(&self) -> Result<String, String> {
        windows::thread_desktop()
    }

    #[cfg(windows)]
    fn input_desktop(&self) -> Result<String, String> {
        windows::input_desktop()
    }

    #[cfg(not(windows))]
    fn thread_desktop(&self) -> Result<String, String> {
        Err("no desktops on this OS".into())
    }

    #[cfg(not(windows))]
    fn input_desktop(&self) -> Result<String, String> {
        Err("no desktops on this OS".into())
    }
}
