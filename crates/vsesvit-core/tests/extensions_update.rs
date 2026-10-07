//! Extension updates end to end against the testkit's stand-in stores: install from each
//! store, publish newer versions, check on a worker thread, commit. Run with `--features testkit`.
#![cfg(feature = "testkit")]

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use vsesvit_core::crdt::TimeSource;
use vsesvit_core::ext_storage::Area;
use vsesvit_core::extensions::permissions::{GRANTED_PERMISSIONS, PermissionMessage, PermissionSet};
use vsesvit_core::extensions::{
    ExtensionId, FIRST_CHECK_DELAY, InstallError, InstallSource, InstalledExtension, UPDATE_INTERVAL, UpdateCheck, UpdateReport, Updates,
    Verification,
};
use vsesvit_core::testkit::{CrxKey, FixtureServer, FixtureStore, update_probe_files, write_crx3};
use vsesvit_core::{Error, OpenOptions, Profile};

const GUID: &str = "update-probe@vsesvit.test";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("vsesvit-update-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A profile pointed at a fixture store. Fields drop in order: the profile before its dir.
struct Fixture {
    p: Profile,
    store: FixtureStore,
    server: FixtureServer,
    t: TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture::with_time(TimeSource::System)
    }

    fn with_time(time: TimeSource) -> Fixture {
        let t = TempDir::new();
        let server = FixtureServer::start().unwrap();
        let store = FixtureStore::start(&server);
        let mut p = Profile::open(&t.path().join("profile"), OpenOptions { time, ..OpenOptions::default() }).unwrap();
        p.set_stores(store.stores());
        Fixture { p, store, server, t }
    }

    fn publish(&self, from: Kind, version: &str, permissions: &[&str]) -> ExtensionId {
        let files = update_probe_files(version, permissions);
        match from {
            Kind::ChromeWebStore | Kind::EdgeAddons => self.store.publish_crx(&files, &from.developer()),
            Kind::Amo => self.store.publish_xpi(GUID, &files),
        }
    }

    fn install(&mut self, source: InstallSource) -> InstalledExtension {
        let job = self.p.extensions().prepare_install(source).unwrap();
        let staged = std::thread::spawn(move || job.run(&mut |_| {})).join().unwrap().unwrap();
        self.p.extensions().commit(staged).unwrap().unwrap()
    }

    /// Prepares a check here, runs it on its own thread as the shells do.
    fn run_check(&mut self) -> Updates {
        let check = self.p.extensions().prepare_update_check().unwrap();
        std::thread::spawn(move || check.run()).join().unwrap()
    }

    fn check(&mut self) -> UpdateReport {
        let updates = self.run_check();
        self.p.extensions().commit_updates(updates)
    }

    fn get(&mut self, id: &ExtensionId) -> Option<InstalledExtension> {
        self.p.extensions().get(id).unwrap()
    }
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    ChromeWebStore,
    EdgeAddons,
    Amo,
}

impl Kind {
    /// Different developer keys, so a profile can hold one extension from each CRX store.
    fn developer(self) -> CrxKey {
        match self {
            Kind::EdgeAddons => CrxKey::probe(),
            _ => CrxKey::second(),
        }
    }

    fn source(self, id: &ExtensionId) -> InstallSource {
        match self {
            Kind::ChromeWebStore => InstallSource::ChromeWebStore { id: id.clone() },
            Kind::EdgeAddons => InstallSource::EdgeAddons { id: id.clone() },
            Kind::Amo => InstallSource::Amo { slug_or_guid: GUID.to_owned() },
        }
    }

    fn verification(self) -> Verification {
        match self {
            Kind::ChromeWebStore => Verification::ChromeWebStore,
            Kind::EdgeAddons => Verification::EdgeAddons,
            Kind::Amo => Verification::AmoHash,
        }
    }
}

/// A CRX of `files` signed by `developer` alone, as a local `.crx` is.
fn local_crx(files: &[(String, Vec<u8>)], developer: &CrxKey) -> Vec<u8> {
    let files: Vec<(&str, &[u8])> = files.iter().map(|(name, bytes)| (name.as_str(), bytes.as_slice())).collect();
    write_crx3(&files, developer)
}

fn texts(warnings: &[PermissionMessage]) -> Vec<&str> {
    warnings.iter().map(|w| w.text.as_str()).collect()
}

#[test]
fn checks_and_their_results_cross_threads() {
    fn assert_send<T: Send + 'static>() {}
    assert_send::<UpdateCheck>();
    assert_send::<Updates>();
}

