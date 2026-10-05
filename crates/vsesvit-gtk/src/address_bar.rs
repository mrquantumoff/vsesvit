//! The address entry: shows the selected tab's committed URI and load progress, takes typed
//! input, and offers suggestions in a popover below it. Unless full URLs are on, the URI reads
//! simplified (`example.com` for `https://www.example.com/`) while the entry does not have
//! focus.
//!
//! While the user edits, it works as Chrome's omnibox: the first suggestion is highlighted
//! and is what Enter opens, the typed text can be completed inline (the completion selected
//! after it), and Up, Down, Tab and Escape move the highlight, showing each row's text in the
//! entry without querying again.
//!
//! It emits `edited` for every change the user makes, with whether the change may be completed
//! inline, `submitted` when Enter is pressed while no suggestion shows (or Ctrl+Enter makes
//! the text a `.com` address), `cancelled` when Escape gives up editing, and `bubble-changed`
//! when a bubble opens from it or closes.
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
use std::time::Duration;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::subclass::Signal;
use gtk::{gdk, glib};
use vsesvit_core::address::simplified_url;
use vsesvit_core::search::ctrl_enter_url;
use vsesvit_core::suggest::Queries;

use crate::tab::display_uri;
use crate::zoom;

/// Where the load line starts, so a navigation shows at once.
const PROGRESS_START: f64 = 0.08;
/// How long the full load line stays after the load completed.
const PROGRESS_HOLD: Duration = Duration::from_millis(200);

/// Room between a button over the entry and the icon next to it.
const ICON_GAP: i32 = 2;

/// What a bubble from the address bar points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Anchor {
    Security,
    Zoom,
    Star,
}

/// One suggestion row. `activate` runs when the row is chosen; `forget`, which only history
/// rows have, deletes the row's page from history (Shift+Delete).
#[derive(Clone)]
pub(crate) struct Suggestion {
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) icon_name: &'static str,
    /// What the entry shows while the row is highlighted.
    pub(crate) fill: String,
    pub(crate) activate: Rc<dyn Fn()>,
    pub(crate) forget: Option<Rc<dyn Fn()>>,
}

/// The rows for what the user typed, and the text that completes it inline to the first row's
/// `fill`.
#[derive(Default)]
pub(crate) struct Suggestions {
    pub(crate) rows: Vec<Suggestion>,
    pub(crate) inline: Option<String>,
}

/// The user's edit of the address, from their first change until they submit or give it up.
struct Edit {
    /// What the user typed. The entry shows it while the first row is highlighted, followed by
    /// `inline`, selected.
    typed: String,
    inline: Option<String>,
    rows: Vec<Suggestion>,
    /// The row Enter opens: the first whenever the rows change, and a row whenever there are
    /// any.
    highlighted: usize,
}

impl Edit {
    fn new(typed: String) -> Self {
        Edit { typed, inline: None, rows: Vec::new(), highlighted: 0 }
    }

    /// The entry's text for the highlighted row, and where its selected part starts, in
    /// characters.
    fn shown(&self) -> (String, Option<i32>) {
        match (self.highlighted, &self.inline) {
            (0, Some(inline)) => (format!("{}{inline}", self.typed), Some(char_count(&self.typed))),
            (0, None) => (self.typed.clone(), None),
            (row, _) => (self.rows[row].fill.clone(), None),
        }
    }

    /// Moves the highlight by `delta` rows, stopping at the first and the last.
    fn move_highlight(&mut self, delta: isize) {
        let last = self.rows.len().saturating_sub(1);
        self.highlighted = self.highlighted.saturating_add_signed(delta).min(last);
    }

    /// Makes the inline completion part of the typed text, as Right or End does.
    fn accept_inline(&mut self) {
        if let Some(inline) = self.inline.take() {
            self.typed.push_str(&inline);
        }
    }
}

/// What the address bar shows while the user edits, for tests.
#[cfg(any(test, feature = "self-test"))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Observed {
    pub(crate) text: String,
    pub(crate) selection: Option<(i32, i32)>,
    pub(crate) highlighted: Option<i32>,
    pub(crate) fills: Vec<String>,
    pub(crate) open: bool,
}

