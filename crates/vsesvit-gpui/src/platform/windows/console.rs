//! Owned by W1.2.
//! Release builds are GUI-subsystem programs with no console, so what a headless command (`--help`,
//! `--version`, `--check-for-updates`, `--update`) prints for a terminal is lost unless the process
//! joins the console of the shell that started it (design section 3.1 step 3). Redirected output keeps
//! its handles either way.

use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

/// Joins the parent's console, if it has one. Call it before printing.
pub fn attach_parent_console() {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}
