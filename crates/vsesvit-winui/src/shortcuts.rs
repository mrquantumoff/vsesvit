//! Keyboard shortcuts.
//!
//! Core's keymap says which chords run which command; `Bindings` turns it into Windows key
//! codes for the commands this shell implements, and serves two paths. While XAML has focus,
//! each binding is a `KeyboardAccelerator` on the window root. While the page has focus, keys go
//! to the engine's own window and XAML never sees them (WinUI's `WebView2` does not forward
//! `AcceleratorKeyPressed`), so:
//! - `Native` keys (reload, back/forward, find, print, developer tools) are left to WebView2's
//!   built-in handling;
//! - `Reserved` keys are taken by a script injected into every page before the page sees them;
//! - `Overridable` keys reach the page first and are taken only if it does not
//!   `preventDefault()` them, which is how Chromium treats non-reserved browser shortcuts.
//!
//! The script runs in an isolated world, created through the DevTools protocol before the page's
//! own scripts: it shares the page's DOM but none of its JavaScript objects, so the page cannot
//! redefine what the script reads a key press through, nor read or change the key sets the
//! script holds. It reports through a DevTools binding that exists only in that world, so the
//! page can neither see nor send its messages. Frames from other sites run in processes of their
//! own, which the page's DevTools session does not reach: each gets a session of its own
//! (`AUTO_ATTACH`), set up the same way before its first document runs.
//!
//! Extensions' action commands (`_execute_action` and its MV2 forms) are bound too, `Reserved`,
//! as Chrome handles extension shortcuts before the page. Their named commands are not: the shell
//! has no way to fire `commands.onCommand`, so their chords stay the page's.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;
use vsesvit_core::extensions::commands::{ExtensionCommand, ExtensionShortcuts};
use vsesvit_core::shortcuts::InPage::{self, Native, Overridable, Reserved};
use vsesvit_core::shortcuts::{self as keymap, Chord, Keymap};

use crate::popup::ExtensionAction;
use crate::store::{self, StoreRequest};
use keymap::Command as Core;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    NewTab,
    NewWindow,
    /// Chrome's New incognito window.
    NewPrivateWindow,
    CloseTab,
    ReopenClosedTab,
    /// Chrome's tab search: the open tabs of every window and the recently closed ones.
    SearchTabs,
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
    /// Collapses or expands the vertical tab list. Chrome saves the page with Ctrl+S; Vsesvit
    /// gives the chord to its tab sidebar instead.
    ToggleTabPane,
    /// Copies the page's address without its tracking parameters.
    CopyCleanLink,
    /// Copies the page's address as it is.
    CopyLink,
    /// WebView2's Save As dialog for the page: HTML pages and other files (images, PDFs) alike.
    SavePage,
    /// WebView2's print preview.
    Print,
    /// The page's `view-source:` address in a new tab.
    ViewSource,
    DeveloperTools,
    /// WebView2 cannot open DevTools on a given panel, so this opens them as `DeveloperTools`
    /// does; in the page, WebView2 itself opens the console on Ctrl+Shift+J.
    JavaScriptConsole,
    /// Opens an extension's popup as its toolbar button does; `Bindings::extension_action`
    /// says whose.
    ExtensionAction(usize),
}

/// Every core command this shell implements, with its shell command; core says where each is
/// taken while the page has focus (`Core::in_page`). Zoom, fullscreen, find next/previous,
/// reload without the cache, quit, Settings and the shortcuts list are missing: WebView2 or
/// nothing handles them.
const IMPLEMENTED: &[(Core, Command)] = &[
    (Core::NewTab, Command::NewTab),
    (Core::CloseTab, Command::CloseTab),
    (Core::ReopenClosedTab, Command::ReopenClosedTab),
    (Core::SearchTabs, Command::SearchTabs),
    (Core::NextTab, Command::NextTab),
    (Core::PreviousTab, Command::PreviousTab),
    (Core::SelectTab1, Command::SelectTab(0)),
    (Core::SelectTab2, Command::SelectTab(1)),
    (Core::SelectTab3, Command::SelectTab(2)),
    (Core::SelectTab4, Command::SelectTab(3)),
    (Core::SelectTab5, Command::SelectTab(4)),
    (Core::SelectTab6, Command::SelectTab(5)),
    (Core::SelectTab7, Command::SelectTab(6)),
    (Core::SelectTab8, Command::SelectTab(7)),
    (Core::SelectLastTab, Command::SelectLastTab),
    (Core::ToggleTabList, Command::ToggleTabPane),
    (Core::NewWindow, Command::NewWindow),
    (Core::NewPrivateWindow, Command::NewPrivateWindow),
    (Core::FocusAddress, Command::FocusAddress),
    (Core::Back, Command::Back),
    (Core::Forward, Command::Forward),
    (Core::Reload, Command::Reload),
    (Core::SavePage, Command::SavePage),
    (Core::Print, Command::Print),
    // WebView2 has no Ctrl+U of its own.
    (Core::ViewSource, Command::ViewSource),
    (Core::DeveloperTools, Command::DeveloperTools),
    (Core::JavaScriptConsole, Command::JavaScriptConsole),
    (Core::Find, Command::Find),
    (Core::BookmarkPage, Command::Bookmark),
    (Core::CopyCleanLink, Command::CopyCleanLink),
    (Core::CopyLink, Command::CopyLink),
    (Core::ToggleBookmarksBar, Command::ToggleBookmarksBar),
    (Core::ShowBookmarks, Command::ShowBookmarks),
    (Core::ShowHistory, Command::ShowHistory),
    (Core::ShowDownloads, Command::ShowDownloads),
];

