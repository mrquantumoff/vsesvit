//! Owned by W1.8.
//! `vsesvit --self-test OUT_DIR [--network] [--phase NAME]` (design section 7.2), started by boot step
//! 10 once the first windows and the maintenance browser exist. A run drives the real windows, tabs
//! and engine from one GPUI task, check by check, each bounded by its own timeout; a timeout fails
//! the check with the last value it observed. The run ends through the shutdown sequence with exit 0
//! only if every check of the GPUI flavour passed. A phase is a re-executed run of its own
//! (`--phase NAME`, on a scratch OUT_DIR the parent chose): it does one scripted thing and quits, and
//! the parent's check reads its exit code and log.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use gpui::{Action, App, AsyncApp, Entity, WindowHandle};
use vsesvit_core::testkit::FixtureServer;
use vsesvit_core::testkit::report::{Check, Report};
use vsesvit_engine::{RequestResult, TabId, Url};
use vsesvit_testenv::PrivateInput;

use super::{checks, report};
use crate::app::quit::{self, ExitIntent};
use crate::app::shell::Shell;
use crate::engine_bridge::requests;
use crate::tab::Tab;
use crate::window::BrowserWindow;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
pub const POLL: Duration = Duration::from_millis(100);
/// Boot to the first window with its first page, debug builds included.
const FIRST_WINDOW: Duration = Duration::from_secs(60);

pub fn start(out_dir: PathBuf, network: bool, phase: Option<String>, cx: &mut App) {
    cx.spawn(async move |cx| match phase {
        None => {
            let code = run(&out_dir, network, cx).await;
            cx.update(|cx| quit::request_with_code(ExitIntent::Quit, code, cx));
        }
        Some(name) => {
            // A phase quits itself, in the way it tests; one that cannot ends the run with 1.
            if let Err(e) = checks::foundation::phase(&name, cx).await {
                log::error!("self-test phase {name}: {e}");
                cx.update(|cx| quit::request_with_code(ExitIntent::Quit, 1, cx));
            }
        }
    })
    .detach();
}

async fn run(out_dir: &Path, network: bool, cx: &mut AsyncApp) -> u8 {
    let mut run = match Run::new(out_dir, network, cx).await {
        Ok(run) => run,
        Err(e) => return report::finish(report::new(network), Some(&e), out_dir),
    };
    let stopped = checks::foundation::run(&mut run, cx).await.err();
    report::finish(run.report, stopped.as_deref(), out_dir)
}

/// What every check may use.
pub struct Env {
    pub out_dir: PathBuf,
    pub network: bool,
    pub server: FixtureServer,
    /// Keys and pointer input into the test environment only (design section 7.3).
    pub input: PrivateInput,
    pub window: WindowHandle<BrowserWindow>,
    pub view: Entity<BrowserWindow>,
}

/// A run in progress: its environment and its report.
pub struct Run {
    pub env: Env,
    report: Report,
}

impl Run {
    async fn new(out_dir: &Path, network: bool, cx: &mut AsyncApp) -> Result<Run, String> {
        // Boot's guard passed; asking again hands out the environment's input.
        let input = vsesvit_testenv::require().input()?;
        let server = FixtureServer::start().map_err(|e| format!("fixture server: {e}"))?;
        let started = Instant::now();
        let (window, view) = loop {
            if let Some(found) = cx.update(first_window) {
                break found;
            }
            if started.elapsed() > FIRST_WINDOW {
                return Err(format!("no browser window after {FIRST_WINDOW:?}"));
            }
            sleep(cx, POLL).await;
        };
        let env = Env { out_dir: out_dir.to_owned(), network, server, input, window, view };
        Ok(Run { env, report: report::new(network) })
    }

    /// Runs one check within `limit`: its `Ok` text or `Err` reason becomes the report's detail; past
    /// the limit it fails with what `probe` last saw.
    pub async fn check(
        &mut self,
        name: &'static str,
        limit: Duration,
        cx: &mut AsyncApp,
        f: impl AsyncFnOnce(&Env, &mut AsyncApp, &Probe) -> Result<String, String>,
    ) {
        let probe = Probe::default();
        let started = Instant::now();
        let timer = cx.background_executor().timer(limit);
        let outcome = {
            let mut work = std::pin::pin!(f(&self.env, cx, &probe).fuse());
            let mut timer = timer.fuse();
            futures::select_biased! {
                outcome = work => Some(outcome),
                () = timer => None,
            }
        };
        let (ok, detail) = match outcome {
            Some(Ok(detail)) => (true, detail),
            Some(Err(detail)) => (false, detail),
            None => (false, format!("timed out after {} s; last observed: {}", limit.as_secs(), probe.last())),
        };
        report::push(&mut self.report, Check { name, ok, ms: started.elapsed().as_millis(), detail });
    }
}