#[test]
fn every_store_updates_in_place_and_withholds_new_permissions() {
    for from in [Kind::ChromeWebStore, Kind::EdgeAddons, Kind::Amo] {
        let mut f = Fixture::new();
        let id = f.publish(from, "1.0", &["storage"]);
        let v1 = f.install(from.source(&id));
        assert_eq!((&v1.id, v1.version.as_str(), &v1.verification, v1.enabled), (&id, "1.0", &from.verification(), true), "{from:?}");
        let first = BTreeMap::from([("first".to_owned(), serde_json::json!("1.0"))]);
        f.p.ext_storage().set(&id, Area::Local, first.clone()).unwrap();

        let nothing_newer = f.run_check();
        assert!(nothing_newer.staged.is_empty() && nothing_newer.failed.is_empty(), "{from:?}: {nothing_newer:?}");
        assert_eq!(f.p.extensions().commit_updates(nothing_newer).summary(), "Your extensions are up to date");

        f.publish(from, "2.0", &["storage", "alarms"]);
        let report = f.check();
        assert_eq!(report.summary(), "Updated 1 extension", "{from:?}");
        let v2 = f.get(&id).unwrap();
        assert_eq!((v2.version.as_str(), v2.enabled, &v2.source, &v2.verification), ("2.0", true, &from.source(&id), &from.verification()));
        assert_eq!(report.updated[0].dir, v2.dir);
        assert!(v2.withheld.is_empty(), "{from:?}: alarms has no warning");
        assert_eq!(v2.dir.parent(), v1.dir.parent(), "{from:?}: extensions/<id>/");
        assert!(v2.dir.file_name().unwrap().to_str().unwrap().starts_with("2.0_"), "{from:?}: <version>_<hash32>");
        assert!(v1.dir.is_dir(), "{from:?}: the old dir stays until the next open (the engine may hold it)");
        assert_eq!(v2.engine_id, None, "{from:?}: a new dir must be loaded into the engine again");
        assert_eq!(f.p.ext_storage().get(&id, Area::Local, None).unwrap(), first, "{from:?}: storage.local survives");
        assert_eq!(f.p.extensions().list().unwrap().len(), 1);

        let synced = f.p.change_seq();
        f.publish(from, "3.0", &["storage", "tabs", "https://example.com/*"]);
        let report = f.check();
        assert_eq!(
            report.summary(),
            "Updated 1 extension. The newest version of the extension “Vsesvit update probe” requires more permissions, so it has been disabled. \
             It can now: Read and change your data on 127.0.0.1 and example.com; Read your browsing history",
            "{from:?}"
        );
        let v3 = f.get(&id).unwrap();
        assert_eq!(v3.version, "3.0");
        assert_eq!(texts(&v3.withheld), ["Read and change your data on 127.0.0.1 and example.com", "Read your browsing history"], "{from:?}");
        assert!(!v3.enabled, "{from:?}: off until re-enabled");
        assert_eq!(f.p.change_seq(), synced, "{from:?}: an update leaves the synced record alone");

        f.publish(from, "4.0", &["storage", "tabs", "https://example.com/*"]);
        f.check();
        let v4 = f.get(&id).unwrap();
        assert_eq!((v4.version.as_str(), v4.enabled), ("4.0", false), "{from:?}: still not re-enabled");
        assert_eq!(v4.withheld, v3.withheld);

        f.p.extensions().approve_permissions(&id).unwrap();
        let approved = f.get(&id).unwrap();
        assert!(approved.enabled && approved.withheld.is_empty(), "{from:?}");
        assert_eq!(f.p.change_seq(), synced, "{from:?}: re-enabling is local too");
        let added = PermissionSet::from_manifest_list(["tabs", "https://example.com/*"]);
        assert_eq!(f.p.prefs().get(&GRANTED_PERMISSIONS).get(&id), Some(&added), "{from:?}: re-enabling granted what 3.0 added");

        let report = f.check();
        assert!(report.updated.is_empty() && report.failed.is_empty(), "{from:?}: {report:?}");
    }
}

#[test]
fn a_version_that_drops_the_new_permissions_needs_no_re_enabling() {
    let mut f = Fixture::new();
    let id = f.publish(Kind::ChromeWebStore, "1.0", &[]);
    f.install(Kind::ChromeWebStore.source(&id));
    f.publish(Kind::ChromeWebStore, "2.0", &["history"]);
    f.check();
    assert!(!f.get(&id).unwrap().enabled);
    f.publish(Kind::ChromeWebStore, "3.0", &[]);
    f.check();
    let v3 = f.get(&id).unwrap();
    assert!(v3.enabled && v3.withheld.is_empty());
}

