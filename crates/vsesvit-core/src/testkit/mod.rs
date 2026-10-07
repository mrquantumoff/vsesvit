//! Test helpers shared by core's tests and the shells' `--self-test`: a fixture HTTP
//! server, a stand-in for the extension stores, a CRX3 writer and the self-test's report.
//! Behind the `testkit` feature; never in a release build.
//!
//! Everything is embedded at compile time (`include_bytes!`), so the helpers work from
//! any working directory, including an installed self-test binary.

mod crx_writer;
mod fixture_server;
pub mod report;
mod store;

pub use crx_writer::{CrxKey, encode_crx3, sign_crx3, write_crx3, zip_files};
pub use fixture_server::{FixtureRequest, FixtureResponse, FixtureServer, STALLED_SENT};
pub use store::FixtureStore;

/// The id `probe_crx()` installs as: derived from `tests/fixtures/keys/test-only-probe-key.pem`.
pub const PROBE_ID: &str = "eonajgebgeenbhiiobbhmkafolkeghdb";

macro_rules! probe_file {
    ($name:literal) => {
        ($name, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/extensions/probe/", $name)).as_slice())
    };
}

/// The probe extension (`tests/fixtures/extensions/probe/`), without the `_metadata/`
/// that Chromium writes into the source dir when it loads the dir unpacked.
pub const PROBE_FILES: &[(&str, &[u8])] = &[
    probe_file!("manifest.json"),
    probe_file!("background.js"),
    probe_file!("content.js"),
    probe_file!("dynamic.js"),
    probe_file!("popup.html"),
    probe_file!("popup.js"),
    probe_file!("rules.json"),
];

/// A CRX3 of [`PROBE_FILES`] signed with [`CrxKey::probe`]. Deterministic: every call
/// returns the same bytes, so installing it twice is a same-bytes re-install.
pub fn probe_crx() -> Vec<u8> {
    write_crx3(PROBE_FILES, &CrxKey::probe())
}

/// The update probe (`tests/fixtures/extensions/update-probe/`) at `version`, asking for
/// `permissions` (host patterns among them become host permissions). Its content script
/// runs on every `http://127.0.0.1` page and writes `<running version>:<first version that
/// ran>` to `data-vsesvit-update-probe`: "2.0:1.0" after an update that kept the
/// extension's storage, "2.0:2.0" after one that lost it.
pub fn update_probe_files(version: &str, permissions: &[&str]) -> Vec<(String, Vec<u8>)> {
    let manifest = serde_json::json!({
        "manifest_version": 3,
        "name": "Vsesvit update probe",
        "version": version,
        "description": "Test extension that reports its version and the first version that ran.",
        "permissions": permissions,
        "background": { "service_worker": "background.js" },
        "content_scripts": [{ "matches": ["http://127.0.0.1/*"], "js": ["content.js"], "run_at": "document_end" }],
    });
    let file = |name: &str, bytes: &[u8]| (name.to_owned(), bytes.to_vec());
    vec![
        file("manifest.json", &serde_json::to_vec_pretty(&manifest).expect("a JSON value always serializes")),
        file("background.js", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/extensions/update-probe/background.js"))),
        file("content.js", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/extensions/update-probe/content.js"))),
    ]
}
