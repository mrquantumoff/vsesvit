//! Owned by W1.5.
//! `BrowserState`: the windows and tabs of the profile, the closed-tab stacks, the private session,
//! the session saver and the profile list (UA section 1.4). The submodules each add one part of its
//! behaviour; views hold it weakly, so closing everything releases the profile.

use std::collections::HashMap;

use gpui::{Context, Entity, EventEmitter, Subscription, Task};
use vsesvit_core::closed_tabs::ClosedTabs;
use vsesvit_core::profiles::Registry;
use vsesvit_engine::TabId;

use crate::app::shell::ProfileLocation;
use crate::tab::{Tab, TabIcon};

pub mod closed_tabs;
pub mod history_hooks;
pub mod invocation;
pub mod memory_saver;
pub mod prefs;
pub mod private_session;
pub mod session;
pub mod sync_applied;
pub mod tabs;
pub mod windows;

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;

pub use closed_tabs::ClosedKey;
pub use invocation::{Invocation, InvocationOutcome};
pub use tabs::{CloseReason, KeepAlive};
pub use windows::{WindowEntry, WindowGeometry, WindowId};

#[derive(Clone, Debug, PartialEq)]
pub enum BrowserStateEvent {
    BookmarksChanged,
    HistoryChanged,
    PrefsChanged(Vec<String>),
    ProfilesChanged,
    KeymapChanged,
    ThemeChanged,
    PrivateSessionEnded,
    /// A window entry exists and its GPUI window should open (W1.7).
    OpenWindow(WindowId),
    /// A window should come to the front for a forwarded launch, with that launch's startup id.
    RaiseWindow { window: WindowId, startup_id: Option<String> },
}

struct TabEntry {
    tab: Entity<Tab>,
    window: Option<WindowId>,
    reason: Option<CloseReason>,
    _changes: Subscription,
}

pub struct BrowserState {
    /// Most recently activated first.
    windows: Vec<WindowEntry>,
    tabs: HashMap<TabId, TabEntry>,
    closed: ClosedTabs<TabIcon>,
    private: private_session::PrivateSession,
    /// The use clock: tab search orders tabs by it, closed tabs are named by it.
    clock: u64,
    next_window: u32,
    session_save: Option<Task<()>>,
    /// After `shut_down`, the session stays as it was saved then.
    saving_stopped: bool,
    location: ProfileLocation,
    profiles: Registry,
    keep_alive: KeepAlive,
}

impl EventEmitter<BrowserStateEvent> for BrowserState {}

impl BrowserState {
    pub fn new(location: ProfileLocation) -> BrowserState {
        let profiles = location.home.as_ref().map(|home| home.dir.load()).unwrap_or_default();
        BrowserState {
            windows: Vec::new(),
            tabs: HashMap::new(),
            closed: ClosedTabs::default(),
            private: private_session::PrivateSession::default(),
            clock: 0,
            next_window: 0,
            session_save: None,
            saving_stopped: false,
            location,
            profiles,
            keep_alive: KeepAlive::default(),
        }
    }

    /// The use clock's next tick.
    pub fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    pub fn profiles(&self) -> &Registry {
        &self.profiles
    }

    /// The profile's name for window titles, when the profile list names it.
    pub fn profile_title(&self) -> Option<String> {
        let home = self.location.home.as_ref()?;
        self.profiles.title_name(&home.id).map(str::to_owned)
    }

    /// This run used the profile: the list's last-used profile is this one now.
    pub fn profile_used(&mut self, cx: &mut Context<Self>) {
        let Some(home) = &self.location.home else { return };
        match home.dir.opened(&home.id) {
            Ok(registry) => {
                self.profiles = registry;
                cx.emit(BrowserStateEvent::ProfilesChanged);
            }
            Err(e) => log::warn!("the profile list: {e}"),
        }
    }

    pub fn bookmarks_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(BrowserStateEvent::BookmarksChanged);
    }

    /// Quitting (design section 3.6 step 2): the session is saved now, and stays as saved while
    /// the windows close.
    pub fn shut_down(&mut self, cx: &mut Context<Self>) {
        self.save_session_now(cx);
        self.saving_stopped = true;
    }
}
