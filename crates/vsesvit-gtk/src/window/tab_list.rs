//! The vertical tab list shown in the split view's sidebar: a row per page of the
//! `AdwTabView`, kept in step with its page model, and selecting a row selects the page.
//! Each row shows the favicon (or a spinner while loading), the title, the in-use icon
//! while the page captures, and a close button; rows can be dragged to reorder, and a
//! middle click closes a tab.
//!
//! A `GtkListBox` rather than a `GtkListView`, because the list owns its rows: a new tab's
//! row grows in, and a closed tab's row shrinks out after its page is gone. The tab view
//! closes pages at once, so the tab count and the closed-tab stack never wait on the
//! animation; only the leaving row outlives its page, and it takes no input.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, gio, glib};

use crate::motion;

pub(crate) struct TabList {
    root: gtk::Box,
    /// Owned here; the signal handlers that update it hold it weakly.
    #[cfg_attr(not(any(test, feature = "self-test")), allow(dead_code))]
    rows: Rc<Rows>,
}

struct Rows {
    view: glib::WeakRef<adw::TabView>,
    /// Held because the tab view keeps only a weak reference to its page model.
    pages: gtk::SelectionModel,
    list: gtk::ListBox,
    /// One per page, in the view's order.
    live: RefCell<Vec<Slot>>,
    /// Rows of closed pages, until they have shrunk away.
    leaving: RefCell<Vec<Slot>>,
}

/// A page's row in the list box: the row, the revealer that grows and shrinks it, and
/// the animation fading it while it does.
#[derive(Clone)]
struct Slot(Rc<SlotInner>);

struct SlotInner {
    row: gtk::ListBoxRow,
    revealer: gtk::Revealer,
    tab: TabRow,
    fade: RefCell<Option<adw::TimedAnimation>>,
}

impl TabList {
    pub(crate) fn new(view: &adw::TabView) -> Self {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .vexpand(true)
            .valign(gtk::Align::Start)
            .build();
        let rows = Rc::new(Rows {
            view: view.downgrade(),
            pages: view.pages(),
            list: list.clone(),
            live: RefCell::new(Vec::new()),
            leaving: RefCell::new(Vec::new()),
        });
        let weak = Rc::downgrade(&rows);
        rows.pages.connect_items_changed(move |_, position, removed, added| {
            if let Some(rows) = weak.upgrade() {
                rows.items_changed(position, removed, added);
            }
        });
        let weak = Rc::downgrade(&rows);
        view.connect_selected_page_notify(move |_| {
            if let Some(rows) = weak.upgrade() {
                rows.sync_selection();
            }
        });
        let weak = Rc::downgrade(&rows);
        list.connect_row_selected(move |_, row| {
            if let (Some(rows), Some(row)) = (weak.upgrade(), row) {
                rows.row_selected(row);
            }
        });
        rows.items_changed(0, 0, rows.pages.n_items());

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();

        let new_tab = gtk::Button::builder()
            .child(
                &adw::ButtonContent::builder()
                    .icon_name("tab-new-symbolic")
                    .label("New Tab")
                    .build(),
            )
            .action_name("win.new-tab")
            .tooltip_text("New Tab")
            .css_classes(["flat"])
            .halign(gtk::Align::Fill)
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .build();

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("tab-sidebar");
        root.append(&scroller);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&new_tab);
        TabList { root, rows }
    }

    pub(crate) fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// The rows the list shows now, `(live, leaving)`, and whether every live row has
    /// finished growing in and nothing is still shrinking out.
    #[cfg(any(test, feature = "self-test"))]
    pub(crate) fn row_counts(&self) -> (usize, usize, bool) {
        let live = self.rows.live.borrow();
        let leaving = self.rows.leaving.borrow().len();
        let settled = leaving == 0
            && live.iter().all(|slot| {
                slot.0.revealer.is_child_revealed() && slot.0.revealer.opacity() >= 1.0
            });
        (live.len(), leaving, settled)
    }

    /// The pages of the rows in the order the list box shows them, leaving rows included.
    #[cfg(test)]
    pub(crate) fn shown_pages(&self) -> Vec<adw::TabPage> {
        let mut pages = Vec::new();
        let mut child = self.rows.list.first_child();
        while let Some(row) = child {
            if let Some(tab) = row.first_child().and_then(|revealer| revealer.first_child()).and_downcast::<TabRow>() {
                pages.extend(tab.page());
            }
            child = row.next_sibling();
        }
        pages
    }

    /// The opacity of the row showing `page`, while it grows in or is shown.
    #[cfg(feature = "self-test")]
    pub(crate) fn row_opacity(&self, page: &adw::TabPage) -> Option<f64> {
        self.rows
            .live
            .borrow()
            .iter()
            .find(|slot| slot.page().as_ref() == Some(page))
            .map(|slot| slot.0.revealer.opacity())
    }
}

