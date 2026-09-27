//! The install pipeline end to end against a real profile on disk: prepare, run on a
//! worker thread, commit, list, re-install, uninstall, reconcile, reopen. Run with
//! `--features testkit`.
#![cfg(feature = "testkit")]

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use vsesvit_core::extensions::manifest::Manifest;
use vsesvit_core::extensions::{
    ExtensionId, InstallError, InstallJob, InstallPhase, InstallSource, InstalledExtension, StagedInstall, Verification,
};
use vsesvit_core::testkit::{self, CrxKey, PROBE_FILES, write_crx3};
use vsesvit_core::{Error, OpenOptions, Profile};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("vsesvit-install-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
    fn profile_root(&self) -> PathBuf {
        self.0.join("profile")
    }
    fn open(&self) -> Profile {
        Profile::open(&self.profile_root(), OpenOptions::default()).unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs the job on its own thread, as the shells do, and collects its phases.
fn run(job: InstallJob) -> (Result<StagedInstall, InstallError>, Vec<InstallPhase>) {
    std::thread::spawn(move || {
        let mut phases = Vec::new();
        let staged = job.run(&mut |phase| phases.push(phase));
        (staged, phases)
    })
    .join()
    .unwrap()
}

fn install(p: &mut Profile, source: InstallSource) -> Result<Option<InstalledExtension>, Error> {
    let job = p.extensions().prepare_install(source)?;
    let staged = run(job).0?;
    p.extensions().commit(staged)
}

fn install_file(t: &TempDir, p: &mut Profile, name: &str, bytes: &[u8]) -> Result<Option<InstalledExtension>, Error> {
    let path = t.file(name, bytes);
    install(p, InstallSource::from_path(&path).unwrap())
}

fn probe_id() -> ExtensionId {
    ExtensionId::parse(testkit::PROBE_ID).unwrap()
}

/// The probe's files with some replaced or added.
fn probe_with(changes: &[(&'static str, &'static [u8])]) -> Vec<(&'static str, &'static [u8])> {
    let mut files: Vec<_> = PROBE_FILES.iter().copied().filter(|(name, _)| !changes.iter().any(|(n, _)| n == name)).collect();
    files.extend_from_slice(changes);
    files
}

fn dir_entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    names
}

#[test]
fn jobs_and_their_results_cross_threads() {
    fn assert_send<T: Send + 'static>() {}
    assert_send::<InstallJob>();
    assert_send::<Result<StagedInstall, InstallError>>();
}

#[test]
fn probe_crx_installs_end_to_end() {
    let t = TempDir::new();
    let crx_path = t.file("probe.crx", &testkit::probe_crx());
    let mut p = t.open();

    let source = InstallSource::parse(crx_path.to_str().unwrap()).unwrap();
    let job = p.extensions().prepare_install(source.clone()).unwrap();
    let (staged, phases) = run(job);
    let staged = staged.unwrap();
    assert_eq!(phases, [InstallPhase::Verifying, InstallPhase::Unpacking, InstallPhase::ReadingManifest]);
    assert_eq!(staged.id(), &probe_id());
    assert_eq!(staged.verification(), &Verification::LocalCrx);

    let ext = p.extensions().commit(staged).unwrap().expect("a user install always commits");
    assert_eq!(ext.id, probe_id());
    assert_eq!(ext.version, "1.0.0");
    assert_eq!(ext.source, source);
    assert_eq!(ext.verification, Verification::LocalCrx);
    assert!(ext.enabled);
    assert_eq!(ext.engine_id, None);

    // Content-addressed, immutable dir: extensions/<id>/<version>_<hash8>.
    let id_dir = t.profile_root().join("extensions").join(testkit::PROBE_ID);
    assert_eq!(ext.dir.parent().unwrap(), id_dir);
    let dir_name = ext.dir.file_name().unwrap().to_str().unwrap();
    let hash8 = dir_name.strip_prefix("1.0.0_").expect("<version>_<hash8>");
    assert!(hash8.len() == 8 && hash8.bytes().all(|b| b.is_ascii_hexdigit()), "{dir_name}");

    // The injected key keeps the CRX id for engines that load the dir unpacked.
    let on_disk = Manifest::load(&ext.dir, "en").unwrap();
    assert_eq!(on_disk.key_id(), Some(probe_id()));
    assert_eq!(ext.manifest.key_id(), Some(probe_id()));
    for (name, bytes) in PROBE_FILES.iter().filter(|(n, _)| *n != "manifest.json") {
        assert_eq!(fs::read(ext.dir.join(name)).unwrap(), *bytes, "{name}");
    }
    assert!(!ext.dir.join("_metadata").exists());
    assert_eq!(ext.manifest.content_scripts.len(), 1);
    assert!(dir_entries(&t.profile_root().join("staging")).is_empty(), "commit leaves no staging residue");

    let listed = p.extensions().list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].dir, ext.dir);

    // Re-installing the same bytes converges: same dir, nothing lost.
    p.extensions().set_engine_id(&ext.id, testkit::PROBE_ID).unwrap();
    p.extensions().set_enabled(&ext.id, false).unwrap();
    let again = install(&mut p, source.clone()).unwrap().unwrap();
    assert_eq!(again.dir, ext.dir);
    assert_eq!(again.engine_id.as_deref(), Some(testkit::PROBE_ID), "same dir: the engine's id still applies");
    assert!(!again.enabled, "re-install keeps the enabled state");
    assert_eq!(p.extensions().list().unwrap().len(), 1);
    assert_eq!(dir_entries(&id_dir), [dir_name]);

    // Local installs are not synced desired state; reconcile leaves them alone.
    let work = p.extensions().reconcile().unwrap();
    assert!(work.install.is_empty() && work.removed.is_empty());
    assert_eq!(p.extensions().list().unwrap().len(), 1);

    p.extensions().uninstall(&ext.id).unwrap();
    assert!(p.extensions().list().unwrap().is_empty());
    assert!(p.extensions().get(&ext.id).unwrap().is_none());
    assert!(!id_dir.exists(), "managed files are removed");
    assert!(matches!(p.extensions().uninstall(&ext.id), Err(Error::NotFound)));
    assert!(matches!(p.extensions().set_enabled(&ext.id, true), Err(Error::NotFound)));
    assert!(matches!(p.extensions().set_engine_id(&ext.id, "x"), Err(Error::NotFound)));

    // And it can come back.
    assert!(install(&mut p, source).unwrap().unwrap().enabled);
}

