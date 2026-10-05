//! Settings' Shortcuts page: one row per command this shell can reassign, grouped
//! by core's sections, then the enabled extensions' commands, and the dialog that captures a
//! new shortcut. Every change goes through [`Browser::edit_keymap`], so it is stored, synced
//! and live in every window at once.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};
use vsesvit_core::extensions::commands::{ExtensionCommand, ExtensionShortcuts};
use vsesvit_core::shortcuts::{self, Chord, Command, Keymap, Section};

use super::confirm;
use crate::browser::Browser;
use crate::keymap::{self, Binding, Pressed};

/// Chrome takes neither a function key alone nor Shift alone for an extension.
const EXTENSION_NEEDS_MODIFIER_NOTE: &str = "An extension's shortcut needs Ctrl or Alt";

/// What a row and the capture dialog change: one of the browser's commands, which may have
/// several chords, or an extension's, which has at most one.
#[derive(Clone)]
pub(crate) enum Target {
    Browser(Command),
    Extension(ExtensionCommand),
}

impl Target {
    fn title(&self) -> &str {
        match self {
            Target::Browser(cmd) => cmd.title(),
            Target::Extension(command) => command.title(),
        }
    }

    fn chords(&self, keymap: &Keymap, extensions: &ExtensionShortcuts) -> Vec<Chord> {
        match self {
            Target::Browser(cmd) => keymap.chords(*cmd).to_vec(),
            Target::Extension(c) => extensions.chord(&c.extension, &c.command.name).into_iter().collect(),
        }
    }

    fn is_default(&self, keymap: &Keymap, extensions: &ExtensionShortcuts) -> bool {
        match self {
            Target::Browser(cmd) => keymap.is_default(*cmd),
            Target::Extension(c) => extensions.is_default(&c.extension, &c.command.name),
        }
    }

    /// The chord saving would assign, if any, and a note about it.
    fn offer(&self, keymap: &Keymap, extensions: &ExtensionShortcuts, chord: Chord) -> (Option<Chord>, Option<String>) {
        match self {
            Target::Browser(cmd) => {
                let (offered, note) = keymap.offer(*cmd, chord, |holder| matches!(keymap::binding(holder), Some(Binding::BuiltIn(_))));
                (offered, note.or_else(|| extensions.note_for_browser(chord)))
            }
            Target::Extension(_) if !chord.mods().ctrl && !chord.mods().alt => (None, Some(EXTENSION_NEEDS_MODIFIER_NOTE.to_owned())),
            Target::Extension(c) => keymap.offer_extension(extensions, &c.extension, &c.command.name, chord),
        }
    }

    fn needs_modifier_note(&self) -> &'static str {
        match self {
            Target::Browser(_) => shortcuts::NEEDS_MODIFIER_NOTE,
            Target::Extension(_) => EXTENSION_NEEDS_MODIFIER_NOTE,
        }
    }

    /// Gives it exactly `chord`, or no shortcut.
    fn assign(&self, keymap: &mut Keymap, extensions: &ExtensionShortcuts, chord: Option<Chord>) {
        match self {
            Target::Browser(cmd) => {
                keymap.assign(*cmd, chord);
            }
            Target::Extension(c) => keymap.assign_extension(extensions, &c.extension, &c.command.name, chord),
        }
    }

    fn reset(&self, keymap: &mut Keymap, extensions: &ExtensionShortcuts) {
        match self {
            Target::Browser(cmd) => {
                keymap.reset(*cmd);
            }
            Target::Extension(c) => keymap.reset_extension(extensions, &c.extension, &c.command.name),
        }
    }
}

/// Changes the browser's keymap, with the extension commands' shortcuts as it resolves them.
fn edit(browser: &Browser, change: impl FnOnce(&mut Keymap, &ExtensionShortcuts)) {
    browser.edit_keymap(|keymap| {
        let extensions = browser.extension_shortcuts(keymap);
        change(keymap, &extensions);
    });
}

struct Row {
    target: Target,
    chords: glib::WeakRef<adw::ShortcutLabel>,
    reset: glib::WeakRef<gtk::Button>,
}

/// The rows' handlers hold it, so it holds the rows weakly.
struct Page {
    browser: Browser,
    rows: Vec<Row>,
    reset_all: glib::WeakRef<adw::ButtonRow>,
}

