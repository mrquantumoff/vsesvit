//! Extensions' keyboard shortcuts: suggested keys resolved against the browser's keymap and
//! each other, the user's changes stored next to the browser's, and a property test that no
//! sequence of edits puts a chord on two commands.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use proptest::prelude::*;

use vsesvit_core::crdt::TimeSource;
use vsesvit_core::extensions::commands::{ExtensionCommand, ExtensionShortcuts};
use vsesvit_core::extensions::manifest::ManifestCommand;
use vsesvit_core::extensions::{ExtensionId, InstallSource};
use vsesvit_core::shortcuts::{Chord, Command, Keymap};
use vsesvit_core::{OpenOptions, Profile};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct World {
    p: Profile,
    now: Rc<Cell<u64>>,
    dir: TempDir,
}

fn open() -> World {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-ext-shortcuts-{}", uuid::Uuid::new_v4().simple())));
    let now = Rc::new(Cell::new(1_780_000_000_000));
    let p = Profile::open(&dir.0.join("profile"), OpenOptions { time: TimeSource::Manual(now.clone()), ..OpenOptions::default() }).unwrap();
    World { p, now, dir }
}

impl World {
    /// Installs an unpacked extension named `name` with an action and these `commands`, a
    /// millisecond after the previous one, so install order is call order.
    fn install(&mut self, name: &str, commands: serde_json::Value) -> ExtensionId {
        let dev = self.dir.0.join(name);
        fs::create_dir_all(&dev).unwrap();
        let manifest = serde_json::json!({"manifest_version": 3, "name": name, "version": "1", "action": {}, "commands": commands});
        fs::write(dev.join("manifest.json"), manifest.to_string()).unwrap();
        self.install_dir(dev)
    }

    fn install_dir(&mut self, dir: PathBuf) -> ExtensionId {
        self.now.set(self.now.get() + 1);
        let staged = self.p.extensions().prepare_install(InstallSource::Unpacked { dir }).unwrap().run(&mut |_| {}).unwrap();
        self.p.extensions().commit(staged).unwrap().unwrap().id
    }

    fn shortcuts(&mut self) -> ExtensionShortcuts {
        self.p.extension_shortcuts().unwrap()
    }

    /// Runs `edit` on the stored keymap and stores the result.
    fn edit(&mut self, edit: impl FnOnce(&mut Keymap, &ExtensionShortcuts)) {
        let shortcuts = self.shortcuts();
        let mut keymap = self.p.prefs().keymap();
        edit(&mut keymap, &shortcuts);
        self.p.prefs().set_keymap(&keymap).unwrap();
    }

    fn chords(&mut self) -> Vec<(String, Option<String>)> {
        self.shortcuts().iter().map(|(c, chord)| (format!("{}:{}", c.extension_name, c.command.name), chord.map(|k| k.to_string()))).collect()
    }
}

fn c(s: &str) -> Chord {
    s.parse().unwrap_or_else(|e| panic!("{e}"))
}

fn run(key: &str) -> serde_json::Value {
    serde_json::json!({"run": {"suggested_key": key, "description": "Run"}})
}

fn row(name: &str, chord: Option<&str>) -> (String, Option<String>) {
    (format!("{name}:run"), chord.map(str::to_owned))
}

fn key(id: &ExtensionId, name: &str) -> String {
    format!("extension:{}:{name}", id.as_str())
}

#[test]
fn the_probe_lists_its_commands() {
    let mut w = open();
    let probe = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/extensions/probe").canonicalize().unwrap();
    let id = w.install_dir(probe);
    let shortcuts = w.shortcuts();
    let listed: Vec<(&str, &str, Option<Chord>)> = shortcuts.iter().map(|(cmd, chord)| (cmd.command.name.as_str(), cmd.title(), chord)).collect();
    assert_eq!(
        listed,
        [
            ("_execute_action", "Activate the extension", Some(c("Alt+Shift+P"))),
            ("probe-command", "Vsesvit Probe command", Some(c("Alt+Shift+K"))),
        ]
    );
    assert!(shortcuts.is_default(&id, "probe-command"));
    assert_eq!(shortcuts.command_for(c("Alt+Shift+K")).map(|cmd| cmd.extension_name.as_str()), Some("Vsesvit Probe"));

    w.p.extensions().set_enabled(&id, false).unwrap();
    assert_eq!(w.shortcuts().iter().count(), 0, "a disabled extension's commands are not listed");
}

