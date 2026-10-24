//! Owned by W1.3.
//! Keys while a page has focus (UA section 1.5, design section 1.5): the shell's `KeyRouting` decides
//! which chords leave the page. `BeforePage` chords are swallowed in `on_pre_key_event` (the page never
//! sees them); `AfterPage` chords are marked as shortcuts there, and reach the shell from `on_key_event`
//! only if the page did not consume them. Both end in `BrowserEvent::Shortcut`. Chrome's own
//! accelerators are refused separately, in `command.rs`.
//!
//! The key table maps core's `Key` to Windows virtual-key codes, which CEF reports on every OS
//! (`windows_key_code`). Symbol keys assume the US layout, as WinUI's table does.

use std::sync::Arc;

use cef::*;
use vsesvit_core::shortcuts::{Key, Mods};
use vsesvit_engine::{BrowserEvent, Chord, RoutePhase, RoutedKey, TabId};

use crate::client::HandlerCtx;
use crate::registry::{self, BrowserId};

/// A key event, as the glue reads and sends it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KeyInput {
    /// Down, up or character.
    pub kind: KeyKind,
    /// The Windows virtual-key code.
    pub windows_key_code: i32,
    /// The OS's own code.
    pub native_key_code: i32,
    /// `cef_event_flags_t` bits.
    pub modifiers: u32,
    /// The character typed, for `Char`.
    pub character: u16,
}

/// CEF's key event types.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// `KEYEVENT_RAWKEYDOWN`.
    RawKeyDown,
    /// `KEYEVENT_KEYDOWN`.
    KeyDown,
    /// `KEYEVENT_KEYUP`.
    KeyUp,
    /// `KEYEVENT_CHAR`.
    Char,
}

const SHIFT: u32 = sys::cef_event_flags_t::EVENTFLAG_SHIFT_DOWN.0 as u32;
const CONTROL: u32 = sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0 as u32;
const ALT: u32 = sys::cef_event_flags_t::EVENTFLAG_ALT_DOWN.0 as u32;

/// The virtual-key code of `key`, and whether typing it takes Shift (Plus is Shift with `=+`,
/// Question is Shift with `/?`).
pub fn key_vk(key: Key) -> (i32, bool) {
    let runs = [(Key::A, Key::Z, 0x41), (Key::Digit0, Key::Digit9, 0x30), (Key::F1, Key::F24, 0x70), (Key::Keypad0, Key::Keypad9, 0x60)];
    let at = key as i32;
    if let Some((first, _, base)) = runs.iter().find(|(first, last, _)| (*first as i32..=*last as i32).contains(&at)) {
        return (base + at - *first as i32, false);
    }
    let vk = match key {
        Key::Tab => 0x09,
        Key::Space => 0x20,
        Key::PageUp => 0x21,
        Key::PageDown => 0x22,
        Key::End => 0x23,
        Key::Home => 0x24,
        Key::Left => 0x25,
        Key::Up => 0x26,
        Key::Right => 0x27,
        Key::Down => 0x28,
        Key::Insert => 0x2D,
        Key::Delete => 0x2E,
        Key::KeypadPlus => 0x6B,
        Key::KeypadMinus => 0x6D,
        Key::Semicolon => 0xBA,
        Key::Equal => 0xBB,
        Key::Plus => return (0xBB, true),
        Key::Comma => 0xBC,
        Key::Minus => 0xBD,
        Key::Period => 0xBE,
        Key::Slash => 0xBF,
        Key::Question => return (0xBF, true),
        _ => unreachable!("{key:?} is in a run of keys above"),
    };
    (vk, false)
}

/// The chord a key press is, if any: Shift with the `=+` or `/?` key reads as Plus or Question.
pub fn chord_of(vk: i32, modifiers: u32) -> Option<Chord> {
    let mods = Mods { ctrl: modifiers & CONTROL != 0, alt: modifiers & ALT != 0, shift: modifiers & SHIFT != 0 };
    let find = |shift: bool| Key::ALL.iter().copied().find(|&k| key_vk(k) == (vk, shift));
    let (key, mods) = match find(true).filter(|_| mods.shift) {
        Some(key) => (key, Mods { shift: false, ..mods }),
        None => (find(false)?, mods),
    };
    Chord::new(mods, key)
}

