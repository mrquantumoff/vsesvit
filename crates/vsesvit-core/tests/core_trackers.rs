//! Tracking protection: what the bundled list blocks at each level, site exceptions and how they
//! sync, and the declarativeNetRequest rules Linux compiles.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::{Value, json};
use vsesvit_core::crdt::{DeviceId, Seq, TimeSource};
use vsesvit_core::permissions::{Capturing, Origin, Permission, Setting, site_rows};
use vsesvit_core::prefs::keys;
use vsesvit_core::sync::Kind;
use vsesvit_core::trackers::{self, Category, TrackerList, TrackingProtection};
use vsesvit_core::{OpenOptions, Profile};

use TrackingProtection::{Off, Standard, Strict};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open(device: u64) -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-trackers-{}", uuid::Uuid::new_v4())));
    let p = Profile::open(
        &dir.0,
        OpenOptions {
            time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000 + device))),
            new_device_id: Some(DeviceId(device)),
            ..OpenOptions::default()
        },
    )
    .unwrap();
    (p, dir)
}

fn origin(s: &str) -> Origin {
    Origin::parse(s).unwrap()
}

#[test]
fn standard_blocks_advertising_and_analytics_on_other_sites() {
    let list = TrackerList::bundled();
    assert_eq!(list.blocks(Standard, "news.example", "www.google-analytics.com"), Some("google-analytics.com"));
    assert_eq!(list.blocks(Standard, "news.example", "stats.g.doubleclick.net"), Some("doubleclick.net"));
    assert_eq!(list.blocks(Standard, "news.example", "doubleclick.net"), Some("doubleclick.net"));
    assert_eq!(list.blocks(Standard, "news.example", "analytics.google.com"), Some("analytics.google.com"));
    assert_eq!(list.blocks(Standard, "news.example", "www.google.com"), None, "a company's site is no tracker");
    assert_eq!(list.blocks(Standard, "news.example", "notdoubleclick.net"), None, "domains match whole labels");
    assert_eq!(list.blocks(Standard, "news.example", "news.example"), None);
    assert_eq!(list.blocks(Off, "news.example", "www.google-analytics.com"), None);
}

#[test]
fn a_company_s_trackers_load_on_its_own_sites() {
    let list = TrackerList::bundled();
    assert_eq!(list.blocks(Standard, "www.youtube.com", "googleads.g.doubleclick.net"), None);
    assert_eq!(list.blocks(Standard, "mail.google.com", "www.google-analytics.com"), None);
    assert_eq!(list.blocks(Standard, "www.google-analytics.com", "www.google-analytics.com"), None, "a tracker's own pages");
    assert_eq!(list.blocks(Strict, "www.facebook.com", "connect.facebook.net"), None);
    assert_eq!(list.blocks(Strict, "www.instagram.com", "www.facebook.com"), None);
    assert_eq!(list.blocks(Standard, "www.bing.com", "bat.bing.com"), None);
}

#[test]
fn strict_also_blocks_social_trackers() {
    let list = TrackerList::bundled();
    assert_eq!(list.blocks(Standard, "news.example", "connect.facebook.net"), None);
    assert_eq!(list.blocks(Strict, "news.example", "connect.facebook.net"), Some("facebook.net"));
    assert_eq!(list.blocks(Strict, "news.example", "platform.twitter.com"), Some("twitter.com"));
    assert_eq!(list.blocks(Strict, "news.example", "www.google-analytics.com"), Some("google-analytics.com"));
}

#[test]
fn an_added_tracker_is_blocked_everywhere_but_on_itself() {
    let list = TrackerList::bundled().clone().with_tracker("localhost", Category::Analytics);
    assert_eq!(list.blocks(Standard, "127.0.0.1", "localhost"), Some("localhost"));
    assert_eq!(list.blocks(Standard, "localhost", "localhost"), None);
    assert_eq!(list.blocks(Standard, "news.example", "www.google-analytics.com"), Some("google-analytics.com"), "the bundled list stays");
}

