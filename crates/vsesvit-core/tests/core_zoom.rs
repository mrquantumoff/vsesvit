//! Page zoom per site: one level per host, the default forgets the site, and it survives a reopen.

use std::path::PathBuf;

use vsesvit_core::private::Browsing;
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open_at(dir: &TempDir) -> Profile {
    Profile::open(&dir.0, OpenOptions::default()).unwrap()
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

#[test]
fn a_site_keeps_its_zoom_across_pages_schemes_and_restarts() {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-zoom-{}", uuid::Uuid::new_v4())));
    {
        let mut p = open_at(&dir);
        assert_eq!(p.site_zoom(Browsing::Normal).get(&url("https://docs.example/")).unwrap(), 1.0);
        p.site_zoom(Browsing::Normal).set(&url("https://docs.example/a"), 1.25).unwrap();
        p.site_zoom(Browsing::Normal).set(&url("https://other.example/"), 0.67).unwrap();
    }
    let mut p = open_at(&dir);
    assert_eq!(p.site_zoom(Browsing::Normal).get(&url("https://docs.example/b?q=1")).unwrap(), 1.25, "any page of the site");
    assert_eq!(p.site_zoom(Browsing::Normal).get(&url("http://DOCS.example:8080/")).unwrap(), 1.25, "any scheme, port or case");
    assert_eq!(p.site_zoom(Browsing::Normal).get(&url("https://sub.docs.example/")).unwrap(), 1.0, "a subdomain is another site");
    assert_eq!(p.site_zoom(Browsing::Normal).get(&url("https://other.example/")).unwrap(), 0.67);
}

#[test]
fn the_default_forgets_the_site_and_pages_off_the_web_are_not_remembered() {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-zoom-{}", uuid::Uuid::new_v4())));
    let mut p = open_at(&dir);
    let site = url("https://docs.example/");
    p.site_zoom(Browsing::Normal).set(&site, 2.0).unwrap();
    p.site_zoom(Browsing::Normal).set(&site, 1.0).unwrap();
    assert_eq!(p.site_zoom(Browsing::Normal).get(&site).unwrap(), 1.0);
    let rows: i64 = rusqlite::Connection::open(dir.0.join("vsesvit.db"))
        .unwrap()
        .query_row("SELECT count(*) FROM site_zoom", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);

    for page in ["file:///tmp/a.html", "about:blank", "data:text/html,hi"] {
        p.site_zoom(Browsing::Normal).set(&url(page), 1.5).unwrap();
        assert_eq!(p.site_zoom(Browsing::Normal).get(&url(page)).unwrap(), 1.0, "{page}");
    }
}
