//! Plain executable: `entry::run` without a sandbox.
//! No console: a console process attached to the foreground console may take the foreground.
#![windows_subsystem = "windows"]

fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(winspike::entry::run(std::ptr::null_mut()).clamp(0, 255) as u8)
}
