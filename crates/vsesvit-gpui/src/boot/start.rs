//! Owned by W1.4.
//! Boot steps 10 and 10a (design section 3.1), inside GPUI's `run`: the theme, `Desktop`, the engine
//! bridge (pump, router, hooks), the shutdown sequence, the app with its first windows, the hidden host
//! with the maintenance browser, the start-up clear, the instance accept thread, the extension
//! hand-off once the first window shows, and the self-test runner.

use std::rc::Rc;
use std::time::Duration;

use futures::channel::mpsc::UnboundedReceiver;
use futures::{FutureExt as _, StreamExt as _};
use gpui::{App, Global};
use vsesvit_core::cli::Mode;
use vsesvit_engine::{BrowserEvent, Engine};

use super::extensions::AtStart;
use super::instance::{Invocation as Forwarded, Listener};
use crate::app::quit::{self, DriverSlot, EndedSlot, QuitParts};
use crate::app::shell::{Core, ProfileLocation, Routes, Shutdown};
use crate::app::{self, StartArgs as AppArgs};
use crate::platform::{self, Desktop as _, Os};
use crate::state::{BrowserState, Invocation};
use crate::window::WindowHost;

/// The start-up clear's cap (design section 3.7), as WinUI's.
const STARTUP_CLEAR_CAP: Duration = Duration::from_secs(5);

/// What boot hands GPUI.
pub struct StartArgs {
    pub core: Core,
    pub engine: Rc<dyn Engine>,
    pub events: UnboundedReceiver<BrowserEvent>,
    /// Holds the engine's driver: the pump steps it, shutdown takes it.
    pub driver: DriverSlot,
    pub ended: EndedSlot,
    pub listener: Option<Listener>,
    pub location: ProfileLocation,
    pub mode: Mode,
    /// The process's own command line.
    pub first: Invocation,
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
    let desktop = match <Os as platform::Desktop>::init() {
        Ok(desktop) => Rc::new(desktop),
        Err(e) => {
            // Without a desktop there is no bridge to pump the engine through its shutdown, so the
            // run ends as a stuck one: no engine call follows.
            platform::fatal_message(&format!("vsesvit: the desktop could not be set up: {e}"));
            cx.quit();
            return;
        }
    };
    cx.set_global(Booted { desktop: desktop.clone() });
    let presses = desktop.clone();
    crate::overlay::install(move || presses.pointer_presses(), cx);
    let started = crate::engine_bridge::start(cx, args.engine, args.events, args.driver.clone(), desktop.clone(), Rc::new(Routes::default()));
    let pump = started.pump;
    quit::install(
        QuitParts {
            engine: started.engine.clone(),
            shell: Rc::new(Shutdown),
            stop_pump: Box::new(move || pump.halt()),
            driver: args.driver,
            ended: args.ended,
        },
        cx,
    );

    let scripted = !matches!(args.mode, Mode::Browse);
    let app = AppArgs {
        core: args.core,
        engine: started.engine.clone(),
        location: args.location,
        scripted,
        hidden_host: desktop.hidden_host(),
        first: args.first,
    };
    let state = app::start(cx, app, WindowHost::native(desktop.clone()));
    // On Windows the hidden host lives in a browser window, so it is set once the first windows have
    // their surfaces. It starts the maintenance browser, before any tab's browser: those are created
    // when their window first lays out.
    started.engine.set_hidden_host(desktop.hidden_host());
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
    let first_window = state.read(cx).windows().iter().find_map(|w| w.handle);
    match first_window {
        Some(window) => {
            let _ = window.update(cx, |_, window, _| {
                window.on_next_frame(move |_, cx| crate::extensions::restore::start(needs_restoring, cx))
            });
        }
        None => crate::extensions::restore::start(needs_restoring, cx),
    }

    if let Some(listener) = args.listener {
        match listener.spawn_accept() {
            Ok(invocations) => serve(invocations, state, cx),
            Err(e) => log::error!("instance: no accept thread, so later launches cannot reach this process: {e}"),
        }
    }
    #[cfg(feature = "self-test")]
    if let Mode::SelfTest { out_dir, network, phase } = args.mode {
        crate::self_test::runner::start(out_dir, network, phase, cx);
    }
}

/// Runs each forwarded launch as the state's invocation and answers the launching process: a line on
/// its stderr for each target that could not be opened, then its exit status. While quitting, it
/// exits 1 with the reason.
fn serve(mut invocations: UnboundedReceiver<Forwarded>, state: gpui::Entity<BrowserState>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Some(Forwarded { request, mut reply }) = invocations.next().await {
            cx.update(|cx| {
                log::info!("a launch was forwarded here: {:?} in {}", request.args, request.cwd.display());
                if quit::is_quitting(cx) {
                    reply.stderr("vsesvit: Vsesvit is shutting down; start it again in a moment");
                    reply.finish(1);
                    return;
                }
                let targets = match super::cli::parse(request.args.iter().map(Into::into)) {
                    super::cli::Launch::Run(args) => args.urls,
                    super::cli::Launch::Headless(_) => Vec::new(),
                };
                let invocation = Invocation { targets, cwd: request.cwd, startup_id: request.startup_id };
                let (_, errors) = state.update(cx, |state, cx| state.handle_invocation(invocation, cx));
                for error in &errors {
                    reply.stderr(error);
                }
                reply.finish(u8::from(!errors.is_empty()));
            });
        }
    })
    .detach();
}

