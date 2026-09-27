//! Search engines: built-ins, overlays, custom engines, default resolution.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, Record, Seq, Stamp, TimeSource};
use vsesvit_core::search::{EngineEdit, EngineRecord, NavTarget, SearchEngineId, UrlTemplate};
use vsesvit_core::sync::Kind;
use vsesvit_core::{Error, OpenOptions, Profile};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-se-{}", uuid::Uuid::new_v4())));
    let p = Profile::open(
        &dir.0,
        OpenOptions {
            time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000))),
            new_device_id: Some(DeviceId(2)),
            ..OpenOptions::default()
        },
    )
    .unwrap();
    (p, dir)
}

fn id(s: &str) -> SearchEngineId {
    SearchEngineId(s.to_owned())
}

fn records(p: &mut Profile) -> Vec<EngineRecord> {
    p.sync().changes_since(Kind::SearchEngines, Seq::ZERO, usize::MAX).unwrap().records.iter().map(|w| serde_json::from_slice(&w.body).unwrap()).collect()
}

#[test]
fn builtins_ship_in_code_and_are_never_seeded() {
    let (mut p, _dir) = open();
    let list = p.search_engines().list().unwrap();
    assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Bing", "DuckDuckGo", "Google", "Wikipedia"]);
    assert!(list.iter().all(|e| e.builtin));
    assert_eq!(list.iter().find(|e| e.name == "Wikipedia").unwrap().keyword.as_deref(), Some("w"));
    assert_eq!(p.search_engines().default_engine().unwrap().id, id("builtin:ddg"));
    assert!(records(&mut p).is_empty());
}

#[test]
fn editing_a_builtin_stores_only_the_edited_field() {
    let (mut p, _dir) = open();
    p.search_engines().update(&id("builtin:wikipedia"), EngineEdit { keyword: Some(Some("wiki".into())), ..EngineEdit::default() }).unwrap();
    let list = p.search_engines().list().unwrap();
    let wiki = list.iter().find(|e| e.id == id("builtin:wikipedia")).unwrap();
    assert_eq!(wiki.keyword.as_deref(), Some("wiki"));
    assert_eq!(wiki.name, "Wikipedia");
    let recs = records(&mut p);
    assert_eq!(recs.len(), 1);
    let Record::Live(f) = &recs[0].state else { panic!("live") };
    assert_ne!(f.keyword.at, Stamp::ZERO);
    assert_eq!(f.name.at, Stamp::ZERO, "unedited fields stay at the zero stamp and read through to code");
    assert_eq!(f.search_url.at, Stamp::ZERO);
    assert_eq!(p.omnibox().resolve("wiki rust").unwrap().unwrap().url().as_str(), "https://en.wikipedia.org/wiki/Special:Search?search=rust");
    assert!(matches!(p.omnibox().resolve("w rust").unwrap().unwrap(), NavTarget::Search { engine, .. } if engine == id("builtin:ddg")));

    let upto = p.sync().changes_since(Kind::SearchEngines, Seq::ZERO, usize::MAX).unwrap().upto;
    p.search_engines().update(&id("builtin:wikipedia"), EngineEdit { keyword: Some(Some("wiki".into())), name: Some("Wikipedia".into()), ..EngineEdit::default() }).unwrap();
    assert_eq!(p.sync().changes_since(Kind::SearchEngines, Seq::ZERO, usize::MAX).unwrap().upto, upto, "unchanged values mint nothing");
    assert!(matches!(p.search_engines().update(&id("nope"), EngineEdit::default()), Err(Error::NotFound)));
}

#[test]
fn custom_engines_and_removal() {
    let (mut p, _dir) = open();
    let mine = p.search_engines().add("Crates", Some("c"), UrlTemplate("https://crates.io/search?q={searchTerms}".into())).unwrap();
    assert!(!mine.is_builtin());
    let list = p.search_engines().list().unwrap();
    assert_eq!(list.len(), 5);
    let e = list.iter().find(|e| e.id == mine).unwrap();
    assert_eq!((e.name.as_str(), e.keyword.as_deref(), e.builtin), ("Crates", Some("c"), false));
    assert_eq!(p.omnibox().resolve("c serde").unwrap().unwrap().url().as_str(), "https://crates.io/search?q=serde");

    p.search_engines().set_default(&mine).unwrap();
    assert_eq!(p.search_engines().default_engine().unwrap().id, mine);
    assert_eq!(p.omnibox().resolve("hello").unwrap().unwrap().url().as_str(), "https://crates.io/search?q=hello");

    p.search_engines().remove(&mine).unwrap();
    assert_eq!(p.search_engines().list().unwrap().len(), 4);
    assert_eq!(p.search_engines().default_engine().unwrap().id, id("builtin:ddg"), "a deleted default falls back to the first live built-in");
    p.search_engines().remove(&mine).unwrap();
    assert!(matches!(p.search_engines().set_default(&mine), Err(Error::NotFound)));
    assert!(matches!(p.search_engines().update(&mine, EngineEdit::default()), Err(Error::NotFound)));
    assert!(matches!(p.search_engines().remove(&id("ghost")), Err(Error::NotFound)));

    p.search_engines().remove(&id("builtin:ddg")).unwrap();
    assert_eq!(p.search_engines().default_engine().unwrap().id, id("builtin:google"));
    let recs = records(&mut p);
    assert_eq!(recs.len(), 2);
    assert!(recs.iter().all(|r| r.state.is_deleted()));

    for b in ["builtin:google", "builtin:bing", "builtin:wikipedia"] {
        p.search_engines().remove(&id(b)).unwrap();
    }
    assert!(p.search_engines().list().unwrap().is_empty());
    assert!(matches!(p.search_engines().default_engine(), Err(Error::NotFound)));
    assert_eq!(p.omnibox().resolve("example.com").unwrap().unwrap().url().as_str(), "https://example.com/");
    assert!(p.omnibox().resolve("just words").unwrap().is_none());
}
