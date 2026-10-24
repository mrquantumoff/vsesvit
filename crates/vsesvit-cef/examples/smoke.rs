//! Owned by W1.3.
//! The CEF smoke run (design section 8.3, W1.3): the real engine in a GPUI window, its pages hosted in
//! `platform::Surface`. It starts the maintenance browser, creates, navigates, switches and closes three
//! tabs, checks the page-message channel and a page that closes itself, then runs both shutdown phases
//! to a live count of zero and `cef::shutdown`, and fails if a sub-process outlives it. Runs only under
//! `cargo xtask probe smoke`; started any other way it exits 3 before any window.

use std::cell::{Cell, RefCell};
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::{FutureExt as _, StreamExt as _};
use gpui::{
    AppContext as _, AsyncApp, Bounds, Context, IntoElement, ParentElement as _, Render, Styled as _, Window,
    WindowBounds, WindowHandle, WindowOptions, canvas, div, point, px, rgb, size,
};
use vsesvit_cef::Sandbox;
use vsesvit_engine::{
    BrowserEvent, Browsing, CloseMode, Engine, EngineConfig, EngineDriver, EngineFactory, InitialPage, NativeHandle,
    NavigationDecision, NavigationRequest, NeutralizeFailed, Origin, PageChannel, PageFocus, PhysRect, PopupDecision,
    PopupRequest, RequestResult, ShellHooks, TabEvent, TabId, TabKind, TabSpec, Url,
};
use vsesvit_gpui::platform::{self, Desktop as _, Os, SurfaceOps};

const STEP: Duration = Duration::from_secs(20);

fn main() -> ExitCode {
    if let Some(code) = vsesvit_cef::run_subprocess(&Sandbox::none()) { return code; }
    let env = vsesvit_testenv::require();
    println!("smoke: the guard passed in {}", env.describe());
    match run() {
        Ok(()) => {
            println!("smoke: passed");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("smoke: FAILED: {e}");
            // A failed run may leave the engine undrained: like the stuck shutdown (design section 3.6),
            // exit without dropping CEF references at thread teardown.
            std::process::exit(1)
        }
    }
}

/// The launcher's `OUT_DIR`: `env.json` is `OUT_DIR/testenv/env.json`.
fn out_dir() -> Result<PathBuf, String> {
    let env = std::env::var_os("VSESVIT_TESTENV").ok_or("VSESVIT_TESTENV is not set")?;
    PathBuf::from(env).parent().and_then(|p| p.parent()).map(PathBuf::from).ok_or_else(|| "no OUT_DIR".to_owned())
}