#[test]
fn changed_bytes_with_the_same_version_get_a_new_dir() {
    let t = TempDir::new();
    let mut p = t.open();
    let first = install_file(&t, &mut p, "a.crx", &testkit::probe_crx()).unwrap().unwrap();
    p.extensions().set_engine_id(&first.id, "engine-id").unwrap();

    let changed = write_crx3(&probe_with(&[("background.js", b"// changed\n")]), &CrxKey::probe());
    let second = install_file(&t, &mut p, "b.crx", &changed).unwrap().unwrap();
    assert_ne!(second.dir, first.dir, "an existing dir is never modified");
    assert!(first.dir.is_dir(), "the old dir stays until the next open (the engine may hold it)");
    assert_eq!(second.engine_id, None, "a new dir must be loaded into the engine again");
    assert_eq!(fs::read(second.dir.join("background.js")).unwrap(), b"// changed\n");
    drop(p);

    let mut p = t.open();
    assert!(!first.dir.exists(), "open collects dirs no row references");
    assert_eq!(p.extensions().list().unwrap()[0].dir, second.dir);
}

#[test]
fn an_older_version_never_replaces_a_newer_one() {
    let t = TempDir::new();
    let mut p = t.open();
    let manifest_v2: &[u8] = br#"{"manifest_version": 3, "name": "Vsesvit Probe", "version": "2.0"}"#;
    let newer = install_file(&t, &mut p, "new.crx", &write_crx3(&[("manifest.json", manifest_v2)], &CrxKey::probe())).unwrap().unwrap();
    let kept = install_file(&t, &mut p, "old.crx", &testkit::probe_crx()).unwrap().unwrap();
    assert_eq!(kept.version, "2.0");
    assert_eq!(kept.dir, newer.dir);
    assert_eq!(dir_entries(&t.profile_root().join("extensions").join(testkit::PROBE_ID)).len(), 1);
}