/// The last value a check observed, reported if it times out.
#[derive(Clone, Default)]
pub struct Probe(Rc<RefCell<String>>);

impl Probe {
    pub fn observe(&self, value: impl std::fmt::Display) {
        *self.0.borrow_mut() = value.to_string();
    }

    pub fn last(&self) -> String {
        self.0.borrow().clone()
    }
}

pub async fn sleep(cx: &mut AsyncApp, duration: Duration) {
    cx.background_executor().timer(duration).await;
}

/// Polls `f` until it returns `Some`; the check's timeout bounds the wait.
pub async fn until<T>(cx: &mut AsyncApp, probe: &Probe, mut f: impl FnMut(&mut App, &Probe) -> Option<T>) -> T {
    loop {
        if let Some(value) = cx.update(|cx| f(cx, probe)) {
            return value;
        }
        sleep(cx, POLL).await;
    }
}

/// The most recently activated browser window, once it has its view.
pub fn first_window(cx: &mut App) -> Option<(WindowHandle<BrowserWindow>, Entity<BrowserWindow>)> {
    let handle = Shell::state(cx).read(cx).windows().first()?.handle?;
    let handle = handle.downcast::<BrowserWindow>()?;
    let view = handle.entity(cx).ok()?;
    Some((handle, view))
}

/// A window's tabs, in order.
pub fn tabs(view: &Entity<BrowserWindow>, cx: &App) -> Vec<Entity<Tab>> {
    let state = Shell::state(cx);
    view.read(cx).tabs().read(cx).tabs().iter().filter_map(|&id| state.read(cx).tab(id)).collect()
}

/// A window's selected tab.
pub fn selected(view: &Entity<BrowserWindow>, cx: &App) -> Option<Entity<Tab>> {
    let id = view.read(cx).tabs().read(cx).selected()?;
    Shell::state(cx).read(cx).tab(id)
}

/// Runs `action` in `window`, as its key or button would.
pub fn dispatch(window: WindowHandle<BrowserWindow>, action: impl Action, cx: &mut AsyncApp) -> Result<(), String> {
    window.update(cx, |_, window, cx| window.dispatch_action(Box::new(action), cx)).map_err(|e| e.to_string())
}

/// Evaluates `script` in `tab`'s main frame; its value as JSON.
pub async fn eval(tab: TabId, script: &str, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    let task = cx.update(|cx| requests::eval(cx, tab, script));
    match task.await {
        Some(RequestResult::Eval(Ok(json))) => serde_json::from_str(&json).map_err(|e| format!("{json}: {e}")),
        Some(RequestResult::Eval(Err(e))) => Err(e),
        other => Err(format!("eval answered {other:?}")),
    }
}

/// Waits until `tab` shows `title` and has stopped loading.
pub async fn loaded(tab: &Entity<Tab>, title: &str, cx: &mut AsyncApp, probe: &Probe) {
    shows(tab, None, title, cx, probe).await
}

/// Waits until `tab` has committed `url`, shows `title` and has stopped loading: a page loaded
/// again with the same title is not taken for the new one.
pub async fn loaded_at(tab: &Entity<Tab>, url: &Url, title: &str, cx: &mut AsyncApp, probe: &Probe) {
    shows(tab, Some(url), title, cx, probe).await
}

async fn shows(tab: &Entity<Tab>, url: Option<&Url>, title: &str, cx: &mut AsyncApp, probe: &Probe) {
    until(cx, probe, |cx, p| {
        let t = tab.read(cx);
        p.observe(format!("tab {} url={:?} title={:?} loading={}", t.id().get(), t.committed_url().map(Url::as_str), t.page_title(), t.loading()));
        (t.page_title() == title && !t.loading() && url.is_none_or(|url| t.committed_url() == Some(url))).then_some(())
    })
    .await
}
