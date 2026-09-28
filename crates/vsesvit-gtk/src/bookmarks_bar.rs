//! The bookmarks bar: the toolbar folder's items as buttons, folders as menus, and "Other
//! Bookmarks" at the end when it has anything. Visibility follows the synced preference
//! through `win.show-bookmarks-bar`. The items scroll sideways when they do not fit, and
//! can be dragged to reorder them or into a folder.

use std::collections::HashMap;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use vsesvit_core::Url;
use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, Bookmarks, NodeKind};

use crate::bookmark_drag::{self, Zone};
use crate::favicons;
use crate::profile::Core;
use crate::window::BrowserWindow;

/// Folder nesting deeper than this is not shown in menus.
const MAX_DEPTH: usize = 12;
const MAX_LABEL_CHARS: i32 = 18;
/// Pixels one mouse wheel step scrolls the bar sideways.
const WHEEL_STEP: f64 = 48.0;

pub(crate) const OPEN_ACTION: &str = "win.open-bookmark";

/// A bookmark subtree copied out of core, so widgets are built without a borrow held.
struct Item {
    node: BookmarkNode,
    /// A folder's children.
    children: Vec<Item>,
}

pub(crate) struct BookmarksBar {
    revealer: gtk::Revealer,
    row: gtk::Box,
    /// Holds "Other Bookmarks" outside the scrolling part, so it stays in reach.
    end: gtk::Box,
}

impl BookmarksBar {
    pub(crate) fn new() -> Self {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(1)
            .build();
        // Items keep their natural width and scroll, rather than shrinking to their ellipses.
        let viewport = gtk::Viewport::builder()
            .child(&row)
            .hscroll_policy(gtk::ScrollablePolicy::Natural)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&viewport)
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .build();
        scroller.add_controller(wheel_scrolls_sideways(&scroller));
        // Dropping past the last item appends to the bar.
        row.add_controller(drop_target(&row, |window| window.browser().core().borrow_mut().bookmarks().get(BookmarkId::TOOLBAR)));
        let end = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(1)
            .css_classes(["toolbar", "bookmarks-bar"])
            .build();
        bar.append(&scroller);
        bar.append(&end);
        let revealer = gtk::Revealer::builder()
            .child(&bar)
            .reveal_child(true)
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .build();
        BookmarksBar { revealer, row, end }
    }

    pub(crate) fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    pub(crate) fn set_revealed(&self, revealed: bool) {
        self.revealer.set_reveal_child(revealed);
    }

    /// Rebuilds the bar from the profile's toolbar and "other" folders.
    pub(crate) fn refresh(&self, core: &Core) {
        let (toolbar, other, icons) = {
            let mut profile = core.borrow_mut();
            let bookmarks = profile.bookmarks();
            let toolbar = collect(&bookmarks, BookmarkId::TOOLBAR, 0);
            let other = bookmarks.get(BookmarkId::OTHER).map(|node| Item {
                children: collect(&bookmarks, node.id, 0),
                node,
            });
            let icons: HashMap<BookmarkId, gdk::Texture> = toolbar
                .iter()
                .filter_map(|item| Some((item.node.id, favicons::stored(&mut profile, item.node.url.as_ref()?)?)))
                .collect();
            (toolbar, other, icons)
        };
        for slot in [&self.row, &self.end] {
            while let Some(child) = slot.first_child() {
                slot.remove(&child);
            }
        }
        if toolbar.is_empty() {
            let empty = gtk::Label::builder()
                .label("Bookmarks you add to the bar appear here")
                .css_classes(["dim-label", "caption"])
                .margin_start(6)
                .build();
            self.row.append(&empty);
        }
        for item in &toolbar {
            let widget = item_widget(item, icons.get(&item.node.id));
            let id = item.node.id;
            widget.add_controller(bookmark_drag::drag_source(move || Some(id)));
            let node = item.node.clone();
            widget.add_controller(drop_target(&self.row, move |_| Some(node.clone())));
            self.row.append(&widget);
        }
        if let Some(other) = other.filter(|other| !other.children.is_empty()) {
            let button = folder_button("Other Bookmarks", &other.children);
            button.add_controller(drop_target(&self.row, move |_| Some(other.node.clone())));
            self.end.append(&button);
        }
    }

    /// The bar's button for `url`, if it has one.
    fn button_for(&self, url: &str) -> Option<gtk::Button> {
        let mut child = self.row.first_child();
        while let Some(widget) = child {
            if widget.tooltip_text().as_deref() == Some(url)
                && let Ok(button) = widget.clone().downcast::<gtk::Button>()
            {
                return Some(button);
            }
            child = widget.next_sibling();
        }
        None
    }

    /// Whether the bar has a button for `url` (used by the self-test).
    pub(crate) fn shows(&self, url: &str) -> bool {
        self.button_for(url).is_some()
    }

    /// Whether the bar's button for `url` shows a stored favicon rather than the generic icon.
    #[cfg(test)]
    pub(crate) fn shows_favicon(&self, url: &str) -> bool {
        self.button_for(url)
            .and_then(|button| button.child())
            .and_then(|content| content.first_child())
            .and_downcast::<gtk::Image>()
            .is_some_and(|image| image.storage_type() == gtk::ImageType::Paintable)
    }
}

