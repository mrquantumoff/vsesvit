//! Owned by W1.6.
//! Keys (UA section 1.9). While GPUI has the keyboard, core's keymap becomes GPUI key bindings in the
//! `BrowserWindow` context. While a page has it, the same chords become the engine's `KeyRouting`,
//! with the phase from core's in-page policy (`Command::in_page_on`): Chrome's reserved keys are
//! taken before the page; every other chord reaches the page first and is taken only if the page
//! leaves it. A `Native` command is taken after the page too: the engine refuses Chromium's own
//! accelerators (`vsesvit-cef`'s IDC default-deny), so nothing is left for it to run.
//! The shortcut capture reads GPUI key presses through core's capture rules.

use gpui::{Action, App, KeyBinding, KeyBindingContextPredicate, KeyBindingMetaIndex, KeyDownEvent};
use vsesvit_core::shortcuts::{self, Capture, Chord, InPage, Key, Keymap, Mods};
use vsesvit_engine::{KeyRouting, RoutePhase, RoutedKey};

use crate::app::actions;

/// The key context of a browser window's root element.
pub const CONTEXT: &str = "BrowserWindow";

/// Tags the bindings `install` adds, so a later `install` or `suspend` replaces only those.
const OURS: KeyBindingMetaIndex = KeyBindingMetaIndex(0x7673);

const fn chord(mods: Mods, key: Key) -> Chord {
    match Chord::new(mods, key) {
        Some(chord) => chord,
        None => panic!("a built-in key must be bindable"),
    }
}

const CTRL_SHIFT: Mods = Mods::CTRL.and(Mods::SHIFT);

/// Chrome's tab keys, which nobody rebinds: they work beside whatever the keymap binds. Chrome keeps
/// next and previous tab from pages, and lets pages have the tab moves.
pub const BUILT_INS: [(Chord, RoutePhase, RoutedKey); 6] = [
    (chord(Mods::CTRL, Key::Tab), RoutePhase::BeforePage, RoutedKey::NextTab),
    (chord(Mods::CTRL, Key::PageDown), RoutePhase::BeforePage, RoutedKey::NextTab),
    (chord(CTRL_SHIFT, Key::Tab), RoutePhase::BeforePage, RoutedKey::PreviousTab),
    (chord(Mods::CTRL, Key::PageUp), RoutePhase::BeforePage, RoutedKey::PreviousTab),
    (chord(CTRL_SHIFT, Key::PageUp), RoutePhase::AfterPage, RoutedKey::MoveTabLeft),
    (chord(CTRL_SHIFT, Key::PageDown), RoutePhase::AfterPage, RoutedKey::MoveTabRight),
];

/// Every key-bound chord with what it does: the built-ins first, then each keymap chord they do not
/// already take.
fn entries(keymap: &Keymap) -> Vec<(Chord, RoutePhase, RoutedKey)> {
    let mut entries = BUILT_INS.to_vec();
    for (command, chords) in keymap.iter() {
        for &chord in chords {
            if entries.iter().any(|(taken, _, _)| *taken == chord) {
                continue;
            }
            let phase = match command.in_page_on(chord) {
                InPage::Reserved => RoutePhase::BeforePage,
                InPage::Overridable | InPage::Native => RoutePhase::AfterPage,
            };
            entries.push((chord, phase, RoutedKey::Command(command)));
        }
    }
    entries
}

/// The keys the engine hands back while a page has the keyboard.
pub fn routing_for(keymap: &Keymap) -> KeyRouting {
    KeyRouting { entries: entries(keymap) }
}

/// The GPUI action a routed key runs.
pub fn action_for(key: RoutedKey) -> Box<dyn Action> {
    match key {
        RoutedKey::Command(command) => actions::for_command(command),
        RoutedKey::NextTab => Box::new(actions::NextTab),
        RoutedKey::PreviousTab => Box::new(actions::PreviousTab),
        RoutedKey::MoveTabLeft => Box::new(actions::MoveTabLeft),
        RoutedKey::MoveTabRight => Box::new(actions::MoveTabRight),
    }
}