fn implemented(core: Core) -> Option<Command> {
    IMPLEMENTED
        .iter()
        .find(|(c, _)| *c == core)
        .map(|&(_, command)| command)
}

/// The core commands this shell implements, in core's order, for the shortcuts list.
pub(crate) fn listed() -> impl Iterator<Item = Core> {
    Core::ALL
        .iter()
        .copied()
        .filter(|&c| implemented(c).is_some())
}

fn core_of(command: Command) -> Option<Core> {
    IMPLEMENTED
        .iter()
        .find(|(_, c)| *c == command)
        .map(|&(core, _)| core)
}

/// Modifiers as the page script and XAML encode them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Mods(u8);

impl Mods {
    pub const NONE: Self = Self(0);
    pub const CTRL: Self = Self(1);
    pub const SHIFT: Self = Self(2);
    pub const ALT: Self = Self(4);

    pub fn from_bits(bits: u8) -> Option<Self> {
        (bits <= 7).then_some(Self(bits))
    }

    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn of(ctrl: bool, shift: bool, alt: bool) -> Self {
        Self(u8::from(ctrl) | u8::from(shift) << 1 | u8::from(alt) << 2)
    }

    pub fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn core(self) -> keymap::Mods {
        keymap::Mods {
            ctrl: self.has(Self::CTRL),
            alt: self.has(Self::ALT),
            shift: self.has(Self::SHIFT),
        }
    }
}

/// The key code and modifiers that type `chord` (`Chord::vk`), as the page script and XAML
/// encode them.
fn vk_chord(chord: Chord) -> (u16, Mods) {
    let (vk, m) = chord.vk();
    (vk, Mods::of(m.ctrl, m.shift, m.alt))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    /// Windows virtual-key code; `KeyboardEvent.keyCode` carries the same value on Windows.
    pub vk: u16,
    pub mods: Mods,
    pub command: Command,
    pub in_page: InPage,
}

/// The effective key bindings of the commands this shell implements.
#[derive(PartialEq)]
pub(crate) struct Bindings {
    keymap: Keymap,
    list: Vec<Binding>,
    /// Default chords of `Native` commands that no longer run them and run nothing this shell
    /// implements: the script takes them so WebView2 does not act on them.
    swallowed: Vec<(u16, Mods)>,
    extensions: ExtensionShortcuts,
    /// The engine id of the extension each `Command::ExtensionAction` opens, and its chord.
    extension_actions: Vec<(String, Chord)>,
}

/// Binds `chord` unless its key press is bound already, and says whether it did: Ctrl+Plus and
/// Ctrl+Shift+Equal are one key press.
fn bind(list: &mut Vec<Binding>, chord: Chord, command: Command, in_page: InPage) -> bool {
    let (vk, mods) = vk_chord(chord);
    let free = !list.iter().any(|b| b.vk == vk && b.mods == mods);
    if free {
        list.push(Binding {
            vk,
            mods,
            command,
            in_page,
        });
    }
    free
}

impl Bindings {
    /// `commands` are the enabled extensions' commands and `actions` the toolbar's: an action
    /// command is bound once the engine has loaded its extension.
    pub fn new(
        keymap: Keymap,
        commands: Vec<ExtensionCommand>,
        actions: &[ExtensionAction],
    ) -> Self {
        let mut list: Vec<Binding> = Vec::new();
        for (core, chords) in keymap.iter() {
            let Some(command) = implemented(core) else {
                continue;
            };
            for &chord in chords {
                bind(&mut list, chord, command, core.in_page_on(chord));
            }
        }
        let extensions = keymap.extension_shortcuts(commands);
        let mut extension_actions = Vec::new();
        let action_commands = extensions
            .iter()
            .filter(|(c, _)| c.command.activates_action());
        for (command, chord) in action_commands {
            let action = actions.iter().find(|a| a.id == command.extension.as_str());
            let index = extension_actions.len();
            if let (Some(chord), Some(action)) = (chord, action)
                && bind(&mut list, chord, Command::ExtensionAction(index), Reserved)
            {
                extension_actions.push((action.extension_id.clone(), chord));
            }
        }
        let swallowed = IMPLEMENTED
            .iter()
            .filter(|(core, _)| core.in_page() == Native)
            .flat_map(|&(core, _)| core.defaults().iter().map(move |&chord| (core, chord)))
            .filter(|&(core, chord)| {
                keymap
                    .command_for(chord)
                    .is_none_or(|owner| owner != core && implemented(owner).is_none())
            })
            .map(|(_, chord)| vk_chord(chord))
            .collect();
        Self {
            keymap,
            list,
            swallowed,
            extensions,
            extension_actions,
        }
    }

    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// The enabled extensions' commands and their shortcuts, bound or not.
    pub fn extensions(&self) -> &ExtensionShortcuts {
        &self.extensions
    }

    /// The engine id of the extension whose popup `Command::ExtensionAction(index)` opens.
    pub fn extension_action(&self, index: usize) -> Option<&str> {
        let (engine_id, _) = self.extension_actions.get(index)?;
        Some(engine_id)
    }

    pub fn list(&self) -> &[Binding] {
        &self.list
    }

    /// The command a key press the page script reported runs. Never a `Native` one: WebView2
    /// runs those itself, and the script does not report them.
    fn reported(&self, vk: u16, mods: Mods) -> Option<Command> {
        self.list
            .iter()
            .find(|b| b.vk == vk && b.mods == mods && b.in_page != Native)
            .map(|b| b.command)
    }

