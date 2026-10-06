//! Core's shortcut commands on GTK: which action each one runs, chords as GTK accelerators,
//! and a key press as a chord for the capture dialog. Extensions' commands run
//! [`EXTENSION_COMMAND`], one detailed action per command.

use gtk::prelude::*;
use gtk::{gdk, gio};
use vsesvit_core::extensions::ExtensionId;
use vsesvit_core::extensions::commands::ExtensionShortcuts;
use vsesvit_core::shortcuts::{Chord, Command, Key, Keymap, Mods};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binding {
    /// An action whose accelerators follow the keymap.
    Action(&'static str),
    /// Built into a widget (`AdwTabView`) with a fixed accelerator: listed in the shortcuts
    /// help, never reassignable.
    BuiltIn(&'static str),
}

/// `None` for the commands this shell does not have.
pub(crate) fn binding(cmd: Command) -> Option<Binding> {
    use Binding::{Action, BuiltIn};
    use Command::*;
    Some(match cmd {
        NewTab => Action("win.new-tab"),
        CloseTab => Action("win.close-tab"),
        ReopenClosedTab => Action("win.reopen-closed-tab"),
        SearchTabs => Action("win.search-tabs"),
        NextTab => BuiltIn("<Control>Tab"),
        PreviousTab => BuiltIn("<Control><Shift>Tab"),
        SelectTab1 | SelectTab2 | SelectTab3 | SelectTab4 | SelectTab5 | SelectTab6 | SelectTab7 | SelectTab8
        | SelectLastTab => return None,
        ToggleTabList => Action("win.toggle-tab-sidebar"),
        NewWindow => Action("app.new-window"),
        NewPrivateWindow => Action("app.new-private-window"),
        Quit => Action("app.quit"),
        FocusAddress => Action("win.focus-location"),
        Back => Action("win.back"),
        Forward => Action("win.forward"),
        Reload => Action("win.reload"),
        ReloadBypassCache => Action("win.reload-bypass-cache"),
        SavePage => Action("win.save-page"),
        Find => Action("win.find"),
        FindNext => Action("win.find-next"),
        FindPrevious => Action("win.find-previous"),
        BookmarkPage => Action("win.bookmark-page"),
        CopyCleanLink => Action("win.copy-clean-link"),
        CopyLink => Action("win.copy-link"),
        Print => Action("win.print"),
        ViewSource => Action("win.view-source"),
        DeveloperTools => Action("win.developer-tools"),
        JavaScriptConsole => Action("win.javascript-console"),
        ZoomIn => Action("win.zoom-in"),
        ZoomOut => Action("win.zoom-out"),
        ZoomReset => Action("win.zoom-reset"),
        Fullscreen => Action("win.fullscreen"),
        ToggleBookmarksBar => Action("win.show-bookmarks-bar"),
        ShowBookmarks => Action("win.show-bookmarks"),
        ShowHistory => Action("win.show-history"),
        ShowDownloads => Action("win.show-downloads"),
        ShowSettings => Action("win.show-settings"),
        ShowShortcuts => Action("app.shortcuts"),
    })
}

/// The commands the user can reassign here, with their actions, in display order.
pub(crate) fn actions() -> impl Iterator<Item = (Command, &'static str)> {
    Command::ALL.iter().filter_map(|&cmd| match binding(cmd) {
        Some(Binding::Action(action)) => Some((cmd, action)),
        _ => None,
    })
}

/// The window action an extension's command runs, with the extension's id and the command's
/// name as its target.
pub(crate) const EXTENSION_COMMAND: &str = "win.extension-command";

/// [`EXTENSION_COMMAND`] for `name` of `extension`, which carries its accelerator.
pub(crate) fn extension_action(extension: &ExtensionId, name: &str) -> String {
    gio::Action::print_detailed_name(EXTENSION_COMMAND, Some(&(extension.as_str(), name).to_variant())).into()
}

/// Sets every action's accelerators from `keymap`, and the extension commands' from
/// `extensions`, taking them from commands that no longer have one.
pub(crate) fn apply(app: &impl IsA<gtk::Application>, keymap: &Keymap, extensions: &ExtensionShortcuts) {
    for (cmd, action) in actions() {
        let accels: Vec<String> = keymap.chords(cmd).iter().map(|&chord| accelerator(chord)).collect();
        let accels: Vec<&str> = accels.iter().map(String::as_str).collect();
        app.set_accels_for_action(action, &accels);
    }
    clear_extension_commands(app);
    for (command, chord) in extensions.iter() {
        if let Some(chord) = chord {
            app.set_accels_for_action(&extension_action(&command.extension, &command.command.name), &[&accelerator(chord)]);
        }
    }
}

/// Takes every accelerator away until the next [`apply`], so the capture dialog sees the
/// chords the window would otherwise run.
pub(crate) fn suspend(app: &impl IsA<gtk::Application>) {
    for (_, action) in actions() {
        app.set_accels_for_action(action, &[]);
    }
    clear_extension_commands(app);
}

fn clear_extension_commands(app: &impl IsA<gtk::Application>) {
    let prefix = format!("{EXTENSION_COMMAND}(");
    for action in app.list_action_descriptions().iter().filter(|action| action.starts_with(&prefix)) {
        app.set_accels_for_action(action, &[]);
    }
}

/// The GDK keyval name of `key`: core's name, except letters are lower case and a few keys
/// are named differently.
#[rustfmt::skip]
fn keyval_name(key: Key) -> &'static str {
    use Key::*;
    match key {
        A => "a", B => "b", C => "c", D => "d", E => "e", F => "f", G => "g", H => "h", I => "i",
        J => "j", K => "k", L => "l", M => "m", N => "n", O => "o", P => "p", Q => "q", R => "r",
        S => "s", T => "t", U => "u", V => "v", W => "w", X => "x", Y => "y", Z => "z",
        Space => "space",
        PageUp => "Page_Up",
        PageDown => "Page_Down",
        Plus => "plus",
        Minus => "minus",
        Equal => "equal",
        Comma => "comma",
        Period => "period",
        Slash => "slash",
        Question => "question",
        Semicolon => "semicolon",
        KeypadPlus => "KP_Add",
        KeypadMinus => "KP_Subtract",
        Keypad0 => "KP_0", Keypad1 => "KP_1", Keypad2 => "KP_2", Keypad3 => "KP_3", Keypad4 => "KP_4",
        Keypad5 => "KP_5", Keypad6 => "KP_6", Keypad7 => "KP_7", Keypad8 => "KP_8", Keypad9 => "KP_9",
        Digit0 | Digit1 | Digit2 | Digit3 | Digit4 | Digit5 | Digit6 | Digit7 | Digit8 | Digit9 | F1 | F2 | F3 | F4
        | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12 | F13 | F14 | F15 | F16 | F17 | F18 | F19 | F20 | F21 | F22 | F23
        | F24 | Tab | Insert | Delete | Home | End | Left | Right | Up | Down => key.name(),
    }
}

pub(crate) fn accelerator(chord: Chord) -> String {
    let mods = chord.mods();
    let mut accel = String::new();
    for (held, name) in [(mods.ctrl, "<Control>"), (mods.alt, "<Alt>"), (mods.shift, "<Shift>")] {
        if held {
            accel.push_str(name);
        }
    }
    accel.push_str(keyval_name(chord.key()));
    accel
}

/// Space-separated, the way `AdwShortcutLabel` shows several.
pub(crate) fn accelerators(chords: &[Chord]) -> String {
    chords.iter().map(|&chord| accelerator(chord)).collect::<Vec<_>>().join(" ")
}

fn key_of(keyval: gdk::Key) -> Option<Key> {
    let name = keyval.to_lower().name()?;
    if name == "ISO_Left_Tab" {
        return Some(Key::Tab);
    }
    Key::ALL.iter().copied().find(|&key| keyval_name(key) == name)
}

/// What a key press in the capture dialog means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pressed {
    Cancel,
    Clear,
    Confirm,
    Chord(Chord),
    /// A key a shortcut can use, but not without Ctrl or Alt.
    NeedsModifier,
    /// A key no shortcut can use.
    Unusable,
}

