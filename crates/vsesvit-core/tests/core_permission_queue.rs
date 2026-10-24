//! The permission prompt queue: what waits, what is answered at once, and what each answer,
//! navigation and settings change does to the requests in it.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, TimeSource};
use vsesvit_core::permissions::queue::{Answered, MAX_WAITING, Queue, Requested};
use vsesvit_core::permissions::{Answer, Decision, Origin, Permission, Setting, prompt};
use vsesvit_core::private::Browsing;
use vsesvit_core::{OpenOptions, Profile, Url};

use Permission::{Camera, Location, Microphone, Notifications};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-queue-{}", uuid::Uuid::new_v4())));
    let p = Profile::open(
        &dir.0,
        OpenOptions { time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000))), new_device_id: Some(DeviceId(1)), ..OpenOptions::default() },
    )
    .unwrap();
    (p, dir)
}

fn origin(s: &str) -> Option<Origin> {
    Origin::parse(s)
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

/// Requests are numbers; an answer is `(request, allowed)`.
fn pairs(answers: Vec<Answered<u32>>) -> Vec<(u32, bool)> {
    answers.into_iter().map(|a| (a.request, a.allowed)).collect()
}

fn waiting(requested: Requested<u32>) -> bool {
    matches!(requested, Requested::Waiting)
}

fn answered(requested: Requested<u32>) -> (u32, bool) {
    match requested {
        Requested::Answered(a) => (a.request, a.allowed),
        Requested::Waiting => panic!("the request waits"),
    }
}

#[test]
fn settings_answer_a_request_at_once_and_an_unset_one_waits() {
    let (mut p, _dir) = open();
    let a = origin("https://a.test");
    let mut q = Queue::new(Browsing::Normal);
    p.site_permissions().set(a.as_ref().unwrap(), Camera, Some(Setting::Allow)).unwrap();
    p.site_permissions().set(a.as_ref().unwrap(), Location, Some(Setting::Block)).unwrap();
    assert_eq!(answered(q.request(&mut p, a.clone(), vec![Camera], 1)), (1, true));
    assert_eq!(answered(q.request(&mut p, a.clone(), vec![Location], 2)), (2, false));
    assert!(waiting(q.request(&mut p, a, vec![Notifications], 3)));
    assert_eq!(q.len(), 1);
}

#[test]
fn camera_and_microphone_of_one_origin_share_a_prompt_even_one_already_shown() {
    let (mut p, _dir) = open();
    let a = origin("https://a.test");
    let mut q = Queue::new(Browsing::Normal);
    assert!(waiting(q.request(&mut p, a.clone(), vec![Camera], 1)));
    let first = q.next_prompt(&mut p);
    assert!(first.prompt.is_some() && first.changed);
    assert!(!q.next_prompt(&mut p).changed, "the same prompt is not news");

    assert!(waiting(q.request(&mut p, a.clone(), vec![Microphone], 2)));
    assert!(q.next_prompt(&mut p).changed, "the shown prompt now asks for both");
    assert!(waiting(q.request(&mut p, origin("https://b.test"), vec![Location], 3)));

    let answers = q.answer(&mut p, Answer::AllowWhileVisiting);
    assert_eq!(pairs(answers), [(1, true), (2, true)]);
    assert_eq!(q.len(), 1, "the other site's request is next");
    assert_eq!(p.site_permissions().get(&a.unwrap(), Microphone), Some(Setting::Allow));
}

#[test]
fn a_ninth_request_is_denied_while_eight_wait() {
    let (mut p, _dir) = open();
    let mut q = Queue::new(Browsing::Normal);
    for n in 0..MAX_WAITING as u32 {
        // One origin each, so none joins another's group.
        assert!(waiting(q.request(&mut p, origin(&format!("https://site{n}.test")), vec![Location], n)));
    }
    assert_eq!(answered(q.request(&mut p, origin("https://more.test"), vec![Location], 99)), (99, false));
    assert_eq!(q.len(), MAX_WAITING);
}

#[test]
fn a_dismissed_request_is_not_asked_again_by_the_same_document() {
    let (mut p, _dir) = open();
    let a = origin("https://a.test");
    let mut q = Queue::new(Browsing::Normal);
    assert!(waiting(q.request(&mut p, a.clone(), vec![Microphone], 1)));
    assert!(q.next_prompt(&mut p).prompt.is_some());
    assert_eq!(pairs(q.answer(&mut p, Answer::Dismiss)), [(1, false)]);

    assert_eq!(answered(q.request(&mut p, a.clone(), vec![Microphone], 2)), (2, false), "refused, not asked");
    assert!(waiting(q.request(&mut p, a.clone(), vec![Camera], 3)), "another permission is asked");

    let settled = q.committed(&url("https://a.test/next"), true);
    assert_eq!(pairs(settled.answers), [], "the same origin keeps its waiting request");
    assert!(waiting(q.request(&mut p, a, vec![Microphone], 4)), "a new document may ask again");
}

#[test]
fn never_allow_stores_a_block_and_dismisses_too() {
    let (mut p, _dir) = open();
    let a = origin("https://a.test");
    let mut q = Queue::new(Browsing::Normal);
    q.request(&mut p, a.clone(), vec![Location], 1);
    q.next_prompt(&mut p);
    assert_eq!(pairs(q.answer(&mut p, Answer::NeverAllow)), [(1, false)]);
    assert_eq!(p.site_permissions().get(a.as_ref().unwrap(), Location), Some(Setting::Block));
}

#[test]
fn allow_this_time_is_a_grant_that_ends_when_the_tab_leaves_the_site() {
    let (mut p, _dir) = open();
    let a = origin("https://a.test");
    let mut q = Queue::new(Browsing::Normal);
    q.request(&mut p, a.clone(), vec![Location], 1);
    q.next_prompt(&mut p);
    assert_eq!(pairs(q.answer(&mut p, Answer::AllowThisTime)), [(1, true)]);
    assert_eq!(q.grants(), [Location]);
    assert_eq!(p.site_permissions().get(a.as_ref().unwrap(), Location), None, "nothing is stored");
    assert_eq!(answered(q.request(&mut p, a.clone(), vec![Location], 2)), (2, true));

    q.committed(&url("https://elsewhere.test/"), false);
    assert!(q.grants().is_empty());
    assert!(waiting(q.request(&mut p, a, vec![Location], 3)));
}

#[test]
fn a_new_document_of_another_origin_denies_what_the_old_one_waits_for() {
    let (mut p, _dir) = open();
    let mut q = Queue::new(Browsing::Normal);
    q.request(&mut p, origin("https://a.test"), vec![Location], 1);
    q.request(&mut p, origin("https://b.test"), vec![Notifications], 2);
    q.next_prompt(&mut p);

    let same_document = q.committed(&url("https://elsewhere.test/#x"), false);
    assert!(same_document.answers.is_empty() && !same_document.prompt_changed);

    let settled = q.committed(&url("https://b.test/"), true);
    assert_eq!(pairs(settled.answers), [(1, false)]);
    assert!(settled.prompt_changed, "the shown prompt was a's");
    assert_eq!(q.len(), 1);
}

#[test]
fn a_request_without_an_origin_waits_but_never_survives_a_new_document() {
    let (mut p, _dir) = open();
    let mut q = Queue::new(Browsing::Normal);
    assert!(waiting(q.request(&mut p, None, vec![Location], 1)));
    let settled = q.committed(&url("https://a.test/"), true);
    assert_eq!(pairs(settled.answers), [(1, false)]);
}

#[test]
fn closing_the_tab_denies_everything_that_waits() {
    let (mut p, _dir) = open();
    let mut q = Queue::new(Browsing::Normal);
    q.request(&mut p, origin("https://a.test"), vec![Location], 1);
    q.request(&mut p, origin("https://b.test"), vec![Location], 2);
    assert_eq!(pairs(q.close()), [(1, false), (2, false)]);
    assert!(q.is_empty());
}

#[test]
fn a_change_of_settings_answers_what_it_decides_and_narrows_the_shown_prompt() {
    let (mut p, _dir) = open();
    let a = origin("https://a.test");
    let mut q = Queue::new(Browsing::Normal);
    q.request(&mut p, a.clone(), vec![Camera], 1);
    q.request(&mut p, a.clone(), vec![Microphone], 2);
    q.next_prompt(&mut p);

    p.site_permissions().set(a.as_ref().unwrap(), Camera, Some(Setting::Allow)).unwrap();
    let settled = q.settle_decided(&mut p);
    assert!(settled.answers.is_empty(), "the group still asks for the microphone");
    assert!(settled.prompt_changed);
    assert_eq!(q.next_prompt(&mut p).prompt, Some(prompt(a.as_ref(), &[Microphone])));

    p.site_permissions().set(a.as_ref().unwrap(), Microphone, Some(Setting::Block)).unwrap();
    let settled = q.settle_decided(&mut p);
    assert_eq!(pairs(settled.answers), [(1, true), (2, false)]);
    assert!(settled.prompt_changed);
    assert!(q.is_empty());
}

#[test]
fn the_next_prompt_answers_the_groups_another_tab_already_decided() {
    let (mut p, _dir) = open();
    let (a, b) = (origin("https://a.test"), origin("https://b.test"));
    let mut q = Queue::new(Browsing::Normal);
    q.request(&mut p, a.clone(), vec![Location], 1);
    q.request(&mut p, b, vec![Location], 2);
    p.site_permissions().set(a.as_ref().unwrap(), Location, Some(Setting::Allow)).unwrap();

    let next = q.next_prompt(&mut p);
    assert_eq!(pairs(next.answers), [(1, true)]);
    assert!(next.prompt.is_some() && next.changed);
}

#[test]
fn a_withdrawn_prompt_shows_again_from_the_start() {
    let (mut p, _dir) = open();
    let mut q = Queue::new(Browsing::Normal);
    q.request(&mut p, origin("https://a.test"), vec![Location], 1);
    assert!(q.next_prompt(&mut p).changed);
    q.prompt_hidden();
    let again = q.next_prompt(&mut p);
    assert!(again.changed && again.prompt.is_some());
    assert_eq!(q.len(), 1, "nothing was denied");
}

#[test]
fn a_private_tab_decides_by_the_private_settings() {
    let (mut p, _dir) = open();
    let a = origin("https://a.test");
    let mut q = Queue::new(Browsing::Private);
    assert_eq!(answered(q.request(&mut p, a.clone(), vec![Notifications], 1)), (1, false), "blocked without asking in private");
    q.request(&mut p, a.clone(), vec![Location], 2);
    q.next_prompt(&mut p);
    q.answer(&mut p, Answer::AllowWhileVisiting);
    assert_eq!(p.site_permissions().get(a.as_ref().unwrap(), Location), None, "the stored settings are untouched");
    assert_eq!(q.decide(&mut p, a.as_ref(), &[Location]), Decision::Allow);
}
