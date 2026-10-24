//! Keyboard shortcuts: the commands that have them, their default chords, and the user's
//! reassignments, resolved into one keymap.
//!
//! Everything here is platform-neutral. Each shell maps a [`Command`] to its own action and a
//! [`Chord`] to its native key representation. The user's reassignments are stored in
//! [`keys::SHORTCUTS`] as overrides only, so a build that changes a default reaches everyone who
//! never touched that command.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Error;
use crate::prefs::{Prefs, keys};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { ctrl: false, alt: false, shift: false };
    pub const CTRL: Mods = Mods { ctrl: true, ..Mods::NONE };
    pub const ALT: Mods = Mods { alt: true, ..Mods::NONE };
    pub const SHIFT: Mods = Mods { shift: true, ..Mods::NONE };

    pub const fn and(self, other: Mods) -> Mods {
        Mods { ctrl: self.ctrl || other.ctrl, alt: self.alt || other.alt, shift: self.shift || other.shift }
    }
}

macro_rules! keys {
    ($($key:ident = $name:literal,)*) => {
        /// A key as the shortcut model names it. Escape, Enter and Backspace are missing on
        /// purpose: the shortcut capture UI uses them to cancel, confirm and clear.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Key {
            $($key,)*
        }

        impl Key {
            pub const ALL: &'static [Key] = &[$(Key::$key,)*];

            /// The name in a chord string. Symbol keys are named by the character they type.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Key::$key => $name,)*
                }
            }
        }
    };
}

keys! {
    A = "A", B = "B", C = "C", D = "D", E = "E", F = "F", G = "G", H = "H", I = "I", J = "J",
    K = "K", L = "L", M = "M", N = "N", O = "O", P = "P", Q = "Q", R = "R", S = "S", T = "T",
    U = "U", V = "V", W = "W", X = "X", Y = "Y", Z = "Z",
    Digit0 = "0", Digit1 = "1", Digit2 = "2", Digit3 = "3", Digit4 = "4",
    Digit5 = "5", Digit6 = "6", Digit7 = "7", Digit8 = "8", Digit9 = "9",
    F1 = "F1", F2 = "F2", F3 = "F3", F4 = "F4", F5 = "F5", F6 = "F6",
    F7 = "F7", F8 = "F8", F9 = "F9", F10 = "F10", F11 = "F11", F12 = "F12",
    F13 = "F13", F14 = "F14", F15 = "F15", F16 = "F16", F17 = "F17", F18 = "F18",
    F19 = "F19", F20 = "F20", F21 = "F21", F22 = "F22", F23 = "F23", F24 = "F24",
    Tab = "Tab", Space = "Space", Insert = "Insert", Delete = "Delete", Home = "Home", End = "End",
    PageUp = "PageUp", PageDown = "PageDown", Left = "Left", Right = "Right", Up = "Up", Down = "Down",
    Plus = "Plus", Minus = "Minus", Equal = "Equal", Comma = "Comma", Period = "Period",
    Slash = "Slash", Question = "Question", Semicolon = "Semicolon",
    KeypadPlus = "KeypadPlus", KeypadMinus = "KeypadMinus",
    Keypad0 = "Keypad0", Keypad1 = "Keypad1", Keypad2 = "Keypad2", Keypad3 = "Keypad3", Keypad4 = "Keypad4",
    Keypad5 = "Keypad5", Keypad6 = "Keypad6", Keypad7 = "Keypad7", Keypad8 = "Keypad8", Keypad9 = "Keypad9",
}

impl Key {
    pub const fn is_function(self) -> bool {
        (self as u8) >= (Key::F1 as u8) && (self as u8) <= (Key::F24 as u8)
    }

    pub fn from_name(name: &str) -> Option<Key> {
        Key::ALL.iter().copied().find(|key| key.name().eq_ignore_ascii_case(name))
    }
}

/// A key with modifiers. Only [`Chord::new`] makes one, so every chord is bindable.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    mods: Mods,
    key: Key,
}

impl Chord {
    /// `None` unless Ctrl or Alt is held or the key is an F-key. Without them, a letter,
    /// digit or symbol types text (Shift only changes its case), and Tab, Space, arrows and
    /// the editing keys move the focus, the caret or a selection.
    pub const fn new(mods: Mods, key: Key) -> Option<Chord> {
        if mods.ctrl || mods.alt || key.is_function() { Some(Chord { mods, key }) } else { None }
    }

