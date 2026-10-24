//! Owned by W1.5.
//! The process-wide handles every view reaches through `cx` (UA section 1.4): the core profile, the
//! engine, where the profile lives, and the browser state. Set once by `app::start`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{App, Entity, Global};
use vsesvit_core::Profile;
use vsesvit_core::profiles::Home;
use vsesvit_engine::{Engine, NativeHandle};

use crate::state::BrowserState;

/// Borrowed for one call at a time, never across an engine call (design section 1.6, rule 3).
pub type Core = Rc<RefCell<Profile>>;

/// The profile's directory, and its place in the profile list when it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileLocation {
    pub root: PathBuf,
    pub home: Option<Home>,
}

pub struct Shell {
    pub core: Core,
    pub engine: Rc<dyn Engine>,
    pub location: ProfileLocation,
    /// A self-test run.
    pub scripted: bool,
    /// `Desktop`'s hidden window, where parked download browsers go (design section 4.2).
    pub hidden_host: NativeHandle,
    state: Entity<BrowserState>,
}

impl Global for Shell {}

impl Shell {
    pub fn new(
        core: Core,
        engine: Rc<dyn Engine>,
        location: ProfileLocation,
        scripted: bool,
        hidden_host: NativeHandle,
        state: Entity<BrowserState>,
    ) -> Shell {
        Shell { core, engine, location, scripted, hidden_host, state }
    }

    pub fn get(cx: &App) -> &Shell {
        cx.global::<Shell>()
    }

    pub fn state(cx: &App) -> Entity<BrowserState> {
        Shell::get(cx).state.clone()
    }

    pub fn engine(cx: &App) -> Rc<dyn Engine> {
        Shell::get(cx).engine.clone()
    }

    pub fn core(cx: &App) -> Core {
        Shell::get(cx).core.clone()
    }
}
