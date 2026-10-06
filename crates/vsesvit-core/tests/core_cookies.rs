//! Cookie controls: the third-party cookies choice, per-site rules and what the shells enforce
//! from them, the site-info words, sync, and the v9 migration that lets a rule clear on exit.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::cookies::{self, SiteCookies, ThirdPartyCookies};
use vsesvit_core::crdt::{DeviceId, Seq, TimeSource};
use vsesvit_core::permissions::{Capturing, Origin, Permission, Setting, site_rows};
use vsesvit_core::prefs::keys;
use vsesvit_core::private::Browsing;
use vsesvit_core::sync::Kind;
use vsesvit_core::{OpenOptions, Profile};

use Setting::{Allow, Block, ClearOnExit};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tmp() -> TempDir {
    TempDir(std::env::temp_dir().join(format!("vsesvit-cookies-{}", uuid::Uuid::new_v4())))
}

fn open_at(dir: &TempDir, device: u64) -> Profile {
    Profile::open(
        &dir.0,
        OpenOptions {
            time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000 + device))),
            new_device_id: Some(DeviceId(device)),
            ..OpenOptions::default()
        },
    )
    .unwrap()
}

fn open(device: u64) -> (Profile, TempDir) {
    let dir = tmp();
    (open_at(&dir, device), dir)
}

fn origin(s: &str) -> Origin {
    Origin::parse(s).unwrap()
}

#[test]
fn third_party_cookies_are_blocked_in_private_windows_by_default_and_the_choice_syncs() {
    let (mut p, _dir) = open(1);
    assert_eq!(p.prefs().get(&keys::THIRD_PARTY_COOKIES), ThirdPartyCookies::BlockInPrivate);
    assert_eq!(keys::THIRD_PARTY_COOKIES.scope, vsesvit_core::prefs::Scope::Synced);
    p.prefs().set(&keys::THIRD_PARTY_COOKIES, &ThirdPartyCookies::Block).unwrap();
    assert_eq!(p.prefs().get(&keys::THIRD_PARTY_COOKIES), ThirdPartyCookies::Block);
    assert_eq!(serde_json::to_value(ThirdPartyCookies::BlockInPrivate).unwrap(), "block_in_private");
    let labels: Vec<&str> = ThirdPartyCookies::ALL.iter().map(|c| c.label()).collect();
    assert_eq!(labels, ["Allow third-party cookies", "Block third-party cookies in private windows", "Block third-party cookies"]);
    assert!(ThirdPartyCookies::ALL.iter().all(|c| !c.description().is_empty() && !c.description().ends_with('.')));
    assert_eq!(cookies::SITE_TITLE, Permission::Cookies.label());
}

#[test]
fn each_choice_blocks_in_the_windows_it_names() {
    use Browsing::{Normal, Private};
    let blocks = |c: ThirdPartyCookies| [c.blocks(Normal), c.blocks(Private)];
    assert_eq!(blocks(ThirdPartyCookies::Allow), [false, false]);
    assert_eq!(blocks(ThirdPartyCookies::BlockInPrivate), [false, true]);
    assert_eq!(blocks(ThirdPartyCookies::Block), [true, true]);
}

#[test]
fn allowing_a_site_lifts_third_party_blocking_on_its_pages() {
    let (mut p, _dir) = open(1);
    let site = origin("https://shop.example");
    let other = origin("https://news.example");
    assert!(!cookies::third_party_blocked(&mut p, Browsing::Normal, Some(&site)));
    assert!(cookies::third_party_blocked(&mut p, Browsing::Private, Some(&site)));

    p.prefs().set(&keys::THIRD_PARTY_COOKIES, &ThirdPartyCookies::Block).unwrap();
    cookies::set(&mut p, &site, Some(Allow)).unwrap();
    assert_eq!(cookies::setting(&mut p, &site), Some(Allow));
    assert!(!cookies::third_party_blocked(&mut p, Browsing::Normal, Some(&site)));
    assert!(!cookies::third_party_blocked(&mut p, Browsing::Private, Some(&site)));
    assert!(cookies::third_party_blocked(&mut p, Browsing::Normal, Some(&other)));
    assert!(cookies::third_party_blocked(&mut p, Browsing::Normal, None), "one policy for every site ignores site rules");

    for rule in [Block, ClearOnExit] {
        cookies::set(&mut p, &site, Some(rule)).unwrap();
        assert!(cookies::third_party_blocked(&mut p, Browsing::Normal, Some(&site)), "{rule:?} lifts nothing");
    }
    p.prefs().set(&keys::THIRD_PARTY_COOKIES, &ThirdPartyCookies::Allow).unwrap();
    assert!(!cookies::third_party_blocked(&mut p, Browsing::Private, None));
}