    pub fn mods(self) -> Mods {
        self.mods
    }

    pub fn key(self) -> Key {
        self.key
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (held, name) in [(self.mods.ctrl, "Ctrl"), (self.mods.alt, "Alt"), (self.mods.shift, "Shift")] {
            if held {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(self.key.name())
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a keyboard shortcut")]
pub struct ParseChordError(String);

impl FromStr for Chord {
    type Err = ParseChordError;

    /// Modifiers in any order, each at most once, then one key. Displays in canonical order.
    fn from_str(s: &str) -> Result<Chord, ParseChordError> {
        let fail = || ParseChordError(s.to_owned());
        let mut names: Vec<&str> = s.split('+').collect();
        let key = names.pop().and_then(Key::from_name).ok_or_else(fail)?;
        let mut held = Mods::NONE;
        for name in names {
            let (flag, _) = [(&mut held.ctrl, "Ctrl"), (&mut held.alt, "Alt"), (&mut held.shift, "Shift")]
                .into_iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(name))
                .ok_or_else(fail)?;
            if std::mem::replace(flag, true) {
                return Err(fail());
            }
        }
        Chord::new(held, key).ok_or_else(fail)
    }
}

impl Serialize for Chord {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Chord {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Chord, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Section {
    TabsAndWindows,
    Navigation,
    Page,
    General,
}

impl Section {
    pub const ALL: [Section; 4] = [Self::TabsAndWindows, Self::Navigation, Self::Page, Self::General];

    pub fn title(self) -> &'static str {
        match self {
            Self::TabsAndWindows => "Tabs and Windows",
            Self::Navigation => "Navigation",
            Self::Page => "Page",
            Self::General => "General",
        }
    }
}

struct Entry {
    id: &'static str,
    title: &'static str,
    section: Section,
    defaults: &'static [Chord],
}

macro_rules! commands {
    ($($section:ident { $($cmd:ident = $id:literal, $title:literal, [$($chord:expr),+ $(,)?];)* })*) => {
        /// Every command either shell binds a shortcut to, in the order the shortcuts list
        /// shows them.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Command {
            $($($cmd,)*)*
        }

        impl Command {
            pub const ALL: &'static [Command] = &[$($(Command::$cmd,)*)*];
        }

        const ENTRIES: &[Entry] = {
            use Key::*;
            &[$($(
                Entry { id: $id, title: $title, section: Section::$section, defaults: &[$($chord),+] },
            )*)*]
        };
    };
}

const fn chord(mods: Mods, key: Key) -> Chord {
    match Chord::new(mods, key) {
        Some(chord) => chord,
        None => panic!("a default shortcut must be bindable"),
    }
}

const fn ctrl(key: Key) -> Chord {
    chord(Mods::CTRL, key)
}

const fn ctrl_shift(key: Key) -> Chord {
    chord(Mods::CTRL.and(Mods::SHIFT), key)
}

const fn alt(key: Key) -> Chord {
    chord(Mods::ALT, key)
}

const fn shift(key: Key) -> Chord {
    chord(Mods::SHIFT, key)
}

const fn bare(key: Key) -> Chord {
    chord(Mods::NONE, key)
}

// Defaults follow Chrome, except where a comment on the row says otherwise.
commands! {
    TabsAndWindows {
        NewTab = "new-tab", "New tab", [ctrl(T)];
        CloseTab = "close-tab", "Close tab", [ctrl(W), ctrl(F4)];
        ReopenClosedTab = "reopen-closed-tab", "Reopen closed tab", [ctrl_shift(T)];
        SearchTabs = "search-tabs", "Search tabs", [ctrl_shift(A)];
        NextTab = "next-tab", "Next tab", [ctrl(Tab)];
        PreviousTab = "previous-tab", "Previous tab", [ctrl_shift(Tab)];
        SelectTab1 = "select-tab-1", "Go to tab 1", [ctrl(Digit1)];
        SelectTab2 = "select-tab-2", "Go to tab 2", [ctrl(Digit2)];
        SelectTab3 = "select-tab-3", "Go to tab 3", [ctrl(Digit3)];
        SelectTab4 = "select-tab-4", "Go to tab 4", [ctrl(Digit4)];
        SelectTab5 = "select-tab-5", "Go to tab 5", [ctrl(Digit5)];
        SelectTab6 = "select-tab-6", "Go to tab 6", [ctrl(Digit6)];
        SelectTab7 = "select-tab-7", "Go to tab 7", [ctrl(Digit7)];
        SelectTab8 = "select-tab-8", "Go to tab 8", [ctrl(Digit8)];
        SelectLastTab = "select-last-tab", "Go to the last tab", [ctrl(Digit9)];
        // Chrome saves the page with Ctrl+S; Vsesvit gives it to the tab list and saves with Ctrl+Shift+S.
        ToggleTabList = "toggle-tab-list", "Show or hide the tab list", [ctrl(S), bare(F9)];
        NewWindow = "new-window", "New window", [ctrl(N)];
        NewPrivateWindow = "new-private-window", "New private window", [ctrl_shift(N)];
        Quit = "quit", "Quit", [ctrl(Q)];
    }
    Navigation {
        FocusAddress = "focus-address", "Focus the address bar", [ctrl(L), alt(D), bare(F6)];
        Back = "back", "Back", [alt(Left)];
        Forward = "forward", "Forward", [alt(Right)];
        Reload = "reload", "Reload", [ctrl(R), bare(F5)];
        ReloadBypassCache = "reload-bypass-cache", "Reload, ignoring the cache", [ctrl_shift(R), shift(F5)];
    }
    Page {
        SavePage = "save-page", "Save page as…", [ctrl_shift(S)];
        Print = "print", "Print…", [ctrl(P)];
        Find = "find", "Find", [ctrl(F)];
        FindNext = "find-next", "Next match", [ctrl(G)];
        FindPrevious = "find-previous", "Previous match", [ctrl_shift(G)];
        BookmarkPage = "bookmark-page", "Bookmark this page", [ctrl(D)];
        // Chrome opens the element inspector with Ctrl+Shift+C.
        CopyCleanLink = "copy-clean-link", "Copy link without tracking", [ctrl_shift(C)];
        CopyLink = "copy-link", "Copy link", [chord(Mods::CTRL.and(Mods::ALT).and(Mods::SHIFT), C)];
        ViewSource = "view-source", "View page source", [ctrl(U)];
        ZoomIn = "zoom-in", "Zoom in", [ctrl(Plus), ctrl(Equal), ctrl(KeypadPlus)];
        ZoomOut = "zoom-out", "Zoom out", [ctrl(Minus), ctrl(KeypadMinus)];
        ZoomReset = "zoom-reset", "Reset zoom", [ctrl(Digit0), ctrl(Keypad0)];
        Fullscreen = "fullscreen", "Fullscreen", [bare(F11)];
    }
    General {
        ToggleBookmarksBar = "toggle-bookmarks-bar", "Show or hide the bookmarks bar", [ctrl_shift(B)];
        ShowBookmarks = "show-bookmarks", "Bookmarks", [ctrl_shift(O)];
        ShowHistory = "show-history", "History", [ctrl(H)];
        ShowDownloads = "show-downloads", "Downloads", [ctrl(J)];
        ShowSettings = "show-settings", "Settings", [ctrl(Comma)];
        ShowShortcuts = "show-shortcuts", "Keyboard shortcuts", [ctrl(Question)];
        DeveloperTools = "developer-tools", "Developer tools", [ctrl_shift(I), bare(F12)];
        JavaScriptConsole = "javascript-console", "JavaScript console", [ctrl_shift(J)];
    }
}

impl Command {
    fn entry(self) -> &'static Entry {
        &ENTRIES[self as usize]
    }

    /// The key its override is stored under. Synced, so it never changes.
    pub fn id(self) -> &'static str {
        self.entry().id
    }

    pub fn title(self) -> &'static str {
        self.entry().title
    }

