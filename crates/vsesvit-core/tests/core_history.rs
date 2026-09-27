//! History through the API: visits, titles, search, deletion directives, the 64-visit cap.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, Extra, Lww, Seq, Stamp, TimeSource};
use vsesvit_core::history::{PageRecord, Transition, Visit, MAX_VISITS};
use vsesvit_core::prefs::{keys, Theme};
use vsesvit_core::sync::{Kind, WireRecord};
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const T0: u64 = 1_780_000_000_000;

fn open() -> (Profile, Rc<Cell<u64>>, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-hist-{}", uuid::Uuid::new_v4())));
    let time = Rc::new(Cell::new(T0));
    let p = Profile::open(
        &dir.0,
        OpenOptions { time: TimeSource::Manual(time.clone()), new_device_id: Some(DeviceId(3)), ..OpenOptions::default() },
    )
    .unwrap();
    (p, time, dir)
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn pages(p: &mut Profile) -> Vec<PageRecord> {
    p.sync().changes_since(Kind::HistoryPages, Seq::ZERO, usize::MAX).unwrap().records.iter().map(|w| serde_json::from_slice(&w.body).unwrap()).collect()
}

#[test]
fn visits_titles_and_search() {
    let (mut p, time, _dir) = open();
    let a = url("https://www.example.com/docs");
    p.history().record_visit(&a, Transition::Typed).unwrap();
    time.set(T0 + 1000);
    p.history().record_visit(&a, Transition::Link).unwrap();
    p.history().set_title(&a, "Example Docs").unwrap();
    p.history().record_visit(&url("about:blank"), Transition::Link).unwrap();
    p.history().set_title(&url("https://never.example/"), "ignored").unwrap();

    let hits = p.history().search("example.com/do", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].title.as_str(), hits[0].visit_count, hits[0].typed_count, hits[0].last_visit_ms), ("Example Docs", 2, 1, (T0 + 1000) as i64));
    assert_eq!(p.history().search("https://www.example.com", 10).unwrap().len(), 1, "scheme and www are ignored");
    assert_eq!(p.history().search("ample doc", 10).unwrap().len(), 1, "title substring");
    assert!(p.history().search("nothing", 10).unwrap().is_empty());
    assert!(p.history().search("%", 10).unwrap().is_empty(), "LIKE wildcards are escaped");

    let all = pages(&mut p);
    assert_eq!(all.len(), 1, "about: pages are not history");
    assert_eq!(all[0].visits.len(), 2);
    assert_eq!(all[0].title.v, "Example Docs");

    let visits = p.history().visits_between(0, i64::MAX, 10).unwrap();
    assert_eq!(visits.len(), 2);
    assert_eq!(visits[0].1.at_ms, (T0 + 1000) as i64, "newest first");
    assert_eq!(visits[1].1.transition, Transition::Typed);
    assert_eq!(p.history().visits_between(0, T0 as i64 + 1, 10).unwrap().len(), 1);
}

#[test]
fn same_millisecond_is_one_visit_and_unchanged_title_mints_nothing() {
    let (mut p, _time, _dir) = open();
    let a = url("https://a.example/");
    p.history().record_visit(&a, Transition::Link).unwrap();
    let upto = p.sync().changes_since(Kind::HistoryPages, Seq::ZERO, usize::MAX).unwrap().upto;
    p.history().record_visit(&a, Transition::Link).unwrap();
    p.history().set_title(&a, "").unwrap();
    assert_eq!(p.sync().changes_since(Kind::HistoryPages, Seq::ZERO, usize::MAX).unwrap().upto, upto);
    assert_eq!(pages(&mut p)[0].visits.len(), 1);
}

#[test]
fn a_page_keeps_only_its_newest_visits() {
    let (mut p, time, _dir) = open();
    let a = url("https://a.example/");
    for i in 0..(MAX_VISITS as u64 + 10) {
        time.set(T0 + i);
        p.history().record_visit(&a, Transition::Link).unwrap();
    }
    let page = &pages(&mut p)[0];
    assert_eq!(page.visits.len(), MAX_VISITS);
    assert_eq!(page.visits.first().unwrap().at_ms, (T0 + 10) as i64);
    assert_eq!(p.history().search("a.example", 1).unwrap()[0].visit_count as usize, MAX_VISITS);
}

#[test]
fn delete_url_and_delete_range() {
    let (mut p, time, _dir) = open();
    let a = url("https://a.example/");
    let b = url("https://b.example/");
    for i in 0..5u64 {
        time.set(T0 + i * 1000);
        p.history().record_visit(&a, Transition::Link).unwrap();
        p.history().record_visit(&b, Transition::Link).unwrap();
    }
    p.history().delete_url(&a).unwrap();
    assert!(p.history().search("a.example", 10).unwrap().is_empty());
    assert_eq!(pages(&mut p).len(), 1);
    // a later visit is outside the directive's range and survives
    time.set(T0 + 10_000);
    p.history().record_visit(&a, Transition::Link).unwrap();
    assert_eq!(p.history().search("a.example", 10).unwrap()[0].visit_count, 1);

    p.history().delete_range(T0 as i64 + 1000, T0 as i64 + 3000).unwrap();
    let b_page = pages(&mut p).into_iter().find(|pg| pg.url == b).unwrap();
    assert_eq!(b_page.visits.iter().map(|v| v.at_ms - T0 as i64).collect::<Vec<_>>(), [0, 4000]);
    assert_eq!(p.history().search("b.example", 10).unwrap()[0].visit_count, 2);

    let directives = p.sync().changes_since(Kind::HistoryDeletions, Seq::ZERO, usize::MAX).unwrap().records;
    assert_eq!(directives.len(), 2);
    p.history().delete_range(0, i64::MAX).unwrap();
    assert!(pages(&mut p).is_empty());
    assert!(p.history().visits_between(0, i64::MAX, 10).unwrap().is_empty());
}

