//! Owned by W0.1.
//! The platform probe: a GPUI window with a foreign child standing in for CEF (design section 7.1). One
//! probe for both OSes; W1.1 fills `x11.rs` and W1.2 `windows.rs`. Runs only under
//! `cargo xtask probe native_probe`, inside a test environment.

use std::process::ExitCode;

#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
mod x11;

fn main() -> ExitCode {
    let env = vsesvit_testenv::require();
    #[cfg(target_os = "linux")]
    return x11::run(env);
    #[cfg(windows)]
    return windows::run(env);
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        drop(env);
        ExitCode::FAILURE
    }
}