    pub fn section(self) -> Section {
        self.entry().section
    }

    pub fn defaults(self) -> &'static [Chord] {
        self.entry().defaults
    }

    pub fn from_id(id: &str) -> Option<Command> {
        Command::ALL.iter().copied().find(|cmd| cmd.id() == id)
    }

    /// Where a key press for the command is taken while the page has the keyboard focus, on its
    /// default chords. Reserved are the keys Chrome keeps from pages; the rest reach the page
    /// first, unless the engine knows the command's keys itself.
    pub const fn in_page(self) -> InPage {
        use Command::*;
        use InPage::{Native, Overridable, Reserved};
        match self {
            NewTab | CloseTab | ReopenClosedTab | NextTab | PreviousTab | NewWindow | NewPrivateWindow | Quit => Reserved,
            // Not among the keys Chrome keeps from pages.
            SearchTabs | SelectTab1 | SelectTab2 | SelectTab3 | SelectTab4 | SelectTab5 | SelectTab6 | SelectTab7 | SelectTab8
            | SelectLastTab | ToggleTabList | FocusAddress => Overridable,
            Back | Forward | Reload | ReloadBypassCache => Native,
            // Pages such as editors keep a Ctrl+Shift+S of their own, as Chrome's Ctrl+S.
            SavePage => Overridable,
            Print | Find | FindNext | FindPrevious | ZoomIn | ZoomOut | ZoomReset => Native,
            // Pages keep their Ctrl+U, as in Chrome.
            BookmarkPage | ViewSource | Fullscreen => Overridable,
            CopyCleanLink | CopyLink => Reserved,
            ToggleBookmarksBar | ShowBookmarks | ShowHistory | ShowDownloads | ShowSettings | ShowShortcuts => Overridable,
            DeveloperTools | JavaScriptConsole => Native,
        }
    }

    /// Where a key press of `chord` for the command is taken while the page has the keyboard
    /// focus: a [`InPage::Native`] command is left to the engine only on the chords the engine
    /// knows it by, its defaults; on any other chord the shell takes it after the page.
    pub fn in_page_on(self, chord: Chord) -> InPage {
        match self.in_page() {
            InPage::Native if !self.defaults().contains(&chord) => InPage::Overridable,
            kind => kind,
        }
    }
}