impl Page {
    fn refresh(&self) {
        let keymap = self.browser.keymap();
        let extensions = self.browser.extension_shortcuts(&keymap);
        for row in &self.rows {
            if let (Some(chords), Some(reset)) = (row.chords.upgrade(), row.reset.upgrade()) {
                chords.set_accelerator(&keymap::accelerators(&row.target.chords(&keymap, &extensions)));
                reset.set_visible(!row.target.is_default(&keymap, &extensions));
            }
        }
        if let Some(reset_all) = self.reset_all.upgrade() {
            reset_all.set_sensitive(keymap != Keymap::default());
        }
    }
}

pub(super) fn page(browser: &Browser) -> adw::PreferencesPage {
    let preferences = adw::PreferencesPage::builder()
        .name("shortcuts")
        .title("Shortcuts")
        .icon_name("preferences-desktop-keyboard-shortcuts-symbolic")
        .build();
    let mut widgets = Vec::new();
    let mut add = |group: &adw::PreferencesGroup, target: Target, subtitle: &str| {
        let row = adw::ActionRow::builder().title(target.title()).subtitle(subtitle).use_markup(false).activatable(true).build();
        let chords = adw::ShortcutLabel::new("");
        chords.set_disabled_text("Disabled");
        chords.set_valign(gtk::Align::Center);
        let reset = super::row_button("edit-undo-symbolic", "Reset to Default");
        row.add_suffix(&chords);
        row.add_suffix(&reset);
        group.add(&row);
        widgets.push((target, row, chords, reset));
    };
    for section in Section::ALL {
        let group = adw::PreferencesGroup::builder().title(section.title()).build();
        for (cmd, _) in keymap::actions().filter(|(cmd, _)| cmd.section() == section) {
            add(&group, Target::Browser(cmd), "");
        }
        preferences.add(&group);
    }
    let extensions = browser.extension_shortcuts(&browser.keymap());
    if extensions.iter().next().is_some() {
        let group = adw::PreferencesGroup::builder().title("Extension Shortcuts").build();
        for (command, _) in extensions.iter() {
            add(&group, Target::Extension(command.clone()), &command.extension_name);
        }
        preferences.add(&group);
    }
    let reset_all = adw::ButtonRow::builder().title("Reset All").build();
    let group = adw::PreferencesGroup::new();
    group.add(&reset_all);
    preferences.add(&group);

    let rows = widgets.iter().map(|(target, _, chords, reset)| Row { target: target.clone(), chords: chords.downgrade(), reset: reset.downgrade() }).collect();
    let page = Rc::new(Page { browser: browser.clone(), rows, reset_all: reset_all.downgrade() });
    page.refresh();
    let weak = Rc::downgrade(&page);
    browser.watch_prefs(move |_| weak.upgrade().inspect(|page| page.refresh()).is_some());
    for (target, row, _, reset) in widgets {
        row.connect_activated(glib::clone!(
            #[strong]
            page,
            #[strong]
            target,
            move |row| {
                let refreshed = page.clone();
                capture(row, &page.browser, target.clone(), move || refreshed.refresh());
            }
        ));
        reset.connect_clicked(glib::clone!(
            #[strong]
            page,
            move |_| {
                edit(&page.browser, |keymap, extensions| target.reset(keymap, extensions));
                page.refresh();
            }
        ));
    }
    reset_all.connect_activated(glib::clone!(
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
/// The dialog's handlers hold it, so it holds the dialog weakly.
pub(crate) struct Capture {
    dialog: glib::WeakRef<adw::AlertDialog>,
    browser: Browser,
    target: Target,
    preview: adw::ShortcutLabel,
    note: gtk::Label,
    chord: Cell<Option<Chord>>,
    done: Box<dyn Fn()>,
}

/// Asks for a new shortcut for `target`: the window's own shortcuts are off while it is open,
/// so any chord can be pressed. `done` runs after a change was saved.
pub(crate) fn capture(parent: &impl IsA<gtk::Widget>, browser: &Browser, target: Target, done: impl Fn() + 'static) -> Rc<Capture> {
    let keymap = browser.keymap();
    let current = keymap::accelerators(&target.chords(&keymap, &browser.extension_shortcuts(&keymap)));
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
    let dialog = adw::AlertDialog::new(Some("Set Shortcut"), Some(&format!("Press the new shortcut for {}", target.title())));
    dialog.set_extra_child(Some(&content));
    // No mnemonics: Alt and a letter is a shortcut being captured.
    dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("save", false);
    dialog.set_close_response("cancel");

    let capture = Rc::new(Capture { dialog: dialog.downgrade(), browser: browser.clone(), target, preview, note, chord: Cell::new(None), done: Box::new(done) });
    dialog.connect_response(
        Some("save"),
        glib::clone!(
            #[strong]
            capture,
            move |_, _| {
                if let Some(chord) = capture.chord.get() {
                    capture.save(Some(chord));
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
    pub(crate) fn close(&self) {
        if let Some(dialog) = self.dialog.upgrade() {
            dialog.close();
        }
    }

    pub(crate) fn press(&self, pressed: Pressed) {
        match pressed {
            Pressed::Cancel => {
                self.close();
            }
            Pressed::Clear => {
                self.save(None);
                self.close();
            }
            Pressed::Confirm => {
                if let Some(chord) = self.chord.get() {
                    self.save(Some(chord));
                    self.close();
                }
            }
            Pressed::Chord(chord) => {
                self.preview.set_accelerator(&keymap::accelerator(chord));
                self.preview.set_visible(true);
                let keymap = self.browser.keymap();
                let (chord, note) = self.target.offer(&keymap, &self.browser.extension_shortcuts(&keymap), chord);
                self.offer(chord, note.as_deref().unwrap_or(""));
            }
            Pressed::NeedsModifier => {
                self.preview.set_visible(false);
                self.offer(None, self.target.needs_modifier_note());
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

    fn offer(&self, chord: Option<Chord>, note: &str) {
        self.chord.set(chord);
        self.note.set_label(note);
        self.note.set_visible(!note.is_empty());
        if let Some(dialog) = self.dialog.upgrade() {
            dialog.set_response_enabled("save", chord.is_some());
        }
    }

    fn save(&self, chord: Option<Chord>) {
        edit(&self.browser, |keymap, extensions| self.target.assign(keymap, extensions, chord));
        (self.done)();
    }
}

#[cfg(test)]
mod tests {
    use vsesvit_core::extensions::ExtensionId;
    use vsesvit_core::extensions::manifest::ManifestCommand;

    use super::*;
    use crate::test_support::{browser, wait_until};
    use crate::window::BrowserWindow;

    #[gtk::test]
    fn the_page_and_a_closed_capture_let_the_browser_go() {
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        window.present();
        let held = || Rc::strong_count(&browser.0);
        let before = held();
        drop(page(&browser));
        wait_until("the page to let the browser go", || held() == before);
        let capture = capture(&window, &browser, Target::Browser(Command::ShowHistory), || {});
        capture.press(Pressed::Cancel);
        drop(capture);
        wait_until("the capture to let the browser go", || held() == before);
        window.destroy();
    }

    #[test]
    fn an_extension_shortcut_needs_ctrl_or_alt_and_the_browser_names_the_command_it_takes_from() {
        let chord = |text: &str| text.parse::<Chord>().expect("a chord");
        let command = ExtensionCommand {
            extension: ExtensionId::parse("commands@vsesvit.test").expect("an extension id"),
            extension_name: "Commands".to_owned(),
            command: ManifestCommand { name: "run".to_owned(), description: "Run it".to_owned(), suggested_key: Some(chord("Alt+Shift+R")) },
        };
        let keymap = Keymap::default();
        let extensions = keymap.extension_shortcuts(vec![command.clone()]);
        let run = Target::Extension(command.clone());
        let refused = (None, Some(EXTENSION_NEEDS_MODIFIER_NOTE.to_owned()));
        assert_eq!(run.offer(&keymap, &extensions, chord("F9")), refused);
        assert_eq!(run.offer(&keymap, &extensions, chord("Shift+F5")), refused);
        assert_eq!(run.offer(&keymap, &extensions, chord("Alt+Shift+Y")), (Some(chord("Alt+Shift+Y")), None));
        assert_eq!(run.offer(&keymap, &extensions, chord("Ctrl+T")).0, None, "the browser's chord");
        assert_eq!(run.chords(&keymap, &extensions), [chord("Alt+Shift+R")]);

        let note = "Also used by Commands: Run it. Saving moves it here.".to_owned();
        assert_eq!(Target::Browser(Command::ShowHistory).offer(&keymap, &extensions, chord("Alt+Shift+R")), (Some(chord("Alt+Shift+R")), Some(note)));

        let mut edited = keymap.clone();
        run.assign(&mut edited, &extensions, None);
        let removed = edited.extension_shortcuts(vec![command]);
        assert!(run.chords(&edited, &removed).is_empty() && !run.is_default(&edited, &removed));
        run.reset(&mut edited, &removed);
        assert_eq!(edited, keymap);
    }
}
