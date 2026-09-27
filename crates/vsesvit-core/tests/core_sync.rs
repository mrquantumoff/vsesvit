//! The sync surface itself: paging over seq groups, boundary rejections, engine state,
//! the extensions desired-state table, and the dirty-marking rule against a stale copy.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, BookmarkRecord, InsertAt, NodeKind};
use vsesvit_core::crdt::{DeviceId, Extra, Hlc, Lww, Seq, Stamp, TimeSource};
use vsesvit_core::extensions::{ExtensionId, ExtensionRecord, StoreRef};
use vsesvit_core::sync::{Kind, WireRecord};
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open(device: u64) -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-sync-{}", uuid::Uuid::new_v4())));
    let p = Profile::open(
        &dir.0,
        OpenOptions {
            time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000 + device))),
            new_device_id: Some(DeviceId(device)),
            ..OpenOptions::default()
        },
    )
    .unwrap();
    (p, dir)
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn all(p: &mut Profile, kind: Kind) -> Vec<WireRecord> {
    p.sync().changes_since(kind, Seq::ZERO, usize::MAX).unwrap().records
}

#[test]
fn kind_codes() {
    for &k in Kind::ALL {
        assert_eq!(Kind::from_code(k.code()), Some(k));
    }
    assert_eq!(Kind::from_code(9), None);
    assert_eq!(Kind::from_code(0), None);
}

#[test]
fn paging_walks_every_row_once_and_never_splits_a_seq_group() {
    let (mut p, _dir) = open(1);
    for i in 0..3 {
        p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, &format!("a{i}"), &url(&format!("https://a{i}.example/"))).unwrap();
    }
    let f = p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "F").unwrap();
    for i in 0..5 {
        p.bookmarks().add_url(f, InsertAt::End, &format!("c{i}"), &url(&format!("https://c{i}.example/"))).unwrap();
    }
    p.bookmarks().remove(f).unwrap(); // six rows re-marked with one seq
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "tail", &url("https://tail.example/")).unwrap();

    let mut cursor = Seq::ZERO;
    let mut sizes = Vec::new();
    let mut seen = Vec::new();
    loop {
        let batch = p.sync().changes_since(Kind::Bookmarks, cursor, 2).unwrap();
        if batch.records.is_empty() {
            assert!(!batch.more);
            break;
        }
        sizes.push(batch.records.len());
        seen.extend(batch.records.into_iter().map(|w| w.id));
        cursor = batch.upto;
    }
    assert_eq!(sizes, [2, 1, 6, 1]);
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 10);
    assert_eq!(seen.len(), 10);

    let first = p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, 2).unwrap();
    assert!(first.more);
    assert!(!p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, 10).unwrap().more);
    let empty = p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, 0).unwrap();
    assert!(empty.records.is_empty() && empty.more && empty.upto == Seq::ZERO);
}

#[test]
fn boundary_rejections_never_partially_apply() {
    let (mut p, _dir) = open(1);
    let a = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "A", &url("https://a.example/")).unwrap();
    let rec: BookmarkRecord = serde_json::from_slice(&all(&mut p, Kind::Bookmarks)[0].body).unwrap();
    let wire = |id: String, body: Vec<u8>| WireRecord { kind: Kind::Bookmarks, id, body };

    let garbage = wire(a.0.to_string(), b"{not json".to_vec());
    let mismatch = wire(BookmarkId(uuid::Uuid::from_u128(77)).0.to_string(), serde_json::to_vec(&rec).unwrap());
    let mut as_folder = serde_json::to_value(&rec).unwrap();
    as_folder["kind"] = serde_json::json!("folder");
    as_folder.as_object_mut().unwrap().remove("url");
    let kind_flip = wire(a.0.to_string(), serde_json::to_vec(&as_folder).unwrap());
    let mut root = serde_json::to_value(&rec).unwrap();
    root["id"] = serde_json::json!(BookmarkId::MOBILE.0.to_string());
    let root_rec = wire(BookmarkId::MOBILE.0.to_string(), serde_json::to_vec(&root).unwrap());
    let unknown_kind = WireRecord { kind: Kind::Prefs, id: "x".into(), body: b"[]".to_vec() };

    let report = p.sync().apply(vec![garbage, mismatch, kind_flip, root_rec, unknown_kind, wire(a.0.to_string(), serde_json::to_vec(&rec).unwrap())]).unwrap();
    assert_eq!(report.rejected.len(), 5, "{:?}", report.rejected);
    assert_eq!(report.unchanged, 1);
    assert_eq!(report.merged, 0);
    assert!(!report.changed.bookmarks);
    assert_eq!(p.bookmarks().get(a).unwrap().kind, NodeKind::Url);
    assert_eq!(all(&mut p, Kind::Bookmarks).len(), 1);
}

