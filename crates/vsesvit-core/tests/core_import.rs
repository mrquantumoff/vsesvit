//! Reading bookmarks exported by or stored in other browsers, and importing them into a profile.

use std::path::PathBuf;

use vsesvit_core::bookmarks::{BookmarkId, ImportItem, NodeKind};
use vsesvit_core::import::{self, Source};
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("vsesvit-importtest-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn link(title: &str, url: &str, added_ms: Option<i64>) -> ImportItem {
    ImportItem::Url { title: title.into(), url: Url::parse(url).unwrap(), added_ms }
}

fn folder(title: &str, children: Vec<ImportItem>) -> ImportItem {
    ImportItem::Folder { title: title.into(), children }
}

const CHROME_HTML: &str = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<!-- This is an automatically generated file.
     It will be read and overwritten.
     DO NOT EDIT! -->
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><H3 ADD_DATE="1700000000" LAST_MODIFIED="0" PERSONAL_TOOLBAR_FOLDER="true">Bookmarks bar</H3>
    <DL><p>
        <DT><A HREF="https://news.example/?a=1&amp;b=2" ADD_DATE="1700000001" ICON="data:image/png;base64,AAAA">News &amp; views</A>
        <DT><H3 ADD_DATE="1700000002">Work</H3>
        <DL><p>
            <DT><A HREF="https://tracker.example/">Tracker &#8212; &#x263A;</A>
            <HR>
            <DT><A HREF="javascript:alert(1)">Bookmarklet</A>
        </DL><p>
        <DT><H3>Empty</H3>
        <DT><A HREF='https://single.example/'>Single quoted</A>
    </DL><p>
    <DT><A HREF="https://other.example/" ADD_DATE="1700000003">Loose</A>
</DL><p>
"#;

#[test]
fn chrome_html_export_puts_the_bar_first_and_keeps_folders() {
    assert_eq!(
        import::parse_html(CHROME_HTML),
        vec![
            link("News & views", "https://news.example/?a=1&b=2", Some(1_700_000_001_000)),
            folder(
                "Work",
                vec![link("Tracker \u{2014} \u{263A}", "https://tracker.example/", None), ImportItem::Separator]
            ),
            folder("Empty", vec![]),
            link("Single quoted", "https://single.example/", None),
            link("Loose", "https://other.example/", Some(1_700_000_003_000)),
        ]
    );
}

#[test]
fn firefox_html_export_keeps_menu_items_and_unwraps_the_toolbar() {
    let html = r#"<DL><p>
    <DT><A HREF="https://menu.example/">In the menu</A>
    <DT><H3 PERSONAL_TOOLBAR_FOLDER="true">Bookmarks Toolbar</H3>
    <DL><p>
        <DT><A HREF="https://bar.example/">On the bar</A>
    </DL><p>
    <DT><H3 UNFILED_BOOKMARKS_FOLDER="true">Other Bookmarks</H3>
    <DL><p>
        <DT><A HREF="place:sort=8&maxResults=10">Recent</A>
        <DT><A HREF="https://unfiled.example/">Unfiled</A>
    </DL><p>
</DL>"#;
    assert_eq!(
        import::parse_html(html),
        vec![
            link("On the bar", "https://bar.example/", None),
            link("In the menu", "https://menu.example/", None),
            folder("Other Bookmarks", vec![link("Unfiled", "https://unfiled.example/", None)]),
        ]
    );
}

#[test]
fn unclosed_lists_still_yield_their_folders() {
    let html = "<DL><DT><H3>A</H3><DL><DT><A HREF=https://a.example/>a</A>";
    assert_eq!(import::parse_html(html), vec![folder("A", vec![link("a", "https://a.example/", None)])]);
    assert_eq!(import::parse_html("<html><body>not bookmarks</body></html>"), vec![]);
}

#[test]
fn out_of_range_add_dates_are_dropped() {
    let html = r#"<DL><p>
    <DT><A HREF="https://a.example/" ADD_DATE="9999999999999999">Huge</A>
    <DT><A HREF="https://b.example/" ADD_DATE="-5">Negative</A>
    <DT><A HREF="https://c.example/" ADD_DATE="0">Zero</A>
    <DT><A HREF="https://d.example/" ADD_DATE="1700000000">Ok</A>
</DL>"#;
    assert_eq!(
        import::parse_html(html),
        vec![
            link("Huge", "https://a.example/", None),
            link("Negative", "https://b.example/", None),
            link("Zero", "https://c.example/", None),
            link("Ok", "https://d.example/", Some(1_700_000_000_000)),
        ]
    );
}

const CHROMIUM_JSON: &str = r#"{
   "checksum": "x",
   "roots": {
      "bookmark_bar": { "children": [
         { "date_added": "13345000000000000", "name": "Bar link", "type": "url", "url": "https://bar.example/" },
         { "children": [ { "date_added": "0", "name": "Deep", "type": "url", "url": "https://deep.example/" } ],
           "name": "Folder", "type": "folder" }
      ], "name": "Bookmarks bar", "type": "folder" },
      "other": { "children": [
         { "name": "Other link", "type": "url", "url": "https://other.example/" },
         { "name": "Script", "type": "url", "url": "javascript:void(0)" }
      ], "name": "Other bookmarks", "type": "folder" },
      "synced": { "children": [], "name": "Mobile bookmarks", "type": "folder" }
   },
   "version": 1
}"#;

