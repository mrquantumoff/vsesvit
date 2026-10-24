//! Entry: api_hash, execute_process (children return), then the browser process's private
//! display guard before anything else, then CEF and GPUI on X11.
use extwin::cefglue::{Shared, app::BrowserApp, app::ChildApp};
use extwin::{log, platform, pump, shell};
use futures::channel::mpsc::unbounded;
use std::os::fd::FromRawFd as _;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Chromium's ends of `--remote-debugging-pipe` on fds 3 (read) and 4 (write); ours CLOEXEC.
fn setup_cdp_pipe() -> bool {
    let cloexec_dup = |fd: i32| unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 100) };
    let mut a = [0i32; 2];
    let mut b = [0i32; 2];
    unsafe {
        if libc::pipe2(a.as_mut_ptr(), libc::O_CLOEXEC) != 0 || libc::pipe2(b.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            return false;
        }
        let (ar, aw, br, bw) = (cloexec_dup(a[0]), cloexec_dup(a[1]), cloexec_dup(b[0]), cloexec_dup(b[1]));
        for fd in [a[0], a[1], b[0], b[1]] {
            libc::close(fd);
        }
        if libc::dup2(ar, 3) != 3 || libc::dup2(bw, 4) != 4 {
            return false;
        }
        libc::close(ar);
        libc::close(bw);
        extwin::cdp::start(std::fs::File::from_raw_fd(aw), std::fs::File::from_raw_fd(br));
    }
    true
}

fn main() -> ExitCode {
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();
    let same_exe = std::env::var("SPIKE_SAME_EXE").is_ok_and(|v| v == "1");
    {
        let mut child_app = ChildApp::new();
        let code = cef::execute_process(Some(args.as_main_args()), Some(&mut child_app), std::ptr::null_mut());
        if code >= 0 {
            return ExitCode::from(code as u8);
        }
    }
    extwin::init_log_clock();
    match platform::guard() {
        Ok(d) => log(&format!("main: guard ok ({d})")),
        Err(e) => {
            log(&format!("GUARD: refusing to start: {e}"));
            return ExitCode::from(3);
        }
    }
    if std::env::var("SPIKE_CDP_PIPE").is_ok_and(|v| v == "1") {
        log(&format!("main: remote-debugging-pipe fds 3/4: {}", setup_cdp_pipe()));
    }
    log(&format!("main: report server {:?}", extwin::report::spawn()));
    let scale = std::env::var("SPIKE_SCALE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
    unsafe {
        std::env::set_var("GPUI_X11_SCALE_FACTOR", scale.to_string());
        std::env::set_var("SPIKE_SCALE", scale.to_string());
    }
    let root = std::env::var("SPIKE_ROOT").expect("SPIKE_ROOT");
    std::fs::create_dir_all(&root).ok();
    let cache_name = std::env::var("SPIKE_CACHE_NAME").unwrap_or_else(|_| "Default".into());
    let downloads = format!("{root}/downloads");
    std::fs::create_dir_all(&downloads).ok();
    let (ev_tx, ev_rx) = unbounded();
    let (pump_tx, pump_rx) = unbounded::<i64>();
    let shared = Shared::new(ev_tx, pump_tx, downloads);
    let mut app = BrowserApp::create(shared.clone());
    let exe = std::env::current_exe().expect("current_exe");
    let helper = exe.parent().unwrap().join("vsesvit-extwin-helper");
    let policy = std::env::var("SPIKE_POLICY_ID").unwrap_or_default();
    let settings = cef::Settings {
        no_sandbox: std::env::var("SPIKE_NO_SANDBOX").is_ok() as i32,
        external_message_pump: 1,
        multi_threaded_message_loop: 0,
        browser_subprocess_path: if same_exe { cef::CefString::from("") } else { cef::CefString::from(helper.to_str().unwrap()) },
        root_cache_path: cef::CefString::from(root.as_str()),
        cache_path: cef::CefString::from(format!("{root}/{cache_name}").as_str()),
        log_file: cef::CefString::from(format!("{root}/cef.log").as_str()),
        log_severity: if std::env::var("SPIKE_CEF_VERBOSE").is_ok() { cef::LogSeverity::INFO } else { cef::LogSeverity::WARNING },
        chrome_policy_id: cef::CefString::from(policy.as_str()),
        ..Default::default()
    };
    let t0 = std::time::Instant::now();
    if cef::initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()) != 1 {
        log(&format!("main: cef initialize failed, exit code {}", cef::get_exit_code()));
        return ExitCode::FAILURE;
    }
    log(&format!("main: cef initialized in {:?} root={root} cache={cache_name} policy_id={policy:?}", t0.elapsed()));
    extwin::rootwatch::spawn();
    let stats = Arc::new(pump::PumpStats::default());
    let stop = Arc::new(AtomicBool::new(false));
    {
        let shared = shared.clone();
        let stats = stats.clone();
        let stop = stop.clone();
        let ctl = extwin::ctl::spawn();
        gpui_platform::linux(gpui::WindowingModes::X11).run(move |cx| {
            gpui_component::init(cx);
            pump::start(cx, pump_rx, stats.clone(), stop.clone()).detach();
            shell::open(cx, shell::ShellInit { shared, events: ev_rx, ctl, stats, stop_pump: stop });
        });
    }
    log("main: GPUI run() returned");
    pump::pump_blocking(Duration::from_millis(200), || false);
    drop(app);
    drop(shared);
    let t1 = std::time::Instant::now();
    cef::shutdown();
    log(&format!("main: cef shutdown in {:?}; exiting", t1.elapsed()));
    ExitCode::SUCCESS
}