#[test]
fn what_the_user_granted_since_install_counts_as_approved() {
    let mut f = Fixture::new();
    let id = f.publish(Kind::ChromeWebStore, "1.0", &["storage"]);
    f.install(Kind::ChromeWebStore.source(&id));
    let history = PermissionSet { apis: ["history".to_owned()].into(), ..Default::default() };
    f.p.extensions().grant_permissions(&id, &history).unwrap();
    f.publish(Kind::ChromeWebStore, "2.0", &["storage", "history"]);
    f.check();
    let v2 = f.get(&id).unwrap();
    assert!(v2.enabled && v2.withheld.is_empty(), "as in Chrome, a version that requires a granted permission adds no warning");
}

#[test]
fn a_disabled_extension_updates_and_stays_disabled() {
    let mut f = Fixture::new();
    let id = f.publish(Kind::ChromeWebStore, "1.0", &["storage"]);
    f.install(Kind::ChromeWebStore.source(&id));
    f.p.extensions().set_enabled(&id, false).unwrap();
    f.publish(Kind::ChromeWebStore, "2.0", &["storage"]);
    assert_eq!(f.check().updated.len(), 1, "Chrome updates disabled extensions too");
    let v2 = f.get(&id).unwrap();
    assert_eq!((v2.version.as_str(), v2.enabled), ("2.0", false));

    f.publish(Kind::ChromeWebStore, "3.0", &["storage", "tabs"]);
    f.check();
    f.p.extensions().approve_permissions(&id).unwrap();
    assert!(!f.get(&id).unwrap().enabled, "re-enabling does not override the user's choice");
}

#[test]
fn a_failing_store_fails_only_its_own_extensions() {
    let mut f = Fixture::new();
    let cws = f.publish(Kind::ChromeWebStore, "1.0", &["storage"]);
    f.install(Kind::ChromeWebStore.source(&cws));
    let edge = f.publish(Kind::EdgeAddons, "1.0", &["storage"]);
    f.install(Kind::EdgeAddons.source(&edge));
    let amo = f.publish(Kind::Amo, "1.0", &["storage"]);
    f.install(Kind::Amo.source(&amo));
    for from in [Kind::ChromeWebStore, Kind::EdgeAddons, Kind::Amo] {
        f.publish(from, "2.0", &["storage"]);
    }

    let mut broken = f.store.stores();
    broken.cws_update_url = f.server.url("/no-such-store");
    f.p.set_stores(broken);
    let updates = f.run_check();
    assert!(matches!(&updates.failed[..], [(id, InstallError::Http(404))] if *id == cws), "{:?}", updates.failed);
    let report = f.p.extensions().commit_updates(updates);
    let mut updated: Vec<&ExtensionId> = report.updated.iter().map(|ext| &ext.id).collect();
    updated.sort();
    let mut expected = vec![&edge, &amo];
    expected.sort();
    assert_eq!(updated, expected);
    assert_eq!(report.failed, [(cws.clone(), "server returned HTTP 404".to_owned())]);
    assert_eq!(report.summary(), "Updated 2 extensions; 1 could not be updated");
    assert_eq!(f.get(&cws).unwrap().version, "1.0");

    f.p.set_stores(f.store.stores());
    assert_eq!(f.check().summary(), "Updated 1 extension", "the next check catches up");
}

