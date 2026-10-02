//! Real store installs. Ignored by default because they need the network:
//!
//! ```text
//! cargo test -p vsesvit-core --test extensions_network -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use sha2::{Digest, Sha256};
use vsesvit_core::extensions::crx::{self, CWS_PUBLISHER_KEY_SHA256, CrxStore, EDGE_PUBLISHER_KEY_SHA256};
use vsesvit_core::extensions::{DEFAULT_CHROME_VERSION, ExtensionId, InstallPhase, InstallSource, Verification};
use vsesvit_core::onboarding::{RECOMMENDED_EXTENSIONS, Recommended};
use vsesvit_core::{OpenOptions, Profile};

const UBO_LITE: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";
const PROTON_PASS: &str = "gcllgfdnfnllodcaambdaknbipemelie";

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

/// Prints the proofs the live CRX at `url` carries.
fn print_proofs(url: &str) {
    let bytes = ureq::get(url).call().unwrap().into_body().with_config().limit(256 << 20).read_to_vec().unwrap();
    let parsed = crx::parse(&bytes).unwrap();
    println!("CRX: {} bytes, header {} bytes", bytes.len(), bytes.len() - parsed.zip.len() - 12);
    for (algorithm, proof) in parsed.proofs() {
        let key_hash: [u8; 32] = Sha256::digest(&proof.public_key).into();
        let role = if key_hash[..16] == parsed.crx_id {
            "developer (derives to crx_id)"
        } else if key_hash == CWS_PUBLISHER_KEY_SHA256 {
            "Chrome Web Store publisher"
        } else if key_hash == EDGE_PUBLISHER_KEY_SHA256 {
            "Edge Add-ons publisher"
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
}

/// Prints the proofs the live CRX carries, then installs it through the real pipeline.
#[test]
#[ignore = "downloads uBlock Origin Lite from the Chrome Web Store"]
fn installs_ublock_origin_lite_from_the_chrome_web_store() {
    let id = ExtensionId::parse(UBO_LITE).unwrap();
    print_proofs(InstallSource::cws_download_url(&id, DEFAULT_CHROME_VERSION).as_str());

    let t = TempDir::new();
    let mut p = Profile::open(&t.0.join("profile"), OpenOptions::default()).unwrap();
    let job = p.extensions().prepare_install(InstallSource::parse(UBO_LITE).unwrap()).unwrap();
    let mut last_download = None;
    let mut phases = Vec::new();
    assert_eq!(recommended("uBlock Origin Lite").install_source(), InstallSource::parse(UBO_LITE).unwrap());
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
    assert_eq!(ext.verification, Verification::ChromeWebStore);
    assert_eq!(ext.manifest.key_id(), Some(id), "the injected key keeps the store id");
    assert!(!ext.dir.join("_metadata").exists());
    assert!(!ext.manifest.dnr_rulesets.is_empty(), "uBO Lite filters with declarativeNetRequest");
}

/// Prints the proofs the live CRX carries, then installs it through the real pipeline.
#[test]
#[ignore = "downloads Proton Pass from Microsoft Edge Add-ons"]
fn installs_proton_pass_from_edge_add_ons() {
    let id = ExtensionId::parse(PROTON_PASS).unwrap();
    // The redirect form ends at a plain-http CDN URL; the pipeline asks the update service
    // for an https one instead.
    print_proofs(&format!(
        "https://edge.microsoft.com/extensionwebstorebase/v1/crx?response=redirect&x=id%3D{PROTON_PASS}%26installsource%3Dondemand%26uc"
    ));

    let t = TempDir::new();
    let mut p = Profile::open(&t.0.join("profile"), OpenOptions::default()).unwrap();
    let source =
        InstallSource::parse("https://microsoftedge.microsoft.com/addons/detail/proton-pass-free-passwor/gcllgfdnfnllodcaambdaknbipemelie")
            .unwrap();
    assert_eq!(source, InstallSource::EdgeAddons { id: id.clone() });
    let staged = p.extensions().prepare_install(source).unwrap().run(&mut |_| {}).unwrap();
    let ext = p.extensions().commit(staged).unwrap().unwrap();
    println!("installed {} {} into {}; verification = {:?}", ext.manifest.name, ext.version, ext.dir.display(), ext.verification);

    assert_eq!(ext.id, id);
    assert_eq!(ext.verification, Verification::EdgeAddons);
    assert_eq!(ext.manifest.key_id(), Some(id), "the injected key keeps the store id");
    assert!(!ext.dir.join("_metadata").exists());
    assert!(ext.enabled);
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

fn recommended(name: &str) -> &'static Recommended {
    RECOMMENDED_EXTENSIONS.iter().find(|r| r.name == name).unwrap()
}

/// Installs a welcome-flow recommendation from the table's own source.
fn install_recommended(name: &str) {
    let r = recommended(name);
    let t = TempDir::new();
    let mut p = Profile::open(&t.0.join("profile"), OpenOptions::default()).unwrap();
    let staged = p.extensions().prepare_install(r.install_source()).unwrap().run(&mut |_| {}).unwrap();
    let ext = p.extensions().commit(staged).unwrap().unwrap();
    println!("installed {} {} into {}; verification = {:?}", ext.manifest.name, ext.version, ext.dir.display(), ext.verification);

    assert_eq!(ext.id, r.id());
    let expected = match r.store {
        CrxStore::ChromeWebStore => Verification::ChromeWebStore,
        CrxStore::EdgeAddons => Verification::EdgeAddons,
    };
    assert_eq!(ext.verification, expected);
    assert_eq!(ext.manifest.key_id(), Some(r.id()), "the injected key keeps the store id");
    assert!(ext.enabled);
}

#[test]
#[ignore = "downloads Bitwarden from its recommended store"]
fn installs_recommended_bitwarden() {
    install_recommended("Bitwarden");
}

#[test]
#[ignore = "downloads Proton Pass from its recommended store"]
fn installs_recommended_proton_pass() {
    install_recommended("Proton Pass");
}
