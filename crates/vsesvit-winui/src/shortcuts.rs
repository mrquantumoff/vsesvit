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
//! The script reports keys with `chrome.webview.postMessage`, tagged with a per-process nonce so
//! a page cannot forge them by posting its own messages.

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

/// A message the injected page script posted to the host.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PageMessage {
    Key(Command),
    /// The user Ctrl+clicked or middle-clicked a link; the new-window request that follows
    /// should open a background tab.
    BackgroundLink(String),
}

pub(crate) fn parse_page_message(message: &str, nonce: &str) -> Option<PageMessage> {
    let value: Value = serde_json::from_str(message).ok()?;
    if value.get("vsesvit")?.as_str()? != nonce {
        return None;
    }
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

/// The script added to every tab with `AddScriptToExecuteOnDocumentCreatedAsync`. It keeps its
/// own references to `postMessage` and `JSON.stringify`, taken before any page script runs.
pub(crate) fn page_script(nonce: &str) -> String {
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
  const webview = globalThis.chrome && chrome.webview;
  if (!webview || window !== window.top) return;
  const post = webview.postMessage.bind(webview);
  const stringify = JSON.stringify;
  const nonce = "{nonce}";
  const reserved = new Set([{reserved}]);
  const overridable = new Set([{overridable}]);
  const chord = (e) => e.keyCode + ":" + ((e.ctrlKey ? 1 : 0) | (e.shiftKey ? 2 : 0) | (e.altKey ? 4 : 0));
  const send = (e) => {{
    e.preventDefault();
    e.stopImmediatePropagation();
    post(stringify({{ vsesvit: nonce, t: "key", vk: e.keyCode, m: (e.ctrlKey ? 1 : 0) | (e.shiftKey ? 2 : 0) | (e.altKey ? 4 : 0) }}));
  }};
  addEventListener("keydown", (e) => {{ if (e.isTrusted && reserved.has(chord(e))) send(e); }}, true);
  addEventListener("keydown", (e) => {{ if (e.isTrusted && !e.defaultPrevented && overridable.has(chord(e))) send(e); }}, false);
  const link = (e) => {{
    const a = e.target instanceof Element ? e.target.closest("a[href]") : null;
    if (a) post(stringify({{ vsesvit: nonce, t: "link", url: a.href }}));
  }};
  addEventListener("click", (e) => {{ if (e.isTrusted && (e.ctrlKey || e.metaKey) && !e.shiftKey) link(e); }}, true);
  addEventListener("auxclick", (e) => {{ if (e.isTrusted && e.button === 1 && !e.shiftKey) link(e); }}, true);
}})();"#,
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

    #[test]
    fn page_messages_need_the_nonce() {
        let key = r#"{"vsesvit":"n1","t":"key","vk":87,"m":1}"#;
        assert_eq!(
            parse_page_message(key, "n1"),
            Some(PageMessage::Key(Command::CloseTab))
        );
        assert_eq!(parse_page_message(key, "n2"), None);
        assert_eq!(
            parse_page_message(r#"{"t":"key","vk":87,"m":1}"#, "n1"),
            None
        );
    }

    #[test]
    fn pages_cannot_trigger_native_bindings_or_garbage() {
        let reload = r#"{"vsesvit":"n","t":"key","vk":82,"m":1}"#;
        assert_eq!(parse_page_message(reload, "n"), None);
        assert_eq!(
            parse_page_message(r#"{"vsesvit":"n","t":"key","vk":87,"m":9}"#, "n"),
            None
        );
        assert_eq!(parse_page_message("not json", "n"), None);
    }

    #[test]
    fn link_hints_parse() {
        let link = r#"{"vsesvit":"n","t":"link","url":"https://a.test/x"}"#;
        assert_eq!(
            parse_page_message(link, "n"),
            Some(PageMessage::BackgroundLink("https://a.test/x".into()))
        );
    }

    #[test]
    fn script_lists_only_page_handled_keys() {
        let script = page_script("abc");
        assert!(script.contains("\"87:1\""), "Ctrl+W is reserved");
        assert!(script.contains("\"68:1\""), "Ctrl+D is overridable");
        assert!(!script.contains("\"82:1\""), "Ctrl+R is left to WebView2");
        assert!(script.contains("const nonce = \"abc\""));
    }
}
