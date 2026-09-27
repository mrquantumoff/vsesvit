//! End-to-end check of the runtime under a real WebKitGTK, mirroring the self-test's
//! `content_script`, `dnr_blocked` and `popup` checks (docs/design/self-test.md):
//!
//! 1. serve `tests/fixtures/site/` from a local HTTP server that logs request paths;
//! 2. load `tests/fixtures/extensions/probe/` through `Runtime::load` into a fresh profile;
//! 3. open one tab on `/index.html` and wait for the content script's round trip to the
//!    background (`dataset.vsesvitProbe == "background-replied"`);
//! 4. check the server saw `/allowed.png` and never `/vsesvit-blocked/pixel.png`;
//! 5. open the action popup and wait for its title to become `visits=N`, N >= 1.
//!
//! Prints every observation and exits non-zero on failure. Runs under WSLg; the window
//! is created but not presented unless `--show` is given, so nothing steals focus.
//!
//! ```text
//! bash scripts/wsl.sh run -p vsesvit-webext --example harness [-- --show]
//! ```

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the vsesvit-webext harness runs on Linux only");
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    linux::main()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::cell::{Cell, RefCell};
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use serde_json::Value;
    use vsesvit_core::extensions::{ExtensionId, InstallSource, InstalledExtension};
    use vsesvit_core::{OpenOptions, Profile};
    use vsesvit_webext::{Runtime, TabHost, TabId, TabInfo};
    use webkit::prelude::*;
    use webkit::glib;

    const TIMEOUT: Duration = Duration::from_secs(20);

    pub fn main() -> ExitCode {
        let show = std::env::args().any(|a| a == "--show");
        let _ = log::set_logger(&STDOUT_LOG).map(|()| log::set_max_level(log::LevelFilter::Debug));
        // WSLg has no working DMA-BUF path for WebKit's compositor; harmless elsewhere.
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            // SAFETY: called before any other thread exists.
            unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
        }
        gtk::init().expect("gtk::init");

        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let site = repo.join("tests/fixtures/site");
        let server = FixtureServer::start(site);
        println!("[harness] fixture server on 127.0.0.1:{}", server.port);

        let out_dir = std::env::temp_dir().join(format!("vsesvit-webext-harness-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out_dir);
        let profile = Profile::open(&out_dir.join("profile"), OpenOptions::default()).expect("open profile");
        let paths = profile.paths().clone();
        let profile = Rc::new(RefCell::new(profile));
        let session = webkit::NetworkSession::new(paths.engine_data.to_str(), paths.engine_cache.to_str());

        let host = Rc::new(Host::default());
        let runtime = Runtime::new(profile.clone(), &session, host.clone());

        let installed = install_probe(&profile, &out_dir);
        if let Err(e) = runtime.load(&installed) {
            println!("[harness] FAIL: Runtime::load: {e}");
            return ExitCode::FAILURE;
        }
        println!("[harness] loaded {:?}; pending filters = {}", runtime.loaded().iter().map(|i| i.as_str()).collect::<Vec<_>>(), runtime.pending_filters());

        let tab = TabId(1);
        let view = webkit::WebView::builder().network_session(&session).user_content_manager(&runtime.user_content_manager(tab)).build();
        host.tabs.borrow_mut().push(Tab { id: tab, view: view.clone() });
        let window = gtk::Window::new();
        window.set_default_size(800, 600);
        window.set_title(Some("vsesvit-webext harness"));
        window.set_child(Some(&view));
        if show {
            window.present();
        }

        let main_loop = glib::MainLoop::new(None, false);
        let outcome = Rc::new(Cell::new(false));
        let checks = Checks { runtime: runtime.clone(), view: view.clone(), tab, server_port: server.port, hits: server.hits.clone(), installed_id: installed.id.clone(), window: window.clone() };
        glib::spawn_future_local({
            let (main_loop, outcome) = (main_loop.clone(), outcome.clone());
            async move {
                let ok = checks.run().await;
                outcome.set(ok);
                main_loop.quit();
            }
        });
        glib::timeout_add_local_once(TIMEOUT + Duration::from_secs(10), {
            let main_loop = main_loop.clone();
            move || {
                println!("[harness] FAIL: global timeout");
                main_loop.quit();
            }
        });
        main_loop.run();

        let ok = outcome.get();
        println!("[harness] RESULT: {}", if ok { "PASS" } else { "FAIL" });
        window.destroy();
        drop(view);
        drop(runtime);
        drop(host);
        let _ = std::fs::remove_dir_all(&out_dir);
        if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
    }

    struct Checks {
        runtime: Runtime,
        view: webkit::WebView,
        tab: TabId,
        server_port: u16,
        hits: Arc<Mutex<Vec<String>>>,
        installed_id: ExtensionId,
        window: gtk::Window,
    }

    impl Checks {
        async fn run(&self) -> bool {
            let started = Instant::now();
            // Filters compile asynchronously; the self-test must navigate only after they
            // are attached, and so does the harness.
            let ready = Rc::new(Cell::new(false));
            let flag = ready.clone();
            self.runtime.on_filters_ready(move || flag.set(true));
            if !wait_until(|| ready.get(), TIMEOUT).await {
                println!("[harness] FAIL: filters never became ready");
                return false;
            }
            println!("[harness] filters ready after {} ms", started.elapsed().as_millis());

            self.view.load_uri(&format!("http://127.0.0.1:{}/index.html", self.server_port));

            // 1. content script -> background -> reply
            let probe = self.wait_for_js(&self.view, "document.documentElement.dataset.vsesvitProbe", None, |v| v == "background-replied").await;
            println!("[harness] content_script: dataset.vsesvitProbe = {probe:?} after {} ms", started.elapsed().as_millis());
            let visits = self.eval(&self.view, "document.documentElement.dataset.vsesvitVisits", None).await;
            println!("[harness] content_script: dataset.vsesvitVisits = {visits:?}");
            let content_ok = probe.as_deref() == Some("background-replied");

            // 2. declarativeNetRequest: control image requested, blocked image never
            glib::timeout_future(Duration::from_millis(1000)).await;
            let hits = self.hits.lock().unwrap().clone();
            let allowed = hits.iter().any(|p| p == "/allowed.png");
            let blocked = hits.iter().any(|p| p == "/vsesvit-blocked/pixel.png");
            println!("[harness] dnr: server saw {hits:?}");
            println!("[harness] dnr: /allowed.png requested = {allowed}; /vsesvit-blocked/pixel.png requested = {blocked}");
            let dnr_ok = allowed && !blocked;

            // 3. action popup
            let actions = self.runtime.actions();
            println!("[harness] actions: {actions:?}");
            let popup = self.runtime.activate_action(&self.installed_id, Some(self.tab));
            let Some(popup) = popup else {
                println!("[harness] FAIL: activate_action returned no popup view");
                return false;
            };
            let popup_window = gtk::Window::new();
            popup_window.set_transient_for(Some(&self.window));
            popup_window.set_child(Some(&popup));
            let title = wait_for_value(|| popup.title().map(String::from).filter(|t| t.starts_with("visits=")), TIMEOUT).await;
            let n = title.as_deref().and_then(|t| t.strip_prefix("visits=")).and_then(|n| n.parse::<u64>().ok());
            println!("[harness] popup: title = {title:?} (visits = {n:?})");
            let popup_ok = n.is_some_and(|n| n >= 1);

            // Extra: page-only APIs from the popup, storage change events, unload.
            let query = self.eval_async(&popup, "return chrome.tabs.query({ active: true });").await;
            let query_ok = query.as_ref().and_then(|v| v.as_array()).is_some_and(|tabs| tabs.len() == 1 && tabs[0]["url"].as_str().is_some_and(|u| u.ends_with("/index.html")));
            println!("[harness] extra: popup chrome.tabs.query({{active:true}}) = {query} -> {}", if query_ok { "ok" } else { "unexpected" }, query = query.map(|v| v.to_string()).unwrap_or_default());
            let badge = self
                .eval_async(&popup, "await chrome.action.setBadgeText({ text: '7' }); return chrome.runtime.getURL('x/y.png') + ' ' + chrome.i18n.getUILanguage();")
                .await;
            let badge_state = self.runtime.actions().first().map(|a| a.badge_text.clone());
            println!("[harness] extra: setBadgeText -> actions().badge_text = {badge_state:?}; getURL/i18n = {badge:?}");
            let badge_ok = badge_state.as_deref() == Some("7") && badge.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("chrome-extension://") && s.contains("/x/y.png "));
            let onchanged = self
                .eval_async(
                    &popup,
                    "return await new Promise((resolve) => { chrome.storage.onChanged.addListener((c, area) => resolve({ area, keys: Object.keys(c), nv: c.harness && c.harness.newValue })); chrome.storage.local.set({ harness: 42 }); });",
                )
                .await;
            let onchanged_ok = onchanged.as_ref().is_some_and(|v| v["area"] == "local" && v["nv"] == 42);
            println!("[harness] extra: storage.onChanged in popup = {onchanged:?} -> {}", if onchanged_ok { "ok" } else { "unexpected" });
            let no_receiver = self.eval_async(&popup, "try { await chrome.tabs.sendMessage(1, { ping: 1 }); return 'replied'; } catch (e) { return String(e.message); }").await;
            println!("[harness] extra: tabs.sendMessage to a tab without a listener -> {no_receiver:?}");
            let no_receiver_ok = no_receiver.as_ref().and_then(Value::as_str).is_some_and(|s| s.contains("Receiving end does not exist"));

            drop(popup_window);
            self.runtime.unload(&self.installed_id);
            let unloaded_ok = self.runtime.loaded().is_empty() && self.runtime.actions().is_empty();
            println!("[harness] extra: after unload loaded() = {:?}, actions() = {:?}", self.runtime.loaded(), self.runtime.actions());

            let extras_ok = query_ok && badge_ok && onchanged_ok && no_receiver_ok && unloaded_ok;
            println!("[harness] content_script={content_ok} dnr_blocked={dnr_ok} popup={popup_ok} extras={extras_ok}");
            content_ok && dnr_ok && popup_ok && extras_ok
        }

        async fn eval(&self, view: &webkit::WebView, script: &str, world: Option<&str>) -> Option<String> {
            match view.evaluate_javascript_future(script, world, None).await {
                Ok(v) if v.is_string() => Some(v.to_str().to_string()),
                Ok(v) => v.to_json(0).map(|j| j.to_string()),
                Err(e) => {
                    println!("[harness] eval error: {e}");
                    None
                }
            }
        }

        async fn eval_async(&self, view: &webkit::WebView, body: &str) -> Option<Value> {
            match view.call_async_javascript_function_future(body, None, None, None).await {
                Ok(v) => v.to_json(0).and_then(|j| serde_json::from_str(&j).ok()),
                Err(e) => {
                    println!("[harness] eval_async error: {e}");
                    None
                }
            }
        }

        async fn wait_for_js(&self, view: &webkit::WebView, script: &str, world: Option<&str>, done: impl Fn(&str) -> bool) -> Option<String> {
            let deadline = Instant::now() + TIMEOUT;
            let mut last = None;
            while Instant::now() < deadline {
                last = self.eval(view, script, world).await;
                if last.as_deref().is_some_and(&done) {
                    break;
                }
                glib::timeout_future(Duration::from_millis(100)).await;
            }
            last
        }
    }

    async fn wait_until(cond: impl Fn() -> bool, timeout: Duration) -> bool {
        wait_for_value(|| cond().then_some(()), timeout).await.is_some()
    }

    async fn wait_for_value<T>(probe: impl Fn() -> Option<T>, timeout: Duration) -> Option<T> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(v) = probe() {
                return Some(v);
            }
            if Instant::now() >= deadline {
                return None;
            }
            glib::timeout_future(Duration::from_millis(50)).await;
        }
    }

    struct StdoutLog;
    static STDOUT_LOG: StdoutLog = StdoutLog;

    impl log::Log for StdoutLog {
        fn enabled(&self, m: &log::Metadata) -> bool {
            m.target().starts_with("vsesvit")
        }
        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                println!("[log] {} {}: {}", record.level(), record.target(), record.args());
            }
        }
        fn flush(&self) {}
    }

    // --- the shell side: one window, one tab -------------------------------------------

    struct Tab {
        id: TabId,
        view: webkit::WebView,
    }

    #[derive(Default)]
    struct Host {
        tabs: RefCell<Vec<Tab>>,
    }

    impl TabHost for Host {
        fn tabs(&self) -> Vec<TabInfo> {
            self.tabs
                .borrow()
                .iter()
                .enumerate()
                .map(|(i, t)| TabInfo {
                    id: t.id,
                    window_id: 1,
                    index: i as u32,
                    url: t.view.uri().map(String::from).unwrap_or_default(),
                    title: t.view.title().map(String::from).unwrap_or_default(),
                    active: i == 0,
                })
                .collect()
        }

        fn create_tab(&self, url: &str, _active: bool) -> Option<TabId> {
            println!("[harness] host: create_tab({url}) refused (single-tab harness)");
            None
        }

        fn update_tab(&self, tab: TabId, url: Option<&str>, _active: Option<bool>) -> bool {
            let tabs = self.tabs.borrow();
            let Some(t) = tabs.iter().find(|t| t.id == tab) else { return false };
            if let Some(u) = url {
                t.view.load_uri(u);
            }
            true
        }

        fn remove_tab(&self, _tab: TabId) -> bool {
            false
        }

        fn web_view(&self, tab: TabId) -> Option<webkit::WebView> {
            self.tabs.borrow().iter().find(|t| t.id == tab).map(|t| t.view.clone())
        }
    }

    // --- the probe as an InstalledExtension ---------------------------------------------

    /// Installs the probe through the real pipeline: signed test CRX, verify, unpack, commit.
    fn install_probe(profile: &Rc<RefCell<Profile>>, out_dir: &Path) -> InstalledExtension {
        let crx = out_dir.join("probe.crx");
        std::fs::write(&crx, vsesvit_core::testkit::probe_crx()).expect("write probe.crx");
        let source = InstallSource::from_path(&crx).expect("probe.crx is an install source");
        let job = profile.borrow_mut().extensions().prepare_install(source).expect("prepare install");
        let staged = job.run(&mut |_| {}).expect("install probe.crx");
        let installed = profile.borrow_mut().extensions().commit(staged).expect("commit").expect("installed");
        assert_eq!(installed.id.as_str(), vsesvit_core::testkit::PROBE_ID);
        installed
    }

    // --- fixture HTTP server --------------------------------------------------------------

    struct FixtureServer {
        port: u16,
        hits: Arc<Mutex<Vec<String>>>,
    }

    impl FixtureServer {
        fn start(dir: PathBuf) -> FixtureServer {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
            let port = listener.local_addr().expect("local addr").port();
            let hits = Arc::new(Mutex::new(Vec::new()));
            let hits_for_thread = hits.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let (dir, hits) = (dir.clone(), hits_for_thread.clone());
                    std::thread::spawn(move || serve(stream, &dir, &hits));
                }
            });
            FixtureServer { port, hits }
        }
    }

    fn serve(mut stream: TcpStream, dir: &Path, hits: &Mutex<Vec<String>>) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok() && line != "\r\n" && !line.is_empty() {
            line.clear();
        }
        let path = request_line.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/").to_owned();
        hits.lock().unwrap().push(path.clone());
        let safe = path.trim_start_matches('/');
        let file = if safe.split('/').any(|seg| seg == "..") { None } else { Some(dir.join(safe)) };
        let (status, body, mime) = match file.filter(|f| f.is_file()).and_then(|f| std::fs::read(&f).ok()) {
            Some(bytes) => ("200 OK", bytes, vsesvit_webext::mime::for_path(&path)),
            None => ("404 Not Found", b"not found".to_vec(), "text/plain"),
        };
        let head = format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    }
}
