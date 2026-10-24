//! Owned by W1.3.
//! Windows: the DLL CEF's `bootstrap.exe` (renamed `vsesvit.exe`) loads from its own directory as
//! `vsesvit.dll`, calling `RunWinMain` with the sandbox information (design section 1.1, SW section
//! 3.2). Every process of a sandboxed package enters here: boot step 0 returns in a sub-process, step
//! 0a refuses a bootstrap of another CEF version, and the browser process runs the GPUI shell. Empty
//! off Windows.
#![cfg(windows)]

use std::ffi::c_void;

/// The entry point `bootstrap.exe` calls (`include/cef_sandbox_win.h`).
///
/// # Safety
///
/// Called by the bootstrap only, with `version_info` null or pointing at its `cef_version_info_t`, which
/// begins with the fields of [`vsesvit_cef::VersionInfoPrefix`] and lives for the process.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn RunWinMain(
    _instance: *mut c_void,
    _command_line: *mut u16,
    _show: i32,
    sandbox_info: *mut c_void,
    version_info: *const vsesvit_cef::VersionInfoPrefix,
) -> i32 {
    let sandbox = vsesvit_cef::Sandbox::from_bootstrap(sandbox_info);
    if let Some(code) = vsesvit_cef::subprocess_exit_code(&sandbox) {
        return code;
    }
    // SAFETY: the bootstrap's contract, above.
    let version = unsafe { version_info.as_ref() };
    if let Err(why) = vsesvit_cef::check_bootstrap_version(version) {
        eprintln!("vsesvit: {why}");
        return 1;
    }
    // `ExitCode` has no stable integer accessor. The codes that matter beyond success and failure
    // (70 for a stuck shutdown) leave through `std::process::exit` before `run` returns.
    let code = vsesvit_gpui::run(&vsesvit_cef::CefEngineFactory::new(sandbox));
    if code == std::process::ExitCode::SUCCESS { 0 } else { 1 }
}
