//! Private browsing: site choices, HTTPS-only exceptions, zoom and download rows kept by the
//! private session over the stored ones, never written, and gone when the session ends.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, Seq, TimeSource};
use vsesvit_core::downloads::{State, unconfirmed_path};
use vsesvit_core::https_only;
use vsesvit_core::permissions::{Answer, Decision, Origin, Permission, Setting, TabGrants};
use vsesvit_core::private::Browsing::{Normal, Private};
use vsesvit_core::sync::Kind;
use vsesvit_core::{OpenOptions, Profile, Url};

use Permission::{Camera, Location, Notifications, Trackers};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const T0: u64 = 1_780_000_000_000;

fn open_at(dir: &TempDir) -> Profile {
    Profile::open(
        &dir.0,
        OpenOptions { time: TimeSource::Manual(Rc::new(Cell::new(T0))), new_device_id: Some(DeviceId(4)), ..OpenOptions::default() },
    )
    .unwrap()
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-private-{}", uuid::Uuid::new_v4())));
    (open_at(&dir), dir)
}

fn origin(s: &str) -> Origin {
    Origin::parse(s).unwrap()
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn uploaded_up_to(p: &mut Profile) -> Seq {
    p.sync().changes_since(Kind::SitePermissions, Seq::ZERO, usize::MAX).unwrap().upto
}

fn rows(dir: &TempDir, table: &str) -> i64 {
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap()
}

/// What a normal and a private window see of `host`: its camera setting and zoom over https,
/// and whether HTTPS-only lets its http pages load.
fn seen(p: &mut Profile, host: &str) -> [(Option<Setting>, f64, bool); 2] {
    [Normal, Private].map(|b| {
        let camera = p.site_permissions_in(b).get(&origin(&format!("https://{host}")), Camera);
        let zoom = p.site_zoom(b).get(&url(&format!("https://{host}/"))).unwrap();
        (camera, zoom, https_only::allowed(p, b, &origin(&format!("http://{host}"))))
    })
}

#[test]
fn private_choices_are_read_back_in_private_only() {
    let (mut p, _dir) = open();
    let site = origin("https://meet.example.com");
    let mut grants = TabGrants::default();
    let before = uploaded_up_to(&mut p);

    p.site_permissions_in(Private).answer(Some(&site), &[Camera], Answer::AllowWhileVisiting, &mut grants).unwrap();
    p.site_permissions_in(Private).set(&site, Location, Some(Setting::Block)).unwrap();
    p.site_zoom(Private).set(&url("https://meet.example.com/call"), 1.5).unwrap();
    https_only::allow(&mut p, Private, &url("http://meet.example.com/")).unwrap();

    assert_eq!(p.site_permissions_in(Private).for_site(&site), [(Camera, Setting::Allow), (Location, Setting::Block)]);
    assert_eq!(p.site_permissions_in(Private).decide(Some(&site), &[Camera], &grants), Decision::Allow);
    assert_eq!(seen(&mut p, "meet.example.com"), [(None, 1.0, false), (Some(Setting::Allow), 1.5, true)]);

    assert_eq!(p.site_permissions().for_site(&site), [], "a normal window sees none of it");
    assert!(p.site_permissions().all().is_empty(), "nor does Settings");
    assert_eq!(uploaded_up_to(&mut p), before, "nothing to sync");
}

#[test]
fn private_choices_still_may_not_remember_what_is_asked_every_time() {
    let (mut p, _dir) = open();
    let site = origin("https://meet.example.com");
    let refused = p.site_permissions_in(Private).set(&site, Permission::ScreenShare, Some(Setting::Allow));
    assert!(matches!(refused, Err(vsesvit_core::Error::AlwaysAsks(Permission::ScreenShare))));
}

#[test]
fn nothing_private_reaches_the_database() {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-private-{}", uuid::Uuid::new_v4())));
    {
        let mut p = open_at(&dir);
        let site = origin("https://meet.example.com");
        p.site_permissions_in(Private).set(&site, Camera, Some(Setting::Allow)).unwrap();
        p.site_permissions_in(Private).reset_site(&site).unwrap();
        p.site_permissions_in(Private).set(&site, Notifications, Some(Setting::Block)).unwrap();
        https_only::allow(&mut p, Private, &url("http://meet.example.com/")).unwrap();
        p.site_zoom(Private).set(&url("https://meet.example.com/"), 1.5).unwrap();
        p.downloads().start("https://meet.example.com/a.zip", Path::new("/dl/a.zip"), None, T0, Private).unwrap();
        for table in ["site_permissions", "site_zoom", "downloads"] {
            assert_eq!(rows(&dir, table), 0, "{table} while the session lasts");
        }
    }
    let mut p = open_at(&dir);
    assert_eq!(seen(&mut p, "meet.example.com"), [(None, 1.0, false), (None, 1.0, false)], "after a restart");
    assert!(p.downloads().list(10, Private).unwrap().is_empty());
}

#[test]
fn ending_the_private_session_forgets_what_it_kept() {
    let (mut p, _dir) = open();
    p.site_permissions_in(Private).set(&origin("https://meet.example.com"), Camera, Some(Setting::Allow)).unwrap();
    p.site_zoom(Private).set(&url("https://meet.example.com/"), 1.5).unwrap();
    https_only::allow(&mut p, Private, &url("http://meet.example.com/")).unwrap();
    p.downloads().start("https://meet.example.com/a.zip", Path::new("/dl/a.zip"), None, T0, Private).unwrap();
    assert_eq!(seen(&mut p, "meet.example.com")[1], (Some(Setting::Allow), 1.5, true));

    p.end_private_session();
    assert_eq!(seen(&mut p, "meet.example.com"), [(None, 1.0, false), (None, 1.0, false)]);
    assert!(p.downloads().list(10, Private).unwrap().is_empty());
    p.end_private_session();
    assert!(p.downloads().list(10, Private).unwrap().is_empty(), "ending twice is ending once");
}

#[test]
fn private_windows_inherit_stored_state_and_mask_it_without_changing_it() {
    let (mut p, _dir) = open();
    let site = origin("https://meet.example.com");
    p.site_permissions().set(&site, Camera, Some(Setting::Allow)).unwrap();
    p.site_permissions().set(&site, Trackers, Some(Setting::Allow)).unwrap();
    https_only::allow(&mut p, Normal, &url("http://meet.example.com/")).unwrap();
    p.site_zoom(Normal).set(&url("https://meet.example.com/"), 1.5).unwrap();
    let stored = (Some(Setting::Allow), 1.5, true);
    assert_eq!(seen(&mut p, "meet.example.com"), [stored, stored], "inherited");

    p.site_permissions_in(Private).set(&site, Camera, None).unwrap();
    p.site_zoom(Private).set(&url("https://meet.example.com/"), 1.0).unwrap();
    p.site_permissions_in(Private).reset_site(&origin("http://meet.example.com")).unwrap();
    assert_eq!(seen(&mut p, "meet.example.com"), [stored, (None, 1.0, false)], "back to ask, 100% and no exception");
    p.site_permissions_in(Private).reset_site(&site).unwrap();
    assert_eq!(p.site_permissions_in(Private).for_site(&site), [(Trackers, Setting::Allow)], "reset leaves tracking protection alone");

    p.end_private_session();
    assert_eq!(seen(&mut p, "meet.example.com"), [stored, stored]);
}

#[test]
fn private_windows_block_notifications_without_asking() {
    let (mut p, _dir) = open();
    let site = origin("https://news.example.com");
    let grants = TabGrants::default();
    assert_eq!(p.site_permissions().decide(Some(&site), &[Notifications], &grants), Decision::Ask(vec![Notifications]));
    assert_eq!(p.site_permissions_in(Private).decide(Some(&site), &[Notifications], &grants), Decision::Block);
    assert_eq!(p.site_permissions_in(Private).decide(None, &[Notifications], &grants), Decision::Block, "with no origin too");

    p.site_permissions().set(&site, Notifications, Some(Setting::Allow)).unwrap();
    assert_eq!(p.site_permissions().decide(Some(&site), &[Notifications], &grants), Decision::Allow);
    assert_eq!(p.site_permissions_in(Private).decide(Some(&site), &[Notifications], &grants), Decision::Block, "even if allowed");
    assert_eq!(p.site_permissions_in(Private).decide(Some(&site), &[Camera], &grants), Decision::Ask(vec![Camera]), "others still ask");
}

#[test]
fn private_downloads_are_listed_in_private_windows_only_until_the_session_ends() {
    let (mut p, _dir) = open();
    let path = Path::new("/dl/f");
    let mut dl = p.downloads();
    let stored_old = dl.start("https://old.example/", path, None, T0, Normal).unwrap();
    let private_a = dl.start("https://a.example/", path, None, T0 + 1, Private).unwrap();
    let private_b = dl.start("https://b.example/", path, Some(5), T0 + 2, Private).unwrap();
    let private_c = dl.start("https://c.example/", path, Some(5), T0 + 2, Private).unwrap();
    let stored_new = dl.start("https://new.example/", path, None, T0 + 3, Normal).unwrap();
    assert!(private_a.id.0 < 0 && private_b.id.0 < 0 && stored_old.id.0 > 0);
    assert_ne!(private_a.id, private_b.id);

    let ids = |p: &mut Profile, limit, browsing| p.downloads().list(limit, browsing).unwrap().into_iter().map(|d| d.id).collect::<Vec<_>>();
    assert_eq!(ids(&mut p, 10, Private), [stored_new.id, private_c.id, private_b.id, private_a.id, stored_old.id], "newest first");
    assert_eq!(ids(&mut p, 2, Private), [stored_new.id, private_c.id]);
    assert_eq!(ids(&mut p, 10, Normal), [stored_new.id, stored_old.id], "a normal window lists no private download");

    p.downloads().update(private_a.id, State::Completed, 7, Some(7)).unwrap();
    let a = p.downloads().list(10, Private).unwrap().into_iter().find(|d| d.id == private_a.id).unwrap();
    assert_eq!((a.state, a.received, a.total), (State::Completed, 7, Some(7)));
    p.downloads().remove(private_c.id).unwrap();
    assert_eq!(ids(&mut p, 10, Private), [stored_new.id, private_b.id, private_a.id, stored_old.id]);

    assert_eq!(p.downloads().interrupt_stale().unwrap(), 2, "only stored rows are interrupted");
    p.downloads().clear(Normal).unwrap();
    assert_eq!(ids(&mut p, 10, Private), [private_b.id, private_a.id], "a normal window clears only what it lists");
    p.downloads().clear(Private).unwrap();
    assert_eq!(ids(&mut p, 10, Private), [private_b.id], "clear keeps what is in progress");

    p.end_private_session();
    assert!(p.downloads().list(10, Private).unwrap().is_empty());
    p.downloads().update(private_b.id, State::Cancelled, 0, None).unwrap();
    let next = p.downloads().start("https://d.example/", path, None, T0 + 4, Private).unwrap();
    assert!(next.id.0 < private_c.id.0, "a later session never reuses an ended one's ids");
    assert_eq!(p.downloads().list(10, Private).unwrap()[0].state, State::InProgress);
}

#[test]
fn a_private_file_waiting_to_be_kept_is_kept_or_discarded_in_the_session_and_deleted_when_it_ends() {
    let (mut p, dir) = open();
    let wait = |p: &mut Profile, name: &str| {
        let path = dir.0.join(name);
        std::fs::write(unconfirmed_path(&path), "held").unwrap();
        let d = p.downloads().start("https://a.example/", &path, None, T0, Private).unwrap();
        p.downloads().update(d.id, State::Unconfirmed, 4, Some(4)).unwrap();
        (d.id, path)
    };
    let (kept, kept_path) = wait(&mut p, "kept.sh");
    let (discarded, discarded_path) = wait(&mut p, "discarded.sh");
    let (left, left_path) = wait(&mut p, "left.sh");

    assert_eq!(p.downloads().keep(kept).unwrap(), Some(kept_path.clone()));
    p.downloads().discard(discarded).unwrap();
    let listed = p.downloads().list(10, Private).unwrap();
    let row = listed.iter().find(|d| d.id == kept).unwrap();
    assert_eq!((row.state, &row.path), (State::Completed, &kept_path));
    assert!(!listed.iter().any(|d| d.id == discarded));
    assert_eq!(std::fs::read_to_string(&kept_path).unwrap(), "held");
    assert!(!unconfirmed_path(&discarded_path).exists());
    assert_eq!(p.downloads().keep(kept).unwrap(), None, "kept already");

    p.end_private_session();
    assert!(!unconfirmed_path(&left_path).exists() && !left_path.exists(), "nothing can keep it any more");
    assert!(kept_path.is_file(), "a kept file stays");
    assert_eq!(p.downloads().keep(left).unwrap(), None);
}