#[test]
fn open_housekeeping_wipes_staging_and_collects_garbage() {
    let t = TempDir::new();
    let mut p = t.open();
    let ext = install_file(&t, &mut p, "probe.crx", &testkit::probe_crx()).unwrap().unwrap();
    drop(p);

    let root = t.profile_root();
    fs::create_dir_all(root.join("staging/crashed-job/root")).unwrap();
    fs::write(root.join("staging/crashed-job/root/manifest.json"), b"{}").unwrap();
    let stray_version = root.join("extensions").join(testkit::PROBE_ID).join("0.9.0_deadbeef");
    let stray_id = root.join("extensions/kpjcccehndkfpfbbofnjkcelnoonmhja/1.0_00000000");
    fs::create_dir_all(&stray_version).unwrap();
    fs::create_dir_all(&stray_id).unwrap();

    let mut p = t.open();
    assert!(dir_entries(&root.join("staging")).is_empty());
    assert!(!stray_version.exists());
    assert!(!root.join("extensions/kpjcccehndkfpfbbofnjkcelnoonmhja").exists());
    assert!(ext.dir.join("manifest.json").is_file(), "referenced dirs survive");
    assert_eq!(p.extensions().list().unwrap().len(), 1);
    drop(p);

    // A row whose dir vanished is dropped; the next install recreates it.
    fs::remove_dir_all(&ext.dir).unwrap();
    let mut p = t.open();
    assert!(p.extensions().list().unwrap().is_empty());
}

#[test]
fn unpacked_dirs_load_in_place_and_reload() {
    let t = TempDir::new();
    let dev = t.path().join("dev-ext");
    fs::create_dir_all(&dev).unwrap();
    for (name, bytes) in PROBE_FILES {
        fs::write(dev.join(name), bytes).unwrap();
    }
    let mut p = t.open();
    let job = p.extensions().prepare_install(InstallSource::parse(dev.to_str().unwrap()).unwrap()).unwrap();
    let (staged, phases) = run(job);
    assert_eq!(phases, [InstallPhase::ReadingManifest]);
    let ext = p.extensions().commit(staged.unwrap()).unwrap().unwrap();
    let abs = std::path::absolute(&dev).unwrap();
    assert_eq!(ext.dir, abs, "loaded in place, never copied");
    assert_eq!(ext.id, ExtensionId::for_unpacked_dir(&abs), "no key: the id comes from the path");
    assert_eq!(ext.verification, Verification::Unpacked);
    assert!(!t.profile_root().join("extensions").join(ext.id.as_str()).exists());

    let edited = String::from_utf8(PROBE_FILES[0].1.to_vec()).unwrap().replace("\"Vsesvit Probe\"", "\"Probe (edited)\"");
    fs::write(dev.join("manifest.json"), &edited).unwrap();
    let reloaded = p.extensions().reload_unpacked(&ext.id).unwrap();
    assert_eq!(reloaded.manifest.name, "Probe (edited)");

    // Adding a key would change the id: that is a different extension, not a reload.
    let key = base64_spki(&CrxKey::second());
    fs::write(dev.join("manifest.json"), edited.replacen('{', &format!("{{\"key\": \"{key}\","), 1)).unwrap();
    assert!(matches!(p.extensions().reload_unpacked(&ext.id), Err(Error::Install(InstallError::IdMismatch { .. }))));

    p.extensions().uninstall(&ext.id).unwrap();
    assert!(dev.join("manifest.json").is_file(), "uninstall never deletes a developer's dir");

    // With the key, a fresh load takes the key's id.
    let keyed = install(&mut p, InstallSource::Unpacked { dir: abs }).unwrap().unwrap();
    assert_eq!(keyed.id, CrxKey::second().extension_id());
    assert!(matches!(p.extensions().reload_unpacked(&probe_id()), Err(Error::NotFound)));
}