fn char_count(text: &str) -> i32 {
    i32::try_from(text.chars().count()).unwrap_or(i32::MAX)
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
        /// The permission prompt, also on the security icon; the window shows it only while
        /// no bubble is open.
        pub(super) prompt: RefCell<Option<gtk::Popover>>,
        pub(super) popover: gtk::Popover,
        pub(super) list: gtk::ListBox,
        pub(super) shown: RefCell<Shown>,
        /// Set while the user edits the text.
        pub(super) edit: RefCell<Option<Edit>>,
        /// The search engine's suggestions asked for the edit; cancelled when it ends, the
        /// entry loses focus, or another tab's state shows.
        pub(super) queries: Queries,
        /// The user's latest change only removed text (Backspace, Delete, cut), so it gets
        /// no inline completion.
        pub(super) deleted: Cell<bool>,
        /// The entry has the keyboard focus, and so shows the whole URI.
        pub(super) focused: Cell<bool>,
        pub(super) full_urls: Cell<bool>,
        pub(super) security: Cell<Security>,
        pub(super) updating: Cell<bool>,
        /// Hides the full load line after a load completed.
        pub(super) progress_hide: RefCell<Option<glib::SourceId>>,
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
                        .param_types([String::static_type(), bool::static_type()])
                        .build(),
                    Signal::builder("submitted")
                        .param_types([String::static_type()])
                        .build(),
                    Signal::builder("cancelled").build(),
                    Signal::builder("bubble-changed").build(),
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
            self.prompt.take();
        }
    }

    impl WidgetImpl for AddressBar {}

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
        entry.add_css_class("address-entry");
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
        // A change that replaces text deletes, then inserts, before `changed`.
        if let Some(text) = entry.delegate() {
            text.connect_delete_text(glib::clone!(
                #[weak(rename_to = bar)]
                self,
                move |_, _, _| bar.imp().deleted.set(true)
            ));
            text.connect_insert_text(glib::clone!(
                #[weak(rename_to = bar)]
                self,
                move |_, _, _| bar.imp().deleted.set(false)
            ));
        }
        entry.connect_activate(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |_| bar.submit()
        ));
        list.connect_row_activated(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |_, row| {
                if let Ok(index) = usize::try_from(row.index()) {
                    bar.choose(index);
                }
            }
        ));

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| bar.key_pressed(key, modifiers)
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
        if !self.is_editing() {
            self.show_resting();
        }
    }

    /// Switches to another tab's state: its unsubmitted text if it had any, else its URI.
    pub(crate) fn restore(&self, typed: Option<String>, uri: Option<&str>) {
        let imp = self.imp();
        imp.shown.replace(Shown::of(uri));
        imp.queries.cancel();
        match typed {
            Some(typed) => {
                self.set_text_quietly(&typed);
                imp.edit.replace(Some(Edit::new(typed)));
            }
            None => {
                imp.edit.take();
                self.show_resting();
            }
        }
        imp.popover.popdown();
        self.show_security();
        self.align_text();
    }

    /// Whole URIs even while the entry does not have focus.
    pub(crate) fn set_full_urls(&self, full: bool) {
        let imp = self.imp();
        imp.full_urls.set(full);
        if !self.is_editing() {
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
            imp.queries.cancel();
        }
        self.align_text();
        if self.is_editing() {
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
                    if imp.focused.get() && !bar.is_editing() {
                        imp.entry.select_region(0, -1);
                    }
                }
            ));
        }
    }

    /// What the user typed and did not submit, if they have been editing.
    pub(crate) fn take_edit(&self) -> Option<String> {
        self.imp().edit.take().map(|edit| edit.typed)
    }

    /// The search engine's suggestions asked for while the user edits.
    pub(crate) fn queries(&self) -> &Queries {
        &self.imp().queries
    }

    fn is_editing(&self) -> bool {
        self.imp().edit.borrow().is_some()
    }

    pub(crate) fn focus_for_typing(&self) {
        let entry = &self.imp().entry;
        entry.grab_focus();
        entry.select_region(0, -1);
    }

    /// Enters `text` as if the user typed it and pressed Enter (the self-test's way
    /// through the omnibox).
    #[cfg(feature = "self-test")]
    pub(crate) fn submit_text(&self, text: &str) {
        self.imp().edit.replace(Some(Edit::new(text.to_owned())));
        self.set_text_quietly(text);
        self.submit();
    }

    /// Shows a load at `fraction`, which never moves back while the same load shows.
    pub(crate) fn set_progress(&self, fraction: f64) {
        let imp = self.imp();
        let shown = match imp.progress_hide.take() {
            Some(pending) => {
                pending.remove();
                0.0
            }
            None => imp.entry.progress_fraction(),
        };
        imp.entry
            .set_progress_fraction(fraction.max(shown).max(PROGRESS_START));
    }

    #[cfg(feature = "self-test")]
    pub(crate) fn progress(&self) -> f64 {
        self.imp().entry.progress_fraction()
    }

    /// The shown load completed: the line fills, then goes.
    pub(crate) fn finish_progress(&self) {
        let imp = self.imp();
        if imp.entry.progress_fraction() == 0.0 || imp.progress_hide.borrow().is_some() {
            return;
        }
        imp.entry.set_progress_fraction(1.0);
        let bar = self.downgrade();
        let pending = glib::timeout_add_local_once(PROGRESS_HOLD, move || {
            if let Some(bar) = bar.upgrade() {
                bar.imp().progress_hide.take();
                bar.imp().entry.set_progress_fraction(0.0);
            }
        });
        imp.progress_hide.replace(Some(pending));
    }

    /// Hides the load line at once, as for another tab.
    pub(crate) fn clear_progress(&self) {
        let imp = self.imp();
        if let Some(pending) = imp.progress_hide.take() {
            pending.remove();
        }
        imp.entry.set_progress_fraction(0.0);
    }

    /// The connection state of the shown URI. It is hidden while the user edits the text.
    pub(crate) fn set_security(&self, security: Security) {
        self.imp().security.set(security);
        self.show_security();
    }

    fn show_security(&self) {
        let imp = self.imp();
        let shown = if self.is_editing() {
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
        let shown = imp.capturing.get() && !self.is_editing();
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
                    bar.emit_by_name::<()>("bubble-changed", &[]);
                }
            }
        ));
        imp.bubble.replace(Some(popover.clone()));
        crate::popup(popover, self);
        self.emit_by_name::<()>("bubble-changed", &[]);
    }

    /// Opens a permission prompt on the security icon. Other bubbles stay as they are.
    pub(crate) fn show_prompt(&self, popover: &gtk::Popover) {
        let imp = self.imp();
        popover.set_pointing_to(Some(&self.anchor_rect(Anchor::Security)));
        popover.set_position(gtk::PositionType::Bottom);
        popover.connect_closed(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |popover| {
                let imp = bar.imp();
                if imp.prompt.borrow().as_ref() == Some(popover) {
                    imp.prompt.take();
                }
            }
        ));
        imp.prompt.replace(Some(popover.clone()));
        crate::popup(popover, self);
    }

    /// The permission prompt on screen, if any.
    #[cfg(any(test, feature = "self-test"))]
    pub(crate) fn prompt(&self) -> Option<gtk::Popover> {
        self.imp().prompt.borrow().clone().filter(|p| p.is_visible())
    }

    /// The zoom level shown before the star, once it is laid out, if the page is zoomed.
    #[cfg(feature = "self-test")]
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
        let editing = imp.focused.get() || self.is_editing();
        EditableExt::set_alignment(&imp.entry, if editing { 0.0 } else { 0.5 });
    }

    /// Replaces the rows for what the user typed: the first is highlighted, and the inline
    /// completion follows the typed text, selected. The popover opens only while the entry has
    /// focus.
    pub(crate) fn set_suggestions(&self, suggestions: Suggestions) {
        let Suggestions { rows, inline } = suggestions;
        {
            let mut edit = self.imp().edit.borrow_mut();
            let Some(edit) = edit.as_mut() else { return };
            self.list_rows(&rows);
            edit.rows = rows;
            edit.inline = inline;
            edit.highlighted = 0;
        }
        self.show_highlight(false);
        self.show_list();
    }

    /// Replaces the rows for the same typed text, as when the search engine's suggestions
    /// arrive, without disturbing the user: the highlighted row stays highlighted (found by its
    /// `fill`, else the first), and the entry keeps its text, inline completion and caret.
    pub(crate) fn refill_suggestions(&self, rows: Vec<Suggestion>) {
        {
            let mut edit = self.imp().edit.borrow_mut();
            let Some(edit) = edit.as_mut() else { return };
            let kept = edit.rows.get(edit.highlighted).and_then(|shown| rows.iter().position(|row| row.fill == shown.fill));
            self.list_rows(&rows);
            edit.rows = rows;
            edit.highlighted = kept.unwrap_or(0);
        }
        self.show_highlight(false);
        self.show_list();
    }

    fn list_rows(&self, rows: &[Suggestion]) {
        let list = &self.imp().list;
        list.remove_all();
        for row in rows {
            list.append(&suggestion_row(row));
        }
    }

    /// Opens the list while the entry has focus and there are rows, and closes it otherwise.
    fn show_list(&self) {
        let imp = self.imp();
        let any = imp.edit.borrow().as_ref().is_some_and(|edit| !edit.rows.is_empty());
        if any && imp.focused.get() {
            // As wide as the bar. The bar's layout manager allocates it, so there is no
            // allocation to follow.
            imp.popover.set_size_request(self.width(), -1);
            imp.popover.popup();
        } else {
            imp.popover.popdown();
        }
    }

    /// Shows the highlighted row in the list and its text in the entry. The caret goes to the
    /// end when the text changes, or when `caret_to_end` asks for it.
    fn show_highlight(&self, caret_to_end: bool) {
        let imp = self.imp();
        let Some((text, selected_from, row)) = imp.edit.borrow().as_ref().map(|edit| {
            let (text, selected_from) = edit.shown();
            (text, selected_from, edit.highlighted)
        }) else {
            return;
        };
        let changed = imp.entry.text() != text;
        if changed {
            self.set_text_quietly(&text);
        }
        match selected_from {
            Some(from) => imp.entry.select_region(from, -1),
            None if changed || caret_to_end => imp.entry.set_position(-1),
            None => {}
        }
        let row = imp.list.row_at_index(i32::try_from(row).unwrap_or(i32::MAX));
        imp.list.select_row(row.as_ref());
    }

    /// Moves the highlight while the list is open, without querying again.
    fn change_highlight(&self, change: impl FnOnce(&mut Edit)) {
        if let Some(edit) = self.imp().edit.borrow_mut().as_mut() {
            change(edit);
        }
        self.show_highlight(true);
    }

    /// Emits `edited` again for the same typed text, as after a row was deleted.
    fn query_again(&self) {
        let again = self.imp().edit.borrow().as_ref().map(|edit| (edit.typed.clone(), edit.inline.is_some()));
        if let Some((typed, inline_allowed)) = again {
            self.emit_by_name::<()>("edited", &[&typed, &inline_allowed]);
        }
    }

    /// `f` gets the typed text and whether it may be completed inline: not after a change that
    /// only deleted, nor with the caret before the end.
    pub(crate) fn connect_edited<F: Fn(&Self, &str, bool) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "edited",
            false,
            glib::closure_local!(move |bar: &Self, text: String, inline_allowed: bool| f(bar, &text, inline_allowed)),
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

    pub(crate) fn connect_bubble_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "bubble-changed",
            false,
            glib::closure_local!(move |bar: &Self| f(bar)),
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
        let inline_allowed = !imp.deleted.get() && imp.entry.position() == char_count(text);
        let started = {
            let mut edit = imp.edit.borrow_mut();
            match edit.as_mut() {
                Some(edit) => {
                    edit.typed = text.to_owned();
                    edit.inline = None;
                    edit.highlighted = 0;
                    false
                }
                None => {
                    *edit = Some(Edit::new(text.to_owned()));
                    true
                }
            }
        };
        if started {
            self.show_security();
            self.align_text();
        }
        imp.list.select_row(imp.list.row_at_index(0).as_ref());
        self.emit_by_name::<()>("edited", &[&text.to_owned(), &inline_allowed]);
    }

    /// Enter: the highlighted row while the list shows, else the text as it reads.
    fn submit(&self) {
        let imp = self.imp();
        let highlighted = imp.edit.borrow().as_ref().filter(|edit| !edit.rows.is_empty()).map(|edit| edit.highlighted);
        if let Some(row) = highlighted.filter(|_| imp.popover.is_visible()) {
            self.choose(row);
            return;
        }
        let text: String = imp.entry.text().into();
        self.finish_editing();
        self.emit_by_name::<()>("submitted", &[&text]);
    }

    /// Ctrl+Enter: `www.<typed>.com` for a single word, else what Enter does.
    fn submit_as_com(&self) {
        let typed = self.imp().edit.borrow().as_ref().map(|edit| edit.typed.clone());
        match typed.as_deref().and_then(ctrl_enter_url) {
            Some(url) => {
                self.finish_editing();
                self.emit_by_name::<()>("submitted", &[&url.to_string()]);
            }
            None => self.submit(),
        }
    }

    fn choose(&self, index: usize) {
        let chosen = self.imp().edit.borrow().as_ref().and_then(|edit| edit.rows.get(index).cloned());
        let Some(suggestion) = chosen else { return };
        self.finish_editing();
        self.show_resting();
        (suggestion.activate)();
    }

    /// Shift+Delete: deletes the highlighted row's page from history, if it is a history row,
    /// and shows the rows for the same text again.
    fn forget_highlighted(&self) {
        let forget = self.imp().edit.borrow().as_ref().and_then(|edit| edit.rows.get(edit.highlighted)?.forget.clone());
        if let Some(forget) = forget {
            forget();
            self.query_again();
        }
    }

    fn cancel(&self) {
        self.finish_editing();
        self.show_resting();
        self.emit_by_name::<()>("cancelled", &[]);
    }

    fn finish_editing(&self) {
        let imp = self.imp();
        imp.edit.take();
        imp.queries.cancel();
        imp.popover.popdown();
        self.show_security();
        self.align_text();
    }

    fn key_pressed(&self, key: gdk::Key, modifiers: gdk::ModifierType) -> glib::Propagation {
        use gdk::Key;
        let imp = self.imp();
        let highlighted = imp.edit.borrow().as_ref().filter(|edit| !edit.rows.is_empty()).map(|edit| edit.highlighted);
        let open = imp.popover.is_visible() && highlighted.is_some();
        let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
        let plain = !modifiers.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK);
        // Up and Down never move focus out of the address bar, as they would in a plain entry.
        match key {
            Key::Return | Key::KP_Enter | Key::ISO_Enter if ctrl => self.submit_as_com(),
            Key::Down | Key::KP_Down if open => self.change_highlight(|edit| edit.move_highlight(1)),
            Key::Up | Key::KP_Up if open => self.change_highlight(|edit| edit.move_highlight(-1)),
            Key::Tab | Key::ISO_Left_Tab if open && plain => {
                let delta = if shift || key == Key::ISO_Left_Tab { -1 } else { 1 };
                self.change_highlight(|edit| edit.move_highlight(delta));
            }
            Key::Down | Key::KP_Down | Key::Up | Key::KP_Up => {}
            Key::Escape if open && highlighted != Some(0) => self.change_highlight(|edit| edit.highlighted = 0),
            Key::Escape => self.cancel(),
            Key::Delete | Key::KP_Delete if open && shift => self.forget_highlighted(),
            Key::Right | Key::KP_Right | Key::End | Key::KP_End if !shift => {
                // The entry then moves the caret past the completion, which leaves it typed.
                if let Some(edit) = imp.edit.borrow_mut().as_mut().filter(|edit| edit.highlighted == 0) {
                    edit.accept_inline();
                }
                return glib::Propagation::Proceed;
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// Types `text` at the caret over any selection, as the keyboard does (in two changes
    /// rather than one).
    #[cfg(any(test, feature = "self-test"))]
    pub(crate) fn type_text(&self, text: &str) {
        let entry = &self.imp().entry;
        entry.delete_selection();
        if let Some(delegate) = entry.delegate() {
            delegate.emit_by_name::<()>("insert-at-cursor", &[&text]);
        }
    }

    /// Presses `key` in the entry, as the user would; Enter ends in the entry's activation.
    #[cfg(any(test, feature = "self-test"))]
    pub(crate) fn press(&self, key: gdk::Key, modifiers: gdk::ModifierType) {
        let proceeded = self.key_pressed(key, modifiers) == glib::Propagation::Proceed;
        if proceeded && matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) {
            self.imp().entry.emit_activate();
        }
    }

    /// What the entry and the list show.
    #[cfg(any(test, feature = "self-test"))]
    pub(crate) fn observe(&self) -> Observed {
        let imp = self.imp();
        Observed {
            text: imp.entry.text().into(),
            selection: imp.entry.selection_bounds(),
            highlighted: imp.list.selected_row().map(|row| row.index()),
            fills: imp.edit.borrow().as_ref().map(|edit| edit.rows.iter().map(|row| row.fill.clone()).collect()).unwrap_or_default(),
            open: imp.popover.is_visible(),
        }
    }

    #[cfg(feature = "self-test")]
    pub(crate) fn suggestions_popover(&self) -> gtk::Popover {
        self.imp().popover.clone()
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

    fn row(fill: &str) -> Suggestion {
        Suggestion {
            title: String::new(),
            subtitle: String::new(),
            icon_name: "",
            fill: fill.to_owned(),
            activate: Rc::new(|| {}),
            forget: None,
        }
    }

    #[test]
    fn the_highlight_stops_at_both_ends() {
        let mut edit = Edit::new("gi".to_owned());
        edit.rows = vec![row("github.com"), row("gi"), row("gitlab.com")];
        edit.inline = Some("thub.com".to_owned());
        let mut seen = Vec::new();
        for delta in [-1, 1, 1, 1, -1, -1, -1] {
            edit.move_highlight(delta);
            seen.push((edit.highlighted, edit.shown()));
        }
        let first = (0, ("github.com".to_owned(), Some(2)));
        assert_eq!(seen[0], first, "no row above the first");
        assert_eq!(seen[1], (1, ("gi".to_owned(), None)));
        assert_eq!(seen[3], (2, ("gitlab.com".to_owned(), None)), "no row below the last");
        assert_eq!(seen[6], first, "back on the first row, the completion is selected again");
        edit.accept_inline();
        assert_eq!(edit.shown(), ("github.com".to_owned(), None));
    }

    /// Rows for a bar that stands in for the browser: the completion to `github.com`, the
    /// typed text as a search, and a history row that records being forgotten.
    struct Omnibox {
        queries: Rc<RefCell<Vec<(String, bool)>>>,
        chosen: Rc<RefCell<Vec<String>>>,
        submitted: Rc<RefCell<Vec<String>>>,
        forgotten: Rc<Cell<u32>>,
    }

    fn connect_omnibox(bar: &AddressBar) -> Omnibox {
        let omnibox = Omnibox {
            queries: Rc::default(),
            chosen: Rc::default(),
            submitted: Rc::default(),
            forgotten: Rc::default(),
        };
        let row = |fill: &str, chosen: &Rc<RefCell<Vec<String>>>, forget: Option<Rc<dyn Fn()>>| {
            let (fill, chosen) = (fill.to_owned(), chosen.clone());
            Suggestion {
                title: fill.clone(),
                subtitle: String::new(),
                icon_name: "web-browser-symbolic",
                fill: fill.clone(),
                activate: Rc::new(move || chosen.borrow_mut().push(fill.clone())),
                forget,
            }
        };
        let (queries, chosen, forgotten) = (omnibox.queries.clone(), omnibox.chosen.clone(), omnibox.forgotten.clone());
        bar.connect_edited(move |bar, text, inline_allowed| {
            queries.borrow_mut().push((text.to_owned(), inline_allowed));
            let site = "github.com";
            let inline = site.strip_prefix(text).filter(|rest| inline_allowed && !rest.is_empty());
            let forgotten = forgotten.clone();
            let forget: Rc<dyn Fn()> = Rc::new(move || forgotten.set(forgotten.get() + 1));
            bar.set_suggestions(Suggestions {
                rows: vec![row(site, &chosen, None), row(text, &chosen, None), row("gitlab.com", &chosen, Some(forget))],
                inline: inline.map(str::to_owned),
            });
        });
        let submitted = omnibox.submitted.clone();
        bar.connect_submitted(move |_, text| submitted.borrow_mut().push(text.to_owned()));
        omnibox
    }

    fn typing_in(bar: &AddressBar) {
        let entry = bar.imp().entry.clone();
        entry.grab_focus();
        wait_until("the entry to take the focus", || bar.imp().focused.get());
    }

    fn shows(text: &str, selection: Option<(i32, i32)>, highlighted: i32) -> (String, Option<(i32, i32)>, Option<i32>, bool) {
        (text.to_owned(), selection, Some(highlighted), true)
    }

    fn seen(bar: &AddressBar) -> (String, Option<(i32, i32)>, Option<i32>, bool) {
        let o = bar.observe();
        (o.text, o.selection, o.highlighted, o.open)
    }

    const NONE: gdk::ModifierType = gdk::ModifierType::empty();

    #[gtk::test]
    fn typing_completes_inline_and_the_keys_move_the_highlight_without_querying() {
        let (window, bar, _) = bar_in_window();
        let omnibox = connect_omnibox(&bar);
        typing_in(&bar);

        bar.type_text("git");
        let typed = seen(&bar);
        let queries = omnibox.queries.borrow().len();
        bar.press(gdk::Key::Down, NONE);
        let down = seen(&bar);
        bar.press(gdk::Key::Tab, NONE);
        let tab = seen(&bar);
        bar.press(gdk::Key::Down, NONE);
        let clamped = seen(&bar);
        bar.press(gdk::Key::ISO_Left_Tab, gdk::ModifierType::SHIFT_MASK);
        let shift_tab = seen(&bar);
        bar.press(gdk::Key::Up, NONE);
        bar.press(gdk::Key::Up, NONE);
        let top = seen(&bar);
        let moves_queried = omnibox.queries.borrow().len() - queries;
        bar.press(gdk::Key::Down, NONE);
        bar.press(gdk::Key::Down, NONE);
        bar.press(gdk::Key::Escape, NONE);
        let escaped = seen(&bar);
        bar.press(gdk::Key::Escape, NONE);
        let cancelled = (bar.is_editing(), bar.observe().open);
        let last_query = omnibox.queries.borrow().last().cloned();
        window.destroy();

        assert_eq!(last_query, Some(("git".to_owned(), true)));
        assert_eq!(typed, shows("github.com", Some((3, 10)), 0), "the completion follows the typed text, selected");
        assert_eq!(down, shows("git", None, 1));
        assert_eq!(tab, shows("gitlab.com", None, 2));
        assert_eq!(clamped, shows("gitlab.com", None, 2), "no row below the last");
        assert_eq!(shift_tab, shows("git", None, 1));
        assert_eq!(top, shows("github.com", Some((3, 10)), 0), "the first row brings the completion back");
        assert_eq!(moves_queried, 0, "moving the highlight does not query");
        assert_eq!(escaped, shows("github.com", Some((3, 10)), 0), "Escape goes back to the first row");
        assert_eq!(cancelled, (false, false), "Escape on the first row gives up editing");
    }

    #[gtk::test]
    fn deleting_gets_no_inline_completion() {
        let (window, bar, _) = bar_in_window();
        let omnibox = connect_omnibox(&bar);
        typing_in(&bar);
        let text = bar.imp().entry.delegate().expect("the entry's text");

        bar.type_text("git");
        text.emit_by_name::<()>("backspace", &[]);
        let completion_removed = seen(&bar);
        text.emit_by_name::<()>("backspace", &[]);
        let backspaced = seen(&bar);
        bar.type_text("t");
        let typed_again = seen(&bar);
        bar.imp().entry.set_position(0);
        bar.type_text("x");
        let typed_before_the_end = seen(&bar);
        let queries = omnibox.queries.borrow().clone();
        window.destroy();

        assert_eq!(completion_removed, shows("git", None, 0), "Backspace removes the completion only");
        assert_eq!(backspaced, shows("gi", None, 0));
        assert_eq!(typed_again, shows("github.com", Some((3, 10)), 0));
        assert_eq!(typed_before_the_end.0, "xgithub.com");
        assert_eq!(
            queries[queries.len() - 4..],
            [("git".to_owned(), false), ("gi".to_owned(), false), ("git".to_owned(), true), ("xgithub.com".to_owned(), false)]
        );
    }

    #[gtk::test]
    fn enter_opens_the_highlighted_row_and_right_keeps_the_completion() {
        let (window, bar, _) = bar_in_window();
        let omnibox = connect_omnibox(&bar);
        typing_in(&bar);
        let text = bar.imp().entry.delegate().expect("the entry's text");

        bar.type_text("git");
        let queries = omnibox.queries.borrow().len();
        bar.press(gdk::Key::Right, NONE);
        text.emit_by_name::<()>("move-cursor", &[&gtk::MovementStep::VisualPositions, &1i32, &false]);
        let accepted = seen(&bar);
        bar.press(gdk::Key::Down, NONE);
        bar.press(gdk::Key::Up, NONE);
        let back = seen(&bar);
        let right_queried = omnibox.queries.borrow().len() - queries;
        bar.press(gdk::Key::Return, NONE);
        let first = omnibox.chosen.borrow().clone();

        typing_in(&bar);
        bar.type_text("gitl");
        bar.press(gdk::Key::Down, NONE);
        bar.press(gdk::Key::Down, NONE);
        bar.press(gdk::Key::KP_Enter, NONE);
        let chosen = omnibox.chosen.borrow().clone();
        let submitted = omnibox.submitted.borrow().len();
        window.destroy();

        assert_eq!(accepted, shows("github.com", None, 0), "Right moves past the completion");
        assert_eq!(back, shows("github.com", None, 0), "the accepted completion is typed text now");
        assert_eq!(right_queried, 0);
        assert_eq!(first, ["github.com"], "Enter opens the first row");
        assert_eq!(chosen, ["github.com", "gitlab.com"], "Enter opens the highlighted row");
        assert_eq!(submitted, 0, "rows open by themselves, not by resolving the text");
    }

    #[gtk::test]
    fn a_refill_keeps_the_highlighted_row_and_the_entry_as_they_are() {
        let (window, bar, _) = bar_in_window();
        let omnibox = connect_omnibox(&bar);
        typing_in(&bar);
        let refill = |fills: &[&str]| bar.refill_suggestions(fills.iter().map(|fill| row(fill)).collect());

        bar.type_text("git");
        let queries = omnibox.queries.borrow().len();
        refill(&["github.com", "git", "git one", "git two", "gitlab.com"]);
        let first = (seen(&bar), bar.observe().fills.len());
        bar.press(gdk::Key::Down, NONE);
        bar.press(gdk::Key::Down, NONE);
        let caret = bar.imp().entry.position();
        refill(&["github.com", "git", "git zero", "git one", "gitlab.com"]);
        let moved = (seen(&bar), bar.imp().entry.position());
        refill(&["github.com", "git", "gitlab.com"]);
        let gone = seen(&bar);
        let refills_queried = omnibox.queries.borrow().len() - queries;
        window.destroy();

        assert_eq!(first, (shows("github.com", Some((3, 10)), 0), 5), "the typed text and its completion stay");
        assert_eq!(moved, (shows("git one", None, 3), caret), "the highlighted row stays highlighted where it went");
        assert_eq!(gone, shows("github.com", Some((3, 10)), 0), "without it, the first row is highlighted");
        assert_eq!(refills_queried, 0);
    }

    #[gtk::test]
    fn ctrl_enter_opens_the_com_address_and_shift_delete_forgets_history_rows() {
        let (window, bar, _) = bar_in_window();
        let omnibox = connect_omnibox(&bar);
        typing_in(&bar);

        bar.type_text("example");
        bar.press(gdk::Key::Delete, gdk::ModifierType::SHIFT_MASK);
        let first_row_kept = (omnibox.forgotten.get(), bar.observe().text);
        bar.press(gdk::Key::Down, NONE);
        bar.press(gdk::Key::Down, NONE);
        let queries = omnibox.queries.borrow().len();
        bar.press(gdk::Key::Delete, gdk::ModifierType::SHIFT_MASK);
        let forgotten = omnibox.forgotten.get();
        let asked_again = omnibox.queries.borrow()[queries..].to_vec();
        let after = seen(&bar);
        bar.press(gdk::Key::Return, gdk::ModifierType::CONTROL_MASK);
        let com = omnibox.submitted.borrow().clone();

        typing_in(&bar);
        bar.imp().entry.select_region(0, -1);
        bar.type_text("two words");
        bar.press(gdk::Key::Return, gdk::ModifierType::CONTROL_MASK);
        let chosen = omnibox.chosen.borrow().clone();
        window.destroy();

        assert_eq!(first_row_kept, (0, "example".to_owned()), "Shift+Delete leaves other rows alone");
        assert_eq!(forgotten, 1);
        assert_eq!(asked_again, [("example".to_owned(), false)], "the rows are shown again for the same text");
        assert_eq!(after, shows("example", None, 0));
        assert_eq!(com, ["https://www.example.com/"]);
        assert_eq!(chosen, ["github.com"], "Ctrl+Enter on more than a word acts as Enter");
    }
}
