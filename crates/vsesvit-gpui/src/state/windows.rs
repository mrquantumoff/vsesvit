//! Owned by W1.5.
//! The window registry: each browser window's kind, tab list and geometry, most recently activated
//! first. The GPUI window itself is W1.7's: `open_window` makes the entry and asks for the window
//! with `BrowserStateEvent::OpenWindow`, and the window reports back through `window_attached`,
//! `window_activated`, `set_window_geometry` and `window_closed`.

use gpui::{AnyWindowHandle, AppContext, Context, Entity, Subscription};
use vsesvit_core::Url;
use vsesvit_core::private::Browsing;
use vsesvit_engine::{InitialPage, TabId};

use super::{BrowserState, BrowserStateEvent, CloseReason};
use crate::tab::Tab;
use crate::tab::model::{Insert, TabListEvent, TabListModel};

/// The shell's id for a browser window, stable for the window's life.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u32);

/// What the session restores a window's frame from.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowGeometry {
    pub bounds: Option<(i32, i32, u32, u32)>,
    pub maximized: bool,
}

pub struct WindowEntry {
    pub id: WindowId,
    pub browsing: Browsing,
    pub tabs: Entity<TabListModel>,
    /// Its GPUI window, once W1.7 opened it.
    pub handle: Option<AnyWindowHandle>,
    pub geometry: WindowGeometry,
    /// Its tabs are closing with it; the session no longer counts it.
    pub closing: bool,
    _list: Subscription,
}

impl BrowserState {
    /// Most recently activated first.
    pub fn windows(&self) -> &[WindowEntry] {
        &self.windows
    }

    pub fn window(&self, id: WindowId) -> Option<&WindowEntry> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn windows_of(&self, browsing: Browsing) -> Vec<WindowId> {
        self.windows.iter().filter(|w| w.browsing == browsing && !w.closing).map(|w| w.id).collect()
    }

    pub fn window_of(&self, tab: TabId) -> Option<WindowId> {
        self.tabs.get(&tab)?.window
    }

    /// A window of `browsing`'s kind with a tab for each target, the first one selected, or the
    /// new tab page.
    pub fn open_window(&mut self, browsing: Browsing, targets: Vec<Url>, cx: &mut Context<Self>) -> WindowId {
        let window = self.add_window(browsing, WindowGeometry::default(), cx);
        if targets.is_empty() {
            let tab = self.new_tab_entity(browsing, InitialPage::Blank, None, cx);
            tab.update(cx, |tab, cx| tab.load_new_tab_page(cx));
            self.attach_tab(window, &tab, Insert::End, true, cx);
        }
        for (i, url) in targets.into_iter().enumerate() {
            let tab = self.new_tab_entity(browsing, InitialPage::Url(url), None, cx);
            self.attach_tab(window, &tab, Insert::End, i == 0, cx);
        }
        cx.emit(BrowserStateEvent::OpenWindow(window));
        window
    }

    /// The most recently activated normal window, else a new one: where what comes from outside
    /// any window (another launch, an extension, sync) opens, never among private tabs.
    pub fn normal_window(&mut self, cx: &mut Context<Self>) -> WindowId {
        match self.windows_of(Browsing::Normal).first() {
            Some(&window) => window,
            None => self.open_window(Browsing::Normal, Vec::new(), cx),
        }
    }

    pub fn window_attached(&mut self, id: WindowId, handle: AnyWindowHandle) {
        if let Some(window) = self.windows.iter_mut().find(|w| w.id == id) {
            window.handle = Some(handle);
        }
    }

    /// The window came to the front: it goes first, and its selected tab counts as used.
    pub fn window_activated(&mut self, id: WindowId, cx: &mut Context<Self>) {
        let Some(at) = self.windows.iter().position(|w| w.id == id) else { return };
        let window = self.windows.remove(at);
        let selected = window.tabs.read(cx).selected();
        self.windows.insert(0, window);
        if let Some(tab) = selected {
            self.tab_used(tab, cx);
        }
        self.schedule_session_save(cx);
    }

    pub fn set_window_geometry(&mut self, id: WindowId, geometry: WindowGeometry, cx: &mut Context<Self>) {
        if let Some(window) = self.windows.iter_mut().find(|w| w.id == id)
            && window.geometry != geometry
        {
            window.geometry = geometry;
            self.schedule_session_save(cx);
        }
    }

    /// The GPUI window is gone. W1.7 closes or parks its tabs first; any still open close now, as
    /// with their window, and end in the registry when their browsers have closed.
    pub fn window_closed(&mut self, id: WindowId, cx: &mut Context<Self>) {
        let Some(window) = self.window(id) else { return };
        let left: Vec<TabId> = window.tabs.read(cx).tabs().to_vec();
        for tab in left {
            self.close_tab(tab, CloseReason::Window, None, cx);
        }
        self.windows.retain(|w| w.id != id);
        self.schedule_session_save(cx);
    }

    pub(super) fn add_window(&mut self, browsing: Browsing, geometry: WindowGeometry, cx: &mut Context<Self>) -> WindowId {
        self.next_window += 1;
        let id = WindowId(self.next_window);
        let tabs = cx.new(|_| TabListModel::default());
        let list = cx.subscribe(&tabs, |state, _, event, cx| {
            if let TabListEvent::Selected { new: Some(tab), .. } = event {
                state.tab_used(*tab, cx);
            }
            state.schedule_session_save(cx);
        });
        self.windows.insert(0, WindowEntry { id, browsing, tabs, handle: None, geometry, closing: false, _list: list });
        id
    }

    /// Puts a registered tab into `window`'s list.
    pub fn attach_tab(&mut self, window: WindowId, tab: &Entity<Tab>, at: Insert, select: bool, cx: &mut Context<Self>) {
        let id = tab.read(cx).id();
        let Some(model) = self.window(window).map(|w| w.tabs.clone()) else { return };
        if let Some(entry) = self.tabs.get_mut(&id) {
            entry.window = Some(window);
        }
        model.update(cx, |model, cx| model.insert(id, at, select, cx));
    }
}