impl Rows {
    /// Mirrors a change of the page model. A page that is removed and added back in the
    /// same change (a reorder) keeps its row, unanimated; rows grow in only while the list
    /// is on screen, so a restored session's tabs are simply there.
    fn items_changed(self: &Rc<Self>, position: u32, removed: u32, added: u32) {
        let Some(view) = self.view.upgrade() else { return };
        let animate = self.list.is_mapped();
        let at = position as usize;
        let mut gone: Vec<Slot> = self
            .live
            .borrow_mut()
            .drain(at..at + removed as usize)
            .collect();
        let mut fresh = Vec::with_capacity(added as usize);
        let mut entering = Vec::new();
        for i in position..position + added {
            let Some(page) = self.pages.item(i).and_downcast::<adw::TabPage>() else {
                continue;
            };
            let slot = match gone.iter().position(|slot| slot.page().as_ref() == Some(&page)) {
                Some(kept) => gone.remove(kept),
                None => {
                    let slot = Slot::new(&view, &page, animate);
                    entering.push(slot.clone());
                    slot
                }
            };
            fresh.push(slot);
        }
        // The list box emits `row-selected` while rows move, so no borrow is held here.
        for slot in &fresh {
            if slot.0.row.parent().is_some() {
                self.list.remove(&slot.0.row);
            }
        }
        let next = self.live.borrow().get(at).map(|slot| slot.0.row.clone());
        let mut index = next.map_or(-1, |row| row.index());
        for slot in &fresh {
            self.list.insert(&slot.0.row, index);
            if index >= 0 {
                index += 1;
            }
        }
        self.live.borrow_mut().splice(at..at, fresh);
        if animate {
            for slot in entering {
                slot.enter();
            }
        }
        for slot in gone {
            self.leave(slot);
        }
        self.sync_selection();
    }

    fn leave(self: &Rc<Self>, slot: Slot) {
        let row = &slot.0.row;
        if self.list.selected_row().as_ref() == Some(row) {
            self.list.unselect_row(row);
        }
        row.set_selectable(false);
        row.set_activatable(false);
        row.set_can_target(false);
        row.set_can_focus(false);
        self.leaving.borrow_mut().push(slot.clone());
        let weak: Weak<Self> = Rc::downgrade(self);
        let done = {
            let slot = slot.clone();
            move || {
                if let Some(rows) = weak.upgrade() {
                    rows.leaving.borrow_mut().retain(|s| !Rc::ptr_eq(&s.0, &slot.0));
                    if slot.0.row.parent().is_some() {
                        rows.list.remove(&slot.0.row);
                    }
                }
                slot.0.tab.unbind();
            }
        };
        slot.0.revealer.set_reveal_child(false);
        slot.animate(0.0, done);
    }

    fn sync_selection(&self) {
        let selected = self.view.upgrade().and_then(|view| view.selected_page());
        let row = selected.and_then(|page| {
            self.live
                .borrow()
                .iter()
                .find(|slot| slot.page().as_ref() == Some(&page))
                .map(|slot| slot.0.row.clone())
        });
        match row {
            Some(row) if self.list.selected_row().as_ref() != Some(&row) => {
                self.list.select_row(Some(&row));
            }
            Some(_) => {}
            None => self.list.unselect_all(),
        }
    }

