//! Owned by W1.5.
//! The app inside GPUI's `run` (design section 3.1 step 10, UA section 1.2): the `Shell` global and
//! the browser state, then the process's own launch.

use std::rc::Rc;

use gpui::{App, AppContext, Entity};
use vsesvit_engine::{Engine, NativeHandle};

use crate::state::{BrowserState, Invocation};

pub mod actions;
pub mod quit;
pub mod shell;

use shell::{Core, ProfileLocation, Shell};

/// What boot hands the app. Boot keeps `Desktop` alive and passes its hidden window.
pub struct StartArgs {
    pub core: Core,
    pub engine: Rc<dyn Engine>,
    pub location: ProfileLocation,
    pub scripted: bool,
    pub hidden_host: NativeHandle,
    /// The process's own command line.
    pub first: Invocation,
}

/// Sets the `Shell` global and runs the process's own launch. W1.6's `engine_bridge::start` belongs
/// between `init` and that launch, so the first tabs' events reach the router.
pub fn start(cx: &mut App, args: StartArgs) -> Entity<BrowserState> {
    let state = init(cx, args.core, args.engine, args.location, args.scripted, args.hidden_host);
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
    engine.set_hidden_host(hidden_host);
    let state = cx.new(|_| BrowserState::new(location.clone()));
    cx.set_global(Shell::new(core, engine, location, scripted, hidden_host, state.clone()));
    state
}
