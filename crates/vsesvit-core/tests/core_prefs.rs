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

/// `Prefs::set` is generic: a `Pref<Option<T>>` set to `None` serializes to JSON `null`,
/// which is a stored value, not a reset. It must reach other devices as that value.
#[test]
fn a_pref_whose_value_is_json_null_syncs_as_a_value() {
    let opt: Pref<Option<String>> = Pref { key: "future.opt", scope: Scope::Synced, default: || Some("d".to_owned()) };
    let (mut a, _da) = open();
    let (mut b, _db) = open();
    a.prefs().set(&opt, &None).unwrap();
    assert_eq!(a.prefs().get(&opt), None, "the stored null wins over the default");
    let upload = a.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap();
    let report = b.sync().apply(upload.records.clone()).unwrap();
    assert_eq!(report.merged, 1);
    assert_eq!(exported(&mut b), exported(&mut a));
    assert_eq!(exported(&mut b)[0].1.value.v, Some(JsonText::from_value(&serde_json::Value::Null)));
    assert_eq!(b.prefs().get(&opt), None);
    let report = a.sync().apply(upload.records).unwrap();
    assert_eq!((report.merged, report.unchanged), (0, 1));
    assert!(a.sync().changes_since(Kind::Prefs, upload.upto, usize::MAX).unwrap().records.is_empty(), "the record echoes");
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

#[test]
fn the_media_player_and_picture_in_picture_are_on_by_default_and_synced() {
    let (mut p, _dir) = open();
    assert_eq!((keys::SHOW_MEDIA_PLAYER.key, keys::PICTURE_IN_PICTURE.key), ("media.player", "media.picture_in_picture"));
    assert_eq!((keys::SHOW_MEDIA_PLAYER.scope, keys::PICTURE_IN_PICTURE.scope), (Scope::Synced, Scope::Synced));
    assert!(p.prefs().get(&keys::SHOW_MEDIA_PLAYER));
    assert!(p.prefs().get(&keys::PICTURE_IN_PICTURE));
    p.prefs().set(&keys::SHOW_MEDIA_PLAYER, &false).unwrap();
    p.prefs().set(&keys::PICTURE_IN_PICTURE, &false).unwrap();
    assert!(!p.prefs().get(&keys::SHOW_MEDIA_PLAYER));
    assert!(!p.prefs().get(&keys::PICTURE_IN_PICTURE));
    let synced: Vec<String> = exported(&mut p).into_iter().map(|(key, _)| key).collect();
    assert_eq!(synced, ["media.player", "media.picture_in_picture"]);
}

#[test]
fn the_extension_toolbar_list_round_trips_and_syncs() {
    use vsesvit_core::extensions::toolbar::{Entry, TOOLBAR};

    let (mut p, _dir) = open();
    assert_eq!(p.prefs().get(&TOOLBAR), Vec::<Entry>::new());
    let list = vec![Entry { id: "b".into(), pinned: true }, Entry { id: "a".into(), pinned: false }];
    p.prefs().set(&TOOLBAR, &list).unwrap();
    assert_eq!(p.prefs().get(&TOOLBAR), list);
    let (_, record) = exported(&mut p).into_iter().find(|(key, _)| key == TOOLBAR.key).expect("synced");
    assert_eq!(record.value.v.unwrap().as_str(), r#"[{"id":"b","pinned":true},{"id":"a","pinned":false}]"#);
}

/// Local prefs are this device's alone. A sync server or another device sending one (an honest
/// peer never uploads them) must not set it, with or without a row here, so a server cannot pick
/// where downloads land, which server this device signs in to, or what it syncs.
#[test]
fn remote_records_never_write_local_prefs() {
    use vsesvit_core::sync::DataType;
    let (mut p, _dir) = open();
    p.prefs().set(&keys::DOWNLOADS_DIR, &Some(PathBuf::from("/home/me/Incoming"))).unwrap();
    let shell_local: Pref<u32> = Pref { key: "shell.local", scope: Scope::Local, default: || 0 };
    p.prefs().set(&shell_local, &7).unwrap();
    let at = Stamp { hlc: Hlc(1_780_000_000_000 << 16), device: DeviceId(9) };
    let rec = |key: &str, value: serde_json::Value| PrefRecord { key: key.into(), value: Lww::new(Some(JsonText::from_value(&value)), at) };
    let wire = |r: PrefRecord| WireRecord { kind: Kind::Prefs, id: r.key.clone(), body: serde_json::to_vec(&r).unwrap() };
    let report = p
        .sync()
        .apply(vec![
            wire(rec("downloads.directory", serde_json::json!("/attacker/autostart"))),
            wire(rec("sync.server", serde_json::json!("https://evil.example"))),
            wire(rec("sync.types", serde_json::json!(["history"]))),
            wire(rec("shell.local", serde_json::json!(9))),
            wire(rec("theme", serde_json::json!("dark"))),
        ])
        .unwrap();
    let rejected: Vec<_> = report.rejected.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(rejected, ["downloads.directory", "sync.server", "sync.types", "shell.local"]);
    assert_eq!((report.merged, &report.changed.prefs[..]), (1, &["theme".to_owned()][..]));
    assert_eq!(p.prefs().get(&keys::THEME), Theme::Dark);
    assert_eq!(p.prefs().get(&keys::DOWNLOADS_DIR), Some(PathBuf::from("/home/me/Incoming")));
    assert_eq!(p.prefs().get(&keys::SYNC_SERVER), vsesvit_core::prefs::DEFAULT_SYNC_SERVER);
    assert_eq!(p.prefs().get(&keys::SYNC_TYPES), DataType::ALL.to_vec());
    assert_eq!(p.prefs().get(&shell_local), 7);
    let keys: Vec<_> = exported(&mut p).into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["theme"]);
}

/// A Local pref declared outside core (a shell's) that this device never wrote has no row to
/// guard it, so a remote record for it is stored like any unknown key; reading the pref still
/// ignores it, and writing it, even to the stored value, takes the row back as local.
#[test]
fn a_local_pref_reads_only_what_this_device_wrote() {
    let (mut p, _dir) = open();
    let shell_local: Pref<u32> = Pref { key: "shell.fresh", scope: Scope::Local, default: || 0 };
    let at = Stamp { hlc: Hlc(1 << 16), device: DeviceId(9) };
    let rec = PrefRecord { key: shell_local.key.into(), value: Lww::new(Some(JsonText::from_value(&serde_json::json!(9))), at) };
    let wire = WireRecord { kind: Kind::Prefs, id: rec.key.clone(), body: serde_json::to_vec(&rec).unwrap() };
    assert_eq!(p.sync().apply(vec![wire]).unwrap().merged, 1);
    assert_eq!(p.prefs().get(&shell_local), 0);
    p.prefs().set(&shell_local, &9).unwrap();
    assert_eq!(p.prefs().get(&shell_local), 9, "setting the value sync stored still sticks");
    assert!(exported(&mut p).is_empty(), "the local write takes the row back from sync");
    p.prefs().set(&shell_local, &5).unwrap();
    assert_eq!(p.prefs().get(&shell_local), 5);
    assert!(exported(&mut p).is_empty(), "the local write takes the row back from sync");
}

/// A Local pref has nothing to upload, so writing one must not move `change_seq`, which the
/// shells watch to start a sync.
#[test]
fn local_prefs_leave_the_change_seq_alone() {
    let (mut p, _dir) = open();
    let before = p.change_seq();
    p.prefs().set(&keys::DOWNLOADS_DIR, &Some(PathBuf::from("/tmp/x"))).unwrap();
    p.prefs().set(&keys::HARDWARE_ACCELERATION, &false).unwrap();
    p.prefs().reset(&keys::HARDWARE_ACCELERATION).unwrap();
    assert_eq!(p.change_seq(), before);
    p.prefs().set(&keys::DOWNLOADS_ASK, &true).unwrap();
    assert!(p.change_seq() > before);
    assert_eq!(p.prefs().get(&keys::DOWNLOADS_DIR), Some(PathBuf::from("/tmp/x")));
}
