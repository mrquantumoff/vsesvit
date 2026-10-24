//! Owned by W1.6.
//! The bridge between GPUI and the engine (UA section 4.3, design sections 1.5, 1.6 and 3.4): the
//! pump, the router, prompts, requests, hooks, keys and the page-focus policy.
//!
//! `start` takes the engine's parts and hands back the engine the rest of the shell holds, which
//! publishes the page-focus policy before every tab creation and navigation, and the pump's handle,
//! which shutdown step 7 stops to get the driver back.

use std::rc::Rc;

use gpui::{App, Global};
use vsesvit_engine::{Engine, EngineParts};

use crate::platform::Desktop;

pub mod focus;
mod guarded;
pub mod hooks;
pub mod keymap;
pub mod prompts;
pub mod pump;
pub mod requests;
pub mod router;

#[cfg(test)]
mod tests;

pub use focus::{Focus, PageArea, SharedSurface};
pub use pump::PumpHandle;
pub use router::ShellRoutes;

/// What `start` hands back.
pub struct Started {
    /// The engine for the rest of the shell (the `Shell`'s `engine`).
    pub engine: Rc<dyn Engine>,
    /// The pump, kept by the `Shell` until shutdown step 7.
    pub pump: PumpHandle,
}

struct Bridge {
    engine: Rc<dyn Engine>,
    focus: Focus,
    routes: Rc<dyn ShellRoutes>,
    hooks: hooks::SharedSnapshot,
    requests: requests::Pending,
    prompts: prompts::Open,
}

impl Global for Bridge {}

/// Starts the bridge: installs the hooks, then the pump, the router and the pointer-press watch as
/// GPUI foreground tasks. Once per process, on main, before the first window.
pub fn start<D: Desktop>(cx: &mut App, parts: EngineParts, desktop: Rc<D>, routes: Rc<dyn ShellRoutes>) -> Started {
    let EngineParts { engine: raw, driver, events } = parts;
    let presses = desktop.pointer_presses();
    let focus = Focus::new(
        raw.clone(),
        presses,
        {
            let desktop = desktop.clone();
            move |at| desktop.toplevel_at(at)
        },
        cx.background_executor().clone(),
    );
    let engine: Rc<dyn Engine> = Rc::new(guarded::GuardedEngine::new(raw, focus.clone()));
    let snapshot = hooks::SharedSnapshot::default();
    engine.set_hooks(Rc::new(hooks::Hooks::new(snapshot.clone(), move |window| desktop.neutralize(window))));
    cx.set_global(Bridge {
        engine: engine.clone(),
        focus: focus.clone(),
        routes,
        hooks: snapshot,
        requests: requests::Pending::default(),
        prompts: prompts::Open::default(),
    });
    focus.watch_presses(cx);
    let pump = pump::start(cx, driver);
    router::start(cx, events);
    Started { engine, pump }
}

/// Whether `start` has run.
pub fn running(cx: &App) -> bool {
    cx.has_global::<Bridge>()
}

/// The engine as `start` handed it out.
pub fn engine(cx: &App) -> Rc<dyn Engine> {
    bridge(cx).engine.clone()
}

/// The page-focus policy, for the windows.
pub fn focus(cx: &App) -> Focus {
    bridge(cx).focus.clone()
}

fn bridge(cx: &App) -> &Bridge {
    hooks::assert_outside_hook();
    cx.global::<Bridge>()
}

fn bridge_mut(cx: &mut App) -> &mut Bridge {
    hooks::assert_outside_hook();
    cx.global_mut::<Bridge>()
}
