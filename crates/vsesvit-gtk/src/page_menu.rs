//! The page's context menu. With text selected, Chrome's item for it follows Copy: it
//! searches the default engine for the text, or goes to it when it is an address, in a new tab
//! next to the page. On a link, Copy Link Without Tracking follows Copy Link Address. On the
//! page itself, Print and View Page Source come before Inspect Element. Extensions' items
//! (`chrome.contextMenus`) come last, before Inspect Element.
//!
//! WebKit hands over no selected text with the menu and does not say which frame it is for, so
//! a script in a world of its own tells the tab whenever the selection changes, in any frame,
//! and which frame's document a context menu opens on.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::{gio, glib};
use vsesvit_webext::menus::{Media, Target};
use webkit::prelude::*;

use crate::extension_menus::{self, Choose};
use crate::tab::Tab;
use crate::window::Focus;

const WORLD: &str = "vsesvit-shell";
const HANDLER: &str = "vsesvitSelection";
const CLICK_HANDLER: &str = "vsesvitContextMenu";
const TRACK: &str = "document.addEventListener('selectionchange', () => webkit.messageHandlers.vsesvitSelection.postMessage(String(getSelection())));\n\
    addEventListener('contextmenu', (e) => { const media = e.target instanceof Element ? e.target.closest('video, audio') : null; \
    webkit.messageHandlers.vsesvitContextMenu.postMessage([location.href, window === top, media ? media.localName : '']); }, true);";

/// The document a context menu opens on, as its frame reports it: the URL, whether it is the
/// top one, and the tag of the video or audio element clicked, if any.
type Clicked = (String, bool, String);

/// Tracks the selection and the frame clicked in `tab`'s pages, and adds the items to its
/// context menu.
pub(crate) fn attach(tab: &Tab) {
    let view = tab.web_view();
    let clicked: Rc<RefCell<Option<Clicked>>> = Rc::default();
    if let Some(content) = view.user_content_manager() {
        content.add_script(&webkit::UserScript::for_world(
            TRACK,
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
        content.register_script_message_handler(CLICK_HANDLER, Some(WORLD));
        let report = clicked.clone();
        content.connect_script_message_received(Some(CLICK_HANDLER), move |_, value| {
            report.replace(value.to_json(0).and_then(|json| serde_json::from_str(&json).ok()));
        });
    }
    view.connect_context_menu(glib::clone!(
        #[weak]
        tab,
        #[upgrade_or]
        false,
        move |_, menu, hit| {
            // A report left over from a menu the page cancelled must not stand for this one.
            let clicked = clicked.take();
            if let Some(link) = hit.link_uri().filter(|_| hit.context_is_link()) {
                add_link_item(&tab, menu, &link);
            }
            if hit.context_is_selection() {
                add_selection_item(&tab, menu);
            } else if is_page(hit) {
                add_page_items(&tab, menu);
            }
            if !hit.context_is_scrollbar() {
                add_extension_items(&tab, menu, &target(&tab, hit, clicked));
            }
            false
        }
    ));
}

/// What the menu opens on, as extensions see it: the hit, the frame's report and the selection.
fn target(tab: &Tab, hit: &webkit::HitTestResult, clicked: Option<Clicked>) -> Target {
    let (frame, top, tag) = clicked.unwrap_or_else(|| (String::new(), true, String::new()));
    let media = if hit.context_is_image() {
        Some(Media::Image)
    } else if hit.context_is_media() {
        Some(if tag == "audio" { Media::Audio } else { Media::Video })
    } else {
        None
    };
    let src_url = match media {
        Some(Media::Image) => hit.image_uri(),
        Some(_) => hit.media_uri(),
        None => None,
    };
    Target {
        page_url: tab.committed_uri().unwrap_or_default(),
        frame_url: (!top).then_some(frame),
        link_url: hit.link_uri().filter(|_| hit.context_is_link()).map(String::from),
        src_url: src_url.map(String::from),
        media,
        selection: if hit.context_is_selection() { tab.selection() } else { String::new() },
        editable: hit.context_is_editable(),
    }
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

/// Adds the item that copies `link` without its tracking parameters, right after Copy Link
/// Address, or last when the menu has no such item.
pub(crate) fn add_link_item(tab: &Tab, menu: &webkit::ContextMenu, link: &str) -> webkit::ContextMenuItem {
    let copy = gio::SimpleAction::new("copy-clean-link", None);
    let clean = vsesvit_core::clean_url::clean(link);
    copy.connect_activate(glib::clone!(
        #[weak]
        tab,
        move |_, _| tab.clipboard().set_text(&clean)
    ));
    let item = webkit::ContextMenuItem::from_gaction(&copy, "Copy Link _Without Tracking", None);
    let after = menu.items().iter().position(|item| item.stock_action() == webkit::ContextMenuAction::CopyLinkToClipboard);
    match after.and_then(|at| i32::try_from(at + 1).ok()) {
        Some(at) => menu.insert(&item, at),
        None => menu.append(&item),
    }
    item
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
    insert_at(menu, &items, inspect_element(menu));
    items
}

/// Adds what extensions offer for `target`, a separator then each extension's item or submenu,
/// above Inspect Element and the separator over it, or last.
pub(crate) fn add_extension_items(tab: &Tab, menu: &webkit::ContextMenu, target: &Target) -> Vec<webkit::ContextMenuItem> {
    let runtime = tab.runtime();
    let found = runtime.page_menu(target);
    if found.is_empty() {
        return Vec::new();
    }
    let mut n = 0;
    let entries = found.into_iter().map(|(extension, entry)| {
        let (runtime, tab, target) = (runtime.clone(), tab.id(), target.clone());
        let choose: Choose = Rc::new(move |item| runtime.menu_clicked(&extension, item, Some(tab), Some(&target)));
        extension_menus::page_item(&entry, &choose, &mut n)
    });
    let items: Vec<webkit::ContextMenuItem> = std::iter::once(webkit::ContextMenuItem::new_separator()).chain(entries).collect();
    let at = inspect_element(menu).map(|at| {
        let above = at.checked_sub(1).and_then(|i| menu.item_at_position(i as u32));
        if above.is_some_and(|item| item.is_separator()) { at - 1 } else { at }
    });
    insert_at(menu, &items, at);
    items
}

fn inspect_element(menu: &webkit::ContextMenu) -> Option<i32> {
    let at = menu.items().iter().position(|item| item.stock_action() == webkit::ContextMenuAction::InspectElement)?;
    i32::try_from(at).ok()
}

/// Inserts `items`, in order, from position `at`, or appends them.
fn insert_at(menu: &webkit::ContextMenu, items: &[webkit::ContextMenuItem], at: Option<i32>) {
    for (item, offset) in items.iter().zip(0..) {
        match at {
            Some(at) => menu.insert(item, at + offset),
            None => menu.append(item),
        }
    }
}