/// Takes a dragged bookmark onto the node `target` finds: one of the bar's items, or a root.
fn drop_target(row: &gtk::Box, target: impl Fn(&BrowserWindow) -> Option<BookmarkNode> + 'static) -> gtk::DropTarget {
    let row = row.downgrade();
    let window = move || row.upgrade()?.root().and_downcast::<BrowserWindow>();
    bookmark_drag::drop_target(
        gtk::Orientation::Horizontal,
        {
            let window = window.clone();
            move || target(&window()?)
        },
        move |id, target, zone| {
            if let Some(window) = window() {
                move_dropped(&window, id, &target, zone);
            }
        },
    )
}

fn move_dropped(window: &BrowserWindow, id: BookmarkId, target: &BookmarkNode, zone: Zone) {
    let browser = window.browser();
    match bookmark_drag::apply(browser.core(), id, target, zone) {
        Ok(true) => browser.bookmarks_changed(),
        Ok(false) => {}
        Err(e) => window.toast(adw::Toast::new(&format!("Cannot move the bookmark: {e}"))),
    }
}

/// Turns mouse wheel steps into sideways scrolling; touchpads scroll sideways themselves.
fn wheel_scrolls_sideways(scroller: &gtk::ScrolledWindow) -> gtk::EventControllerScroll {
    let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
    wheel.connect_scroll(glib::clone!(
        #[weak]
        scroller,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |wheel, _, dy| {
            if wheel.unit() != gdk::ScrollUnit::Wheel || dy == 0.0 {
                return glib::Propagation::Proceed;
            }
            let adjustment = scroller.hadjustment();
            adjustment.set_value(adjustment.value() + dy * WHEEL_STEP);
            glib::Propagation::Stop
        }
    ));
    wheel
}

fn collect(bookmarks: &Bookmarks<'_>, folder: BookmarkId, depth: usize) -> Vec<Item> {
    if depth > MAX_DEPTH {
        return Vec::new();
    }
    bookmarks
        .children(folder)
        .into_iter()
        .filter(|node| node.kind != NodeKind::Url || node.url.is_some())
        .map(|node| Item {
            children: match node.kind {
                NodeKind::Folder => collect(bookmarks, node.id, depth + 1),
                _ => Vec::new(),
            },
            node,
        })
        .collect()
}

fn item_widget(item: &Item, icon: Option<&gdk::Texture>) -> gtk::Widget {
    let node = &item.node;
    match (node.kind, &node.url) {
        (NodeKind::Url, Some(url)) => url_button(&node.title, url, icon).upcast(),
        (NodeKind::Folder, _) => folder_button(&node.title, &item.children).upcast(),
        _ => gtk::Separator::new(gtk::Orientation::Vertical).upcast(),
    }
}

