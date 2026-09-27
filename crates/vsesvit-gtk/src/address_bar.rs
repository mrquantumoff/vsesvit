//! The address entry: shows the selected tab's committed URI and load progress, takes typed
//! input, and offers suggestions in a popover below it.
//!
//! It emits `edited` for every change the user makes, `submitted` when Enter is pressed without
//! a suggestion selected, and `cancelled` when Escape gives up editing.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::subclass::Signal;
use gtk::{gdk, glib};

use crate::tab::display_uri;

/// One suggestion row. `activate` runs when the row is chosen.
#[derive(Clone)]
pub(crate) struct Suggestion {
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) icon_name: &'static str,
    pub(crate) activate: Rc<dyn Fn()>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Security {
    /// `about:`, `file:` and blank pages.
    #[default]
    NotApplicable,
    /// HTTPS; pages with certificate errors are never shown, so every HTTPS page qualifies.
    Secure,
    Insecure,
}

impl Security {
    pub(crate) fn of(uri: Option<&str>) -> Self {
        match uri
            .and_then(|u| u.split_once(':'))
            .map(|(scheme, _)| scheme)
        {
            Some("https") => Security::Secure,
            Some("http") => Security::Insecure,
            _ => Security::NotApplicable,
        }
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AddressBar {
        pub(super) entry: gtk::Entry,
        pub(super) popover: gtk::Popover,
        pub(super) list: gtk::ListBox,
        pub(super) focus: OnceCell<gtk::EventControllerFocus>,
        pub(super) items: RefCell<Vec<Suggestion>>,
        /// What the entry shows when the user is not editing it.
        pub(super) shown: RefCell<String>,
        pub(super) editing: Cell<bool>,
        pub(super) security: Cell<Security>,
        pub(super) updating: Cell<bool>,
        pub(super) popover_width: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AddressBar {
        const NAME: &'static str = "VsesvitAddressBar";
        type Type = super::AddressBar;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for AddressBar {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder("edited")
                        .param_types([String::static_type()])
                        .build(),
                    Signal::builder("submitted")
                        .param_types([String::static_type()])
                        .build(),
                    Signal::builder("cancelled").build(),
                ]
            })
        }

        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }

        fn dispose(&self) {
            self.popover.unparent();
        }
    }

    impl WidgetImpl for AddressBar {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            if self.popover_width.replace(width) != width {
                self.popover.set_size_request(width, -1);
            }
            self.popover.present();
        }
    }

    impl BinImpl for AddressBar {}
}

