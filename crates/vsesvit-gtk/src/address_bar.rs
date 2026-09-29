//! The address entry: shows the selected tab's committed URI and load progress, takes typed
//! input, and offers suggestions in a popover below it. Unless full URLs are on, the URI reads
//! simplified (`example.com` for `https://www.example.com/`) while the entry does not have
//! focus.
//!
//! It emits `edited` for every change the user makes, `submitted` when Enter is pressed without
//! a suggestion selected, and `cancelled` when Escape gives up editing.
//!
//! Inside the entry, the page's security is at the start and the bookmark star at the end;
//! clicking the star runs `win.bookmark-page`. While the page uses the camera, microphone or
//! screen, a button after the security icon says so and opens the site information. When the
//! page is zoomed, its zoom level sits before the star and opens Chrome's zoom bubble. The bubbles these open are anchored to
//! them through [`AddressBar::show_popover`]. The text is centered while the entry rests,
//! and starts at the left while the user is in it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::subclass::Signal;
use gtk::{gdk, glib};
use vsesvit_core::address::simplified_url;

use crate::tab::display_uri;
use crate::zoom;

/// Room between a button over the entry and the icon next to it.
const ICON_GAP: i32 = 2;

/// What a bubble from the address bar points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Anchor {
    Security,
    Zoom,
    Star,
}

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
    /// Blank pages and error pages.
    #[default]
    NotApplicable,
    /// `file:`, `about:` and other pages that come from no site.
    Internal,
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
            Some(_) if uri != Some("about:blank") => Security::Internal,
            _ => Security::NotApplicable,
        }
    }
}

/// The page's URI as the entry shows it when the user is not editing.
#[derive(Default)]
struct Shown {
    /// While the entry has focus, or with full URLs on.
    full: String,
    simplified: String,
}

impl Shown {
    fn of(uri: Option<&str>) -> Self {
        let full = uri.map(display_uri).unwrap_or_default();
        let simplified = simplified_url(&full);
        Shown { full, simplified }
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AddressBar {
        pub(super) entry: gtk::Entry,
        /// Holds the entry, with the zoom level over it.
        pub(super) overlay: gtk::Overlay,
        pub(super) zoom: gtk::Button,
        /// What the page captures; shown while it captures and the user is not editing.
        pub(super) in_use: gtk::Button,
        pub(super) capturing: Cell<bool>,
        /// The zoom bubble's level, while it is open.
        pub(super) zoom_label: glib::WeakRef<gtk::Label>,
        /// The bubble open from the security icon, the zoom level or the star.
        pub(super) bubble: RefCell<Option<gtk::Popover>>,
        pub(super) popover: gtk::Popover,
        pub(super) list: gtk::ListBox,
        pub(super) items: RefCell<Vec<Suggestion>>,
        pub(super) shown: RefCell<Shown>,
        pub(super) editing: Cell<bool>,
        /// The entry has the keyboard focus, and so shows the whole URI.
        pub(super) focused: Cell<bool>,
        pub(super) full_urls: Cell<bool>,
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
            self.bubble.take();
        }
    }

