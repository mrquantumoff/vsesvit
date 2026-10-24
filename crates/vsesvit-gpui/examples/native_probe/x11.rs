//! Owned by W1.1.

use std::process::ExitCode;

/// The X11 checks of the platform probe. Not built yet.
pub fn run(_env: vsesvit_testenv::TestEnv) -> ExitCode {
    eprintln!("native_probe: the X11 probe is not built yet");
    ExitCode::FAILURE
}
