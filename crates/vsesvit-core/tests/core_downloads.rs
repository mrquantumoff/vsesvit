//! The downloads list through the API: lifecycle, order, clearing, interrupted downloads,
//! the schema migration, and the download prefs.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::crdt::{DeviceId, Seq, TimeSource};
use vsesvit_core::downloads::{Download, State};
use vsesvit_core::prefs::{Scope, keys};
use vsesvit_core::sync::Kind;
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const T0: u64 = 1_780_000_000_000;

fn tmp() -> TempDir {
    TempDir(std::env::temp_dir().join(format!("vsesvit-dl-{}", uuid::Uuid::new_v4())))
}

fn open_at(dir: &Path) -> Profile {
    Profile::open(
        dir,
        OpenOptions { time: TimeSource::Manual(Rc::new(Cell::new(T0))), new_device_id: Some(DeviceId(3)), ..OpenOptions::default() },
    )
    .unwrap()
}

fn open() -> (Profile, TempDir) {
    let dir = tmp();
    (open_at(&dir.0), dir)
}

fn states(p: &mut Profile) -> Vec<(String, State)> {
    p.downloads().list(usize::MAX).unwrap().into_iter().map(|d| (d.url, d.state)).collect()
}

fn user_version(dir: &Path) -> u32 {
    let conn = rusqlite::Connection::open(dir.join("vsesvit.db")).unwrap();
    conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

#[test]
fn start_then_finish() {
    let (mut p, _dir) = open();
    let path = Path::new("/dl/a.zip");
    let started = p.downloads().start("https://example.com/a.zip", path, Some(10), T0).unwrap();
    assert_eq!((started.state, started.received, started.total, started.started_ms), (State::InProgress, 0, Some(10), T0));
    assert_eq!(p.downloads().list(10).unwrap(), vec![started.clone()], "start returns the stored row");

    p.downloads().finish(started.id, State::Completed, 12, Some(12)).unwrap();
    let done = p.downloads().list(10).unwrap().remove(0);
    assert_eq!(done, Download { state: State::Completed, received: 12, total: Some(12), ..started.clone() });
    assert_eq!(done.path, path);

    p.downloads().remove(started.id).unwrap();
    assert!(p.downloads().list(10).unwrap().is_empty());
    p.downloads().finish(started.id, State::Failed, 0, None).unwrap();
    assert!(p.downloads().list(10).unwrap().is_empty(), "finish after remove is a no-op");
    p.downloads().remove(started.id).unwrap();
}

#[test]
fn list_is_newest_first_and_limited() {
    let (mut p, _dir) = open();
    let path = Path::new("/dl/f");
    p.downloads().start("https://a.example/", path, None, T0).unwrap();
    p.downloads().start("https://c.example/", path, None, T0 + 2).unwrap();
    p.downloads().start("https://b1.example/", path, None, T0 + 1).unwrap();
    p.downloads().start("https://b2.example/", path, None, T0 + 1).unwrap();
    let urls: Vec<String> = states(&mut p).into_iter().map(|(url, _)| url).collect();
    assert_eq!(urls, ["https://c.example/", "https://b2.example/", "https://b1.example/", "https://a.example/"], "same start time: later id first");
    assert_eq!(p.downloads().list(2).unwrap().len(), 2);
    assert!(p.downloads().list(0).unwrap().is_empty());
}

#[test]
fn clear_keeps_downloads_in_progress() {
    let (mut p, _dir) = open();
    let path = Path::new("/dl/f");
    let mut dl = p.downloads();
    let running = dl.start("https://running.example/", path, None, T0 + 4).unwrap();
    for (i, state) in [State::Completed, State::Failed, State::Cancelled].into_iter().enumerate() {
        let d = dl.start(&format!("https://{i}.example/"), path, None, T0 + i as u64).unwrap();
        dl.finish(d.id, state, 1, None).unwrap();
    }
    dl.clear().unwrap();
    assert_eq!(dl.list(10).unwrap(), vec![running]);
}

#[test]
fn interrupted_downloads_read_as_failed_after_a_restart() {
    let dir = tmp();
    {
        let mut p = open_at(&dir.0);
        let mut dl = p.downloads();
        dl.start("https://running.example/", Path::new("/dl/r"), Some(100), T0 + 1).unwrap();
        let done = dl.start("https://done.example/", Path::new("/dl/d"), None, T0).unwrap();
        dl.finish(done.id, State::Completed, 5, Some(5)).unwrap();
    }
    let mut p = open_at(&dir.0);
    assert_eq!(p.downloads().interrupt_stale().unwrap(), 1);
    assert_eq!(p.downloads().interrupt_stale().unwrap(), 0, "idempotent");
    assert_eq!(
        states(&mut p),
        [("https://running.example/".to_owned(), State::Failed), ("https://done.example/".to_owned(), State::Completed)]
    );
    assert_eq!(p.downloads().list(1).unwrap()[0].received, 0, "an interrupted download keeps its last stored counts");
}

#[test]
fn downloads_are_local() {
    let (mut p, _dir) = open();
    let d = p.downloads().start("https://example.com/", Path::new("/dl/f"), None, T0).unwrap();
    p.downloads().finish(d.id, State::Completed, 1, None).unwrap();
    for &kind in Kind::ALL {
        assert!(p.sync().changes_since(kind, Seq::ZERO, usize::MAX).unwrap().records.is_empty(), "{kind:?}");
    }
}

#[test]
fn a_v1_profile_gains_the_table_and_keeps_its_data() {
    let dir = tmp();
    let bookmark = Url::parse("https://kept.example/").unwrap();
    {
        let mut p = open_at(&dir.0);
        p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Kept", &bookmark).unwrap();
    }
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    conn.execute_batch("DROP TABLE downloads; DROP TABLE favicons; DROP TABLE favicon_failures; DROP TABLE site_permissions; DROP TABLE site_zoom; PRAGMA user_version = 1;").unwrap();
    drop(conn);

    let mut p = open_at(&dir.0);
    assert!(p.bookmarks().is_bookmarked(&bookmark));
    let d = p.downloads().start("https://example.com/", Path::new("/dl/f"), None, T0).unwrap();
    drop(p);
    assert_eq!(user_version(&dir.0), 7);

    let mut p = open_at(&dir.0);
    assert_eq!(user_version(&dir.0), 7, "reopening migrates nothing");
    assert_eq!(p.downloads().list(10).unwrap(), vec![d]);
    assert!(p.bookmarks().is_bookmarked(&bookmark));
}

#[test]
fn download_prefs() {
    let (mut p, _dir) = open();
    assert_eq!((keys::DOWNLOADS_DIR.scope, keys::DOWNLOADS_ASK.scope), (Scope::Local, Scope::Synced));
    assert_eq!(p.prefs().get(&keys::DOWNLOADS_DIR), None, "None = the platform's Downloads folder");
    assert!(!p.prefs().get(&keys::DOWNLOADS_ASK));

    let custom = PathBuf::from("/home/me/Incoming");
    p.prefs().set(&keys::DOWNLOADS_DIR, &Some(custom.clone())).unwrap();
    p.prefs().set(&keys::DOWNLOADS_ASK, &true).unwrap();
    assert_eq!(p.prefs().get(&keys::DOWNLOADS_DIR), Some(custom));
    assert!(p.prefs().get(&keys::DOWNLOADS_ASK));

    let synced = p.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap();
    let synced_keys: Vec<&str> = synced.records.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(synced_keys, [keys::DOWNLOADS_ASK.key], "the folder is this device's, the ask switch syncs");

    p.prefs().reset(&keys::DOWNLOADS_DIR).unwrap();
    assert_eq!(p.prefs().get(&keys::DOWNLOADS_DIR), None);
}
