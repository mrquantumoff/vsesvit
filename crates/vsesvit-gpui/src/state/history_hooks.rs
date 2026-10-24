//! Owned by W1.5.
//! What a tab's navigation writes to history. Every main-frame commit of a normal tab records a
//! visit, the engine's error pages aside. Titles arrive after the commit and go under the committed
//! address, unless the page is an error page, whose title is ours, or the title is empty. A private
//! tab writes nothing.

use gpui::{Context, Entity};
use vsesvit_core::private::Browsing;
use vsesvit_engine::CommitKind;

use super::{BrowserState, BrowserStateEvent};
use crate::app::shell::Shell;
use crate::tab::Tab;

impl BrowserState {
    pub(super) fn record_visit(&mut self, tab: &Entity<Tab>, cx: &mut Context<Self>) {
        let (browsing, visit) = tab.update(cx, |tab, _| (tab.browsing(), tab.take_visit()));
        let Some(visit) = visit.filter(|v| v.kind != CommitKind::ErrorPage && browsing == Browsing::Normal) else {
            return;
        };
        let recorded = Shell::core(cx).borrow_mut().history().record_visit(&visit.url, visit.transition);
        match recorded {
            Ok(()) => cx.emit(BrowserStateEvent::HistoryChanged),
            Err(e) => log::warn!("history: {e}"),
        }
    }

    pub(super) fn record_title(&mut self, tab: &Entity<Tab>, cx: &mut Context<Self>) {
        let tab = tab.read(cx);
        if tab.browsing() != Browsing::Normal || tab.shows_error_page() || tab.page_title().trim().is_empty() {
            return;
        }
        let Some(url) = tab.committed_url().cloned() else { return };
        let title = tab.page_title().to_owned();
        let set = Shell::core(cx).borrow_mut().history().set_title(&url, &title);
        match set {
            Ok(()) => cx.emit(BrowserStateEvent::HistoryChanged),
            Err(e) => log::warn!("history title: {e}"),
        }
    }
}