#[test]
fn a_bad_list_is_an_error() {
    assert!(TrackerList::parse("{}").is_err());
    assert!(TrackerList::parse(r#"{"entities": {"X": {"cookies": ["x.example"]}}}"#).is_err(), "unknown category");
    let list = TrackerList::parse(r#"{"entities": {"X": {"social": ["x.example"]}}}"#).unwrap();
    assert_eq!(list.blocks(Strict, "a.example", "x.example"), Some("x.example"));
}

#[test]
fn standard_is_the_default_level_and_syncs() {
    let (mut p, _dir) = open(1);
    assert_eq!(p.prefs().get(&keys::TRACKING_PROTECTION), Standard);
    assert_eq!(keys::TRACKING_PROTECTION.scope, vsesvit_core::prefs::Scope::Synced);
    p.prefs().set(&keys::TRACKING_PROTECTION, &Strict).unwrap();
    assert_eq!(p.prefs().get(&keys::TRACKING_PROTECTION), Strict);
    assert_eq!(serde_json::to_value(Strict).unwrap(), "strict");
    let labels: Vec<&str> = TrackingProtection::ALL.iter().map(|l| l.label()).collect();
    assert_eq!(labels, ["Off", "Standard", "Strict"]);
}

#[test]
fn turning_protection_off_for_a_site() {
    let (mut p, _dir) = open(1);
    let site = origin("https://shop.example");
    let other = origin("https://other.example");
    assert_eq!(trackers::level_for(&mut p, Some(&site)), Standard);
    trackers::set_allowed(&mut p, &site, true).unwrap();
    assert!(trackers::allowed(&mut p, &site));
    assert_eq!(trackers::level_for(&mut p, Some(&site)), Off);
    assert_eq!(trackers::level_for(&mut p, Some(&other)), Standard);
    assert_eq!(trackers::level_for(&mut p, None), Standard);
    assert_eq!(trackers::allowed_sites(&mut p), std::slice::from_ref(&site));
    assert_eq!(p.site_permissions().for_site(&site), [(Permission::Trackers, Setting::Allow)], "listed with the site's settings");

    assert!(site_rows(true, &[(Permission::Trackers, Setting::Allow)], &[], Capturing::default()).is_empty(), "the popup has a switch for it");
    p.site_permissions().set(&site, Permission::Camera, Some(Setting::Block)).unwrap();
    p.site_permissions().reset_site(&site).unwrap();
    assert!(trackers::allowed(&mut p, &site), "Reset permissions leaves the switch alone");
    assert_eq!(p.site_permissions().get(&site, Permission::Camera), None);

    trackers::set_allowed(&mut p, &site, false).unwrap();
    assert_eq!(trackers::level_for(&mut p, Some(&site)), Standard);
    assert!(trackers::allowed_sites(&mut p).is_empty());
    p.prefs().set(&keys::TRACKING_PROTECTION, &Off).unwrap();
    assert_eq!(trackers::level_for(&mut p, Some(&site)), Off);
}

#[test]
fn a_site_exception_syncs_with_the_site_settings() {
    let (mut a, _da) = open(1);
    let (mut b, _db) = open(2);
    let site = origin("https://shop.example");
    trackers::set_allowed(&mut a, &site, true).unwrap();
    let wire = a.sync().changes_since(Kind::SitePermissions, Seq::ZERO, usize::MAX).unwrap().records;
    assert_eq!(wire.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["trackers|https://shop.example"]);
    let report = b.sync().apply(wire).unwrap();
    assert!(report.changed.site_permissions);
    assert_eq!(trackers::level_for(&mut b, Some(&site)), Off);
}

fn rules(list: &TrackerList, level: TrackingProtection, allowed: &[Origin]) -> Vec<Value> {
    serde_json::from_str::<Vec<Value>>(&list.dnr_rules(level, allowed)).unwrap()
}

#[test]
fn dnr_rules_block_each_company_s_trackers_off_its_sites() {
    let list = TrackerList::parse(
        r#"{"entities": {
            "Ads": {"advertising": ["ads.example"]},
            "Social": {"sites": ["social.example"], "analytics": ["stats.social.example"], "social": ["social.example"]}
        }}"#,
    )
    .unwrap();
    assert_eq!(list.dnr_rules(Off, &[origin("https://shop.example")]), "[]", "nothing to block, nothing to allow");
    assert_eq!(
        rules(&list, Standard, &[]),
        [
            json!({"id": 1, "action": {"type": "block"}, "condition": {"requestDomains": ["ads.example"], "excludedInitiatorDomains": ["ads.example"]}}),
            json!({"id": 2, "action": {"type": "block"}, "condition": {"requestDomains": ["stats.social.example"], "excludedInitiatorDomains": ["social.example", "stats.social.example"]}}),
        ]
    );
    let strict = rules(&list, Strict, &[origin("https://shop.example"), origin("http://127.0.0.1:8080")]);
    assert_eq!(strict[1]["condition"]["requestDomains"], json!(["stats.social.example", "social.example"]));
    assert_eq!(
        strict[2..],
        [
            json!({"id": 3, "priority": 2, "action": {"type": "allowAllRequests"}, "condition": {"urlFilter": "|https://shop.example/", "resourceTypes": ["main_frame"]}}),
            json!({"id": 4, "priority": 2, "action": {"type": "allowAllRequests"}, "condition": {"urlFilter": "|http://127.0.0.1:8080/", "resourceTypes": ["main_frame"]}}),
        ]
    );
}
