//! Owned by W1.4.

use std::process::ExitCode;

pub mod cli;
pub mod headless;
pub mod instance;
pub mod logging;
pub mod profile_start;
pub mod webkit_import;

/// Boot steps 1 to 11 (design section 3.1). Not built yet: refuses to start.
pub fn run(_factory: &dyn vsesvit_engine::EngineFactory) -> ExitCode {
    eprintln!("vsesvit: the GPUI shell is not built yet");
    ExitCode::FAILURE
}
