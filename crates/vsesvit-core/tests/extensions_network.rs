//! Real store installs. Ignored by default because they need the network:
//!
//! ```text
//! cargo test -p vsesvit-core --test extensions_network -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use sha2::{Digest, Sha256};
use vsesvit_core::extensions::crx::{self, CWS_PUBLISHER_KEY_SHA256};
use vsesvit_core::extensions::{DEFAULT_CHROME_VERSION, ExtensionId, InstallPhase, InstallSource, Verification};
use vsesvit_core::{OpenOptions, Profile};

const UBO_LITE: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("vsesvit-network-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Prints the proofs the live CRX carries, then installs it through the real pipeline.
#[test]
#[ignore = "downloads uBlock Origin Lite from the Chrome Web Store"]
fn installs_ublock_origin_lite_from_the_chrome_web_store() {
    let id = ExtensionId::parse(UBO_LITE).unwrap();
    let url = InstallSource::cws_download_url(&id, DEFAULT_CHROME_VERSION);
    let bytes = ureq::get(url.as_str()).call().unwrap().into_body().with_config().limit(256 << 20).read_to_vec().unwrap();
    let parsed = crx::parse(&bytes).unwrap();
    println!("CRX: {} bytes, header {} bytes", bytes.len(), bytes.len() - parsed.zip.len() - 12);
    for (algorithm, proof) in parsed.proofs() {
        let key_hash: [u8; 32] = Sha256::digest(&proof.public_key).into();
        let role = if key_hash[..16] == parsed.crx_id {
            "developer (derives to crx_id)"
        } else if key_hash == CWS_PUBLISHER_KEY_SHA256 {
            "Chrome Web Store publisher"
        } else {
            "other"
        };
        println!(
            "proof {algorithm:?}: key sha256 {} ({role}), {} byte key, {} byte signature",
            hex(&key_hash),
            proof.public_key.len(),
            proof.signature.len()
        );
    }

    let t = TempDir::new();
    let mut p = Profile::open(&t.0.join("profile"), OpenOptions::default()).unwrap();
    let job = p.extensions().prepare_install(InstallSource::parse(UBO_LITE).unwrap()).unwrap();
    let mut last_download = None;
    let mut phases = Vec::new();
    let staged = job
        .run(&mut |phase| match phase {
            InstallPhase::Downloading { .. } => last_download = Some(phase),
            other => phases.push(other),
        })
        .unwrap();
    println!("last download progress: {last_download:?}; then {phases:?}");
    let ext = p.extensions().commit(staged).unwrap().unwrap();
    println!("installed {} {} into {}; verification = {:?}", ext.manifest.name, ext.version, ext.dir.display(), ext.verification);

    assert_eq!(ext.id, id);
    assert_eq!(ext.verification, Verification::ChromeWebStore { publisher_verified: true });
    assert_eq!(ext.manifest.key_id(), Some(id), "the injected key keeps the store id");
    assert!(!ext.dir.join("_metadata").exists());
    assert!(!ext.manifest.dnr_rulesets.is_empty(), "uBO Lite filters with declarativeNetRequest");
}

#[test]
#[ignore = "downloads uBlock Origin from addons.mozilla.org"]
fn installs_ublock_origin_from_amo() {
    let t = TempDir::new();
    let mut p = Profile::open(&t.0.join("profile"), OpenOptions::default()).unwrap();
    let source = InstallSource::parse("https://addons.mozilla.org/en-US/firefox/addon/ublock-origin/").unwrap();
    let job = p.extensions().prepare_install(source).unwrap();
    let staged = job.run(&mut |_| {}).unwrap();
    let ext = p.extensions().commit(staged).unwrap().unwrap();
    println!("installed {} {} as {}; verification = {:?}", ext.manifest.name, ext.version, ext.id.as_str(), ext.verification);
    assert_eq!(ext.id.as_str(), "uBlock0@raymondhill.net");
    assert_eq!(ext.verification, Verification::AmoHash);
}