    /// How menus and tooltips name the command's shortcut: its first chord.
    pub fn label(&self, command: Command) -> Option<String> {
        let chord = match command {
            Command::ExtensionAction(index) => self.extension_actions.get(index).map(|&(_, c)| c),
            command => core_of(command).and_then(|core| self.keymap.chords(core).first().copied()),
        };
        chord.map(|chord| chord.to_string())
    }

    /// `text` with the command's shortcut in parentheses, for a tooltip.
    pub fn tip(&self, text: &str, command: Command) -> String {
        match self.label(command) {
            Some(label) => format!("{text} ({label})"),
            None => text.to_owned(),
        }
    }

    /// Sets the key sets in the shortcut world (see `page_script`).
    pub fn keys_script(&self) -> String {
        let set = |keys: &mut dyn Iterator<Item = (u16, Mods)>| {
            keys.map(|(vk, mods)| format!("\"{vk}:{}\"", mods.bits()))
                .collect::<Vec<_>>()
                .join(",")
        };
        let kind = |kind: InPage| {
            set(&mut self
                .list
                .iter()
                .filter(move |b| b.in_page == kind)
                .map(|b| (b.vk, b.mods)))
        };
        format!(
            "globalThis.{KEYS} = {{ reserved: new Set([{}]), overridable: new Set([{}]), swallowed: new Set([{}]) }};",
            kind(Reserved),
            kind(Overridable),
            set(&mut self.swallowed.iter().copied()),
        )
    }
}

thread_local! {
    static CURRENT: RefCell<Rc<Bindings>> =
        RefCell::new(Rc::new(Bindings::new(Keymap::default(), Vec::new(), &[])));
}

/// The bindings in effect, for every window of this (UI) thread.
pub(crate) fn current() -> Rc<Bindings> {
    CURRENT.with_borrow(Rc::clone)
}

/// Puts `bindings` in effect; false when they already are.
pub(crate) fn set_current(bindings: Bindings) -> bool {
    let changed = *current() != bindings;
    if changed {
        CURRENT.set(Rc::new(bindings));
    }
    changed
}

/// A message the shortcut script sent to the host.
#[derive(Debug, PartialEq)]
pub(crate) enum PageMessage {
    Key(Command),
    /// The user Ctrl+clicked or middle-clicked a link; the new-window request that follows
    /// should open a background tab.
    BackgroundLink(String),
    /// An extension store page asked to install, remove or list extensions (see `store`).
    Store(StoreRequest),
    /// The page's `devicePixelRatio`, when the document starts (`start`) and whenever it
    /// changes (see `zoom`).
    Zoom { ratio: f64, start: bool },
}

/// The binding the script reports through (`Runtime.bindingCalled` events carry its name).
pub(crate) const BINDING: &str = "vsesvitShortcut";

/// Where the shortcut world keeps its key sets.
const KEYS: &str = "vsesvitKeys";

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

/// The shortcut world in one engine view: what keeps its key sets current.
#[derive(Debug, Default)]
pub(crate) struct World {
    pub name: String,
    /// The identifier of the new-document script that sets the key sets, to remove it once a
    /// newer one replaces it.
    pub keys_script: Option<String>,
    /// The world's execution contexts in loaded documents.
    pub contexts: Vec<i64>,
}

/// The DevTools events `World::track` follows.
pub(crate) const CONTEXT_EVENTS: [&str; 3] = [
    "Runtime.executionContextCreated",
    "Runtime.executionContextDestroyed",
    "Runtime.executionContextsCleared",
];

impl World {
    /// Follows one of `CONTEXT_EVENTS`, given its parameters.
    pub fn track(&mut self, event: &str, params: &str) {
        let Ok(value) = serde_json::from_str::<Value>(params) else {
            return;
        };
        match event {
            "Runtime.executionContextCreated" => {
                let context = &value["context"];
                if context["name"].as_str() == Some(self.name.as_str())
                    && let Some(id) = context["id"].as_i64()
                {
                    self.contexts.push(id);
                }
            }
            "Runtime.executionContextDestroyed" => {
                if let Some(id) = value["executionContextId"].as_i64() {
                    self.contexts.retain(|&c| c != id);
                }
            }
            "Runtime.executionContextsCleared" => self.contexts.clear(),
            _ => {}
        }
    }
}

/// A `Runtime.bindingCalled` event's parameters: a message if it is a call of `BINDING`. A frame
/// with a session of its own (`in_frame`) only reports keys; the rest is the top document's.
pub(crate) fn parse_binding_call(
    event: &str,
    bindings: &Bindings,
    in_frame: bool,
) -> Option<PageMessage> {
    let value: Value = serde_json::from_str(event).ok()?;
    if value.get("name")?.as_str()? != BINDING {
        return None;
    }
    parse_page_message(value.get("payload")?.as_str()?, bindings)
        .filter(|message| !in_frame || matches!(message, PageMessage::Key(_)))
}

/// `Target.setAutoAttach` parameters for every DevTools session of a tab: it attaches a session
/// to each frame from another site, which waits to start until `Runtime.runIfWaitingForDebugger`.
pub(crate) const AUTO_ATTACH: &str = r#"{"autoAttach":true,"waitForDebuggerOnStart":true,
    "flatten":true,"filter":[{"type":"iframe"}]}"#;