fn url_button(title: &str, url: &Url, icon: Option<&gdk::Texture>) -> gtk::Button {
    let button = gtk::Button::builder()
        .child(&labelled(favicons::image(icon, "web-browser-symbolic"), label_for(title, url)))
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
        .child(&labelled(gtk::Image::from_icon_name("folder-symbolic"), title.to_owned()))
        .menu_model(&menu)
        .tooltip_text(title)
        .css_classes(["flat"])
        .build()
}

/// Bookmarks as menu items, folders as submenus, separators as section breaks.
fn fill_menu(menu: &gio::Menu, items: &[Item]) {
    let mut section = gio::Menu::new();
    for item in items {
        let node = &item.node;
        match (node.kind, &node.url) {
            (NodeKind::Url, Some(url)) => {
                let entry = gio::MenuItem::new(Some(&label_for(&node.title, url)), None);
                entry.set_action_and_target_value(Some(OPEN_ACTION), Some(&url.as_str().to_variant()));
                section.append_item(&entry);
            }
            (NodeKind::Folder, _) => {
                let submenu = gio::Menu::new();
                fill_menu(&submenu, &item.children);
                section.append_submenu(Some(&node.title), &submenu);
            }
            _ => {
                menu.append_section(None, &section);
                section = gio::Menu::new();
            }
        }
    }
    menu.append_section(None, &section);
}

fn labelled(icon: gtk::Image, text: String) -> gtk::Box {
    let label = gtk::Label::builder()
        .label(&text)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(MAX_LABEL_CHARS)
        .single_line_mode(true)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    content.append(&icon);
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
    use vsesvit_core::bookmarks::InsertAt;

    use super::*;
    use crate::test_support::{browser, wait_until};

    #[gtk::test]
    fn many_bookmarks_scroll_sideways_instead_of_widening_the_window() {
        let browser = browser();
        let folder = {
            let mut profile = browser.core().borrow_mut();
            let mut bookmarks = profile.bookmarks();
            let folder = bookmarks.add_folder(BookmarkId::TOOLBAR, InsertAt::End, "Many").unwrap();
            for i in 0..40 {
                let url = Url::parse(&format!("https://site{i}.example/")).unwrap();
                bookmarks.add_url(BookmarkId::TOOLBAR, InsertAt::End, &format!("Bookmark number {i}"), &url).unwrap();
            }
            folder
        };
        let window = BrowserWindow::new(&browser);
        window.set_default_size(900, 600);
        window.present();
        let bar = window.bookmarks_bar();
        let (min_width, ..) = bar.widget().measure(gtk::Orientation::Horizontal, -1);
        let scroller = bar.row.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>().unwrap();
        let adjustment = scroller.hadjustment();
        wait_until("the bar to lay out", || adjustment.page_size() > 0.0);
        let label = bar.button_for("https://site0.example/").and_then(|b| b.child()).and_then(|c| c.last_child()).unwrap();
        let (_, natural, ..) = label.measure(gtk::Orientation::Horizontal, -1);
        let (scrolls, full_width) = (adjustment.upper() > adjustment.page_size(), label.width() >= natural);
        {
            let mut profile = browser.core().borrow_mut();
            let mut bookmarks = profile.bookmarks();
            let added: Vec<BookmarkId> = bookmarks.children(BookmarkId::TOOLBAR).into_iter().map(|node| node.id).skip_while(|&id| id != folder).collect();
            for id in added {
                bookmarks.remove(id).unwrap();
            }
        }
        window.destroy();
        assert!(min_width < 400, "the bar asks for {min_width} px");
        assert!(scrolls, "the items overflow into a row that scrolls");
        assert!(full_width, "the labels keep their natural width");
    }

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
