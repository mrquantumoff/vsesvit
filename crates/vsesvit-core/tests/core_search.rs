//! Search engines: built-ins, overlays, custom engines, default resolution.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, Extra, Hlc, JsonText, Lww, Record, Seq, Stamp, TimeSource};
use vsesvit_core::search::{EngineFields, EngineForm, EngineRecord, FormError, FormField, NavTarget, SearchEngine, SearchEngineId, UrlTemplate};
use vsesvit_core::sync::{Kind, WireRecord};
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

fn engine(p: &mut Profile, id: &SearchEngineId) -> SearchEngine {
    p.search_engines().list().unwrap().into_iter().find(|e| &e.id == id).unwrap()
}

/// The editor's form for `id` with `edit` applied.
fn edited(p: &mut Profile, id: &SearchEngineId, edit: impl FnOnce(&mut EngineForm)) -> EngineForm {
    let mut form = EngineForm::of(&engine(p, id));
    edit(&mut form);
    form
}

fn form(name: &str, keyword: &str, url: &str) -> EngineForm {
    EngineForm { name: name.into(), keyword: keyword.into(), url: url.into() }
}

/// A tombstone for `id` from another device, applied through sync.
fn removed_elsewhere(p: &mut Profile, id: &SearchEngineId) {
    let at = Stamp { hlc: Hlc((1_780_000_000_000 + 60_000) << 16), device: DeviceId(9) };
    let gone = EngineRecord { id: id.clone(), state: Record::Tombstone(at) };
    p.sync().apply(vec![WireRecord { kind: Kind::SearchEngines, id: id.0.clone(), body: serde_json::to_vec(&gone).unwrap() }]).unwrap();
}

#[test]
fn editing_a_builtin_stores_only_the_edited_field() {
    let (mut p, _dir) = open();
    let wiki = id("builtin:wikipedia");
    let shortcut = edited(&mut p, &wiki, |f| f.keyword = "wiki".into());
    assert_eq!(shortcut.url, "https://en.wikipedia.org/wiki/Special:Search?search=%s", "the editor shows %s for the terms");
    p.search_engines().save(Some(&wiki), &shortcut).unwrap();
    let shown = engine(&mut p, &wiki);
    assert_eq!(shown.keyword.as_deref(), Some("wiki"));
    assert_eq!(shown.name, "Wikipedia");
    assert_eq!(shown.search_url.0, "https://en.wikipedia.org/wiki/Special:Search?search={searchTerms}");
    let recs = records(&mut p);
    assert_eq!(recs.len(), 1);
    let Record::Live(f) = &recs[0].state else { panic!("live") };
    assert_ne!(f.keyword.at, Stamp::ZERO);
    assert_eq!(f.name.at, Stamp::ZERO, "unedited fields stay at the zero stamp and read through to code");
    assert_eq!(f.search_url.at, Stamp::ZERO);
    assert_eq!(p.omnibox().resolve("wiki rust").unwrap().unwrap().url().as_str(), "https://en.wikipedia.org/wiki/Special:Search?search=rust");
    assert!(matches!(p.omnibox().resolve("w rust").unwrap().unwrap(), NavTarget::Search { engine, .. } if engine == id("builtin:ddg")));

    let upto = p.sync().changes_since(Kind::SearchEngines, Seq::ZERO, usize::MAX).unwrap().upto;
    let same = edited(&mut p, &wiki, |_| {});
    p.search_engines().save(Some(&wiki), &same).unwrap();
    assert_eq!(p.sync().changes_since(Kind::SearchEngines, Seq::ZERO, usize::MAX).unwrap().upto, upto, "unchanged values mint nothing");
    assert!(matches!(p.search_engines().save(Some(&id("nope")), &form("Nope", "nope", "https://nope.example/?q=%s")), Err(Error::NotFound)));
}

