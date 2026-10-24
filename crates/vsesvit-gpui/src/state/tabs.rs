//! Owned by W1.5.
//! The tab registry and the tab close path. A tab ends when the engine reports its browser closed
//! or detached; only then does it leave its window's list, go on the closed-tab stack and end the
//! private session if it was the last private tab. Closing a normal tab with a running download
//! parks its page instead of closing the browser (design section 4.2).

use std::rc::Rc;

use gpui::{App, AppContext, Context, Entity};
use vsesvit_core::private::Browsing;
use vsesvit_engine::{InitialPage, NativeHandle, TabId};

use super::{BrowserState, TabEntry, WindowId};
use crate::app::shell::Shell;
use crate::downloads::keep_alive::{self, ParkDecision};
use crate::platform::SurfaceOps;
use crate::tab::{Tab, TabChanged};

/// Why a tab closes. A tab closed on its own goes on the closed-tab stack; the tabs of a closing
/// window do not.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CloseReason {
    Tab,
    Window,
}

/// Whether a closing tab's page is parked for its download.
pub type ParkDecider = Rc<dyn Fn(TabId, &App) -> ParkDecision>;

/// What the close path asks W3.3's downloads code (`downloads/keep_alive.rs`). Both run while
/// `BrowserState` is being updated, so they must not read it through `Shell::state`. Tests swap in
/// their own answers.
#[derive(Clone)]
pub struct KeepAlive {
    pub park_decision: ParkDecider,
    /// Cancels the private session's downloads, before its last browser closes.
    pub end_private_downloads: Rc<dyn Fn(&mut App)>,
}

impl Default for KeepAlive {
    fn default() -> Self {
        KeepAlive {
            park_decision: Rc::new(keep_alive::park_decision),
            end_private_downloads: Rc::new(keep_alive::end_private_downloads),
        }
    }
}

impl BrowserState {
    pub fn tab(&self, id: TabId) -> Option<Entity<Tab>> {
        self.tabs.get(&id).map(|e| e.tab.clone())
    }

    /// A new tab, registered but in no window yet: `attach_tab` puts it in one.
    pub fn new_tab_entity(
        &mut self,
        browsing: Browsing,
        page: InitialPage,
        opener: Option<TabId>,
        cx: &mut Context<Self>,
    ) -> Entity<Tab> {
        let id = TabId::next();
        let tab = cx.new(|_| Tab::new(id, browsing, page, opener));
        let changes = cx.subscribe(&tab, move |state, tab, change, cx| state.tab_changed(&tab, change, cx));
        self.tabs.insert(id, TabEntry { tab: tab.clone(), window: None, reason: None, _changes: changes });
        if browsing == Browsing::Private {
            self.private_tab_added(cx);
        }
        tab
    }

    /// The tab was opened or selected, or its window activated: tab search lists it first now.
    pub fn tab_used(&mut self, id: TabId, cx: &mut Context<Self>) {
        let tick = self.tick();
        if let Some(entry) = self.tabs.get(&id) {
            entry.tab.update(cx, |tab, _| tab.set_used(tick));
        }
    }

    /// Closes a tab. When W3.3's downloads code says its page has a running download, the page is
    /// parked in the hidden host and the tab detached, so the browser lives on for the download;
    /// `surface` is the tab's window's, which does the native reparent. Otherwise the browser
    /// closes, running the page's `beforeunload`. The tab ends when the engine reports it.
    pub fn close_tab(
        &mut self,
        id: TabId,
        reason: CloseReason,
        surface: Option<&mut dyn SurfaceOps>,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.tabs.get_mut(&id) else { return };
        let tab = entry.tab.clone();
        let (browsing, native, ending) = tab.read_with(cx, |t, _| (t.browsing(), t.native(), t.is_ending()));
        if ending {
            return;
        }
        entry.reason = Some(reason);
        if reason == CloseReason::Window {
            self.window_closing(id, cx);
        }
        if browsing == Browsing::Private && self.is_last_private_tab(id, cx) {
            self.private_session_ending(cx);
        }
        if browsing == Browsing::Normal
            && let (Some(native), Some(surface)) = (native, surface)
            && (self.keep_alive.park_decision)(id, cx) == ParkDecision::Park
            && park(id, native, surface, cx)
        {
            tab.update(cx, |tab, _| tab.parked());
            return;
        }
        if !tab.update(cx, |tab, cx| tab.close_browser(cx)) {
            self.tab_ended(id, cx);
        }
    }

