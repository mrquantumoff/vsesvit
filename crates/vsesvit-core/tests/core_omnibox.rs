//! Omnibox classification, table-tested, plus `resolve`/`suggest` through a real profile.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::crdt::{DeviceId, TimeSource};
use vsesvit_core::history::Transition;
use vsesvit_core::prefs::keys;
use vsesvit_core::search::{
    classify, classify_url, ctrl_enter_url, selection_action, NavTarget, SearchEngine, SearchEngineId, Suggestions, SuggestionSource,
    UrlTemplate,
};
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
fn internationalized_hosts_open_as_urls() {
    assert_eq!(url_of("пример.укр").as_deref(), Some("https://xn--e1afmkfd.xn--j1amh/"));
    assert_eq!(url_of("münchen.de").as_deref(), Some("https://xn--mnchen-3ya.de/"));
    assert_eq!(url_of("яндекс.рф/вики").as_deref(), Some("https://xn--d1acpjx3f.xn--p1ai/%D0%B2%D0%B8%D0%BA%D0%B8"));
    assert_eq!(url_of("bücher.example:8080").as_deref(), Some("https://xn--bcher-kva.example:8080/"));
    assert_eq!(url_of("example.xn--p1ai").as_deref(), Some("https://example.xn--p1ai/"));
    // Single words and a one-letter TLD still search, as in ASCII.
    assert!(search_of("привіт").is_some());
    assert!(search_of("münchen").is_some());
    assert!(search_of("т.д").is_some());
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

    let s = p.omnibox().suggest("rust", 8, false).unwrap().items;
    assert_eq!(s[0].source, SuggestionSource::Search);
    assert!(s[0].title.contains("DuckDuckGo"));
    let urls: Vec<&str> = s[1..].iter().map(|x| x.target.url().as_str()).collect();
    // url-prefix match first, then the rest by frecency; no duplicates
    assert_eq!(urls[0], "https://www.rust-lang.org/");
    assert!(urls.contains(&"https://example.org/rustic"));
    assert_eq!(urls.iter().filter(|u| **u == "https://docs.rs/serde").count(), 0, "no substring match on 'rust'");

    let s = p.omnibox().suggest("serde", 8, false).unwrap().items;
    assert_eq!(s[0].source, SuggestionSource::Search);
    assert_eq!(s[1].source, SuggestionSource::Bookmark);
    assert_eq!(s[1].title, "Serde docs");
    assert_eq!(s.len(), 2, "the bookmark and the history entry share a url");

    let s = p.omnibox().suggest("docs.rs", 8, false).unwrap().items;
    assert_eq!(s[0].source, SuggestionSource::Typed);
    assert_eq!(s[0].target.url().as_str(), "https://docs.rs/");

    assert_eq!(p.omnibox().suggest("rust", 1, false).unwrap().items.len(), 1);
    assert!(p.omnibox().suggest("  ", 8, true).unwrap() == Suggestions::default());
}

#[test]
fn suggest_leaves_out_the_sources_turned_off() {
    let (mut p, _dir) = open();
    let marked = Url::parse("https://rust.example/marked").unwrap();
    let visited = Url::parse("https://rust.example/visited").unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "rust marked", &marked).unwrap();
    p.history().record_visit(&visited, Transition::Typed).unwrap();
    let sources = |p: &mut Profile| -> Vec<SuggestionSource> {
        p.omnibox().suggest("rust", 8, false).unwrap().items.into_iter().map(|s| s.source).collect()
    };

    assert_eq!(sources(&mut p), [SuggestionSource::Search, SuggestionSource::Bookmark, SuggestionSource::History]);
    p.prefs().set(&keys::SUGGEST_BOOKMARKS, &false).unwrap();
    assert_eq!(sources(&mut p), [SuggestionSource::Search, SuggestionSource::History]);
    p.prefs().set(&keys::SUGGEST_HISTORY, &false).unwrap();
    assert_eq!(sources(&mut p), [SuggestionSource::Search]);
}