#[test]
fn custom_engines_and_removal() {
    let (mut p, _dir) = open();
    let mine = p.search_engines().save(None, &form(" Crates ", " c ", " https://crates.io/search?q=%s ")).unwrap();
    assert!(!mine.is_builtin());
    let list = p.search_engines().list().unwrap();
    assert_eq!(list.len(), 5);
    let e = list.iter().find(|e| e.id == mine).unwrap();
    assert_eq!((e.name.as_str(), e.keyword.as_deref(), e.builtin), ("Crates", Some("c"), false));
    assert_eq!(e.search_url.0, "https://crates.io/search?q={searchTerms}");
    assert_eq!(e.suggest_url, None);
    assert_eq!(p.omnibox().resolve("c serde").unwrap().unwrap().url().as_str(), "https://crates.io/search?q=serde");

    p.search_engines().set_default(&mine).unwrap();
    assert_eq!(p.search_engines().default_engine().unwrap().id, mine);
    assert_eq!(p.omnibox().resolve("hello").unwrap().unwrap().url().as_str(), "https://crates.io/search?q=hello");
    assert!(matches!(p.search_engines().remove(&mine), Err(Error::RemoveDefaultEngine)), "the default stays");

    p.search_engines().set_default(&id("builtin:bing")).unwrap();
    p.search_engines().remove(&mine).unwrap();
    assert_eq!(p.search_engines().list().unwrap().len(), 4);
    p.search_engines().remove(&mine).unwrap();
    assert!(matches!(p.search_engines().set_default(&mine), Err(Error::NotFound)));
    assert!(matches!(p.search_engines().save(Some(&mine), &form("Crates", "c", "https://crates.io/?q=%s")), Err(Error::NotFound)));
    assert!(matches!(p.search_engines().remove(&id("ghost")), Err(Error::NotFound)));

    p.search_engines().remove(&id("builtin:ddg")).unwrap();
    let recs = records(&mut p);
    assert_eq!(recs.len(), 2);
    assert!(recs.iter().all(|r| r.state.is_deleted()));

    for b in ["builtin:google", "builtin:wikipedia"] {
        p.search_engines().remove(&id(b)).unwrap();
    }
    assert!(matches!(p.search_engines().remove(&id("builtin:bing")), Err(Error::RemoveDefaultEngine)), "the last engine is the default");
    assert_eq!(p.search_engines().list().unwrap().len(), 1);
}

#[test]
fn a_default_removed_on_another_device_falls_back() {
    let (mut p, _dir) = open();
    let mine = p.search_engines().save(None, &form("Crates", "c", "crates.io/search?q=%s")).unwrap();
    p.search_engines().set_default(&mine).unwrap();
    removed_elsewhere(&mut p, &mine);
    assert_eq!(p.search_engines().default_engine().unwrap().id, id("builtin:ddg"), "the first live built-in");

    removed_elsewhere(&mut p, &id("builtin:ddg"));
    assert_eq!(p.search_engines().default_engine().unwrap().id, id("builtin:google"));

    for b in ["builtin:google", "builtin:bing", "builtin:wikipedia"] {
        removed_elsewhere(&mut p, &id(b));
    }
    assert!(p.search_engines().list().unwrap().is_empty());
    assert!(matches!(p.search_engines().default_engine(), Err(Error::NotFound)));
    assert_eq!(p.omnibox().resolve("example.com").unwrap().unwrap().url().as_str(), "https://example.com/");
    assert!(p.omnibox().resolve("just words").unwrap().is_none());
}

#[test]
fn the_editor_form_is_checked_field_by_field() {
    let (mut p, _dir) = open();
    let check = |p: &mut Profile, editing: Option<&str>, f: EngineForm| p.search_engines().check(editing.map(id).as_ref(), &f).unwrap();
    let ok = "https://example.com/?q=%s";
    assert_eq!(check(&mut p, None, form("Example", "ex", ok)), []);
    assert_eq!(check(&mut p, None, form("Example", "ex", "https://example.com/search/{searchTerms}")), []);
    assert_eq!(check(&mut p, None, EngineForm::default()), [FormError::NoName, FormError::NoKeyword, FormError::NoUrl]);
    assert_eq!(check(&mut p, None, form("  ", "e x", ok)), [FormError::NoName, FormError::KeywordHasSpace]);
    assert_eq!(check(&mut p, None, form("Example", "w", ok)), [FormError::KeywordTaken], "Wikipedia has w");
    assert_eq!(check(&mut p, Some("builtin:wikipedia"), form("Example", "w", ok)), [], "its own shortcut");
    assert_eq!(check(&mut p, None, form("Example", "W", ok)), [], "shortcuts match as typed, like the address bar");
    assert_eq!(check(&mut p, None, form("Example", "ex", "https://example.com/")), [FormError::NoTerms]);
    for not_web in ["ftp://example.com/?q=%s", "javascript:alert(%s)", "file:///search/%s", "https:///?q=%s", "not a url %s"] {
        assert_eq!(check(&mut p, None, form("Example", "ex", not_web)), [FormError::NotWebAddress], "{not_web}");
    }
    assert!([FormError::NoName, FormError::NoKeyword, FormError::NoUrl].iter().all(|e| e.is_blank()));
    assert_eq!(FormError::KeywordTaken.field(), FormField::Keyword);
    assert_eq!(FormError::NoTerms.field(), FormField::Url);

    let refused = p.search_engines().save(None, &form("Example", "w", "example.com"));
    assert!(matches!(refused, Err(Error::EngineForm(FormError::KeywordTaken))), "the first problem: {refused:?}");
    assert_eq!(p.search_engines().list().unwrap().len(), 4, "nothing saved");
}

