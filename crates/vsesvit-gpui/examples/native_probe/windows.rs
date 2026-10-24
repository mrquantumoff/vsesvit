//! Owned by W1.2.

use std::process::ExitCode;

/// The Windows checks of the platform probe. Not built yet.
pub fn run(_env: vsesvit_testenv::TestEnv) -> ExitCode {
    eprintln!("native_probe: the Windows probe is not built yet");
    ExitCode::FAILURE
}
