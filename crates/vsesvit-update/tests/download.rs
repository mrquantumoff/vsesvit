mod support;

use std::path::Path;

use semver::Version;
use support::{Server, Signer, artifact, config};
use vsesvit_update::{Downloaded, Error, Format, Installation, Update, Updater, remove_stale_downloads};

struct Release {
    server: Server,
    signer: Signer,
}

impl Release {
    /// Announces `announced` over the dynamic format, serving `bytes` with `signature`.
    fn serve(signer: Signer, announced: &str, bytes: Vec<u8>, signature: String) -> Release {
        let server = Server::start();
        server.route("/artifact", 200, bytes);
        let body = serde_json::json!({
            "version": announced,
            "url": server.url("/artifact").as_str(),
            "signature": signature,
        });
        server.route("/update", 200, body.to_string());
        Release { server, signer }
    }

    fn update(&self, installation: Installation) -> Update {
        let config = config(self.signer.pubkey(), vec![self.server.url("/update")]);
        let updater = Updater::new(config, Version::new(0, 1, 0), installation).unwrap();
        updater.check("stable").unwrap().expect("the release is newer").into_update().unwrap()
    }
}

fn signed(format: Format, signed_version: &str) -> Release {
    let signer = Signer::new();
    let bytes = artifact(format);
    let signature = signer.sign_release(&bytes, signed_version);
    Release::serve(signer, "0.2.0", bytes, signature)
}

fn download(update: &Update, dir: &Path) -> Result<Downloaded, Error> {
    update.download(dir, |_, _| {})
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
}

#[test]
fn a_correctly_signed_artifact_downloads_and_verifies() {
    let release = signed(Format::Deb, "0.2.0");
    let dir = tempfile::tempdir().unwrap();
    let mut progress = Vec::new();
    let downloaded = release.update(Installation::Deb).download(dir.path(), |received, total| progress.push((received, total))).unwrap();

    let expected = artifact(Format::Deb);
    assert_eq!(std::fs::read(downloaded.path()).unwrap(), expected);
    assert_eq!(downloaded.format(), Format::Deb);
    assert_eq!(downloaded.version(), &Version::new(0, 2, 0));
    assert_eq!(downloaded.path().file_name().unwrap(), "vsesvit-0.2.0.deb");
    assert_eq!(leftovers(dir.path()), ["vsesvit-0.2.0.deb"], "no partial file remains");

    let len = expected.len() as u64;
    assert_eq!(progress.first(), Some(&(0, Some(len))), "progress starts at zero with the content length");
    assert_eq!(progress.last(), Some(&(len, Some(len))), "progress ends at the full length");
    assert!(progress.windows(2).all(|w| w[0].0 <= w[1].0), "progress only grows");

    let request = release.server.requests().into_iter().find(|r| r.target == "/artifact").unwrap();
    assert_eq!(request.headers.get("accept").map(String::as_str), Some("application/octet-stream"));
}

#[test]
fn a_leading_v_in_the_signed_version_matches() {
    let release = signed(Format::Rpm, "v0.2.0");
    let dir = tempfile::tempdir().unwrap();
    download(&release.update(Installation::Rpm), dir.path()).expect("v0.2.0 is 0.2.0");
}

fn rejected(release: Release, installation: Installation) -> Error {
    let dir = tempfile::tempdir().unwrap();
    let err = download(&release.update(installation), dir.path()).unwrap_err();
    assert_eq!(leftovers(dir.path()), Vec::<String>::new(), "a rejected download is deleted");
    err
}

#[test]
fn a_signature_over_other_bytes_is_rejected() {
    let signer = Signer::new();
    let signature = signer.sign_release(b"some other file", "0.2.0");
    let err = rejected(Release::serve(signer, "0.2.0", artifact(Format::Deb), signature), Installation::Deb);
    assert!(matches!(err, Error::Signature(_)), "{err:?}");
}

#[test]
fn a_signature_from_another_key_is_rejected() {
    let bytes = artifact(Format::Deb);
    let signature = Signer::new().sign_release(&bytes, "0.2.0");
    let err = rejected(Release::serve(Signer::new(), "0.2.0", bytes, signature), Installation::Deb);
    assert!(matches!(err, Error::Signature(_)), "{err:?}");
}

#[test]
fn a_signature_that_is_not_base64_minisign_is_rejected() {
    let err = rejected(Release::serve(Signer::new(), "0.2.0", artifact(Format::Deb), "c2ln".into()), Installation::Deb);
    assert!(matches!(err, Error::Signature(_)), "{err:?}");
}

#[test]
fn an_older_release_announced_as_newer_is_rejected() {
    let err = rejected(signed(Format::Deb, "0.1.5"), Installation::Deb);
    let Error::SignedVersionMismatch { signed, announced } = err else { panic!("expected a mismatch, got {err:?}") };
    assert_eq!((signed.as_str(), announced), ("0.1.5", Version::new(0, 2, 0)));
}

