//! Owned by W1.3.
//! The CEF smoke run (design section 8.3, W1.3). Runs only under `cargo xtask probe smoke`. Not built yet.

use std::process::ExitCode;

use vsesvit_cef::Sandbox;

fn main() -> ExitCode {
    if let Some(code) = vsesvit_cef::run_subprocess(&Sandbox::none()) { return code; }
    let _env = vsesvit_testenv::require();
    eprintln!("smoke: not built yet");
    ExitCode::FAILURE
}