glib::wrapper! {
    pub struct AddressBar(ObjectSubclass<imp::AddressBar>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AddressBar {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    fn setup(&self) {
        let imp = self.imp();
        let entry = &imp.entry;
        entry.set_hexpand(true);
        entry.set_input_purpose(gtk::InputPurpose::Url);
        entry.set_input_hints(gtk::InputHints::NO_SPELLCHECK);
        entry.set_placeholder_text(Some("Enter address"));
        entry.update_property(&[gtk::accessible::Property::Label("Address")]);
        self.set_child(Some(entry));

        let list = &imp.list;
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        list.set_can_focus(false);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(440)
            .child(list)
            .build();
        let popover = &imp.popover;
        popover.set_child(Some(&scroller));
        popover.set_parent(self);
        popover.set_autohide(false);
        popover.set_has_arrow(false);
        popover.set_can_focus(false);
        popover.set_position(gtk::PositionType::Bottom);
        popover.add_css_class("address-suggestions");

        entry.connect_changed(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |entry| bar.text_changed(&entry.text())
        ));
        entry.connect_activate(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |_| bar.submit()
        ));
        list.connect_row_activated(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |_, row| bar.choose(row.index())
        ));

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| bar.key_pressed(key)
        ));
        entry.add_controller(keys);

        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(glib::clone!(
            #[weak]
            popover,
            move |_| popover.popdown()
        ));
        entry.add_controller(focus.clone());
        imp.focus.set(focus).expect("setup runs once");
    }

    /// Shows `uri` (the selected tab's committed URI) unless the user is editing.
    pub(crate) fn show_uri(&self, uri: Option<&str>) {
        let imp = self.imp();
        imp.shown.replace(uri.map(display_uri).unwrap_or_default());
        if !imp.editing.get() {
            self.set_text_quietly(&imp.shown.borrow());
        }
    }

    /// Switches to another tab's state: its unsubmitted text if it had any, else its URI.
    pub(crate) fn restore(&self, typed: Option<String>, uri: Option<&str>) {
        let imp = self.imp();
        imp.shown.replace(uri.map(display_uri).unwrap_or_default());
        imp.editing.set(typed.is_some());
        self.set_text_quietly(&typed.unwrap_or_else(|| imp.shown.borrow().clone()));
        imp.popover.popdown();
        self.show_security();
    }

    /// The unsubmitted text, if the user has been editing.
    pub(crate) fn take_edit(&self) -> Option<String> {
        let imp = self.imp();
        imp.editing.replace(false).then(|| imp.entry.text().into())
    }

    pub(crate) fn focus_for_typing(&self) {
        let entry = &self.imp().entry;
        entry.grab_focus();
        entry.select_region(0, -1);
    }

    /// Enters `text` as if the user typed it and pressed Enter (the self-test's way
    /// through the omnibox).
    pub(crate) fn submit_text(&self, text: &str) {
        let imp = self.imp();
        imp.editing.set(true);
        self.set_text_quietly(text);
        self.submit();
    }

    pub(crate) fn set_progress(&self, fraction: f64) {
        self.imp().entry.set_progress_fraction(fraction);
    }

    /// The connection state of the shown URI. It is hidden while the user edits the text.
    pub(crate) fn set_security(&self, security: Security) {
        self.imp().security.set(security);
        self.show_security();
    }

    fn show_security(&self) {
        let imp = self.imp();
        let shown = if imp.editing.get() {
            Security::NotApplicable
        } else {
            imp.security.get()
        };
        let (icon, tooltip) = match shown {
            Security::NotApplicable => (None, None),
            Security::Secure => (Some("channel-secure-symbolic"), Some("Secure connection")),
            Security::Insecure => (
                Some("channel-insecure-symbolic"),
                Some("This connection is not secure"),
            ),
        };
        imp.entry.set_primary_icon_name(icon);
        imp.entry.set_primary_icon_tooltip_text(tooltip);
    }

    /// Replaces the suggestion rows. The popover opens only while the entry has focus.
    pub(crate) fn set_suggestions(&self, items: Vec<Suggestion>) {
        let imp = self.imp();
        imp.list.remove_all();
        for item in &items {
            imp.list.append(&suggestion_row(item));
        }
        let has_focus = imp.focus.get().is_some_and(|f| f.contains_focus());
        if items.is_empty() || !has_focus {
            imp.popover.popdown();
        } else {
            imp.popover.popup();
        }
        imp.items.replace(items);
    }

    pub(crate) fn connect_edited<F: Fn(&Self, &str) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "edited",
            false,
            glib::closure_local!(move |bar: &Self, text: String| f(bar, &text)),
        )
    }

    pub(crate) fn connect_submitted<F: Fn(&Self, &str) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "submitted",
            false,
            glib::closure_local!(move |bar: &Self, text: String| f(bar, &text)),
        )
    }

    pub(crate) fn connect_cancelled<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "cancelled",
            false,
            glib::closure_local!(move |bar: &Self| f(bar)),
        )
    }

    fn set_text_quietly(&self, text: &str) {
        let imp = self.imp();
        imp.updating.set(true);
        imp.entry.set_text(text);
        imp.updating.set(false);
    }

    fn text_changed(&self, text: &str) {
        let imp = self.imp();
        if imp.updating.get() {
            return;
        }
        if !imp.editing.replace(true) {
            self.show_security();
        }
        imp.list.unselect_all();
        self.emit_by_name::<()>("edited", &[&text.to_owned()]);
    }

    fn submit(&self) {
        let imp = self.imp();
        if let Some(row) = imp.list.selected_row().filter(|_| imp.popover.is_visible()) {
            self.choose(row.index());
            return;
        }
        let text: String = imp.entry.text().into();
        self.finish_editing();
        self.emit_by_name::<()>("submitted", &[&text]);
    }

    fn choose(&self, index: i32) {
        let chosen = usize::try_from(index)
            .ok()
            .and_then(|i| self.imp().items.borrow().get(i).cloned());
        let Some(suggestion) = chosen else { return };
        self.finish_editing();
        self.set_text_quietly(&self.imp().shown.borrow());
        (suggestion.activate)();
    }

    fn cancel(&self) {
        let imp = self.imp();
        self.finish_editing();
        self.set_text_quietly(&imp.shown.borrow());
        self.emit_by_name::<()>("cancelled", &[]);
    }

    fn finish_editing(&self) {
        let imp = self.imp();
        imp.editing.set(false);
        imp.popover.popdown();
        self.show_security();
    }

    fn key_pressed(&self, key: gdk::Key) -> glib::Propagation {
        let imp = self.imp();
        let open = imp.popover.is_visible() && !imp.items.borrow().is_empty();
        // Up and Down never move focus out of the address bar, as they would in a plain entry.
        match key {
            gdk::Key::Down | gdk::Key::KP_Down if open => self.move_selection(1),
            gdk::Key::Up | gdk::Key::KP_Up if open => self.move_selection(-1),
            gdk::Key::Down | gdk::Key::KP_Down | gdk::Key::Up | gdk::Key::KP_Up => {}
            gdk::Key::Escape if open => imp.popover.popdown(),
            gdk::Key::Escape => self.cancel(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    fn move_selection(&self, delta: i32) {
        let imp = self.imp();
        let count = i32::try_from(imp.items.borrow().len()).unwrap_or(i32::MAX);
        let next = match imp.list.selected_row() {
            Some(row) => row.index() + delta,
            None if delta > 0 => 0,
            None => count - 1,
        };
        match imp.list.row_at_index(next) {
            Some(row) => imp.list.select_row(Some(&row)),
            None => imp.list.unselect_all(),
        }
    }
}

fn suggestion_row(item: &Suggestion) -> gtk::ListBoxRow {
    let title = gtk::Label::builder()
        .label(&item.title)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let subtitle = gtk::Label::builder()
        .label(&item.subtitle)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["dim-label", "caption"])
        .build();
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.append(&title);
    text.append(&subtitle);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(8)
        .margin_end(8)
        .build();
    content.append(&gtk::Image::from_icon_name(item.icon_name));
    content.append(&text);
    gtk::ListBoxRow::builder()
        .child(&content)
        .focusable(false)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_follows_the_scheme() {
        assert_eq!(Security::of(Some("https://example.com/")), Security::Secure);
        assert_eq!(
            Security::of(Some("http://example.com/")),
            Security::Insecure
        );
        assert_eq!(Security::of(Some("file:///tmp/x")), Security::NotApplicable);
        assert_eq!(Security::of(Some("about:blank")), Security::NotApplicable);
        assert_eq!(Security::of(None), Security::NotApplicable);
    }
}