#[test]
fn chromium_json_unwraps_the_bar_and_skips_empty_roots() {
    // 13345000000000000 us since 1601 = 1700526400000 ms since 1970.
    assert_eq!(
        import::parse_chromium(CHROMIUM_JSON).unwrap(),
        vec![
            link("Bar link", "https://bar.example/", Some(1_700_526_400_000)),
            folder("Folder", vec![link("Deep", "https://deep.example/", None)]),
            folder("Other bookmarks", vec![link("Other link", "https://other.example/", None)]),
        ]
    );
    assert!(import::parse_chromium("{\"no\": 1}").is_err());
}

#[test]
fn a_file_source_tells_json_from_html() {
    let dir = TempDir::new();
    let json = dir.0.join("Bookmarks");
    std::fs::write(&json, CHROMIUM_JSON).unwrap();
    let html = dir.0.join("bookmarks.html");
    std::fs::write(&html, CHROME_HTML).unwrap();
    assert_eq!(Source::File(json).read().unwrap().len(), 3);
    assert_eq!(Source::File(html).read().unwrap().len(), 5);
    assert!(Source::File(dir.0.join("missing.html")).read().is_err());
}

#[test]
fn firefox_places_are_read_from_a_copy_while_the_original_is_locked() {
    let dir = TempDir::new();
    let places = dir.0.join("places.sqlite");
    let db = rusqlite::Connection::open(&places).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT);
         CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER, parent INTEGER,
                                     position INTEGER, title TEXT, dateAdded INTEGER, guid TEXT);
         INSERT INTO moz_places VALUES (1, 'https://bar.example/'), (2, 'https://menu.example/'),
                                       (3, 'place:type=6'), (4, 'https://nested.example/');
         INSERT INTO moz_bookmarks VALUES
           (1, 2, NULL, 0, 0, '', 0, 'root________'),
           (2, 2, NULL, 1, 0, 'menu', 0, 'menu________'),
           (3, 2, NULL, 1, 1, 'toolbar', 0, 'toolbar_____'),
           (4, 2, NULL, 1, 3, 'unfiled', 0, 'unfiled_____'),
           (5, 2, NULL, 1, 4, 'mobile', 0, 'mobile______'),
           (10, 1, 1, 3, 1, 'Bar', 1700000000000000, 'a'),
           (11, 2, NULL, 3, 0, 'Sub', 0, 'b'),
           (12, 1, 4, 11, 0, 'Nested', NULL, 'c'),
           (13, 1, 3, 3, 2, 'Most visited', 0, 'd'),
           (14, 3, NULL, 2, 0, NULL, 0, 'e'),
           (15, 1, 2, 2, 1, NULL, 0, 'f');
         PRAGMA locking_mode = EXCLUSIVE;
         BEGIN EXCLUSIVE;",
    )
    .unwrap();
    let items = Source::Firefox(places).read().unwrap();
    assert_eq!(
        items,
        vec![
            folder("Sub", vec![link("Nested", "https://nested.example/", None)]),
            link("Bar", "https://bar.example/", Some(1_700_000_000_000)),
            folder("Bookmarks Menu", vec![ImportItem::Separator, link("", "https://menu.example/", None)]),
        ]
    );
    drop(db);
    let leftovers = std::fs::read_dir(std::env::temp_dir()).unwrap().filter_map(Result::ok).any(|e| {
        e.file_name().to_string_lossy().starts_with("vsesvit-import-") && e.path().join("places.sqlite").exists()
    });
    assert!(!leftovers, "the copy is removed");
}

#[test]
fn import_folder_lands_on_the_bookmarks_bar() {
    let dir = TempDir::new();
    let mut p = Profile::open(&dir.0.join("profile"), OpenOptions::default()).unwrap();
    let items = import::parse_chromium(CHROMIUM_JSON).unwrap();
    assert_eq!(p.bookmarks().import_folder("Imported from Chrome", items).unwrap(), 5);
    let bar = p.bookmarks().children(BookmarkId::TOOLBAR);
    assert_eq!(bar.len(), 1);
    assert_eq!((bar[0].kind, bar[0].title.as_str()), (NodeKind::Folder, "Imported from Chrome"));
    let inside: Vec<String> = p.bookmarks().children(bar[0].id).into_iter().map(|n| n.title).collect();
    assert_eq!(inside, ["Bar link", "Folder", "Other bookmarks"]);
    assert!(p.bookmarks().is_bookmarked(&Url::parse("https://deep.example/").unwrap()));
    assert_eq!(p.bookmarks().import_folder("Nothing", vec![]).unwrap(), 0);
    assert_eq!(p.bookmarks().children(BookmarkId::TOOLBAR).len(), 1);
}