    /// The page's `beforeunload` kept the tab open (W2.6 calls this when the user chooses to stay):
    /// the tab, its window and the private session are as they were before the close.
    pub fn close_refused(&mut self, id: TabId, cx: &mut Context<Self>) {
        let Some(entry) = self.tabs.get_mut(&id) else { return };
        let (tab, window, reason) = (entry.tab.clone(), entry.window, entry.reason.take());
        let browsing = tab.update(cx, |tab, _| {
            tab.close_refused();
            tab.browsing()
        });
        if reason == Some(CloseReason::Window)
            && let Some(w) = self.windows.iter_mut().find(|w| Some(w.id) == window)
        {
            w.closing = false;
        }
        if browsing == Browsing::Private && self.private == super::private_session::PrivateSession::Ending {
            self.private = super::private_session::PrivateSession::On;
        }
    }

    fn tab_changed(&mut self, tab: &Entity<Tab>, change: &TabChanged, cx: &mut Context<Self>) {
        let id = tab.read(cx).id();
        match change {
            TabChanged::EngineClosed | TabChanged::ClosedByPage => {
                if change == &TabChanged::ClosedByPage
                    && let Some(entry) = self.tabs.get_mut(&id)
                {
                    entry.reason.get_or_insert(CloseReason::Tab);
                    if tab.read(cx).browsing() == Browsing::Private && self.is_last_private_tab(id, cx) {
                        self.private_session_ending(cx);
                    }
                }
                self.tab_ended(id, cx);
            }
            TabChanged::Committed(_) => {
                self.record_visit(tab, cx);
                self.schedule_session_save(cx);
            }
            TabChanged::Title => {
                self.record_title(tab, cx);
                self.schedule_session_save(cx);
            }
            _ => {}
        }
    }

    /// The tab's browser is gone: it leaves its window and the registry. The last tab of the last
    /// normal window is saved with the session first, so the next start brings it back.
    fn tab_ended(&mut self, id: TabId, cx: &mut Context<Self>) {
        let Some(entry) = self.tabs.get(&id) else { return };
        let (tab, reason) = (entry.tab.clone(), entry.reason);
        let model = entry.window.and_then(|w| self.window(w)).map(|w| (w.id, w.tabs.clone()));
        if let Some((window, model)) = model {
            if model.read(cx).tabs() == [id] {
                if self.is_last_live_normal_window(window) {
                    self.save_session_now(cx);
                }
                self.mark_window_closing(window);
            }
            let removed = model.update(cx, |model, cx| model.remove(id, cx));
            if reason == Some(CloseReason::Tab) {
                self.tab_closed(&tab, removed.and_then(|r| r.group).map(|g| g.id), cx);
            }
        }
        self.tabs.remove(&id);
        self.end_private_session_if_over(cx);
        self.schedule_session_save(cx);
    }

    /// The first tab of a closing window: the window leaves the session now, saved first when it
    /// is the last normal one.
    fn window_closing(&mut self, tab: TabId, cx: &mut Context<Self>) {
        let Some(window) = self.window_of(tab) else { return };
        if self.window(window).is_some_and(|w| w.closing) {
            return;
        }
        if self.is_last_live_normal_window(window) {
            self.save_session_now(cx);
        }
        self.mark_window_closing(window);
    }

    fn mark_window_closing(&mut self, window: WindowId) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == window) {
            w.closing = true;
        }
    }

    fn is_last_live_normal_window(&self, window: WindowId) -> bool {
        let normal = self.windows_of(Browsing::Normal);
        normal == [window]
    }
}

/// Moves the page into the hidden host, then detaches the tab from its browser (design section 4.2).
fn park(id: TabId, native: NativeHandle, surface: &mut dyn SurfaceOps, cx: &App) -> bool {
    let shell = Shell::get(cx);
    if let Err(e) = surface.park_page(native, shell.hidden_host) {
        log::warn!("cannot park tab {}, closing it: {e}", id.get());
        return false;
    }
    match shell.engine.detach_tab(id) {
        Ok(_) => true,
        Err(e) => {
            log::warn!("cannot detach tab {}, closing it: {e}", id.get());
            false
        }
    }
}
