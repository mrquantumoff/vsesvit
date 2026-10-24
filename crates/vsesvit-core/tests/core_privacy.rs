//! The privacy snapshot answers every question the profile answers, the same way, without
//! borrowing it.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use vsesvit_core::cookies::{self, ThirdPartyCookies};
use vsesvit_core::crdt::{DeviceId, TimeSource};
use vsesvit_core::https_only::{self, Reach};
use vsesvit_core::permissions::{Origin, Permission, Setting};
use vsesvit_core::prefs::keys;
use vsesvit_core::privacy::Snapshot;
use vsesvit_core::private::Browsing;
use vsesvit_core::trackers::{self, Category, TrackerList, TrackingProtection};
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-privacy-{}", uuid::Uuid::new_v4())));
    let p = Profile::open(
        &dir.0,
        OpenOptions { time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000))), new_device_id: Some(DeviceId(1)), ..OpenOptions::default() },
    )
    .unwrap();
    (p, dir)
}

fn origin(s: &str) -> Origin {
    Origin::parse(s).unwrap()
}

fn snapshot(p: &mut Profile) -> Snapshot {
    Snapshot::capture(p, Arc::new(TrackerList::bundled().clone().with_tracker("localhost", Category::Analytics)))
}

#[test]
fn tracker_levels_follow_the_preference_and_the_sites_it_is_off_for() {
    let (mut p, _dir) = open();
    let (a, b) = (origin("https://a.test"), origin("https://b.test"));
    p.prefs().set(&keys::TRACKING_PROTECTION, &TrackingProtection::Strict).unwrap();
    trackers::set_allowed(&mut p, &a, true).unwrap();
    let snap = snapshot(&mut p);
    for site in [Some(&a), Some(&b), None] {
        assert_eq!(snap.tracker_level(site), trackers::level_for(&mut p, site), "{site:?}");
    }
    assert_eq!(snap.tracker_level(Some(&a)), TrackingProtection::Off);
    assert_eq!(snap.tracker_level(Some(&b)), TrackingProtection::Strict);

    let list = snap.tracker_list();
    assert_eq!(snap.tracker_blocks(TrackingProtection::Standard, "a.test", "localhost"), list.blocks(TrackingProtection::Standard, "a.test", "localhost"));
    assert_eq!(snap.tracker_blocks(TrackingProtection::Standard, "a.test", "localhost"), Some("localhost"));
}

#[test]
fn third_party_cookies_and_site_rules_match_the_profile() {
    let (mut p, _dir) = open();
    let (allowed, blocked, plain) = (origin("https://allowed.test"), origin("https://blocked.test"), origin("https://plain.test"));
    cookies::set(&mut p, &allowed, Some(Setting::Allow)).unwrap();
    cookies::set(&mut p, &blocked, Some(Setting::Block)).unwrap();
    for choice in ThirdPartyCookies::ALL {
        p.prefs().set(&keys::THIRD_PARTY_COOKIES, &choice).unwrap();
        let snap = snapshot(&mut p);
        for browsing in [Browsing::Normal, Browsing::Private] {
            for top in [Some(&allowed), Some(&blocked), Some(&plain), None] {
                assert_eq!(
                    snap.third_party_cookies_blocked(browsing, top),
                    cookies::third_party_blocked(&mut p, browsing, top),
                    "{choice:?} {browsing:?} {top:?}"
                );
            }
        }
    }
    let snap = snapshot(&mut p);
    assert_eq!(snap.cookie_rules().blocked_hosts(), ["blocked.test"]);
    assert!(snap.blocks_cookie(".blocked.test"));
    assert!(!snap.blocks_cookie("plain.test"));
}

#[test]
fn https_upgrades_match_the_profile_in_normal_and_private_windows() {
    let (mut p, _dir) = open();
    let url = |s: &str| Url::parse(s).unwrap();
    let (kept, masked, private_only) = (url("http://kept.example.com/"), url("http://masked.example.com/"), url("http://private.example.com/"));
    let all = [&kept, &masked, &private_only, &url("http://other.example.com/"), &url("http://localhost:8123/")];

    let snap = snapshot(&mut p);
    assert!(!snap.https_only(), "off by default");
    for u in all {
        assert_eq!(snap.https_upgrade(Browsing::Normal, u, Reach::Public), None);
    }

    p.prefs().set(&keys::HTTPS_ONLY, &true).unwrap();
    https_only::allow(&mut p, Browsing::Normal, &kept).unwrap();
    https_only::allow(&mut p, Browsing::Normal, &masked).unwrap();
    // A private window's reset hides the stored exception from that window only.
    p.site_permissions_in(Browsing::Private).set(&origin("http://masked.example.com"), Permission::Http, None).unwrap();
    https_only::allow(&mut p, Browsing::Private, &private_only).unwrap();

    let snap = snapshot(&mut p);
    assert!(snap.https_only());
    for browsing in [Browsing::Normal, Browsing::Private] {
        for reach in [Reach::Public, Reach::Everywhere] {
            for u in all {
                assert_eq!(snap.https_upgrade(browsing, u, reach), https_only::upgrade(&mut p, browsing, u, reach), "{browsing:?} {reach:?} {u}");
            }
        }
    }
    assert!(snap.https_upgrade(Browsing::Normal, &private_only, Reach::Public).is_some());
    assert!(snap.https_upgrade(Browsing::Private, &private_only, Reach::Public).is_none());
    assert!(snap.https_upgrade(Browsing::Normal, &masked, Reach::Public).is_none());
    assert!(snap.https_upgrade(Browsing::Private, &masked, Reach::Public).is_some());
}
