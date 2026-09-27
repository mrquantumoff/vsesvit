//! The vertical tab list shown in the split view's sidebar: a `GtkListView` over
//! `AdwTabView`'s page model, so it is always in step with the tabs and selecting a row
//! selects the page. Each row shows the favicon (or a spinner while loading), the title
//! and a close button; rows can be dragged to reorder, and a middle click closes a tab.

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, gio, glib};

pub(crate) struct TabList {
    root: gtk::Box,
}

impl TabList {
    pub(crate) fn new(view: &adw::TabView) -> Self {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(glib::clone!(
            #[weak]
            view,
            move |_, item| {
                if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                    item.set_child(Some(&TabRow::new(&view)));
                }
            }
        ));
        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            if let (Some(row), Some(page)) = (
                item.child().and_downcast::<TabRow>(),
                item.item().and_downcast::<adw::TabPage>(),
            ) {
                row.bind(&page);
            }
        });
        factory.connect_unbind(|_, item| {
            if let Some(row) = item
                .downcast_ref::<gtk::ListItem>()
                .and_then(|item| item.child())
                .and_downcast::<TabRow>()
            {
                row.unbind();
            }
        });

        let list = gtk::ListView::new(Some(view.pages()), Some(factory));
        list.add_css_class("navigation-sidebar");
        list.set_vexpand(true);
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
        TabList { root }
    }

    pub(crate) fn widget(&self) -> &gtk::Box {
        &self.root
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