fn visit(at_ms: i64, device: u64, transition: Transition) -> Visit {
    Visit { at_ms, device: DeviceId(device), transition }
}

fn page_wire(page: &PageRecord) -> WireRecord {
    WireRecord { kind: Kind::HistoryPages, id: page.url.to_string(), body: serde_json::to_vec(page).unwrap() }
}

/// The table keys visits by `(url, at_ms, device)`. A record from a peer (buggy, hostile,
/// or a copied profile) may hold two visits for one such key that differ only in
/// transition. Storing it must not fail, let alone roll back the batch it arrived in.
#[test]
fn a_record_with_two_transitions_at_one_visit_key_is_stored_as_one_visit_and_repaired() {
    let (mut p, _time, _dir) = open();
    let a = url("https://a.example/");
    let visits: BTreeSet<Visit> = [visit(100, 7, Transition::Link), visit(100, 7, Transition::Typed), visit(50, 7, Transition::Link)].into();
    let doubled = PageRecord { url: a.clone(), title: Lww::new(String::new(), Stamp::ZERO), visits, extra: Extra::default() };
    let good = PageRecord {
        url: url("https://b.example/"),
        title: Lww::new(String::new(), Stamp::ZERO),
        visits: [visit(1, 7, Transition::Link)].into(),
        extra: Extra::default(),
    };
    let cursor = p.sync().changes_since(Kind::HistoryPages, Seq::ZERO, usize::MAX).unwrap().upto;

    let report = p.sync().apply(vec![page_wire(&doubled), page_wire(&good)]).unwrap();
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    assert_eq!(report.merged, 2);
    assert!(report.changed.history);
    let stored = pages(&mut p);
    assert_eq!(stored.len(), 2, "the other record in the batch is applied too");
    let page = stored.iter().find(|pg| pg.url == a).unwrap();
    assert_eq!(page.visits, [visit(50, 7, Transition::Link), visit(100, 7, Transition::Typed)].into(), "one visit per key, the greatest transition");
    assert_eq!(p.history().search("a.example", 1).unwrap()[0].typed_count, 1);

    // The stored form differs from the incoming record, so it is re-uploaded to repair the
    // server copy; the repaired record then applies as a no-op everywhere.
    let pending = p.sync().changes_since(Kind::HistoryPages, cursor, usize::MAX).unwrap();
    let repaired: Vec<PageRecord> = pending.records.iter().map(|w| serde_json::from_slice(&w.body).unwrap()).collect();
    assert!(repaired.iter().any(|pg| pg == page), "the canonical record is pending upload: {repaired:?}");
    let report = p.sync().apply(vec![page_wire(page)]).unwrap();
    assert_eq!((report.merged, report.unchanged), (0, 1));
    assert!(p.sync().changes_since(Kind::HistoryPages, pending.upto, usize::MAX).unwrap().records.is_empty(), "no echo");
}

/// A copied profile directory shares its device id. Two such devices visiting the same
/// page in the same millisecond with different transitions produce the duplicate key
/// through the ordinary API, not through a crafted record.
#[test]
fn copied_profiles_visiting_in_the_same_millisecond_still_merge() {
    let (mut a, _ta, _da) = open();
    let (mut b, _tb, _db) = open(); // same DeviceId(3), same clock start
    let u = url("https://a.example/");
    a.history().record_visit(&u, Transition::Link).unwrap();
    b.history().record_visit(&u, Transition::Typed).unwrap();
    b.prefs().set(&keys::THEME, &Theme::Dark).unwrap();
    let mut batch = b.sync().changes_since(Kind::HistoryPages, Seq::ZERO, usize::MAX).unwrap().records;
    batch.extend(b.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap().records);

    let report = a.sync().apply(batch).unwrap();
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    assert_eq!(a.prefs().get(&keys::THEME), Theme::Dark, "an unrelated record in the same batch survives");
    let page = &pages(&mut a)[0];
    assert_eq!(page.visits.len(), 1);
    assert_eq!(page.visits.first().unwrap().transition, Transition::Typed);
    assert_eq!(a.history().search("a.example", 1).unwrap()[0].visit_count, 1);
    // b, once it holds a's record, agrees byte for byte
    let from_a = a.sync().changes_since(Kind::HistoryPages, Seq::ZERO, usize::MAX).unwrap().records;
    b.sync().apply(from_a.clone()).unwrap();
    assert_eq!(b.sync().changes_since(Kind::HistoryPages, Seq::ZERO, usize::MAX).unwrap().records, from_a);
}
