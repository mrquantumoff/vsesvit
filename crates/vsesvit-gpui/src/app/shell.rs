//! Owned by W1.5.
//! The process-wide handles every view reaches through `cx` (UA section 1.4): the core profile, the
//! engine, where the profile lives, and the browser state. Set once by `app::start`. Also the shell's
//! halves of the engine bridge's router ([`Routes`]) and of the shutdown sequence ([`Shutdown`]).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{App, Entity, Global};
use vsesvit_core::Profile;
use vsesvit_core::profiles::Home;
use vsesvit_engine::{Engine, NativeHandle, OpenDisposition, RoutedKey, TabEvent, TabId, Url};

use crate::app::quit::{self, ShutdownShell};
use crate::engine_bridge::ShellRoutes;
use crate::state::{BrowserState, Invocation};

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
    /// The engine as `engine_bridge::start` hands it out: it publishes the page-focus policy before
    /// every tab creation and navigation.
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

/// The shell's half of the router. Events that arrive before `app::start` set the `Shell` are
/// dropped with a log line; the pump only steps once boot has started the app.
#[derive(Default)]
pub struct Routes {
    /// Pages created for no tab of ours yet: a popup's browser exists (`TabEvent::Created`) before
    /// `PopupOpened` names its opener.
    unclaimed: RefCell<HashMap<TabId, NativeHandle>>,
}

impl ShellRoutes for Routes {
    fn tab(&self, tab: TabId, event: TabEvent, cx: &mut App) {
        let Some(entity) = cx.try_global::<Shell>().and_then(|shell| shell.state.read(cx).tab(tab)) else {
            match event {
                TabEvent::Created { native } => {
                    self.unclaimed.borrow_mut().insert(tab, native);
                }
                TabEvent::Closed | TabEvent::Detached => {
                    self.unclaimed.borrow_mut().remove(&tab);
                }
                other => log::debug!("router: tab {} is not ours: {other:?}", tab.get()),
            }
            return;
        };
        entity.update(cx, |t, cx| t.apply(event, cx));
    }

    fn popup_opened(&self, opener: TabId, tab: TabId, url: Url, disposition: OpenDisposition, cx: &mut App) {
        let native = self.unclaimed.borrow_mut().remove(&tab);
        crate::window::adopt_popup(opener, tab, url, native, disposition, cx);
    }

    fn open_url(&self, opener: TabId, url: Url, disposition: OpenDisposition, _user_gesture: bool, cx: &mut App) {
        crate::window::open_from_page(opener, url, disposition, cx);
    }

    fn shortcut(&self, tab: Option<TabId>, key: RoutedKey, cx: &mut App) {
        crate::window::run_shortcut(tab, key, cx);
    }

    /// Handled as a forwarded launch with no one to answer: CEF's singleton is the fallback behind the
    /// instance channel (UA section 1.2).
    fn app_relaunch(&self, argv: Vec<OsString>, cwd: PathBuf, cx: &mut App) {
        let Some(shell) = cx.try_global::<Shell>() else { return };
        let state = shell.state.clone();
        let args = argv.into_iter().skip(1);
        let targets = match crate::boot::cli::parse(args) {
            crate::boot::cli::Launch::Run(args) => args.urls,
            crate::boot::cli::Launch::Headless(_) => Vec::new(),
        };
        let invocation = Invocation { targets, cwd, startup_id: None };
        state.update(cx, |state, cx| {
            let (_, errors) = state.handle_invocation(invocation, cx);
            for error in errors {
                log::warn!("a relaunch through the engine: {error}");
            }
        });
    }

    fn user_browsers_closed(&self, cx: &mut App) {
        quit::user_browsers_closed(cx);
    }

    fn all_browsers_closed(&self, cx: &mut App) {
        quit::all_browsers_closed(cx);
    }
}

/// Steps 2 and 3 of the shutdown sequence for the browser state (design section 3.6).
pub struct Shutdown;

impl ShutdownShell for Shutdown {
    /// The session is saved now and stays as saved while the windows close.
    fn save_session(&self, cx: &mut App) {
        if let Some(shell) = cx.try_global::<Shell>() {
            shell.state.clone().update(cx, |state, cx| state.shut_down(cx));
        }
    }

    /// Nothing to do before the engine closes the user's browsers: each window closes itself once its
    /// last page has closed, which is as soon as GPUI can let it go without a live page under it.
    fn hide_windows(&self, _: &mut App) {}
}
