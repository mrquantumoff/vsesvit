//! Bookmark API behaviour through a real profile: ordering, moves, cycle refusals,
//! subtree removal, import, persistence across reopen.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkError, BookmarkId, BookmarkRecord, ImportItem, InsertAt, NodeKind};
use vsesvit_core::crdt::{DeviceId, Seq, TimeSource};
use vsesvit_core::sync::{Kind, WireRecord};
use vsesvit_core::{Error, OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn options(device: u64) -> OpenOptions {
    OpenOptions {
        time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000 + device))),
        new_device_id: Some(DeviceId(device)),
        ..OpenOptions::default()
    }
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-bm-{}", uuid::Uuid::new_v4())));
    let p = Profile::open(&dir.0, options(1)).unwrap();
    (p, dir)
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn titles(p: &mut Profile, folder: BookmarkId) -> Vec<String> {
    p.bookmarks().children(folder).into_iter().map(|n| n.title).collect()
}

fn export(p: &mut Profile) -> Vec<WireRecord> {
    p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, usize::MAX).unwrap().records
}

fn records(p: &mut Profile) -> Vec<BookmarkRecord> {
    export(p).iter().map(|w| serde_json::from_slice(&w.body).unwrap()).collect()
}

#[test]
fn roots_and_empty_profile() {
    let (mut p, _dir) = open();
    let roots = p.bookmarks().children(BookmarkId::ROOT);
    assert_eq!(roots.iter().map(|n| n.id).collect::<Vec<_>>(), vec![BookmarkId::TOOLBAR, BookmarkId::OTHER, BookmarkId::MOBILE]);
    assert_eq!(roots[0].title, "Bookmarks bar");
    assert!(p.bookmarks().children(BookmarkId::TOOLBAR).is_empty());
    assert!(p.bookmarks().get(BookmarkId(uuid::Uuid::from_u128(99))).is_none());
    assert!(export(&mut p).is_empty());
}

#[test]
fn add_orders_and_star_state() {
    let (mut p, _dir) = open();
    let a = url("https://a.example/");
    assert!(!p.bookmarks().is_bookmarked(&a));
    let ida = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "A", &a).unwrap();
    let idb = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "B", &url("https://b.example/")).unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::Start, "S", &url("https://s.example/")).unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::Index(2), "M", &url("https://m.example/")).unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::Index(99), "Z", &url("https://z.example/")).unwrap();
    assert_eq!(titles(&mut p, BookmarkId::TOOLBAR), ["S", "A", "M", "B", "Z"]);
    assert!(p.bookmarks().is_bookmarked(&a));
    let found = p.bookmarks().find_by_url(&a);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, ida);
    assert_eq!(found[0].parent, BookmarkId::TOOLBAR);
    assert_eq!(found[0].index, 1);
    assert_eq!(p.bookmarks().get(idb).unwrap().index, 3);
    let sep = p.bookmarks().add_separator(BookmarkId::OTHER, InsertAt::End).unwrap();
    let node = p.bookmarks().get(sep).unwrap();
    assert_eq!((node.kind, node.title.as_str(), node.url), (NodeKind::Separator, "", None));
}

#[test]
fn invalid_targets_are_refused() {
    let (mut p, _dir) = open();
    let leaf = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "A", &url("https://a.example/")).unwrap();
    assert!(matches!(
        p.bookmarks().add_folder(leaf, InsertAt::End, "x"),
        Err(Error::Bookmark(BookmarkError::NotAFolder(id))) if id == leaf
    ));
    assert!(matches!(p.bookmarks().add_folder(BookmarkId::ROOT, InsertAt::End, "x"), Err(Error::Bookmark(BookmarkError::NotAFolder(_)))));
    assert!(matches!(p.bookmarks().rename(BookmarkId::TOOLBAR, "x"), Err(Error::Bookmark(BookmarkError::IsRoot))));
    assert!(matches!(p.bookmarks().remove(BookmarkId::OTHER), Err(Error::Bookmark(BookmarkError::IsRoot))));
    assert!(matches!(p.bookmarks().set_url(BookmarkId::TOOLBAR, &url("https://x/")), Err(Error::Bookmark(BookmarkError::IsRoot))));
    let folder = p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "F").unwrap();
    assert!(matches!(p.bookmarks().set_url(folder, &url("https://x/")), Err(Error::Bookmark(BookmarkError::NotAUrl))));
    p.bookmarks().remove(leaf).unwrap();
    assert!(matches!(p.bookmarks().rename(leaf, "x"), Err(Error::Bookmark(BookmarkError::NotFound(_)))));
}