#[test]
fn a_new_shortcut_works_in_the_address_bar_at_once() {
    let (mut p, _dir) = open();
    let docs = p.search_engines().save(None, &form("Docs", "rs", "docs.rs/releases/search?query=%s")).unwrap();
    assert_eq!(engine(&mut p, &docs).search_url.0, "https://docs.rs/releases/search?query={searchTerms}", "https:// when no scheme is typed");
    let target = p.omnibox().resolve("rs serde json").unwrap().unwrap();
    assert!(matches!(&target, NavTarget::Search { engine, .. } if *engine == docs));
    assert_eq!(target.url().as_str(), "https://docs.rs/releases/search?query=serde+json");
    let first = p.omnibox().suggest("rs serde", 5, false).unwrap().items.remove(0);
    assert_eq!(first.title, "Search Docs for \"serde\"");

    let renamed = edited(&mut p, &docs, |f| f.keyword = "docs".into());
    p.search_engines().save(Some(&docs), &renamed).unwrap();
    assert!(matches!(p.omnibox().resolve("docs serde").unwrap().unwrap(), NavTarget::Search { engine, .. } if engine == docs));
    assert!(matches!(p.omnibox().resolve("rs serde").unwrap().unwrap(), NavTarget::Search { engine, .. } if engine == id("builtin:ddg")));
}

#[test]
fn editing_a_builtin_back_to_a_stale_stored_value_takes_effect() {
    let (mut p, _dir) = open();
    // A rename made on an older release, whose Bing searched old.example: the untouched
    // search url is stored with that release's value at the zero stamp.
    let old = UrlTemplate("https://old.example/?q={searchTerms}".into());
    let bing = id("builtin:bing");
    let renamed = EngineRecord {
        id: bing.clone(),
        state: Record::Live(EngineFields {
            name: Lww::new("Old Bing".into(), Stamp { hlc: Hlc(1_779_000_000_000 << 16), device: DeviceId(9) }),
            keyword: Lww::new(Some("b".into()), Stamp::ZERO),
            search_url: Lww::new(old.clone(), Stamp::ZERO),
            suggest_url: Lww::new(None, Stamp::ZERO),
            extra: Extra::default(),
        }),
    };
    let wire = WireRecord { kind: Kind::SearchEngines, id: bing.0.clone(), body: serde_json::to_vec(&renamed).unwrap() };
    p.sync().apply(vec![wire]).unwrap();
    assert_eq!(engine(&mut p, &bing).search_url.0, "https://www.bing.com/search?q={searchTerms}", "the zero-stamped url reads through");

    let form = edited(&mut p, &bing, |f| f.url = "https://old.example/?q=%s".into());
    p.search_engines().save(Some(&bing), &form).unwrap();
    assert_eq!(engine(&mut p, &bing).search_url, old);
    assert_eq!(engine(&mut p, &bing).name, "Old Bing");
    let Record::Live(f) = &records(&mut p)[0].state else { panic!("live") };
    assert_ne!(f.search_url.at, Stamp::ZERO);
    assert_eq!(f.suggest_url.at, Stamp::ZERO, "the suggest url still reads through");
}

#[test]
fn a_synced_engine_reads_back_whole_and_its_removal_keeps_a_tombstone() {
    let (mut p, _dir) = open();
    let at = |n: u64| Stamp { hlc: Hlc((1_779_000_000_000 << 16) + n), device: DeviceId(9) };
    let mine = id("synced-crates");
    let mut extra = Extra::default();
    extra.insert("icon".into(), Lww::new(JsonText::from_value(&serde_json::json!("crates.png")), at(5)));
    let live = EngineRecord {
        id: mine.clone(),
        state: Record::Live(EngineFields {
            name: Lww::new("Crates".into(), at(1)),
            keyword: Lww::new(Some("c".into()), at(2)),
            search_url: Lww::new(UrlTemplate("https://crates.io/search?q={searchTerms}".into()), at(3)),
            suggest_url: Lww::new(Some(UrlTemplate("https://crates.io/suggest?q={searchTerms}".into())), at(4)),
            extra,
        }),
    };
    let wire = WireRecord { kind: Kind::SearchEngines, id: mine.0.clone(), body: serde_json::to_vec(&live).unwrap() };
    p.sync().apply(vec![wire]).unwrap();
    assert_eq!(records(&mut p), [live]);

    p.search_engines().remove(&mine).unwrap();
    let recs = records(&mut p);
    let [EngineRecord { id: gone, state: Record::Tombstone(deleted) }] = recs.as_slice() else { panic!("one tombstone: {recs:?}") };
    assert_eq!(gone, &mine);
    assert!(*deleted > at(5));
}
