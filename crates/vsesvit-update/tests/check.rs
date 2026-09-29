mod support;

use std::path::PathBuf;

use semver::Version;
use support::{Server, config, dead_url};
use url::Url;
use vsesvit_update::{ARCH, Available, DisabledReason, Error, Format, Installation, TARGET, Update, Updater};

const PUBKEY: &str = "UldRZjM5aGF0Y2hlZA==";

fn updater(endpoints: Vec<Url>, current: &str, installation: Installation) -> Updater {
    let key = support::Signer::new().pubkey();
    Updater::new(config(key, endpoints), Version::parse(current).unwrap(), installation).unwrap()
}

fn templated(server: &Server) -> Url {
    server.url("/u/{{target}}/{{arch}}/{{current_version}}?variant={{bundle_type}}")
}

fn dynamic(version: &str) -> String {
    format!(
        r#"{{"version": "{version}", "notes": "Fixes", "pub_date": "2026-09-20T12:00:00Z",
            "url": "https://dl.test/Vsesvit_{version}_amd64.deb", "signature": "c2ln"}}"#
    )
}

fn check(updater: &Updater) -> Option<Update> {
    updater.check("stable").expect("check succeeds").map(|available| available.into_update().expect("installable"))
}

#[test]
fn dynamic_format_offers_a_newer_release() {
    let server = Server::start();
    server.route(&format!("/u/{TARGET}/{ARCH}/1.0.0"), 200, dynamic("1.1.0"));
    let update = check(&updater(vec![templated(&server)], "1.0.0", Installation::Deb)).expect("1.1.0 is newer");

    assert_eq!(update.release.version, Version::new(1, 1, 0));
    assert_eq!(update.release.notes.as_deref(), Some("Fixes"));
    assert_eq!(update.release.pub_date.map(|d| d.unix_timestamp()), Some(1_789_905_600));
    assert_eq!(update.url.as_str(), "https://dl.test/Vsesvit_1.1.0_amd64.deb");
    assert_eq!(update.signature, "c2ln");
    assert_eq!(update.format, Format::Deb);

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].target, format!("/u/{TARGET}/{ARCH}/1.0.0?variant=deb"), "every placeholder is filled");
    assert_eq!(requests[0].headers.get("accept").map(String::as_str), Some("application/json"));
    assert_eq!(requests[0].headers.get("user-agent").map(String::as_str), Some("vsesvit/1.0.0"));
}

#[test]
fn build_metadata_in_the_current_version_is_percent_encoded() {
    let server = Server::start();
    let _ = updater(vec![templated(&server)], "1.0.0+nightly.3", Installation::Unpackaged).check("stable");
    assert_eq!(
        server.requests()[0].target,
        format!("/u/{TARGET}/{ARCH}/1.0.0%2Bnightly.3?variant=unknown"),
        "`+` is encoded and an unpackaged build asks for bundle type `unknown`, as Tauri does"
    );
}

