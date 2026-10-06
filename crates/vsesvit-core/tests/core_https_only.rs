//! HTTPS-only: which http URLs upgrade, site exceptions, and one tab's way from an upgrade to
//! the warning page and past it.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, TimeSource};
use vsesvit_core::https_only::{self, Cause, Next, Reach, Upgrades};
use vsesvit_core::permissions::{Capturing, Origin, Permission, Setting, SiteSetting, site_rows};
use vsesvit_core::prefs::keys;
use vsesvit_core::private::Browsing;
use vsesvit_core::{OpenOptions, Profile, Url};

use Reach::{Everywhere, Public};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-https-only-{}", uuid::Uuid::new_v4())));
    let p = Profile::open(
        &dir.0,
        OpenOptions { time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000))), new_device_id: Some(DeviceId(1)), ..OpenOptions::default() },
    )
    .unwrap();
    (p, dir)
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn upgraded(s: &str, reach: Reach) -> Option<String> {
    https_only::upgraded(&url(s), reach).map(String::from)
}

#[test]
fn public_sites_upgrade_on_the_default_port() {
    assert_eq!(upgraded("http://example.com/a?b#c", Public).as_deref(), Some("https://example.com/a?b#c"));
    assert_eq!(upgraded("http://www.example.co.uk:80/", Public).as_deref(), Some("https://www.example.co.uk/"), "port 80 is the default");
    assert_eq!(upgraded("http://93.184.216.34/", Public).as_deref(), Some("https://93.184.216.34/"));
    assert_eq!(upgraded("http://[2606:2800:220:1::]/", Public).as_deref(), Some("https://[2606:2800:220:1::]/"));
    for local in [
        "http://localhost/",
        "http://app.localhost/",
        "http://127.0.0.1/",
        "http://192.168.1.1/",
        "http://10.0.0.8/",
        "http://172.16.0.1/",
        "http://169.254.1.1/",
        "http://100.64.0.1/",
        "http://0.0.0.0/",
        "http://[::1]/",
        "http://[fd00::1]/",
        "http://[fe80::1]/",
        "http://router/",
        "http://printer.local/",
        "http://nas.home.arpa/",
        "http://site.test/",
        "http://localhost./",
        "http://example.com:8080/",
    ] {
        assert_eq!(upgraded(local, Public), None, "{local}");
    }
}

#[test]
fn everywhere_upgrades_every_http_url_and_keeps_its_port() {
    assert_eq!(upgraded("http://127.0.0.1:8080/page", Everywhere).as_deref(), Some("https://127.0.0.1:8080/page"));
    assert_eq!(upgraded("http://localhost/", Everywhere).as_deref(), Some("https://localhost/"));
    assert_eq!(upgraded("http://example.com:8080/", Everywhere).as_deref(), Some("https://example.com:8080/"));
    assert_eq!(upgraded("http://[::1]:3000/", Everywhere).as_deref(), Some("https://[::1]:3000/"));
}

#[test]
fn only_http_upgrades() {
    for reach in [Public, Everywhere] {
        for other in ["https://example.com/", "file:///C:/page.html", "ftp://example.com/", "data:text/html,hi", "about:blank"] {
            assert_eq!(upgraded(other, reach), None, "{other}");
        }
    }
}

#[test]
fn upgrades_once_on_until_the_site_has_an_exception() {
    let (mut p, _dir) = open();
    let page = url("http://plain.example.org/page");
    let site = Origin::parse("http://plain.example.org").unwrap();
    assert!(!p.prefs().get(&keys::HTTPS_ONLY), "off by default");
    assert_eq!(keys::HTTPS_ONLY.scope, vsesvit_core::prefs::Scope::Synced);
    assert_eq!(https_only::upgrade(&mut p, Browsing::Normal, &page, Public), None);

    p.prefs().set(&keys::HTTPS_ONLY, &true).unwrap();
    assert_eq!(https_only::upgrade(&mut p, Browsing::Normal, &page, Public), Some(url("https://plain.example.org/page")));
    assert!(!https_only::allowed(&mut p, Browsing::Normal, &site));

    https_only::allow(&mut p, Browsing::Normal, &page).unwrap();
    assert!(https_only::allowed(&mut p, Browsing::Normal, &site));
    assert_eq!(https_only::upgrade(&mut p, Browsing::Normal, &page, Public), None);
    assert_eq!(https_only::upgrade(&mut p, Browsing::Normal, &url("http://plain.example.org/other"), Public), None, "the whole site");
    assert!(https_only::upgrade(&mut p, Browsing::Normal, &url("http://other.example.org/"), Public).is_some());
    assert_eq!(p.site_permissions().all(), [SiteSetting { origin: site, permission: Permission::Http, setting: Setting::Allow }]);
    assert!(site_rows(true, &[(Permission::Http, Setting::Allow)], &[], Capturing::default()).is_empty(), "site info has no row for it");
}

const HTTP: &str = "http://plain.example.org/page";
const HTTPS: &str = "https://plain.example.org/page";

fn start(tab: &mut Upgrades, s: &str, cause: Cause) -> Next {
    let u = url(s);
    tab.starting(&u, cause, https_only::upgraded(&u, Public))
}