#[test]
fn a_local_crx_is_not_reloadable() {
    let t = TempDir::new();
    let mut p = t.open();
    install_file(&t, &mut p, "probe.crx", &testkit::probe_crx()).unwrap().unwrap();
    assert!(matches!(p.extensions().reload_unpacked(&probe_id()), Err(Error::Install(InstallError::NotUnpacked))));
}

fn base64_spki(key: &CrxKey) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(key.public_key_der())
}

// ---------------------------------------------------------------------------
// Hostile archives
// ---------------------------------------------------------------------------

const XPI_MANIFEST: &[u8] =
    br#"{"manifest_version": 2, "name": "X", "version": "1.0", "browser_specific_settings": {"gecko": {"id": "x@example.org"}}}"#;

/// A zip with entries the testkit writer would never produce.
fn raw_zip(entries: &[(&str, &[u8])], symlinks: &[(&str, &str)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        w.start_file(*name, options).unwrap();
        w.write_all(data).unwrap();
    }
    for (name, target) in symlinks {
        w.add_symlink(*name, *target, options).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn assert_nothing_escaped(t: &TempDir) {
    let mut stack = vec![t.path().to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            assert_ne!(entry.file_name(), "evil.js", "{} escaped", entry.path().display());
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            }
        }
    }
    assert!(dir_entries(&t.profile_root().join("staging")).is_empty(), "a failed job leaves no staging dir");
}

#[test]
fn zip_slip_is_rejected() {
    let t = TempDir::new();
    let mut p = t.open();
    for evil in ["../evil.js", "../../evil.js", "a/../../evil.js", "/evil.js", "C:/evil.js", "a\\..\\..\\evil.js"] {
        let xpi = raw_zip(&[("manifest.json", XPI_MANIFEST), (evil, b"alert(1)")], &[]);
        match install_file(&t, &mut p, "evil.xpi", &xpi) {
            Err(Error::Install(InstallError::UnsafePath(name))) => assert_eq!(name, evil),
            other => panic!("{evil:?}: {other:?}"),
        }
    }
    // The signed path runs the same checks after verification.
    let crx = write_crx3(&probe_with(&[("../evil.js", b"alert(1)")]), &CrxKey::probe());
    assert!(matches!(install_file(&t, &mut p, "evil.crx", &crx), Err(Error::Install(InstallError::UnsafePath(_)))));
    assert_nothing_escaped(&t);
    assert!(p.extensions().list().unwrap().is_empty());
}

#[test]
fn symlinks_are_rejected() {
    let t = TempDir::new();
    let mut p = t.open();
    for target in ["/etc/passwd", "../../outside", "manifest.json"] {
        let xpi = raw_zip(&[("manifest.json", XPI_MANIFEST)], &[("link.js", target)]);
        assert!(
            matches!(install_file(&t, &mut p, "link.xpi", &xpi), Err(Error::Install(InstallError::UnsafePath(n))) if n == "link.js"),
            "{target}"
        );
    }
    assert_nothing_escaped(&t);
}

