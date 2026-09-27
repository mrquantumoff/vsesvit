//! Typed preferences: defaults, set/reset, no-op writes, local scope, unknown keys.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, Hlc, JsonText, Lww, Seq, Stamp, TimeSource};
use vsesvit_core::prefs::{keys, Pref, PrefRecord, Scope, Startup, Theme};
use vsesvit_core::search::SearchEngineId;
use vsesvit_core::sync::{Kind, WireRecord};
use vsesvit_core::{OpenOptions, Profile};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-prefs-{}", uuid::Uuid::new_v4())));
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

fn exported(p: &mut Profile) -> Vec<(String, PrefRecord)> {
    p.sync()
        .changes_since(Kind::Prefs, Seq::ZERO, usize::MAX)
        .unwrap()
        .records
        .into_iter()
        .map(|w| (w.id, serde_json::from_slice(&w.body).unwrap()))
        .collect()
}

#[test]
fn defaults_set_and_reset() {
    let (mut p, _dir) = open();
    assert_eq!(p.prefs().get(&keys::THEME), Theme::System);
    assert_eq!(p.prefs().get(&keys::HOMEPAGE), "about:home");
    assert_eq!(p.prefs().get(&keys::STARTUP), Startup::RestoreSession);
    assert!(p.prefs().get(&keys::SHOW_BOOKMARKS_BAR));
    assert_eq!(p.prefs().get(&keys::DEFAULT_SEARCH_ENGINE), SearchEngineId::builtin_default());

    p.prefs().set(&keys::THEME, &Theme::Dark).unwrap();
    p.prefs().set(&keys::HOMEPAGE, &"https://start.example/".to_owned()).unwrap();
    assert_eq!(p.prefs().get(&keys::THEME), Theme::Dark);
    assert_eq!(p.prefs().get(&keys::HOMEPAGE), "https://start.example/");

    let before = p.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap().upto;
    p.prefs().set(&keys::THEME, &Theme::Dark).unwrap();
    assert_eq!(p.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap().upto, before, "unchanged set mints nothing");

    p.prefs().reset(&keys::THEME).unwrap();
    assert_eq!(p.prefs().get(&keys::THEME), Theme::System);
    let ex = exported(&mut p);
    let theme = &ex.iter().find(|(k, _)| k == "theme").unwrap().1;
    assert_eq!(theme.value.v, None, "a reset is a synced None, not a missing row");
    let again = p.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap().upto;
    p.prefs().reset(&keys::THEME).unwrap();
    assert_eq!(p.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap().upto, again);
}

#[test]
fn explicit_default_is_stored_and_local_scope_never_exports() {
    let (mut p, _dir) = open();
    p.prefs().set(&keys::SHOW_BOOKMARKS_BAR, &true).unwrap();
    p.prefs().set(&keys::DEVICE_NAME, &"my laptop".to_owned()).unwrap();
    assert_eq!(p.prefs().get(&keys::DEVICE_NAME), "my laptop");
    let ex = exported(&mut p);
    assert_eq!(ex.len(), 1);
    assert_eq!(ex[0].0, "bookmarks_bar.visible");
    assert_eq!(ex[0].1.value.v.as_ref().unwrap().as_str(), "true");
}

#[test]
fn undecodable_values_read_as_default_and_unknown_keys_round_trip() {
    let (mut p, _dir) = open();
    let at = Stamp { hlc: Hlc(1 << 16), device: DeviceId(9) };
    let newer_theme = PrefRecord { key: "theme".into(), value: Lww::new(Some(JsonText::from_value(&serde_json::json!({"mode": "oled"}))), at) };
    let unknown = PrefRecord { key: "future.flag".into(), value: Lww::new(Some(JsonText::from_value(&serde_json::json!(3))), at) };
    let wire = |r: &PrefRecord| WireRecord { kind: Kind::Prefs, id: r.key.clone(), body: serde_json::to_vec(r).unwrap() };
    let report = p.sync().apply(vec![wire(&newer_theme), wire(&unknown)]).unwrap();
    assert_eq!(report.merged, 2);
    assert_eq!(report.changed.prefs, ["theme", "future.flag"]);
    assert_eq!(p.prefs().get(&keys::THEME), Theme::System, "a shape this build cannot decode reads as the default");
    let ex = exported(&mut p);
    assert_eq!(ex.len(), 2);
    assert!(ex.iter().any(|(k, r)| k == "future.flag" && r == &unknown), "unknown keys are kept intact");
    // and the stored value was left alone for the newer build
    assert!(ex.iter().any(|(k, r)| k == "theme" && r == &newer_theme));

    let custom: Pref<u32> = Pref { key: "future.flag", scope: Scope::Synced, default: || 0 };
    assert_eq!(p.prefs().get(&custom), 3);
}

#[test]
fn tabs_are_vertical_on_the_left_by_default_and_the_choice_is_synced() {
    use vsesvit_core::prefs::TabsPosition;
    let (mut p, _dir) = open();
    assert_eq!(p.prefs().get(&keys::TABS_POSITION), TabsPosition::Left);
    p.prefs().set(&keys::TABS_POSITION, &TabsPosition::Right).unwrap();
    assert_eq!(p.prefs().get(&keys::TABS_POSITION), TabsPosition::Right);
    assert!(exported(&mut p).iter().any(|(id, _)| id == "tabs.position"));
}