#[test]
fn unchanged_edits_mint_nothing() {
    let (mut p, _dir) = open();
    let id = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "A", &url("https://a.example/")).unwrap();
    let before = p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, usize::MAX).unwrap().upto;
    p.bookmarks().rename(id, "A").unwrap();
    p.bookmarks().set_url(id, &url("https://a.example/")).unwrap();
    assert_eq!(p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, usize::MAX).unwrap().upto, before);
    p.bookmarks().rename(id, "B").unwrap();
    p.bookmarks().set_url(id, &url("https://b.example/")).unwrap();
    assert!(p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, usize::MAX).unwrap().upto > before);
    let node = p.bookmarks().get(id).unwrap();
    assert_eq!((node.title.as_str(), node.url.unwrap().as_str()), ("B", "https://b.example/"));
    assert!(!p.bookmarks().is_bookmarked(&url("https://a.example/")));
}

#[test]
fn move_between_folders_and_reorder() {
    let (mut p, _dir) = open();
    let f = p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "F").unwrap();
    let a = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "A", &url("https://a.example/")).unwrap();
    let b = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "B", &url("https://b.example/")).unwrap();
    let c = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "C", &url("https://c.example/")).unwrap();
    assert_eq!(titles(&mut p, BookmarkId::TOOLBAR), ["F", "A", "B", "C"]);
    p.bookmarks().move_to(c, BookmarkId::TOOLBAR, InsertAt::Index(0)).unwrap();
    assert_eq!(titles(&mut p, BookmarkId::TOOLBAR), ["C", "F", "A", "B"]);
    p.bookmarks().move_to(a, f, InsertAt::End).unwrap();
    p.bookmarks().move_to(b, f, InsertAt::Start).unwrap();
    assert_eq!(titles(&mut p, BookmarkId::TOOLBAR), ["C", "F"]);
    assert_eq!(titles(&mut p, f), ["B", "A"]);
    assert_eq!(p.bookmarks().get(a).unwrap().parent, f);
    // moving a node onto its own slot changes nothing
    let upto = p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, usize::MAX).unwrap().upto;
    p.bookmarks().move_to(a, f, InsertAt::Index(1)).unwrap();
    p.bookmarks().move_to(a, f, InsertAt::Index(99)).unwrap();
    p.bookmarks().move_to(a, f, InsertAt::End).unwrap();
    p.bookmarks().move_to(b, f, InsertAt::Start).unwrap();
    p.bookmarks().move_to(b, f, InsertAt::Index(0)).unwrap();
    assert_eq!(titles(&mut p, f), ["B", "A"]);
    assert_eq!(p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, usize::MAX).unwrap().upto, upto, "nothing to upload");
    // cycles
    let g = p.bookmarks().add_folder(f, InsertAt::End, "G").unwrap();
    assert!(matches!(p.bookmarks().move_to(f, g, InsertAt::End), Err(Error::Bookmark(BookmarkError::WouldCycle))));
    assert!(matches!(p.bookmarks().move_to(f, f, InsertAt::End), Err(Error::Bookmark(BookmarkError::WouldCycle))));
    assert!(matches!(p.bookmarks().move_to(BookmarkId::TOOLBAR, f, InsertAt::End), Err(Error::Bookmark(BookmarkError::IsRoot))));
    p.bookmarks().move_to(g, BookmarkId::MOBILE, InsertAt::End).unwrap();
    p.bookmarks().move_to(f, g, InsertAt::End).unwrap();
    assert_eq!(p.bookmarks().get(f).unwrap().parent, g);
}