    impl WidgetImpl for AddressBar {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            if self.popover_width.replace(width) != width {
                self.popover.set_size_request(width, -1);
            }
            self.popover.present();
            if let Some(bubble) = self.bubble.borrow().as_ref() {
                bubble.present();
            }
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
        entry.connect_icon_release(|entry, position| {
            let action = match position {
                gtk::EntryIconPosition::Primary => "win.show-site-info",
                _ => "win.bookmark-page",
            };
            let _ = entry.activate_action(action, None);
        });
        let zoom = &imp.zoom;
        zoom.add_css_class("flat");
        zoom.add_css_class("address-zoom");
        zoom.set_tooltip_text(Some("Zoom"));
        zoom.set_valign(gtk::Align::Center);
        zoom.set_visible(false);
        zoom.connect_clicked(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |_| bar.show_zoom_bubble()
        ));
        let in_use = &imp.in_use;
        in_use.add_css_class("flat");
        in_use.add_css_class("address-in-use");
        in_use.set_valign(gtk::Align::Center);
        in_use.set_action_name(Some("win.show-site-info"));
        in_use.set_visible(false);
        let overlay = &imp.overlay;
        overlay.set_child(Some(entry));
        overlay.add_overlay(zoom);
        overlay.add_overlay(in_use);
        overlay.connect_get_child_position(glib::clone!(
            #[weak]
            entry,
            #[weak]
            in_use,
            #[upgrade_or]
            None,
            move |_, child| {
                if child == in_use.upcast_ref::<gtk::Widget>() {
                    Some(after_security(&entry, child))
                } else {
                    Some(before_star(&entry, child))
                }
            }
        ));
        self.set_child(Some(overlay));
        self.set_starred(false);
        self.align_text();

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
        focus.connect_enter(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |_| bar.focus_changed(true)
        ));
        focus.connect_leave(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |_| bar.focus_changed(false)
        ));
        entry.add_controller(focus);
    }

    /// Shows `uri` (the selected tab's committed URI) unless the user is editing.
    pub(crate) fn show_uri(&self, uri: Option<&str>) {
        let imp = self.imp();
        imp.shown.replace(Shown::of(uri));
        if !imp.editing.get() {
            self.show_resting();
        }
    }

    /// Switches to another tab's state: its unsubmitted text if it had any, else its URI.
    pub(crate) fn restore(&self, typed: Option<String>, uri: Option<&str>) {
        let imp = self.imp();
        imp.shown.replace(Shown::of(uri));
        imp.editing.set(typed.is_some());
        match typed {
            Some(typed) => self.set_text_quietly(&typed),
            None => self.show_resting(),
        }
        imp.popover.popdown();
        self.show_security();
        self.align_text();
    }

    /// Whole URIs even while the entry does not have focus.
    pub(crate) fn set_full_urls(&self, full: bool) {
        let imp = self.imp();
        imp.full_urls.set(full);
        if !imp.editing.get() {
            self.show_resting();
        }
    }

    /// The page's URI, whole or simplified, for when the user is not editing.
    fn show_resting(&self) {
        let imp = self.imp();
        let text = {
            let shown = imp.shown.borrow();
            if imp.focused.get() || imp.full_urls.get() {
                shown.full.clone()
            } else {
                shown.simplified.clone()
            }
        };
        self.set_text_quietly(&text);
    }

    /// Focus in shows the whole URI, selected; focus out goes back to the simplified one.
    /// Text the user typed stays either way.
    fn focus_changed(&self, focused: bool) {
        let imp = self.imp();
        imp.focused.set(focused);
        if !focused {
            imp.popover.popdown();
        }
        self.align_text();
        if imp.editing.get() {
            return;
        }
        self.show_resting();
        if focused {
            // After the click that focused the entry has placed the cursor.
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = bar)]
                self,
                move || {
                    let imp = bar.imp();
                    if imp.focused.get() && !imp.editing.get() {
                        imp.entry.select_region(0, -1);
                    }
                }
            ));
        }
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
            Security::Internal => (Some("dialog-information-symbolic"), Some("View site information")),
            Security::Secure => (Some("channel-secure-symbolic"), Some("Secure connection")),
            Security::Insecure => (
                Some("channel-insecure-symbolic"),
                Some("This connection is not secure"),
            ),
        };
        imp.entry.set_primary_icon_name(icon);
        imp.entry.set_primary_icon_tooltip_text(tooltip);
        self.show_in_use();
    }

    /// The in-use icon and its tooltip while the page captures, from
    /// [`crate::permissions::indicator`].
    pub(crate) fn set_in_use(&self, indicator: Option<(&str, String)>) {
        let imp = self.imp();
        imp.capturing.set(indicator.is_some());
        if let Some((icon, tooltip)) = indicator {
            imp.in_use.set_icon_name(icon);
            imp.in_use.set_tooltip_text(Some(&tooltip));
        }
        self.show_in_use();
    }

    fn show_in_use(&self) {
        let imp = self.imp();
        let shown = imp.capturing.get() && !imp.editing.get();
        imp.in_use.set_visible(shown);
        // The text starts after the button rather than run under it.
        let reserve = if shown { imp.in_use.measure(gtk::Orientation::Horizontal, -1).1 + ICON_GAP } else { 0 };
        if let Some(text) = imp.entry.delegate() {
            text.set_margin_start(reserve);
        }
    }

    /// Whether the shown page is bookmarked, as the star at the entry's end.
    pub(crate) fn set_starred(&self, starred: bool) {
        let (icon, tooltip) = if starred {
            ("starred-symbolic", "Edit Bookmark")
        } else {
            ("non-starred-symbolic", "Bookmark This Page")
        };
        let entry = &self.imp().entry;
        entry.set_secondary_icon_name(Some(icon));
        entry.set_secondary_icon_tooltip_text(Some(tooltip));
    }

    /// Shows the zoom level before the star unless it is 100%, and in the zoom bubble.
    pub(crate) fn set_zoom(&self, level: f64) {
        let imp = self.imp();
        let percent = zoom::percent(level);
        let zoomed = (level - zoom::DEFAULT).abs() > 0.001;
        imp.zoom.set_label(&percent);
        imp.zoom.set_visible(zoomed);
        // The text stops short of the zoom level rather than run under it.
        let reserve = if zoomed { imp.zoom.measure(gtk::Orientation::Horizontal, -1).1 + ICON_GAP } else { 0 };
        if let Some(text) = imp.entry.delegate() {
            text.set_margin_end(reserve);
        }
        if let Some(label) = imp.zoom_label.upgrade() {
            label.set_label(&percent);
        }
    }

    fn show_zoom_bubble(&self) {
        let level = gtk::Label::builder()
            .label(self.imp().zoom.label().unwrap_or_default())
            .width_chars(5)
            .css_classes(["numeric", "heading"])
            .build();
        let step = |icon: &str, action: &str, tooltip: &str| {
            gtk::Button::builder().icon_name(icon).action_name(action).tooltip_text(tooltip).build()
        };
        let steps = gtk::Box::builder().css_classes(["linked"]).build();
        steps.append(&step("zoom-out-symbolic", "win.zoom-out", "Zoom Out"));
        steps.append(&step("zoom-in-symbolic", "win.zoom-in", "Zoom In"));
        let reset = gtk::Button::builder().label("Reset").action_name("win.zoom-reset").tooltip_text("Reset to 100%").build();
        let content = gtk::Box::builder().spacing(8).margin_start(6).margin_end(6).margin_top(4).margin_bottom(4).build();
        content.append(&gtk::Label::new(Some("Zoom:")));
        content.append(&level);
        content.append(&steps);
        content.append(&reset);
        let popover = gtk::Popover::builder().child(&content).css_classes(["zoom-bubble"]).build();
        self.imp().zoom_label.set(Some(&level));
        self.show_popover(&popover, Anchor::Zoom);
    }

    /// Opens `popover` pointing at `anchor`, closing any other bubble.
    pub(crate) fn show_popover(&self, popover: &gtk::Popover, anchor: Anchor) {
        let imp = self.imp();
        if let Some(previous) = imp.bubble.take() {
            previous.popdown();
        }
        popover.set_pointing_to(Some(&self.anchor_rect(anchor)));
        popover.set_position(gtk::PositionType::Bottom);
        popover.connect_closed(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |popover| {
                let imp = bar.imp();
                if imp.bubble.borrow().as_ref() == Some(popover) {
                    imp.bubble.take();
                }
            }
        ));
        imp.bubble.replace(Some(popover.clone()));
        crate::popup(popover, self);
    }

    /// The zoom level shown before the star, once it is laid out, if the page is zoomed.
    #[cfg(any(test, feature = "self-test"))]
    pub(crate) fn shown_zoom(&self) -> Option<String> {
        let zoom = &self.imp().zoom;
        (zoom.is_mapped() && zoom.width() > 0).then(|| zoom.label().unwrap_or_default().into())
    }

    /// Clicks the security icon, as the user would.
    #[cfg(feature = "self-test")]
    pub(crate) fn click_security(&self) {
        self.imp().entry.emit_by_name::<()>("icon-release", &[&gtk::EntryIconPosition::Primary]);
    }

    /// The in-use button's tooltip, once it is laid out.
    #[cfg(feature = "self-test")]
    pub(crate) fn shown_in_use(&self) -> Option<String> {
        let in_use = &self.imp().in_use;
        (in_use.is_mapped() && in_use.width() > 0).then(|| in_use.tooltip_text().unwrap_or_default().into())
    }

    /// Clicks the in-use button, as the user would.
    #[cfg(feature = "self-test")]
    pub(crate) fn click_in_use(&self) {
        self.imp().in_use.emit_clicked();
    }

    /// Clicks the zoom level, as the user would.
    #[cfg(feature = "self-test")]
    pub(crate) fn click_zoom(&self) {
        self.imp().zoom.emit_clicked();
    }

    /// The bubble open from the address bar, if any.
    #[cfg_attr(not(any(test, feature = "self-test")), allow(dead_code))]
    pub(crate) fn bubble(&self) -> Option<gtk::Popover> {
        self.imp().bubble.borrow().clone()
    }

    /// Where `anchor` is, in the bar's coordinates.
    fn anchor_rect(&self, anchor: Anchor) -> gdk::Rectangle {
        let imp = self.imp();
        let (widget, rect): (&gtk::Widget, gdk::Rectangle) = match anchor {
            Anchor::Security => (imp.entry.upcast_ref(), imp.entry.icon_area(gtk::EntryIconPosition::Primary)),
            Anchor::Star => (imp.entry.upcast_ref(), imp.entry.icon_area(gtk::EntryIconPosition::Secondary)),
            Anchor::Zoom => (imp.zoom.upcast_ref(), gdk::Rectangle::new(0, 0, imp.zoom.width(), imp.zoom.height())),
        };
        let origin = widget
            .compute_point(self, &gtk::graphene::Point::new(rect.x() as f32, rect.y() as f32))
            .map_or((rect.x(), rect.y()), |p| (p.x() as i32, p.y() as i32));
        gdk::Rectangle::new(origin.0, origin.1, rect.width().max(1), rect.height().max(1))
    }

    /// Centered while the entry shows the page's address, at the start while the user is in
    /// it or has typed something.
    fn align_text(&self) {
        let imp = self.imp();
        let editing = imp.focused.get() || imp.editing.get();
        EditableExt::set_alignment(&imp.entry, if editing { 0.0 } else { 0.5 });
    }

    /// Replaces the suggestion rows. The popover opens only while the entry has focus.
    pub(crate) fn set_suggestions(&self, items: Vec<Suggestion>) {
        let imp = self.imp();
        imp.list.remove_all();
        for item in &items {
            imp.list.append(&suggestion_row(item));
        }
        if items.is_empty() || !imp.focused.get() {
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
            self.align_text();
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
        self.show_resting();
        (suggestion.activate)();
    }

    fn cancel(&self) {
        self.finish_editing();
        self.show_resting();
        self.emit_by_name::<()>("cancelled", &[]);
    }

    fn finish_editing(&self) {
        let imp = self.imp();
        imp.editing.set(false);
        imp.popover.popdown();
        self.show_security();
        self.align_text();
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

/// Where the zoom level goes in the entry: just before the star, vertically centered.
fn before_star(entry: &gtk::Entry, child: &gtk::Widget) -> gdk::Rectangle {
    let (_, width, ..) = child.measure(gtk::Orientation::Horizontal, -1);
    let (_, height, ..) = child.measure(gtk::Orientation::Vertical, -1);
    let star = entry.icon_area(gtk::EntryIconPosition::Secondary);
    let x = if entry.direction() == gtk::TextDirection::Rtl {
        star.x() + star.width() + ICON_GAP
    } else {
        star.x() - width - ICON_GAP
    };
    gdk::Rectangle::new(x, (entry.height() - height) / 2, width, height)
}

/// Where the in-use button goes in the entry: just after the security icon, vertically
/// centered.
fn after_security(entry: &gtk::Entry, child: &gtk::Widget) -> gdk::Rectangle {
    let (_, width, ..) = child.measure(gtk::Orientation::Horizontal, -1);
    let (_, height, ..) = child.measure(gtk::Orientation::Vertical, -1);
    let security = entry.icon_area(gtk::EntryIconPosition::Primary);
    let x = if entry.direction() == gtk::TextDirection::Rtl {
        security.x() - width - ICON_GAP
    } else {
        security.x() + security.width() + ICON_GAP
    };
    gdk::Rectangle::new(x, (entry.height() - height) / 2, width, height)
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
    use crate::test_support::{Reply, Server, browser, wait_until};
    use crate::window::{BrowserWindow, Focus};

    #[test]
    fn security_follows_the_scheme() {
        assert_eq!(Security::of(Some("https://example.com/")), Security::Secure);
        assert_eq!(
            Security::of(Some("http://example.com/")),
            Security::Insecure
        );
        assert_eq!(Security::of(Some("file:///tmp/x")), Security::Internal);
        assert_eq!(Security::of(Some("about:version")), Security::Internal);
        assert_eq!(Security::of(Some("about:blank")), Security::NotApplicable);
        assert_eq!(Security::of(None), Security::NotApplicable);
    }

    #[test]
    fn simplifying_keeps_the_decoded_form() {
        let simplified = |_uri: &str, display: &str| simplified_url(display);
        assert_eq!(
            simplified("https://www.xn--e1afmkfd.xn--j1amh/", "https://www.пример.укр/"),
            "пример.укр"
        );
        assert_eq!(
            simplified("https://uk.wikipedia.org/wiki/%D0%9A%D0%B8%D1%97%D0%B2", "https://uk.wikipedia.org/wiki/Київ"),
            "uk.wikipedia.org/wiki/Київ"
        );
        assert_eq!(simplified("https://example.com/a/", "https://example.com/a/"), "example.com/a/");
        assert_eq!(simplified("https://www.com/", "https://www.com/"), "www.com");
        assert_eq!(simplified("http://www.example.com/", "http://www.example.com/"), "http://www.example.com/");
        assert_eq!(simplified("file:///tmp/x", "file:///tmp/x"), "file:///tmp/x");
    }

    fn bar_in_window() -> (gtk::Window, AddressBar, gtk::Button) {
        let bar = AddressBar::new();
        let other = gtk::Button::with_label("Elsewhere");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&bar);
        content.append(&other);
        let window = gtk::Window::builder().child(&content).build();
        window.present();
        other.grab_focus();
        (window, bar, other)
    }

    fn selected(bar: &AddressBar) -> Option<(i32, i32)> {
        bar.imp().entry.selection_bounds()
    }

    #[gtk::test]
    fn focus_shows_the_whole_url_selected_and_leaving_simplifies_it() {
        let (window, bar, other) = bar_in_window();
        let entry = bar.imp().entry.clone();
        bar.show_uri(Some("https://www.example.com/"));
        let unfocused = entry.text();

        entry.grab_focus();
        wait_until("the whole URL, selected", || {
            entry.text() == "https://www.example.com/" && selected(&bar).is_some()
        });
        let focused_selection = selected(&bar);

        other.grab_focus();
        let after_leaving = entry.text();

        bar.set_full_urls(true);
        let full = entry.text();
        bar.set_full_urls(false);

        entry.grab_focus();
        entry.set_text("typed");
        other.grab_focus();
        let typed = entry.text();

        bar.restore(None, Some("about:blank"));
        let blank = entry.text();
        window.destroy();

        assert_eq!(unfocused, "example.com");
        assert_eq!(focused_selection, Some((0, 24)));
        assert_eq!(after_leaving, "example.com");
        assert_eq!(full, "https://www.example.com/");
        assert_eq!(typed, "typed", "leaving keeps what the user typed");
        assert_eq!(blank, "");
    }

    #[gtk::test]
    fn the_address_is_centered_until_the_user_is_in_the_entry() {
        let (window, bar, other) = bar_in_window();
        let entry = bar.imp().entry.clone();
        bar.show_uri(Some("https://www.example.com/"));
        let resting = EditableExt::alignment(&entry);

        entry.grab_focus();
        wait_until("the entry to take the focus", || bar.imp().focused.get());
        let focused = EditableExt::alignment(&entry);

        other.grab_focus();
        wait_until("the entry to lose the focus", || !bar.imp().focused.get());
        let left = EditableExt::alignment(&entry);

        entry.grab_focus();
        entry.set_text("typed");
        other.grab_focus();
        wait_until("the entry to lose the focus", || !bar.imp().focused.get());
        let typed = EditableExt::alignment(&entry);

        bar.restore(None, Some("https://example.com/"));
        let restored = EditableExt::alignment(&entry);
        window.destroy();

        assert_eq!(resting, 0.5);
        assert_eq!(focused, 0.0);
        assert_eq!(left, 0.5);
        assert_eq!(typed, 0.0, "text the user typed starts at the left");
        assert_eq!(restored, 0.5);
    }

    /// The button labelled `label` in `widget`'s tree.
    fn button_in(widget: &gtk::Widget, label: &str) -> Option<gtk::Button> {
        if let Some(button) = widget.downcast_ref::<gtk::Button>()
            && button.label().as_deref() == Some(label)
        {
            return Some(button.clone());
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            if let Some(found) = button_in(&c, label) {
                return Some(found);
            }
            child = c.next_sibling();
        }
        None
    }

    fn heading_of(bubble: &gtk::Popover) -> Option<String> {
        fn find(widget: &gtk::Widget) -> Option<String> {
            if let Some(label) = widget.downcast_ref::<gtk::Label>()
                && label.has_css_class("heading")
            {
                return Some(label.label().into());
            }
            let mut child = widget.first_child();
            while let Some(c) = child {
                if let Some(found) = find(&c) {
                    return Some(found);
                }
                child = c.next_sibling();
            }
            None
        }
        find(bubble.upcast_ref())
    }

    #[gtk::test]
    fn the_star_bookmarks_the_page_and_opens_its_bubble() {
        let server = Server::start("127.0.0.1", |path| match path {
            "/starred" => Reply::Page("Starred"),
            _ => Reply::NotFound,
        });
        let browser = browser();
        let url = server.url("/starred");
        let window = BrowserWindow::new(&browser);
        window.present();
        let tab = window.open_tab(Some(&url), None, Focus::Foreground);
        wait_until("the page to commit", || tab.committed_uri().as_deref() == Some(url.as_str()));
        let bar = window.address_bar().clone();
        let entry = bar.imp().entry.clone();
        let state = || {
            let icon = entry.secondary_icon_name().map(String::from);
            (browser.is_bookmarked(Some(&url)), icon.unwrap_or_default())
        };
        let click = |position: gtk::EntryIconPosition| entry.emit_by_name::<()>("icon-release", &[&position]);

        let before = state();
        click(gtk::EntryIconPosition::Secondary);
        let added = (state(), bar.bubble().as_ref().and_then(heading_of));
        click(gtk::EntryIconPosition::Secondary);
        let bubble = bar.bubble().expect("the star opens a bubble");
        let again = (state(), heading_of(&bubble));
        button_in(bubble.upcast_ref(), "_Remove").expect("a Remove button").emit_clicked();
        let removed = state();
        window.destroy();

        let starred = (true, "starred-symbolic".to_owned());
        assert_eq!(before, (false, "non-starred-symbolic".to_owned()));
        assert_eq!(added, (starred.clone(), Some("Bookmark added".to_owned())));
        assert_eq!(again, (starred, Some("Edit bookmark".to_owned())), "a second click edits rather than removes");
        assert_eq!(removed, before);
    }
}
