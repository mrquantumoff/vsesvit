//! The default engine's search suggestions through a real profile, against the fixture
//! server's `/suggest`.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::crdt::{DeviceId, TimeSource};
use vsesvit_core::history::Transition;
use vsesvit_core::prefs::keys;
use vsesvit_core::search::{EngineForm, NavTarget, SearchEngineId, SuggestionSource, Suggestions};
use vsesvit_core::suggest::{Queries, SearchSuggestions};
use vsesvit_core::testkit::FixtureServer;
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A profile whose default engine searches and suggests from the fixture server.
fn open() -> (Profile, TempDir, FixtureServer, SearchEngineId) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-suggest-{}", uuid::Uuid::new_v4())));
    let mut p = Profile::open(
        &dir.0,
        OpenOptions {
            time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000))),
            new_device_id: Some(DeviceId(7)),
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let server = FixtureServer::start().unwrap();
    let form = EngineForm { name: "Fixture".into(), keyword: "fx".into(), url: format!("{}/search?q=%s", server.origin()) };
    let fixture = p.search_engines().save(None, &form).unwrap();
    p.search_engines().set_suggest_url(&fixture, Some(&format!("{}/suggest?q={{searchTerms}}", server.origin()))).unwrap();
    p.search_engines().set_default(&fixture).unwrap();
    (p, dir, server, fixture)
}

fn rows(found: &SearchSuggestions) -> Vec<String> {
    let mut s = Suggestions::default();
    s.add_search_suggestions(found);
    s.items.into_iter().map(|row| row.title).collect()
}

#[test]
fn a_plain_search_brings_the_engines_suggestions_above_bookmarks_and_history() {
    let (mut p, _dir, server, fixture) = open();
    let visited = Url::parse("https://rust.example/h").unwrap();
    p.history().record_visit(&visited, Transition::Typed).unwrap();
    p.history().set_title(&visited, "rust history").unwrap();
    p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "rust bookmark", &Url::parse("https://rust.example/b").unwrap()).unwrap();

    let queries = Queries::default();
    let found = p.omnibox().suggest_request("rust", &queries, false).unwrap().expect("a plain search").run().expect("still the newest");
    assert!(found.is_current());
    assert_eq!(server.hits(), ["/suggest"]);

    let mut s = p.omnibox().suggest("rust", 8, false).unwrap();
    s.add_search_suggestions(&found);
    let sources: Vec<SuggestionSource> = s.items.iter().map(|row| row.source.clone()).collect();
    let (search, suggested) = (SuggestionSource::Search, SuggestionSource::SuggestedSearch);
    assert_eq!(sources, [search.clone(), suggested.clone(), suggested.clone(), SuggestionSource::Bookmark, SuggestionSource::History]);
    let shown: Vec<(&str, &str)> = s.items[1..3].iter().map(|row| (row.title.as_str(), row.fill.as_str())).collect();
    assert_eq!(shown, [("rust one", "rust one"), ("rust two", "rust two")]);
    assert_eq!(s.items[1].target, NavTarget::Search { engine: fixture, url: server.url("/search?q=rust+one") });

    let mut s = p.omnibox().suggest("rust", 8, true).unwrap();
    s.add_search_suggestions(&found);
    let sources: Vec<SuggestionSource> = s.items.iter().map(|row| row.source.clone()).collect();
    assert_eq!(s.items[0].fill, "rust.example", "the inline completion stays the default match");
    assert_eq!(s.inline.as_deref(), Some(".example"));
    assert_eq!(sources, [SuggestionSource::Typed, search, suggested.clone(), suggested, SuggestionSource::Bookmark, SuggestionSource::History]);

    let mut s = p.omnibox().suggest("rust two", 8, false).unwrap();
    s.add_search_suggestions(&found);
    let titles: Vec<&str> = s.items.iter().map(|row| row.title.as_str()).collect();
    assert_eq!(titles, ["Search Fixture for \"rust two\"", "rust one"], "the typed text's own search shows once");
}