/// `keyval` and `state` as the key event reports them, `consumed` the modifiers the layout
/// used to produce `keyval`, `base` the key's keyval without modifiers.
///
/// A letter keeps Shift. A symbol typed with Shift is named by what it typed, so Ctrl+? is
/// Ctrl+Question, not Ctrl+Shift+Question; a shifted symbol core has no name for (Ctrl+! on
/// a US layout) falls back to its key, keeping Shift (Ctrl+Shift+1).
pub(crate) fn pressed(keyval: gdk::Key, state: gdk::ModifierType, consumed: gdk::ModifierType, base: Option<gdk::Key>) -> Pressed {
    let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
    let alt = state.contains(gdk::ModifierType::ALT_MASK);
    let mut shift = state.contains(gdk::ModifierType::SHIFT_MASK);
    if !(ctrl || alt || shift) {
        match keyval {
            gdk::Key::Escape => return Pressed::Cancel,
            gdk::Key::BackSpace => return Pressed::Clear,
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => return Pressed::Confirm,
            _ => {}
        }
    }
    let key = match key_of(keyval) {
        Some(key) => {
            let letter = key.name().len() == 1 && key.name().as_bytes()[0].is_ascii_alphabetic();
            if !letter && keyval != gdk::Key::ISO_Left_Tab && consumed.contains(gdk::ModifierType::SHIFT_MASK) {
                shift = false;
            }
            key
        }
        None => match base.and_then(key_of) {
            Some(key) => key,
            None => return Pressed::Unusable,
        },
    };
    match Chord::new(Mods { ctrl, alt, shift }, key) {
        Some(chord) => Pressed::Chord(chord),
        None => Pressed::NeedsModifier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::ModifierType as M;

    fn chord(s: &str) -> Chord {
        s.parse().expect("a chord")
    }

    #[test]
    fn every_key_has_its_own_gdk_keyval() {
        let mut seen = std::collections::HashSet::new();
        for &key in Key::ALL {
            let keyval = gdk::Key::from_name(keyval_name(key)).unwrap_or_else(|| panic!("{key:?} has no keyval"));
            assert!(seen.insert(keyval), "{key:?} shares its keyval");
            assert_eq!(key_of(keyval), Some(key));
        }
    }

    #[gtk::test]
    fn every_chord_is_an_accelerator_gtk_reads_back() {
        let mods = [Mods::CTRL, Mods::ALT, Mods::CTRL.and(Mods::SHIFT), Mods::CTRL.and(Mods::ALT).and(Mods::SHIFT)];
        for &key in Key::ALL {
            for mods in mods {
                let chord = Chord::new(mods, key).expect("Ctrl or Alt makes any key bindable");
                let accel = accelerator(chord);
                let (keyval, parsed) = gtk::accelerator_parse(&accel).unwrap_or_else(|| panic!("GTK cannot read {accel}"));
                assert_eq!(key_of(keyval), Some(key), "{accel}");
                assert_eq!(
                    (parsed.contains(M::CONTROL_MASK), parsed.contains(M::ALT_MASK), parsed.contains(M::SHIFT_MASK)),
                    (mods.ctrl, mods.alt, mods.shift),
                    "{accel}"
                );
            }
        }
    }

    #[test]
    fn accelerators_use_gdk_names() {
        assert_eq!(accelerator(chord("Ctrl+Plus")), "<Control>plus");
        assert_eq!(accelerator(chord("Ctrl+Question")), "<Control>question");
        assert_eq!(accelerator(chord("Ctrl+KeypadPlus")), "<Control>KP_Add");
        assert_eq!(accelerator(chord("Ctrl+Keypad0")), "<Control>KP_0");
        assert_eq!(accelerator(chord("Ctrl+Shift+S")), "<Control><Shift>s");
        assert_eq!(accelerator(chord("Shift+F5")), "<Shift>F5");
        assert_eq!(accelerator(chord("Ctrl+Alt+PageDown")), "<Control><Alt>Page_Down");
        assert_eq!(accelerators(&[chord("Ctrl+W"), chord("Ctrl+F4")]), "<Control>w <Control>F4");
    }

    #[test]
    fn a_key_press_becomes_the_chord_it_names() {
        let none = M::empty();
        let (ctrl, shift, alt) = (M::CONTROL_MASK, M::SHIFT_MASK, M::ALT_MASK);
        assert_eq!(pressed(gdk::Key::S, ctrl | shift, shift, Some(gdk::Key::s)), Pressed::Chord(chord("Ctrl+Shift+S")));
        assert_eq!(pressed(gdk::Key::question, ctrl | shift, shift, Some(gdk::Key::slash)), Pressed::Chord(chord("Ctrl+Question")));
        assert_eq!(pressed(gdk::Key::plus, ctrl | shift, shift, Some(gdk::Key::equal)), Pressed::Chord(chord("Ctrl+Plus")));
        assert_eq!(pressed(gdk::Key::exclam, ctrl | shift, shift, Some(gdk::Key::_1)), Pressed::Chord(chord("Ctrl+Shift+1")));
        assert_eq!(pressed(gdk::Key::ISO_Left_Tab, ctrl | shift, shift, Some(gdk::Key::Tab)), Pressed::Chord(chord("Ctrl+Shift+Tab")));
        assert_eq!(pressed(gdk::Key::F5, shift, none, Some(gdk::Key::F5)), Pressed::Chord(chord("Shift+F5")));
        assert_eq!(pressed(gdk::Key::d, alt, none, Some(gdk::Key::d)), Pressed::Chord(chord("Alt+D")));
        assert_eq!(pressed(gdk::Key::KP_Add, ctrl, none, Some(gdk::Key::KP_Add)), Pressed::Chord(chord("Ctrl+KeypadPlus")));
        assert_eq!(pressed(gdk::Key::F9, none, none, Some(gdk::Key::F9)), Pressed::Chord(chord("F9")));
    }

    #[test]
    fn capture_keys_and_unbindable_presses_are_told_apart() {
        let none = M::empty();
        assert_eq!(pressed(gdk::Key::Escape, none, none, None), Pressed::Cancel);
        assert_eq!(pressed(gdk::Key::BackSpace, none, none, None), Pressed::Clear);
        assert_eq!(pressed(gdk::Key::Return, none, none, None), Pressed::Confirm);
        assert_eq!(pressed(gdk::Key::h, none, none, Some(gdk::Key::h)), Pressed::NeedsModifier);
        assert_eq!(pressed(gdk::Key::H, M::SHIFT_MASK, M::SHIFT_MASK, Some(gdk::Key::h)), Pressed::NeedsModifier);
        assert_eq!(pressed(gdk::Key::BackSpace, M::CONTROL_MASK, none, None), Pressed::Unusable);
        assert_eq!(pressed(gdk::Key::Print, M::CONTROL_MASK, none, Some(gdk::Key::Print)), Pressed::Unusable);
    }

    #[test]
    fn only_widget_built_ins_and_actions_are_bound_and_actions_are_unique() {
        let actions: Vec<&str> = actions().map(|(_, action)| action).collect();
        let unique: std::collections::HashSet<&&str> = actions.iter().collect();
        assert_eq!(unique.len(), actions.len());
        assert_eq!(binding(Command::SavePage), Some(Binding::Action("win.save-page")));
        assert_eq!(binding(Command::ViewSource), Some(Binding::Action("win.view-source")));
        assert_eq!(binding(Command::DeveloperTools), Some(Binding::Action("win.developer-tools")));
        assert_eq!(binding(Command::NextTab), Some(Binding::BuiltIn("<Control>Tab")));
        assert_eq!(binding(Command::CopyCleanLink), Some(Binding::Action("win.copy-clean-link")));
        assert_eq!(binding(Command::SelectTab1), None);
    }

    #[gtk::test]
    fn extension_commands_get_their_chords_and_lose_stale_ones() {
        use vsesvit_core::extensions::commands::ExtensionCommand;
        use vsesvit_core::extensions::manifest::ManifestCommand;

        let app = gtk::Application::builder().build();
        let id = ExtensionId::parse("commands@vsesvit.test").expect("an extension id");
        let command = |name: &str, key: &str| ExtensionCommand {
            extension: id.clone(),
            extension_name: "Commands".to_owned(),
            command: ManifestCommand { name: name.to_owned(), description: String::new(), suggested_key: Some(chord(key)) },
        };
        let keymap = Keymap::default();
        let runs = |accel: &str| -> Vec<String> { app.actions_for_accel(accel).iter().map(|a| a.to_string()).collect() };
        let run = extension_action(&id, "run");
        assert_eq!(run, "win.extension-command(('commands@vsesvit.test', 'run'))");

        apply(&app, &keymap, &keymap.extension_shortcuts(vec![command("_execute_action", "Alt+Shift+A"), command("run", "Alt+Shift+R")]));
        assert_eq!(runs("<Alt><Shift>r"), [run]);
        assert_eq!(runs("<Alt><Shift>a"), [extension_action(&id, "_execute_action")]);
        assert_eq!(runs("<Control>t"), ["win.new-tab"]);

        apply(&app, &keymap, &keymap.extension_shortcuts(vec![command("_execute_action", "Alt+Shift+A")]));
        assert!(runs("<Alt><Shift>r").is_empty(), "a command gone keeps its accelerator");
        suspend(&app);
        assert!(runs("<Alt><Shift>a").is_empty() && runs("<Control>t").is_empty(), "suspend left accelerators");
    }
}