fn static_manifest(keys: &[&str]) -> String {
    let platforms: Vec<String> = keys
        .iter()
        .map(|key| format!(r#""{key}": {{"url": "https://dl.test/{key}", "signature": "sig-{key}"}}"#))
        .collect();
    format!(r#"{{"version": "v2.0.0", "notes": null, "platforms": {{{}}}}}"#, platforms.join(","))
}

#[test]
fn static_format_prefers_the_variant_key() {
    let server = Server::start();
    let variant_key = format!("{TARGET}-{ARCH}-rpm");
    let plain_key = format!("{TARGET}-{ARCH}");
    server.route("/latest.json", 200, static_manifest(&[&plain_key, &variant_key, "linux-riscv64-rpm"]));
    let update = check(&updater(vec![server.url("/latest.json")], "1.0.0", Installation::Rpm)).expect("v2.0.0 is newer");
    assert_eq!(update.release.version, Version::new(2, 0, 0), "the leading v is accepted");
    assert_eq!(update.url.as_str(), format!("https://dl.test/{variant_key}"));
    assert_eq!(update.signature, format!("sig-{variant_key}"));
    assert_eq!(update.release.pub_date, None);
}

#[test]
fn static_format_falls_back_to_the_plain_key() {
    let server = Server::start();
    let plain_key = format!("{TARGET}-{ARCH}");
    server.route("/latest.json", 200, static_manifest(&[&plain_key, &format!("{TARGET}-{ARCH}-deb")]));
    let update = check(&updater(vec![server.url("/latest.json")], "1.0.0", Installation::Pacman)).unwrap();
    assert_eq!(update.url.as_str(), format!("https://dl.test/{plain_key}"));
}

#[test]
fn static_format_without_a_matching_key_names_the_keys_tried() {
    let server = Server::start();
    server.route("/latest.json", 200, static_manifest(&["plan9-mips"]));
    let err = updater(vec![server.url("/latest.json")], "1.0.0", Installation::Rpm).check("stable").unwrap_err();
    let Error::NoArtifactForTarget(keys) = err else { panic!("expected NoArtifactForTarget, got {err:?}") };
    assert_eq!(keys, [format!("{TARGET}-{ARCH}-rpm"), format!("{TARGET}-{ARCH}")]);
}

#[test]
fn an_older_release_without_our_key_is_simply_no_update() {
    let server = Server::start();
    server.route("/latest.json", 200, static_manifest(&["plan9-mips"]));
    assert!(check(&updater(vec![server.url("/latest.json")], "3.0.0", Installation::Rpm)).is_none());
}

#[test]
fn quadrant_database_row_is_accepted_verbatim() {
    let server = Server::start();
    let row = r#"{
  "product": "vsesvit",
  "version": "0.2.0",
  "pub_date": "2026-09-20T12:34:56.789012+02:00",
  "arch": "x86_64",
  "signature": "dW50cnVzdGVkIGNvbW1lbnQ=",
  "platform": "linux",
  "url": "https://github.com/mrquantumoff/vsesvit/releases/download/0.2.0/Vsesvit_0.2.0_amd64.deb",
  "branch": "stable",
  "public": true,
  "version_id": "0b1c1a8e-5d4f-4f7e-9d59-3f0a3c6d1e2b",
  "platform_variation": "deb"
}"#;
    server.route(&format!("/api/any/vsesvit/updates/nightly/{TARGET}/{ARCH}/0.1.0"), 200, row);
    let endpoint = server
        .url("/api/any/vsesvit/updates/{{channel}}/{{target}}/{{arch}}/{{current_version}}?variant={{bundle_type}}");
    let available = updater(vec![endpoint], "0.1.0", Installation::Deb).check("nightly").expect("check succeeds");
    let update = available.expect("0.2.0 is newer").into_update().expect("installable");
    assert_eq!(update.release.version, Version::new(0, 2, 0));
    assert_eq!(update.signature, "dW50cnVzdGVkIGNvbW1lbnQ=");
    assert!(update.url.as_str().ends_with("Vsesvit_0.2.0_amd64.deb"));
    let date = update.release.pub_date.expect("pub_date parses");
    assert_eq!((date.offset().whole_hours(), date.nanosecond()), (2, 789_012_000));
}

#[test]
fn no_content_means_no_update() {
    let server = Server::start();
    server.route("/u", 204, "");
    server.route("/second", 200, dynamic("9.0.0"));
    assert!(check(&updater(vec![server.url("/u"), server.url("/second")], "1.0.0", Installation::Deb)).is_none());
    assert_eq!(server.requests().len(), 1, "a 204 ends the check");
}

#[test]
fn not_found_means_no_update() {
    let server = Server::start();
    server.route("/second", 200, dynamic("9.0.0"));
    assert!(check(&updater(vec![server.url("/missing"), server.url("/second")], "1.0.0", Installation::Deb)).is_none());
    assert_eq!(server.requests().len(), 1, "a 404 ends the check");
}

#[test]
fn a_failing_endpoint_falls_through_to_the_next() {
    let server = Server::start();
    server.route("/broken", 500, "oops");
    server.route("/garbage", 200, "<html>");
    server.route("/good", 200, dynamic("1.1.0"));
    let endpoints = vec![dead_url(), server.url("/broken"), server.url("/garbage"), server.url("/good")];
    let update = check(&updater(endpoints, "1.0.0", Installation::Deb)).expect("the last endpoint answers");
    assert_eq!(update.release.version, Version::new(1, 1, 0));
    let targets: Vec<String> = server.requests().into_iter().map(|r| r.target).collect();
    assert_eq!(targets, ["/broken", "/garbage", "/good"]);
}

#[test]
fn when_every_endpoint_fails_the_last_error_is_returned() {
    let server = Server::start();
    server.route("/broken", 503, "down");
    let err = updater(vec![dead_url(), server.url("/broken")], "1.0.0", Installation::Deb).check("stable").unwrap_err();
    assert!(matches!(err, Error::Http(503)), "{err:?}");

    let err = updater(vec![server.url("/broken"), dead_url()], "1.0.0", Installation::Deb).check("stable").unwrap_err();
    assert!(matches!(err, Error::Network(_)), "{err:?}");

    server.route("/garbage", 200, r#"{"version": "not semver", "url": "https://dl.test/a", "signature": "s"}"#);
    let err = updater(vec![server.url("/garbage")], "1.0.0", Installation::Deb).check("stable").unwrap_err();
    assert!(matches!(err, Error::BadResponse(_)), "{err:?}");
}

#[test]
fn only_a_strictly_newer_version_is_offered() {
    let server = Server::start();
    server.route("/u", 200, dynamic("1.2.0"));
    let offered = |current: &str| check(&updater(vec![server.url("/u")], current, Installation::Deb)).is_some();
    assert!(offered("1.1.9"), "older running version");
    assert!(offered("1.2.0-beta.1"), "a prerelease of the same version is older");
    assert!(!offered("1.2.0"), "same version");
    assert!(!offered("1.3.0"), "newer running version");
}

#[test]
fn installations_that_do_not_update_themselves_see_the_release_without_an_artifact() {
    let server = Server::start();
    server.route("/u", 200, static_manifest(&[&format!("{TARGET}-{ARCH}-deb"), &format!("{TARGET}-{ARCH}-appimage")]));
    for installation in [Installation::Unpackaged, Installation::Flatpak] {
        let available = updater(vec![server.url("/u")], "1.0.0", installation.clone()).check("stable").unwrap();
        let Some(Available::NotInstallable(release)) = available else {
            panic!("{installation:?}: expected a release it cannot install, got {available:?}")
        };
        assert_eq!(release.version, Version::new(2, 0, 0));
        let err = Available::NotInstallable(release).into_update().unwrap_err();
        assert!(matches!(err, Error::Disabled(DisabledReason::NotSelfUpdating)), "{err:?}");
    }
    assert_eq!(server.requests().len(), 2, "one check each, and nothing downloaded");
}

#[test]
fn a_config_without_key_or_endpoints_disables_the_updater() {
    let installation = Installation::Nsis { install_dir: PathBuf::from("C:/Vsesvit") };
    let new = |pubkey: &str, endpoints: Vec<Url>| {
        Updater::new(config(pubkey.to_owned(), endpoints), Version::new(1, 0, 0), installation.clone()).unwrap_err()
    };
    let endpoint = || vec![Url::parse("https://u.test/x").unwrap()];
    assert!(matches!(new("", endpoint()), Error::Disabled(DisabledReason::NoPublicKey)));
    assert!(matches!(new(PUBKEY, endpoint()), Error::Disabled(DisabledReason::InvalidPublicKey(_))));
    assert!(matches!(new(&support::Signer::new().pubkey(), vec![]), Error::Disabled(DisabledReason::NoEndpoints)));
}

#[test]
fn plain_http_is_refused_when_https_only() {
    let server = Server::start();
    server.route("/u", 200, dynamic("9.0.0"));
    let mut config = config(support::Signer::new().pubkey(), vec![server.url("/u")]);
    config.https_only = true;
    let err = Updater::new(config, Version::new(1, 0, 0), Installation::Deb).unwrap().check("stable").unwrap_err();
    assert!(matches!(err, Error::Network(_)), "{err:?}");
    assert!(server.requests().is_empty(), "nothing was sent over plain http");
}

#[test]
fn values_that_do_io_are_send() {
    fn send<T: Send>() {}
    send::<Updater>();
    send::<Available>();
    send::<Update>();
    send::<vsesvit_update::Downloaded>();
    send::<vsesvit_update::InstallFailed>();
    send::<Error>();
}