#[test]
fn nothing_is_sent_but_a_plain_search_of_the_default_engine() {
    let (mut p, _dir, server, fixture) = open();
    let queries = Queries::default();
    let page = format!("127.0.0.1:{}/page2.html", server.port());
    for text in ["example.com", &page, "https://x.test/?q=a b", "/usr/share/doc", "C:\\Users", "file:///etc/passwd", "", "  ", "w rust", "fx rust"] {
        assert!(p.omnibox().suggest_request(text, &queries, false).unwrap().is_none(), "{text:?}");
    }
    assert!(p.omnibox().suggest_request("rust", &queries, true).unwrap().is_none(), "a private window");

    p.prefs().set(&keys::SEARCH_SUGGESTIONS, &false).unwrap();
    assert!(p.omnibox().suggest_request("rust", &queries, false).unwrap().is_none(), "search suggestions turned off");
    p.prefs().set(&keys::SEARCH_SUGGESTIONS, &true).unwrap();
    assert!(p.omnibox().suggest_request("rust", &queries, false).unwrap().is_some());

    p.search_engines().set_suggest_url(&fixture, Some("file:///suggest?q={searchTerms}")).unwrap();
    assert!(p.omnibox().suggest_request("rust", &queries, false).unwrap().is_none(), "not a web address");
    p.search_engines().set_suggest_url(&fixture, None).unwrap();
    assert!(p.omnibox().suggest_request("rust", &queries, false).unwrap().is_none(), "no suggest URL");
    p.search_engines().set_default(&SearchEngineId("builtin:wikipedia".to_owned())).unwrap();
    assert!(p.omnibox().suggest_request("rust", &queries, false).unwrap().is_none(), "a built-in without one");

    assert_eq!(server.hits(), Vec::<String>::new());
}

#[test]
fn only_the_newest_of_quick_keystrokes_is_sent() {
    let (mut p, _dir, server, _) = open();
    let queries = Queries::default();
    let requests: Vec<_> = ["r", "ru", "rus"].map(|text| p.omnibox().suggest_request(text, &queries, false).unwrap().unwrap()).into();
    let results: Vec<Option<SearchSuggestions>> = std::thread::scope(|s| {
        let running: Vec<_> = requests.into_iter().map(|request| s.spawn(move || request.run())).collect();
        running.into_iter().map(|r| r.join().unwrap()).collect()
    });
    assert_eq!(results.iter().map(Option::is_some).collect::<Vec<_>>(), [false, false, true]);
    assert_eq!(rows(results[2].as_ref().unwrap()), ["rus one", "rus two"]);
    assert_eq!(server.hits(), ["/suggest"]);
}

#[test]
fn a_result_goes_stale_when_the_user_types_again() {
    let (mut p, _dir, _server, _) = open();
    let queries = Queries::default();
    let found = p.omnibox().suggest_request("rust", &queries, false).unwrap().unwrap().run().unwrap();
    assert!(found.is_current());
    assert!(p.omnibox().suggest_request("example.com", &queries, false).unwrap().is_none());
    assert!(!found.is_current(), "a URL typed after it, though that asks for nothing");
}

#[test]
fn a_cancelled_query_sends_nothing() {
    let (mut p, _dir, server, _) = open();
    let queries = Queries::default();
    let request = p.omnibox().suggest_request("rust", &queries, false).unwrap().unwrap();
    queries.cancel();
    assert!(request.run().is_none());
    assert_eq!(server.hits(), Vec::<String>::new());
}

#[test]
fn a_failed_fetch_is_no_suggestions() {
    let (mut p, _dir, server, fixture) = open();
    let queries = Queries::default();
    for path in ["/missing", "/index.html"] {
        p.search_engines().set_suggest_url(&fixture, Some(&format!("{}{path}?q={{searchTerms}}", server.origin()))).unwrap();
        let found = p.omnibox().suggest_request("rust", &queries, false).unwrap().unwrap().run().expect("still the newest");
        assert_eq!(rows(&found), Vec::<String>::new(), "{path}");
    }
    assert_eq!(server.hits(), ["/missing", "/index.html"]);
}
