//! Keyboard shortcuts.
//!
//! One table serves two paths. While XAML has focus, each binding is a `KeyboardAccelerator` on
//! the window root. While the page has focus, keys go to the engine's own window and XAML never
//! sees them (WinUI's `WebView2` does not forward `AcceleratorKeyPressed`), so:
//! - `Native` keys (reload, back/forward, find, zoom) are left to WebView2's built-in handling;
//! - `Reserved` keys are taken by a script injected into every page before the page sees them;
//! - `Overridable` keys reach the page first and are taken only if it does not
//!   `preventDefault()` them, which is how Chromium treats non-reserved browser shortcuts.
//!
//! The script runs in an isolated world, created through the DevTools protocol before the page's
//! own scripts: it shares the page's DOM but none of its JavaScript objects, so the page cannot
//! redefine what the script reads a key press through. It reports through a DevTools binding that
//! exists only in that world, so the page can neither see nor send its messages.

use serde_json::Value;

use Command as C;
use InPage::{Native, Overridable, Reserved};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    NewTab,
    NewWindow,
    CloseTab,
    ReopenClosedTab,
    FocusAddress,
    Reload,
    Back,
    Forward,
    NextTab,
    PreviousTab,
    /// Zero-based; Ctrl+1..Ctrl+8.
    SelectTab(u8),
    SelectLastTab,
    Bookmark,
    Find,
    ToggleBookmarksBar,
    ShowBookmarks,
    ShowHistory,
    ShowDownloads,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Mods(u8);

impl Mods {
    pub const NONE: Self = Self(0);
    pub const CTRL: Self = Self(1);
    pub const SHIFT: Self = Self(2);
    pub const ALT: Self = Self(4);
    const CTRL_SHIFT: Self = Self(3);

    pub fn from_bits(bits: u8) -> Option<Self> {
        (bits <= 7).then_some(Self(bits))
    }