/// The key code and event flags that type `chord`.
pub fn press_of(chord: Chord) -> (i32, u32) {
    let (vk, shift) = key_vk(chord.key());
    let m = chord.mods();
    let flags = if m.ctrl { CONTROL } else { 0 } | if m.alt { ALT } else { 0 } | if m.shift || shift { SHIFT } else { 0 };
    (vk, flags)
}

/// `on_pre_key_event`'s answer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PreKey {
    /// The page gets the key.
    Pass,
    /// Swallowed: a `BeforePage` chord (its down event went to the shell).
    Swallow,
    /// An `AfterPage` chord: the page sees it first (`is_keyboard_shortcut`).
    Shortcut,
}

fn route(browser: BrowserId, input: &KeyInput) -> Option<(Option<TabId>, RoutePhase, RoutedKey)> {
    let chord = chord_of(input.windows_key_code, input.modifiers)?;
    registry::with(|r| {
        let tab = r.tab_of(browser)?;
        r.routing.entries.iter().find(|(c, _, _)| *c == chord).map(|(_, phase, key)| (Some(tab), *phase, *key))
    })
    .flatten()
}

fn is_down(input: &KeyInput) -> bool {
    matches!(input.kind, KeyKind::RawKeyDown | KeyKind::KeyDown)
}

/// Sends the shortcut on the key's down event; its up and char events are only consumed.
fn shortcut(input: &KeyInput, tab: Option<TabId>, key: RoutedKey) {
    if is_down(input)
        && let Some(sink) = registry::sink()
    {
        sink.send(BrowserEvent::Shortcut { tab, key });
    }
}

/// The logic of `on_pre_key_event`.
pub fn pre_key(browser: BrowserId, input: &KeyInput) -> PreKey {
    let Some((tab, phase, key)) = route(browser, input) else { return PreKey::Pass };
    match phase {
        RoutePhase::BeforePage => {
            shortcut(input, tab, key);
            PreKey::Swallow
        }
        RoutePhase::AfterPage => PreKey::Shortcut,
    }
}

/// The logic of `on_key_event`: the page did not consume the key. True when it went to the shell.
pub fn key(browser: BrowserId, input: &KeyInput) -> bool {
    let Some((tab, RoutePhase::AfterPage, key)) = route(browser, input) else { return false };
    shortcut(input, tab, key);
    true
}

/// `Engine::set_key_routing`.
pub fn set_routing(routing: vsesvit_engine::KeyRouting) {
    registry::with(|r| r.routing = routing);
}

/// `Engine::send_key`: down, then up, as a page sees a real press.
pub fn send_key(tab: TabId, chord: Chord) {
    let (vk, modifiers) = press_of(chord);
    let input = |kind| KeyInput { kind, windows_key_code: vk, native_key_code: 0, modifiers, character: 0 };
    registry::on_tab(tab, |page| {
        page.send_key(&input(KeyKind::RawKeyDown));
        page.send_key(&input(KeyKind::KeyUp));
    });
}

fn kind_of(t: KeyEventType) -> KeyKind {
    match t.get_raw() {
        x if x == KeyEventType::RAWKEYDOWN.get_raw() => KeyKind::RawKeyDown,
        x if x == KeyEventType::KEYDOWN.get_raw() => KeyKind::KeyDown,
        x if x == KeyEventType::KEYUP.get_raw() => KeyKind::KeyUp,
        _ => KeyKind::Char,
    }
}