/// The keypad's plus and minus as GPUI names them: X11 reports the keysyms `KP_Add` and
/// `KP_Subtract` without their prefix; Windows maps them to the characters they type.
#[cfg(target_os = "linux")]
const KEYPAD_PLUS_MINUS: (&str, &str) = ("add", "subtract");
#[cfg(not(target_os = "linux"))]
const KEYPAD_PLUS_MINUS: (&str, &str) = ("+", "-");

/// GPUI's name for a key pressed without Shift. The keypad's digits share the main digits' names on
/// both backends, so binding both to one action is harmless.
fn key_name(key: Key) -> String {
    let name = match key {
        Key::Plus => "+",
        Key::Minus => "-",
        Key::KeypadPlus => KEYPAD_PLUS_MINUS.0,
        Key::KeypadMinus => KEYPAD_PLUS_MINUS.1,
        Key::Equal => "=",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Slash => "/",
        Key::Question => "?",
        Key::Semicolon => ";",
        Key::Keypad0 => "0",
        Key::Keypad1 => "1",
        Key::Keypad2 => "2",
        Key::Keypad3 => "3",
        Key::Keypad4 => "4",
        Key::Keypad5 => "5",
        Key::Keypad6 => "6",
        Key::Keypad7 => "7",
        Key::Keypad8 => "8",
        Key::Keypad9 => "9",
        other => return other.name().to_ascii_lowercase(),
    };
    name.to_owned()
}

/// What a main-row digit or symbol key types with Shift on the US layout, which core's key names
/// assume. Both GPUI backends report such a press as that character, without Shift.
fn shifted(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::Digit1 => "!",
        Key::Digit2 => "@",
        Key::Digit3 => "#",
        Key::Digit4 => "$",
        Key::Digit5 => "%",
        Key::Digit6 => "^",
        Key::Digit7 => "&",
        Key::Digit8 => "*",
        Key::Digit9 => "(",
        Key::Digit0 => ")",
        Key::Minus => "_",
        Key::Equal => "+",
        Key::Comma => "<",
        Key::Period => ">",
        Key::Slash => "?",
        Key::Semicolon => ":",
        _ => return None,
    })
}

/// GPUI's keystroke for a chord, as a real press of it arrives and `KeyBinding` parses it.
pub fn keystroke(chord: Chord) -> String {
    let mods = chord.mods();
    let typed = shifted(chord.key()).filter(|_| mods.shift);
    let mut out = String::new();
    for (held, name) in [(mods.ctrl, "ctrl-"), (mods.alt, "alt-"), (mods.shift && typed.is_none(), "shift-")] {
        if held {
            out.push_str(name);
        }
    }
    match typed {
        Some(character) => out.push_str(character),
        None => out.push_str(&key_name(chord.key())),
    }
    out
}

fn bindings(keymap: &Keymap) -> Vec<KeyBinding> {
    let context = KeyBindingContextPredicate::parse(CONTEXT).ok().map(Into::into);
    entries(keymap)
        .into_iter()
        .filter_map(|(chord, _, key)| {
            let binding = KeyBinding::load(&keystroke(chord), action_for(key), context.clone(), false, None, &gpui::DummyKeyboardMapper);
            match binding {
                Ok(binding) => Some(binding.with_meta(OURS)),
                Err(e) => {
                    log::error!("keymap: {chord} has no GPUI keystroke ({e})");
                    None
                }
            }
        })
        .collect()
}

/// Replaces the shell's key bindings with the keymap's, keeping every other binding (gpui-component's
/// text editing keys), and hands the engine the matching routing.
pub fn install(cx: &mut App, keymap: &Keymap) {
    replace_ours(cx, bindings(keymap));
    if super::running(cx) {
        super::engine(cx).set_key_routing(routing_for(keymap));
    }
}