    pub fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InPage {
    Native,
    Reserved,
    Overridable,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Binding {
    /// Windows virtual-key code; `KeyboardEvent.keyCode` carries the same value on Windows.
    pub vk: u16,
    pub mods: Mods,
    pub command: Command,
    pub in_page: InPage,
}

mod vk {
    pub const TAB: u16 = 0x09;
    pub const LEFT: u16 = 0x25;
    pub const RIGHT: u16 = 0x27;
    pub const KEY_0: u16 = 0x30;
    pub const B: u16 = 0x42;
    pub const D: u16 = 0x44;
    pub const F: u16 = 0x46;
    pub const H: u16 = 0x48;
    pub const J: u16 = 0x4A;
    pub const L: u16 = 0x4C;
    pub const N: u16 = 0x4E;
    pub const O: u16 = 0x4F;
    pub const R: u16 = 0x52;
    pub const T: u16 = 0x54;
    pub const W: u16 = 0x57;
    pub const F4: u16 = 0x73;
    pub const F5: u16 = 0x74;
    pub const F6: u16 = 0x75;
}

const fn bind(vk: u16, mods: Mods, command: Command, in_page: InPage) -> Binding {
    Binding {
        vk,
        mods,
        command,
        in_page,
    }
}

pub(crate) const BINDINGS: &[Binding] = &[
    bind(vk::T, Mods::CTRL, C::NewTab, Reserved),
    bind(vk::N, Mods::CTRL, C::NewWindow, Reserved),
    bind(vk::W, Mods::CTRL, C::CloseTab, Reserved),
    bind(vk::F4, Mods::CTRL, C::CloseTab, Reserved),
    bind(vk::T, Mods::CTRL_SHIFT, C::ReopenClosedTab, Reserved),
    bind(vk::TAB, Mods::CTRL, C::NextTab, Reserved),
    bind(vk::TAB, Mods::CTRL_SHIFT, C::PreviousTab, Reserved),
    bind(vk::L, Mods::CTRL, C::FocusAddress, Overridable),
    bind(vk::D, Mods::ALT, C::FocusAddress, Overridable),
    bind(vk::F6, Mods::NONE, C::FocusAddress, Overridable),
    bind(vk::D, Mods::CTRL, C::Bookmark, Overridable),
    bind(vk::B, Mods::CTRL_SHIFT, C::ToggleBookmarksBar, Overridable),
    bind(vk::O, Mods::CTRL_SHIFT, C::ShowBookmarks, Overridable),
    bind(vk::H, Mods::CTRL, C::ShowHistory, Overridable),
    bind(vk::J, Mods::CTRL, C::ShowDownloads, Overridable),
    bind(vk::KEY_0 + 1, Mods::CTRL, C::SelectTab(0), Overridable),
    bind(vk::KEY_0 + 2, Mods::CTRL, C::SelectTab(1), Overridable),
    bind(vk::KEY_0 + 3, Mods::CTRL, C::SelectTab(2), Overridable),
    bind(vk::KEY_0 + 4, Mods::CTRL, C::SelectTab(3), Overridable),
    bind(vk::KEY_0 + 5, Mods::CTRL, C::SelectTab(4), Overridable),
    bind(vk::KEY_0 + 6, Mods::CTRL, C::SelectTab(5), Overridable),
    bind(vk::KEY_0 + 7, Mods::CTRL, C::SelectTab(6), Overridable),
    bind(vk::KEY_0 + 8, Mods::CTRL, C::SelectTab(7), Overridable),
    bind(vk::KEY_0 + 9, Mods::CTRL, C::SelectLastTab, Overridable),
    bind(vk::R, Mods::CTRL, C::Reload, Native),
    bind(vk::F5, Mods::NONE, C::Reload, Native),
    bind(vk::LEFT, Mods::ALT, C::Back, Native),
    bind(vk::RIGHT, Mods::ALT, C::Forward, Native),
    bind(vk::F, Mods::CTRL, C::Find, Native),
];

/// A message the shortcut script sent to the host.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PageMessage {
    Key(Command),
    /// The user Ctrl+clicked or middle-clicked a link; the new-window request that follows
    /// should open a background tab.
    BackgroundLink(String),
}

/// The binding the script reports through (`Runtime.bindingCalled` events carry its name).
pub(crate) const BINDING: &str = "vsesvitShortcut";

/// The script and the isolated world it runs in.
pub(crate) struct PageScript {
    /// Secret: the engine exposes the binding to every world of this name, and extensions'
    /// content scripts run in worlds named after the extension.
    pub world: String,
    pub source: String,
}

impl PageScript {
    pub fn new(secret: &str) -> Self {
        Self {
            world: format!("vsesvit-{secret}"),
            source: page_script(),
        }
    }
}

/// A `Runtime.bindingCalled` event's parameters: a message if it is a call of `BINDING`.
pub(crate) fn parse_binding_call(event: &str) -> Option<PageMessage> {
    let value: Value = serde_json::from_str(event).ok()?;
    if value.get("name")?.as_str()? != BINDING {
        return None;
    }
    parse_page_message(value.get("payload")?.as_str()?)
}

fn parse_page_message(message: &str) -> Option<PageMessage> {
    let value: Value = serde_json::from_str(message).ok()?;
    match value.get("t")?.as_str()? {
        "key" => {
            let vk = u16::try_from(value.get("vk")?.as_u64()?).ok()?;
            let mods = Mods::from_bits(u8::try_from(value.get("m")?.as_u64()?).ok()?)?;
            let binding = BINDINGS
                .iter()
                .find(|b| b.vk == vk && b.mods == mods && b.in_page != Native)?;
            Some(PageMessage::Key(binding.command))
        }
        "link" => Some(PageMessage::BackgroundLink(
            value.get("url")?.as_str()?.to_owned(),
        )),
        _ => None,
    }
}

/// The script every new top-level document runs in the shortcut world.
fn page_script() -> String {
    let keys = |kind: InPage| {
        BINDINGS
            .iter()
            .filter(|b| b.in_page == kind)
            .map(|b| format!("\"{}:{}\"", b.vk, b.mods.bits()))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        r#"(() => {{
  const report = globalThis.{binding};
  if (typeof report !== "function" || window !== window.top) return;
  const reserved = new Set([{reserved}]);
  const overridable = new Set([{overridable}]);
  const chord = (e) => e.keyCode + ":" + ((e.ctrlKey ? 1 : 0) | (e.shiftKey ? 2 : 0) | (e.altKey ? 4 : 0));
  const send = (e) => {{
    e.preventDefault();
    e.stopImmediatePropagation();
    report(JSON.stringify({{ t: "key", vk: e.keyCode, m: (e.ctrlKey ? 1 : 0) | (e.shiftKey ? 2 : 0) | (e.altKey ? 4 : 0) }}));
  }};
  addEventListener("keydown", (e) => {{ if (e.isTrusted && reserved.has(chord(e))) send(e); }}, true);
  addEventListener("keydown", (e) => {{ if (e.isTrusted && !e.defaultPrevented && overridable.has(chord(e))) send(e); }}, false);
  const link = (e) => {{
    const a = e.target instanceof Element ? e.target.closest("a[href]") : null;
    if (a) report(JSON.stringify({{ t: "link", url: a.href }}));
  }};
  addEventListener("click", (e) => {{ if (e.isTrusted && (e.ctrlKey || e.metaKey) && !e.shiftKey) link(e); }}, true);
  addEventListener("auxclick", (e) => {{ if (e.isTrusted && e.button === 1 && !e.shiftKey) link(e); }}, true);
}})();"#,
        binding = BINDING,
        reserved = keys(Reserved),
        overridable = keys(Overridable),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(vk: u16, mods: Mods) -> Option<Command> {
        BINDINGS
            .iter()
            .find(|b| b.vk == vk && b.mods == mods)
            .map(|b| b.command)
    }

    #[test]
    fn every_chord_is_bound_once() {
        for (i, a) in BINDINGS.iter().enumerate() {
            for b in &BINDINGS[i + 1..] {
                assert!(
                    !(a.vk == b.vk && a.mods == b.mods),
                    "{a:?} and {b:?} share a chord"
                );
            }
        }
    }

    #[test]
    fn lookup_finds_commands() {
        assert_eq!(lookup(0x54, Mods::CTRL), Some(Command::NewTab));
        assert_eq!(
            lookup(0x54, Mods::CTRL_SHIFT),
            Some(Command::ReopenClosedTab)
        );
        assert_eq!(lookup(0x09, Mods::CTRL_SHIFT), Some(Command::PreviousTab));
        assert_eq!(lookup(0x33, Mods::CTRL), Some(Command::SelectTab(2)));
        assert_eq!(lookup(0x74, Mods::NONE), Some(Command::Reload));
        assert_eq!(lookup(0x54, Mods::ALT), None);
    }

    /// A `Runtime.bindingCalled` event as WebView2 hands it over.
    fn called(name: &str, payload: &str) -> String {
        serde_json::json!({ "name": name, "payload": payload, "executionContextId": 7 }).to_string()
    }

    #[test]
    fn only_calls_of_the_shortcut_binding_count() {
        let key = r#"{"t":"key","vk":87,"m":1}"#;
        assert_eq!(
            parse_binding_call(&called(BINDING, key)),
            Some(PageMessage::Key(Command::CloseTab))
        );
        assert_eq!(parse_binding_call(&called("other", key)), None);
        assert_eq!(parse_binding_call(r#"{"payload":"{}"}"#), None);
    }

    #[test]
    fn pages_cannot_trigger_native_bindings_or_garbage() {
        let reload = r#"{"t":"key","vk":82,"m":1}"#;
        assert_eq!(parse_binding_call(&called(BINDING, reload)), None);
        let bad_mods = r#"{"t":"key","vk":87,"m":9}"#;
        assert_eq!(parse_binding_call(&called(BINDING, bad_mods)), None);
        assert_eq!(parse_binding_call(&called(BINDING, "not json")), None);
        assert_eq!(parse_binding_call("not json"), None);
    }

    #[test]
    fn link_hints_parse() {
        let link = r#"{"t":"link","url":"https://a.test/x"}"#;
        assert_eq!(
            parse_binding_call(&called(BINDING, link)),
            Some(PageMessage::BackgroundLink("https://a.test/x".into()))
        );
    }

    #[test]
    fn script_lists_only_page_handled_keys() {
        let script = page_script();
        assert!(script.contains("\"87:1\""), "Ctrl+W is reserved");
        assert!(script.contains("\"68:1\""), "Ctrl+D is overridable");
        assert!(!script.contains("\"82:1\""), "Ctrl+R is left to WebView2");
    }

    #[test]
    fn the_script_reports_only_through_its_binding() {
        let script = PageScript::new("q7x9secret");
        assert_eq!(script.world, "vsesvit-q7x9secret");
        assert!(script.source.contains(&format!("globalThis.{BINDING};")));
        assert!(!script.source.contains("webview"));
        assert!(
            !script.source.contains("q7x9secret"),
            "the world's name stays out of the page"
        );
    }
}
