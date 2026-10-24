//! Owned by W1.4.
//! Stand-ins for what wave 1's other units build, so boot runs end to end before they land: the pump
//! and the router (W1.6's `engine_bridge`), the first window (W1.7's `BrowserWindow`, opened by
//! W1.5's state from the startup plan), and the handling of forwarded launches (W1.5's
//! `BrowserState::handle_invocation`). When those units land, `boot::start` calls theirs and this
//! file goes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use futures::channel::mpsc::UnboundedReceiver;
use futures::{FutureExt as _, StreamExt as _};
use gpui::{
    App, AppContext as _, Bounds, Context, IntoElement, Render, Styled as _, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, div,
};
use gpui_component::ActiveTheme as _;
use vsesvit_engine::BrowserEvent;

use super::instance::Invocation;
use crate::app::quit::{self, DriverSlot, ExitIntent, ShutdownShell};
use crate::platform::{self, Desktop as _, Os, SurfaceOps};
use crate::theme::{self, tokens};

/// The pump's longest wait while the engine is active, and once it has been idle for [`IDLE_AFTER`]
/// (design section 3.4).
const ACTIVE_CAP: Duration = Duration::from_millis(33);
const IDLE_CAP: Duration = Duration::from_millis(250);
const IDLE_AFTER: Duration = Duration::from_secs(1);

/// Starts the one task that calls the driver's `do_work`, outside any GPUI update. Returns what
/// stops it: the task ends after its current step, and the driver stays in `driver`.
pub fn pump(driver: DriverSlot, cx: &mut App) -> Box<dyn FnOnce()> {
    let Some(wakes) = driver.borrow_mut().as_mut().map(|d| d.schedule()) else {
        return Box::new(|| {});
    };
    let stopped = Rc::new(Cell::new(false));
    let executor = cx.background_executor().clone();
    let running = stopped.clone();
    cx.spawn(async move |_| {
        let mut wakes = wakes;
        let mut due = executor.now();
        let mut active_until = executor.now() + IDLE_AFTER;
        while !running.get() {
            let now = executor.now();
            let cap = if now < active_until { ACTIVE_CAP } else { IDLE_CAP };
            let mut timer = executor.timer(due.saturating_duration_since(now).min(cap)).fuse();
            futures::select_biased! {
                delay = wakes.select_next_some() => {
                    due = due.min(now + delay);
                    active_until = now + IDLE_AFTER;
                    continue;
                }
                _ = timer => {}
            }
            if running.get() {
                break;
            }
            if let Some(driver) = driver.borrow().as_ref() {
                driver.do_work();
            }
            let now = executor.now();
            due = now + if now < active_until { ACTIVE_CAP } else { IDLE_CAP };
        }
    })
    .detach();
    Box::new(move || stopped.set(true))
}

/// Delivers the engine's events after each pump step: the two shutdown events to `app::quit`;
/// everything else waits for W1.6's router.
pub fn route(mut events: UnboundedReceiver<BrowserEvent>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Some(event) = events.next().await {
            cx.update(|cx| match event {
                BrowserEvent::UserBrowsersClosed => quit::user_browsers_closed(cx),
                BrowserEvent::AllBrowsersClosed => quit::all_browsers_closed(cx),
                BrowserEvent::EngineError(e) => log::error!("engine: {e}"),
                other => log::debug!("engine event with no route yet: {other:?}"),
            });
        }
    })
    .detach();
}

/// The first window until W1.7's: an empty themed window whose content host carries the platform
/// surface, so the hidden host has a browser window on Windows. Closing it quits.
pub struct StubWindow {
    surface: Option<Box<dyn SurfaceOps>>,
}

impl Render for StubWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(cx.theme().background)
    }
}

pub fn open_window(cx: &mut App) -> Result<WindowHandle<StubWindow>, String> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, tokens::WINDOW_DEFAULT, cx))),
        titlebar: Some(TitlebarOptions { title: Some("Vsesvit".into()), ..Default::default() }),
        ..Default::default()
    };
    let failed: Rc<RefCell<Option<String>>> = Rc::default();
    let report = failed.clone();
    let window = cx
        .open_window(options, move |window, cx| {
            theme::follow_desktop(window);
            // Deferred: inside this callback the window is checked out of the app, so the quit's step 3
            // could not hide it.
            window.on_window_should_close(cx, |_, cx| {
                cx.defer(|cx| quit::request(ExitIntent::Quit, cx));
                false
            });
            let surface = platform::attach(window).map_err(|e| *report.borrow_mut() = Some(format!("the window's content host: {e}"))).ok();
            cx.new(|_| StubWindow { surface })
        })
        .map_err(|e| format!("the window did not open: {e}"))?;
    match failed.take() {
        Some(e) => Err(e),
        None => Ok(window),
    }
}

/// Steps 2 and 3 for the stub window: no session to save; the window's surface detaches (on
/// Windows the hidden host moves out first) and the window goes.
#[derive(Default)]
pub struct StubShell {
    pub window: RefCell<Option<WindowHandle<StubWindow>>>,
}

impl ShutdownShell for StubShell {
    fn save_session(&self, _: &mut App) {}

    fn hide_windows(&self, cx: &mut App) {
        let Some(window) = self.window.borrow_mut().take() else { return };
        let hidden = window.update(cx, |view, window, _| {
            if let Some(surface) = view.surface.take() {
                surface.detach();
            }
            window.remove_window();
        });
        if let Err(e) = hidden {
            log::error!("shutdown: the window stays until GPUI ends: {e}");
        }
    }
}

/// Answers forwarded launches until W1.5 opens their windows: the stub window comes forward and the
/// launch exits 0; while quitting, it exits 1 with the reason.
pub fn serve(mut invocations: UnboundedReceiver<Invocation>, desktop: Rc<Os>, shell: Rc<StubShell>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Some(Invocation { request, mut reply }) = invocations.next().await {
            cx.update(|cx| {
                log::info!("a launch was forwarded here: {:?} in {}", request.args, request.cwd.display());
                if quit::is_quitting(cx) {
                    reply.stderr("vsesvit: Vsesvit is shutting down; start it again in a moment");
                    reply.finish(1);
                    return;
                }
                if let Some(window) = *shell.window.borrow() {
                    let _ = window.update(cx, |_, window, _| desktop.raise_for_invocation(window, request.startup_id.as_deref()));
                }
                reply.finish(0);
            });
        }
    })
    .detach();
}
