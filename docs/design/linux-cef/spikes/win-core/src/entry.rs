//! Browser and sub-process entry, shared by `main.rs` (plain executable) and the
//! `RunWinMain` export (`lib.rs`) that CEF's `bootstrap.exe` calls for the sandbox.
//! CEF first (api_hash, execute_process, initialize), then GPUI.
use crate::cefglue::{PumpMode, Shared, app::BrowserApp, app::ChildApp};
use crate::{log, pump, shell, win};
use futures::channel::mpsc::unbounded;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// `sandbox_info`: null for the plain executable, bootstrap's pointer under the sandbox.
pub fn run(sandbox_info: *mut u8) -> i32 {
    // 0. DPI awareness for the whole process, before CEF touches it (execute_process already
    //    enables per-monitor v2) and before GPUI creates windows.
    let dpi_mode = std::env::var("SPIKE_DPI").unwrap_or_else(|_| "pmv2".into());
    let dpi_ok = win::set_dpi_awareness(&dpi_mode);
    let dpi_err = std::io::Error::last_os_error();

    // 1. Pin the CEF API version before any other CEF call (every process).
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new(); // outlives initialize/shutdown (G9)

    // 2. Same-executable mode: sub-processes come back here with --type=... and stop.
    // Under bootstrap.exe (sandbox) every process is the same executable.
    let sandboxed = !sandbox_info.is_null();
    let same_exe = sandboxed || std::env::var("SPIKE_SAME_EXE").is_ok_and(|v| v == "1");
    {
        let mut child_app = ChildApp::new();
        let code = cef::execute_process(Some(args.as_main_args()), Some(&mut child_app), sandbox_info);
        if code >= 0 {
            return code;
        }
    }
    crate::init_log_clock();

    let (desk, input_desk, safe) = win::init_desktop_guard();
    log(&format!("main: pid={} same_exe={same_exe} dpi={dpi_mode} set={dpi_ok} ({dpi_err}) awareness={} desktop={desk} input_desktop={input_desk} may_activate={safe} exe={}", std::process::id(), win::dpi_awareness(), std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()));

    // 4. Browser process: initialize CEF before GPUI spawns its dispatcher threads.
    let root = std::env::var("SPIKE_ROOT").unwrap_or_else(|_| format!("{}\\vsesvit-win-spike-{}", std::env::temp_dir().display(), std::process::id()));
    let downloads = format!("{root}\\downloads");
    std::fs::create_dir_all(&downloads).ok();
    let (ev_tx, ev_rx) = unbounded();
    let (pump_tx, pump_rx) = unbounded::<i64>();
    let shared = Shared::new(ev_tx, pump_tx, downloads);
    // Chromium focuses a new browser's HWND on its own (FocusHandler::on_set_focus, source
    // navigation); on the user's desktop that activates our toplevel, so refuse it there.
    shared.page_focus_allowed.store(safe, std::sync::atomic::Ordering::SeqCst);
    let mut app = BrowserApp::create(shared.clone());
    let exe = std::env::current_exe().expect("current_exe");
    let helper = exe.parent().unwrap().join("vsesvit-win-spike-helper.exe");
    let mt = shared.pump_mode == PumpMode::MultiThreaded;
    let settings = cef::Settings {
        no_sandbox: (!sandboxed) as i32,
        external_message_pump: (!mt) as i32,
        multi_threaded_message_loop: mt as i32,
        browser_subprocess_path: if same_exe { cef::CefString::from("") } else { cef::CefString::from(helper.to_str().unwrap()) },
        root_cache_path: cef::CefString::from(root.as_str()),
        cache_path: cef::CefString::from(format!("{root}\\Default").as_str()),
        log_file: cef::CefString::from(format!("{root}\\cef.log").as_str()),
        log_severity: cef::LogSeverity::WARNING,
        ..Default::default()
    };
    let t0 = std::time::Instant::now();
    if cef::initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), sandbox_info) != 1 {
        log(&format!("main: cef initialize failed, exit code {}", cef::get_exit_code()));
        return 1;
    }
    log(&format!("main: cef initialized in {:?} pump={:?} sandbox={} (root {root})", t0.elapsed(), shared.pump_mode, settings.no_sandbox == 0));

    // 5. GPUI.
    let stats = Arc::new(pump::PumpStats::default());
    let stop = Arc::new(AtomicBool::new(false));
    {
        let shared = shared.clone();
        let stats = stats.clone();
        let stop = stop.clone();
        let ctl = crate::ctl::spawn();
        gpui_platform::application().run(move |cx| {
            gpui_component::init(cx);
            if !mt {
                pump::start(cx, pump_rx, stats.clone(), stop.clone()).detach();
            }
            shell::open(cx, shell::ShellInit { shared, events: ev_rx, ctl, stats, stop_pump: stop });
        });
    }
    log("main: GPUI run() returned");

    // 6. All browsers are closed (on_before_close seen). Let CEF finish pending work,
    //    drop our last references, then shut down on the main thread.
    if !mt {
        pump::pump_blocking(Duration::from_millis(200), || false);
    } else if std::env::var("SPIKE_MT_DRAIN").map(|v| v != "0").unwrap_or(true) {
        // CEF's UI thread may be inside a cross-thread SendMessage to our windows (its
        // HWNDs are children of ours); keep answering until it settles.
        let n = win::drain_messages(Duration::from_millis(200));
        log(&format!("main: drained {n} Win32 messages before shutdown"));
    }
    drop(app);
    drop(shared);
    log("main: calling cef::shutdown");
    let t1 = std::time::Instant::now();
    cef::shutdown();
    log(&format!("main: cef shutdown in {:?}; exiting", t1.elapsed()));
    0
}
