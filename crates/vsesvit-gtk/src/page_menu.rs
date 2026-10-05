//! The page's context menu. With text selected, Chrome's item for it follows Copy: it
//! searches the default engine for the text, or goes to it when it is an address, in a new tab
//! next to the page. On the page itself, Print and View Page Source come before Inspect Element.
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
            } else if is_page(hit) {
                add_page_items(&tab, menu);
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

/// A click on the page itself, not on a link, an image, media, a field or a scrollbar.
fn is_page(hit: &webkit::HitTestResult) -> bool {
    !(hit.context_is_link()
        || hit.context_is_image()
        || hit.context_is_media()
        || hit.context_is_editable()
        || hit.context_is_scrollbar()
        || hit.context_is_selection())
}

/// Adds Print and View Page Source for `tab` right before Inspect Element, or last when the
/// menu has no Inspect Element. View Page Source is disabled for a page with no source.
pub(crate) fn add_page_items(tab: &Tab, menu: &webkit::ContextMenu) -> [webkit::ContextMenuItem; 2] {
    let print = gio::SimpleAction::new("print", None);
    print.connect_activate(glib::clone!(
        #[weak]
        tab,
        move |_, _| tab.print()
    ));
    let view_source = gio::SimpleAction::new("view-source", None);
    view_source.set_enabled(tab.source_url().is_some());
    view_source.connect_activate(glib::clone!(
        #[weak]
        tab,
        move |_, _| tab.view_source()
    ));
    let items = [
        webkit::ContextMenuItem::from_gaction(&print, "_Print…", None),
        webkit::ContextMenuItem::from_gaction(&view_source, "View Page _Source", None),
    ];
    let inspect = menu.items().iter().position(|item| item.stock_action() == webkit::ContextMenuAction::InspectElement);
    let inspect = inspect.and_then(|at| i32::try_from(at).ok());
    for (item, offset) in items.iter().zip(0..) {
        match inspect {
            Some(at) => menu.insert(item, at + offset),
            None => menu.append(item),
        }
    }
    items
}
