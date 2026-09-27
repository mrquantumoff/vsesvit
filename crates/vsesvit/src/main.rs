#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    return vsesvit_gtk::run();
    #[cfg(windows)]
    return vsesvit_winui::run();
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        eprintln!("vsesvit supports Windows and Linux only");
        ExitCode::FAILURE
    }
}