#[test]
fn a_signature_without_a_version_is_rejected() {
    let signer = Signer::new();
    let bytes = artifact(Format::Deb);
    let signature = signer.sign(&bytes, "timestamp:1790000000\tfile:artifact");
    let err = rejected(Release::serve(signer, "0.2.0", bytes, signature), Installation::Deb);
    assert!(matches!(err, Error::Signature(_)), "{err:?}");
}

#[test]
fn a_signed_artifact_of_the_wrong_format_is_rejected() {
    for (served, installation) in [
        (Format::Rpm, Installation::Deb),
        (Format::Deb, Installation::Rpm),
        (Format::Nsis, Installation::Pacman),
        (Format::Deb, Installation::AppImage { image: "/opt/Vsesvit.AppImage".into() }),
        (Format::AppImage, Installation::Nsis),
    ] {
        let expected = installation.format().unwrap();
        let err = rejected(signed(served, "0.2.0"), installation);
        assert!(matches!(err, Error::WrongArtifactType(f) if f == expected), "{served:?} as {expected:?}: {err:?}");
    }
}

#[test]
fn a_plain_elf_is_not_an_appimage() {
    let signer = Signer::new();
    let mut bytes = artifact(Format::AppImage);
    bytes[8..11].copy_from_slice(&[0, 0, 0]);
    let signature = signer.sign_release(&bytes, "0.2.0");
    let err = rejected(Release::serve(signer, "0.2.0", bytes, signature), Installation::AppImage { image: "/a".into() });
    assert!(matches!(err, Error::WrongArtifactType(Format::AppImage)), "{err:?}");
}

#[test]
fn an_artifact_the_server_does_not_have_is_an_http_error() {
    let release = signed(Format::Deb, "0.2.0");
    release.server.route("/artifact", 404, "gone");
    let err = rejected(release, Installation::Deb);
    assert!(matches!(err, Error::Http(404)), "{err:?}");
}

fn artifact_requests(release: &Release) -> usize {
    release.server.requests().iter().filter(|r| r.target == "/artifact").count()
}

#[test]
fn a_verified_earlier_download_is_used_without_fetching() {
    let release = signed(Format::Pacman, "0.2.0");
    let dir = tempfile::tempdir().unwrap();
    let update = release.update(Installation::Pacman);
    download(&update, dir.path()).unwrap();
    let mut progress = Vec::new();
    let again = update.download(dir.path(), |received, total| progress.push((received, total))).unwrap();
    assert_eq!(again.path().file_name().unwrap(), "vsesvit-0.2.0.pkg.tar.zst");
    assert_eq!(leftovers(dir.path()).len(), 1);
    assert_eq!(artifact_requests(&release), 1, "the second download fetched nothing");
    assert!(progress.is_empty());
}

#[test]
fn an_earlier_file_that_does_not_verify_is_downloaded_again() {
    let release = signed(Format::Deb, "0.2.0");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("vsesvit-0.2.0.deb"), b"!<arch>\ndebian-binary tampered").unwrap();
    let downloaded = download(&release.update(Installation::Deb), dir.path()).unwrap();
    assert_eq!(std::fs::read(downloaded.path()).unwrap(), artifact(Format::Deb));
    assert_eq!(artifact_requests(&release), 1);
}

#[test]
fn stale_downloads_are_removed_and_the_kept_version_stays() {
    let dir = tempfile::tempdir().unwrap();
    let names = [
        "vsesvit-0.9.0.deb",
        "vsesvit-1.1.0.deb",
        "vsesvit-1.1.0.deb.part",
        "vsesvit-1.2.0-setup.exe",
        "vsesvit-1.2.0-setup.exe.part",
        "notes.txt",
    ];
    for name in names {
        std::fs::write(dir.path().join(name), b"x").unwrap();
    }
    remove_stale_downloads(dir.path(), Some(&Version::new(1, 1, 0))).unwrap();
    let mut left = leftovers(dir.path());
    left.sort();
    assert_eq!(left, ["notes.txt", "vsesvit-1.1.0.deb", "vsesvit-1.1.0.deb.part"], "a partial file can be resumed");

    remove_stale_downloads(dir.path(), None).unwrap();
    assert_eq!(leftovers(dir.path()), ["notes.txt"]);
    remove_stale_downloads(&dir.path().join("missing"), None).expect("a missing directory has nothing stale");
}

#[test]
fn a_partial_download_in_progress_survives_stale_cleanup() {
    let release = signed(Format::Deb, "0.2.0");
    let dir = tempfile::tempdir().unwrap();
    let mut cleaned = false;
    let downloaded = release
        .update(Installation::Deb)
        .download(dir.path(), |_, _| {
            if !std::mem::replace(&mut cleaned, true) {
                // Another profile, which found nothing to keep, cleans up meanwhile.
                remove_stale_downloads(dir.path(), None).unwrap();
            }
        })
        .expect("the cleanup left the partial file alone");
    assert_eq!(std::fs::read(downloaded.path()).unwrap(), artifact(Format::Deb));
}

