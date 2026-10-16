//! Entry point: CEF first (api_hash, execute_process, initialize), then GPUI on X11.
use futures::channel::mpsc::unbounded;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use vsesvit_spike::cefglue::{Shared, app::BrowserApp, app::ChildApp};
use vsesvit_spike::{log, pump, shell};

/// Scale factor both toolkits must agree on: SPIKE_SCALE, else GPUI_X11_SCALE_FACTOR,
/// else Xft.dpi/96 (GPUI's own rule, minus its RandR fallback that Chromium lacks).
fn decide_scale() -> f32 {
    if let Some(s) = std::env::var("SPIKE_SCALE").ok().and_then(|v| v.parse().ok()) {
        return s;
    }
    if let Some(s) = std::env::var("GPUI_X11_SCALE_FACTOR").ok().and_then(|v| v.parse().ok()) {
        return s;
    }
    let dpi = x11rb::connect(None).ok().and_then(|(conn, _)| {
        let db = x11rb::resource_manager::new_from_default(&conn).ok()?;
        db.get_value::<f32>("Xft.dpi", "Xft.dpi").ok().flatten()
    });
    dpi.map(|d| d / 96.0).unwrap_or(1.0)
}

fn main() -> ExitCode {
    // 1. Pin the CEF API version before any other CEF call (every process).
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new(); // must outlive initialize/shutdown (G9)

    // 2. Same-executable mode: sub-processes come back here with --type=... and stop.
    //    (With the helper, only the browser process ever runs this binary.)
    let same_exe = std::env::var("SPIKE_SAME_EXE").is_ok_and(|v| v == "1");
    {
        let mut child_app = ChildApp::new();
        let code = cef::execute_process(Some(args.as_main_args()), Some(&mut child_app), std::ptr::null_mut());
        if code >= 0 {
            return ExitCode::from(code as u8);
        }
    }
    vsesvit_spike::init_log_clock();

    // 2b. In-process CDP over --remote-debugging-pipe: fds 3/4 must be ours before anything
    //     else opens a file descriptor and before cef::initialize reads them.
    if std::env::var("SPIKE_CDP_PIPE").is_ok_and(|v| v == "1") {
        let ok = vsesvit_spike::cdp::setup_pipe();
        log(&format!("main: remote-debugging-pipe fds 3/4 set up: {ok}"));
    }
    // Report server + fixture site for extensions to talk to.
    let report_port: u16 = std::env::var("SPIKE_REPORT_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(8792);
    vsesvit_spike::report::spawn(report_port);

    // 3. One scale factor for both GPUI and Chromium (set env before any thread exists).
    let scale = decide_scale();
    unsafe {
        std::env::set_var("GPUI_X11_SCALE_FACTOR", scale.to_string());
        std::env::set_var("SPIKE_SCALE", scale.to_string());
    }
    log(&format!("main: scale factor {scale}, same_exe={same_exe}, pid={}", std::process::id()));

    // 4. Browser process: initialize CEF before GPUI spawns its dispatcher threads.
    let (ev_tx, ev_rx) = unbounded();
    let (pump_tx, pump_rx) = unbounded::<i64>();
    let shared = Shared::new(ev_tx, pump_tx);
    let mut app = BrowserApp::create(shared.clone());

    let exe = std::env::current_exe().expect("current_exe");
    let dir = exe.parent().unwrap().to_path_buf();
    let helper = dir.join("vsesvit-spike-helper");
    let root = std::env::var("SPIKE_ROOT").unwrap_or_else(|_| format!("/tmp/vsesvit-scratch/extload-env/cef-root-{}", std::process::id()));
    std::fs::create_dir_all(&root).ok();
    unsafe { std::env::set_var("SPIKE_ROOT", &root) };
    let cache = std::env::var("SPIKE_CACHE").unwrap_or_else(|_| format!("{root}/Default"));
    let rdp_port: i32 = std::env::var("SPIKE_RDP_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let settings = cef::Settings {
        no_sandbox: std::env::var("SPIKE_NO_SANDBOX").is_ok() as i32,
        external_message_pump: 1,
        multi_threaded_message_loop: 0,
        browser_subprocess_path: if same_exe { cef::CefString::from("") } else { cef::CefString::from(helper.to_str().unwrap()) },
        root_cache_path: cef::CefString::from(root.as_str()),
        cache_path: cef::CefString::from(cache.as_str()),
        remote_debugging_port: rdp_port,
        log_file: cef::CefString::from(format!("{root}/cef.log").as_str()),
        log_severity: cef::LogSeverity::WARNING,
        ..Default::default()
    };
    let t0 = std::time::Instant::now();
    if cef::initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()) != 1 {
        log(&format!("main: cef initialize failed, exit code {}", cef::get_exit_code()));
        return ExitCode::FAILURE;
    }
    log(&format!("main: cef initialized in {:?} (root {root}, cache {cache:?}, rdp_port {rdp_port})", t0.elapsed()));

    // 5. GPUI on X11 only (XWayland under a Wayland session).
    let stats = Arc::new(pump::PumpStats::default());
    let stop = Arc::new(AtomicBool::new(false));
    {
        let shared = shared.clone();
        let stats = stats.clone();
        let stop = stop.clone();
        let ctl = vsesvit_spike::ctl::spawn();
        gpui_platform::linux(gpui::WindowingModes::X11).run(move |cx| {
            gpui_component::init(cx);
            pump::start(cx, pump_rx, stats.clone(), stop.clone()).detach();
            shell::open(cx, shell::ShellInit { shared, events: ev_rx, ctl, stats, stop_pump: stop });
        });
    }
    log("main: GPUI run() returned");

    // 6. All browsers are closed (on_before_close seen). Let CEF finish pending work,
    //    drop our last references, then shut down on the main thread.
    pump::pump_blocking(Duration::from_millis(200), || false);
    drop(app);
    drop(shared);
    let t1 = std::time::Instant::now();
    cef::shutdown();
    log(&format!("main: cef shutdown in {:?}; exiting", t1.elapsed()));
    ExitCode::SUCCESS
}
