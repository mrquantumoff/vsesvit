//! Favicons of bookmarked pages and sites: what is kept, the site fallback, pruning at open,
//! fetching icons of bookmarks never visited, and the upgrade of a v1 profile.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::crdt::TimeSource;
use vsesvit_core::favicons::{FaviconFetch, Fetched, MAX_BYTES, Outcome, RETRY_AFTER_MS, UNREACHABLE_RETRY_MS};
use vsesvit_core::testkit::FixtureServer;
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
fn visiting_more_of_a_bookmarked_site_keeps_one_site_icon() {
    let (mut p, dir, clock) = open_with_clock();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "a", &url("https://docs.example/a")).unwrap();
    p.favicons().record(&url("https://docs.example/a"), b"page").unwrap();
    for i in 0..50 {
        clock.set(clock.get() + 1);
        p.favicons().record(&url(&format!("https://docs.example/p{i}?q={i}")), b"site").unwrap();
    }
    assert!(!p.favicons().record(&url("https://docs.example/p50"), b"site").unwrap(), "the site's icon is unchanged");
    assert_eq!(icon_rows(&dir), 2, "the bookmarked page's icon and one for the site");
    assert_eq!(p.favicons().get(&url("https://docs.example/a")).unwrap().as_deref(), Some(&b"page"[..]));
    assert_eq!(p.favicons().get(&url("https://docs.example/never")).unwrap().as_deref(), Some(&b"site"[..]));
}

#[test]
fn icons_kept_per_page_visited_collapse_to_one_per_site_at_open() {
    let (mut p, dir) = open();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "a", &url("https://docs.example/a")).unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "b", &url("https://blog.example/b")).unwrap();
    p.favicons().record(&url("https://blog.example/"), b"blog").unwrap();
    drop(p);
    // How profiles stored the icons of other pages of a bookmarked site before.
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    for (page, png, at) in [
        ("https://docs.example/a", "page", 1),
        ("https://docs.example/x", "old", 2),
        ("https://docs.example/y", "new", 3),
        ("https://docs.example/z", "older", 0),
        ("https://blog.example/old", "blog-old", 0),
    ] {
        conn.execute(
            "INSERT OR REPLACE INTO favicons (page_url, origin, png, updated_ms) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![page, Url::parse(page).unwrap().origin().ascii_serialization(), png.as_bytes(), at],
        )
        .unwrap();
    }
    drop(conn);

    let mut p = Profile::open(&dir.0, OpenOptions::default()).unwrap();
    assert_eq!(icon_rows(&dir), 3, "the bookmarked page's icon and one per site");
    assert_eq!(p.favicons().get(&url("https://docs.example/a")).unwrap().as_deref(), Some(&b"page"[..]));
    assert_eq!(p.favicons().get(&url("https://docs.example/x")).unwrap().as_deref(), Some(&b"new"[..]), "the newest");
    assert_eq!(p.favicons().get(&url("https://blog.example/old")).unwrap().as_deref(), Some(&b"blog"[..]));
}

fn icon_rows(dir: &TempDir) -> i64 {
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    conn.query_row("SELECT COUNT(*) FROM favicons", [], |r| r.get(0)).unwrap()
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
fn missing_lists_one_page_per_site_without_an_icon_bar_first() {
    let (mut p, _dir) = open();
    let mut add = |parent, u: &str| p.bookmarks().add_url(parent, InsertAt::End, "t", &url(u)).unwrap();
    add(BookmarkId::TOOLBAR, "https://a.example/x");
    add(BookmarkId::TOOLBAR, "https://a.example/y");
    add(BookmarkId::TOOLBAR, "https://has-icon.example/");
    add(BookmarkId::TOOLBAR, "file:///C:/notes/a.html");
    add(BookmarkId::OTHER, "https://other.example/");
    let folder = p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "Folder").unwrap();
    p.bookmarks().add_url(folder, InsertAt::End, "c", &url("https://nested.example/")).unwrap();
    p.favicons().record(&url("https://has-icon.example/other-page"), b"icon").unwrap();

    let all = ["https://a.example/x", "https://other.example/", "https://nested.example/"].map(url);
    assert_eq!(p.favicons().missing(10).unwrap(), all);
    assert_eq!(p.favicons().missing(2).unwrap(), all[..2]);
    assert_eq!(p.favicons().missing(0).unwrap(), []);
}