#[test]
fn remove_tombstones_the_effective_subtree() {
    let (mut p, _dir) = open();
    let f = p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "F").unwrap();
    let g = p.bookmarks().add_folder(f, InsertAt::End, "G").unwrap();
    let a = p.bookmarks().add_url(g, InsertAt::End, "A", &url("https://a.example/")).unwrap();
    let keep = p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "K", &url("https://k.example/")).unwrap();
    p.bookmarks().remove(f).unwrap();
    assert_eq!(titles(&mut p, BookmarkId::TOOLBAR), ["K"]);
    assert!(p.bookmarks().get(a).is_none());
    assert!(!p.bookmarks().is_bookmarked(&url("https://a.example/")));
    let recs = records(&mut p);
    let dead: Vec<_> = recs.iter().filter(|r| r.state.is_deleted()).map(|r| r.id).collect();
    assert_eq!(dead.len(), 3);
    assert!(dead.contains(&f) && dead.contains(&g) && dead.contains(&a));
    assert!(recs.iter().any(|r| r.id == keep && !r.state.is_deleted()));
    // tombstones keep their placement and kind
    let g_rec = recs.iter().find(|r| r.id == g).unwrap();
    assert_eq!(g_rec.placement.v.parent, f);
    assert_eq!(g_rec.state.kind(), NodeKind::Folder);
}

#[test]
fn import_builds_a_nested_tree_in_one_write() {
    let (mut p, _dir) = open();
    p.bookmarks().add_url(BookmarkId::OTHER, InsertAt::End, "first", &url("https://f.example/")).unwrap();
    let items = vec![
        ImportItem::Url { title: "one".into(), url: url("https://1.example/"), added_ms: Some(5) },
        ImportItem::Folder {
            title: "dir".into(),
            children: vec![
                ImportItem::Separator,
                ImportItem::Url { title: "two".into(), url: url("https://2.example/"), added_ms: Some(6) },
            ],
        },
        ImportItem::Url { title: "three".into(), url: url("https://3.example/"), added_ms: Some(7) },
    ];
    assert_eq!(p.bookmarks().import(BookmarkId::OTHER, items).unwrap(), 5);
    assert_eq!(titles(&mut p, BookmarkId::OTHER), ["first", "one", "dir", "three"]);
    let dir = p.bookmarks().children(BookmarkId::OTHER)[2].id;
    let kids = p.bookmarks().children(dir);
    assert_eq!(kids.iter().map(|n| n.kind).collect::<Vec<_>>(), [NodeKind::Separator, NodeKind::Url]);
    assert_eq!(kids[1].added_ms, 6);
    // "first" has its own seq; the five imported rows share one, and a batch never splits a
    // seq group: the cut moves back before the group, which then comes whole.
    let batch = p.sync().changes_since(Kind::Bookmarks, Seq::ZERO, 2).unwrap();
    assert_eq!(batch.records.len(), 1);
    assert!(batch.more);
    let batch = p.sync().changes_since(Kind::Bookmarks, batch.upto, 2).unwrap();
    assert_eq!(batch.records.len(), 5);
    assert!(!batch.more);
    assert_eq!(p.bookmarks().import(BookmarkId::OTHER, vec![]).unwrap(), 0);
}

#[test]
fn search_matches_title_then_url() {
    let (mut p, _dir) = open();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Rust book", &url("https://doc.rust-lang.org/book/")).unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Crates", &url("https://crates.io/")).unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Trust me", &url("https://example.org/")).unwrap();
    p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "rust folder").unwrap();
    let hits: Vec<String> = p.bookmarks().search("RUST", 10).into_iter().map(|n| n.title).collect();
    assert_eq!(hits, ["Rust book", "Trust me"]);
    let hits: Vec<String> = p.bookmarks().search("crates.io", 10).into_iter().map(|n| n.title).collect();
    assert_eq!(hits, ["Crates"]);
    assert_eq!(p.bookmarks().search("rust", 1).len(), 1);
    assert!(p.bookmarks().search("", 10).is_empty());
}

