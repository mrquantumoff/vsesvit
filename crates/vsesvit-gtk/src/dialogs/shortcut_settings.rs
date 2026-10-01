//! Settings' Shortcuts page: one row per command this shell can reassign, grouped
//! by core's sections, and the dialog that captures a new shortcut. Every change goes through
//! [`Browser::edit_keymap`], so it is stored, synced and live in every window at once.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};
use vsesvit_core::shortcuts::{self, Chord, Command, Keymap, Section};

use super::confirm;
use crate::browser::Browser;
use crate::keymap::{self, Binding, Pressed};

struct Row {
    cmd: Command,
    row: adw::ActionRow,
    chords: adw::ShortcutLabel,
    reset: gtk::Button,
}

struct Page {
    browser: Browser,
    rows: Vec<Row>,
    reset_all: adw::ButtonRow,
}

impl Page {
    fn refresh(&self) {
        let keymap = self.browser.keymap();
        for row in &self.rows {
            row.chords.set_accelerator(&keymap::accelerators(keymap.chords(row.cmd)));
            row.reset.set_visible(!keymap.is_default(row.cmd));
        }
        self.reset_all.set_sensitive(keymap != Keymap::default());
    }
}

pub(super) fn page(browser: &Browser) -> adw::PreferencesPage {
    let preferences = adw::PreferencesPage::builder()
        .name("shortcuts")
        .title("Shortcuts")
        .icon_name("preferences-desktop-keyboard-shortcuts-symbolic")
        .build();
    let mut rows = Vec::new();
    for section in Section::ALL {
        let group = adw::PreferencesGroup::builder().title(section.title()).build();
        for (cmd, _) in keymap::actions().filter(|(cmd, _)| cmd.section() == section) {
            let row = adw::ActionRow::builder().title(cmd.title()).activatable(true).build();
            let chords = adw::ShortcutLabel::new("");
            chords.set_disabled_text("Disabled");
            chords.set_valign(gtk::Align::Center);
            let reset = gtk::Button::builder()
                .icon_name("edit-undo-symbolic")
                .tooltip_text("Reset to Default")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            row.add_suffix(&chords);
            row.add_suffix(&reset);
            group.add(&row);
            rows.push(Row { cmd, row, chords, reset });
        }
        preferences.add(&group);
    }
    let reset_all = adw::ButtonRow::builder().title("Reset All").build();
    let group = adw::PreferencesGroup::new();
    group.add(&reset_all);
    preferences.add(&group);

    let page = Rc::new(Page { browser: browser.clone(), rows, reset_all });
    page.refresh();
    let weak = Rc::downgrade(&page);
    browser.watch_prefs(move |_| weak.upgrade().inspect(|page| page.refresh()).is_some());
    for row in &page.rows {
        let cmd = row.cmd;
        row.row.connect_activated(glib::clone!(
            #[strong]
            page,
            move |row| {
                let refreshed = page.clone();
                capture(row, &page.browser, cmd, move || refreshed.refresh());
            }
        ));
        row.reset.connect_clicked(glib::clone!(
            #[strong]
            page,
            move |_| {
                page.browser.edit_keymap(|keymap| keymap.reset(cmd));
                page.refresh();
            }
        ));
    }
    page.reset_all.connect_activated(glib::clone!(
        #[strong]
        page,
        move |button| {
            let page = page.clone();
            let button = button.clone();
            glib::spawn_future_local(async move {
                if confirm(&button, "Reset All Shortcuts?", "Every command gets its default shortcuts back.", "_Reset").await {
                    page.browser.edit_keymap(Keymap::reset_all);
                    page.refresh();
                }
            });
        }
    ));
    preferences
}

/// The open capture dialog. The self-test feeds it key presses through [`Capture::press`].
pub(crate) struct Capture {
    dialog: adw::AlertDialog,
    browser: Browser,
    cmd: Command,
    preview: adw::ShortcutLabel,
    note: gtk::Label,
    chord: Cell<Option<Chord>>,
    done: Box<dyn Fn()>,
}

