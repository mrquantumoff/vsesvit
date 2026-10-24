//! Owned by W1.3.
//! Focus (design section 1.5, the page-focus policy). Chromium focuses new browsers and navigations
//! itself; `on_set_focus` refuses every request the tab's published `PageFocus` does not allow, whatever
//! its source. Hidden, parked and Chrome-created browsers never take focus (on Windows their toplevel
//! would activate). DevTools windows are ordinary top-level windows and keep their own focus.

use std::sync::Arc;

use cef::*;
use vsesvit_engine::{PageFocus, TabEvent, TabId};

use crate::client::HandlerCtx;
use crate::handlers::display::to_tab;
use crate::registry::{self, BrowserId, BrowserKind};

/// The logic of `on_set_focus`: true refuses the focus.
pub fn refuse(browser: BrowserId) -> bool {
    registry::with(|r| match r.entries.get(&browser).map(|e| e.kind) {
        Some(BrowserKind::Tab(tab)) => !r.focus_of(tab).allows(),
        Some(BrowserKind::DevTools(_)) => false,
        _ => true,
    })
    .unwrap_or(true)
}

/// `Engine::set_page_focus`.
pub fn set_policy(tab: TabId, focus: PageFocus) {
    registry::with(|r| r.focus.insert(tab, focus));
}

/// `Engine::focus_page`.
pub fn focus_page(tab: TabId) {
    registry::on_tab(tab, |page| page.set_focus(true));
}

/// `Engine::blur_page`.
pub fn blur_page(tab: TabId) {
    if registry::on_tab(tab, |page| page.set_focus(false)).is_some()
        && let Some(sink) = registry::sink()
    {
        sink.tab(tab, TabEvent::FocusChanged { focused: false });
    }
}

wrap_focus_handler! {
    pub struct Focus {
        ctx: Arc<HandlerCtx>,
    }

    impl FocusHandler {
        fn on_take_focus(&self, browser: Option<&mut Browser>, next: i32) {
            if let Some(browser) = browser {
                to_tab(browser.identifier(), TabEvent::TakeFocus { next: next != 0 });
            }
        }

        fn on_set_focus(&self, browser: Option<&mut Browser>, _source: FocusSource) -> i32 {
            browser.map(|b| refuse(b.identifier()) as i32).unwrap_or(1)
        }

        fn on_got_focus(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                to_tab(browser.identifier(), TabEvent::FocusChanged { focused: true });
            }
        }
    }
}

impl Focus {
    /// The handler for a client.
    pub fn handler(ctx: &Arc<HandlerCtx>) -> FocusHandler {
        Focus::new(ctx.clone())
    }
}
