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
        Find = "find", "Find", [ctrl(F)];
        FindNext = "find-next", "Next match", [ctrl(G)];
        FindPrevious = "find-previous", "Previous match", [ctrl_shift(G)];
        BookmarkPage = "bookmark-page", "Bookmark this page", [ctrl(D)];
        // Chrome opens the element inspector with Ctrl+Shift+C.
        CopyCleanLink = "copy-clean-link", "Copy link without tracking", [ctrl_shift(C)];
        CopyLink = "copy-link", "Copy link", [chord(Mods::CTRL.and(Mods::ALT).and(Mods::SHIFT), C)];
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
    /// Overrides of commands this build does not know.
    unknown: Overrides,
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
