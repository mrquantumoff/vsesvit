//! Owned by W1.5.
//! The session: the normal windows and their tabs as core's session record, saved about two seconds
//! after the last change and at quit, and opened again at startup (UA section 2.2). Private windows
//! are never saved, and nothing is written while no normal window is left, so quitting or closing
//! the last normal window never overwrites the last real session with an empty one.

use std::time::Duration;

use gpui::{App, Context};
use vsesvit_core::Url;
use vsesvit_core::prefs::{self, Startup, keys};
use vsesvit_core::private::Browsing;
use vsesvit_core::session::plan::{self, TabPlan, WindowPlan};
use vsesvit_core::session::{SessionSnapshot, WindowSnapshot};
use vsesvit_engine::{InitialPage, TabId};

use super::{BrowserState, WindowEntry, WindowGeometry, WindowId};
use crate::app::shell::Shell;
use crate::tab::model::Insert;

/// How long the session waits after a change before it is written.
pub const SAVE_DELAY: Duration = Duration::from_secs(2);

impl BrowserState {
    /// Saves the session `SAVE_DELAY` after this call, unless a save is already waiting.
    pub fn schedule_session_save(&mut self, cx: &mut Context<Self>) {
        if self.session_save.is_some() || self.saving_stopped {
            return;
        }
        self.session_save = Some(cx.spawn(async move |state, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            state
                .update(cx, |state, cx| {
                    state.session_save = None;
                    state.save_session_now(cx);
                })
                .ok();
        }));
    }

    /// Writes the session now, with the normal windows that are not closing.
    pub fn save_session_now(&mut self, cx: &mut Context<Self>) {
        self.session_save = None;
        if self.saving_stopped || self.windows_of(Browsing::Normal).is_empty() {
            return;
        }
        let snapshot = self.snapshot(cx);
        if let Err(e) = Shell::core(cx).borrow_mut().session().save(&snapshot) {
            log::warn!("cannot save the session: {e}");
        }
    }

    /// The normal windows with a tab to save, most recently activated first, so the active window
    /// is index 0.
    pub fn snapshot(&self, cx: &App) -> SessionSnapshot {
        let windows = self
            .windows
            .iter()
            .filter(|w| w.browsing == Browsing::Normal && !w.closing)
            .filter_map(|w| self.window_snapshot(w, cx))
            .collect();
        SessionSnapshot { device_name: device_name(cx), windows, active_window: 0 }
    }

    fn window_snapshot(&self, window: &WindowEntry, cx: &App) -> Option<WindowSnapshot> {
        let model = window.tabs.read(cx);
        let mut active = None;
        let mut tabs = Vec::new();
        for (index, &id) in model.tabs().iter().enumerate() {
            let Some(tab) = self.tab(id) else { continue };
            let tab = tab.read(cx);
            let Some(url) = tab.session_url() else { continue };
            if model.selected() == Some(id) {
                active = Some(tabs.len());
            }
            let group = model.group_of(id).cloned();
            let title = tab.display_title();
            let pinned = index < model.pinned_count();
            tabs.push(plan::tab_snapshot(tab.session_id(), url.as_str(), &title, pinned, tab.last_active_ms(), group));
        }
        plan::window_snapshot(tabs, active, window.geometry.bounds, window.geometry.maximized)
    }

    /// The first windows of a run: the previous session, the homepage or the new tab page, as the
    /// startup preference says, with `targets` from the command line as tabs of the first window.
    pub fn open_startup_windows(&mut self, targets: Vec<Url>, cx: &mut Context<Self>) {
        let core = Shell::core(cx);
        let (startup, homepage) = {
            let mut core = core.borrow_mut();
            let mut prefs = core.prefs();
            (prefs.get(&keys::STARTUP), prefs::homepage_url(&prefs.get(&keys::HOMEPAGE)))
        };
        let restored = match startup {
            Startup::RestoreSession => match core.borrow_mut().session().restore() {
                Ok(snapshot) => snapshot.map(active_window_first),
                Err(e) => {
                    log::warn!("cannot restore the session: {e}");
                    None
                }
            },
            Startup::Homepage | Startup::NewTab => None,
        };
        let urls = targets.iter().map(Url::to_string).collect();
        let plans = plan::startup_plan(startup, restored, homepage.map(String::from), urls);
        // The active window opens last, so it ends first among the windows and on top.
        for window in plans.into_iter().rev() {
            self.open_planned_window(window, cx);
        }
    }

    fn open_planned_window(&mut self, plan: WindowPlan, cx: &mut Context<Self>) -> WindowId {
        let geometry = WindowGeometry { bounds: plan.bounds, maximized: plan.maximized };
        let window = self.add_window(Browsing::Normal, geometry, cx);
        let mut groups = Vec::with_capacity(plan.tabs.len());
        let mut selected = None;
        for (index, tab_plan) in plan.tabs.into_iter().enumerate() {
            let id = self.open_planned_tab(window, tab_plan.clone(), cx);
            groups.push((id, tab_plan.group));
            if index == plan.active {
                selected = Some(id);
            }
        }
        if let Some(model) = self.window(window).map(|w| w.tabs.clone()) {
            model.update(cx, |model, cx| {
                if let Some(tab) = selected {
                    model.select(tab, cx);
                }
                // After the selection, which expands the selected tab's group.
                model.restore_groups(groups, cx);
            });
        }
        cx.emit(super::BrowserStateEvent::OpenWindow(window));
        window
    }

    fn open_planned_tab(&mut self, window: WindowId, plan: TabPlan, cx: &mut Context<Self>) -> TabId {
        let url = plan.url.as_deref().and_then(|u| Url::parse(u).ok());
        let page = url.map_or(InitialPage::Blank, InitialPage::Url);
        let tab = self.new_tab_entity(Browsing::Normal, page, None, cx);
        let fresh = plan.id.is_none() && plan.url.is_none();
        tab.update(cx, |tab, cx| {
            if let Some(id) = plan.id {
                tab.restored(id, plan.title);
            }
            if fresh {
                tab.load_new_tab_page(cx);
            }
        });
        let pinned = self.window(window).map_or(0, |w| w.tabs.read(cx).pinned_count());
        let at = if plan.pinned { Insert::Pinned(pinned) } else { Insert::End };
        self.attach_tab(window, &tab, at, false, cx);
        tab.read(cx).id()
    }
}

/// The record's active window first, where the snapshot of a save puts it too.
fn active_window_first(mut snapshot: SessionSnapshot) -> SessionSnapshot {
    if snapshot.active_window < snapshot.windows.len() {
        let active = snapshot.windows.remove(snapshot.active_window);
        snapshot.windows.insert(0, active);
    }
    snapshot.active_window = 0;
    snapshot
}

fn device_name(cx: &App) -> String {
    let configured = Shell::core(cx).borrow_mut().prefs().get(&keys::DEVICE_NAME);
    if configured.trim().is_empty() { host_name() } else { configured }
}

fn host_name() -> String {
    let var = if cfg!(windows) { "COMPUTERNAME" } else { "HOSTNAME" };
    std::env::var(var)
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Vsesvit".to_owned())
}