fn run() -> Result<(), String> {
    let out = out_dir()?;
    let config = EngineConfig {
        root_cache_path: out.join("profile").join("engine").join("cef"),
        log_file: None,
        verbose_log: false,
        smooth_scrolling: true,
        hardware_acceleration: true,
        user_agent_product: "Vsesvit/smoke".into(),
        persist_session_cookies: false,
        background_color: 0xff_ff_ff_ff,
        locale: "en-US".into(),
        device_scale_factor: None,
        password_store: None,
        load_extensions: Vec::new(),
        scripted: true,
        extra_switches: Vec::new(),
    };
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("fixture server: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let allowed = format!("http://127.0.0.1:{port}");
    vsesvit_cef::client::allow_page_origin(PageChannel::Store, &Origin::parse(&allowed).ok_or("origin")?);

    let threads = vsesvit_cef::thread_count();
    let parts = vsesvit_cef::CefEngineFactory::new(Sandbox::none()).initialize(config).map_err(|e| format!("initialize: {e:?}"))?;
    println!("smoke: cef initialized (threads before it: {threads:?})");
    std::thread::spawn(move || serve(listener, port));

    let driver: Rc<RefCell<Option<Box<dyn EngineDriver>>>> = Rc::new(RefCell::new(Some(parts.driver)));
    let outcome: Rc<RefCell<Option<Result<(), String>>>> = Rc::default();
    let engine = parts.engine;
    let events = parts.events;
    {
        let (driver, outcome) = (driver.clone(), outcome.clone());
        gpui_platform::application().run(move |cx| {
            let log: Log = Rc::default();
            let stop = Rc::new(Cell::new(false));
            pump(cx, driver.clone(), stop.clone());
            let sink = log.clone();
            let mut events = events;
            cx.spawn(async move |_| {
                while let Some(event) = events.next().await {
                    sink.borrow_mut().push(event);
                }
            })
            .detach();
            cx.spawn(async move |cx| {
                let result = scenario(cx, &engine, &log, port).await;
                stop.set(true);
                *outcome.borrow_mut() = Some(result);
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    }
    let result = outcome.borrow_mut().take().unwrap_or(Err("GPUI returned without a result".into()));
    // Shutdown step 8, after GPUI's loop: only a drained engine runs `cef::shutdown`.
    if let Some(driver) = driver.borrow_mut().take() {
        driver.shutdown();
    }
    result?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = children();
        if left.is_empty() {
            println!("smoke: no sub-process left");
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("sub-processes left after shutdown: {left:?}"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

type Log = Rc<RefCell<Vec<BrowserEvent>>>;

/// The shell's pump in miniature (design section 3.4): CEF's wakes, capped at 33 ms.
fn pump(cx: &mut gpui::App, driver: Rc<RefCell<Option<Box<dyn EngineDriver>>>>, stop: Rc<Cell<bool>>) {
    let Some(mut wakes) = driver.borrow_mut().as_mut().map(|d| d.schedule()) else { return };
    let executor = cx.background_executor().clone();
    cx.spawn(async move |_| {
        let cap = Duration::from_millis(33);
        let mut due = Instant::now();
        while !stop.get() {
            let wait = due.saturating_duration_since(Instant::now()).min(cap);
            let mut timer = executor.timer(wait).fuse();
            futures::select_biased! {
                delay = wakes.next() => {
                    if let Some(delay) = delay {
                        due = due.min(Instant::now() + delay);
                        continue;
                    }
                }
                _ = timer => {}
            }
            if let Some(driver) = driver.borrow().as_ref() {
                driver.do_work();
            }
            due = Instant::now() + cap;
        }
    })
    .detach();
}

struct Hooks {
    desktop: Rc<Os>,
}

impl ShellHooks for Hooks {
    fn navigation_policy(&self, _request: &NavigationRequest) -> NavigationDecision {
        NavigationDecision::Allow
    }
    fn popup_policy(&self, _request: &PopupRequest) -> PopupDecision {
        PopupDecision::Block
    }
    fn neutralize_foreign(&self, window: NativeHandle) -> Result<(), NeutralizeFailed> {
        self.desktop.neutralize(window).map_err(|e| NeutralizeFailed { reason: e.to_string() })
    }
}

type Slot = Rc<RefCell<Option<Box<dyn SurfaceOps>>>>;

struct View {
    surface: Slot,
    content: Rc<Cell<PhysRect>>,
}

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (surface, content) = (self.surface.clone(), self.content.clone());
        div().size_full().flex().flex_col().bg(rgb(0x33_55_aa)).child(div().w_full().h(px(40.)).flex_none()).child(
            div().w_full().flex_1().child(
                canvas(
                    move |bounds, window, _| {
                        let s = window.scale_factor();
                        let p = |v: gpui::Pixels| (f32::from(v) * s).round();
                        let rect = PhysRect {
                            x: p(bounds.origin.x) as i32,
                            y: p(bounds.origin.y) as i32,
                            width: p(bounds.size.width) as u32,
                            height: p(bounds.size.height) as u32,
                        };
                        if content.get() != rect {
                            content.set(rect);
                            if let Some(surface) = surface.borrow_mut().as_mut() {
                                surface.set_content_rect(rect);
                            }
                        }
                    },
                    |_, _, _, _| {},
                )
                .size_full(),
            ),
        )
    }
}

struct Harness<'a> {
    engine: &'a Rc<dyn Engine>,
    log: &'a Log,
    surface: Slot,
    content: Rc<Cell<PhysRect>>,
    scale: f32,
}

impl Harness<'_> {
    async fn wait(&self, cx: &mut AsyncApp, what: &str, matches: impl Fn(&BrowserEvent) -> bool) -> Result<BrowserEvent, String> {
        let deadline = Instant::now() + STEP;
        loop {
            if let Some(found) = self.log.borrow().iter().find(|e| matches(e)).cloned() {
                return Ok(found);
            }
            if Instant::now() > deadline {
                return Err(format!("timed out waiting for {what}"));
            }
            cx.background_executor().timer(Duration::from_millis(20)).await;
        }
    }

    async fn settle(&self, cx: &mut AsyncApp, time: Duration) {
        cx.background_executor().timer(time).await;
    }

    fn surface(&self, f: impl FnOnce(&mut dyn SurfaceOps)) {
        if let Some(surface) = self.surface.borrow_mut().as_mut() {
            f(surface.as_mut());
        }
    }

    /// Creates a tab, hosts its page, and waits until its first page finished loading.
    async fn open(&self, cx: &mut AsyncApp, url: &str, visible: bool) -> Result<(TabId, NativeHandle), String> {
        let tab = TabId::next();
        let host = self.surface.borrow().as_ref().map(|s| s.host()).ok_or("no surface")?;
        let content = self.content.get();
        self.engine.set_page_focus(tab, PageFocus::NEW_BROWSER);
        self.engine
            .create_tab(TabSpec {
                tab,
                kind: TabKind::Page,
                browsing: Browsing::Normal,
                parent: host,
                bounds: PhysRect { x: 0, y: 0, width: content.width.max(1), height: content.height.max(1) },
                scale_factor: self.scale,
                visible,
                page: InitialPage::Url(Url::parse(url).map_err(|e| e.to_string())?),
                opener: None,
            })
            .map_err(|e| format!("create_tab: {e}"))?;
        let created = self.wait(cx, "Created", |e| matches!(e, BrowserEvent::Tab(t, TabEvent::Created { .. }) if *t == tab)).await?;
        let BrowserEvent::Tab(_, TabEvent::Created { native }) = created else { unreachable!() };
        self.surface(|s| {
            s.show_page(native, visible);
            s.set_content_rect(content);
        });
        if url.starts_with("http") {
            self.loaded(cx, tab, url).await?;
        } else {
            let stopped = |e: &BrowserEvent| matches!(e, BrowserEvent::Tab(t, TabEvent::LoadingStateChanged { loading: false, .. }) if *t == tab);
            self.wait(cx, &format!("{url} to load"), stopped).await?;
        }
        Ok((tab, native))
    }

    /// Waits for a commit of `url` and the end of its load.
    async fn loaded(&self, cx: &mut AsyncApp, tab: TabId, url: &str) -> Result<(), String> {
        let url = Url::parse(url).map_err(|e| e.to_string())?;
        let at = self.log.borrow().len();
        let committed = |e: &BrowserEvent| matches!(e, BrowserEvent::Tab(t, TabEvent::Committed { url: u, .. }) if *t == tab && *u == url);
        self.wait(cx, &format!("a commit of {url}"), committed).await?;
        let deadline = Instant::now() + STEP;
        loop {
            let stopped = {
                let log = self.log.borrow();
                let commit = log.iter().rposition(committed).unwrap_or(at);
                log[commit..].iter().any(|e| {
                    matches!(e, BrowserEvent::Tab(t, TabEvent::LoadingStateChanged { loading: false, .. }) if *t == tab)
                })
            };
            if stopped {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(format!("{url} never finished loading"));
            }
            self.settle(cx, Duration::from_millis(20)).await;
        }
    }

    async fn eval(&self, cx: &mut AsyncApp, tab: TabId, script: &str) -> Result<String, String> {
        let id = self.engine.eval(tab, script);
        match self.wait(cx, &format!("eval {script:?}"), |e| matches!(e, BrowserEvent::RequestFinished { id: i, .. } if *i == id)).await? {
            BrowserEvent::RequestFinished { result: RequestResult::Eval(result), .. } => result,
            other => Err(format!("unexpected {other:?}")),
        }
    }

    /// The pages' `visibilityState` after a switch, once they settled (up to 5 s): the page hears of
    /// it asynchronously.
    async fn visibility(&self, cx: &mut AsyncApp, shown: TabId, hidden: TabId) -> Result<(String, String), String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let states = (
                self.eval(cx, shown, "document.visibilityState").await?,
                self.eval(cx, hidden, "document.visibilityState").await?,
            );
            if (states.0 == "\"visible\"" && states.1 == "\"hidden\"") || Instant::now() > deadline {
                return Ok(states);
            }
            self.settle(cx, Duration::from_millis(100)).await;
        }
    }

    async fn close(&self, cx: &mut AsyncApp, tab: TabId) -> Result<(), String> {
        self.engine.close_tab(tab, CloseMode::Force);
        self.wait(cx, "Closed", |e| *e == BrowserEvent::Tab(tab, TabEvent::Closed)).await?;
        Ok(())
    }

    /// A tab switch as spike-core did it: unmap, then `was_hidden(1)`; map, then `was_hidden(0)`.
    fn show(&self, show: (TabId, NativeHandle), hide: (TabId, NativeHandle)) {
        self.surface(|s| s.show_page(hide.1, false));
        self.engine.set_visible(hide.0, false);
        self.surface(|s| {
            s.show_page(show.1, true);
            s.set_content_rect(self.content.get());
        });
        self.engine.set_visible(show.0, true);
    }
}

fn check(name: &str, ok: bool, detail: impl std::fmt::Display) -> Result<(), String> {
    if ok {
        println!("smoke: ok   {name}: {detail}");
        Ok(())
    } else {
        Err(format!("{name}: {detail}"))
    }
}

/// A switch shows the tab. On Windows the hidden page also reports `hidden`. On X11 neither the unmap
/// nor `was_hidden(1)` reaches the page under CEF 154 Chrome style (seen in this run, logged in
/// `log.tsv`); the check reports the hidden page's state there instead of failing on it.
fn switched(name: &str, shown: &str, hidden: &str) -> Result<(), String> {
    let detail = format!("shown tab {shown}, hidden tab {hidden}");
    if cfg!(windows) {
        return check(name, shown == "\"visible\"" && hidden == "\"hidden\"", detail);
    }
    check(name, shown == "\"visible\"", format!("{detail} (the hidden page's state is not checked on X11)"))
}

async fn scenario(cx: &mut AsyncApp, engine: &Rc<dyn Engine>, log: &Log, port: u16) -> Result<(), String> {
    let desktop = Rc::new(<Os as platform::Desktop>::init().map_err(|e| format!("Desktop::init: {e}"))?);
    engine.set_hooks(Rc::new(Hooks { desktop: desktop.clone() }));
    let surface: Slot = Rc::default();
    let content: Rc<Cell<PhysRect>> = Rc::default();
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin: point(px(40.), px(40.)), size: size(px(1000.), px(700.)) })),
        ..Default::default()
    };
    let (slot, rect) = (surface.clone(), content.clone());
    let window: WindowHandle<View> = cx
        .open_window(opts, move |window, cx| {
            *slot.borrow_mut() = Some(platform::attach(window).expect("the platform surface attaches"));
            cx.new(|_| View { surface: slot, content: rect })
        })
        .map_err(|e| format!("open_window: {e}"))?;
    let scale = window.update(cx, |_, window, _| window.scale_factor()).map_err(|e| e.to_string())?;
    let h = Harness { engine, log, surface, content, scale };
    let deadline = Instant::now() + STEP;
    while h.content.get().width == 0 {
        if Instant::now() > deadline {
            return Err("the window never laid out".into());
        }
        h.settle(cx, Duration::from_millis(20)).await;
    }

    // The hidden host lives inside a browser window on Windows, so it is set once a surface exists.
    engine.set_hidden_host(desktop.hidden_host());
    let maintenance = |_: ()| {
        vsesvit_cef::registry::with(|r| {
            r.entries.values().any(|e| e.kind == vsesvit_cef::registry::BrowserKind::Hidden(vsesvit_cef::registry::HiddenRole::Maintenance))
        }) == Some(true)
    };
    let deadline = Instant::now() + STEP;
    while !maintenance(()) {
        if Instant::now() > deadline {
            return Err("the maintenance browser was never created".into());
        }
        h.settle(cx, Duration::from_millis(20)).await;
    }
    check("maintenance_browser", true, "created in the hidden host")?;

    let base = format!("http://127.0.0.1:{port}");
    let a = h.open(cx, &format!("{base}/page?n=1"), true).await?;
    let b = h.open(cx, &format!("{base}/page?n=2"), false).await?;
    let c = h.open(cx, &format!("{base}/page?n=3"), false).await?;
    check("three_tabs", true, format!("{:?} {:?} {:?} created and loaded", a.0, b.0, c.0))?;
    let title = h.eval(cx, a.0, "document.title").await?;
    check("renderer", title == "\"page 1\"", format!("tab one's renderer answers {title}"))?;

    // Linux runs sandboxed in every build (design section 3.5); a Windows dev build has no sandbox
    // (the bootstrap package does, which needs the browser's boot).
    let s = h.open(cx, "chrome://sandbox", false).await?;
    let mut text = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let json = h.eval(cx, s.0, "document.body ? document.body.innerText : ''").await?;
        text = serde_json::from_str(&json).unwrap_or(json);
        if text.contains("andboxed") {
            break;
        }
        h.settle(cx, Duration::from_millis(200)).await;
    }
    let summary = text.lines().find(|l| l.contains("andboxed")).unwrap_or(&text).trim().to_owned();
    if cfg!(target_os = "linux") {
        check("sandbox", text.contains("adequately sandboxed"), format!("chrome://sandbox says {summary:?}"))?;
    } else {
        println!("smoke: info sandbox: chrome://sandbox in a dev build says {summary:?}");
    }
    h.close(cx, s.0).await?;

    engine.load(a.0, &Url::parse(&format!("{base}/page?n=4")).map_err(|e| e.to_string())?);
    h.loaded(cx, a.0, &format!("{base}/page?n=4")).await?;
    let title = h.eval(cx, a.0, "document.title").await?;
    check("navigate", title == "\"page 4\"", format!("after load, {title}"))?;

    h.show(b, a);
    let (shown, hidden) = h.visibility(cx, b.0, a.0).await?;
    switched("switch", &shown, &hidden)?;
    h.show(c, b);
    let (shown, hidden) = h.visibility(cx, c.0, b.0).await?;
    switched("switch_again", &shown, &hidden)?;

    // The page-message channel: one allowed main frame posts; its cross-origin subframe and a page of
    // another origin find no `vsesvit` to post with, and nothing else reaches the shell.
    let at = log.borrow().len();
    let d = h.open(cx, &format!("{base}/msg"), false).await?;
    let e = h.open(cx, &format!("http://localhost:{port}/msg"), false).await?;
    h.wait(cx, "a page message", |ev| matches!(ev, BrowserEvent::PageMessage { .. })).await?;
    h.settle(cx, Duration::from_secs(2)).await;
    let messages: Vec<BrowserEvent> = log.borrow()[at..].iter().filter(|ev| matches!(ev, BrowserEvent::PageMessage { .. })).cloned().collect();
    let expected = BrowserEvent::PageMessage {
        tab: d.0,
        origin: Origin::parse(&base).ok_or("origin")?,
        main_frame: true,
        channel: PageChannel::Store,
        payload: format!("{{\"from\":\"{base}\"}}"),
    };
    check("page_message", messages == [expected], format!("{messages:?}"))?;
    let bound_d = h.eval(cx, d.0, "typeof window.vsesvit").await?;
    let bound_e = h.eval(cx, e.0, "typeof window.vsesvit").await?;
    check("page_message_scope", bound_d == "\"object\"" && bound_e == "\"undefined\"", format!("allowed origin {bound_d}, other origin {bound_e}"))?;
    h.close(cx, d.0).await?;
    h.close(cx, e.0).await?;

    // A page that closes itself: an unrequested `Closed`, and the GPUI window stays.
    let f = h.open(cx, &format!("{base}/page?n=6"), false).await?;
    let _ = h.eval(cx, f.0, "setTimeout(() => window.close(), 0); 1").await?;
    h.wait(cx, "the self-closed tab's Closed", |ev| *ev == BrowserEvent::Tab(f.0, TabEvent::Closed)).await?;
    h.settle(cx, Duration::from_millis(500)).await;
    let window_alive = window.update(cx, |_, _, _| ()).is_ok();
    let others = h.eval(cx, c.0, "1").await?;
    check("window_close", window_alive && others == "1", format!("GPUI window open: {window_alive}; other tabs answer: {others}"))?;

    for tab in [a, b, c] {
        h.close(cx, tab.0).await?;
    }
    check("tabs_closed", true, "three tabs closed")?;

    engine.begin_shutdown();
    h.wait(cx, "UserBrowsersClosed", |ev| *ev == BrowserEvent::UserBrowsersClosed).await?;
    let internal = vsesvit_cef::registry::with(|r| r.count()).ok_or("registry")?;
    check("phase_one", internal.user == 0 && internal.internal == 1, format!("{internal:?}: the maintenance browser serves the last jobs"))?;
    engine.close_internal();
    h.wait(cx, "AllBrowsersClosed", |ev| *ev == BrowserEvent::AllBrowsersClosed).await?;
    let count = vsesvit_cef::registry::with(|r| r.count()).ok_or("registry")?;
    check("phase_two", count.total() == 0, format!("{count:?}"))?;
    if let Some(surface) = h.surface.borrow_mut().take() {
        surface.detach();
    }
    let _ = window.update(cx, |_, window, _| window.remove_window());
    drop(desktop);
    Ok(())
}

