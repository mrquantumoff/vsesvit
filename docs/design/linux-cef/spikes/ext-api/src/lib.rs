//! Vsesvit Linux shell spike: GPUI (X11) chrome + Chrome-style CEF children.
pub mod cefglue;
pub mod ctl;
pub mod pump;
pub mod rootwatch;
pub mod shell;
pub mod xwin;

use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub fn log(msg: &str) {
    let t = START.get_or_init(Instant::now).elapsed().as_secs_f64();
    eprintln!("[{t:9.3}] {msg}");
}

pub fn init_log_clock() {
    let _ = START.get_or_init(Instant::now);
}
