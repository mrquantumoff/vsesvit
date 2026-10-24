//! Browser and sub-process entry, shared by `main.rs` (plain exe) and `RunWinMain` (sandbox
//! bundle): api_hash, execute_process (children return here), then the browser process's
//! private-desktop guard, before anything else touches CEF or creates a window.
use crate::cefglue::{PumpMode, Shared, app::BrowserApp, app::ChildApp};
use crate::{log, platform, pump, shell, win};
use futures::channel::mpsc::unbounded;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub const EXIT_GUARD: i32 = 3;

pub fn run(sandbox_info: *mut u8) -> i32 {
    let dpi_mode = std::env::var("SPIKE_DPI").unwrap_or_else(|_| "pmv2".into());
    let dpi_ok = win::set_dpi_awareness(&dpi_mode);

    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();
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

    // Fail closed: only on the private desktop. Before any CEF initialization or window.
    match platform::guard() {
        Ok(d) => log(&format!("main: guard ok ({d})")),
        Err(e) => {
            log(&format!("GUARD: refusing to start: {e}"));
            return EXIT_GUARD;
        }
    }
    log(&format!("main: pid={} sandboxed={sandboxed} same_exe={same_exe} dpi={dpi_mode} set={dpi_ok} exe={}", std::process::id(), std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()));

    if let Ok(sub) = std::env::var("SPIKE_REG_OVERRIDE") {
        log(&format!("main: registry override: {:?}", platform::reg_override(&sub)));
    }
    if std::env::var("SPIKE_CDP_PIPE").is_ok_and(|v| v == "1") {
        log(&format!("main: cdp io pipes: {:?}", platform::setup_cdp_pipes()));
    }
    let report = crate::report::spawn();
    log(&format!("main: report server {report:?}"));

    let root = std::env::var("SPIKE_ROOT").expect("SPIKE_ROOT");
    let cache_name = std::env::var("SPIKE_CACHE_NAME").unwrap_or_else(|_| "Default".into());
    let downloads = format!("{root}\\downloads");
    std::fs::create_dir_all(&downloads).ok();
    let (ev_tx, ev_rx) = unbounded();
    let (pump_tx, pump_rx) = unbounded::<i64>();
    let shared = Shared::new(ev_tx, pump_tx, downloads);
    let mut app = BrowserApp::create(shared.clone());
    let exe = std::env::current_exe().expect("current_exe");
    let helper = exe.parent().unwrap().join("vsesvit-extwin-helper.exe");
    let mt = shared.pump_mode == PumpMode::MultiThreaded;
    let policy = std::env::var("SPIKE_POLICY_ID").unwrap_or_default();
    let settings = cef::Settings {
        no_sandbox: (!sandboxed) as i32,
        external_message_pump: (!mt) as i32,
        multi_threaded_message_loop: mt as i32,
        browser_subprocess_path: if same_exe { cef::CefString::from("") } else { cef::CefString::from(helper.to_str().unwrap()) },
        root_cache_path: cef::CefString::from(root.as_str()),
        cache_path: cef::CefString::from(format!("{root}\\{cache_name}").as_str()),
        log_file: cef::CefString::from(format!("{root}\\cef.log").as_str()),
        log_severity: if std::env::var("SPIKE_CEF_VERBOSE").is_ok() { cef::LogSeverity::INFO } else { cef::LogSeverity::WARNING },
        chrome_policy_id: cef::CefString::from(policy.as_str()),
        ..Default::default()
    };
    let t0 = std::time::Instant::now();
    if cef::initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), sandbox_info) != 1 {
        log(&format!("main: cef initialize failed, exit code {}", cef::get_exit_code()));
        return 1;
    }
    log(&format!("main: cef initialized in {:?} sandbox={} root={root} cache={cache_name} policy_id={policy:?}", t0.elapsed(), settings.no_sandbox == 0));

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
    if !mt {
        pump::pump_blocking(Duration::from_millis(200), || false);
    } else {
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
