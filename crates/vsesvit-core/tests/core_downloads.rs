//! The downloads list through the API: lifecycle, order, clearing, interrupted downloads,
//! the schema migration, and the download prefs.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::crdt::{DeviceId, Seq, TimeSource};
use vsesvit_core::downloads::{Download, State, unconfirmed_path};
use vsesvit_core::prefs::{Scope, keys};
use vsesvit_core::private::Browsing;
use vsesvit_core::sync::Kind;
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const T0: u64 = 1_780_000_000_000;

const STATES: [State; 7] =
    [State::InProgress, State::Paused, State::Interrupted, State::Unconfirmed, State::Completed, State::Failed, State::Cancelled];

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
    p.downloads().list(usize::MAX, Browsing::Normal).unwrap().into_iter().map(|d| (d.url, d.state)).collect()
}

fn user_version(dir: &Path) -> u32 {
    let conn = rusqlite::Connection::open(dir.join("vsesvit.db")).unwrap();
    conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

#[test]
fn start_then_finish() {
    let (mut p, _dir) = open();
    let path = Path::new("/dl/a.zip");
    let started = p.downloads().start("https://example.com/a.zip", path, Some(10), T0, Browsing::Normal).unwrap();
    assert_eq!((started.state, started.received, started.total, started.started_ms), (State::InProgress, 0, Some(10), T0));
    assert_eq!(p.downloads().list(10, Browsing::Normal).unwrap(), vec![started.clone()], "start returns the stored row");

    p.downloads().update(started.id, State::Completed, 12, Some(12)).unwrap();
    let done = p.downloads().list(10, Browsing::Normal).unwrap().remove(0);
    assert_eq!(done, Download { state: State::Completed, received: 12, total: Some(12), ..started.clone() });
    assert_eq!(done.path, path);

    p.downloads().remove(started.id).unwrap();
    assert!(p.downloads().list(10, Browsing::Normal).unwrap().is_empty());
    p.downloads().update(started.id, State::Failed, 0, None).unwrap();
    assert!(p.downloads().list(10, Browsing::Normal).unwrap().is_empty(), "finish after remove is a no-op");
    p.downloads().remove(started.id).unwrap();
}

#[test]
fn list_is_newest_first_and_limited() {
    let (mut p, _dir) = open();
    let path = Path::new("/dl/f");
    p.downloads().start("https://a.example/", path, None, T0, Browsing::Normal).unwrap();
    p.downloads().start("https://c.example/", path, None, T0 + 2, Browsing::Normal).unwrap();
    p.downloads().start("https://b1.example/", path, None, T0 + 1, Browsing::Normal).unwrap();
    p.downloads().start("https://b2.example/", path, None, T0 + 1, Browsing::Normal).unwrap();
    let urls: Vec<String> = states(&mut p).into_iter().map(|(url, _)| url).collect();
    assert_eq!(urls, ["https://c.example/", "https://b2.example/", "https://b1.example/", "https://a.example/"], "same start time: later id first");
    assert_eq!(p.downloads().list(2, Browsing::Normal).unwrap().len(), 2);
    assert!(p.downloads().list(0, Browsing::Normal).unwrap().is_empty());
}

#[test]
fn clear_keeps_what_is_not_over() {
    let (mut p, _dir) = open();
    let path = Path::new("/dl/f");
    let mut dl = p.downloads();
    let mut kept = Vec::new();
    for (i, state) in STATES.into_iter().enumerate() {
        let mut d = dl.start(&format!("https://{i}.example/"), path, None, T0 + i as u64, Browsing::Normal).unwrap();
        dl.update(d.id, state, 1, None).unwrap();
        (d.state, d.received) = (state, 1);
        if !state.is_final() {
            kept.insert(0, d);
        }
    }
    dl.clear(Browsing::Normal).unwrap();
    assert_eq!(dl.list(10, Browsing::Normal).unwrap(), kept);
    assert_eq!(kept.iter().map(|d| d.state).collect::<Vec<_>>(), [State::Unconfirmed, State::Interrupted, State::Paused, State::InProgress]);
}

#[test]
fn downloads_the_engine_held_read_as_failed_after_a_restart() {
    let dir = tmp();
    {
        let mut p = open_at(&dir.0);
        let mut dl = p.downloads();
        for (i, state) in STATES.into_iter().enumerate() {
            let d = dl.start(&format!("https://{i}.example/"), Path::new("/dl/f"), Some(100), T0 + i as u64, Browsing::Normal).unwrap();
            dl.update(d.id, state, 7, Some(100)).unwrap();
        }
    }
    let mut p = open_at(&dir.0);
    assert_eq!(p.downloads().interrupt_stale().unwrap(), 3);
    assert_eq!(p.downloads().interrupt_stale().unwrap(), 0, "idempotent");
    let after: Vec<State> = states(&mut p).into_iter().rev().map(|(_, state)| state).collect();
    use State::*;
    assert_eq!(after, [Failed, Failed, Failed, Unconfirmed, Completed, Failed, Cancelled], "an unconfirmed file still waits");
    assert!(p.downloads().list(10, Browsing::Normal).unwrap().iter().all(|d| d.received == 7), "the last stored counts stay");
}

#[test]
fn a_kept_file_takes_its_name_and_a_discarded_one_is_gone() {
    let (mut p, dir) = open();
    let folder = dir.0.join("Downloads");
    std::fs::create_dir_all(&folder).unwrap();
    let mut dl = p.downloads();
    let start = |dl: &mut vsesvit_core::downloads::Downloads<'_>, name: &str| {
        let path = folder.join(name);
        std::fs::write(unconfirmed_path(&path), name).unwrap();
        let mut d = dl.start("https://example.com/", &path, None, T0, Browsing::Normal).unwrap();
        dl.update(d.id, State::Unconfirmed, 9, Some(9)).unwrap();
        (d.state, d.received, d.total) = (State::Unconfirmed, 9, Some(9));
        d
    };

    let setup = start(&mut dl, "setup.exe");
    assert_eq!(dl.keep(setup.id).unwrap(), Some(folder.join("setup.exe")));
    assert_eq!(std::fs::read_to_string(folder.join("setup.exe")).unwrap(), "setup.exe");
    assert!(!unconfirmed_path(&setup.path).exists());
    let listed = dl.list(10, Browsing::Normal).unwrap();
    assert_eq!(listed, [Download { state: State::Completed, ..setup.clone() }]);

    assert_eq!(dl.keep(setup.id).unwrap(), None, "kept already");
    dl.discard(setup.id).unwrap();
    assert!(folder.join("setup.exe").exists(), "a stale Discard leaves a kept file alone");

    let again = start(&mut dl, "setup.exe");
    assert_eq!(dl.keep(again.id).unwrap(), Some(folder.join("setup (1).exe")), "a file took the name meanwhile");
    assert_eq!(dl.list(1, Browsing::Normal).unwrap()[0].path, folder.join("setup (1).exe"));
    assert_eq!(std::fs::read_to_string(folder.join("setup.exe")).unwrap(), "setup.exe", "never overwritten");

    let script = start(&mut dl, "run.sh");
    dl.discard(script.id).unwrap();
    assert!(!unconfirmed_path(&script.path).exists() && !script.path.exists());
    assert!(dl.list(10, Browsing::Normal).unwrap().iter().all(|d| d.id != script.id), "off the list");

    let gone = start(&mut dl, "gone.bat");
    std::fs::remove_file(unconfirmed_path(&gone.path)).unwrap();
    dl.discard(gone.id).unwrap();
    assert!(dl.list(10, Browsing::Normal).unwrap().iter().all(|d| d.id != gone.id), "discarding a file deleted meanwhile still clears the entry");

    let next = start(&mut dl, "next.bat");
    assert!(next.id.0 > gone.id.0, "the id of a removed entry is never given out again");
    assert_eq!(dl.keep(gone.id).unwrap(), None, "a stale Keep finds nothing");
    assert!(unconfirmed_path(&next.path).exists());
}