/// Removes the shell's key bindings while the shortcut capture listens; `install` brings them back.
/// The engine's routing stays: the capture has the keyboard, not a page.
pub fn suspend(cx: &mut App) {
    replace_ours(cx, Vec::new());
}

fn replace_ours(cx: &mut App, ours: Vec<KeyBinding>) {
    let others: Vec<KeyBinding> = cx.key_bindings().borrow().bindings().filter(|b| b.meta() != Some(OURS)).cloned().collect();
    cx.clear_key_bindings();
    cx.bind_keys(others.into_iter().chain(ours));
}

/// What a key press does while the shortcut capture listens, by core's capture rules.
pub fn capture(event: &KeyDownEvent) -> Capture {
    let keystroke = &event.keystroke;
    let (vk, typed_with_shift) = vk_of(&keystroke.key);
    let m = keystroke.modifiers;
    shortcuts::capture(vk, Mods { ctrl: m.control, alt: m.alt, shift: m.shift || typed_with_shift })
}

/// The virtual-key code of a GPUI key name, and whether typing it takes Shift; 0 for a key core has
/// no name for, which capture reads as not a key.
fn vk_of(key: &str) -> (u16, bool) {
    match key {
        "escape" => (0x1B, false),
        "backspace" => (0x08, false),
        "enter" => (0x0D, false),
        "shift" => (0x10, false),
        "control" => (0x11, false),
        "alt" => (0x12, false),
        "platform" => (0x5B, false),
        _ => match Key::ALL.iter().copied().find(|&k| key_name(k) == key) {
            Some(k) => k.vk(),
            None => Key::ALL.iter().copied().find(|&k| shifted(k) == Some(key)).map_or((0, false), |k| (k.vk().0, true)),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use gpui::{Keystroke, TestAppContext};
    use vsesvit_core::shortcuts::Command;

    use super::*;

    fn press(source: &str) -> KeyDownEvent {
        KeyDownEvent { keystroke: Keystroke::parse(source).unwrap(), is_held: false, prefer_character_input: false }
    }

    fn c(s: &str) -> Chord {
        s.parse().unwrap()
    }

    #[test]
    fn every_chord_round_trips_through_the_press_gpui_reports_for_it() {
        let keypad = |k: Key| match k {
            Key::Keypad0 => Some(Key::Digit0),
            Key::Keypad1 => Some(Key::Digit1),
            Key::Keypad2 => Some(Key::Digit2),
            Key::Keypad3 => Some(Key::Digit3),
            Key::Keypad4 => Some(Key::Digit4),
            Key::Keypad5 => Some(Key::Digit5),
            Key::Keypad6 => Some(Key::Digit6),
            Key::Keypad7 => Some(Key::Digit7),
            Key::Keypad8 => Some(Key::Digit8),
            Key::Keypad9 => Some(Key::Digit9),
            Key::KeypadPlus if cfg!(not(target_os = "linux")) => Some(Key::Plus),
            Key::KeypadMinus if cfg!(not(target_os = "linux")) => Some(Key::Minus),
            _ => None,
        };
        for &key in Key::ALL {
            for bits in 0..8u8 {
                let mods = Mods { ctrl: bits & 1 != 0, alt: bits & 2 != 0, shift: bits & 4 != 0 };
                let Some(chord) = Chord::new(mods, key) else { continue };
                let source = keystroke(chord);
                assert!(!(source.contains("shift-") && shifted(key).is_some()), "{chord}: GPUI reports {source} without Shift");
                let main = match keypad(key).unwrap_or(key) {
                    Key::Equal if mods.shift => Key::Plus,
                    Key::Slash if mods.shift => Key::Question,
                    main => main,
                };
                let typed_with_shift = matches!(main, Key::Plus | Key::Question);
                let expected = Chord::new(Mods { shift: mods.shift && !typed_with_shift, ..mods }, main).unwrap();
                assert_eq!(capture(&press(&source)), Capture::Chord(expected), "{chord} as {source}");
            }
        }
        assert_eq!(keystroke(c("Ctrl+Shift+1")), "ctrl-!");
        assert_eq!(keystroke(c("Ctrl+Shift+Comma")), "ctrl-<");
        assert_eq!(keystroke(c("Ctrl+Shift+T")), "ctrl-shift-t");
        assert_eq!(keystroke(c("Ctrl+KeypadPlus")), if cfg!(target_os = "linux") { "ctrl-add" } else { "ctrl-+" });
        assert_eq!(keystroke(c("Ctrl+Minus")), "ctrl--");
        assert_eq!(keystroke(c("Ctrl+Shift+Tab")), "ctrl-shift-tab");
        assert_eq!(keystroke(c("F11")), "f11");
        assert_eq!(keystroke(c("Alt+Left")), "alt-left");
        assert_eq!(keystroke(c("Ctrl+Plus")), "ctrl-+");
    }

    #[test]
    fn the_default_routing_follows_cores_in_page_policy() {
        let keymap = Keymap::default();
        let routing = routing_for(&keymap);
        let phase = |s: &str| routing.entries.iter().find(|(chord, _, _)| *chord == c(s)).map(|(_, p, k)| (*p, *k));
        assert_eq!(phase("Ctrl+T"), Some((RoutePhase::BeforePage, RoutedKey::Command(Command::NewTab))));
        assert_eq!(phase("Ctrl+Q"), Some((RoutePhase::BeforePage, RoutedKey::Command(Command::Quit))));
        assert_eq!(phase("Ctrl+1"), Some((RoutePhase::AfterPage, RoutedKey::Command(Command::SelectTab1))), "Chrome leaves Ctrl+1 to pages");
        assert_eq!(phase("Ctrl+9"), Some((RoutePhase::AfterPage, RoutedKey::Command(Command::SelectLastTab))));
        assert_eq!(phase("Ctrl+F"), Some((RoutePhase::AfterPage, RoutedKey::Command(Command::Find))));
        assert_eq!(phase("Ctrl+R"), Some((RoutePhase::AfterPage, RoutedKey::Command(Command::Reload))));
        assert_eq!(phase("Ctrl+Shift+C"), Some((RoutePhase::BeforePage, RoutedKey::Command(Command::CopyCleanLink))));
        assert_eq!(phase("Ctrl+Tab"), Some((RoutePhase::BeforePage, RoutedKey::NextTab)));
        assert_eq!(phase("Ctrl+PageUp"), Some((RoutePhase::BeforePage, RoutedKey::PreviousTab)));
        assert_eq!(phase("Ctrl+Shift+PageDown"), Some((RoutePhase::AfterPage, RoutedKey::MoveTabRight)));
        for (command, chords) in keymap.iter() {
            for &chord in chords {
                let Some((p, key)) = phase(&chord.to_string()) else { panic!("{chord} is not routed") };
                let builtin = BUILT_INS.iter().any(|(b, _, _)| *b == chord);
                if !builtin {
                    assert_eq!(key, RoutedKey::Command(command), "{chord}");
                }
                let reserved = command.in_page_on(chord) == InPage::Reserved;
                assert_eq!(p == RoutePhase::BeforePage, reserved, "{chord} for {command:?}");
            }
        }
        let chords: HashSet<Chord> = routing.entries.iter().map(|(chord, _, _)| *chord).collect();
        assert_eq!(chords.len(), routing.entries.len(), "a chord is routed twice");
    }

    #[test]
    fn a_reassigned_chord_routes_to_its_new_command_and_a_built_in_stays() {
        let mut keymap = Keymap::default();
        keymap.assign(Command::ShowHistory, [c("Ctrl+Shift+Y"), c("Ctrl+Tab")]);
        let routing = routing_for(&keymap);
        let find = |s: &str| routing.entries.iter().find(|(chord, _, _)| *chord == c(s)).map(|(_, p, k)| (*p, *k));
        assert_eq!(find("Ctrl+Shift+Y"), Some((RoutePhase::AfterPage, RoutedKey::Command(Command::ShowHistory))));
        assert_eq!(find("Ctrl+H"), None, "the old chord is gone");
        assert_eq!(find("Ctrl+Tab"), Some((RoutePhase::BeforePage, RoutedKey::NextTab)));
    }

    #[test]
    fn every_routed_key_names_its_action() {
        assert_eq!(action_for(RoutedKey::NextTab).name(), actions::NextTab.name());
        assert_eq!(action_for(RoutedKey::MoveTabRight).name(), actions::MoveTabRight.name());
        assert_eq!(action_for(RoutedKey::Command(Command::Reload)).name(), actions::Reload.name());
    }

    #[test]
    fn a_key_press_becomes_the_chord_it_names() {
        assert_eq!(capture(&press("ctrl-t")), Capture::Chord(c("Ctrl+T")));
        assert_eq!(capture(&press("ctrl-shift-tab")), Capture::Chord(c("Ctrl+Shift+Tab")));
        assert_eq!(capture(&press("alt-left")), Capture::Chord(c("Alt+Left")));
        assert_eq!(capture(&press("f5")), Capture::Chord(c("F5")));
        assert_eq!(capture(&press("shift-f5")), Capture::Chord(c("Shift+F5")));
        assert_eq!(capture(&press("ctrl-=")), Capture::Chord(c("Ctrl+Equal")));
        assert_eq!(capture(&press("ctrl-+")), Capture::Chord(c("Ctrl+Plus")), "Shift with =+ arrives as +");
        assert_eq!(capture(&press("ctrl-?")), Capture::Chord(c("Ctrl+Question")));
        assert_eq!(capture(&press("ctrl-!")), Capture::Chord(c("Ctrl+Shift+1")));
        assert_eq!(capture(&press("ctrl-<")), Capture::Chord(c("Ctrl+Shift+Comma")));
        assert_eq!(capture(&press("ctrl-_")), Capture::Chord(c("Ctrl+Shift+Minus")));
        assert_eq!(capture(&press("ctrl--")), Capture::Chord(c("Ctrl+Minus")));
        assert_eq!(capture(&press("ctrl-pagedown")), Capture::Chord(c("Ctrl+PageDown")));
    }

    #[test]
    fn capture_keys_and_unbindable_presses_are_told_apart() {
        assert_eq!(capture(&press("escape")), Capture::Cancel);
        assert_eq!(capture(&press("backspace")), Capture::Remove);
        assert_eq!(capture(&press("enter")), Capture::Save);
        assert_eq!(capture(&press("ctrl")), Capture::Modifier);
        assert_eq!(capture(&press("shift")), Capture::Modifier);
        assert_eq!(capture(&press("alt")), Capture::Modifier);
        assert_eq!(capture(&press("a")), Capture::NeedsModifier);
        assert_eq!(capture(&press("shift-a")), Capture::NeedsModifier);
        assert_eq!(capture(&press("left")), Capture::NeedsModifier);
        assert_eq!(capture(&press("ctrl-printscreen")), Capture::NotAKey);
    }

    #[gpui::test]
    fn install_replaces_only_its_own_bindings(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.bind_keys([KeyBinding::new("ctrl-a", actions::About, Some("Input"))]);
            let mut keymap = Keymap::default();
            install(cx, &keymap);
            let before = entries(&keymap).len();
            assert_eq!(cx.key_bindings().borrow().bindings().count(), before + 1);
            keymap.assign(Command::ShowHistory, []);
            install(cx, &keymap);
            assert_eq!(entries(&keymap).len(), before - 1);
            assert_eq!(cx.key_bindings().borrow().bindings().count(), before, "Ctrl+H is unbound and nothing doubled");
            suspend(cx);
            let left: Vec<String> = cx.key_bindings().borrow().bindings().map(|b| b.action().name().to_owned()).collect();
            assert_eq!(left, [actions::About.name()], "the capture keeps only other bindings");
        });
    }
}