#[test]
fn the_browser_wins_over_suggested_and_overridden_keys() {
    let mut w = open();
    let a = w.install("A", serde_json::json!({"run": {"suggested_key": "Ctrl+T"}, "other": {"suggested_key": "Alt+Shift+O"}}));
    assert_eq!(w.chords(), [("A:other".into(), Some("Alt+Shift+O".into())), row("A", None)], "Ctrl+T is New tab's");

    let mut keymap = Keymap::from_overrides(&BTreeMap::from([(key(&a, "other"), vec!["Ctrl+W".to_owned()])]));
    w.p.prefs().set_keymap(&keymap).unwrap();
    assert_eq!(w.chords(), [("A:other".into(), None), row("A", None)], "Ctrl+W is Close tab's");

    keymap.assign(Command::CloseTab, [c("F4")]);
    w.p.prefs().set_keymap(&keymap).unwrap();
    assert_eq!(w.chords()[0], ("A:other".into(), Some("Ctrl+W".into())), "free again, the stored override claims it");
}

#[test]
fn the_first_installed_wins_a_contested_suggested_key_and_an_override_beats_it() {
    let mut w = open();
    let _a = w.install("A", run("Alt+Shift+K"));
    let b = w.install("B", run("Alt+Shift+K"));
    assert_eq!(w.chords(), [row("A", Some("Alt+Shift+K")), row("B", None)]);
    let shortcuts = w.shortcuts();
    assert!(shortcuts.iter().all(|(cmd, _)| shortcuts.is_default(&cmd.extension, "run")));

    let keymap = Keymap::from_overrides(&BTreeMap::from([(key(&b, "run"), vec!["Alt+Shift+K".to_owned()])]));
    w.p.prefs().set_keymap(&keymap).unwrap();
    assert_eq!(w.chords(), [row("A", None), row("B", Some("Alt+Shift+K"))]);
}

#[test]
fn assign_takes_and_reset_takes_back() {
    let mut w = open();
    let a = w.install("A", run("Alt+Shift+K"));
    let b = w.install("B", run("Alt+Shift+J"));

    w.edit(|keymap, s| keymap.assign_extension(s, &b, "run", Some(c("Alt+Shift+K"))));
    assert_eq!(w.chords(), [row("A", None), row("B", Some("Alt+Shift+K"))]);
    let stored = w.p.prefs().keymap().overrides();
    assert_eq!(stored, BTreeMap::from([(key(&a, "run"), vec![]), (key(&b, "run"), vec!["Alt+Shift+K".to_owned()])]));

    w.edit(|keymap, s| keymap.assign_extension(s, &b, "run", Some(c("Alt+Shift+J"))));
    assert_eq!(w.chords(), [row("A", None), row("B", Some("Alt+Shift+J"))], "a taken chord stays taken after its taker lets go");
    assert_eq!(w.p.prefs().keymap().overrides(), BTreeMap::from([(key(&a, "run"), vec![])]), "B's own key needs no override");
    let shortcuts = w.shortcuts();
    assert!(!shortcuts.is_default(&a, "run") && shortcuts.is_default(&b, "run"));

    w.edit(|keymap, s| keymap.assign_extension(s, &b, "run", Some(c("Alt+Shift+K"))));
    w.edit(|keymap, s| keymap.reset_extension(s, &a, "run"));
    assert_eq!(w.chords(), [row("A", Some("Alt+Shift+K")), row("B", None)]);
    assert_eq!(w.p.prefs().keymap().overrides(), BTreeMap::from([(key(&b, "run"), vec![])]));

    w.edit(|keymap, s| keymap.reset_extension(s, &b, "run"));
    assert!(w.p.prefs().keymap().overrides().is_empty());
    assert_eq!(w.chords(), [row("A", Some("Alt+Shift+K")), row("B", Some("Alt+Shift+J"))]);

    w.edit(|keymap, s| keymap.assign_extension(s, &a, "run", Some(c("Ctrl+T"))));
    assert_eq!(w.chords()[0], row("A", Some("Alt+Shift+K")), "a chord the browser holds is never assigned");
    assert_eq!(w.p.prefs().keymap().command_for(c("Ctrl+T")), Some(Command::NewTab));
}

#[test]
fn reset_takes_the_key_from_an_earlier_command_that_would_claim_it() {
    let mut w = open();
    let a = w.install("A", run("Alt+Shift+K"));
    let b = w.install("B", run("Alt+Shift+K"));
    let c_ = w.install("C", run("Alt+Shift+C"));
    w.edit(|keymap, s| keymap.assign_extension(s, &c_, "run", Some(c("Alt+Shift+K"))));
    assert_eq!(w.chords(), [row("A", None), row("B", None), row("C", Some("Alt+Shift+K"))]);

    w.edit(|keymap, s| keymap.reset_extension(s, &b, "run"));
    assert_eq!(w.chords(), [row("A", None), row("B", Some("Alt+Shift+K")), row("C", None)]);
    assert!(w.shortcuts().is_default(&b, "run"));
    assert!(!w.shortcuts().is_default(&a, "run"));
}