#[test]
fn a_cookie_rule_is_listed_in_settings_and_not_as_a_permission() {
    let (mut p, _dir) = open(1);
    let blocked = origin("https://blocked.example");
    let cleared = origin("https://cleared.example");
    cookies::set(&mut p, &blocked, Some(Block)).unwrap();
    cookies::set(&mut p, &cleared, Some(ClearOnExit)).unwrap();
    p.site_permissions().set(&cleared, Permission::Camera, Some(Allow)).unwrap();

    let sites: Vec<(String, Vec<(Permission, Setting)>)> = p.site_permissions().by_site().into_iter().map(|s| (s.heading, s.settings)).collect();
    assert_eq!(
        sites,
        [
            ("blocked.example".to_owned(), vec![(Permission::Cookies, Block)]),
            ("cleared.example".to_owned(), vec![(Permission::Camera, Allow), (Permission::Cookies, ClearOnExit)]),
        ]
    );
    let stored = p.site_permissions().for_site(&cleared);
    let rows = site_rows(true, &stored, &[], Capturing::default());
    assert_eq!(rows.iter().map(|r| r.permission).collect::<Vec<_>>(), [Permission::Camera], "site info has a section for cookies");

    p.site_permissions().reset_site(&cleared).unwrap();
    assert_eq!(cookies::setting(&mut p, &cleared), Some(ClearOnExit), "Reset permissions leaves the cookie rule alone");
    assert_eq!(p.site_permissions().get(&cleared, Permission::Camera), None);
    cookies::set(&mut p, &cleared, None).unwrap();
    assert_eq!(cookies::setting(&mut p, &cleared), None);
}

#[test]
fn clear_on_exit_is_a_cookie_setting_only() {
    let (mut p, _dir) = open(1);
    let site = origin("https://meet.example.com");
    p.site_permissions().set(&site, Permission::Camera, Some(ClearOnExit)).unwrap();
    assert_eq!(p.site_permissions().get(&site, Permission::Camera), None);
    assert!(p.site_permissions().all().is_empty());
}

#[test]
fn site_rules_cover_a_site_and_its_subdomains() {
    let (mut p, _dir) = open(1);
    cookies::set(&mut p, &origin("https://example.com"), Some(Block)).unwrap();
    cookies::set(&mut p, &origin("http://example.com"), Some(Block)).unwrap();
    cookies::set(&mut p, &origin("https://ads.example"), Some(Block)).unwrap();
    cookies::set(&mut p, &origin("https://www.cleared.example"), Some(ClearOnExit)).unwrap();
    cookies::set(&mut p, &origin("https://allowed.example"), Some(Allow)).unwrap();
    p.site_permissions().set(&origin("https://camera.example"), Permission::Camera, Some(Block)).unwrap();

    let rules = cookies::site_rules(&mut p);
    assert_eq!(rules.blocked_hosts(), ["ads.example", "example.com"]);
    assert!(rules.blocks_cookie("example.com"));
    assert!(rules.blocks_cookie("www.example.com"));
    assert!(rules.blocks_cookie(".www.example.com"));
    assert!(rules.blocks_cookie("a.b.ads.example"));
    assert!(!rules.blocks_cookie("com"), "not a parent");
    assert!(!rules.blocks_cookie("notexample.com"), "domains match whole labels");
    assert!(!rules.blocks_cookie("www.cleared.example"), "clear on exit lets the site use cookies");
    assert!(!rules.blocks_cookie("allowed.example"));
    assert!(!rules.blocks_cookie("camera.example"));
    assert!(!rules.blocks_cookie(".cleared.example"));

    assert_eq!(
        rules.to_clear(),
        [origin("http://example.com"), origin("https://ads.example"), origin("https://example.com"), origin("https://www.cleared.example")]
    );
    assert!(rules.clears_cookie(".example.com"));
    assert!(rules.clears_cookie("www.example.com"));
    assert!(rules.clears_cookie(".www.cleared.example"));
    assert!(rules.clears_cookie(".cleared.example"), "a domain cookie that reaches the rule's host");
    assert!(!rules.clears_cookie("cleared.example"), "the parent's own cookie");
    assert!(rules.clears_cookie("img.www.cleared.example"));
    assert!(!rules.clears_cookie("other.cleared.example"), "a sibling of the rule's host");
    assert!(!rules.clears_cookie("notexample.com"));
    assert!(!rules.clears_cookie("allowed.example"));
    assert!(rules.clears("example.com"));
    assert!(rules.clears("cleared.example"), "a website-data record of the rule's parent domain");
    assert!(!rules.clears("other.example"));
    assert!(!rules.clears("camera.example"));

    let (mut empty, _dir) = open(2);
    let rules = cookies::site_rules(&mut empty);
    assert!(rules.blocked_hosts().is_empty() && rules.to_clear().is_empty());
    assert!(!rules.blocks_cookie("example.com") && !rules.clears("example.com"));
}

#[test]
fn a_parent_domains_cookie_reaches_a_blocked_subdomain() {
    let (mut p, _dir) = open(1);
    cookies::set(&mut p, &origin("https://www.shop.example"), Some(Block)).unwrap();
    let rules = cookies::site_rules(&mut p);
    assert!(!rules.blocks_cookie("shop.example"), "the parent site keeps its own cookies");
    assert!(rules.blocks_cookie(".shop.example"), "but not its domain cookies, which reach www.shop.example");
    assert!(!rules.blocks_cookie("mail.shop.example"));
}

