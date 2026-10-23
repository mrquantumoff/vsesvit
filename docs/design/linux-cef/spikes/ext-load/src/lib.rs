//! Vsesvit Linux shell spike: GPUI (X11) chrome + Chrome-style CEF children.
pub mod cdp;
pub mod cefglue;
pub mod report;
pub mod ctl;
pub mod pump;
pub mod shell;
pub mod xwin;

use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub fn log(msg: &str) {
    let t = START.get_or_init(Instant::now).elapsed().as_secs_f64();
    eprintln!("[{t:9.3}] {msg}");
}

/// Scratch directory for this run (`SPIKE_ENV`, set by run.sh); the current directory otherwise.
pub fn env_dir() -> std::path::PathBuf {
    std::env::var_os("SPIKE_ENV").map(Into::into).unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

pub fn init_log_clock() {
    let _ = START.get_or_init(Instant::now);
}