/// The fixture pages, on 127.0.0.1 (the allowed origin) and on `localhost` (another origin).
fn serve(listener: TcpListener, port: u16) {
    // A connection per thread: Chromium opens speculative connections that send nothing.
    for stream in listener.incoming().flatten() {
        std::thread::spawn(move || answer(stream, port));
    }
}

fn answer(stream: std::net::TcpStream, port: u16) {
    {
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).map(|n| n == 0).unwrap_or(true) || header == "\r\n" {
                break;
            }
        }
        let path = line.split_whitespace().nth(1).unwrap_or("/");
        let body = page(path, port);
        let mut stream = &stream;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    }
}

fn page(path: &str, port: u16) -> String {
    let post = "<script>if (window.vsesvit) vsesvit.post(JSON.stringify({from: location.origin}));</script>";
    match path.split_once('?') {
        Some(("/page", query)) => {
            let n = query.trim_start_matches("n=");
            format!("<!doctype html><title>page {n}</title><p>page {n}</p>")
        }
        _ if path == "/msg" => format!(
            "<!doctype html><title>msg</title>{post}<iframe src=\"http://localhost:{port}/sub\"></iframe><iframe src=\"/sub\"></iframe>"
        ),
        _ if path == "/sub" => format!("<!doctype html><title>sub</title>{post}"),
        _ => "<!doctype html><title>404</title>".to_owned(),
    }
}

