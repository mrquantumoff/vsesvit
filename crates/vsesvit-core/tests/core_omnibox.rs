//! Omnibox classification, table-tested, plus `resolve`/`suggest` through a real profile.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::crdt::{DeviceId, TimeSource};
use vsesvit_core::history::Transition;
use vsesvit_core::prefs::keys;
use vsesvit_core::search::{classify, classify_url, NavTarget, SearchEngine, SearchEngineId, SuggestionSource, UrlTemplate};
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-omnibox-{}", uuid::Uuid::new_v4())));
    let p = Profile::open(
        &dir.0,
        OpenOptions {
            time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000))),
            new_device_id: Some(DeviceId(7)),
            ..OpenOptions::default()
        },
    )
    .unwrap();
    (p, dir)
}

fn engine(id: &str, name: &str, keyword: &str, url: &str) -> SearchEngine {
    SearchEngine {
        id: SearchEngineId(id.to_owned()),
        name: name.to_owned(),
        keyword: Some(keyword.to_owned()),
        search_url: UrlTemplate(url.to_owned()),
        suggest_url: None,
        builtin: true,
    }
}

fn engines() -> Vec<SearchEngine> {
    vec![
        engine("builtin:ddg", "DuckDuckGo", "d", "https://duckduckgo.com/?q={searchTerms}"),
        engine("builtin:wikipedia", "Wikipedia", "w", "https://en.wikipedia.org/wiki/Special:Search?search={searchTerms}"),
    ]
}

fn url_of(text: &str) -> Option<String> {
    let all = engines();
    match classify(text, &all, &all[0]) {
        Some(NavTarget::Url(u)) => Some(u.to_string()),
        Some(NavTarget::Search { .. }) => panic!("{text:?} classified as a search"),
        None => None,
    }
}

fn search_of(text: &str) -> Option<(String, String)> {
    let all = engines();
    match classify(text, &all, &all[0]) {
        Some(NavTarget::Search { engine, url }) => Some((engine.0, url.to_string())),
        Some(NavTarget::Url(u)) => panic!("{text:?} classified as url {u}"),
        None => None,
    }
}

#[test]
fn empty_input_is_nothing() {
    assert_eq!(url_of(""), None);
    assert_eq!(url_of("   \t"), None);
    assert_eq!(classify_url("  "), None);
}

#[test]
fn explicit_schemes() {
    assert_eq!(url_of("https://example.com/a b").as_deref(), Some("https://example.com/a%20b"));
    assert_eq!(url_of("HTTP://EXAMPLE.COM").as_deref(), Some("http://example.com/"));
    assert_eq!(url_of("about:blank").as_deref(), Some("about:blank"));
    assert_eq!(url_of("file:///tmp/x.html").as_deref(), Some("file:///tmp/x.html"));
    assert_eq!(url_of("view-source:https://example.com/").as_deref(), Some("view-source:https://example.com/"));
    assert_eq!(url_of("data:text/html,hi").as_deref(), Some("data:text/html,hi"));
    assert_eq!(url_of("vsesvit:settings").as_deref(), Some("vsesvit:settings"));
}

#[test]
fn non_navigable_schemes_are_searches() {
    assert!(search_of("javascript:alert(1)").is_some());
    assert!(search_of("mailto:someone@example.com").is_some());
    assert!(search_of("ftp://example.com/").is_some());
}

#[test]
fn bare_hosts_get_https() {
    assert_eq!(url_of("example.com").as_deref(), Some("https://example.com/"));
    assert_eq!(url_of("Example.COM/Path?q=1#f").as_deref(), Some("https://example.com/Path?q=1#f"));
    assert_eq!(url_of("www.example.co.uk").as_deref(), Some("https://www.example.co.uk/"));
    assert_eq!(url_of("example.com:8080/x").as_deref(), Some("https://example.com:8080/x"));
    assert_eq!(url_of("foo.bar").as_deref(), Some("https://foo.bar/"));
}

#[test]
fn localhost_and_ip_literals_get_http() {
    assert_eq!(url_of("localhost").as_deref(), Some("http://localhost/"));
    assert_eq!(url_of("LocalHost:3000/a").as_deref(), Some("http://localhost:3000/a"));
    assert_eq!(url_of("127.0.0.1:8000/page2.html").as_deref(), Some("http://127.0.0.1:8000/page2.html"));
    assert_eq!(url_of("192.168.0.1").as_deref(), Some("http://192.168.0.1/"));
    assert_eq!(url_of("[::1]:8080").as_deref(), Some("http://[::1]:8080/"));
    assert_eq!(url_of("::1").as_deref(), Some("http://[::1]/"));
    assert_eq!(url_of("[fe80::1]/x").as_deref(), Some("http://[fe80::1]/x"));
}

#[test]
fn file_paths_become_file_urls() {
    assert_eq!(url_of("/usr/share/doc").as_deref(), Some("file:///usr/share/doc"));
    assert_eq!(url_of("C:\\Users\\me\\a b.html").as_deref(), Some("file:///C:/Users/me/a%20b.html"));
    assert_eq!(url_of("D:/x/y").as_deref(), Some("file:///D:/x/y"));
    assert_eq!(url_of("\\\\server\\share\\f.txt").as_deref(), Some("file://server/share/f.txt"));
}