/// The CEF event for an input.
pub fn to_cef(input: &KeyInput) -> KeyEvent {
    KeyEvent {
        type_: match input.kind {
            KeyKind::RawKeyDown => KeyEventType::RAWKEYDOWN,
            KeyKind::KeyDown => KeyEventType::KEYDOWN,
            KeyKind::KeyUp => KeyEventType::KEYUP,
            KeyKind::Char => KeyEventType::CHAR,
        },
        modifiers: input.modifiers,
        windows_key_code: input.windows_key_code,
        native_key_code: input.native_key_code,
        character: input.character,
        unmodified_character: input.character,
        ..Default::default()
    }
}

fn from_cef(event: &KeyEvent) -> KeyInput {
    KeyInput {
        kind: kind_of(event.type_),
        windows_key_code: event.windows_key_code,
        native_key_code: event.native_key_code,
        modifiers: event.modifiers,
        character: event.character,
    }
}

#[cfg(windows)]
type OsEvent = sys::MSG;
#[cfg(not(windows))]
type OsEvent = sys::XEvent;

wrap_keyboard_handler! {
    pub struct Keyboard {
        ctx: Arc<HandlerCtx>,
    }

    impl KeyboardHandler {
        fn on_pre_key_event(&self, browser: Option<&mut Browser>, event: Option<&KeyEvent>, _os_event: Option<&mut OsEvent>,
                            is_keyboard_shortcut: Option<&mut i32>) -> i32 {
            let (Some(browser), Some(event)) = (browser, event) else { return 0 };
            match pre_key(browser.identifier(), &from_cef(event)) {
                PreKey::Pass => 0,
                PreKey::Swallow => 1,
                PreKey::Shortcut => {
                    if let Some(flag) = is_keyboard_shortcut {
                        *flag = 1;
                    }
                    0
                }
            }
        }

        fn on_key_event(&self, browser: Option<&mut Browser>, event: Option<&KeyEvent>, _os_event: Option<&mut OsEvent>) -> i32 {
            let (Some(browser), Some(event)) = (browser, event) else { return 0 };
            key(browser.identifier(), &from_cef(event)) as i32
        }
    }
}

impl Keyboard {
    /// The handler for a client.
    pub fn handler(ctx: &Arc<HandlerCtx>) -> KeyboardHandler {
        Keyboard::new(ctx.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> Chord {
        s.parse().expect("a chord")
    }

    #[test]
    fn every_chord_survives_the_round_trip_through_its_key_press() {
        for &key in Key::ALL {
            for bits in 0..8u8 {
                let mods = Mods { ctrl: bits & 1 != 0, alt: bits & 2 != 0, shift: bits & 4 != 0 };
                let Some(c) = Chord::new(mods, key) else { continue };
                let (vk, flags) = press_of(c);
                let back = chord_of(vk, flags).expect("a chord reads back");
                let typed_with_shift = match key {
                    Key::Equal | Key::Plus => Some(Key::Plus),
                    Key::Slash | Key::Question => Some(Key::Question),
                    _ => None,
                };
                let expected = match typed_with_shift {
                    Some(symbol) if mods.shift => Chord::new(Mods { shift: false, ..mods }, symbol).expect("bindable"),
                    _ => c,
                };
                assert_eq!(back, expected, "{c:?}");
            }
        }
    }

    #[test]
    fn key_presses_read_as_chords() {
        assert_eq!(chord_of(0xBB, CONTROL | SHIFT), Some(chord("Ctrl+Plus")));
        assert_eq!(chord_of(0xBB, CONTROL), Some(chord("Ctrl+Equal")));
        assert_eq!(chord_of(0x54, CONTROL), Some(chord("Ctrl+T")));
        assert_eq!(chord_of(0x09, CONTROL | SHIFT), Some(chord("Ctrl+Shift+Tab")));
        assert_eq!(chord_of(0x7A, 0), Some(chord("F11")));
        assert_eq!(chord_of(0x41, SHIFT), None, "Shift+A types text");
        assert_eq!(chord_of(0x11, CONTROL), None, "Ctrl alone is no chord");
    }
}
