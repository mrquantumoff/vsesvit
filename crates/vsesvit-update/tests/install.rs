mod support;

use std::path::Path;

use semver::Version;
use support::{Server, Signer, artifact, config};
use vsesvit_update::{DisabledReason, Downloaded, Error, Format, Installation, Updater};

/// A verified download of `format`, fetched as `as_installation` would fetch it.
fn downloaded(format: Format, as_installation: Installation, dir: &Path) -> (Downloaded, Server) {
    let signer = Signer::new();
    let bytes = artifact(format);
    let server = Server::start();
    let body = serde_json::json!({
        "version": "0.2.0",
        "url": server.url("/artifact").as_str(),
        "signature": signer.sign_release(&bytes, "0.2.0"),
    });
    server.route("/artifact", 200, bytes);
    server.route("/update", 200, body.to_string());
    let updater = Updater::new(config(signer.pubkey(), vec![server.url("/update")]), Version::new(0, 1, 0), as_installation);
    let update = updater.unwrap().check().unwrap().unwrap();
    (update.download(dir, |_, _| {}).unwrap(), server)
}

#[test]
fn installations_that_do_not_update_themselves_refuse_to_install() {
    let dir = tempfile::tempdir().unwrap();
    for installation in [Installation::Unpackaged, Installation::Flatpak] {
        let (downloaded, _server) = downloaded(Format::Deb, Installation::Deb, dir.path());
        let err = downloaded.install(&installation, &[]).unwrap_err();
        assert!(matches!(err, Error::Disabled(DisabledReason::NotSelfUpdating)), "{installation:?}: {err:?}");
    }
}

#[test]
fn an_artifact_for_another_installation_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (downloaded, _server) = downloaded(Format::Deb, Installation::Deb, dir.path());
    let err = downloaded.install(&Installation::Rpm, &[]).unwrap_err();
    assert!(matches!(err, Error::WrongArtifactType(Format::Rpm)), "{err:?}");
    assert!(dir.path().join("vsesvit-0.2.0.deb").exists(), "a refused install leaves the file alone");
}

#[cfg(windows)]
#[test]
fn linux_packages_do_not_install_on_windows() {
    let dir = tempfile::tempdir().unwrap();
    let (downloaded, _server) = downloaded(Format::Deb, Installation::Deb, dir.path());
    let err = downloaded.install(&Installation::Deb, &[]).unwrap_err();
    assert!(matches!(err, Error::Install(_)), "{err:?}");
}

#[cfg(not(windows))]
#[test]
fn the_windows_installer_does_not_run_on_linux() {
    let dir = tempfile::tempdir().unwrap();
    let nsis = Installation::Nsis { install_dir: dir.path().to_path_buf() };
    let (downloaded, _server) = downloaded(Format::Nsis, nsis.clone(), dir.path());
    let err = downloaded.install(&nsis, &[]).unwrap_err();
    assert!(matches!(err, Error::Install(_)), "{err:?}");
}

#[cfg(unix)]
#[test]
fn an_appimage_update_replaces_the_image_for_the_next_launch() {
    use std::os::unix::fs::PermissionsExt;

    let apps = tempfile::tempdir().unwrap();
    let image = apps.path().join("Vsesvit.AppImage");
    std::fs::write(&image, b"old image").unwrap();
    std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o750)).unwrap();
    let installation = Installation::AppImage { image: image.clone() };

    let cache = tempfile::tempdir().unwrap();
    let (downloaded, _server) = downloaded(Format::AppImage, installation.clone(), cache.path());
    let installed = downloaded.install(&installation, &[]).unwrap();

    assert_eq!(installed, vsesvit_update::Installed::NextLaunch);
    assert_eq!(std::fs::read(&image).unwrap(), artifact(Format::AppImage), "the image holds the new bytes");
    assert_eq!(std::fs::metadata(&image).unwrap().permissions().mode() & 0o777, 0o750, "the old mode is kept");
    let names: Vec<_> = std::fs::read_dir(apps.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["Vsesvit.AppImage"], "no temporary file is left next to the image");
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0, "the download is removed once applied");
}