/// A `Target.attachedToTarget` event's parameters: the new session, and whether it is a frame's.
pub(crate) fn attached_session(params: &str) -> Option<(String, bool)> {
    let value: Value = serde_json::from_str(params).ok()?;
    let session = value.get("sessionId")?.as_str()?.to_owned();
    let frame = value.pointer("/targetInfo/type").and_then(Value::as_str) == Some("iframe");
    Some((session, frame))
}

/// The DevTools calls that set up the shortcut world in a session, before its documents load.
/// Scripts for new documents need the Page domain on, and a binding reaches the worlds created
/// later only while the Runtime domain is on.
pub(crate) fn world_calls(script: &PageScript) -> [(&'static str, String); 4] {
    [
        ("Page.enable", "{}".to_owned()),
        ("Runtime.enable", "{}".to_owned()),
        (
            "Runtime.addBinding",
            serde_json::json!({ "name": BINDING, "executionContextName": script.world })
                .to_string(),
        ),
        (
            "Page.addScriptToEvaluateOnNewDocument",
            serde_json::json!({ "source": script.source, "worldName": script.world })
                .to_string(),
        ),
    ]
}

fn parse_page_message(message: &str, bindings: &Bindings) -> Option<PageMessage> {
    let value: Value = serde_json::from_str(message).ok()?;
    match value.get("t")?.as_str()? {
        "key" => {
            let vk = u16::try_from(value.get("vk")?.as_u64()?).ok()?;
            let mods = Mods::from_bits(u8::try_from(value.get("m")?.as_u64()?).ok()?)?;
            bindings.reported(vk, mods).map(PageMessage::Key)
        }
        "link" => Some(PageMessage::BackgroundLink(
            value.get("url")?.as_str()?.to_owned(),
        )),
        "zoom" => Some(PageMessage::Zoom {
            ratio: value.get("dpr")?.as_f64()?,
            start: value.get("start")?.as_bool()?,
        }),
        "store" => {
            store::parse_request(value.get("origin")?.as_str()?, value.get("detail")?.as_str()?)
                .map(PageMessage::Store)
        }
        _ => None,
    }
}

/// The script every new document runs in the shortcut world. It reads the key sets at each key
/// press, so `Bindings::keys_script` can change them in a loaded document. Frames report keys
/// too, since a key pressed in a focused frame never reaches the top document (frames from
/// other sites get the script through sessions of their own, see `AUTO_ATTACH`); only the top
/// document reports links, zoom and store requests.
fn page_script() -> String {
    format!(
        r#"(() => {{
  const report = globalThis.{BINDING};
  if (typeof report !== "function") return;
  const none = new Set();
  const keys = (kind) => (globalThis.{KEYS} || {{}})[kind] || none;
  const mods = (e) => (e.ctrlKey ? 1 : 0) | (e.shiftKey ? 2 : 0) | (e.altKey ? 4 : 0);
  const chord = (e) => e.keyCode + ":" + mods(e);
  const send = (e) => {{
    e.preventDefault();
    e.stopImmediatePropagation();
    report(JSON.stringify({{ t: "key", vk: e.keyCode, m: mods(e) }}));
  }};
  addEventListener("keydown", (e) => {{
    if (!e.isTrusted) return;
    if (keys("reserved").has(chord(e))) send(e);
    else if (keys("swallowed").has(chord(e))) e.preventDefault();
  }}, true);
  addEventListener("keydown", (e) => {{ if (e.isTrusted && !e.defaultPrevented && keys("overridable").has(chord(e))) send(e); }}, false);
  if (window !== window.top) return;
  const link = (e) => {{
    const a = e.target instanceof Element ? e.target.closest("a[href]") : null;
    if (a) report(JSON.stringify({{ t: "link", url: a.href }}));
  }};
  addEventListener("click", (e) => {{ if (e.isTrusted && (e.ctrlKey || e.metaKey) && !e.shiftKey) link(e); }}, true);
  addEventListener("auxclick", (e) => {{ if (e.isTrusted && e.button === 1 && !e.shiftKey) link(e); }}, true);
  let ratio = 0;
  let start = true;
  const zoom = () => {{
    if (devicePixelRatio === ratio) return;
    ratio = devicePixelRatio;
    report(JSON.stringify({{ t: "zoom", dpr: ratio, start }}));
    start = false;
    matchMedia("(resolution: " + ratio + "dppx)").addEventListener("change", zoom, {{ once: true }});
  }};
  zoom();
  addEventListener("resize", zoom);
  document.addEventListener("vsesvit-store", (e) => {{
    if (typeof e.detail === "string") report(JSON.stringify({{ t: "store", origin: location.origin, detail: e.detail }}));
  }});
}})();"#
    )
}

#[cfg(test)]
mod tests {
    use vsesvit_core::extensions::ExtensionId;
    use vsesvit_core::extensions::manifest::ManifestCommand;

    use super::*;

    const CTRL_SHIFT: Mods = Mods(3);
    const CTRL_ALT_SHIFT: Mods = Mods(7);

    fn defaults() -> Bindings {
        Bindings::new(Keymap::default(), Vec::new(), &[])
    }

    fn with(edit: impl FnOnce(&mut Keymap)) -> Bindings {
        let mut keymap = Keymap::default();
        edit(&mut keymap);
        Bindings::new(keymap, Vec::new(), &[])
    }

    fn chord(s: &str) -> Chord {
        s.parse().unwrap()
    }

    fn lookup(bindings: &Bindings, vk: u16, mods: Mods) -> Option<Binding> {
        bindings
            .list()
            .iter()
            .find(|b| b.vk == vk && b.mods == mods)
            .copied()
    }