/// Where a key press for a command is taken while the page has the keyboard focus.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum InPage {
    /// Left to the engine, which handles the command's default chords itself.
    Native,
    /// Taken before the page sees it.
    Reserved,
    /// Reaches the page first, and is taken only if the page does not `preventDefault()` it, as
    /// Chromium treats browser shortcuts that are not reserved.
    Overridable,
}

impl Key {
    /// The Windows virtual-key code of the key, and whether typing it takes Shift. Windows and
    /// CEF on every OS report key presses by these codes, as `KeyboardEvent.keyCode` does.
    ///
    /// Symbol keys assume the US layout, where the names match the keys: Plus is Shift with the
    /// `=+` key, Question is Shift with the `/?` key.
    pub fn vk(self) -> (u16, bool) {
        const OEM_1: u16 = 0xBA;
        const OEM_PLUS: u16 = 0xBB;
        const OEM_COMMA: u16 = 0xBC;
        const OEM_MINUS: u16 = 0xBD;
        const OEM_PERIOD: u16 = 0xBE;
        const OEM_2: u16 = 0xBF;
        let runs = [(Key::A, Key::Z, 0x41), (Key::Digit0, Key::Digit9, 0x30), (Key::F1, Key::F24, 0x70), (Key::Keypad0, Key::Keypad9, 0x60)];
        let at = self as u16;
        if let Some((first, _, base)) = runs.iter().find(|(first, last, _)| (*first as u16..=*last as u16).contains(&at)) {
            return (base + at - *first as u16, false);
        }
        let vk = match self {
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
            Key::Semicolon => OEM_1,
            Key::Equal => OEM_PLUS,
            Key::Plus => return (OEM_PLUS, true),
            Key::Comma => OEM_COMMA,
            Key::Minus => OEM_MINUS,
            Key::Period => OEM_PERIOD,
            Key::Slash => OEM_2,
            Key::Question => return (OEM_2, true),
            _ => unreachable!("{self:?} is in a run of keys above"),
        };
        (vk, false)
    }

    /// The key a press of virtual-key code `vk` is, with the modifiers held with it. Shift with
    /// the `=+` or `/?` key is Plus or Question, as those keys type with Shift.
    pub fn of_vk(vk: u16, mods: Mods) -> Option<(Key, Mods)> {
        let find = |shift: bool| Key::ALL.iter().copied().find(|&k| k.vk() == (vk, shift));
        match find(true).filter(|_| mods.shift) {
            Some(key) => Some((key, Mods { shift: false, ..mods })),
            None => find(false).map(|key| (key, mods)),
        }
    }
}

