//! History through the API: visits, titles, search, deletion directives, the 64-visit cap.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, Seq, TimeSource};
use vsesvit_core::history::{PageRecord, Transition, MAX_VISITS};
use vsesvit_core::sync::Kind;
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