    #[test]
    fn only_commands_this_shell_implements_are_bound_or_listed() {
        let absent = [
            Core::Quit,
            Core::ReloadBypassCache,
            Core::FindNext,
            Core::FindPrevious,
            Core::ZoomIn,
            Core::ZoomOut,
            Core::ZoomReset,
            Core::Fullscreen,
            Core::ShowSettings,
            Core::ShowShortcuts,
        ];
        let listed: Vec<Core> = listed().collect();
        assert_eq!(listed.len() + absent.len(), Core::ALL.len());
        assert!(absent.iter().all(|c| !listed.contains(c)));
        let bindings = defaults();
        assert!(
            lookup(&bindings, 0x51, Mods::CTRL).is_none(),
            "Ctrl+Q quits nothing"
        );
        assert!(
            lookup(&bindings, 0x7A, Mods::NONE).is_none(),
            "F11 is WebView2's"
        );
        for (i, (_, a)) in IMPLEMENTED.iter().enumerate() {
            assert!(IMPLEMENTED[i + 1..].iter().all(|(_, b)| a != b));
        }
    }

    #[test]
    fn every_chord_is_bound_once() {
        let bindings = defaults();
        for (i, a) in bindings.list().iter().enumerate() {
            for b in &bindings.list()[i + 1..] {
                assert!(
                    !(a.vk == b.vk && a.mods == b.mods),
                    "{a:?} and {b:?} share a chord"
                );
            }
        }
    }

    #[test]
    fn defaults_bind_as_before() {
        let bindings = defaults();
        let find = |vk, mods| lookup(&bindings, vk, mods).map(|b| (b.command, b.in_page));
        assert_eq!(find(0x54, Mods::CTRL), Some((Command::NewTab, Reserved)));
        assert_eq!(
            find(0x54, CTRL_SHIFT),
            Some((Command::ReopenClosedTab, Reserved))
        );
        assert_eq!(
            find(0x09, CTRL_SHIFT),
            Some((Command::PreviousTab, Reserved))
        );
        assert_eq!(
            find(0x41, CTRL_SHIFT),
            Some((Command::SearchTabs, Overridable))
        );
        assert_eq!(
            find(0x33, Mods::CTRL),
            Some((Command::SelectTab(2), Overridable))
        );
        assert_eq!(find(0x74, Mods::NONE), Some((Command::Reload, Native)));
        assert_eq!(find(0x52, Mods::CTRL), Some((Command::Reload, Native)));
        assert_eq!(find(0x46, Mods::CTRL), Some((Command::Find, Native)));
        assert_eq!(find(0x25, Mods::ALT), Some((Command::Back, Native)));
        assert_eq!(
            find(0x53, Mods::CTRL),
            Some((Command::ToggleTabPane, Overridable))
        );
        assert_eq!(
            find(0x78, Mods::NONE),
            Some((Command::ToggleTabPane, Overridable))
        );
        assert_eq!(
            find(0x53, CTRL_SHIFT),
            Some((Command::SavePage, Overridable))
        );
        assert_eq!(
            find(0x43, CTRL_SHIFT),
            Some((Command::CopyCleanLink, Reserved))
        );
        assert_eq!(
            find(0x43, CTRL_ALT_SHIFT),
            Some((Command::CopyLink, Reserved))
        );
        assert_eq!(find(0x50, Mods::CTRL), Some((Command::Print, Native)));
        assert_eq!(
            find(0x55, Mods::CTRL),
            Some((Command::ViewSource, Overridable))
        );
        assert_eq!(
            find(0x49, CTRL_SHIFT),
            Some((Command::DeveloperTools, Native))
        );
        assert_eq!(
            find(0x7B, Mods::NONE),
            Some((Command::DeveloperTools, Native))
        );
        assert_eq!(
            find(0x4A, CTRL_SHIFT),
            Some((Command::JavaScriptConsole, Native))
        );
        assert_eq!(find(0x54, Mods::ALT), None);
        assert!(bindings.swallowed.is_empty());
        assert_eq!(
            bindings.label(Command::ToggleTabPane).as_deref(),
            Some("Ctrl+S")
        );
        assert_eq!(
            bindings.tip("Downloads", Command::ShowDownloads),
            "Downloads (Ctrl+J)"
        );
    }

    #[test]
    fn a_reassigned_command_follows_its_new_chord() {
        let bindings = with(|k| {
            k.assign(Core::ShowHistory, [chord("Ctrl+Shift+Y")]);
        });
        assert_eq!(
            bindings.reported(0x59, CTRL_SHIFT),
            Some(Command::ShowHistory)
        );
        assert_eq!(bindings.reported(0x48, Mods::CTRL), None);
        assert!(lookup(&bindings, 0x48, Mods::CTRL).is_none());
        assert_eq!(
            bindings.tip("History", Command::ShowHistory),
            "History (Ctrl+Shift+Y)"
        );
    }

    #[test]
    fn a_native_default_given_to_another_command_is_reported_for_it() {
        let bindings = with(|k| {
            k.assign(Core::NewTab, [chord("Ctrl+R")]);
        });
        assert_eq!(
            lookup(&bindings, 0x52, Mods::CTRL).map(|b| (b.command, b.in_page)),
            Some((Command::NewTab, Reserved))
        );
        assert_eq!(bindings.reported(0x52, Mods::CTRL), Some(Command::NewTab));
        assert_eq!(
            lookup(&bindings, 0x74, Mods::NONE).map(|b| b.in_page),
            Some(Native)
        );
        assert!(bindings.swallowed.is_empty());
        assert_eq!(
            bindings.reported(0x54, Mods::CTRL),
            None,
            "Ctrl+T left new tab"
        );
    }

