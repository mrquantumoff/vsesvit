//! Keyboard shortcuts: chord strings, the default table, reassignment and its stealing rule,
//! the stored overrides, and a property test that no sequence of edits puts a chord on two
//! commands.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use proptest::prelude::*;

use vsesvit_core::crdt::{DeviceId, Seq, TimeSource};
use vsesvit_core::prefs::{PrefRecord, keys};
use vsesvit_core::shortcuts::{Chord, Command, Keymap, Mods, Overrides, Section, Taken};
use vsesvit_core::sync::Kind;
use vsesvit_core::{OpenOptions, Profile};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-shortcuts-{}", uuid::Uuid::new_v4())));
    let p = Profile::open(
        &dir.0,
        OpenOptions {
            time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000))),
            new_device_id: Some(DeviceId(2)),
            ..OpenOptions::default()
        },
    )
    .unwrap();
    (p, dir)
}

fn c(s: &str) -> Chord {
    s.parse().unwrap_or_else(|e| panic!("{e}"))
}

fn stored(entries: &[(&str, &[&str])]) -> Overrides {
    entries.iter().map(|(id, chords)| (id.to_string(), chords.iter().map(|s| s.to_string()).collect())).collect()
}

fn effective(keymap: &Keymap) -> Vec<(Command, Vec<Chord>)> {
    keymap.iter().map(|(cmd, chords)| (cmd, chords.to_vec())).collect()
}

#[test]
fn every_default_round_trips_as_a_string() {
    for &cmd in Command::ALL {
        for chord in cmd.defaults() {
            let text = chord.to_string();
            assert_eq!(c(&text), *chord, "{text}");
            let json = serde_json::to_string(chord).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(serde_json::from_str::<Chord>(&json).unwrap(), *chord);
        }
    }
    for text in ["Ctrl+Shift+S", "Alt+Left", "F6", "Ctrl+Keypad0", "Shift+F5", "Ctrl+Alt+Shift+C", "Ctrl+Plus"] {
        assert_eq!(c(text).to_string(), text);
    }
}

#[test]
fn parsing_is_canonical_and_strict() {
    assert_eq!(c("Shift+Ctrl+S").to_string(), "Ctrl+Shift+S");
    assert_eq!(c("shift+alt+ctrl+c").to_string(), "Ctrl+Alt+Shift+C");
    assert_eq!(c("Ctrl+S").mods(), Mods::CTRL);
    for bad in [
        "", "S", "Ctrl+", "+F5", "Ctrl++S", "Ctrl+Ctrl+S", "Ctrl+Escape", "Ctrl+Enter", "Ctrl+Backspace", "Ctrl+Foo",
        "Hyper+S", "Shift+A", "Shift+Tab", "Tab", "Left",
    ] {
        assert!(bad.parse::<Chord>().is_err(), "{bad:?} parsed");
    }
    assert!(serde_json::from_str::<Chord>("\"S\"").is_err());
}

