//! Owned by W1.5.
//! The app inside GPUI's `run` (design section 3.1 step 10, UA section 1.2): the `Shell` global and
//! the browser state, then the process's own launch.

use std::rc::Rc;

use gpui::{App, AppContext, Entity};
use vsesvit_engine::{Engine, NativeHandle};

use crate::state::{BrowserState, BrowserStateEvent, Invocation};
use crate::window::WindowHost;

pub mod actions;
pub mod quit;
pub mod shell;

use shell::{Core, ProfileLocation, Shell};

/// What boot hands the app. Boot keeps `Desktop` alive and passes its hidden window.
pub struct StartArgs {
    pub core: Core,
    /// The engine as `engine_bridge::start` hands it out, which publishes the page-focus policy first.
    pub engine: Rc<dyn Engine>,
    pub location: ProfileLocation,
    pub scripted: bool,
    pub hidden_host: NativeHandle,
    /// The process's own command line.
    pub first: Invocation,
}

/// Sets the `Shell` global, opens browser windows as the state asks for them, binds the keys and runs
/// the process's own launch, which opens the first windows. `engine_bridge::start` has run, so the
/// first tabs' events reach the router. The engine's hidden host is boot's to set once the first
/// window has its surface (on Windows it lives in a browser window).
pub fn start(cx: &mut App, args: StartArgs, host: WindowHost) -> Entity<BrowserState> {
    let state = init(cx, args.core, args.engine, args.location, args.scripted, args.hidden_host);
    crate::window::install(&state, host, cx);
    install_keymap(cx);
    cx.subscribe(&state, |_, event, cx| {
        if *event == BrowserStateEvent::KeymapChanged {
            install_keymap(cx);
        }
    })
    .detach();
    state.update(cx, |state, cx| {
        state.profile_used(cx);
        let (_, errors) = state.handle_invocation(args.first, cx);
        for error in errors {
            eprintln!("{error}");
        }
    });
    state
}

/// The `Shell` global and the browser state, with no window yet.
pub fn init(
    cx: &mut App,
    core: Core,
    engine: Rc<dyn Engine>,
    location: ProfileLocation,
    scripted: bool,
    hidden_host: NativeHandle,
) -> Entity<BrowserState> {
    let state = cx.new(|_| BrowserState::new(location.clone()));
    cx.set_global(Shell::new(core, engine, location, scripted, hidden_host, state.clone()));
    state
}

/// Core's keymap as GPUI bindings in the browser windows and as the engine's key routing.
fn install_keymap(cx: &mut App) {
    let keymap = Shell::core(cx).borrow_mut().prefs().keymap();
    crate::engine_bridge::keymap::install(cx, &keymap);
}
