//! Writing the bookmarks to a bookmarks HTML file, which the importer reads back.

use std::path::PathBuf;

use vsesvit_core::bookmarks::{BookmarkId, ImportItem};
use vsesvit_core::{OpenOptions, Profile, Url, export, import};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("vsesvit-exporttest-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn profile(dir: &TempDir) -> Profile {
    Profile::open(&dir.0.join("profile"), OpenOptions::default()).unwrap()
}

/// Added on a whole second, which is all a bookmarks file keeps.
fn link(title: &str, url: &str, added_s: i64) -> ImportItem {
    ImportItem::Url { title: title.into(), url: Url::parse(url).unwrap(), added_ms: Some(added_s * 1000) }
}

fn folder(title: &str, children: Vec<ImportItem>) -> ImportItem {
    ImportItem::Folder { title: title.into(), children }
}

#[test]
fn an_export_imports_back_as_the_same_tree() {
    let dir = TempDir::new();
    let mut p = profile(&dir);
    let bar = vec![
        link("News & <views>", "https://news.example/?a=1&b=2", 1_700_000_001),
        folder(
            "Work \"projects\" & 'notes'",
            vec![
                link("Tracker \u{2014} \u{263A}", "https://tracker.example/path?q=%22x%22", 1_700_000_002),
                ImportItem::Separator,
                folder("Empty", vec![]),
                folder("Deeper", vec![folder("Deepest", vec![link("Leaf", "https://leaf.example/", 1_700_000_003)])]),
            ],
        ),
        ImportItem::Separator,
    ];
    let other = vec![link("Loose", "https://other.example/", 1_700_000_004), folder("Reading", vec![])];
    let mobile = vec![link("Phone", "https://phone.example/", 1_700_000_005)];
    p.bookmarks().import(BookmarkId::TOOLBAR, bar.clone()).unwrap();
    p.bookmarks().import(BookmarkId::OTHER, other.clone()).unwrap();
    p.bookmarks().import(BookmarkId::MOBILE, mobile.clone()).unwrap();

    let html = export::html(&p.bookmarks());
    assert!(html.starts_with("<!DOCTYPE NETSCAPE-Bookmark-file-1>"));
    assert!(html.contains("<DT><H3 PERSONAL_TOOLBAR_FOLDER=\"true\">Bookmarks bar</H3>"), "{html}");

    // The importer unwraps the bar and keeps the other roots' items, as it does for Chrome's files.
    let mut expected = bar;
    expected.extend(other);
    expected.push(folder("Mobile bookmarks", mobile));
    assert_eq!(import::parse_html(&html), expected);

    let again_dir = TempDir::new();
    let mut again = profile(&again_dir);
    again.bookmarks().import(BookmarkId::OTHER, import::parse_html(&html)).unwrap();
    assert_eq!(import::parse_html(&export::html(&again.bookmarks())), expected);
}

#[test]
fn empty_roots_export_as_chrome_writes_them() {
    let dir = TempDir::new();
    let mut p = profile(&dir);
    let html = export::html(&p.bookmarks());
    assert!(html.ends_with(
        "<DL><p>
    <DT><H3 PERSONAL_TOOLBAR_FOLDER=\"true\">Bookmarks bar</H3>
    <DL><p>
    </DL><p>
</DL><p>
"
    ));
    assert_eq!(import::parse_html(&html), vec![]);
}

#[test]
fn folders_nested_past_what_the_importer_keeps_are_all_written() {
    let dir = TempDir::new();
    let mut p = profile(&dir);
    let chain = (0..200).fold(link("Bottom", "https://bottom.example/", 1_700_000_000), |inner, i| {
        folder(&format!("F{i}"), vec![inner])
    });
    p.bookmarks().import(BookmarkId::OTHER, vec![chain]).unwrap();
    let html = export::html(&p.bookmarks());
    assert_eq!(html.matches("<DL><p>").count(), html.matches("</DL><p>").count());
    assert_eq!(html.matches("<H3").count(), 201);
    assert!(html.contains(&format!("{}<DT><A HREF=\"https://bottom.example/\"", "    ".repeat(201))));
}

#[test]
fn the_file_is_named_for_the_day_as_chrome_names_it() {
    assert_eq!(export::file_name(2026, 10, 6), "bookmarks_10_6_26.html");
    assert_eq!(export::file_name(2005, 1, 31), "bookmarks_1_31_05.html");
}