#[test]
fn engine_state_is_opaque_and_survives() {
    let (mut p, _dir) = open(1);
    assert_eq!(p.sync().engine_state("up/1").unwrap(), None);
    p.sync().set_engine_state("up/1", &[1, 2, 3]).unwrap();
    p.sync().set_engine_state("up/1", &[9]).unwrap();
    p.sync().set_engine_state("token", b"abc").unwrap();
    assert_eq!(p.sync().engine_state("up/1").unwrap(), Some(vec![9]));
    assert_eq!(p.sync().engine_state("token").unwrap(), Some(b"abc".to_vec()));
}

#[test]
fn extension_desired_state_merges_and_is_exported() {
    let (mut a, _da) = open(1);
    let (mut b, _db) = open(2);
    let ext = ExtensionId::parse("cccccccccccccccccccccccccccccccc").unwrap();
    let at = |h: u64, d: u64| Stamp { hlc: Hlc(h << 16), device: DeviceId(d) };
    let rec = |installed: (bool, Stamp), enabled: (bool, Stamp)| ExtensionRecord {
        id: ext.clone(),
        store: Lww::new(StoreRef::ChromeWebStore, at(1, 1)),
        installed: Lww::new(installed.0, installed.1),
        enabled: Lww::new(enabled.0, enabled.1),
        extra: Extra::default(),
    };
    let wire = |r: &ExtensionRecord| WireRecord { kind: Kind::Extensions, id: ext.as_str().to_owned(), body: serde_json::to_vec(r).unwrap() };

    let from_one = rec((true, at(2, 1)), (true, at(2, 1)));
    let from_two = rec((true, at(2, 1)), (false, at(3, 2)));
    let report = a.sync().apply(vec![wire(&from_one)]).unwrap();
    assert_eq!((report.merged, report.unchanged), (1, 0));
    assert!(report.changed.extensions);
    // a clean copy of the server's record is not re-uploaded
    assert!(all(&mut a, Kind::Extensions).is_empty() || a.sync().changes_since(Kind::Extensions, Seq(1), usize::MAX).unwrap().records.is_empty());
    assert_eq!(all(&mut a, Kind::Extensions).len(), 1, "but a full export includes it");

    let report = a.sync().apply(vec![wire(&from_two)]).unwrap();
    assert_eq!(report.merged, 1);
    let merged: ExtensionRecord = serde_json::from_slice(&all(&mut a, Kind::Extensions)[0].body).unwrap();
    assert_eq!(merged, rec((true, at(2, 1)), (false, at(3, 2))));

    // b holds more than a stale record on the server: merged != incoming marks it dirty
    b.sync().apply(vec![wire(&from_two)]).unwrap();
    let cursor = b.sync().changes_since(Kind::Extensions, Seq::ZERO, usize::MAX).unwrap().upto;
    let report = b.sync().apply(vec![wire(&from_one)]).unwrap();
    assert_eq!((report.merged, report.unchanged), (0, 1));
    let pending = b.sync().changes_since(Kind::Extensions, cursor, usize::MAX).unwrap().records;
    assert_eq!(pending.len(), 1, "the newer state is re-uploaded to repair the server");
    assert_eq!(serde_json::from_slice::<ExtensionRecord>(&pending[0].body).unwrap(), merged);

    // re-applying what we hold changes nothing and marks nothing
    let cursor = b.sync().changes_since(Kind::Extensions, Seq::ZERO, usize::MAX).unwrap().upto;
    let report = b.sync().apply(vec![wire(&merged), wire(&merged)]).unwrap();
    assert_eq!((report.merged, report.unchanged), (0, 2));
    assert!(b.sync().changes_since(Kind::Extensions, cursor, usize::MAX).unwrap().records.is_empty());
}
