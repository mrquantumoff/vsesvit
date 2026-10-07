//! A stand-in for the Chrome Web Store, Edge Add-ons and addons.mozilla.org on a
//! [`FixtureServer`], so installs and updates run end to end without the network. Every
//! update check is answered with the newest published version and status `ok`: deciding
//! whether that is newer is core's job. Its CRX files are countersigned by a fixture
//! publisher key, which [`FixtureStore::stores`] pins in place of the real stores' keys.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use sha2::{Digest, Sha256};

use super::crx_writer::{CrxKey, sign_crx3, zip_files};
use super::fixture_server::{FixtureRequest, FixtureResponse, FixtureServer};
use crate::extensions::{ExtensionId, Stores};

/// The fixture publisher key's seed, used nowhere else.
const PUBLISHER_SEED: u8 = 91;

pub struct FixtureStore {
    stores: Stores,
    published: Arc<Mutex<Published>>,
}

#[derive(Default)]
struct Published {
    /// By extension id; served to the Chrome Web Store's and Edge Add-ons' clients alike.
    crx: HashMap<ExtensionId, Package>,
    /// By the path of the add-on's API URL.
    xpi: HashMap<String, (ExtensionId, Package)>,
}

struct Package {
    version: String,
    bytes: Vec<u8>,
    sha256: String,
}

impl Package {
    fn new(files: &[(&str, &[u8])], bytes: Vec<u8>) -> Package {
        let (_, manifest) = files.iter().find(|(name, _)| *name == "manifest.json").expect("a manifest.json");
        let manifest: serde_json::Value = serde_json::from_slice(manifest).expect("a JSON manifest");
        let version = manifest["version"].as_str().expect("a manifest version").to_owned();
        let sha256 = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        Package { version, bytes, sha256 }
    }
}

impl FixtureStore {
    /// Serves the stores under `/store/` on `server`.
    pub fn start(server: &FixtureServer) -> FixtureStore {
        let published = Arc::new(Mutex::new(Published::default()));
        let origin = server.origin();
        let crx_url = move |id: &ExtensionId| format!("{origin}/store/crx/{}.crx", id.as_str());

        server.route("/store/cws/update", {
            let published = Arc::clone(&published);
            let crx_url = crx_url.clone();
            move |request| cws_update(&lock(&published), request, &crx_url)
        });
        server.route("/store/edge/update", {
            let published = Arc::clone(&published);
            move |request| edge_update(&lock(&published), request, &crx_url)
        });
        server.route("/store/crx/", {
            let published = Arc::clone(&published);
            move |request| {
                let id = request.path().strip_prefix("/store/crx/").and_then(|name| name.strip_suffix(".crx"));
                let package = id.and_then(|id| ExtensionId::parse(id).ok()).and_then(|id| lock(&published).crx.get(&id).map(|p| p.bytes.clone()));
                package.map_or_else(|| FixtureResponse::status(404), |bytes| FixtureResponse::ok("application/x-chrome-extension", bytes))
            }
        });
        server.route("/store/amo/api/", {
            let published = Arc::clone(&published);
            let origin = server.origin();
            move |request| match lock(&published).xpi.get(request.path()) {
                Some((guid, package)) => {
                    let addon = serde_json::json!({
                        "guid": guid.as_str(),
                        "current_version": {
                            "version": package.version,
                            "file": { "url": format!("{origin}/store/amo/file/{}.xpi", package.sha256), "hash": format!("sha256:{}", package.sha256) },
                        },
                    });
                    FixtureResponse::ok("application/json", addon.to_string())
                }
                None => FixtureResponse::status(404),
            }
        });
        server.route("/store/amo/file/", {
            let published = Arc::clone(&published);
            move |request| {
                let sha256 = request.path().strip_prefix("/store/amo/file/").and_then(|name| name.strip_suffix(".xpi")).unwrap_or_default();
                let published = lock(&published);
                match published.xpi.values().find(|(_, package)| package.sha256 == sha256) {
                    Some((_, package)) => FixtureResponse::ok("application/x-xpinstall", package.bytes.clone()),
                    None => FixtureResponse::status(404),
                }
            }
        });

        let publisher = CrxKey::ecdsa(PUBLISHER_SEED).key_sha256();
        let stores = Stores {
            cws_update_url: server.url("/store/cws/update"),
            edge_update_url: server.url("/store/edge/update"),
            amo_api_url: server.url("/store/amo/api/"),
            cws_publisher_key_sha256: publisher,
            edge_publisher_key_sha256: publisher,
            https_only: false,
        };
        FixtureStore { stores, published }
    }

    /// For `Profile::set_stores`: these stand-ins, over plain http, and the fixture
    /// publisher key for both CRX stores.
    pub fn stores(&self) -> Stores {
        self.stores.clone()
    }

