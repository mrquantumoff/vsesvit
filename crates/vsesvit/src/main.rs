#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::process::ExitCode;

#[cfg(all(feature = "gpui", any(target_os = "linux", windows)))]
fn main() -> ExitCode {
    // A Windows release enters through vsesvit-sandbox's RunWinMain instead.
    let sandbox = vsesvit_cef::Sandbox::none();
    if let Some(code) = vsesvit_cef::run_subprocess(&sandbox) {
        return code;
    }
    vsesvit_gpui::run(&vsesvit_cef::CefEngineFactory::new(sandbox))
}

#[cfg(all(not(feature = "gpui"), target_os = "linux", feature = "gtk"))]
fn main() -> ExitCode {
    vsesvit_gtk::run()
}

#[cfg(all(not(feature = "gpui"), windows, feature = "winui"))]
fn main() -> ExitCode {
    vsesvit_winui::run()
}

#[cfg(all(not(feature = "gpui"), target_os = "linux", not(feature = "gtk")))]
compile_error!("enable the `gtk` or the `gpui` feature");
#[cfg(all(not(feature = "gpui"), windows, not(feature = "winui")))]
compile_error!("enable the `winui` or the `gpui` feature");

#[cfg(not(any(target_os = "linux", windows)))]
fn main() -> ExitCode {
    eprintln!("vsesvit supports Windows and Linux only");
    ExitCode::FAILURE
}