#[test]
fn tree_survives_reopen() {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-bm-{}", uuid::Uuid::new_v4())));
    let (f, a) = {
        let mut p = Profile::open(&dir.0, options(1)).unwrap();
        let f = p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "F").unwrap();
        let a = p.bookmarks().add_url(f, InsertAt::End, "A", &url("https://a.example/")).unwrap();
        p.bookmarks().add_url(f, InsertAt::Start, "B", &url("https://b.example/")).unwrap();
        (f, a)
    };
    let mut p = Profile::open(&dir.0, options(1)).unwrap();
    assert_eq!(titles(&mut p, BookmarkId::TOOLBAR), ["F"]);
    assert_eq!(titles(&mut p, f), ["B", "A"]);
    assert!(p.bookmarks().is_bookmarked(&url("https://a.example/")));
    assert_eq!(p.bookmarks().get(a).unwrap().index, 1);
}

/// Two concurrent moves that close a raw cycle: the later move loses and its node goes to
/// OTHER. A local move that would only be a cycle along the raw chain is refused, and a raw
/// cycle that does not involve the moved node does not hang the check.
#[test]
fn raw_chain_check_uses_a_visited_set() {
    let (mut p, _dir) = open();
    let x = p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "X").unwrap();
    let y = p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "Y").unwrap();
    let z = p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "Z").unwrap();
    // Device 2 saw the same three folders and moved Y into X, then X into Y (both later than ours).
    let mut recs = records(&mut p);
    let stamp_of = |r: &BookmarkRecord| r.placement.at;
    let latest = recs.iter().map(stamp_of).max().unwrap();
    let bump = |r: &mut BookmarkRecord, parent: BookmarkId, n: u64| {
        r.placement.v.parent = parent;
        r.placement.at = vsesvit_core::crdt::Stamp { hlc: vsesvit_core::crdt::Hlc(latest.hlc.0 + n), device: DeviceId(2) };
    };
    for r in recs.iter_mut() {
        if r.id == y {
            bump(r, x, 1);
        }
        if r.id == x {
            bump(r, y, 2);
        }
    }
    let wire: Vec<WireRecord> = recs
        .iter()
        .map(|r| WireRecord { kind: Kind::Bookmarks, id: r.id.0.to_string(), body: serde_json::to_vec(r).unwrap() })
        .collect();
    let report = p.sync().apply(wire).unwrap();
    assert_eq!(report.merged, 2);
    assert!(report.changed.bookmarks);
    // X's move closed the cycle, so X hangs under OTHER and Y under X.
    assert_eq!(p.bookmarks().get(x).unwrap().parent, BookmarkId::OTHER);
    assert_eq!(p.bookmarks().get(y).unwrap().parent, x);
    // Y -> X is fine effectively, but the raw chain from X leads to Y: refused.
    assert!(matches!(p.bookmarks().move_to(y, x, InsertAt::End), Err(Error::Bookmark(BookmarkError::WouldCycle))));
    // Z into X: the raw chain X -> Y -> X cycles without Z; the visited set ends the walk.
    p.bookmarks().move_to(z, x, InsertAt::End).unwrap();
    assert_eq!(p.bookmarks().get(z).unwrap().parent, x);
    // Moving X out dissolves the raw cycle; Y is then really under X.
    p.bookmarks().move_to(x, BookmarkId::MOBILE, InsertAt::End).unwrap();
    assert_eq!(p.bookmarks().get(x).unwrap().parent, BookmarkId::MOBILE);
    assert_eq!(p.bookmarks().get(y).unwrap().parent, x);
}
