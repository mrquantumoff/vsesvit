//! Scratch crate: compile-checks the snippets in cef-rs-api.md against cef 154.5.0.
#![allow(dead_code, clippy::too_many_arguments)]

mod app;
mod client;
mod misc;
mod renderer;
mod pump;

use cef::{args::Args, *};
use std::sync::mpsc;

/// Messages from CEF threads to the (GPUI) main thread.
pub enum UiMsg {
    /// CefBrowserProcessHandler::OnScheduleMessagePumpWork(delay_ms).
    PumpWork(i64),
    ContextReady,
    Title { browser_id: i32, title: String },
    Address { browser_id: i32, url: String },
}

fn main() -> std::process::ExitCode {
    // 1. Pin the C API version before *any* other CEF call (crate examples do this).
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    let cmd = args.as_cmd_line().expect("command line");
    let is_browser = cmd.has_switch(Some(&CefString::from("type"))) == 0;

    let (tx, rx) = mpsc::channel::<UiMsg>();
    let mut app = app::VsesvitApp::new(tx.clone());

    // 2. Sub-processes (renderer, gpu, utility, zygote) run and exit here.
    let code = execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());
    if code >= 0 {
        return std::process::ExitCode::from(code as u8);
    }
    assert!(is_browser);

    // 3. Browser process.
    let root = "/home/user/.local/share/vsesvit/cef";
    let settings = Settings {
        no_sandbox: 0,
        external_message_pump: 1,
        multi_threaded_message_loop: 0,
        windowless_rendering_enabled: 1,
        root_cache_path: CefString::from(root),
        cache_path: CefString::from(format!("{root}/Default").as_str()),
        resources_dir_path: CefString::from("/opt/vsesvit/cef"),
        locales_dir_path: CefString::from("/opt/vsesvit/cef/locales"),
        log_file: CefString::from("/tmp/vsesvit-cef.log"),
        log_severity: LogSeverity::WARNING,
        persist_session_cookies: 1,
        ..Default::default()
    };
    if initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()) != 1 {
        eprintln!("cef initialize failed: exit code {}", get_exit_code());
        return std::process::ExitCode::FAILURE;
    }

    // 4. External pump: the UI loop calls do_message_loop_work() when asked.
    //    (A real GPUI app would forward these messages into its executor.)
    for msg in rx.iter() {
        match msg {
            UiMsg::PumpWork(delay_ms) => {
                if delay_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms as u64));
                }
                do_message_loop_work();
            }
            UiMsg::ContextReady => {
                client::create_osr_browser(tx.clone(), None, "https://example.org");
            }
            _ => {}
        }
    }

    shutdown();
    std::process::ExitCode::SUCCESS
}