    fn row_selected(&self, row: &gtk::ListBoxRow) {
        let page = self
            .live
            .borrow()
            .iter()
            .find(|slot| slot.0.row == *row)
            .and_then(Slot::page);
        if let (Some(view), Some(page)) = (self.view.upgrade(), page)
            && view.selected_page().as_ref() != Some(&page)
        {
            view.set_selected_page(&page);
        }
    }
}

impl Slot {
    /// Starts collapsed and transparent when it is to grow in.
    fn new(view: &adw::TabView, page: &adw::TabPage, collapsed: bool) -> Self {
        let tab = TabRow::new(view);
        tab.bind(page);
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .transition_duration(motion::TAB_ROW_MS)
            .reveal_child(!collapsed)
            .child(&tab)
            .build();
        if collapsed {
            revealer.set_opacity(0.0);
        }
        let row = gtk::ListBoxRow::builder().child(&revealer).build();
        Slot(Rc::new(SlotInner {
            row,
            revealer,
            tab,
            fade: RefCell::new(None),
        }))
    }

    fn page(&self) -> Option<adw::TabPage> {
        self.0.tab.page()
    }

    fn enter(&self) {
        self.0.revealer.set_reveal_child(true);
        self.animate(1.0, || {});
    }

    /// Fades the revealer, which stays mapped while its child is hidden, from wherever an
    /// earlier fade left it.
    fn animate(&self, to: f64, done: impl Fn() + 'static) {
        if let Some(earlier) = self.0.fade.take() {
            earlier.pause();
        }
        let weak = Rc::downgrade(&self.0);
        let animation = motion::fade(&self.0.revealer, to, motion::TAB_ROW_MS, move || {
            if let Some(inner) = weak.upgrade() {
                inner.fade.take();
            }
            done();
        });
        self.0.fade.replace(Some(animation.clone()));
        animation.play();
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct TabRow {
        pub(super) view: glib::WeakRef<adw::TabView>,
        pub(super) page: RefCell<Option<adw::TabPage>>,
        pub(super) icon: gtk::Image,
        pub(super) spinner: adw::Spinner,
        pub(super) title: gtk::Label,
        pub(super) indicator: gtk::Image,
        pub(super) close: gtk::Button,
        pub(super) bindings: RefCell<Vec<glib::Binding>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TabRow {
        const NAME: &'static str = "VsesvitTabRow";
        type Type = super::TabRow;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for TabRow {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }

        fn dispose(&self) {
            self.obj().unbind();
        }
    }

    impl WidgetImpl for TabRow {}
    impl BoxImpl for TabRow {}
}

glib::wrapper! {
    pub struct TabRow(ObjectSubclass<imp::TabRow>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl TabRow {
    fn new(view: &adw::TabView) -> Self {
        let row: Self = glib::Object::builder()
            .property("orientation", gtk::Orientation::Horizontal)
            .property("spacing", 8)
            .build();
        row.imp().view.set(Some(view));
        row
    }

    fn setup(&self) {
        let imp = self.imp();
        self.add_css_class("tab-row");
        imp.icon.set_pixel_size(16);
        imp.spinner.set_size_request(16, 16);
        imp.spinner.set_visible(false);
        imp.title.set_xalign(0.0);
        imp.title.set_hexpand(true);
        imp.title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        imp.indicator.set_pixel_size(16);
        imp.indicator.add_css_class("tab-indicator");
        imp.close.set_icon_name("window-close-symbolic");
        imp.close.set_tooltip_text(Some("Close Tab"));
        imp.close.set_valign(gtk::Align::Center);
        imp.close.add_css_class("flat");
        imp.close.add_css_class("circular");
        imp.close.add_css_class("tab-close");
        imp.close.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            self,
            move |_| row.close()
        ));
        self.append(&imp.icon);
        self.append(&imp.spinner);
        self.append(&imp.title);
        self.append(&imp.indicator);
        self.append(&imp.close);

        let middle_click = gtk::GestureClick::builder()
            .button(gdk::BUTTON_MIDDLE)
            .build();
        middle_click.connect_released(glib::clone!(
            #[weak(rename_to = row)]
            self,
            move |_, _, _, _| row.close()
        ));
        self.add_controller(middle_click);

        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::MOVE);
        drag.connect_prepare(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[upgrade_or]
            None,
            move |source, _, _| {
                let page = row.page()?;
                source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&row))), 0, 0);
                Some(gdk::ContentProvider::for_value(&page.to_value()))
            }
        ));
        self.add_controller(drag);

        let drop = gtk::DropTarget::new(adw::TabPage::static_type(), gdk::DragAction::MOVE);
        drop.connect_drop(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _, y| row.drop_page(value, y)
        ));
        self.add_controller(drop);
    }

    fn page(&self) -> Option<adw::TabPage> {
        self.imp().page.borrow().clone()
    }

    fn bind(&self, page: &adw::TabPage) {
        self.unbind();
        let imp = self.imp();
        let bindings = vec![
            page.bind_property("title", &imp.title, "label")
                .sync_create()
                .build(),
            page.bind_property("title", self, "tooltip-text")
                .sync_create()
                .build(),
            page.bind_property("icon", &imp.icon, "gicon")
                .transform_to(|_, icon: Option<gio::Icon>| {
                    Some(icon.unwrap_or_else(|| gio::ThemedIcon::new("web-browser-symbolic").upcast::<gio::Icon>()))
                })
                .sync_create()
                .build(),
            page.bind_property("loading", &imp.spinner, "visible")
                .sync_create()
                .build(),
            page.bind_property("loading", &imp.icon, "visible")
                .invert_boolean()
                .sync_create()
                .build(),
            page.bind_property("indicator-icon", &imp.indicator, "gicon")
                .sync_create()
                .build(),
            page.bind_property("indicator-icon", &imp.indicator, "visible")
                .transform_to(|_, icon: Option<gio::Icon>| Some(icon.is_some()))
                .sync_create()
                .build(),
            page.bind_property("indicator-tooltip", &imp.indicator, "tooltip-text")
                .sync_create()
                .build(),
        ];
        imp.bindings.replace(bindings);
        imp.page.replace(Some(page.clone()));
    }

    fn unbind(&self) {
        let imp = self.imp();
        for binding in imp.bindings.take() {
            binding.unbind();
        }
        imp.page.take();
    }

    fn close(&self) {
        if let (Some(view), Some(page)) = (self.imp().view.upgrade(), self.page()) {
            view.close_page(&page);
        }
    }

    /// Drops `value` (a page dragged from another row) before or after this row's page,
    /// depending on which half of the row it landed in.
    fn drop_page(&self, value: &glib::Value, y: f64) -> bool {
        let (Some(view), Some(own)) = (self.imp().view.upgrade(), self.page()) else {
            return false;
        };
        let Ok(dragged) = value.get::<adw::TabPage>() else {
            return false;
        };
        let own_index = view.page_position(&own);
        let dragged_index = view.page_position(&dragged);
        if dragged == own || own_index < 0 || dragged_index < 0 {
            return false;
        }
        let below = y >= f64::from(self.height()) / 2.0;
        let Some(target) = reorder_target(dragged_index, own_index, below) else {
            return false;
        };
        view.reorder_page(&dragged, target)
    }
}

/// The final index for a page dragged from `dragged` and dropped on the row at `own`,
/// above or below its middle. `None` when nothing would move.
fn reorder_target(dragged: i32, own: i32, below: bool) -> Option<i32> {
    let mut target = if below { own + 1 } else { own };
    if dragged < target {
        target -= 1;
    }
    (target != dragged).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::reorder_target;

    #[test]
    fn dragging_down_lands_after_the_target_row() {
        assert_eq!(reorder_target(0, 2, true), Some(2));
        assert_eq!(reorder_target(0, 2, false), Some(1));
    }

    #[test]
    fn dragging_up_lands_before_or_after_the_target_row() {
        assert_eq!(reorder_target(3, 1, false), Some(1));
        assert_eq!(reorder_target(3, 1, true), Some(2));
    }

    #[test]
    fn dropping_next_to_the_original_slot_is_a_no_op() {
        assert_eq!(reorder_target(2, 1, true), None);
        assert_eq!(reorder_target(1, 2, false), None);
    }
}