#[test]
fn not_hosts() {
    assert!(search_of("rust").is_some());
    assert!(search_of("hello world").is_some());
    assert!(search_of("example.c0m").is_some());
    assert!(search_of("1.2.3").is_some());
    assert!(search_of("example.com is down").is_some());
    assert!(search_of("example.com:99999").is_some());
    assert!(search_of("-bad.com").is_some());
    assert!(search_of("a:b").is_some());
    assert!(search_of("[::1").is_some());
}

#[test]
fn keywords_and_default_engine() {
    assert_eq!(
        search_of("w rust lang"),
        Some(("builtin:wikipedia".to_owned(), "https://en.wikipedia.org/wiki/Special:Search?search=rust+lang".to_owned()))
    );
    assert_eq!(search_of("d example.com").unwrap().0, "builtin:ddg");
    assert_eq!(search_of("hello world"), Some(("builtin:ddg".to_owned(), "https://duckduckgo.com/?q=hello+world".to_owned())));
    // A keyword alone is a plain search for that word.
    assert_eq!(search_of("w"), Some(("builtin:ddg".to_owned(), "https://duckduckgo.com/?q=w".to_owned())));
    assert_eq!(search_of("W rust").unwrap().0, "builtin:ddg");
    assert_eq!(search_of("caf\u{e9} & bar").unwrap().1, "https://duckduckgo.com/?q=caf%C3%A9+%26+bar");
}

#[test]
fn resolve_uses_the_profile_engines() {
    let (mut p, _dir) = open();
    let t = p.omnibox().resolve("vsesvit fixture").unwrap().unwrap();
    assert!(matches!(&t, NavTarget::Search { engine, .. } if engine.0 == "builtin:ddg"));
    assert_eq!(t.url().as_str(), "https://duckduckgo.com/?q=vsesvit+fixture");
    let t = p.omnibox().resolve("127.0.0.1:4321/page2.html").unwrap().unwrap();
    assert_eq!(t, NavTarget::Url(Url::parse("http://127.0.0.1:4321/page2.html").unwrap()));
    assert_eq!(p.omnibox().resolve("g maps").unwrap().unwrap().url().as_str(), "https://www.google.com/search?q=maps");
    assert_eq!(p.omnibox().resolve("b maps").unwrap().unwrap().url().host_str(), Some("www.bing.com"));
    assert!(p.omnibox().resolve("").unwrap().is_none());
}

#[test]
fn suggest_ranks_prefix_then_bookmarks_then_history() {
    let (mut p, _dir) = open();
    let docs = Url::parse("https://docs.rs/serde").unwrap();
    let rust = Url::parse("https://www.rust-lang.org/").unwrap();
    let other = Url::parse("https://example.org/rustic").unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Serde docs", &docs).unwrap();
    p.history().record_visit(&rust, Transition::Typed).unwrap();
    p.history().set_title(&rust, "Rust Programming Language").unwrap();
    p.history().record_visit(&other, Transition::Link).unwrap();
    p.history().set_title(&other, "rustic things").unwrap();
    p.history().record_visit(&docs, Transition::Link).unwrap();

    let s = p.omnibox().suggest("rust", 8).unwrap();
    assert_eq!(s[0].source, SuggestionSource::Search);
    assert!(s[0].title.contains("DuckDuckGo"));
    let urls: Vec<&str> = s[1..].iter().map(|x| x.target.url().as_str()).collect();
    // url-prefix match first, then the rest by frecency; no duplicates
    assert_eq!(urls[0], "https://www.rust-lang.org/");
    assert!(urls.contains(&"https://example.org/rustic"));
    assert_eq!(urls.iter().filter(|u| **u == "https://docs.rs/serde").count(), 0, "no substring match on 'rust'");

    let s = p.omnibox().suggest("serde", 8).unwrap();
    assert_eq!(s[0].source, SuggestionSource::Search);
    assert_eq!(s[1].source, SuggestionSource::Bookmark);
    assert_eq!(s[1].title, "Serde docs");
    assert_eq!(s.len(), 2, "the bookmark and the history entry share a url");

    let s = p.omnibox().suggest("docs.rs", 8).unwrap();
    assert_eq!(s[0].source, SuggestionSource::Typed);
    assert_eq!(s[0].target.url().as_str(), "https://docs.rs/");

    assert_eq!(p.omnibox().suggest("rust", 1).unwrap().len(), 1);
    assert!(p.omnibox().suggest("  ", 8).unwrap().is_empty());
}

#[test]
fn suggest_leaves_out_the_sources_turned_off() {
    let (mut p, _dir) = open();
    let marked = Url::parse("https://rust.example/marked").unwrap();
    let visited = Url::parse("https://rust.example/visited").unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "rust marked", &marked).unwrap();
    p.history().record_visit(&visited, Transition::Typed).unwrap();
    let sources = |p: &mut Profile| -> Vec<SuggestionSource> {
        p.omnibox().suggest("rust", 8).unwrap().into_iter().map(|s| s.source).collect()
    };

    assert_eq!(sources(&mut p), [SuggestionSource::Search, SuggestionSource::Bookmark, SuggestionSource::History]);
    p.prefs().set(&keys::SUGGEST_BOOKMARKS, &false).unwrap();
    assert_eq!(sources(&mut p), [SuggestionSource::Search, SuggestionSource::History]);
    p.prefs().set(&keys::SUGGEST_HISTORY, &false).unwrap();
    assert_eq!(sources(&mut p), [SuggestionSource::Search]);
}
