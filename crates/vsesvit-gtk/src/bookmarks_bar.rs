//! The bookmarks bar: the toolbar folder's items as buttons, folders opening menus, and
//! "Other Bookmarks" at the end when it has anything. Visibility follows the synced
//! preference through `win.show-bookmarks-bar`.
//!
//! As in Chrome, the bar shows only the items that fit whole; the rest are in the menu of a
//! "»" button at its end, before "Other Bookmarks". Items can be dragged to reorder them or
//! into a folder, and right-clicked for their context menu (see `bookmark_menu`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, glib};
use vsesvit_core::Url;
use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode};

use crate::bookmark_drag::{self, Zone};
use crate::bookmark_menu::{self, Item, Target, label_for};
use crate::favicons;
use crate::profile::Core;
use crate::window::BrowserWindow;

pub(crate) use crate::bookmark_menu::OPEN_ACTION;

const MAX_LABEL_CHARS: i32 = 18;
const SPACING: i32 = 1;
const URL_FALLBACK_ICON: &str = "web-browser-symbolic";

mod imp {
    use std::cell::Cell;

    use super::*;

    /// The bar's items in a row that shows as many whole items as fit, then the chevron.
    #[derive(Default)]
    pub struct Row {
        pub(super) items: RefCell<Vec<gtk::Widget>>,
        pub(super) chevron: gtk::Button,
        /// How many of the items are shown; the rest are in the chevron's menu.
        pub(super) shown: Cell<usize>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Row {
        const NAME: &'static str = "VsesvitBookmarksRow";
        type Type = super::Row;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Row {
        fn constructed(&self) {
            self.parent_constructed();
            let chevron = &self.chevron;
            chevron.set_label("»");
            chevron.set_tooltip_text(Some("More Bookmarks"));
            chevron.add_css_class("flat");
            chevron.add_css_class("bookmarks-chevron");
            chevron.set_parent(&*self.obj());
            chevron.set_child_visible(false);
        }

        fn dispose(&self) {
            for item in self.items.take() {
                item.unparent();
            }
            self.chevron.unparent();
        }
    }

    impl WidgetImpl for Row {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let items = self.items.borrow();
            if orientation == gtk::Orientation::Vertical {
                let (min, natural) = items
                    .iter()
                    .chain(std::iter::once(self.chevron.upcast_ref()))
                    .map(|w| w.measure(orientation, -1))
                    .fold((0, 0), |(min, nat), (m, n, ..)| (min.max(m), nat.max(n)));
                return (min, natural, -1, -1);
            }
            if items.is_empty() {
                return (0, 0, -1, -1);
            }
            // Nothing but the chevron has to fit; naturally, every item does.
            let (_, chevron, ..) = self.chevron.measure(orientation, -1);
            let natural = items.iter().map(|w| w.measure(orientation, -1).1).sum::<i32>() + gaps(items.len());
            (chevron, natural.max(chevron), -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let items = self.items.borrow();
            let widths: Vec<i32> = items.iter().map(|w| w.measure(gtk::Orientation::Horizontal, -1).1).collect();
            let (_, chevron_width, ..) = self.chevron.measure(gtk::Orientation::Horizontal, -1);
            let shown = fitting(&widths, width, chevron_width);
            let rtl = self.obj().direction() == gtk::TextDirection::Rtl;
            let place = |widget: &gtk::Widget, x: i32, w: i32| {
                let x = if rtl { width - x - w } else { x };
                widget.size_allocate(&gtk::Allocation::new(x, 0, w, height), -1);
            };
            let mut x = 0;
            for (i, (item, &w)) in items.iter().zip(&widths).enumerate() {
                item.set_child_visible(i < shown);
                if i < shown {
                    place(item, x, w);
                    x += w + SPACING;
                }
            }
            let overflows = shown < items.len();
            self.chevron.set_child_visible(overflows);
            if overflows {
                place(self.chevron.upcast_ref(), width - chevron_width, chevron_width);
            }
            self.shown.set(shown);
        }
    }
}

/// The space between `n` items.
fn gaps(n: usize) -> i32 {
    i32::try_from(n.saturating_sub(1)).unwrap_or(i32::MAX) * SPACING
}

/// How many of the items, `widths` wide, are shown in `width`: all when they fit, else as
/// many as fit whole beside the chevron.
fn fitting(widths: &[i32], width: i32, chevron: i32) -> usize {
    if widths.iter().sum::<i32>() + gaps(widths.len()) <= width {
        return widths.len();
    }
    let room = width - chevron - SPACING;
    let mut used = 0;
    widths
        .iter()
        .take_while(|&&w| {
            let fits = used + w <= room;
            used += w + SPACING;
            fits
        })
        .count()
}

glib::wrapper! {
    pub struct Row(ObjectSubclass<imp::Row>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Row {
    fn new() -> Self {
        glib::Object::new()
    }

    fn set_items(&self, widgets: Vec<gtk::Widget>) {
        let imp = self.imp();
        for old in imp.items.take() {
            old.unparent();
        }
        for widget in &widgets {
            widget.insert_before(self, Some(&imp.chevron));
        }
        imp.items.replace(widgets);
        self.queue_resize();
    }

    fn shown(&self) -> usize {
        self.imp().shown.get()
    }

    fn chevron(&self) -> &gtk::Button {
        &self.imp().chevron
    }
}

pub(crate) struct BookmarksBar {
    revealer: gtk::Revealer,
    row: Row,
    empty: gtk::Label,
    /// Holds "Other Bookmarks" after the chevron.
    end: gtk::Box,
    /// What the row's items show, in order.
    items: Rc<RefCell<Vec<Item>>>,
    /// What "Other Bookmarks" shows.
    other: RefCell<Option<Item>>,
}

impl BookmarksBar {
    pub(crate) fn new() -> Self {
        let row = Row::new();
        row.set_hexpand(true);
        // Dropping past the last item appends to the bar.
        row.add_controller(drop_target(&row, |window| window.browser().core().borrow_mut().bookmarks().get(BookmarkId::TOOLBAR)));
        let items: Rc<RefCell<Vec<Item>>> = Rc::default();
        row.chevron().connect_clicked(glib::clone!(
            #[weak]
            row,
            #[strong]
            items,
            move |chevron| {
                let hidden: Vec<Item> = items.borrow().iter().skip(row.shown()).cloned().collect();
                bookmark_menu::popup(chevron, &hidden, gtk::PositionType::Bottom);
            }
        ));
        let empty = gtk::Label::builder()
            .label("Bookmarks you add to the bar appear here")
            .css_classes(["dim-label", "caption"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .xalign(0.0)
            .hexpand(true)
            .margin_start(6)
            .visible(false)
            .build();
        let end = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(1)
            .css_classes(["toolbar", "bookmarks-bar"])
            .build();
        bar.append(&row);
        bar.append(&empty);
        bar.append(&end);
        bookmark_menu::attach_context_menu(&bar, Target::Bar);
        let revealer = gtk::Revealer::builder()
            .child(&bar)
            .reveal_child(true)
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .build();
        BookmarksBar { revealer, row, empty, end, items, other: RefCell::default() }
    }

    pub(crate) fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    pub(crate) fn set_revealed(&self, revealed: bool) {
        self.revealer.set_reveal_child(revealed);
    }

    /// Shows the profile's toolbar and "other" folders. When only icons changed, the buttons
    /// stay and show them: pages of a bookmarked site store their icons as they load, and
    /// building every button again each time would close the bar's menus and drop its hover.
    pub(crate) fn refresh(&self, core: &Core) {
        let (toolbar, other, icons) = {
            let mut profile = core.borrow_mut();
            let bookmarks = profile.bookmarks();
            let toolbar = bookmark_menu::collect(&bookmarks, BookmarkId::TOOLBAR);
            let other = bookmarks.get(BookmarkId::OTHER).map(|node| Item {
                children: bookmark_menu::collect(&bookmarks, node.id).into(),
                node,
            });
            let icons: HashMap<BookmarkId, gdk::Texture> = toolbar
                .iter()
                .filter_map(|item| Some((item.node.id, favicons::stored(&mut profile, item.node.url.as_ref()?)?)))
                .collect();
            (toolbar, other, icons)
        };
        if *self.items.borrow() == toolbar && *self.other.borrow() == other {
            self.show_icons(&toolbar, &icons);
            return;
        }
        while let Some(child) = self.end.first_child() {
            self.end.remove(&child);
        }
        let widgets = toolbar
            .iter()
            .map(|item| {
                let widget = item_widget(item, icons.get(&item.node.id));
                let id = item.node.id;
                widget.add_controller(bookmark_drag::drag_source(move || Some(id)));
                let node = item.node.clone();
                widget.add_controller(drop_target(&self.row, move |_| Some(node.clone())));
                bookmark_menu::attach_context_menu(&widget, Target::Node(item.clone()));
                widget
            })
            .collect();
        self.row.set_items(widgets);
        self.empty.set_visible(toolbar.is_empty());
        self.items.replace(toolbar);
        self.other.replace(other.clone());
        if let Some(other) = other.filter(|other| !other.children.is_empty()) {
            let button = folder_button("Other Bookmarks", &other.children);
            let node = other.node.clone();
            button.add_controller(drop_target(&self.row, move |_| Some(node.clone())));
            bookmark_menu::attach_context_menu(&button, Target::Node(other));
            self.end.append(&button);
        }
    }

    /// Shows `icons` on the buttons of `items`, which the row already holds.
    fn show_icons(&self, items: &[Item], icons: &HashMap<BookmarkId, gdk::Texture>) {
        for (item, widget) in items.iter().zip(self.row.imp().items.borrow().iter()) {
            if item.node.url.is_none() {
                continue;
            }
            let image = widget.first_child().and_then(|content| content.first_child()).and_downcast::<gtk::Image>();
            if let Some(image) = image {
                favicons::show(&image, icons.get(&item.node.id), URL_FALLBACK_ICON);
            }
        }
    }

    /// The bar's button for `url`, if it has one.
    pub(crate) fn button_for(&self, url: &str) -> Option<gtk::Button> {
        self.row
            .imp()
            .items
            .borrow()
            .iter()
            .find(|widget| widget.tooltip_text().as_deref() == Some(url))
            .and_then(|widget| widget.clone().downcast::<gtk::Button>().ok())
    }

    /// Whether the bar has a button for `url` (used by the self-test).
    pub(crate) fn shows(&self, url: &str) -> bool {
        self.button_for(url).is_some()
    }

    /// How many items the bar shows and how many are in the chevron's menu.
    #[cfg_attr(not(any(test, feature = "self-test")), allow(dead_code))]
    pub(crate) fn overflow(&self) -> (usize, usize) {
        let shown = self.row.shown();
        (shown, self.items.borrow().len() - shown)
    }

    /// The chevron, whose menu has the items the bar has no room for.
    #[cfg(feature = "self-test")]
    pub(crate) fn chevron(&self) -> gtk::Button {
        self.row.chevron().clone()
    }

    /// The bar's `index`th item and its button.
    #[cfg(feature = "self-test")]
    pub(crate) fn item(&self, index: usize) -> Option<(Item, gtk::Widget)> {
        let widget = self.row.imp().items.borrow().get(index)?.clone();
        Some((self.items.borrow().get(index)?.clone(), widget))
    }

    /// Whether the bar's button for `url` shows a stored favicon rather than the generic icon.
    #[cfg(any(test, feature = "self-test"))]
    pub(crate) fn shows_favicon(&self, url: &str) -> bool {
        self.button_for(url)
            .and_then(|button| button.child())
            .and_then(|content| content.first_child())
            .and_downcast::<gtk::Image>()
            .is_some_and(|image| image.storage_type() == gtk::ImageType::Paintable)
    }
}

/// Takes a dragged bookmark onto the node `target` finds: one of the bar's items, or a root.
fn drop_target(row: &Row, target: impl Fn(&BrowserWindow) -> Option<BookmarkNode> + 'static) -> gtk::DropTarget {
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

fn item_widget(item: &Item, icon: Option<&gdk::Texture>) -> gtk::Widget {
    let node = &item.node;
    match (node.kind, &node.url) {
        (vsesvit_core::bookmarks::NodeKind::Url, Some(url)) => url_button(&node.title, url, icon).upcast(),
        (vsesvit_core::bookmarks::NodeKind::Folder, _) => folder_button(&node.title, &item.children).upcast(),
        _ => gtk::Separator::new(gtk::Orientation::Vertical).upcast(),
    }
}

fn url_button(title: &str, url: &Url, icon: Option<&gdk::Texture>) -> gtk::Button {
    let button = gtk::Button::builder()
        .child(&labelled(favicons::image(icon, URL_FALLBACK_ICON), label_for(title, url)))
        .tooltip_text(url.as_str())
        .css_classes(["flat"])
        .build();
    button.set_action_name(Some(OPEN_ACTION));
    button.set_action_target_value(Some(&url.as_str().to_variant()));
    let middle = gtk::GestureClick::builder().button(gdk::BUTTON_MIDDLE).build();
    let target = url.as_str().to_owned();
    middle.connect_released(move |gesture, _, _, _| {
        if let Some(button) = gesture.widget() {
            let _ = button.activate_action("win.open-in-new-tab", Some(&target.to_variant()));
        }
    });
    button.add_controller(middle);
    button
}

fn folder_button(title: &str, children: &Rc<[Item]>) -> gtk::Button {
    let button = gtk::Button::builder()
        .child(&labelled(gtk::Image::from_icon_name("folder-symbolic"), title.to_owned()))
        .tooltip_text(title)
        .css_classes(["flat"])
        .build();
    let children = children.clone();
    button.connect_clicked(move |button| {
        bookmark_menu::popup(button, &children, gtk::PositionType::Bottom);
    });
    button
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

/// Parses an `open-bookmark` action target.
pub(crate) fn url_from_target(target: Option<&glib::Variant>) -> Option<String> {
    target.and_then(|v| v.get::<String>())
}

#[cfg(test)]
mod tests {
    use vsesvit_core::bookmarks::InsertAt;

    use super::*;
    use crate::test_support::{browser, wait_until};

    #[test]
    fn items_fit_whole_or_go_to_the_chevron() {
        assert_eq!(fitting(&[50, 50, 50], 152, 20), 3, "everything fits");
        assert_eq!(fitting(&[50, 50, 50], 151, 20), 2, "the chevron takes the last item's place");
        assert_eq!(fitting(&[50, 50, 50], 100, 20), 1);
        assert_eq!(fitting(&[50, 50, 50], 60, 20), 0);
        assert_eq!(fitting(&[], 0, 20), 0);
    }

    #[gtk::test]
    fn bookmarks_that_do_not_fit_move_to_the_chevron_menu_instead_of_widening_the_window() {
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
        wait_until("the bar to lay out", || bar.row.width() > 0 && bar.row.shown() > 0);
        let (shown, hidden) = bar.overflow();
        let widgets = bar.row.imp().items.borrow().clone();
        let label = |w: &gtk::Widget| w.first_child().and_then(|content| content.last_child()).and_downcast::<gtk::Label>();
        let whole = widgets.iter().take(shown).filter_map(label).all(|label| !label.layout().is_ellipsized());
        let chevron = bar.row.chevron().compute_bounds(&bar.row).map(|b| b.x()).unwrap_or(0.0);
        let before_chevron = widgets
            .iter()
            .take(shown)
            .filter_map(|w| w.compute_bounds(&bar.row))
            .all(|b| b.x() + b.width() <= chevron);
        let chevron_visible = bar.row.chevron().is_child_visible();

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
        assert!(shown > 0 && hidden > 0, "{shown} shown, {hidden} in the menu");
        assert!(whole, "every shown item has its natural width");
        assert!(before_chevron, "no item reaches under the chevron");
        assert!(chevron_visible);
    }

    #[test]
    fn open_targets_are_strings() {
        assert_eq!(url_from_target(Some(&"https://x/".to_variant())).as_deref(), Some("https://x/"));
        assert_eq!(url_from_target(Some(&1u32.to_variant())), None);
        assert_eq!(url_from_target(None), None);
    }
}