impl Chord {
    /// The virtual-key code and modifiers that type the chord.
    pub fn vk(self) -> (u16, Mods) {
        let (vk, shift) = self.key.vk();
        (vk, Mods { shift: self.mods.shift || shift, ..self.mods })
    }
}

/// What a key press does while the shortcut capture listens.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Capture {
    Cancel,
    /// Leaves the command without a shortcut.
    Remove,
    Save,
    /// A modifier on its own: the chord is still coming.
    Modifier,
    Chord(Chord),
    /// A key that types or moves the caret without Ctrl or Alt ([`NEEDS_MODIFIER_NOTE`]).
    NeedsModifier,
    /// A key with no name here, such as Print Screen ([`NOT_A_KEY_NOTE`]).
    NotAKey,
}

/// What the key press of virtual-key code `vk` with `mods` held does while the shortcut capture
/// listens: Escape cancels, Backspace removes the shortcut, Enter saves it.
pub fn capture(vk: u16, mods: Mods) -> Capture {
    match vk {
        0x1B => Capture::Cancel,
        0x08 => Capture::Remove,
        0x0D => Capture::Save,
        0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0x90 | 0x91 | 0xA0..=0xA5 => Capture::Modifier,
        _ => match Key::of_vk(vk, mods) {
            Some((key, mods)) => Chord::new(mods, key).map_or(Capture::NeedsModifier, Capture::Chord),
            None => Capture::NotAKey,
        },
    }
}

/// What [`keys::SHORTCUTS`] stores: command id to chord strings, for reassigned commands only.
/// Strings, not [`Command`]s and [`Chord`]s, so a build keeps what a newer one wrote: ids it does
/// not know and keys it cannot name. An empty list means the user removed the shortcut.
pub type Overrides = BTreeMap<String, Vec<String>>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Override {
    chords: Vec<Chord>,
    /// Chord strings this build cannot parse (a key a newer build added), written back until
    /// the user reassigns or resets the command here.
    unparsed: Vec<String>,
}

/// The shortcut-capture dialog's note for a key pressed without Ctrl or Alt.
pub const NEEDS_MODIFIER_NOTE: &str = "A shortcut needs Ctrl or Alt, unless it is a function key";
/// The shortcut-capture dialog's note for a key no shortcut can use.
pub const NOT_A_KEY_NOTE: &str = "This key cannot be part of a shortcut";

/// A command that lost chords to a [`Keymap::assign`] or [`Keymap::reset`], so the UI can say
/// "Ctrl+T was taken from New tab". The chords are in the order the loser had them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Taken {
    pub from: Command,
    pub chords: Vec<Chord>,
}

/// The effective shortcuts: defaults, with the user's overrides applied.
///
/// No chord ever belongs to two commands. Overrides claim their chords first, in
/// [`Command::ALL`] order, so a chord two overrides name (only corrupt or hand-edited data has
/// one) goes to the earlier command. Every other command gets its defaults minus what the
/// overrides claimed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    overrides: BTreeMap<Command, Override>,
    /// Overrides of commands this build does not know, and extensions' commands, which
    /// [`crate::extensions::commands`] resolves.
    pub(crate) unknown: Overrides,
    chords: Vec<Vec<Chord>>,
    owners: BTreeMap<Chord, Command>,
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap::build(BTreeMap::new(), Overrides::new())
    }
}

impl Keymap {
    pub fn from_overrides(stored: &Overrides) -> Keymap {
        let mut overrides = BTreeMap::new();
        let mut unknown = Overrides::new();
        for (id, strings) in stored {
            let Some(cmd) = Command::from_id(id) else {
                unknown.insert(id.clone(), strings.clone());
                continue;
            };
            let (mut chords, mut unparsed) = (Vec::new(), Vec::new());
            for s in strings {
                match s.parse() {
                    Ok(chord) => chords.push(chord),
                    Err(_) => unparsed.push(s.clone()),
                }
            }
            overrides.insert(cmd, Override { chords, unparsed });
        }
        Keymap::build(overrides, unknown)
    }

