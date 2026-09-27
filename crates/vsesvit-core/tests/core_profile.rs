//! Profile lifecycle: the lock, persistence of identity and clock, schema guard.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, BookmarkRecord, InsertAt};
use vsesvit_core::crdt::{DeviceId, Seq, TimeSource};
use vsesvit_core::sync::Kind;
use vsesvit_core::{OpenError, OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tmp() -> TempDir {
    TempDir(std::env::temp_dir().join(format!("vsesvit-prof-{}", uuid::Uuid::new_v4())))
}

fn opts(time: u64, device: Option<u64>) -> OpenOptions {
    OpenOptions { time: TimeSource::Manual(Rc::new(Cell::new(time))), new_device_id: device.map(DeviceId), ..OpenOptions::default() }
}

#[test]
fn second_open_is_locked_until_the_first_closes() {
    let dir = tmp();
    let first = Profile::open(&dir.0, opts(1, Some(1))).unwrap();
    assert!(matches!(Profile::open(&dir.0, opts(1, Some(1))), Err(OpenError::Locked)));
    drop(first);
    Profile::open(&dir.0, opts(1, Some(1))).unwrap();
}

#[test]
fn paths_and_layout() {
    let dir = tmp();
    let p = Profile::open(&dir.0, opts(1, None)).unwrap();
    let paths = p.paths();
    assert_eq!(paths.root, dir.0);
    assert_eq!(paths.db, dir.0.join("vsesvit.db"));
    assert!(paths.extensions.is_dir() && paths.staging.is_dir() && paths.engine_data.is_dir());
    assert!(dir.0.join("LOCK").is_file());
    assert_eq!(paths.engine_cache, dir.0.join("cache"), "a custom root keeps its cache beside it");
    assert_ne!(p.device_id().0, 0);
    let default = Profile::default_root("Default");
    assert!(default.ends_with(std::path::Path::new("profiles").join("Default")));
    assert!(default.to_string_lossy().to_lowercase().contains("vsesvit"));
}

#[test]
fn identity_and_clock_persist_across_reopen() {
    let dir = tmp();
    let (device, first_stamp) = {
        let mut p = Profile::open(&dir.0, opts(5_000_000, None)).unwrap();
        p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "A", &Url::parse("https://a.example/").unwrap()).unwrap();
        let rec: BookmarkRecord = serde_json::from_slice(&p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, 1).unwrap().records[0].body).unwrap();
        (p.device_id(), rec.placement.at)
    };
    assert_eq!(first_stamp.device, device);
    // Reopened with a wall clock far in the past: the persisted HLC still wins.
    let mut p = Profile::open(&dir.0, opts(10, Some(999))).unwrap();
    assert_eq!(p.device_id(), device, "new_device_id is ignored for an existing profile");
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "B", &Url::parse("https://b.example/").unwrap()).unwrap();
    let batch = p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, usize::MAX).unwrap();
    let stamps: Vec<_> = batch.records.iter().map(|w| serde_json::from_slice::<BookmarkRecord>(&w.body).unwrap().placement.at).collect();
    assert!(stamps[1] > stamps[0]);
    assert_eq!(stamps[1].device, device);
    assert!(batch.upto > Seq(2), "the seq counter continued");
}

#[test]
fn a_newer_schema_is_refused_untouched() {
    let dir = tmp();
    drop(Profile::open(&dir.0, opts(1, Some(1))).unwrap());
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert!(matches!(Profile::open(&dir.0, opts(1, Some(1))), Err(OpenError::TooNew { found: 99, supported: 1 })));
}