#[test]
fn downloads_are_local() {
    let (mut p, _dir) = open();
    let d = p.downloads().start("https://example.com/", Path::new("/dl/f"), None, T0, Browsing::Normal).unwrap();
    p.downloads().update(d.id, State::Completed, 1, None).unwrap();
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
    conn.execute_batch("DROP TABLE downloads; DROP TABLE favicons; DROP TABLE favicon_failures; DROP TABLE site_permissions; DROP TABLE site_zoom; DROP TABLE vault_key; DROP TABLE sync_secrets; ALTER TABLE extension_installs DROP COLUMN granted; PRAGMA user_version = 1;").unwrap();
    drop(conn);

    let mut p = open_at(&dir.0);
    assert!(p.bookmarks().is_bookmarked(&bookmark));
    let d = p.downloads().start("https://example.com/", Path::new("/dl/f"), None, T0, Browsing::Normal).unwrap();
    drop(p);
    assert_eq!(user_version(&dir.0), 11);

    let mut p = open_at(&dir.0);
    assert_eq!(user_version(&dir.0), 11, "reopening migrates nothing");
    assert_eq!(p.downloads().list(10, Browsing::Normal).unwrap(), vec![d]);
    assert!(p.bookmarks().is_bookmarked(&bookmark));
}

#[test]
fn a_v9_profile_takes_the_new_states_and_keeps_its_rows() {
    let dir = tmp();
    let d = {
        let mut p = open_at(&dir.0);
        let d = p.downloads().start("https://example.com/a.zip", Path::new("/dl/a.zip"), Some(3), T0, Browsing::Normal).unwrap();
        p.downloads().update(d.id, State::Completed, 3, Some(3)).unwrap();
        Download { state: State::Completed, received: 3, ..d }
    };
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE v9 (id INTEGER PRIMARY KEY, url TEXT NOT NULL, path TEXT NOT NULL, started_ms INTEGER NOT NULL,            state TEXT NOT NULL CHECK (state IN ('in_progress','completed','failed','cancelled')),            received INTEGER NOT NULL DEFAULT 0, total INTEGER);
         INSERT INTO v9 SELECT * FROM downloads;
         DROP TABLE downloads;
         ALTER TABLE v9 RENAME TO downloads;
         ALTER TABLE extension_installs DROP COLUMN granted;
         PRAGMA user_version = 9;",
    )
    .unwrap();
    assert!(conn.execute("UPDATE downloads SET state = 'paused'", []).is_err(), "the v9 table refuses it");
    drop(conn);

    let mut p = open_at(&dir.0);
    assert_eq!(p.downloads().list(10, Browsing::Normal).unwrap(), vec![d.clone()]);
    let paused = p.downloads().start("https://example.com/b.zip", Path::new("/dl/b.zip"), None, T0 + 1, Browsing::Normal).unwrap();
    p.downloads().update(paused.id, State::Paused, 1, None).unwrap();
    drop(p);
    assert_eq!(user_version(&dir.0), 11);
    assert_eq!(states(&mut open_at(&dir.0)), [("https://example.com/b.zip".to_owned(), State::Paused), (d.url, State::Completed)]);
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
