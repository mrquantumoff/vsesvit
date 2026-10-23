//! Vsesvit Windows shell spike: GPUI (DirectComposition) chrome + Chrome-style CEF child HWNDs.
pub mod cefglue;
pub mod ctl;
pub mod entry;
pub mod pump;
pub mod shell;
pub mod wgc;
pub mod win;

use std::io::Write as _;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static LOG: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();

/// Timestamped line to stderr and, when `SPIKE_LOG` is set, to that file (flushed per line,
/// so scripts can tail it while sub-processes still hold stderr).
pub fn log(msg: &str) {
    let t = START.get_or_init(Instant::now).elapsed().as_secs_f64();
    let line = format!("[{t:9.3}] {msg}\n");
    let _ = std::io::stderr().write_all(line.as_bytes());
    let file = LOG.get_or_init(|| Mutex::new(std::env::var("SPIKE_LOG").ok().and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok())));
    if let Ok(mut f) = file.lock() {
        if let Some(f) = f.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

pub fn init_log_clock() {
    let _ = START.get_or_init(Instant::now);
}

/// Entry point CEF's `bootstrap.exe` (renamed to `winspike.exe`) looks up in
/// `winspike.dll` (include/cef_sandbox_win.h). Every process, browser and
/// sub-processes alike, comes through here with bootstrap's sandbox info.
#[unsafe(no_mangle)]
pub extern "system" fn RunWinMain(_instance: *mut core::ffi::c_void, _cmd_line: *const u16, _show: i32, sandbox_info: *mut core::ffi::c_void, _version_info: *mut core::ffi::c_void) -> i32 {
    entry::run(sandbox_info as *mut u8)
}