    #[test]
    fn a_native_command_on_a_new_chord_is_reported_and_its_defaults_swallowed() {
        let bindings = with(|k| {
            k.assign(Core::Reload, [chord("Ctrl+Shift+Y")]);
        });
        assert_eq!(
            lookup(&bindings, 0x59, CTRL_SHIFT).map(|b| (b.command, b.in_page)),
            Some((Command::Reload, Overridable))
        );
        assert_eq!(bindings.reported(0x59, CTRL_SHIFT), Some(Command::Reload));
        assert_eq!(bindings.swallowed, [(0x52, Mods::CTRL), (0x74, Mods::NONE)]);
        let script = bindings.keys_script();
        assert!(
            script.contains("swallowed: new Set([\"82:1\",\"116:0\"])"),
            "{script}"
        );
    }

    #[test]
    fn developer_tools_moved_away_swallow_both_of_webview2s_chords() {
        let bindings = with(|k| {
            k.assign(Core::DeveloperTools, [chord("Ctrl+Shift+Y")]);
        });
        assert_eq!(
            bindings.reported(0x59, CTRL_SHIFT),
            Some(Command::DeveloperTools)
        );
        assert_eq!(bindings.swallowed, [(0x49, CTRL_SHIFT), (0x7B, Mods::NONE)]);
    }

    #[test]
    fn a_native_default_owned_by_a_command_this_shell_lacks_is_swallowed() {
        let bindings = with(|k| {
            k.assign(Core::ZoomIn, [chord("Ctrl+F")]);
        });
        assert!(lookup(&bindings, 0x46, Mods::CTRL).is_none());
        assert_eq!(bindings.swallowed, [(0x46, Mods::CTRL)]);
        let unbound = with(|k| {
            k.assign(Core::Back, []);
        });
        assert_eq!(unbound.swallowed, [(0x25, Mods::ALT)]);
    }

    #[test]
    fn reset_all_restores_the_defaults() {
        let mut keymap = Keymap::default();
        keymap.assign(Core::NewTab, [chord("Ctrl+R")]);
        keymap.assign(Core::ToggleTabList, []);
        keymap.reset_all();
        let bindings = Bindings::new(keymap, Vec::new(), &[]);
        assert_eq!(bindings.list(), defaults().list());
        assert_eq!(bindings.keys_script(), defaults().keys_script());
    }