#[test]
fn removing_a_shortcut_sticks_until_reset() {
    let mut w = open();
    let a = w.install("A", run("Alt+Shift+K"));
    w.edit(|keymap, s| keymap.assign_extension(s, &a, "run", None));
    assert_eq!(w.chords(), [row("A", None)]);
    assert_eq!(w.p.prefs().keymap().overrides(), BTreeMap::from([(key(&a, "run"), vec![])]));
    assert!(!w.shortcuts().is_default(&a, "run"));

    w.edit(|keymap, s| keymap.reset_extension(s, &a, "run"));
    assert_eq!(w.chords(), [row("A", Some("Alt+Shift+K"))]);

    let b = w.install("B", serde_json::json!({"run": {}}));
    w.edit(|keymap, s| keymap.assign_extension(s, &b, "run", None));
    assert_eq!(
        w.p.prefs().keymap().overrides(),
        BTreeMap::from([(key(&b, "run"), vec![])]),
        "kept without a suggested key too, so an update that adds one does not bring it back"
    );
}

#[test]
fn stored_entries_survive_the_keymap_and_reset_all_clears_them() {
    let mut w = open();
    let a = w.install("A", run("Alt+Shift+K"));
    let stored = BTreeMap::from([
        (key(&a, "run"), vec!["Ctrl+MediaPlay".to_owned(), "Alt+Shift+J".to_owned()]),
        ("new-tab".to_owned(), vec!["Ctrl+K".to_owned()]),
        ("extension:gone@example.org:run".to_owned(), vec!["Alt+Shift+K".to_owned()]),
    ]);
    let mut keymap = Keymap::from_overrides(&stored);
    assert_eq!(keymap.overrides(), stored);
    w.p.prefs().set_keymap(&keymap).unwrap();
    assert_eq!(w.p.prefs().keymap().overrides(), stored);
    assert_eq!(w.chords(), [row("A", Some("Alt+Shift+J"))], "the first stored chord this build reads");

    w.edit(|keymap, s| keymap.reset_extension(s, &a, "run"));
    assert_eq!(w.chords(), [row("A", Some("Alt+Shift+K"))], "an uninstalled extension's entry claims nothing");
    assert!(w.p.prefs().keymap().overrides().contains_key("extension:gone@example.org:run"));

    keymap.reset_all();
    assert!(keymap.overrides().is_empty());
}

#[test]
fn an_uninstalled_extensions_entry_claims_nothing() {
    let mut w = open();
    let a = w.install("A", run("Alt+Shift+K"));
    let b = w.install("B", run("Alt+Shift+J"));
    w.edit(|keymap, s| keymap.assign_extension(s, &b, "run", Some(c("Alt+Shift+K"))));
    w.p.extensions().uninstall(&b).unwrap();
    assert_eq!(w.chords(), [row("A", None)], "A lost its key to B, and only reset gives it back");
    w.edit(|keymap, s| keymap.reset_extension(s, &a, "run"));
    assert_eq!(w.chords(), [row("A", Some("Alt+Shift+K"))]);
    assert!(w.p.prefs().keymap().overrides().contains_key(&key(&b, "run")), "kept for a reinstall");
}

#[test]
fn offering_a_chord_says_who_holds_it() {
    let mut w = open();
    let a = w.install("A", serde_json::json!({"_execute_action": {"suggested_key": "Alt+Shift+K"}}));
    let b = w.install("B", run("Alt+Shift+J"));
    let shortcuts = w.shortcuts();
    let keymap = w.p.prefs().keymap();
    assert_eq!(
        keymap.offer_extension(&shortcuts, &b, "run", c("Ctrl+T")),
        (None, Some("Used by New tab, which an extension cannot take".to_owned()))
    );
    assert_eq!(
        keymap.offer_extension(&shortcuts, &b, "run", c("Alt+Shift+K")),
        (Some(c("Alt+Shift+K")), Some("Also used by A: Activate the extension. Saving moves it here.".to_owned()))
    );
    assert_eq!(keymap.offer_extension(&shortcuts, &a, "_execute_action", c("Alt+Shift+K")), (Some(c("Alt+Shift+K")), None), "its own");
    assert_eq!(keymap.offer_extension(&shortcuts, &b, "run", c("Alt+Shift+X")), (Some(c("Alt+Shift+X")), None), "a free chord");

    assert_eq!(shortcuts.note_for_browser(c("Alt+Shift+J")), Some("Also used by B: Run. Saving moves it here.".to_owned()));
    assert_eq!(shortcuts.note_for_browser(c("Alt+Shift+X")), None);
}

