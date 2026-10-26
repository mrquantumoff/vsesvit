//! Owned by W1.7.
//! The window's actions (UA section 1.9): what the keymap's chords, the toolbar's buttons and the
//! routed keys of a page run in a browser window. Wave 1 runs the tab, navigation and window commands;
//! the rest are their features' own (find, zoom, print, bookmarks, the library windows) and do nothing
//! here until those land.

use gpui::{Context, InteractiveElement as _, Stateful, Div, Window};
use vsesvit_core::private::Browsing;
use vsesvit_engine::InitialPage;

use super::BrowserWindow;
use crate::app::actions::*;
use crate::app::quit::{self, ExitIntent};
use crate::tab::model::Insert;

/// What a tab's context menu does to it (`crate::app::actions::TabOp`). W2's tab menu adds the variants.
#[derive(Clone, Debug, PartialEq)]
pub enum TabMenuOp {}

/// What a group's menu or editor does to it (`crate::app::actions::GroupOp`). W2's group editor adds the
/// variants.
#[derive(Clone, Debug, PartialEq)]
pub enum GroupMenuOp {}

/// The window's root element with every wave-1 action bound.
pub(super) fn register(root: Stateful<Div>, cx: &mut Context<BrowserWindow>) -> Stateful<Div> {
    root.on_action(cx.listener(|this, _: &NewTab, window, cx| this.open_new_tab(window, cx)))
        .on_action(cx.listener(|this, _: &CloseTab, _, cx| {
            if let Some(tab) = this.tabs.read(cx).selected() {
                this.close_tab(tab, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &ReopenClosedTab, _, cx| this.reopen_closed_tab(cx)))
        .on_action(cx.listener(|this, _: &NextTab, _, cx| this.step_tab(false, cx)))
        .on_action(cx.listener(|this, _: &PreviousTab, _, cx| this.step_tab(true, cx)))
        .on_action(cx.listener(|this, _: &SelectTab1, _, cx| this.select_nth(0, cx)))
        .on_action(cx.listener(|this, _: &SelectTab2, _, cx| this.select_nth(1, cx)))
        .on_action(cx.listener(|this, _: &SelectTab3, _, cx| this.select_nth(2, cx)))
        .on_action(cx.listener(|this, _: &SelectTab4, _, cx| this.select_nth(3, cx)))
        .on_action(cx.listener(|this, _: &SelectTab5, _, cx| this.select_nth(4, cx)))
        .on_action(cx.listener(|this, _: &SelectTab6, _, cx| this.select_nth(5, cx)))
        .on_action(cx.listener(|this, _: &SelectTab7, _, cx| this.select_nth(6, cx)))
        .on_action(cx.listener(|this, _: &SelectTab8, _, cx| this.select_nth(7, cx)))
        .on_action(cx.listener(|this, _: &SelectLastTab, _, cx| {
            let last = this.tabs.read(cx).tabs().len().checked_sub(1);
            if let Some(last) = last {
                this.select_nth(last, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &MoveTabLeft, _, cx| this.move_selected(-1, cx)))
        .on_action(cx.listener(|this, _: &MoveTabRight, _, cx| this.move_selected(1, cx)))
        .on_action(cx.listener(|this, _: &ToggleTabList, window, cx| {
            this.layout.toggle_sidebar(window.viewport_size().width);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &NewWindow, _, cx| this.open_window(Browsing::Normal, cx)))
        .on_action(cx.listener(|this, _: &NewPrivateWindow, _, cx| this.open_window(Browsing::Private, cx)))
        .on_action(cx.listener(|_, _: &Quit, _, cx| cx.defer(|cx| quit::request(ExitIntent::Quit, cx))))
        .on_action(cx.listener(|this, _: &FocusAddress, window, cx| this.address_takes_keyboard(window, cx)))
        .on_action(cx.listener(|this, _: &Back, _, cx| this.with_selected(cx, |tab, cx| tab.go_back(cx))))
        .on_action(cx.listener(|this, _: &Forward, _, cx| this.with_selected(cx, |tab, cx| tab.go_forward(cx))))
        .on_action(cx.listener(|this, _: &Reload, _, cx| this.with_selected(cx, |tab, cx| tab.reload(false, cx))))
        .on_action(cx.listener(|this, _: &ReloadBypassCache, _, cx| this.with_selected(cx, |tab, cx| tab.reload(true, cx))))
        .on_action(cx.listener(|this, _: &Stop, _, cx| this.with_selected(cx, |tab, cx| tab.stop(cx))))
}

impl BrowserWindow {
    /// A new tab at the end, selected, showing the new tab page with the address field focused.
    pub(super) fn open_new_tab(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.state.upgrade() else { return };
        let (id, browsing) = (self.id, self.browsing);
        state.update(cx, |state, cx| {
            let tab = state.new_tab_entity(browsing, InitialPage::Blank, None, cx);
            tab.update(cx, |tab, cx| tab.load_new_tab_page(cx));
            state.attach_tab(id, &tab, Insert::End, true, cx);
        });
    }

    fn reopen_closed_tab(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.state.upgrade() else { return };
        let (id, browsing) = (self.id, self.browsing);
        state.update(cx, |state, cx| {
            let Some(closed) = state.take_closed(browsing, None) else { return };
            let Ok(url) = vsesvit_core::Url::parse(&closed.url) else { return };
            let tab = state.new_tab_entity(browsing, InitialPage::Url(url), None, cx);
            state.attach_tab(id, &tab, Insert::End, true, cx);
        });
    }

    fn step_tab(&mut self, back: bool, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.read(cx).next_shown(back) {
            self.tabs.update(cx, |model, cx| model.select(tab, cx));
        }
    }

    fn select_nth(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(&tab) = self.tabs.read(cx).tabs().get(index) {
            self.tabs.update(cx, |model, cx| model.select(tab, cx));
        }
    }

    fn move_selected(&mut self, by: isize, cx: &mut Context<Self>) {
        let model = self.tabs.read(cx);
        let Some(tab) = model.selected() else { return };
        let Some(at) = model.index_of(tab) else { return };
        let Some(to) = at.checked_add_signed(by) else { return };
        self.tabs.update(cx, |model, cx| model.move_to(tab, to, cx));
    }

    fn open_window(&mut self, browsing: Browsing, cx: &mut Context<Self>) {
        if let Some(state) = self.state.upgrade() {
            cx.defer(move |cx| {
                state.update(cx, |state, cx| state.open_window(browsing, Vec::new(), cx));
            });
        }
    }

    fn with_selected(&mut self, cx: &mut Context<Self>, run: impl FnOnce(&mut crate::tab::Tab, &mut Context<crate::tab::Tab>)) {
        if let Some(tab) = self.selected_tab(cx) {
            tab.update(cx, run);
        }
    }
}

