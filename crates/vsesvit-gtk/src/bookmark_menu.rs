//! Bookmark folders as menus, and the context menu of every bookmark the bar shows.
//!
//! The menus are popovers of rows built from widgets, because GTK's model menus hide an
//! item's icon when it has a label. A link row shows the page's stored favicon (the generic
//! page icon when there is none) and opens the page; middle-click opens it in a new tab. A
//! folder row opens its own menu to the side on hover, click or Right. Labels are ellipsized
//! and a menu scrolls rather than grow past the screen.
//!
//! Right-click or the menu key on a row, on a bar item or on the bar itself opens the
//! context menu for that bookmark, folder or the bar.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use vsesvit_core::Url;
use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, Bookmarks, InsertAt, NodeKind};

use crate::bookmark_editor::{self, Subject};
use crate::dialogs::{confirm, prompt_text};
use crate::favicons;
use crate::profile::Core;
use crate::window::{BrowserWindow, Focus};

pub(crate) const OPEN_ACTION: &str = "win.open-bookmark";
/// Folder nesting deeper than this is not shown.
const MAX_DEPTH: usize = 12;
/// Chrome caps menu labels too, so a long title never widens a menu off the screen.
const MAX_MENU_LABEL_CHARS: i32 = 40;
/// How much of the monitor's height a menu may take before it scrolls.
const MAX_HEIGHT_FRACTION: f64 = 0.75;

/// A bookmark subtree copied out of core, so widgets are built without a borrow held.
#[derive(Clone, PartialEq)]
pub(crate) struct Item {
    pub(crate) node: BookmarkNode,
    /// A folder's children.
    pub(crate) children: Rc<[Item]>,
}

impl Item {
    /// The URLs "Open all" opens: the folder's own links, not those in subfolders (as Chrome).
    fn links(&self) -> Vec<Url> {
        self.children.iter().filter_map(|child| child.node.url.clone()).collect()
    }
}

pub(crate) fn collect(bookmarks: &Bookmarks<'_>, folder: BookmarkId) -> Vec<Item> {
    collect_at(bookmarks, folder, 0)
}

fn collect_at(bookmarks: &Bookmarks<'_>, folder: BookmarkId, depth: usize) -> Vec<Item> {
    if depth > MAX_DEPTH {
        return Vec::new();
    }
    bookmarks
        .children(folder)
        .into_iter()
        .filter(|node| node.kind != NodeKind::Url || node.url.is_some())
        .map(|node| Item {
            children: match node.kind {
                NodeKind::Folder => collect_at(bookmarks, node.id, depth + 1).into(),
                _ => Rc::from([]),
            },
            node,
        })
        .collect()
}

/// The title, or the host when the bookmark has none.
pub(crate) fn label_for(title: &str, url: &Url) -> String {
    if !title.trim().is_empty() {
        return title.to_owned();
    }
    url.host_str().map_or_else(|| url.to_string(), str::to_owned)
}

pub(crate) fn window_of(widget: &impl IsA<gtk::Widget>) -> Option<BrowserWindow> {
    widget.as_ref().root().and_downcast::<BrowserWindow>()
}

// Menus.

/// Opens a menu of `items` from `anchor`, below it or (for a folder row) beside it.
pub(crate) fn popup(anchor: &impl IsA<gtk::Widget>, items: &[Item], side: gtk::PositionType) -> Option<gtk::Popover> {
    let anchor = anchor.as_ref();
    let window = window_of(anchor)?;
    let menu = build(window.browser().core(), items, max_height(anchor));
    menu.set_position(side);
    // Like Chrome: a menu starts at its button's start, a submenu at its row's top.
    menu.set_halign(gtk::Align::Start);
    menu.set_valign(gtk::Align::Start);
    crate::popup(&menu, anchor);
    Some(menu)
}

fn max_height(anchor: &gtk::Widget) -> i32 {
    let monitor = anchor
        .native()
        .and_then(|native| native.surface())
        .and_then(|surface| surface.display().monitor_at_surface(&surface));
    let height = monitor.map_or(800, |monitor| monitor.geometry().height());
    (f64::from(height) * MAX_HEIGHT_FRACTION) as i32
}