    /// What to store in [`keys::SHORTCUTS`].
    pub fn overrides(&self) -> Overrides {
        let mut stored = self.unknown.clone();
        for (cmd, o) in &self.overrides {
            let strings = o.chords.iter().map(Chord::to_string).chain(o.unparsed.iter().cloned()).collect();
            stored.insert(cmd.id().to_owned(), strings);
        }
        stored
    }

    pub fn chords(&self, cmd: Command) -> &[Chord] {
        &self.chords[cmd as usize]
    }

    pub fn command_for(&self, chord: Chord) -> Option<Command> {
        self.owners.get(&chord).copied()
    }

    pub fn is_default(&self, cmd: Command) -> bool {
        self.chords(cmd) == cmd.defaults()
    }

    /// What the shortcut-capture dialog makes of `chord` pressed for `cmd`: the chord saving
    /// would assign, if any, and a note about the command that holds it. A chord held by a
    /// command `fixed` says cannot be changed is not offered.
    pub fn offer(&self, cmd: Command, chord: Chord, fixed: impl Fn(Command) -> bool) -> (Option<Chord>, Option<String>) {
        match self.command_for(chord).filter(|&holder| holder != cmd) {
            Some(holder) if fixed(holder) => (None, Some(format!("Used by {}, which cannot be changed", holder.title()))),
            Some(holder) => (Some(chord), Some(format!("Also used by {}. Saving moves it here.", holder.title()))),
            None => (Some(chord), None),
        }
    }

    /// Every command in [`Command::ALL`] order, with its chords, which may be none.
    pub fn iter(&self) -> impl Iterator<Item = (Command, &[Chord])> {
        Command::ALL.iter().map(|&cmd| (cmd, self.chords(cmd)))
    }

    /// Gives `cmd` exactly `chords`, taking any of them from the commands that hold them.
    pub fn assign(&mut self, cmd: Command, chords: impl IntoIterator<Item = Chord>) -> Vec<Taken> {
        let mut wanted = Vec::new();
        for chord in chords {
            if !wanted.contains(&chord) {
                wanted.push(chord);
            }
        }
        let taken = self.take(cmd, &wanted);
        self.overrides.insert(cmd, Override { chords: wanted, unparsed: Vec::new() });
        self.rebuild();
        taken
    }

    /// Gives `cmd` its defaults back, taking them from the commands that hold them.
    pub fn reset(&mut self, cmd: Command) -> Vec<Taken> {
        let taken = self.take(cmd, cmd.defaults());
        self.overrides.remove(&cmd);
        self.rebuild();
        taken
    }

    /// Every command back to its defaults, including commands only a newer build knows.
    pub fn reset_all(&mut self) {
        *self = Keymap::default();
    }

    /// Removes `wanted` from every other command. A command that held one of them keeps the
    /// rest of its chords as an override, so it does not get the chord back from its defaults.
    fn take(&mut self, cmd: Command, wanted: &[Chord]) -> Vec<Taken> {
        let losers: BTreeSet<Command> =
            wanted.iter().filter_map(|chord| self.command_for(*chord)).filter(|&from| from != cmd).collect();
        let taken: Vec<Taken> = losers
            .into_iter()
            .map(|from| Taken { from, chords: self.chords(from).iter().copied().filter(|c| wanted.contains(c)).collect() })
            .collect();
        for Taken { from, .. } in &taken {
            if !self.overrides.contains_key(from) {
                self.overrides.insert(*from, Override { chords: self.chords(*from).to_vec(), unparsed: Vec::new() });
            }
        }
        for (other, o) in &mut self.overrides {
            if *other != cmd {
                o.chords.retain(|chord| !wanted.contains(chord));
            }
        }
        taken
    }

    fn rebuild(&mut self) {
        *self = Keymap::build(std::mem::take(&mut self.overrides), std::mem::take(&mut self.unknown));
    }

    /// Resolves the overrides, then drops each one that only restates its command's defaults
    /// where dropping it leaves every command's chords as they are.
    fn build(mut overrides: BTreeMap<Command, Override>, unknown: Overrides) -> Keymap {
        let (chords, owners) = resolve(&overrides);
        let restated: Vec<Command> = overrides
            .iter()
            .filter(|(cmd, o)| o.unparsed.is_empty() && o.chords == cmd.defaults())
            .map(|(cmd, _)| *cmd)
            .collect();
        for cmd in restated {
            let o = overrides.remove(&cmd).expect("restated overrides are present");
            if resolve(&overrides).0 != chords {
                overrides.insert(cmd, o);
            }
        }
        Keymap { overrides, unknown, chords, owners }
    }
}