#[test]
fn a_bad_download_fails_that_extension_only() {
    let mut f = Fixture::new();
    let cws = f.publish(Kind::ChromeWebStore, "1.0", &[]);
    f.install(Kind::ChromeWebStore.source(&cws));
    let edge = f.publish(Kind::EdgeAddons, "1.0", &[]);
    f.install(Kind::EdgeAddons.source(&edge));
    // Signed by the developer alone: no fixture publisher proof.
    f.publish(Kind::EdgeAddons, "2.0", &[]);
    let unpublished = local_crx(&update_probe_files("2.0", &[]), &CrxKey::second());
    f.server.route("/store/cws/update", move |request| {
        if request.query().iter().any(|(k, v)| k == "response" && v == "redirect") {
            vsesvit_core::testkit::FixtureResponse::ok("application/x-chrome-extension", unpublished.clone())
        } else {
            let xml = format!(r#"<gupdate><app appid="{}" status="ok"><updatecheck status="ok" version="2.0"/></app></gupdate>"#, CrxKey::second().extension_id().as_str());
            vsesvit_core::testkit::FixtureResponse::ok("text/xml", xml)
        }
    });

    let updates = f.run_check();
    assert!(matches!(&updates.failed[..], [(id, InstallError::Crx(_))] if *id == cws), "{:?}", updates.failed);
    let report = f.p.extensions().commit_updates(updates);
    assert_eq!(report.updated.iter().map(|ext| &ext.id).collect::<Vec<_>>(), [&edge]);
    assert_eq!(f.get(&cws).unwrap().version, "1.0");
}

#[test]
fn an_update_is_dropped_when_the_extension_left_meanwhile() {
    let mut f = Fixture::new();
    let id = f.publish(Kind::ChromeWebStore, "1.0", &[]);
    f.install(Kind::ChromeWebStore.source(&id));
    f.publish(Kind::ChromeWebStore, "2.0", &[]);

    let updates = f.run_check();
    assert_eq!(updates.staged.len(), 1);
    f.p.extensions().uninstall(&id).unwrap();
    let report = f.p.extensions().commit_updates(updates);
    assert!(report.updated.is_empty() && report.failed.is_empty(), "{report:?}");
    assert!(f.p.extensions().list().unwrap().is_empty(), "an uninstalled extension stays uninstalled");

    assert_eq!(f.install(Kind::ChromeWebStore.source(&id)).version, "2.0");
    f.publish(Kind::ChromeWebStore, "3.0", &[]);
    let updates = f.run_check();
    assert_eq!(updates.staged.len(), 1);
    let crx = f.t.path().join("local.crx");
    fs::write(&crx, local_crx(&update_probe_files("2.5", &[]), &Kind::ChromeWebStore.developer())).unwrap();
    let local = f.install(InstallSource::CrxFile { path: crx.clone() });
    assert_eq!(local.id, id, "the same developer key may replace the store copy");
    let report = f.p.extensions().commit_updates(updates);
    assert!(report.updated.is_empty() && report.failed.is_empty(), "{report:?}");
    let kept = f.get(&id).unwrap();
    assert_eq!((kept.version.as_str(), &kept.source), ("2.5", &InstallSource::CrxFile { path: crx }), "the local copy stays");
}

#[test]
fn only_store_installs_are_checked() {
    let mut f = Fixture::new();
    let crx = f.t.path().join("local.crx");
    fs::write(&crx, local_crx(&update_probe_files("1.0", &[]), &CrxKey::second())).unwrap();
    f.install(InstallSource::CrxFile { path: crx });
    let dev = f.t.path().join("unpacked");
    fs::create_dir_all(&dev).unwrap();
    for (name, bytes) in update_probe_files("1.0", &[]) {
        fs::write(dev.join(name), bytes).unwrap();
    }
    f.install(InstallSource::Unpacked { dir: dev });
    f.store.publish_crx(&update_probe_files("2.0", &[]), &CrxKey::second());
    assert_eq!(f.p.extensions().list().unwrap().len(), 2);
    assert!(f.p.extensions().prepare_update_check().unwrap().is_empty(), "local and unpacked installs have no store to ask");

    let id = f.publish(Kind::Amo, "1.0", &[]);
    f.install(Kind::Amo.source(&id));
    assert!(!f.p.extensions().prepare_update_check().unwrap().is_empty());
}

#[test]
fn checks_are_due_every_five_hours_after_the_first_one() {
    let now = Rc::new(Cell::new(1_000_000_000_000));
    let mut f = Fixture::with_time(TimeSource::Manual(now.clone()));
    assert_eq!(f.p.extensions().next_update_check().unwrap(), FIRST_CHECK_DELAY, "never checked here");
    f.p.extensions().prepare_update_check().unwrap();
    assert_eq!(f.p.extensions().next_update_check().unwrap(), UPDATE_INTERVAL);
    now.set(now.get() + 3_600_000);
    assert_eq!(f.p.extensions().next_update_check().unwrap(), UPDATE_INTERVAL - std::time::Duration::from_secs(3_600));

    let root = f.p.paths().root.clone();
    drop(f.p);
    let mut p = Profile::open(&root, OpenOptions { time: TimeSource::Manual(now.clone()), ..OpenOptions::default() }).unwrap();
    assert_eq!(p.extensions().next_update_check().unwrap(), UPDATE_INTERVAL - std::time::Duration::from_secs(3_600), "kept across restarts");
    now.set(now.get() + 5 * 3_600_000);
    assert_eq!(p.extensions().next_update_check().unwrap(), FIRST_CHECK_DELAY, "overdue");
    assert!(matches!(p.extensions().approve_permissions(&CrxKey::second().extension_id()), Err(Error::NotFound)));
}
