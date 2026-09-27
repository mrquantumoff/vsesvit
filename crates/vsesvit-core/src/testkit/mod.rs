//! Test helpers shared by core's tests and the shells' `--self-test`: a fixture HTTP
//! server and a CRX3 writer. Behind the `testkit` feature; never in a release build.
//!
//! Everything is embedded at compile time (`include_bytes!`), so the helpers work from
//! any working directory, including an installed self-test binary.

mod crx_writer;
mod fixture_server;

pub use crx_writer::{CrxKey, encode_crx3, sign_crx3, write_crx3, zip_files};
pub use fixture_server::FixtureServer;

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
    probe_file!("popup.html"),
    probe_file!("popup.js"),
    probe_file!("rules.json"),
];

/// A CRX3 of [`PROBE_FILES`] signed with [`CrxKey::probe`]. Deterministic: every call
/// returns the same bytes, so installing it twice is a same-bytes re-install.
pub fn probe_crx() -> Vec<u8> {
    write_crx3(PROBE_FILES, &CrxKey::probe())
}