/// A tab that typed `HTTP`, upgraded it and failed to load it over https, and now shows the warning.
fn warning_shown() -> Upgrades {
    let mut tab = Upgrades::default();
    assert_eq!(start(&mut tab, HTTP, Cause::Other), Next::Upgrade(url(HTTPS)));
    assert_eq!(start(&mut tab, HTTPS, Cause::Other), Next::Load);
    assert_eq!(tab.finished(false), Some(url(HTTP)));
    assert_eq!(tab.warning(), None, "not shown until it commits");
    assert_eq!(start(&mut tab, HTTP, Cause::Other), Next::Load, "the warning page's own load");
    tab.committed();
    assert_eq!(tab.warning(), Some(&url(HTTP)));
    tab
}

#[test]
fn an_upgrade_that_loads_warns_about_nothing() {
    let mut tab = Upgrades::default();
    assert_eq!(start(&mut tab, HTTP, Cause::Other), Next::Upgrade(url(HTTPS)));
    assert_eq!(start(&mut tab, HTTPS, Cause::Other), Next::Load);
    tab.committed();
    assert_eq!(tab.finished(true), None);
    assert_eq!(tab.warning(), None);
    assert_eq!(start(&mut tab, "https://example.com/", Cause::Link), Next::Load);
}

#[test]
fn continue_on_the_warning_allows_the_site() {
    let mut tab = warning_shown();
    assert_eq!(start(&mut tab, HTTP, Cause::Link), Next::Allow(url(HTTP)));
    assert_eq!(tab.warning(), None);
    assert_eq!(tab.finished(false), None, "nothing upgraded is under way");
    tab.committed();
    assert_eq!(tab.warning(), None);
}

#[test]
fn reloading_the_warning_tries_https_again() {
    let mut tab = warning_shown();
    assert_eq!(start(&mut tab, HTTP, Cause::Other), Next::Upgrade(url(HTTPS)));
    assert_eq!(start(&mut tab, HTTPS, Cause::Other), Next::Load);
    tab.committed();
    assert_eq!(tab.warning(), None, "the https page replaced the warning");
}

#[test]
fn leaving_the_warning_ends_its_continue() {
    let mut tab = warning_shown();
    tab.leave();
    assert_eq!(tab.warning(), None);
    assert_eq!(start(&mut tab, HTTP, Cause::Link), Next::Upgrade(url(HTTPS)));
}

#[test]
fn https_redirecting_back_to_http_warns() {
    let mut tab = Upgrades::default();
    assert_eq!(start(&mut tab, HTTP, Cause::Other), Next::Upgrade(url(HTTPS)));
    assert_eq!(start(&mut tab, HTTPS, Cause::Other), Next::Load);
    assert_eq!(start(&mut tab, "http://plain.example.org/login", Cause::Redirect), Next::Warn(url("http://plain.example.org/login")));
    assert_eq!(tab.finished(false), None, "the warning is already on its way");
    assert_eq!(start(&mut tab, "http://plain.example.org/login", Cause::Other), Next::Load);
    tab.committed();
    assert_eq!(tab.warning(), Some(&url("http://plain.example.org/login")));
}

#[test]
fn a_redirect_to_another_http_host_upgrades_it_too() {
    let mut tab = Upgrades::default();
    assert_eq!(start(&mut tab, HTTP, Cause::Other), Next::Upgrade(url(HTTPS)));
    assert_eq!(start(&mut tab, HTTPS, Cause::Other), Next::Load);
    assert_eq!(start(&mut tab, "http://cdn.example.net/x", Cause::Redirect), Next::Upgrade(url("https://cdn.example.net/x")));
    assert_eq!(start(&mut tab, "https://cdn.example.net/x", Cause::Other), Next::Load);
    assert_eq!(start(&mut tab, "http://plain.example.org/y", Cause::Redirect), Next::Warn(url("http://plain.example.org/y")), "back to the first host");

    let mut tab = Upgrades::default();
    start(&mut tab, HTTP, Cause::Other);
    start(&mut tab, HTTPS, Cause::Other);
    start(&mut tab, "http://cdn.example.net/x", Cause::Redirect);
    start(&mut tab, "https://cdn.example.net/x", Cause::Other);
    assert_eq!(tab.finished(false), Some(url("http://cdn.example.net/x")));
}

#[test]
fn an_unrelated_navigation_drops_a_stale_upgrade() {
    let mut tab = Upgrades::default();
    assert_eq!(start(&mut tab, HTTP, Cause::Other), Next::Upgrade(url(HTTPS)));
    assert_eq!(start(&mut tab, "https://example.com/", Cause::Other), Next::Load);
    assert_eq!(tab.finished(false), None);
    assert_eq!(tab.warning(), None);
}

#[test]
fn the_warning_page_names_the_site_and_offers_continue() {
    let html = https_only::warning_page(&url(HTTP));
    assert!(html.contains("<title>Connection is not secure</title>"));
    assert!(html.contains("<h1>This site doesn't support a secure connection</h1>"));
    assert!(html.contains("<strong>plain.example.org</strong>"));
    assert!(html.contains(r#"<a id="continue" class="button" href="http://plain.example.org/page">Continue to site</a>"#));
    assert_eq!(html.matches("<a ").count(), 1);
    assert!(html.contains("color-scheme"));

    let html = https_only::warning_page(&url("http://xn--80ak6aa92e.example/?q=1&x='\"><script>alert(1)</script>"));
    assert!(!html.contains("<script"));
    assert!(html.contains(r#"href="http://xn--80ak6aa92e.example/?q=1&amp;x=%27%22%3E%3Cscript%3Ealert(1)%3C/script%3E""#), "{html}");
}
