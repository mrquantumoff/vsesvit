//! `chrome.storage` backing: both areas, change events, quotas, what syncs.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::json;
use vsesvit_core::crdt::{DeviceId, Hlc, JsonText, Lww, Seq, Stamp, TimeSource};
use vsesvit_core::ext_storage::{Area, StorageChange, StorageError, SyncItemRecord, SYNC_MAX_ITEMS, SYNC_QUOTA_BYTES_PER_ITEM};
use vsesvit_core::extensions::ExtensionId;
use vsesvit_core::sync::{Kind, WireRecord};
use vsesvit_core::{Error, OpenOptions, Profile};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-stor-{}", uuid::Uuid::new_v4())));
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

fn ext() -> ExtensionId {
    ExtensionId::parse("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap()
}

fn items(pairs: &[(&str, serde_json::Value)]) -> BTreeMap<String, serde_json::Value> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
}

fn keys(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

fn synced(p: &mut Profile) -> Vec<SyncItemRecord> {
    p.sync().changes_since(Kind::ExtStorageSync, Seq::ZERO, usize::MAX).unwrap().records.iter().map(|w| serde_json::from_slice(&w.body).unwrap()).collect()
}

#[test]
fn set_get_remove_clear_in_both_areas() {
    let (mut p, _dir) = open();
    for area in [Area::Local, Area::Sync] {
        let changes = p.ext_storage().set(&ext(), area, items(&[("a", json!(1)), ("b", json!({"x": [1, 2]}))])).unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0], StorageChange { key: "a".into(), old_value: None, new_value: Some(json!(1)) });
        assert_eq!(p.ext_storage().get(&ext(), area, None).unwrap(), items(&[("a", json!(1)), ("b", json!({"x": [1, 2]}))]));
        assert_eq!(p.ext_storage().get(&ext(), area, Some(&keys(&["b", "zz"]))).unwrap(), items(&[("b", json!({"x": [1, 2]}))]));

        assert!(p.ext_storage().set(&ext(), area, items(&[("a", json!(1))])).unwrap().is_empty(), "same value: no change");
        let changes = p.ext_storage().set(&ext(), area, items(&[("a", json!(2))])).unwrap();
        assert_eq!(changes, [StorageChange { key: "a".into(), old_value: Some(json!(1)), new_value: Some(json!(2)) }]);

        let changes = p.ext_storage().remove(&ext(), area, &keys(&["a", "missing"])).unwrap();
        assert_eq!(changes, [StorageChange { key: "a".into(), old_value: Some(json!(2)), new_value: None }]);
        assert_eq!(p.ext_storage().bytes_in_use(&ext(), area, None).unwrap(), 1 + r#"{"x":[1,2]}"#.len());

        let changes = p.ext_storage().clear(&ext(), area).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(p.ext_storage().get(&ext(), area, None).unwrap().is_empty());
        assert!(p.ext_storage().clear(&ext(), area).unwrap().is_empty());
    }
    // areas are independent
    p.ext_storage().set(&ext(), Area::Local, items(&[("k", json!("local"))])).unwrap();
    assert!(p.ext_storage().get(&ext(), Area::Sync, None).unwrap().is_empty());
}

#[test]
fn only_the_sync_area_is_exported_and_removals_are_tombstones() {
    let (mut p, _dir) = open();
    p.ext_storage().set(&ext(), Area::Local, items(&[("k", json!(1))])).unwrap();
    assert!(synced(&mut p).is_empty());
    p.ext_storage().set(&ext(), Area::Sync, items(&[("k", json!(1))])).unwrap();
    p.ext_storage().remove(&ext(), Area::Sync, &keys(&["k"])).unwrap();
    let recs = synced(&mut p);
    assert_eq!(recs.len(), 1);
    assert_eq!((recs[0].key.as_str(), recs[0].value.v.clone()), ("k", None));
    let ids: Vec<String> = p.sync().changes_since(Kind::ExtStorageSync, Seq::ZERO, usize::MAX).unwrap().records.into_iter().map(|w| w.id).collect();
    assert!(ids[0].starts_with("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:"));
    assert_eq!(ids[0].len(), 32 + 1 + 32);
}

#[test]
fn quotas_are_all_or_nothing_and_local_only() {
    let (mut p, _dir) = open();
    let big = "x".repeat(SYNC_QUOTA_BYTES_PER_ITEM);
    let r = p.ext_storage().set(&ext(), Area::Sync, items(&[("ok", json!(1)), ("big", json!(big))]));
    assert!(matches!(r, Err(Error::Storage(StorageError::QuotaBytesPerItem))));
    assert!(p.ext_storage().get(&ext(), Area::Sync, None).unwrap().is_empty(), "nothing written");

    let many: BTreeMap<String, serde_json::Value> = (0..=SYNC_MAX_ITEMS).map(|i| (format!("k{i}"), json!(0))).collect();
    assert!(matches!(p.ext_storage().set(&ext(), Area::Sync, many), Err(Error::Storage(StorageError::MaxItems))));

    let chunk = "y".repeat(8_000);
    let mut total = BTreeMap::new();
    for i in 0..13 {
        total.insert(format!("c{i}"), json!(chunk));
    }
    assert!(matches!(p.ext_storage().set(&ext(), Area::Sync, total.clone()), Err(Error::Storage(StorageError::QuotaBytes))));
    p.ext_storage().set(&ext(), Area::Local, total).unwrap();

    // a remote record over quota is applied anyway (its stamp is newer than anything local)
    let at = Stamp { hlc: Hlc((1_780_000_000_000 + 60_000) << 16), device: DeviceId(9) };
    let huge = SyncItemRecord { ext: ext(), key: "remote".into(), value: Lww::new(Some(JsonText::from_value(&json!("z".repeat(200_000)))), at) };
    let wire = WireRecord { kind: Kind::ExtStorageSync, id: format!("{}:{}", ext().as_str(), "0".repeat(32)), body: serde_json::to_vec(&huge).unwrap() };
    let report = p.sync().apply(vec![wire]).unwrap();
    assert_eq!(report.rejected.len(), 1, "but its id must match the key hash");
    let real_id = {
        p.ext_storage().set(&ext(), Area::Sync, items(&[("remote", json!(0))])).unwrap();
        p.sync().changes_since(Kind::ExtStorageSync, Seq::ZERO, usize::MAX).unwrap().records.into_iter().find(|w| w.body.windows(8).any(|w| w == b"\"remote\"")).unwrap().id
    };
    let report = p.sync().apply(vec![WireRecord { kind: Kind::ExtStorageSync, id: real_id, body: serde_json::to_vec(&huge).unwrap() }]).unwrap();
    assert_eq!(report.merged, 1);
    assert_eq!(report.changed.ext_storage.len(), 1);
    let (who, changes) = &report.changed.ext_storage[0];
    assert_eq!(who, &ext());
    assert_eq!(changes[0].key, "remote");
    assert_eq!(changes[0].old_value, Some(json!(0)));
    assert_eq!(changes[0].new_value.as_ref().unwrap().as_str().unwrap().len(), 200_000);
}