#[test]
fn two_downloads_of_one_version_into_one_dir_both_succeed() {
    let release = signed(Format::Deb, "0.2.0");
    let dir = tempfile::tempdir().unwrap();
    let update = release.update(Installation::Deb);
    let mut second = None;
    let first = update
        .download(dir.path(), |_, _| {
            if second.is_none() {
                // Another profile downloads the same update while this one starts.
                second = Some(download(&update, dir.path()).expect("the second download succeeds"));
            }
        })
        .expect("the first download succeeds");
    let second = second.unwrap();
    assert_eq!(first.path(), second.path());
    assert_eq!(std::fs::read(first.path()).unwrap(), artifact(Format::Deb));
    assert_eq!(leftovers(dir.path()), ["vsesvit-0.2.0.deb"]);
}

fn partial_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut partial: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    partial.retain(|path| path.extension().is_some_and(|ext| ext == "part"));
    partial
}

fn artifact_ranges(release: &Release) -> Vec<Option<String>> {
    let requests = release.server.requests().into_iter().filter(|r| r.target == "/artifact");
    requests.map(|r| r.headers.get("range").cloned()).collect()
}

#[test]
fn a_dropped_download_asks_for_the_rest() {
    let release = signed(Format::Deb, "0.2.0");
    let len = artifact(Format::Deb).len();
    release.server.resumable("/artifact", &[len / 2, len / 4]);
    let dir = tempfile::tempdir().unwrap();
    let mut progress = Vec::new();
    let downloaded =
        release.update(Installation::Deb).download(dir.path(), |received, total| progress.push((received, total))).unwrap();
    assert_eq!(std::fs::read(downloaded.path()).unwrap(), artifact(Format::Deb));
    assert_eq!(leftovers(dir.path()), ["vsesvit-0.2.0.deb"]);
    let (half, three_quarters) = (len / 2, len / 2 + len / 4);
    assert_eq!(
        artifact_ranges(&release),
        [None, Some(format!("bytes={half}-")), Some(format!("bytes={three_quarters}-"))]
    );
    assert!(progress.windows(2).all(|w| w[0].0 <= w[1].0), "progress only grows");
    assert_eq!(progress.last(), Some(&(len as u64, Some(len as u64))));
}

#[test]
fn a_failed_download_resumes_from_the_partial_file() {
    let release = signed(Format::Deb, "0.2.0");
    let half = artifact(Format::Deb).len() / 2;
    // Half the file, then nothing: the download gives up.
    release.server.resumable("/artifact", &[half, 0]);
    let dir = tempfile::tempdir().unwrap();
    let update = release.update(Installation::Deb);

    let err = download(&update, dir.path()).unwrap_err();
    assert!(matches!(err, Error::Network(_)), "{err:?}");
    // A check from another profile, which keeps the version being downloaded.
    remove_stale_downloads(dir.path(), Some(&Version::new(0, 2, 0))).unwrap();
    let partial = partial_files(dir.path());
    assert_eq!(partial.len(), 1, "the partial file is kept: {:?}", leftovers(dir.path()));
    assert_eq!(std::fs::metadata(&partial[0]).unwrap().len(), half as u64);

    let mut progress = Vec::new();
    let downloaded = update.download(dir.path(), |received, total| progress.push((received, total))).unwrap();
    assert_eq!(std::fs::read(downloaded.path()).unwrap(), artifact(Format::Deb));
    assert_eq!(leftovers(dir.path()), ["vsesvit-0.2.0.deb"]);
    let len = artifact(Format::Deb).len() as u64;
    assert_eq!(progress.first(), Some(&(half as u64, Some(len))), "progress starts where the first download stopped");
    let range = Some(format!("bytes={half}-"));
    assert_eq!(artifact_ranges(&release), [None, range.clone(), range]);
}

#[test]
fn a_partial_file_the_server_cannot_resume_is_downloaded_again() {
    for resumable in [false, true] {
        let release = signed(Format::Deb, "0.2.0");
        if resumable {
            release.server.resumable("/artifact", &[]);
        }
        let dir = tempfile::tempdir().unwrap();
        // Longer than the artifact, so a resumable server answers 416, and one that ignores
        // `Range` sends the whole file.
        std::fs::write(dir.path().join("vsesvit-0.2.0.deb.1-0.part"), vec![0; artifact(Format::Deb).len() + 1]).unwrap();
        let downloaded = download(&release.update(Installation::Deb), dir.path()).unwrap();
        assert_eq!(std::fs::read(downloaded.path()).unwrap(), artifact(Format::Deb), "resumable: {resumable}");
        assert_eq!(leftovers(dir.path()), ["vsesvit-0.2.0.deb"]);
    }
}