#[test]
fn reserved_and_unportable_names_are_rejected() {
    let t = TempDir::new();
    let mut p = t.open();
    for name in ["CON", "js/nul.js", "Com1.txt", "_private/x.js", "_x.js", "a:b.js", "trailing.", "q?.js"] {
        let xpi = raw_zip(&[("manifest.json", XPI_MANIFEST), (name, b"x")], &[]);
        assert!(matches!(install_file(&t, &mut p, "bad.xpi", &xpi), Err(Error::Install(InstallError::UnsafePath(_)))), "{name:?}");
    }
    let dupes = raw_zip(&[("manifest.json", XPI_MANIFEST), ("a.js", b"1"), ("A.JS", b"2")], &[]);
    assert!(matches!(install_file(&t, &mut p, "dupe.xpi", &dupes), Err(Error::Install(InstallError::UnsafePath(n))) if n == "A.JS"));
    let nested = raw_zip(&[("ext/manifest.json", XPI_MANIFEST)], &[]);
    assert!(matches!(install_file(&t, &mut p, "nested.xpi", &nested), Err(Error::Install(InstallError::Zip(_)))));
    assert_nothing_escaped(&t);
}

#[test]
fn archive_bombs_are_rejected() {
    let t = TempDir::new();
    let mut p = t.open();
    let zeros = vec![0u8; 8 << 20];
    let bomb = raw_zip(&[("manifest.json", XPI_MANIFEST), ("zeros.bin", &zeros)], &[]);
    assert!(bomb.len() < 64 << 10, "deflate makes 8 MiB of zeros tiny ({} bytes)", bomb.len());
    assert!(matches!(install_file(&t, &mut p, "bomb.xpi", &bomb), Err(Error::Install(InstallError::TooLargeUnpacked(_)))));
    assert_nothing_escaped(&t);
}

#[test]
fn xpi_metadata_is_stripped_and_the_gecko_id_is_the_id() {
    let t = TempDir::new();
    let mut p = t.open();
    let xpi = raw_zip(
        &[
            ("manifest.json", XPI_MANIFEST),
            ("_metadata/verified_contents.json", b"{}"),
            ("__MACOSX/._manifest.json", b""),
            ("_locales/en/messages.json", b"{}"),
            ("META-INF/mozilla.rsa", b"sig"),
        ],
        &[],
    );
    let ext = install_file(&t, &mut p, "x.xpi", &xpi).unwrap().unwrap();
    assert_eq!(ext.id.as_str(), "x@example.org");
    assert_eq!(ext.verification, Verification::LocalXpi);
    assert_eq!(dir_entries(&ext.dir), ["META-INF", "_locales", "manifest.json"]);
    assert_eq!(ext.manifest.key, None, "no key is injected into an XPI");

    let no_id = raw_zip(&[("manifest.json", br#"{"manifest_version": 2, "name": "Y", "version": "1"}"#)], &[]);
    let path = t.file("no-id.xpi", &no_id);
    let ext = install(&mut p, InstallSource::from_path(&path).unwrap()).unwrap().unwrap();
    assert_eq!(ext.id, ExtensionId::for_unpacked_dir(&std::path::absolute(&path).unwrap()));
}

#[test]
fn crx_problems_surface_as_install_errors() {
    let t = TempDir::new();
    let mut p = t.open();
    let mut tampered = testkit::probe_crx();
    let last = tampered.len() - 30;
    tampered[last] ^= 0xff;
    assert!(matches!(install_file(&t, &mut p, "t.crx", &tampered), Err(Error::Install(InstallError::Crx(_)))));
    let missing = t.path().join("missing.crx");
    assert!(matches!(install(&mut p, InstallSource::CrxFile { path: missing }), Err(Error::Install(InstallError::Io(_)))));
    let no_manifest = write_crx3(&[("readme.txt", b"hi")], &CrxKey::probe());
    assert!(matches!(install_file(&t, &mut p, "n.crx", &no_manifest), Err(Error::Install(InstallError::Zip(_)))));
    let bad_manifest = write_crx3(&[("manifest.json", br#"{"manifest_version": 3, "name": "x", "version": "x"}"#)], &CrxKey::probe());
    assert!(matches!(install_file(&t, &mut p, "m.crx", &bad_manifest), Err(Error::Install(InstallError::Manifest(_)))));
    assert!(p.extensions().list().unwrap().is_empty());
}
