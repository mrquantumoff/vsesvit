//! The page's context menu. With text selected, Chrome's item for it follows Copy: it
//! searches the default engine for the text, or goes to it when it is an address, in a new tab
//! next to the page.
//!
//! WebKit hands over no selected text with the menu, so a script in a world of its own tells
//! the tab whenever the selection changes, in any frame.

use gtk::{gio, glib};
use webkit::prelude::*;

use crate::tab::Tab;
use crate::window::Focus;

const WORLD: &str = "vsesvit-shell";
const HANDLER: &str = "vsesvitSelection";
const TRACK_SELECTION: &str = "document.addEventListener('selectionchange', () => webkit.messageHandlers.vsesvitSelection.postMessage(String(getSelection())));";

/// Tracks the selection in `tab`'s pages and adds the item to its context menu.
pub(crate) fn attach(tab: &Tab) {
    let view = tab.web_view();
    if let Some(content) = view.user_content_manager() {
        content.add_script(&webkit::UserScript::for_world(
            TRACK_SELECTION,
            webkit::UserContentInjectedFrames::AllFrames,
            webkit::UserScriptInjectionTime::Start,
            WORLD,
            &[],
            &[],
        ));
        content.register_script_message_handler(HANDLER, Some(WORLD));
        content.connect_script_message_received(
            Some(HANDLER),
            glib::clone!(
                #[weak]
                tab,
                move |_, value| tab.set_selection(value.to_str().into())
            ),
        );
    }
    view.connect_context_menu(glib::clone!(
        #[weak]
        tab,
        #[upgrade_or]
        false,
        move |_, menu, hit| {
            if hit.context_is_selection() {
                add_selection_item(&tab, menu);
            }
            false
        }
    ));
}

/// Adds the item for `tab`'s selected text right after Copy, or last when the menu has no
/// Copy. None when the selection is blank.
pub(crate) fn add_selection_item(tab: &Tab, menu: &webkit::ContextMenu) -> Option<webkit::ContextMenuItem> {
    let window = tab.window()?;
    let found = window.browser().core().borrow_mut().omnibox().for_selection(&tab.selection());
    let action = found.unwrap_or_else(|e| {
        log::warn!("selection search: {e}");
        None
    })?;
    let open = gio::SimpleAction::new("open-selection", None);
    let url = action.url.to_string();
    open.connect_activate(glib::clone!(
        #[weak]
        tab,
        move |_, _| {
            if let Some(window) = tab.window() {
                window.open_tab(Some(&url), Some(&tab), Focus::Foreground);
            }
        }
    ));
    let item = webkit::ContextMenuItem::from_gaction(&open, &action.label, None);
    let copy = menu.items().iter().position(|item| item.stock_action() == webkit::ContextMenuAction::Copy);
    match copy.and_then(|copy| i32::try_from(copy + 1).ok()) {
        Some(after_copy) => menu.insert(&item, after_copy),
        None => menu.append(&item),
    }
    Some(item)
}