    /// Publishes `files` as a CRX3 signed by `developer` and countersigned by the fixture
    /// publisher, on the Chrome Web Store and Edge Add-ons stand-ins. Replaces what was
    /// published for its id.
    pub fn publish_crx<N: AsRef<str>, B: AsRef<[u8]>>(&self, files: &[(N, B)], developer: &CrxKey) -> ExtensionId {
        let files: Vec<(&str, &[u8])> = files.iter().map(|(name, bytes)| (name.as_ref(), bytes.as_ref())).collect();
        let bytes = sign_crx3(&zip_files(&files), developer.crx_id(), &[developer, &CrxKey::ecdsa(PUBLISHER_SEED)]);
        let id = developer.extension_id();
        lock(&self.published).crx.insert(id.clone(), Package::new(&files, bytes));
        id
    }

    /// Publishes `files` as an XPI under the gecko id `guid` on the addons.mozilla.org
    /// stand-in. Replaces what was published for it.
    pub fn publish_xpi<N: AsRef<str>, B: AsRef<[u8]>>(&self, guid: &str, files: &[(N, B)]) -> ExtensionId {
        let files: Vec<(&str, &[u8])> = files.iter().map(|(name, bytes)| (name.as_ref(), bytes.as_ref())).collect();
        let id = ExtensionId::parse(guid).expect("a gecko id");
        let package = Package::new(&files, zip_files(&files));
        lock(&self.published).xpi.insert(self.stores.amo_api_url(guid).path().to_owned(), (id.clone(), package));
        id
    }
}

fn lock(published: &Mutex<Published>) -> std::sync::MutexGuard<'_, Published> {
    published.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The ids in the request's `x` parameters (`id=<id>&v=<version>&uc`).
fn requested_ids(request: &FixtureRequest) -> Vec<ExtensionId> {
    request
        .query()
        .into_iter()
        .filter(|(name, _)| name == "x")
        .filter_map(|(_, x)| url::form_urlencoded::parse(x.as_bytes()).find(|(k, _)| k == "id").and_then(|(_, id)| ExtensionId::parse(&id).ok()))
        .filter(ExtensionId::is_chrome_style)
        .collect()
}

/// `response=redirect`: the CRX itself, or 204 for an id with nothing published.
/// `response=xml`: the gupdate answer for every requested id.
fn cws_update(published: &Published, request: &FixtureRequest, crx_url: &dyn Fn(&ExtensionId) -> String) -> FixtureResponse {
    let ids = requested_ids(request);
    if request.query().iter().any(|(name, value)| name == "response" && value == "redirect") {
        return match ids.first().and_then(|id| published.crx.get(id)) {
            Some(package) => FixtureResponse::ok("application/x-chrome-extension", package.bytes.clone()),
            None => FixtureResponse::status(204),
        };
    }
    let apps: String = ids
        .iter()
        .map(|id| match published.crx.get(id) {
            Some(package) => format!(
                r#"<app appid="{id}" status="ok"><updatecheck codebase="{url}" hash_sha256="{sha256}" status="ok" version="{version}"/></app>"#,
                id = id.as_str(),
                url = crx_url(id),
                sha256 = package.sha256,
                version = package.version,
            ),
            None => format!(r#"<app appid="{}" status="error-unknownApplication"/>"#, id.as_str()),
        })
        .collect();
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><gupdate xmlns="http://www.google.com/update2/response" protocol="2.0" server="prod">{apps}</gupdate>"#
    );
    FixtureResponse::ok("text/xml; charset=utf-8", xml)
}

/// The Omaha 3.1 answer for every app of the POSTed request, behind Edge's `)]}'` line.
fn edge_update(published: &Published, request: &FixtureRequest, crx_url: &dyn Fn(&ExtensionId) -> String) -> FixtureResponse {
    let Ok(body) = serde_json::from_slice::<serde_json::Value>(&request.body) else { return FixtureResponse::status(400) };
    let apps: Vec<serde_json::Value> = body["request"]["app"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|app| ExtensionId::parse(app["appid"].as_str()?).ok())
        .map(|id| match published.crx.get(&id) {
            Some(package) => serde_json::json!({
                "appid": id.as_str(),
                "status": "ok",
                "updatecheck": {
                    "status": "ok",
                    "urls": { "url": [{ "codebase": crx_url(&id) }] },
                    "manifest": { "version": package.version, "packages": { "package": [{ "hash_sha256": package.sha256 }] } },
                },
            }),
            None => serde_json::json!({ "appid": id.as_str(), "status": "error-unknownApplication" }),
        })
        .collect();
    let answer = serde_json::json!({ "response": { "protocol": "3.1", "app": apps } });
    FixtureResponse::ok("application/json", format!(")]}}'\n{answer}"))
}