    #[test]
    fn the_world_tracks_only_its_own_contexts() {
        let mut world = World {
            name: "vsesvit-s".into(),
            ..World::default()
        };
        let created = |id: i64, name: &str| {
            serde_json::json!({ "context": { "id": id, "name": name, "origin": "https://a.test" } })
                .to_string()
        };
        world.track(CONTEXT_EVENTS[0], &created(3, "vsesvit-s"));
        world.track(CONTEXT_EVENTS[0], &created(4, ""));
        world.track(CONTEXT_EVENTS[0], &created(5, "vsesvit-s"));
        assert_eq!(world.contexts, [3, 5]);
        world.track(CONTEXT_EVENTS[1], r#"{"executionContextId":3}"#);
        assert_eq!(world.contexts, [5]);
        world.track(CONTEXT_EVENTS[2], "{}");
        assert!(world.contexts.is_empty());
    }

    /// A `Runtime.bindingCalled` event as WebView2 hands it over.
    fn called(name: &str, payload: &str) -> String {
        serde_json::json!({ "name": name, "payload": payload, "executionContextId": 7 }).to_string()
    }

    fn parse(event: &str) -> Option<PageMessage> {
        parse_binding_call(event, &defaults(), false)
    }

    #[test]
    fn only_calls_of_the_shortcut_binding_count() {
        let key = r#"{"t":"key","vk":87,"m":1}"#;
        assert_eq!(
            parse(&called(BINDING, key)),
            Some(PageMessage::Key(Command::CloseTab))
        );
        assert_eq!(parse(&called("other", key)), None);
        assert_eq!(parse(r#"{"payload":"{}"}"#), None);
    }

    #[test]
    fn frames_with_sessions_of_their_own_only_report_keys() {
        let in_frame =
            |payload: &str| parse_binding_call(&called(BINDING, payload), &defaults(), true);
        assert_eq!(
            in_frame(r#"{"t":"key","vk":84,"m":1}"#),
            Some(PageMessage::Key(Command::NewTab))
        );
        assert_eq!(in_frame(r#"{"t":"link","url":"https://a.test/x"}"#), None);
        // A frame's document starting would otherwise re-arm the top page's zoom restore.
        let zoom = r#"{"t":"zoom","dpr":2.0,"start":true}"#;
        assert!(parse(&called(BINDING, zoom)).is_some());
        assert_eq!(in_frame(zoom), None);
        let store = r#"{"t":"store","origin":"https://chromewebstore.google.com","detail":"{\"seq\":1,\"op\":\"list\"}"}"#;
        assert!(parse(&called(BINDING, store)).is_some());
        assert_eq!(in_frame(store), None);
    }

    #[test]
    fn frames_from_other_sites_attach_paused_in_flat_sessions() {
        let params: Value = serde_json::from_str(AUTO_ATTACH).unwrap();
        assert_eq!(params["autoAttach"], true);
        assert_eq!(params["waitForDebuggerOnStart"], true);
        assert_eq!(params["flatten"], true);
        let attached = |kind: &str| {
            serde_json::json!({
                "sessionId": "S1",
                "targetInfo": { "targetId": "T1", "type": kind, "url": "https://b.test/" },
                "waitingForDebugger": true,
            })
            .to_string()
        };
        assert_eq!(attached_session(&attached("iframe")), Some(("S1".into(), true)));
        assert_eq!(attached_session(&attached("worker")), Some(("S1".into(), false)));
        assert_eq!(attached_session("{}"), None);
    }

    #[test]
    fn a_frame_session_gets_the_shortcut_world() {
        let script = PageScript::new("s");
        let calls = world_calls(&script);
        let methods: Vec<&str> = calls.iter().map(|(m, _)| *m).collect();
        assert_eq!(
            methods,
            [
                "Page.enable",
                "Runtime.enable",
                "Runtime.addBinding",
                "Page.addScriptToEvaluateOnNewDocument"
            ]
        );
        assert!(calls[2].1.contains("vsesvit-s") && calls[2].1.contains(BINDING));
        assert!(calls[3].1.contains("vsesvit-s"));
    }

    #[test]
    fn pages_cannot_trigger_native_bindings_or_garbage() {
        let reload = r#"{"t":"key","vk":82,"m":1}"#;
        assert_eq!(parse(&called(BINDING, reload)), None);
        let bad_mods = r#"{"t":"key","vk":87,"m":9}"#;
        assert_eq!(parse(&called(BINDING, bad_mods)), None);
        assert_eq!(parse(&called(BINDING, "not json")), None);
        assert_eq!(parse("not json"), None);
    }

    #[test]
    fn link_hints_parse() {
        let link = r#"{"t":"link","url":"https://a.test/x"}"#;
        assert_eq!(
            parse(&called(BINDING, link)),
            Some(PageMessage::BackgroundLink("https://a.test/x".into()))
        );
    }

    #[test]
    fn zoom_reports_carry_the_pixel_ratio_and_whether_the_document_just_started() {
        let start = r#"{"t":"zoom","dpr":1.75,"start":true}"#;
        assert_eq!(
            parse(&called(BINDING, start)),
            Some(PageMessage::Zoom {
                ratio: 1.75,
                start: true
            })
        );
        let zoom = r#"{"t":"zoom","dpr":1.925,"start":false}"#;
        assert_eq!(
            parse(&called(BINDING, zoom)),
            Some(PageMessage::Zoom {
                ratio: 1.925,
                start: false
            })
        );
        assert!(page_script().contains("start = false"));
        let bad = r#"{"t":"zoom","dpr":"big"}"#;
        assert_eq!(parse(&called(BINDING, bad)), None);
    }

    #[test]
    fn store_requests_carry_the_senders_origin() {
        assert!(page_script().contains("origin: location.origin"));
        let request = r#"{"t":"store","origin":"https://chromewebstore.google.com","detail":"{\"seq\":1,\"op\":\"list\"}"}"#;
        assert!(matches!(
            parse(&called(BINDING, request)),
            Some(PageMessage::Store(_))
        ));
        for elsewhere in [
            "https://example.com",
            "http://chromewebstore.google.com",
            "https://chromewebstore.google.com.evil.test",
        ] {
            let request = request.replace("https://chromewebstore.google.com", elsewhere);
            assert_eq!(parse(&called(BINDING, &request)), None, "{elsewhere}");
        }
    }

    #[test]
    fn the_key_sets_list_only_page_handled_keys() {
        let script = defaults().keys_script();
        assert!(script.contains("\"87:1\""), "Ctrl+W is reserved");
        assert!(script.contains("\"68:1\""), "Ctrl+D is overridable");
        assert!(
            script.contains("\"83:1\""),
            "Ctrl+S is overridable, so pages keep their own"
        );
        assert!(
            script.contains("\"83:3\""),
            "Ctrl+Shift+S is overridable too"
        );
        assert!(!script.contains("\"82:1\""), "Ctrl+R is left to WebView2");
        assert!(!script.contains("\"80:1\""), "Ctrl+P is left to WebView2");
        assert!(!script.contains("\"123:0\""), "F12 is left to WebView2");
        assert!(script.contains("\"85:1\""), "Ctrl+U is overridable");
    }

    #[test]
    fn the_script_reports_only_through_its_binding() {
        let script = PageScript::new("q7x9secret");
        assert_eq!(script.world, "vsesvit-q7x9secret");
        assert!(script.source.contains(&format!("globalThis.{BINDING};")));
        assert!(script.source.contains(&format!("globalThis.{KEYS}")));
        assert!(!script.source.contains("webview"));
        assert!(
            !script.source.contains("q7x9secret"),
            "the world's name stays out of the page"
        );
    }

    #[test]
    fn the_script_handles_keys_in_every_frame() {
        let source = PageScript::new("x").source;
        let top_only = source.find("window !== window.top").unwrap();
        assert!(
            top_only > source.rfind(r#"addEventListener("keydown""#).unwrap(),
            "a focused frame gets the key presses, not the top document"
        );
        assert!(top_only < source.find(r#"addEventListener("click""#).unwrap());
        assert!(top_only < source.find("zoom()").unwrap());
        assert!(top_only < source.find("vsesvit-store").unwrap());
    }

    const ALT_SHIFT: Mods = Mods(6);
    const PROBE: &str = "abcdefghijklmnopabcdefghijklmnop";
    const OTHER: &str = "ponmlkjihgfedcbaponmlkjihgfedcba";

    fn id(extension: &str) -> ExtensionId {
        ExtensionId::parse(extension).unwrap()
    }

    fn command(extension: &str, name: &str, key: &str) -> ExtensionCommand {
        ExtensionCommand {
            extension: id(extension),
            extension_name: "Probe".into(),
            command: ManifestCommand {
                name: name.into(),
                description: String::new(),
                suggested_key: Some(chord(key)),
            },
        }
    }

    fn action(extension: &str) -> ExtensionAction {
        ExtensionAction {
            id: extension.into(),
            extension_id: format!("engine-{extension}"),
            title: "Probe".into(),
            popup: Some("popup.html".into()),
            icon: None,
        }
    }

    /// The probe's commands (`tests/fixtures/extensions/probe/manifest.json`).
    fn probe() -> Vec<ExtensionCommand> {
        vec![
            command(PROBE, "_execute_action", "Alt+Shift+P"),
            command(PROBE, "probe-command", "Alt+Shift+K"),
        ]
    }

    fn with_probe(keymap: Keymap) -> Bindings {
        Bindings::new(keymap, probe(), &[action(PROBE)])
    }

    #[test]
    fn an_action_command_opens_its_popup_before_the_page() {
        let bindings = with_probe(Keymap::default());
        assert_eq!(
            lookup(&bindings, 0x50, ALT_SHIFT).map(|b| (b.command, b.in_page)),
            Some((Command::ExtensionAction(0), Reserved))
        );
        assert_eq!(
            bindings.extension_action(0),
            Some(format!("engine-{PROBE}").as_str())
        );
        assert_eq!(bindings.extension_action(1), None);
        let script = bindings.keys_script();
        let (reserved, _) = script.split_once("overridable").unwrap();
        assert!(reserved.contains("\"80:6\""), "{script}");
        let key = r#"{"t":"key","vk":80,"m":6}"#;
        for in_frame in [false, true] {
            assert_eq!(
                parse_binding_call(&called(BINDING, key), &bindings, in_frame),
                Some(PageMessage::Key(Command::ExtensionAction(0)))
            );
        }
        assert_eq!(
            bindings.label(Command::ExtensionAction(0)).as_deref(),
            Some("Alt+Shift+P")
        );
        assert_eq!(
            bindings.tip("Probe", Command::ExtensionAction(0)),
            "Probe (Alt+Shift+P)"
        );
        assert_eq!(bindings.label(Command::ExtensionAction(7)), None);
        assert_eq!(bindings.tip("Probe", Command::ExtensionAction(7)), "Probe");
        for (i, a) in bindings.list().iter().enumerate() {
            for b in &bindings.list()[i + 1..] {
                assert!(!(a.vk == b.vk && a.mods == b.mods), "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn named_commands_are_not_bound() {
        let bindings = with_probe(Keymap::default());
        assert!(lookup(&bindings, 0x4B, ALT_SHIFT).is_none());
        assert!(!bindings.keys_script().contains("\"75:6\""));
        assert_eq!(
            bindings.extensions().chord(&id(PROBE), "probe-command"),
            Some(chord("Alt+Shift+K")),
            "Settings still shows the named command's shortcut"
        );
        let key = r#"{"t":"key","vk":75,"m":6}"#;
        assert_eq!(
            parse_binding_call(&called(BINDING, key), &bindings, false),
            None
        );
    }

    #[test]
    fn an_action_command_is_bound_once_the_engine_has_its_extension() {
        let unloaded = Bindings::new(Keymap::default(), probe(), &[action(OTHER)]);
        assert!(lookup(&unloaded, 0x50, ALT_SHIFT).is_none());
        assert_eq!(unloaded.list(), defaults().list());
        let mv2 = Bindings::new(
            Keymap::default(),
            vec![command(OTHER, "_execute_browser_action", "Alt+Shift+B")],
            &[action(OTHER)],
        );
        assert_eq!(
            lookup(&mv2, 0x42, ALT_SHIFT).map(|b| b.command),
            Some(Command::ExtensionAction(0))
        );
        assert_eq!(
            mv2.extension_action(0),
            Some(format!("engine-{OTHER}").as_str())
        );
    }

    #[test]
    fn the_browsers_chords_are_never_an_extensions() {
        let taken = Bindings::new(
            Keymap::default(),
            vec![command(PROBE, "_execute_action", "Ctrl+T")],
            &[action(PROBE)],
        );
        assert_eq!(
            lookup(&taken, 0x54, Mods::CTRL).map(|b| b.command),
            Some(Command::NewTab)
        );
        assert_eq!(taken.list(), defaults().list());
        let mut keymap = Keymap::default();
        keymap.assign(Core::ShowHistory, [chord("Alt+Shift+P")]);
        let moved = with_probe(keymap);
        assert_eq!(
            lookup(&moved, 0x50, ALT_SHIFT).map(|b| b.command),
            Some(Command::ShowHistory)
        );
        assert_eq!(moved.extension_action(0), None);
    }

    #[test]
    fn an_action_command_follows_the_shortcut_the_user_gives_it() {
        let mut keymap = Keymap::default();
        let shortcuts = keymap.extension_shortcuts(probe());
        keymap.assign_extension(
            &shortcuts,
            &id(PROBE),
            "_execute_action",
            Some(chord("Ctrl+Alt+Y")),
        );
        let bindings = with_probe(keymap);
        assert!(lookup(&bindings, 0x50, ALT_SHIFT).is_none());
        assert_eq!(
            lookup(&bindings, 0x59, Mods(5)).map(|b| (b.command, b.in_page)),
            Some((Command::ExtensionAction(0), Reserved))
        );
        let probe = id(PROBE);
        assert!(!bindings.extensions().is_default(&probe, "_execute_action"));
    }
}
