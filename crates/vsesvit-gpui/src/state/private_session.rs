//! Owned by W1.5.
//! The private session: it begins with the first private tab, which needs the engine's private
//! context, and ends with the last. Ending cancels the private downloads first, then closes the last
//! private browser, then drops the context once that browser has closed (design section 4.2), and
//! forgets the private closed tabs and in-memory data.

use gpui::{App, Context};
use vsesvit_core::private::Browsing;
use vsesvit_engine::TabId;

use super::{BrowserState, BrowserStateEvent};
use crate::app::shell::Shell;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum PrivateSession {
    #[default]
    Off,
    On,
    /// Its downloads are cancelled and its last browser is closing.
    Ending,
}

impl BrowserState {
    pub fn private_session(&self) -> PrivateSession {
        self.private
    }

    /// A private tab was registered. The context exists before its browser is created.
    pub(super) fn private_tab_added(&mut self, cx: &mut Context<Self>) {
        match self.private {
            PrivateSession::Off => {
                Shell::engine(cx).begin_private_context();
                self.private = PrivateSession::On;
            }
            // A new private tab while the last one closes keeps the context it would have dropped.
            PrivateSession::Ending => self.private = PrivateSession::On,
            PrivateSession::On => {}
        }
    }

    pub(super) fn is_last_private_tab(&self, id: TabId, cx: &App) -> bool {
        self.tabs.iter().all(|(&other, entry)| {
            other == id || {
                let tab = entry.tab.read(cx);
                tab.browsing() != Browsing::Private || tab.is_ending()
            }
        })
    }

    /// The last private tab is closing: its downloads are cancelled before its browser closes.
    pub(super) fn private_session_ending(&mut self, cx: &mut Context<Self>) {
        if self.private != PrivateSession::On {
            return;
        }
        self.private = PrivateSession::Ending;
        let end_downloads = self.keep_alive.end_private_downloads.clone();
        end_downloads(cx);
    }

    /// After a tab ended or a window closed: once no private tab is left, the context goes.
    pub(super) fn end_private_session_if_over(&mut self, cx: &mut Context<Self>) {
        if self.private == PrivateSession::Off || self.tabs.values().any(|e| e.tab.read(cx).browsing() == Browsing::Private) {
            return;
        }
        self.private_session_ending(cx);
        self.private = PrivateSession::Off;
        Shell::engine(cx).end_private_context();
        self.closed.end_private_session();
        Shell::core(cx).borrow_mut().end_private_session();
        cx.emit(BrowserStateEvent::PrivateSessionEnded);
    }
}