#[test]
fn the_default_table() {
    let mut seen = BTreeMap::new();
    for &cmd in Command::ALL {
        assert!(!cmd.defaults().is_empty(), "{cmd:?} has no default");
        for chord in cmd.defaults() {
            assert_eq!(seen.insert(*chord, cmd), None, "{chord} is a default of two commands");
        }
    }

    let ids: BTreeSet<&str> = Command::ALL.iter().map(|cmd| cmd.id()).collect();
    assert_eq!(ids.len(), Command::ALL.len(), "ids are unique");
    for id in ids {
        assert!(!id.starts_with('-') && !id.ends_with('-') && !id.contains("--"), "{id}");
        assert!(id.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'), "{id}");
        assert_eq!(Command::from_id(id).map(Command::id), Some(id));
    }

    let sections: Vec<Section> = Command::ALL.iter().map(|cmd| cmd.section()).collect();
    assert!(sections.is_sorted(), "ALL lists each section's commands together, in section order");
    assert_eq!(Section::ALL.map(Section::title), ["Tabs and Windows", "Navigation", "Page", "General"]);

    assert_eq!(Command::SavePage.id(), "save-page");
    assert_eq!(Command::SavePage.title(), "Save page as…");
    assert_eq!(Command::SavePage.defaults(), [c("Ctrl+Shift+S")]);
    assert_eq!(Command::ToggleTabList.defaults(), [c("Ctrl+S"), c("F9")]);
    assert_eq!(Command::FocusAddress.defaults(), [c("Ctrl+L"), c("Alt+D"), c("F6")]);
    assert_eq!(Command::ZoomIn.defaults(), [c("Ctrl+Plus"), c("Ctrl+Equal"), c("Ctrl+KeypadPlus")]);
    assert_eq!(Command::Print.id(), "print");
    assert_eq!(Command::Print.defaults(), [c("Ctrl+P")]);
    assert_eq!(Command::ViewSource.id(), "view-source");
    assert_eq!(Command::ViewSource.defaults(), [c("Ctrl+U")]);
    assert_eq!(Command::DeveloperTools.id(), "developer-tools");
    assert_eq!(Command::DeveloperTools.defaults(), [c("Ctrl+Shift+I"), c("F12")]);
    assert_eq!(Command::JavaScriptConsole.id(), "javascript-console");
    assert_eq!(Command::JavaScriptConsole.defaults(), [c("Ctrl+Shift+J")]);
    assert_eq!(Command::SearchTabs.id(), "search-tabs");
    assert_eq!(Command::SearchTabs.defaults(), [c("Ctrl+Shift+A")]);
    assert_eq!(Command::SearchTabs.section(), Section::TabsAndWindows);
    assert_eq!(
        [Command::Print, Command::ViewSource, Command::DeveloperTools, Command::JavaScriptConsole].map(Command::section),
        [Section::Page, Section::Page, Section::General, Section::General]
    );

    let keymap = Keymap::default();
    assert!(Command::ALL.iter().all(|&cmd| keymap.is_default(cmd) && keymap.chords(cmd) == cmd.defaults()));
    assert_eq!(keymap.command_for(c("Ctrl+Shift+S")), Some(Command::SavePage));
    assert_eq!(keymap.command_for(c("Ctrl+K")), None);
    assert!(keymap.overrides().is_empty());
}

#[test]
fn assign_takes_and_reset_takes_back() {
    let mut keymap = Keymap::default();
    let taken = keymap.assign(Command::Find, [c("Ctrl+T"), c("Ctrl+F"), c("Ctrl+T")]);
    assert_eq!(taken, [Taken { from: Command::NewTab, chords: vec![c("Ctrl+T")] }]);
    assert_eq!(keymap.chords(Command::Find), [c("Ctrl+T"), c("Ctrl+F")]);
    assert_eq!(keymap.chords(Command::NewTab), []);
    assert_eq!(keymap.command_for(c("Ctrl+T")), Some(Command::Find));
    assert!(!keymap.is_default(Command::NewTab));
    assert_eq!(keymap.overrides(), stored(&[("find", &["Ctrl+T", "Ctrl+F"]), ("new-tab", &[])]));

    let taken = keymap.reset(Command::NewTab);
    assert_eq!(taken, [Taken { from: Command::Find, chords: vec![c("Ctrl+T")] }]);
    assert!(keymap.is_default(Command::NewTab));
    assert!(keymap.is_default(Command::Find), "left with exactly its defaults");
    assert!(keymap.overrides().is_empty(), "overrides that restate defaults are dropped");

    let taken = keymap.assign(Command::CloseTab, [c("Ctrl+W")]);
    assert!(taken.is_empty());
    assert_eq!(keymap.command_for(c("Ctrl+F4")), None);
    assert_eq!(keymap.overrides(), stored(&[("close-tab", &["Ctrl+W"])]));

    let taken = keymap.assign(Command::Quit, [c("Ctrl+W"), c("Ctrl+L"), c("F6")]);
    assert_eq!(
        taken,
        [
            Taken { from: Command::CloseTab, chords: vec![c("Ctrl+W")] },
            Taken { from: Command::FocusAddress, chords: vec![c("Ctrl+L"), c("F6")] },
        ],
        "in Command::ALL order"
    );
    assert_eq!(keymap.chords(Command::FocusAddress), [c("Alt+D")]);

    keymap.reset_all();
    assert_eq!(keymap, Keymap::default());
}

#[test]
fn a_taken_chord_stays_taken_after_its_taker_lets_go() {
    let mut keymap = Keymap::default();
    keymap.assign(Command::Find, [c("Ctrl+T")]);
    keymap.assign(Command::Find, [c("Ctrl+F")]);
    assert_eq!(keymap.chords(Command::NewTab), [], "only reset gives defaults back");
    assert_eq!(keymap.command_for(c("Ctrl+T")), None);
    assert_eq!(keymap.overrides(), stored(&[("new-tab", &[])]));
}

#[test]
fn offering_a_chord_says_who_holds_it() {
    let keymap = Keymap::default();
    let never = |_| false;
    assert_eq!(keymap.offer(Command::NewTab, c("Ctrl+T"), never), (Some(c("Ctrl+T")), None), "a command's own chord");
    assert_eq!(keymap.offer(Command::Reload, c("Ctrl+Alt+F12"), never), (Some(c("Ctrl+Alt+F12")), None), "a free chord");
    assert_eq!(
        keymap.offer(Command::Reload, c("Ctrl+T"), never),
        (Some(c("Ctrl+T")), Some("Also used by New tab. Saving moves it here.".to_owned()))
    );
    assert_eq!(
        keymap.offer(Command::Reload, c("Ctrl+T"), |h| h == Command::NewTab),
        (None, Some("Used by New tab, which cannot be changed".to_owned()))
    );
}

#[test]
fn assigning_a_chord_back_normalizes() {
    let mut keymap = Keymap::default();
    keymap.assign(Command::Find, [c("Ctrl+T")]);
    keymap.assign(Command::NewTab, [c("Ctrl+T")]);
    assert_eq!(keymap.chords(Command::Find), []);
    assert_eq!(keymap.overrides(), stored(&[("find", &[])]), "new-tab's override restates its defaults");

    keymap.assign(Command::Find, [c("Ctrl+F")]);
    assert!(keymap.overrides().is_empty());
    assert_eq!(keymap, Keymap::default());
}

#[test]
fn an_empty_override_removes_the_shortcut() {
    let keymap = Keymap::from_overrides(&stored(&[("new-tab", &[])]));
    assert_eq!(keymap.chords(Command::NewTab), []);
    assert_eq!(keymap.command_for(c("Ctrl+T")), None);
    assert!(!keymap.is_default(Command::NewTab));
    assert_eq!(keymap.overrides(), stored(&[("new-tab", &[])]));

    let mut keymap = Keymap::default();
    keymap.assign(Command::Quit, []);
    assert_eq!(keymap.chords(Command::Quit), []);
    assert_eq!(keymap.overrides(), stored(&[("quit", &[])]));
}

#[test]
fn what_this_build_cannot_read_survives() {
    let mut keymap = Keymap::from_overrides(&stored(&[
        ("close-tab", &["Ctrl+W", "Ctrl+MediaStop", "S"]),
        ("summon-assistant", &["Ctrl+Space", "Ctrl+MediaPlay"]),
    ]));
    assert_eq!(keymap.chords(Command::CloseTab), [c("Ctrl+W")]);
    assert_eq!(keymap.command_for(c("Ctrl+Space")), None, "unknown commands claim nothing here");

    keymap.assign(Command::NewTab, [c("Ctrl+K")]);
    keymap.assign(Command::Quit, [c("Ctrl+F4")]);
    assert_eq!(
        keymap.overrides(),
        stored(&[
            ("close-tab", &["Ctrl+W", "Ctrl+MediaStop", "S"]),
            ("new-tab", &["Ctrl+K"]),
            ("quit", &["Ctrl+F4"]),
            ("summon-assistant", &["Ctrl+Space", "Ctrl+MediaPlay"]),
        ]),
        "taking a chord keeps the strings the loser could not parse"
    );

    keymap.assign(Command::CloseTab, [c("Ctrl+W")]);
    assert_eq!(keymap.overrides()["close-tab"], ["Ctrl+W"], "reassigning replaces them");

    keymap.reset_all();
    assert!(keymap.overrides().is_empty(), "reset all resets commands a newer build knows too");
}

#[test]
fn conflicting_overrides_resolve_in_command_order() {
    let keymap = Keymap::from_overrides(&stored(&[
        ("find", &["Ctrl+T", "Ctrl+K"]),
        ("new-tab", &["Ctrl+T"]),
        ("quit", &["Ctrl+K", "Ctrl+K", "Ctrl+Q"]),
    ]));
    assert_eq!(keymap.chords(Command::NewTab), [c("Ctrl+T")]);
    assert_eq!(keymap.chords(Command::Quit), [c("Ctrl+K"), c("Ctrl+Q")]);
    assert_eq!(keymap.chords(Command::Find), [], "Quit comes before Find");
    assert!(
        keymap.overrides().contains_key("new-tab"),
        "restates its defaults, but dropping it would hand Ctrl+T to find"
    );

    let keymap = Keymap::from_overrides(&stored(&[("find", &["Ctrl+Shift+S"])]));
    assert_eq!(keymap.chords(Command::SavePage), [], "defaults lose to overrides");
    assert_eq!(keymap.command_for(c("Ctrl+Shift+S")), Some(Command::Find));
}

#[test]
fn reset_takes_back_from_every_override_that_names_the_chord() {
    let mut keymap = Keymap::from_overrides(&stored(&[("find", &["Ctrl+T"]), ("quit", &["Ctrl+T", "Ctrl+Q"])]));
    assert_eq!(keymap.command_for(c("Ctrl+T")), Some(Command::Quit));
    let taken = keymap.reset(Command::NewTab);
    assert_eq!(taken, [Taken { from: Command::Quit, chords: vec![c("Ctrl+T")] }], "only the holder lost it");
    assert_eq!(keymap.chords(Command::NewTab), [c("Ctrl+T")]);
    assert_eq!(keymap.chords(Command::Quit), [c("Ctrl+Q")]);
    assert_eq!(keymap.chords(Command::Find), [], "find's claim went too, or it would inherit Ctrl+T");
}

#[test]
fn the_keymap_is_one_synced_pref() {
    let (mut p, _dir) = open();
    assert_eq!(p.prefs().keymap(), Keymap::default());

    let mut keymap = p.prefs().keymap();
    keymap.assign(Command::Find, [c("Ctrl+T")]);
    p.prefs().set_keymap(&keymap).unwrap();
    assert_eq!(p.prefs().keymap(), keymap);

    let exported: Vec<PrefRecord> = p
        .sync()
        .changes_since(Kind::Prefs, Seq::ZERO, usize::MAX)
        .unwrap()
        .records
        .into_iter()
        .map(|w| serde_json::from_slice(&w.body).unwrap())
        .collect();
    let record = exported.iter().find(|r| r.key == "keyboard.shortcuts").expect("synced");
    let value: serde_json::Value = serde_json::from_str(record.value.v.as_ref().unwrap().as_str()).unwrap();
    assert_eq!(value, serde_json::json!({ "find": ["Ctrl+T"], "new-tab": [] }));

    keymap.reset_all();
    p.prefs().set_keymap(&keymap).unwrap();
    assert_eq!(p.prefs().keymap(), Keymap::default());
    assert!(p.prefs().get(&keys::SHORTCUTS).is_empty());
}

#[derive(Clone, Debug)]
enum Op {
    Assign(usize, Vec<usize>),
    Reset(usize),
    ResetAll,
}

/// Every default plus a few chords no command has, so edits both collide and don't.
fn pool() -> Vec<Chord> {
    let mut chords: Vec<Chord> = Command::ALL.iter().flat_map(|cmd| cmd.defaults()).copied().collect();
    chords.extend(["Ctrl+K", "Alt+X", "F2", "Ctrl+Shift+Tab"].map(c).into_iter().filter(|x| !chords.contains(x)).collect::<Vec<_>>());
    chords
}

fn op() -> impl Strategy<Value = Op> {
    let cmd = 0..Command::ALL.len();
    let chords = prop::collection::vec(0..pool().len(), 0..4);
    prop_oneof![
        6 => (cmd.clone(), chords).prop_map(|(cmd, chords)| Op::Assign(cmd, chords)),
        3 => cmd.prop_map(Op::Reset),
        1 => Just(Op::ResetAll),
    ]
}

/// Stored overrides as another device or a hand edit could leave them: conflicting, repeated,
/// unknown ids, unparseable strings.
fn raw_overrides() -> impl Strategy<Value = Overrides> {
    let pool: Vec<String> = pool().iter().map(Chord::to_string).chain(["Ctrl+MediaPlay".to_owned(), "S".to_owned()]).collect();
    let ids: Vec<String> = Command::ALL.iter().map(|cmd| cmd.id().to_owned()).chain(["summon-assistant".to_owned()]).collect();
    prop::collection::btree_map(prop::sample::select(ids), prop::collection::vec(prop::sample::select(pool), 0..4), 0..6)
}

fn assert_consistent(keymap: &Keymap) {
    let mut owners = BTreeMap::new();
    for (cmd, chords) in keymap.iter() {
        for chord in chords {
            assert_eq!(owners.insert(*chord, cmd), None, "{chord} is on two commands");
            assert_eq!(keymap.command_for(*chord), Some(cmd));
        }
    }
    assert_eq!(effective(&Keymap::from_overrides(&keymap.overrides())), effective(keymap), "survives its own overrides");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn no_edit_puts_a_chord_on_two_commands(start in raw_overrides(), ops in prop::collection::vec(op(), 0..24)) {
        let pool = pool();
        let mut keymap = Keymap::from_overrides(&start);
        assert_consistent(&keymap);
        for op in ops {
            let before = keymap.clone();
            let (cmd, taken) = match op {
                Op::Assign(i, chords) => {
                    let cmd = Command::ALL[i];
                    let chords: Vec<Chord> = chords.into_iter().map(|j| pool[j]).collect();
                    let taken = keymap.assign(cmd, chords.iter().copied());
                    let mut wanted = Vec::new();
                    for chord in chords {
                        if !wanted.contains(&chord) {
                            wanted.push(chord);
                        }
                    }
                    prop_assert_eq!(keymap.chords(cmd), wanted.as_slice());
                    (cmd, taken)
                }
                Op::Reset(i) => {
                    let cmd = Command::ALL[i];
                    let taken = keymap.reset(cmd);
                    prop_assert!(keymap.is_default(cmd));
                    (cmd, taken)
                }
                Op::ResetAll => {
                    keymap.reset_all();
                    prop_assert_eq!(&keymap, &Keymap::default());
                    continue;
                }
            };
            for Taken { from, chords } in &taken {
                prop_assert_ne!(*from, cmd);
                for chord in chords {
                    prop_assert_eq!(before.command_for(*chord), Some(*from));
                    prop_assert_eq!(keymap.command_for(*chord), Some(cmd));
                }
            }
            for (other, chords) in before.iter().filter(|(other, _)| *other != cmd) {
                let lost: Vec<Chord> = chords.iter().copied().filter(|x| !keymap.chords(other).contains(x)).collect();
                let reported = taken.iter().find(|t| t.from == other).map_or(&[][..], |t| t.chords.as_slice());
                prop_assert_eq!(lost.as_slice(), reported, "{:?} lost exactly what was reported", other);
            }
            assert_consistent(&keymap);
        }

        let (mut p, _dir) = open();
        p.prefs().set_keymap(&keymap).unwrap();
        prop_assert_eq!(effective(&p.prefs().keymap()), effective(&keymap));
    }
}