fn visit(p: &mut Profile, url: &str, title: &str) {
    let url = Url::parse(url).unwrap();
    p.history().record_visit(&url, Transition::Typed).unwrap();
    p.history().set_title(&url, title).unwrap();
}

/// Suggestions whose inline text, when present, completes `typed` to `items[0].fill`.
fn suggest(p: &mut Profile, typed: &str, allow_inline: bool) -> Suggestions {
    let s = p.omnibox().suggest(typed, 8, allow_inline).unwrap();
    if let Some(inline) = &s.inline {
        assert_eq!(format!("{typed}{inline}").to_lowercase(), s.items[0].fill.to_lowercase(), "{typed:?}");
        assert!(s.items[0].fill.ends_with(inline.as_str()), "{typed:?}: the suffix comes from the fill");
    }
    s
}

#[test]
fn fill_drops_scheme_www_and_a_bare_trailing_slash() {
    let (mut p, _dir) = open();
    for url in ["https://www.rust-lang.org/", "https://docs.rs/serde/", "http://example.org/a?b=1", "http://example.org/", "https://example.net/?q=1"] {
        visit(&mut p, url, "");
    }
    let table = [
        ("rust", "https://www.rust-lang.org/", "rust-lang.org"),
        ("www.rust", "https://www.rust-lang.org/", "www.rust-lang.org"),
        ("https://www.rust", "https://www.rust-lang.org/", "https://www.rust-lang.org"),
        ("HTTPS://rust", "https://www.rust-lang.org/", "https://rust-lang.org"),
        ("http://rust", "https://www.rust-lang.org/", "rust-lang.org"),
        ("docs", "https://docs.rs/serde/", "docs.rs/serde/"),
        ("exam", "http://example.org/a?b=1", "example.org/a?b=1"),
        ("http://exa", "http://example.org/", "http://example.org"),
        ("example.n", "https://example.net/?q=1", "example.net/?q=1"),
    ];
    for (typed, url, want) in table {
        let s = suggest(&mut p, typed, false);
        let row = s.items.iter().find(|r| r.target.url().as_str() == url).unwrap_or_else(|| panic!("{typed:?}: no row for {url}"));
        assert_eq!(row.fill, want, "{typed:?} -> {url}");
    }

    let first_fill = |p: &mut Profile, typed: &str| suggest(p, typed, false).items[0].fill.clone();
    assert_eq!(first_fill(&mut p, "  hello world "), "hello world", "a search row fills the trimmed text");
    assert_eq!(first_fill(&mut p, "Example.com/Path"), "example.com/Path");
    assert_eq!(first_fill(&mut p, "HTTPS://EXAMPLE.COM"), "https://example.com");
    assert_eq!(first_fill(&mut p, "about:blank"), "about:blank");
}

#[test]
fn inline_completes_a_host_prefix_to_the_site_root() {
    let (mut p, _dir) = open();
    visit(&mut p, "https://github.com/rust-lang/rust", "rust-lang/rust");

    let s = suggest(&mut p, "git", true);
    assert_eq!(s.inline.as_deref(), Some("hub.com"));
    assert_eq!(s.items[0].target.url().as_str(), "https://github.com/");
    assert_eq!(s.items[0].fill, "github.com");
    assert_eq!(s.items[0].title, "github.com", "an unvisited root is titled by its host");
    assert_eq!(s.items[0].source, SuggestionSource::Typed, "an unvisited root is no history row");
    assert_eq!(s.items[1].source, SuggestionSource::Search, "the what-you-typed row comes second");
    assert_eq!(s.items[2].target.url().as_str(), "https://github.com/rust-lang/rust");

    assert_eq!(suggest(&mut p, "GiT", true).inline.as_deref(), Some("hub.com"), "the typed casing stays the user's");
    let s = suggest(&mut p, "https://git", true);
    assert_eq!((s.inline.as_deref(), s.items[0].fill.as_str()), (Some("hub.com"), "https://github.com"));

    let s = p.omnibox().suggest("git", 1, true).unwrap();
    assert_eq!(s.items.len(), 1);
    assert_eq!(s.items[0].target.url().as_str(), "https://github.com/");
}