fn build(core: &Core, items: &[Item], max_height: i32) -> gtk::Popover {
    let icons: Vec<Option<gdk::Texture>> = {
        let mut profile = core.borrow_mut();
        items
            .iter()
            .map(|item| item.node.url.as_ref().and_then(|url| favicons::stored(&mut profile, url)))
            .collect()
    };
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    // The submenu open from this menu, at most one.
    let open: Rc<RefCell<Option<gtk::Popover>>> = Rc::default();
    for (item, icon) in items.iter().zip(&icons) {
        let row: gtk::Widget = match (item.node.kind, &item.node.url) {
            (NodeKind::Url, Some(url)) => link_row(&item.node, url, icon.as_ref(), &open).upcast(),
            (NodeKind::Folder, _) => folder_row(item, &open).upcast(),
            _ => gtk::Separator::builder().orientation(gtk::Orientation::Horizontal).margin_top(3).margin_bottom(3).build().upcast(),
        };
        list.append(&row);
    }
    if items.is_empty() {
        let empty = row_button(gtk::Image::from_icon_name("folder-symbolic"), "(Empty)", false);
        empty.set_sensitive(false);
        list.append(&empty);
    }
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .max_content_height(max_height)
        .child(&list)
        .build();
    let menu = gtk::Popover::builder()
        .child(&scroller)
        .has_arrow(false)
        .css_classes(["menu", "bookmark-menu"])
        .build();
    menu.connect_closed(move |_| {
        if let Some(submenu) = open.take() {
            submenu.popdown();
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        menu,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            let back = if menu.direction() == gtk::TextDirection::Rtl { gdk::Key::Right } else { gdk::Key::Left };
            if key == back && menu.parent().and_then(|p| p.ancestor(gtk::Popover::static_type())).is_some() {
                menu.popdown();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    ));
    menu.add_controller(keys);
    menu
}

fn row_button(icon: gtk::Image, text: &str, folder: bool) -> gtk::Button {
    icon.set_pixel_size(16);
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(MAX_MENU_LABEL_CHARS)
        .single_line_mode(true)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.append(&icon);
    content.append(&label);
    if folder {
        content.append(&gtk::Image::from_icon_name("go-next-symbolic"));
    }
    let button = gtk::Button::builder().child(&content).css_classes(["flat", "bookmark-menu-row"]).build();
    button.update_property(&[gtk::accessible::Property::Label(text)]);
    button
}

fn link_row(node: &BookmarkNode, url: &Url, icon: Option<&gdk::Texture>, open: &Rc<RefCell<Option<gtk::Popover>>>) -> gtk::Button {
    let row = row_button(favicons::image(icon, "web-browser-symbolic"), &label_for(&node.title, url), false);
    row.set_tooltip_text(Some(&format!("{}\n{}", node.title, url.as_str())));
    row.set_action_name(Some(OPEN_ACTION));
    row.set_action_target_value(Some(&url.as_str().to_variant()));
    row.connect_clicked(|row| close_menus(row.upcast_ref()));
    let middle = gtk::GestureClick::builder().button(gdk::BUTTON_MIDDLE).build();
    let target = url.as_str().to_owned();
    middle.connect_released(move |gesture, _, _, _| {
        if let Some(row) = gesture.widget() {
            let _ = row.activate_action("win.open-in-new-tab", Some(&target.to_variant()));
            close_menus(&row);
        }
    });
    row.add_controller(middle);
    // Hovering a link closes the submenu a folder row opened, as in any menu.
    let motion = gtk::EventControllerMotion::new();
    let open = Rc::downgrade(open);
    motion.connect_enter(move |_, _, _| {
        if let Some(submenu) = open.upgrade().and_then(|open| open.take()) {
            submenu.popdown();
        }
    });
    row.add_controller(motion);
    attach_context_menu(&row, Target::Node(Item { node: node.clone(), children: Rc::from([]) }));
    row
}

fn folder_row(item: &Item, open: &Rc<RefCell<Option<gtk::Popover>>>) -> gtk::Button {
    let row = row_button(gtk::Image::from_icon_name("folder-symbolic"), &item.node.title, true);
    let show = {
        let (children, open) = (item.children.clone(), Rc::downgrade(open));
        move |row: &gtk::Button, focus: bool| {
            let Some(open) = open.upgrade() else { return };
            let already = open.borrow().as_ref().is_some_and(|submenu| submenu.parent().as_ref() == Some(row.upcast_ref()));
            if !already {
                if let Some(previous) = open.take() {
                    previous.popdown();
                }
                let side = if row.direction() == gtk::TextDirection::Rtl { gtk::PositionType::Left } else { gtk::PositionType::Right };
                *open.borrow_mut() = popup(row, &children, side);
            }
            if focus && let Some(submenu) = open.borrow().as_ref() {
                submenu.child_focus(gtk::DirectionType::TabForward);
            }
        }
    };
    let show = Rc::new(show);
    row.connect_clicked({
        let show = show.clone();
        move |row| show(row, true)
    });
    let motion = gtk::EventControllerMotion::new();
    motion.connect_enter({
        let show = show.clone();
        move |motion, _, _| {
            if let Some(row) = motion.widget().and_downcast::<gtk::Button>() {
                show(&row, false);
            }
        }
    });
    row.add_controller(motion);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(move |keys, key, _, _| {
        let Some(row) = keys.widget().and_downcast::<gtk::Button>() else { return glib::Propagation::Proceed };
        let forward = if row.direction() == gtk::TextDirection::Rtl { gdk::Key::Left } else { gdk::Key::Right };
        if key != forward {
            return glib::Propagation::Proceed;
        }
        show(&row, true);
        glib::Propagation::Stop
    });
    row.add_controller(keys);
    attach_context_menu(&row, Target::Node(item.clone()));
    row
}

/// Closes every menu `widget` is in, innermost first.
fn close_menus(widget: &gtk::Widget) {
    let mut at = widget.ancestor(gtk::Popover::static_type());
    while let Some(menu) = at.and_downcast::<gtk::Popover>() {
        at = menu.parent().and_then(|parent| parent.ancestor(gtk::Popover::static_type()));
        menu.popdown();
    }
}

// Context menus.

/// What a context menu is about.
#[derive(Clone)]
pub(crate) enum Target {
    /// A link or a folder, with the folder's children.
    Node(Item),
    /// The bar's empty space.
    Bar,
}

/// Right-click, or the menu key while focused, opens `target`'s context menu on `widget`.
pub(crate) fn attach_context_menu(widget: &impl IsA<gtk::Widget>, target: Target) {
    let target = Rc::new(target);
    let click = gtk::GestureClick::builder().button(gdk::BUTTON_SECONDARY).build();
    click.connect_pressed({
        let target = target.clone();
        move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(widget) = gesture.widget() {
                open_context_menu(&widget, &target, Some((x, y)));
            }
        }
    });
    widget.add_controller(click);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(move |keys, key, _, modifiers| {
        let menu_key = key == gdk::Key::Menu || (key == gdk::Key::F10 && modifiers.contains(gdk::ModifierType::SHIFT_MASK));
        match keys.widget() {
            Some(widget) if menu_key => {
                open_context_menu(&widget, &target, None);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    widget.add_controller(keys);
}

/// Opens `target`'s context menu on `widget`, at the pointer or below the widget.
pub(crate) fn open_context_menu(widget: &gtk::Widget, target: &Target, at: Option<(f64, f64)>) -> gtk::PopoverMenu {
    let (menu, actions) = context_model(widget, target);
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.insert_action_group("bookmark", Some(&actions));
    popover.set_has_arrow(false);
    let (x, y) = at.unwrap_or((f64::from(widget.width()) / 2.0, f64::from(widget.height())));
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.set_position(gtk::PositionType::Bottom);
    crate::popup(&popover, widget);
    popover
}

/// The menu for `target` and the `bookmark.*` actions its items run. Each action ends by
/// closing the bookmark menus `widget` is in.
fn context_model(widget: &gtk::Widget, target: &Target) -> (gio::Menu, gio::SimpleActionGroup) {
    let menu = gio::Menu::new();
    let actions = gio::SimpleActionGroup::new();
    let anchor = widget.downgrade();
    let add = |name: &str, enabled: bool, run: Box<dyn Fn(&BrowserWindow)>| {
        let action = gio::SimpleAction::new(name, None);
        action.set_enabled(enabled);
        let anchor = anchor.clone();
        action.connect_activate(move |_, _| {
            let Some(anchor) = anchor.upgrade() else { return };
            if let Some(window) = window_of(&anchor) {
                close_menus(&anchor);
                run(&window);
            }
        });
        actions.add_action(&action);
    };
    match target {
        Target::Node(item) if item.node.kind == NodeKind::Url => {
            let url = item.node.url.clone().map(|url| url.to_string()).unwrap_or_default();
            let opens = gio::Menu::new();
            opens.append(Some("Open in New _Tab"), Some("bookmark.open-tab"));
            opens.append(Some("Open in New _Window"), Some("bookmark.open-window"));
            let edits = gio::Menu::new();
            edits.append(Some("_Edit…"), Some("bookmark.edit"));
            edits.append(Some("_Copy Link"), Some("bookmark.copy"));
            edits.append(Some("_Delete"), Some("bookmark.delete"));
            menu.append_section(None, &opens);
            menu.append_section(None, &edits);
            add("open-tab", true, Box::new({
                let url = url.clone();
                move |window| {
                    window.open_tab(Some(&url), None, Focus::Foreground);
                }
            }));
            add("open-window", true, Box::new({
                let url = item.node.url.clone();
                move |window| {
                    window.browser().open_window(url.as_slice());
                }
            }));
            add("copy", true, Box::new(move |window| window.clipboard().set_text(&url)));
            add("edit", true, Box::new({
                let node = item.node.clone();
                move |window| edit(window, Subject::Existing(node.clone()))
            }));
            add("delete", true, Box::new({
                let node = item.node.clone();
                move |window| delete(window, node.clone(), 0)
            }));
        }
        Target::Node(folder) => {
            let links = folder.links();
            let opens = gio::Menu::new();
            opens.append(Some(&format!("Open _All ({}) in New Tabs", links.len())), Some("bookmark.open-all"));
            let edits = gio::Menu::new();
            edits.append(Some("_Rename…"), Some("bookmark.rename"));
            edits.append(Some("_Delete"), Some("bookmark.delete"));
            menu.append_section(None, &opens);
            menu.append_section(None, &edits);
            let editable = !folder.node.id.is_root();
            add("open-all", !links.is_empty(), Box::new(move |window| {
                for (i, url) in links.iter().enumerate() {
                    let focus = if i == 0 { Focus::Foreground } else { Focus::Background };
                    window.open_tab(Some(url.as_str()), None, focus);
                }
            }));
            add("rename", editable, Box::new({
                let node = folder.node.clone();
                move |window| edit(window, Subject::Existing(node.clone()))
            }));
            add("delete", editable, Box::new({
                let (node, count) = (folder.node.clone(), folder.children.len());
                move |window| delete(window, node.clone(), count)
            }));
        }
        Target::Bar => {}
    }
    let bar = gio::Menu::new();
    bar.append(Some("Add _Page…"), Some("bookmark.add-page"));
    bar.append(Some("Add _Folder…"), Some("bookmark.add-folder"));
    let view = gio::Menu::new();
    view.append(Some("Show Bookmarks _Bar"), Some("win.show-bookmarks-bar"));
    view.append(Some("Bookmark _Manager"), Some("win.show-bookmarks"));
    menu.append_section(None, &bar);
    menu.append_section(None, &view);
    // New pages and folders go where the menu was opened: into a folder, next to a link,
    // or at the end of the bar.
    let parent = match target {
        Target::Node(item) if item.node.kind == NodeKind::Folder => item.node.id,
        Target::Node(item) => item.node.parent,
        Target::Bar => BookmarkId::TOOLBAR,
    };
    add("add-page", true, Box::new(move |window| {
        let (title, url) = window
            .selected_tab()
            .map(|tab| (tab.display_title(), tab.committed_uri().filter(|uri| uri != "about:blank").unwrap_or_default()))
            .unwrap_or_default();
        edit(window, Subject::New { parent, title, url });
    }));
    add("add-folder", true, Box::new(move |window| add_folder(window, parent)));
    (menu, actions)
}

fn edit(window: &BrowserWindow, subject: Subject) {
    let window = window.clone();
    glib::spawn_future_local(async move {
        match bookmark_editor::edit(&window, window.browser().core(), subject).await {
            Ok(true) => window.browser().bookmarks_changed(),
            Ok(false) => {}
            Err(e) => window.toast(adw::Toast::new(&format!("Cannot change the bookmark: {e}"))),
        }
    });
}

/// Deletes `node`; a folder with `count` items asks first.
fn delete(window: &BrowserWindow, node: BookmarkNode, count: usize) {
    let window = window.clone();
    glib::spawn_future_local(async move {
        if node.kind == NodeKind::Folder && count > 0 {
            let body = format!("“{}” and the {count} items in it will be deleted.", node.title);
            if !confirm(&window, "Delete Folder?", &body, "_Delete").await {
                return;
            }
        }
        let removed = window.browser().core().borrow_mut().bookmarks().remove(node.id);
        if let Err(e) = removed {
            window.toast(adw::Toast::new(&format!("Cannot delete the bookmark: {e}")));
        }
        window.browser().bookmarks_changed();
    });
}

fn add_folder(window: &BrowserWindow, parent: BookmarkId) {
    let window = window.clone();
    glib::spawn_future_local(async move {
        let Some(title) = prompt_text(&window, "Add Folder", "New Folder", "_Add").await else { return };
        let added = window.browser().core().borrow_mut().bookmarks().add_folder(parent, InsertAt::End, &title);
        if let Err(e) = added {
            window.toast(adw::Toast::new(&format!("Cannot add the folder: {e}")));
        }
        window.browser().bookmarks_changed();
    });
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
}