#[test]
fn failed_sites_are_retried_after_a_week() {
    let (mut p, _dir, clock) = open_with_clock();
    for u in ["https://ok.example/", "https://down.example/"] {
        p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "t", &url(u)).unwrap();
    }
    let fetched = vec![
        Fetched { page: url("https://ok.example/"), outcome: Outcome::Icon(b"png".to_vec()) },
        Fetched { page: url("https://down.example/"), outcome: Outcome::NoIcon },
        Fetched { page: url("https://unbookmarked.example/"), outcome: Outcome::Icon(b"png".to_vec()) },
    ];
    assert!(p.favicons().commit_fetched(fetched.clone()).unwrap());
    assert!(!p.favicons().commit_fetched(fetched[..1].to_vec()).unwrap(), "the same icon again");
    assert_eq!(p.favicons().get(&url("https://ok.example/")).unwrap().as_deref(), Some(&b"png"[..]));
    assert_eq!(p.favicons().get(&url("https://unbookmarked.example/")).unwrap(), None);
    assert_eq!(p.favicons().missing(10).unwrap(), []);

    clock.set(clock.get() + RETRY_AFTER_MS as u64 - 1);
    assert_eq!(p.favicons().missing(10).unwrap(), []);
    clock.set(clock.get() + 1);
    assert_eq!(p.favicons().missing(10).unwrap(), [url("https://down.example/")]);

    assert!(!p.favicons().commit_fetched(vec![Fetched { page: url("https://down.example/"), outcome: Outcome::NoIcon }]).unwrap());
    assert_eq!(p.favicons().missing(10).unwrap(), [], "failed again: another week");
}

#[test]
fn unreachable_sites_are_retried_after_an_hour() {
    let (mut p, _dir, clock) = open_with_clock();
    for u in ["https://offline.example/", "https://noicon.example/"] {
        p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "t", &url(u)).unwrap();
    }
    let fetched = vec![
        Fetched { page: url("https://offline.example/"), outcome: Outcome::Unreachable },
        Fetched { page: url("https://noicon.example/"), outcome: Outcome::NoIcon },
    ];
    assert!(!p.favicons().commit_fetched(fetched).unwrap());
    assert_eq!(p.favicons().missing(10).unwrap(), [], "not asked again at once");

    clock.set(clock.get() + UNREACHABLE_RETRY_MS as u64);
    assert_eq!(p.favicons().missing(10).unwrap(), [url("https://offline.example/")]);
}

#[test]
fn fetches_the_declared_icon_or_else_favicon_ico() {
    let server = FixtureServer::start().unwrap();
    let pages = vec![server.url("/icon.html"), server.url("/page2.html"), url("ftp://files.example/")];

    // Loopback and private-network hosts are never asked: the fixture server only answers
    // once the testkit switch for fixture servers is on. Process-wide, so one test does both.
    let refused = FaviconFetch::new(pages.clone()).run();
    assert_eq!(refused[0].outcome, Outcome::NoIcon, "refused for good, not unreachable");
    assert_eq!(server.hits(), Vec::<String>::new(), "nothing reached the loopback server");

    vsesvit_core::favicons::allow_local_hosts();
    let fetched = FaviconFetch::new(pages.clone()).run();

    assert_eq!(fetched.iter().map(|f| f.page.clone()).collect::<Vec<_>>(), pages);
    let Outcome::Icon(png) = &fetched[0].outcome else { panic!("icon.html declares /allowed.png") };
    assert!(png.starts_with(b"\x89PNG") && png.len() <= MAX_BYTES);
    assert_eq!(fetched[1].outcome, Outcome::NoIcon, "page2.html declares none and there is no /favicon.ico");
    assert_eq!(fetched[2].outcome, Outcome::NoIcon, "only http and https");
    let hits = server.hits();
    assert!(hits.contains(&"/allowed.png".to_owned()), "{hits:?}");
    assert!(hits.contains(&"/favicon.ico".to_owned()), "{hits:?}");

    // A page that is not found is an answer; a refused connection or an unknown host is not.
    let pages = vec![server.url("/missing.html"), url("http://127.0.0.1:1/"), url("http://unknown-host.invalid/")];
    let outcomes: Vec<Outcome> = FaviconFetch::new(pages).run().into_iter().map(|f| f.outcome).collect();
    assert_eq!(outcomes, [Outcome::NoIcon, Outcome::Unreachable, Outcome::Unreachable]);
}

#[test]
fn a_v1_profile_gains_the_favicon_table() {
    let (p, dir) = open();
    drop(p);
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    conn.execute_batch("DROP TABLE favicons; DROP TABLE favicon_failures; DROP TABLE downloads; DROP TABLE site_permissions; DROP TABLE site_zoom; DROP TABLE vault_key; DROP TABLE sync_secrets; ALTER TABLE extension_installs DROP COLUMN granted; PRAGMA user_version = 1;").unwrap();
    drop(conn);

    let mut p = Profile::open(&dir.0, OpenOptions::default()).unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "a", &url("https://a.example/")).unwrap();
    assert!(p.favicons().record(&url("https://a.example/"), b"a").unwrap());
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 11);
}