#[test]
fn inline_uses_the_visited_root_once() {
    let (mut p, _dir) = open();
    visit(&mut p, "https://github.com/rust-lang/rust", "rust-lang/rust");
    visit(&mut p, "https://github.com/", "GitHub");

    let s = suggest(&mut p, "git", true);
    assert_eq!(s.inline.as_deref(), Some("hub.com"));
    assert_eq!(s.items[0].title, "GitHub");
    assert_eq!(s.items[0].source, SuggestionSource::History);
    assert_eq!(s.items.iter().filter(|r| r.target.url().as_str() == "https://github.com/").count(), 1);
}

#[test]
fn inline_completes_the_url_itself_after_a_slash() {
    let (mut p, _dir) = open();
    visit(&mut p, "https://github.com/rust-lang/rust", "rust-lang/rust");

    let s = suggest(&mut p, "github.com/r", true);
    assert_eq!(s.inline.as_deref(), Some("ust-lang/rust"));
    assert_eq!(s.items[0].target.url().as_str(), "https://github.com/rust-lang/rust");
    assert_eq!(s.items[1].source, SuggestionSource::Typed);
    assert_eq!(s.items[1].target.url().as_str(), "https://github.com/r");
    assert_eq!(s.items.len(), 2, "the completed row is not repeated");
}

#[test]
fn no_inline_without_permission_whitespace_or_a_suffix() {
    let (mut p, _dir) = open();
    visit(&mut p, "https://github.com/rust-lang/rust", "rust-lang/rust");

    let s = suggest(&mut p, "git", false);
    assert_eq!(s.inline, None);
    assert_eq!(s.items[0].source, SuggestionSource::Search);

    for typed in ["git hub", "git ", " git"] {
        let s = suggest(&mut p, typed, true);
        assert_eq!(s.inline, None, "{typed:?}");
        assert_eq!(s.items[0].source, SuggestionSource::Search, "{typed:?}");
    }

    let s = suggest(&mut p, "github.com", true);
    assert_eq!(s.inline, None, "the typed text is already the whole fill");
    assert_eq!(s.items[0].source, SuggestionSource::Typed);
    assert_eq!(s.items[0].target.url().as_str(), "https://github.com/");

    assert_eq!(suggest(&mut p, "www.git", true).inline, None, "the fill has no www. to match");
    assert_eq!(suggest(&mut p, "zzz", true).inline, None);
}

#[test]
fn a_typed_url_in_history_is_one_row() {
    let (mut p, _dir) = open();
    visit(&mut p, "https://github.com/", "GitHub");
    visit(&mut p, "https://github.com/rust-lang/rust", "rust-lang/rust");

    for allow_inline in [false, true] {
        for (typed, url) in [("github.com", "https://github.com/"), ("github.com/rust-lang/rust", "https://github.com/rust-lang/rust")] {
            let s = suggest(&mut p, typed, allow_inline);
            assert_eq!(s.items.iter().filter(|r| r.target.url().as_str() == url).count(), 1, "{typed:?} {allow_inline}");
            assert_eq!(s.items[0].target.url().as_str(), url, "{typed:?} {allow_inline}");
        }
    }
    let s = suggest(&mut p, "github.com", false);
    assert_eq!((s.items[0].title.as_str(), s.items[0].fill.as_str()), ("GitHub", "github.com"), "the visited row takes the typed row's place");
}

