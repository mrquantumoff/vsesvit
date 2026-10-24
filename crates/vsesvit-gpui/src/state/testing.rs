//! Owned by W1.5.
//! A browser state over a scratch profile and `FakeEngine`, for `TestAppContext` tests. `route`
//! stands in for the engine bridge's router: it hands each queued tab event to its tab.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{Entity, TestAppContext};
use vsesvit_core::{OpenOptions, Profile};
use vsesvit_engine::{BrowserEvent, FakeEngine, NativeHandle, PhysRect, TabId};

use super::{BrowserState, WindowId};
use crate::app::shell::{Core, ProfileLocation};
use crate::tab::Tab;

pub const HIDDEN_HOST: NativeHandle = NativeHandle::from_raw(0x4d1d);
pub const CONTENT_HOST: NativeHandle = NativeHandle::from_raw(0xc0de);

pub struct Harness {
    pub state: Entity<BrowserState>,
    pub engine: Rc<FakeEngine>,
    pub core: Core,
    pub root: PathBuf,
    events: RefCell<UnboundedReceiver<BrowserEvent>>,
}

/// An empty profile directory under the temp dir, one per test name; the next run empties it.
pub fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vsesvit-gpui-test-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

impl Harness {
    pub fn new(cx: &mut TestAppContext, name: &str) -> Harness {
        Harness::at(cx, scratch_dir(name))
    }

    /// Over the profile at `root`, which may hold an earlier run's session.
    pub fn at(cx: &mut TestAppContext, root: PathBuf) -> Harness {
        let profile = Profile::open(&root, OpenOptions::default()).expect("a scratch profile");
        let core: Core = Rc::new(RefCell::new(profile));
        let (engine, events) = FakeEngine::new();
        let location = ProfileLocation { root: root.clone(), home: None };
        let state = cx.update(|cx| crate::app::init(cx, core.clone(), engine.clone(), location, false, HIDDEN_HOST));
        Harness { state, engine, core, root, events: RefCell::new(events) }
    }

    /// Delivers every queued tab event to its tab, until none is left.
    pub fn route(&self, cx: &mut TestAppContext) {
        loop {
            cx.run_until_parked();
            let mut delivered = false;
            while let Ok(event) = self.events.borrow_mut().try_recv() {
                delivered = true;
                if let BrowserEvent::Tab(id, event) = event
                    && let Some(tab) = self.tab(cx, id)
                {
                    tab.update(cx, |tab, cx| tab.apply(event, cx));
                }
            }
            if !delivered {
                return;
            }
        }
    }

    pub fn tab(&self, cx: &mut TestAppContext, id: TabId) -> Option<Entity<Tab>> {
        self.state.read_with(cx, |state, _| state.tab(id))
    }

    pub fn tabs_of(&self, cx: &mut TestAppContext, window: WindowId) -> Vec<TabId> {
        self.state.read_with(cx, |state, cx| state.window(window).map(|w| w.tabs.read(cx).tabs().to_vec()).unwrap_or_default())
    }

    /// What W1.7's window does once it can place pages: a browser for each of its tabs.
    pub fn create_browsers(&self, cx: &mut TestAppContext, window: WindowId) {
        for id in self.tabs_of(cx, window) {
            let tab = self.tab(cx, id).expect("a registered tab");
            tab.update(cx, |tab, cx| tab.create_browser(CONTENT_HOST, PhysRect::default(), 1.0, true, cx))
                .expect("the engine creates the browser");
        }
        self.route(cx);
    }

    pub fn saved_urls(&self) -> Vec<Vec<String>> {
        let saved = self.core.borrow_mut().session().restore().expect("the session reads");
        saved
            .into_iter()
            .flat_map(|s| s.windows)
            .map(|w| w.tabs.into_iter().map(|t| t.url.to_string()).collect())
            .collect()
    }
}
