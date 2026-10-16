//! CEF sub-process executable (renderer, GPU, utility, zygote). It links only the
//! CEF side of the crate: no GPUI, no X11 client code, no threads before
//! `execute_process`.
use vsesvit_spike::cefglue::app::ChildApp;

fn main() -> std::process::ExitCode {
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();
    let mut app = ChildApp::new();
    let code = cef::execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());
    std::process::ExitCode::from(code.clamp(0, 255) as u8)
}
