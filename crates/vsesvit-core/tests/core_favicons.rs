//! Favicons of bookmarked pages and sites: what is kept, the site fallback, pruning at open,
//! and the upgrade of a v1 profile.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::crdt::TimeSource;
use vsesvit_core::favicons::MAX_BYTES;
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open() -> (Profile, TempDir) {
    let (p, dir, _) = open_with_clock();
    (p, dir)
}

fn open_with_clock() -> (Profile, TempDir, Rc<Cell<u64>>) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-favicons-{}", uuid::Uuid::new_v4())));
    let clock = Rc::new(Cell::new(1_780_000_000_000));
    let options = OpenOptions { time: TimeSource::Manual(clock.clone()), ..OpenOptions::default() };
    let p = Profile::open(&dir.0, options).unwrap();
    (p, dir, clock)
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

#[test]
fn only_bookmarked_pages_and_sites_keep_their_icon() {
    let (mut p, _dir, clock) = open_with_clock();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Docs", &url("https://docs.example/a")).unwrap();

    assert!(p.favicons().record(&url("https://docs.example/a"), b"page-a").unwrap());
    clock.set(clock.get() + 1);
    assert!(p.favicons().record(&url("https://docs.example/other"), b"site").unwrap(), "same site");
    assert!(!p.favicons().record(&url("https://elsewhere.example/"), b"x").unwrap(), "not bookmarked");
    assert!(!p.favicons().record(&url("https://docs.example/big"), &vec![0; MAX_BYTES + 1]).unwrap());
    assert!(!p.favicons().record(&url("https://docs.example/empty"), b"").unwrap());

    assert_eq!(p.favicons().get(&url("https://docs.example/a")).unwrap().as_deref(), Some(&b"page-a"[..]));
    assert_eq!(
        p.favicons().get(&url("https://docs.example/never-visited")).unwrap().as_deref(),
        Some(&b"site"[..]),
        "the site's latest icon stands in"
    );
    assert_eq!(p.favicons().get(&url("https://elsewhere.example/")).unwrap(), None);
    assert_eq!(p.favicons().get(&url("http://docs.example/a")).unwrap(), None, "another scheme is another site");

    assert!(!p.favicons().record(&url("https://docs.example/a"), b"page-a").unwrap(), "unchanged");
    assert!(p.favicons().record(&url("https://docs.example/a"), b"page-a2").unwrap());
    assert_eq!(p.favicons().get(&url("https://docs.example/a")).unwrap().as_deref(), Some(&b"page-a2"[..]));
}

#[test]
fn file_pages_never_share_an_icon() {
    let (mut p, _dir) = open();
    let page = url("file:///C:/notes/a.html");
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "a", &page).unwrap();
    assert!(p.favicons().record(&page, b"a").unwrap());
    assert!(!p.favicons().record(&url("file:///C:/notes/b.html"), b"b").unwrap());
    assert_eq!(p.favicons().get(&url("file:///C:/notes/b.html")).unwrap(), None);
}

#[test]
fn icons_of_sites_without_bookmarks_are_dropped_at_open() {
    let (mut p, dir) = open();
    let kept = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "k", &url("https://kept.example/")).unwrap();
    let gone = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "g", &url("https://gone.example/")).unwrap();
    p.favicons().record(&url("https://kept.example/"), b"k").unwrap();
    p.favicons().record(&url("https://gone.example/"), b"g").unwrap();
    p.bookmarks().remove(gone).unwrap();
    drop(p);

    let mut p = Profile::open(&dir.0, OpenOptions::default()).unwrap();
    assert!(p.bookmarks().get(kept).is_some());
    assert!(p.favicons().get(&url("https://kept.example/")).unwrap().is_some());
    assert_eq!(p.favicons().get(&url("https://gone.example/")).unwrap(), None);
}

#[test]
fn a_v1_profile_gains_the_favicon_table() {
    let (p, dir) = open();
    drop(p);
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    conn.execute_batch("DROP TABLE favicons; DROP TABLE downloads; PRAGMA user_version = 1;").unwrap();
    drop(conn);

    let mut p = Profile::open(&dir.0, OpenOptions::default()).unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "a", &url("https://a.example/")).unwrap();
    assert!(p.favicons().record(&url("https://a.example/"), b"a").unwrap());
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 4);
}
