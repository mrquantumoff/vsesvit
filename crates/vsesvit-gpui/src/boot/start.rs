//! Owned by W1.4.
//! Boot steps 10 and 10a (design section 3.1), inside GPUI's `run`: the theme, the pump and the event
//! delivery, the shutdown sequence, `Desktop`, the first window, the hidden host with the
//! maintenance browser, the start-up clear, the instance accept thread, the extension hand-off once
//! the window shows, and the self-test runner.

use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{App, Global};
use vsesvit_core::cli::Mode;
use vsesvit_engine::{BrowserEvent, Engine};

use super::extensions::AtStart;
use super::instance::Listener;
use super::interim::{self, StubShell};
use crate::app::quit::{self, DriverSlot, EndedSlot, ExitIntent, QuitParts};
use crate::platform::{self, Desktop as _, Os};

/// The start-up clear's cap (design section 3.7), as WinUI's.
const STARTUP_CLEAR_CAP: Duration = Duration::from_secs(5);

/// What boot hands GPUI.
pub struct StartArgs {
    pub engine: Rc<dyn Engine>,
    pub events: UnboundedReceiver<BrowserEvent>,
    pub driver: DriverSlot,
    pub ended: EndedSlot,
    pub listener: Option<Listener>,
    pub mode: Mode,
    pub extensions: AtStart,
}

/// The process-wide desktop. It owns the hidden host, so it lives as long as GPUI's app, past the
/// last browser's close.
pub struct Booted {
    pub desktop: Rc<Os>,
}

impl Global for Booted {}

pub fn start(args: StartArgs, cx: &mut App) {
    crate::theme::init(cx);
    let stop_pump = interim::pump(args.driver.clone(), cx);
    interim::route(args.events, cx);
    let shell = Rc::new(StubShell::default());
    let parts = QuitParts { engine: args.engine.clone(), shell: shell.clone(), stop_pump, driver: args.driver, ended: args.ended };
    quit::install(parts, cx);

    let desktop = match <Os as platform::Desktop>::init() {
        Ok(desktop) => Rc::new(desktop),
        Err(e) => return fail(&format!("vsesvit: the desktop could not be set up: {e}"), cx),
    };
    cx.set_global(Booted { desktop: desktop.clone() });
    let window = match interim::open_window(cx) {
        Ok(window) => window,
        Err(e) => return fail(&format!("vsesvit: {e}"), cx),
    };
    *shell.window.borrow_mut() = Some(window);
    // On Windows the hidden host lives in a browser window, so it is set once the first window has its
    // surface. It starts the maintenance browser, before the first restored tab exists.
    args.engine.set_hidden_host(desktop.hidden_host());
    let clear = crate::privacy::site_data::clear_at_start(cx);
    cx.spawn(async move |cx| {
        let mut timer = cx.background_executor().timer(STARTUP_CLEAR_CAP).fuse();
        let mut clear = clear.fuse();
        futures::select_biased! {
            () = clear => {}
            () = timer => log::warn!("the start-up clear ran past {STARTUP_CLEAR_CAP:?}; restored tabs load anyway"),
        }
    })
    .detach();

    let needs_restoring = args.extensions.needs_restoring;
    let _ = window.update(cx, |_, window, _| window.on_next_frame(move |_, cx| crate::extensions::restore::start(needs_restoring, cx)));

    if let Some(listener) = args.listener {
        match listener.spawn_accept() {
            Ok(invocations) => interim::serve(invocations, desktop, shell, cx),
            Err(e) => log::error!("instance: no accept thread, so later launches cannot reach this process: {e}"),
        }
    }
    #[cfg(feature = "self-test")]
    if let Mode::SelfTest { out_dir, network, phase } = args.mode {
        crate::self_test::runner::start(out_dir, network, phase, cx);
    }
}

/// A failure after the engine started: the user sees it, and the engine still shuts down in order.
fn fail(text: &str, cx: &mut App) {
    platform::fatal_message(text);
    quit::request_with_code(ExitIntent::Quit, 1, cx);
}
