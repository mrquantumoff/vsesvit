//! The bookmarks bar: the toolbar folder's items as buttons, folders as menus, and "Other
//! Bookmarks" at the end when it has anything. Visibility follows the synced preference
//! through `win.show-bookmarks-bar`.

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::Url;
use vsesvit_core::bookmarks::{BookmarkId, Bookmarks, NodeKind};

use crate::profile::Core;

/// Folder nesting deeper than this is not shown in menus.
const MAX_DEPTH: usize = 12;
const MAX_LABEL_CHARS: i32 = 22;

pub(crate) const OPEN_ACTION: &str = "win.open-bookmark";

/// A bookmark subtree copied out of core, so widgets are built without a borrow held.
enum Item {
    Url { title: String, url: Url },
    Folder { title: String, children: Vec<Item> },
    Separator,
}

pub(crate) struct BookmarksBar {
    revealer: gtk::Revealer,
    row: gtk::Box,
}

impl BookmarksBar {
    pub(crate) fn new() -> Self {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(2)
            .css_classes(["toolbar", "bookmarks-bar"])
            .build();
        let revealer = gtk::Revealer::builder()
            .child(&row)
            .reveal_child(true)
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .build();
        BookmarksBar { revealer, row }
    }

    pub(crate) fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    pub(crate) fn set_revealed(&self, revealed: bool) {
        self.revealer.set_reveal_child(revealed);
    }

    /// Rebuilds the bar from the profile's toolbar and "other" folders.
    pub(crate) fn refresh(&self, core: &Core) {
        let (toolbar, other) = {
            let mut profile = core.borrow_mut();
            let bookmarks = profile.bookmarks();
            (
                collect(&bookmarks, BookmarkId::TOOLBAR, 0),
                collect(&bookmarks, BookmarkId::OTHER, 0),
            )
        };
        while let Some(child) = self.row.first_child() {
            self.row.remove(&child);
        }
        if toolbar.is_empty() && other.is_empty() {
            let empty = gtk::Label::builder()
                .label("Bookmarks you add to the bar appear here")
                .css_classes(["dim-label", "caption"])
                .margin_start(6)
                .build();
            self.row.append(&empty);
            return;
        }
        for item in &toolbar {
            self.row.append(&item_widget(item));
        }
        if !other.is_empty() {
            let spacer = gtk::Box::builder().hexpand(true).build();
            self.row.append(&spacer);
            self.row.append(&folder_button("Other Bookmarks", &other));
        }
    }

    /// Whether the bar has a button for `url` (used by the self-test).
    pub(crate) fn shows(&self, url: &str) -> bool {
        let mut child = self.row.first_child();
        while let Some(widget) = child {
            if widget.is::<gtk::Button>() && widget.tooltip_text().as_deref() == Some(url) {
                return true;
            }
            child = widget.next_sibling();
        }
        false
    }
}

fn collect(bookmarks: &Bookmarks<'_>, folder: BookmarkId, depth: usize) -> Vec<Item> {
    if depth > MAX_DEPTH {
        return Vec::new();
    }
    bookmarks
        .children(folder)
        .into_iter()
        .filter_map(|node| match (node.kind, node.url) {
            (NodeKind::Url, Some(url)) => Some(Item::Url { title: node.title, url }),
            (NodeKind::Url, None) => None,
            (NodeKind::Folder, _) => Some(Item::Folder {
                title: node.title,
                children: collect(bookmarks, node.id, depth + 1),
            }),
            (NodeKind::Separator, _) => Some(Item::Separator),
        })
        .collect()
}

fn item_widget(item: &Item) -> gtk::Widget {
    match item {
        Item::Url { title, url } => url_button(title, url).upcast(),
        Item::Folder { title, children } => folder_button(title, children).upcast(),
        Item::Separator => gtk::Separator::new(gtk::Orientation::Vertical).upcast(),
    }
}

fn url_button(title: &str, url: &Url) -> gtk::Button {
    let button = gtk::Button::builder()
        .child(&labelled("web-browser-symbolic", label_for(title, url)))
        .tooltip_text(url.as_str())
        .css_classes(["flat"])
        .build();
    button.set_action_name(Some(OPEN_ACTION));
    button.set_action_target_value(Some(&url.as_str().to_variant()));
    button
}

fn folder_button(title: &str, children: &[Item]) -> gtk::MenuButton {
    let menu = gio::Menu::new();
    fill_menu(&menu, children);
    if children.is_empty() {
        let empty = gio::MenuItem::new(Some("(Empty)"), None);
        empty.set_action_and_target_value(Some("win.none"), None);
        menu.append_item(&empty);
    }
    gtk::MenuButton::builder()
        .child(&labelled("folder-symbolic", title.to_owned()))
        .menu_model(&menu)
        .tooltip_text(title)
        .css_classes(["flat"])
        .build()
}

/// Bookmarks as menu items, folders as submenus, separators as section breaks.
fn fill_menu(menu: &gio::Menu, items: &[Item]) {
    let mut section = gio::Menu::new();
    for item in items {
        match item {
            Item::Url { title, url } => {
                let entry = gio::MenuItem::new(Some(&label_for(title, url)), None);
                entry.set_action_and_target_value(Some(OPEN_ACTION), Some(&url.as_str().to_variant()));
                section.append_item(&entry);
            }
            Item::Folder { title, children } => {
                let submenu = gio::Menu::new();
                fill_menu(&submenu, children);
                section.append_submenu(Some(title), &submenu);
            }
            Item::Separator => {
                menu.append_section(None, &section);
                section = gio::Menu::new();
            }
        }
    }
    menu.append_section(None, &section);
}

fn labelled(icon: &str, text: String) -> gtk::Box {
    let label = gtk::Label::builder()
        .label(&text)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(MAX_LABEL_CHARS)
        .single_line_mode(true)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&label);
    content
}

/// The title, or the host when the bookmark has none.
fn label_for(title: &str, url: &Url) -> String {
    if !title.trim().is_empty() {
        return title.to_owned();
    }
    url.host_str().map_or_else(|| url.to_string(), str::to_owned)
}

/// Parses an `open-bookmark` action target.
pub(crate) fn url_from_target(target: Option<&glib::Variant>) -> Option<String> {
    target.and_then(|v| v.get::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_fall_back_to_the_host() {
        let url = Url::parse("https://example.com/path").unwrap();
        assert_eq!(label_for("Example", &url), "Example");
        assert_eq!(label_for("  ", &url), "example.com");
    }

    #[test]
    fn open_targets_are_strings() {
        assert_eq!(url_from_target(Some(&"https://x/".to_variant())).as_deref(), Some("https://x/"));
        assert_eq!(url_from_target(Some(&1u32.to_variant())), None);
        assert_eq!(url_from_target(None), None);
    }
}