#[derive(Clone, Debug)]
enum Op {
    AssignBrowser(usize, Vec<usize>),
    ResetBrowser(usize),
    Assign(usize, Option<usize>),
    Reset(usize),
    ResetAll,
}

/// Browser defaults and suggested keys mixed, so edits both collide and don't.
fn pool() -> Vec<Chord> {
    ["Ctrl+T", "Ctrl+W", "Alt+Shift+K", "Alt+Shift+J", "Alt+Shift+P", "Ctrl+Shift+Y", "F2"].map(c).to_vec()
}

/// Four extensions' commands, three of them suggesting a key another one or the browser holds.
fn commands() -> Vec<ExtensionCommand> {
    [("A", "Alt+Shift+K"), ("B", "Alt+Shift+K"), ("C", "Ctrl+T"), ("D", "Alt+Shift+J")]
        .into_iter()
        .enumerate()
        .map(|(i, (name, suggested))| ExtensionCommand {
            extension: ExtensionId::parse(&format!("ext{i}@example.org")).unwrap(),
            extension_name: name.to_owned(),
            command: ManifestCommand { name: "run".to_owned(), description: String::new(), suggested_key: Some(c(suggested)) },
        })
        .collect()
}

fn op() -> impl Strategy<Value = Op> {
    let browser = 0..Command::ALL.len();
    let ext = 0..commands().len();
    let chord = 0..pool().len();
    prop_oneof![
        2 => (browser.clone(), prop::collection::vec(chord.clone(), 0..3)).prop_map(|(cmd, chords)| Op::AssignBrowser(cmd, chords)),
        1 => browser.prop_map(Op::ResetBrowser),
        5 => (ext.clone(), prop::option::of(chord)).prop_map(|(cmd, chord)| Op::Assign(cmd, chord)),
        3 => ext.prop_map(Op::Reset),
        1 => Just(Op::ResetAll),
    ]
}

fn assert_consistent(keymap: &Keymap, shortcuts: &ExtensionShortcuts) {
    let mut owners = BTreeMap::new();
    for (cmd, chords) in keymap.iter() {
        for chord in chords {
            owners.insert(*chord, format!("{cmd:?}"));
        }
    }
    for (cmd, chord) in shortcuts.iter() {
        if let Some(chord) = chord {
            assert_eq!(owners.insert(chord, cmd.extension_name.clone()), None, "{chord} is on two commands");
            assert_eq!(shortcuts.command_for(chord), Some(cmd));
        }
    }
    assert_eq!(Keymap::from_overrides(&keymap.overrides()).extension_shortcuts(commands()), *shortcuts, "survives its own overrides");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn no_edit_puts_a_chord_on_two_commands(ops in prop::collection::vec(op(), 0..24)) {
        let pool = pool();
        let mut keymap = Keymap::default();
        for op in ops {
            let before = keymap.extension_shortcuts(commands());
            match op {
                Op::AssignBrowser(i, chords) => {
                    keymap.assign(Command::ALL[i], chords.into_iter().map(|j| pool[j]));
                }
                Op::ResetBrowser(i) => {
                    keymap.reset(Command::ALL[i]);
                }
                Op::Assign(i, chord) => {
                    let target = &commands()[i];
                    let chord = chord.map(|j| pool[j]);
                    let browser_holds = chord.is_some_and(|chord| keymap.command_for(chord).is_some());
                    keymap.assign_extension(&before, &target.extension, "run", chord);
                    let after = keymap.extension_shortcuts(commands());
                    if browser_holds {
                        prop_assert_eq!(&after, &before);
                    } else {
                        prop_assert_eq!(after.chord(&target.extension, "run"), chord);
                    }
                }
                Op::Reset(i) => {
                    let target = &commands()[i];
                    keymap.reset_extension(&before, &target.extension, "run");
                    let after = keymap.extension_shortcuts(commands());
                    prop_assert!(after.is_default(&target.extension, "run"));
                    let suggested = target.command.suggested_key.filter(|&k| keymap.command_for(k).is_none());
                    prop_assert_eq!(after.chord(&target.extension, "run"), suggested);
                }
                Op::ResetAll => {
                    keymap.reset_all();
                    prop_assert_eq!(&keymap, &Keymap::default());
                }
            }
            assert_consistent(&keymap, &keymap.extension_shortcuts(commands()));
        }
    }
}
