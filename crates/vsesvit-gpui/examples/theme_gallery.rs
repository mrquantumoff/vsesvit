//! Owned by W0.4.
//! Every theme token and icon in light and dark (UA U02). Runs only under
//! `cargo xtask probe theme_gallery`. Not built yet.

use std::process::ExitCode;

fn main() -> ExitCode {
    let _env = vsesvit_testenv::require();
    eprintln!("theme_gallery: not built yet");
    ExitCode::FAILURE
}