fn resolve(overrides: &BTreeMap<Command, Override>) -> (Vec<Vec<Chord>>, BTreeMap<Chord, Command>) {
    let mut chords = vec![Vec::new(); Command::ALL.len()];
    let mut owners = BTreeMap::new();
    let overridden = overrides.iter().map(|(&cmd, o)| (cmd, o.chords.as_slice()));
    let defaulted = Command::ALL.iter().filter(|cmd| !overrides.contains_key(*cmd)).map(|&cmd| (cmd, cmd.defaults()));
    for (cmd, wanted) in overridden.chain(defaulted) {
        for &chord in wanted {
            if let std::collections::btree_map::Entry::Vacant(slot) = owners.entry(chord) {
                slot.insert(cmd);
                chords[cmd as usize].push(chord);
            }
        }
    }
    (chords, owners)
}

impl Prefs<'_> {
    pub fn keymap(&mut self) -> Keymap {
        Keymap::from_overrides(&self.get(&keys::SHORTCUTS))
    }

    /// Stores `keymap`'s overrides; none at all is stored as a reset.
    pub fn set_keymap(&mut self, keymap: &Keymap) -> Result<(), Error> {
        let overrides = keymap.overrides();
        if overrides.is_empty() { self.reset(&keys::SHORTCUTS) } else { self.set(&keys::SHORTCUTS, &overrides) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL_SHIFT: Mods = Mods::CTRL.and(Mods::SHIFT);

    fn chord(s: &str) -> Chord {
        s.parse().unwrap()
    }

    fn chord_of(vk: u16, mods: Mods) -> Option<Chord> {
        match capture(vk, mods) {
            Capture::Chord(chord) => Some(chord),
            _ => None,
        }
    }

    #[test]
    fn chromes_reserved_keys_are_taken_before_the_page_and_native_ones_only_on_their_defaults() {
        let reserved: Vec<Command> = Command::ALL.iter().copied().filter(|c| c.in_page() == InPage::Reserved).collect();
        use Command::*;
        assert_eq!(reserved, [NewTab, CloseTab, ReopenClosedTab, NextTab, PreviousTab, NewWindow, NewPrivateWindow, Quit, CopyCleanLink, CopyLink]);
        assert_eq!(SelectTab1.in_page(), InPage::Overridable, "Chrome leaves Ctrl+1 to pages");
        assert_eq!(Reload.in_page_on(chord("Ctrl+R")), InPage::Native);
        assert_eq!(Reload.in_page_on(chord("Ctrl+Shift+Y")), InPage::Overridable, "the engine knows Reload by its defaults only");
        for command in Command::ALL.iter().copied().filter(|c| c.in_page() != InPage::Native) {
            assert_eq!(command.in_page_on(chord("Ctrl+Shift+Y")), command.in_page(), "{command:?}");
            for &default in command.defaults() {
                assert_eq!(command.in_page_on(default), command.in_page(), "{command:?}");
            }
        }
    }

    #[test]
    fn every_key_has_its_own_key_press() {
        for (i, &a) in Key::ALL.iter().enumerate() {
            for &b in &Key::ALL[i + 1..] {
                assert_ne!(a.vk(), b.vk(), "{a:?} and {b:?}");
            }
        }
        assert_eq!(Key::A.vk(), (0x41, false));
        assert_eq!(Key::Z.vk(), (0x5A, false));
        assert_eq!(Key::Digit9.vk(), (0x39, false));
        assert_eq!(Key::F1.vk(), (0x70, false));
        assert_eq!(Key::F9.vk(), (0x78, false));
        assert_eq!(Key::F24.vk(), (0x87, false));
        assert_eq!(Key::Keypad0.vk(), (0x60, false));
        assert_eq!(Key::Keypad9.vk(), (0x69, false));
        assert_eq!(Key::KeypadPlus.vk(), (0x6B, false));
        assert_eq!(Key::Plus.vk(), (0xBB, true));
        assert_eq!(Key::Equal.vk(), (0xBB, false));
        assert_eq!(Key::Question.vk(), (0xBF, true));
        assert_eq!(Key::Slash.vk(), (0xBF, false));
        assert_eq!(Key::Tab.vk(), (0x09, false));
        assert_eq!(Key::Left.vk(), (0x25, false));
    }

    #[test]
    fn every_chord_survives_the_round_trip_through_its_key_press() {
        for &key in Key::ALL {
            for bits in 0..8u8 {
                let mods = Mods { ctrl: bits & 1 != 0, shift: bits & 2 != 0, alt: bits & 4 != 0 };
                let Some(chord) = Chord::new(mods, key) else {
                    continue;
                };
                let (vk, pressed) = chord.vk();
                let back = chord_of(vk, pressed).unwrap();
                let typed_with_shift = match key {
                    Key::Equal | Key::Plus => Some(Key::Plus),
                    Key::Slash | Key::Question => Some(Key::Question),
                    _ => None,
                };
                let expected = match typed_with_shift {
                    Some(symbol) if mods.shift => Chord::new(Mods { shift: false, ..mods }, symbol).unwrap(),
                    _ => chord,
                };
                assert_eq!(back, expected, "{chord}");
            }
        }
    }

    #[test]
    fn key_presses_read_as_chords() {
        assert_eq!(chord_of(0xBB, CTRL_SHIFT), Some(chord("Ctrl+Plus")));
        assert_eq!(chord_of(0xBB, Mods::CTRL), Some(chord("Ctrl+Equal")));
        assert_eq!(chord_of(0xBF, CTRL_SHIFT), Some(chord("Ctrl+Question")));
        assert_eq!(chord_of(0x59, CTRL_SHIFT), Some(chord("Ctrl+Shift+Y")));
        assert_eq!(chord_of(0x78, Mods::NONE), Some(chord("F9")));
        assert_eq!(chord_of(0x41, Mods::SHIFT), None, "Shift+A types text");
        assert_eq!(chord_of(0x11, Mods::CTRL), None, "Ctrl alone is no chord");
    }

    #[test]
    fn capture_reads_key_presses() {
        assert_eq!(capture(0x1B, Mods::CTRL), Capture::Cancel);
        assert_eq!(capture(0x08, Mods::NONE), Capture::Remove);
        assert_eq!(capture(0x0D, Mods::NONE), Capture::Save);
        assert_eq!(capture(0x11, Mods::CTRL), Capture::Modifier);
        assert_eq!(capture(0xA0, Mods::SHIFT), Capture::Modifier);
        assert_eq!(capture(0x59, CTRL_SHIFT), Capture::Chord(chord("Ctrl+Shift+Y")));
        assert_eq!(capture(0x75, Mods::NONE), Capture::Chord(chord("F6")));
        assert_eq!(capture(0x59, Mods::SHIFT), Capture::NeedsModifier);
        assert_eq!(capture(0x25, Mods::NONE), Capture::NeedsModifier);
        assert_eq!(capture(0x2C, Mods::CTRL), Capture::NotAKey, "Print Screen");
    }

    #[test]
    fn only_f_keys_are_bindable_without_ctrl_or_alt() {
        let function: Vec<Key> = Key::ALL.iter().copied().filter(|k| k.is_function()).collect();
        assert_eq!(function.len(), 24);
        assert!(function.iter().all(|k| k.name().starts_with('F') && k.name().len() > 1));
        for &key in Key::ALL {
            assert_eq!(Chord::new(Mods::NONE, key).is_some(), key.is_function(), "{key:?}");
            assert_eq!(Chord::new(Mods::SHIFT, key).is_some(), key.is_function(), "{key:?}");
            assert!(Chord::new(Mods::CTRL, key).is_some());
            assert!(Chord::new(Mods::ALT, key).is_some());
        }
    }

    #[test]
    fn key_names_are_unique_and_plus_free() {
        for (i, a) in Key::ALL.iter().enumerate() {
            assert!(!a.name().contains('+'));
            assert_eq!(Key::from_name(a.name()), Some(*a));
            assert!(Key::ALL[i + 1..].iter().all(|b| a.name() != b.name()));
        }
    }

    #[test]
    fn table_rows_line_up_with_commands() {
        assert_eq!(ENTRIES.len(), Command::ALL.len());
        for (i, cmd) in Command::ALL.iter().enumerate() {
            assert_eq!(*cmd as usize, i);
        }
    }
}