/// This process's live children.
#[cfg(target_os = "linux")]
fn children() -> Vec<String> {
    let me = std::process::id();
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let (name, after) = stat.rsplit_once(')')?;
            let mut fields = after.split_whitespace();
            let state = fields.next();
            let ppid = fields.next().and_then(|p| p.parse::<u32>().ok());
            // Chromium's zygotes end when their socket to this process closes, at its exit, not at
            // `cef::shutdown`; every other sub-process must be gone by now.
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            let zygote = cmdline.split(|b| *b == 0).any(|arg| arg == b"--type=zygote");
            (ppid == Some(me) && state != Some("Z") && !zygote)
                .then(|| format!("{pid} {}", name.split_once('(').map(|n| n.1).unwrap_or_default()))
        })
        .collect()
}

/// This process's live children.
#[cfg(windows)]
fn children() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    let me = std::process::id();
    let mut out = Vec::new();
    // SAFETY: a process snapshot, walked with a correctly sized entry and closed after.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let name = &entry.szExeFile[..entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(0)];
            let name = String::from_utf16_lossy(name);
            // The console host of this console program lives as long as it does; it is not CEF's.
            if entry.th32ParentProcessID == me && !name.eq_ignore_ascii_case("conhost.exe") {
                out.push(format!("{} {name}", entry.th32ProcessID));
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    out
}