#[test]
fn the_block_script_names_the_blocked_hosts() {
    assert_eq!(cookies::block_script(&[]), None);
    let script = cookies::block_script(&["ads.example".to_owned(), "example.com".to_owned()]).unwrap();
    assert!(script.contains(r#"["ads.example","example.com"]"#), "{script}");
    assert!(script.contains("Document.prototype, 'cookie'"), "{script}");
    assert!(script.contains("cookieStore"), "{script}");
    assert!(!script.contains("HOSTS"), "{script}");
}

#[test]
fn site_info_offers_allow_only_where_the_engine_makes_exceptions() {
    assert_eq!(cookies::site_choices(None, true), [None, Some(Allow), Some(Block), Some(ClearOnExit)]);
    assert_eq!(cookies::site_choices(Some(Block), false), [None, Some(Block), Some(ClearOnExit)]);
    assert_eq!(cookies::site_choices(Some(Allow), false), [None, Some(Allow), Some(Block), Some(ClearOnExit)], "synced from another device");
    let labels: Vec<&str> = cookies::site_choices(None, true).into_iter().map(cookies::choice_label).collect();
    assert_eq!(labels, ["Default", "Allow", "Block", "Clear on exit"]);
}

#[test]
fn site_info_says_what_the_rule_means_on_the_page() {
    assert_eq!(cookies::site_status(true, None), "Third-party cookies are blocked on this site");
    assert_eq!(cookies::site_status(false, None), "This site and sites it embeds can use cookies");
    assert_eq!(cookies::site_status(false, Some(Allow)), "This site and sites it embeds can use cookies");
    assert_eq!(cookies::site_status(true, Some(Allow)), "This site can use cookies. Third-party cookies are blocked on it");
    assert_eq!(cookies::site_status(false, Some(Block)), "This site can't use cookies");
    assert_eq!(cookies::site_status(true, Some(ClearOnExit)), "This site's cookies and data are deleted when you close Vsesvit");
}

#[test]
fn cookies_are_grouped_by_site() {
    assert!(cookies::group_by_site([]).is_empty());
    let groups = cookies::group_by_site([".example.com", "www.example.com", "example.com", "a.example", ".example.com"]);
    assert_eq!(
        groups,
        [
            SiteCookies { site: "a.example".to_owned(), count: 1 },
            SiteCookies { site: "example.com".to_owned(), count: 3 },
            SiteCookies { site: "www.example.com".to_owned(), count: 1 },
        ]
    );
}

#[test]
fn a_cookie_rule_syncs_with_the_site_settings() {
    let (mut a, _da) = open(1);
    let (mut b, _db) = open(2);
    let site = origin("https://shop.example");
    cookies::set(&mut a, &site, Some(ClearOnExit)).unwrap();
    let wire = a.sync().changes_since(Kind::SitePermissions, Seq::ZERO, usize::MAX).unwrap().records;
    assert_eq!(wire.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["cookies|https://shop.example"]);
    let body: serde_json::Value = serde_json::from_slice(&wire[0].body).unwrap();
    assert_eq!(body["setting"]["v"], "clear_on_exit");
    let report = b.sync().apply(wire).unwrap();
    assert!(report.changed.site_permissions);
    assert_eq!(cookies::setting(&mut b, &site), Some(ClearOnExit));
    assert_eq!(cookies::site_rules(&mut b).to_clear(), [site]);
}

/// A profile from before Clear on exit keeps its site settings, and its table then takes the
/// new setting.
#[test]
fn a_v8_profile_gains_clear_on_exit() {
    let dir = tmp();
    let site = origin("https://kept.example");
    let mut p = open_at(&dir, 1);
    p.site_permissions().set(&site, Permission::Location, Some(Block)).unwrap();
    drop(p);
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE v8 (origin TEXT NOT NULL, permission TEXT NOT NULL, setting TEXT CHECK (setting IN ('allow', 'block')), \
           setting_at BLOB NOT NULL, seq INTEGER NOT NULL, PRIMARY KEY (origin, permission)) WITHOUT ROWID;
         INSERT INTO v8 SELECT * FROM site_permissions;
         DROP TABLE site_permissions;
         ALTER TABLE v8 RENAME TO site_permissions;
         CREATE INDEX site_permissions_seq ON site_permissions(seq);
         PRAGMA user_version = 8;",
    )
    .unwrap();
    let refused = conn.execute("UPDATE site_permissions SET setting = 'clear_on_exit'", []);
    assert!(refused.is_err(), "the v8 table refuses it");
    drop(conn);

    let mut p = open_at(&dir, 1);
    assert_eq!(p.site_permissions().get(&site, Permission::Location), Some(Block));
    cookies::set(&mut p, &site, Some(ClearOnExit)).unwrap();
    drop(p);
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 10);
    let indexed: bool = conn.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'site_permissions_seq'", [], |r| r.get(0)).unwrap();
    assert!(indexed);
    drop(conn);
    let mut p = open_at(&dir, 1);
    assert_eq!(p.site_permissions().for_site(&site), [(Permission::Location, Block), (Permission::Cookies, ClearOnExit)]);
}
