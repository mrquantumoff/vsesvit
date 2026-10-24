//! Owned by W2.1.
//! Overlays over a foreign child: menus, a passive list, outside-press dismissal (design section 8.5).
//! Runs only under `cargo xtask probe overlay_demo`. Not built yet.

use std::process::ExitCode;

fn main() -> ExitCode {
    let _env = vsesvit_testenv::require();
    eprintln!("overlay_demo: not built yet");
    ExitCode::FAILURE
}