/// Asks for a new shortcut for `cmd`: the window's own shortcuts are off while it is open, so
/// any chord can be pressed. `done` runs after a change was saved.
pub(crate) fn capture(parent: &impl IsA<gtk::Widget>, browser: &Browser, cmd: Command, done: impl Fn() + 'static) -> Rc<Capture> {
    let current = keymap::accelerators(browser.keymap().chords(cmd));
    let preview = adw::ShortcutLabel::new(&current);
    preview.set_disabled_text("Disabled");
    preview.set_halign(gtk::Align::Center);
    let note = gtk::Label::builder()
        .label("Backspace removes the shortcut")
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["dim-label"])
        .build();
    let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12).build();
    content.append(&preview);
    content.append(&note);
    let dialog = adw::AlertDialog::new(Some("Set Shortcut"), Some(&format!("Press the new shortcut for {}", cmd.title())));
    dialog.set_extra_child(Some(&content));
    // No mnemonics: Alt and a letter is a shortcut being captured.
    dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("save", false);
    dialog.set_close_response("cancel");

    let capture = Rc::new(Capture { dialog: dialog.clone(), browser: browser.clone(), cmd, preview, note, chord: Cell::new(None), done: Box::new(done) });
    dialog.connect_response(
        Some("save"),
        glib::clone!(
            #[strong]
            capture,
            move |_, _| {
                if let Some(chord) = capture.chord.get() {
                    capture.save(&[chord]);
                }
            }
        ),
    );
    dialog.connect_closed(glib::clone!(
        #[strong]
        browser,
        move |_| browser.apply_keymap()
    ));
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(&capture);
    keys.connect_key_pressed(move |controller, keyval, _, state| {
        let (Some(capture), Some(event)) = (weak.upgrade(), controller.current_event().and_then(|event| event.downcast::<gdk::KeyEvent>().ok())) else {
            return glib::Propagation::Proceed;
        };
        if !event.is_modifier() {
            let base = controller
                .widget()
                .and_then(|widget| widget.display().translate_key(event.keycode(), gdk::ModifierType::empty(), i32::try_from(event.layout()).unwrap_or(0)))
                .map(|(base, ..)| base);
            capture.press(keymap::pressed(keyval, state, event.consumed_modifiers(), base));
        }
        glib::Propagation::Stop
    });
    dialog.add_controller(keys);

    keymap::suspend(browser.app());
    dialog.present(Some(parent));
    capture
}

impl Capture {
    pub(crate) fn press(&self, pressed: Pressed) {
        match pressed {
            Pressed::Cancel => {
                self.dialog.close();
            }
            Pressed::Clear => {
                self.save(&[]);
                self.dialog.close();
            }
            Pressed::Confirm => {
                if let Some(chord) = self.chord.get() {
                    self.save(&[chord]);
                    self.dialog.close();
                }
            }
            Pressed::Chord(chord) => {
                self.preview.set_accelerator(&keymap::accelerator(chord));
                self.preview.set_visible(true);
                let (chord, note) =
                    self.browser.keymap().offer(self.cmd, chord, |holder| matches!(keymap::binding(holder), Some(Binding::BuiltIn(_))));
                self.offer(chord, note.as_deref().unwrap_or(""));
            }
            Pressed::NeedsModifier => {
                self.preview.set_visible(false);
                self.offer(None, shortcuts::NEEDS_MODIFIER_NOTE);
            }
            Pressed::Unusable => {
                self.preview.set_visible(false);
                self.offer(None, shortcuts::NOT_A_KEY_NOTE);
            }
        }
    }

    #[cfg(feature = "self-test")]
    pub(crate) fn note(&self) -> String {
        self.note.label().into()
    }

    #[cfg(feature = "self-test")]
    pub(crate) fn close(&self) {
        self.dialog.close();
    }

    fn offer(&self, chord: Option<Chord>, note: &str) {
        self.chord.set(chord);
        self.note.set_label(note);
        self.note.set_visible(!note.is_empty());
        self.dialog.set_response_enabled("save", chord.is_some());
    }

    fn save(&self, chords: &[Chord]) {
        self.browser.edit_keymap(|keymap| keymap.assign(self.cmd, chords.iter().copied()));
        (self.done)();
    }
}