#[test]
fn search_terms_outside_the_query_encode_a_space_as_percent_20() {
    let path = UrlTemplate("https://en.wiktionary.org/wiki/{searchTerms}".into());
    assert_eq!(path.expand("ice cream").unwrap().as_str(), "https://en.wiktionary.org/wiki/ice%20cream");
    assert_eq!(path.expand("a+b/c?d#e").unwrap().as_str(), "https://en.wiktionary.org/wiki/a%2Bb%2Fc%3Fd%23e");
    let fragment = UrlTemplate("https://example.com/#{searchTerms}?x".into());
    assert_eq!(fragment.expand("a b").unwrap().as_str(), "https://example.com/#a%20b?x");
    let query = UrlTemplate("https://duckduckgo.com/?q={searchTerms}".into());
    assert_eq!(query.expand("ice cream+x").unwrap().as_str(), "https://duckduckgo.com/?q=ice+cream%2Bx");
}

#[test]
fn selection_actions() {
    let ddg = &engines()[0];
    let long = "a".repeat(60);
    let fifty = "b".repeat(50);
    let table = [
        ("  hello \n  world\t".to_owned(), "Search DuckDuckGo for \u{201c}hello world\u{201d}".to_owned(), "https://duckduckgo.com/?q=hello+world".to_owned()),
        (long.clone(), format!("Search DuckDuckGo for \u{201c}{}\u{2026}\u{201d}", "a".repeat(50)), format!("https://duckduckgo.com/?q={long}")),
        (fifty.clone(), format!("Search DuckDuckGo for \u{201c}{fifty}\u{201d}"), format!("https://duckduckgo.com/?q={fifty}")),
        ("example.com".to_owned(), "Go to example.com".to_owned(), "https://example.com/".to_owned()),
        ("пример.укр".to_owned(), "Go to пример.укр".to_owned(), "https://xn--e1afmkfd.xn--j1amh/".to_owned()),
        (" https://a.test/x ".to_owned(), "Go to https://a.test/x".to_owned(), "https://a.test/x".to_owned()),
        (
            "see example.com now".to_owned(),
            "Search DuckDuckGo for \u{201c}see example.com now\u{201d}".to_owned(),
            "https://duckduckgo.com/?q=see+example.com+now".to_owned(),
        ),
        ("/usr/share".to_owned(), "Search DuckDuckGo for \u{201c}/usr/share\u{201d}".to_owned(), "https://duckduckgo.com/?q=%2Fusr%2Fshare".to_owned()),
    ];
    for (text, label, url) in &table {
        let a = selection_action(text, ddg).unwrap_or_else(|| panic!("{text:?}"));
        assert_eq!((a.label.as_str(), a.url.as_str()), (label.as_str(), url.as_str()), "{text:?}");
    }
    assert_eq!(selection_action(" \n\t", ddg), None);
    assert_eq!(selection_action("", ddg), None);
}

#[test]
fn for_selection_uses_the_default_engine() {
    let (mut p, _dir) = open();
    let a = p.omnibox().for_selection("rust  lang").unwrap().unwrap();
    assert_eq!(a.label, "Search DuckDuckGo for \u{201c}rust lang\u{201d}");
    assert_eq!(a.url.as_str(), "https://duckduckgo.com/?q=rust+lang");
    p.search_engines().set_default(&SearchEngineId("builtin:google".to_owned())).unwrap();
    assert_eq!(p.omnibox().for_selection("rust").unwrap().unwrap().label, "Search Google for \u{201c}rust\u{201d}");
    assert_eq!(p.omnibox().for_selection("   ").unwrap(), None);
}

#[test]
fn ctrl_enter_wraps_a_bare_word() {
    assert_eq!(ctrl_enter_url("google").map(String::from).as_deref(), Some("https://www.google.com/"));
    assert_eq!(ctrl_enter_url("  My-Site ").map(String::from).as_deref(), Some("https://www.my-site.com/"));
    for text in ["a.b", "two words", "http://x", "", "  ", "caf\u{e9}"] {
        assert_eq!(ctrl_enter_url(text), None, "{text:?}");
    }
}
