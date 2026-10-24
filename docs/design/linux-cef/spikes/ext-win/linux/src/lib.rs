//! WX.0 Linux spike shell: GPUI (X11) chrome + Chrome-style CEF children, with the shared
//! extension machinery in `common/`.
#[path = "../../common/cefglue/mod.rs"]
pub mod cefglue;
#[path = "../../common/cdp.rs"]
pub mod cdp;
#[path = "../../common/ctl.rs"]
pub mod ctl;
#[path = "../../common/report.rs"]
pub mod report;
pub mod platform;
pub mod pump;
pub mod rootwatch;
pub mod shell;
pub mod xwin;

use std::io::Write as _;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static LOG: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();

/// Timestamped line to stderr and, when `SPIKE_LOG` is set, to that file (flushed per line).
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

/// Scratch directory for this run (`SPIKE_ENV`); the current directory otherwise.
pub fn env_dir() -> std::path::PathBuf {
    std::env::var_os("SPIKE_ENV").map(Into::into).unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

pub fn init_log_clock() {
    let _ = START.get_or_init(Instant::now);
}
