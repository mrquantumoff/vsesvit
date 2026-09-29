//! Install pipeline. `InstallJob::run` does everything that is slow or touches the
//! network. It is `Send`, holds no database handle, and writes only inside its own
//! staging dir. `Extensions::commit` (UI thread) is the only step that publishes
//! anything.
//!
//! ```text
//! run():  resolve ─▶ fetch (in memory, size-capped; AMO, Edge Add-ons: sha256 checked)
//!                 ─▶ verify (CRX3: every proof + id binding + the store's publisher proof;  AMO XPI: sha256 from the API)
//!                 ─▶ unpack (zip-slip / symlink / name / size checks, drop _metadata/)
//!                 ─▶ set manifest "key" (CRX: the verified key, so the unpacked dir keeps the store id;
//!                                        XPI: removed, since nothing verified it)
//!                 ─▶ Manifest::load (validate + localize)
//!                 ─▶ StagedInstall
//! ```

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::crx::{self, CrxError, CrxStore, VerifyPolicy};
use super::manifest::{self, Manifest, ManifestError};
use super::{ExtensionId, StoreRef, Verification, is_windows_reserved_name};
use crate::Url;

/// Largest package (`.crx` / `.xpi`) the pipeline reads, from the network or from disk.
pub const MAX_ARCHIVE_BYTES: u64 = 256 << 20;
/// Largest total size an archive may expand to.
pub const MAX_UNPACKED_BYTES: u64 = 256 << 20;
pub const MAX_ENTRIES: usize = 20_000;
/// Expansion ratio cap (uncompressed / archive size). Real extensions compress about
/// 3-10x; a zip bomb compresses thousands of times. Small archives get
/// `RATIO_ALLOWANCE` bytes of slack so a tiny package of empty-ish files is not refused.
const MAX_RATIO: u64 = 100;
const RATIO_ALLOWANCE: u64 = 1 << 20;
const MAX_API_RESPONSE_BYTES: u64 = 1 << 20;
/// Edge Add-ons' update service. Its `?response=redirect` form sends downloads to a
/// plain-http CDN URL, which the https-only agent refuses, so installs go through an
/// update check, which names an https one.
const EDGE_UPDATE_URL: &str = "https://edge.microsoft.com/extensionwebstorebase/v1/crx";

/// What the user gave us, parsed once at the boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallSource {
    ChromeWebStore {
        id: ExtensionId,
    },
    /// Microsoft Edge Add-ons. Its ids are Chrome-style, so a bare id stays Chrome Web Store.
    EdgeAddons {
        id: ExtensionId,
    },
    /// AMO slug or gecko id. After install the record is keyed by the gecko id.
    Amo {
        slug_or_guid: String,
    },
    CrxFile {
        path: PathBuf,
    },
    XpiFile {
        path: PathBuf,
    },
    /// Loaded in place and never copied: developer edits apply on "reload".
    Unpacked {
        dir: PathBuf,
    },
}

impl InstallSource {
    /// Accepts:
    /// - `https://chromewebstore.google.com/detail/<slug>/<id>` and `.../detail/<id>`
    /// - `https://chrome.google.com/webstore/detail/<slug>/<id>` (legacy)
    /// - `https://microsoftedge.microsoft.com/addons/detail/<slug>/<id>` and `.../detail/<id>`
    /// - a bare 32-char `a..p` id (Chrome Web Store)
    /// - `https://addons.mozilla.org/<locale>/firefox/addon/<slug>/` (the locale and
    ///   app segments are optional), or a bare gecko id (`name@domain`, `{uuid}`)
    /// - `file://` URLs or paths ending in `.crx` / `.xpi`, or naming a directory with a
    ///   `manifest.json` (or the `manifest.json` itself)
    ///
    /// Query strings and fragments are ignored.
    pub fn parse(input: &str) -> Result<InstallSource, SourceParseError> {
        let input = input.trim();
        if let Ok(id) = ExtensionId::parse(input)
            && id.is_chrome_style()
        {
            return Ok(InstallSource::ChromeWebStore { id });
        }
        if let Ok(url) = Url::parse(input) {
            match url.scheme() {
                "http" | "https" | "file" => return Self::from_url(&url),
                // `C:\x.crx` parses as a URL whose scheme is the drive letter.
                drive if drive.len() == 1 => {}
                _ => return Err(SourceParseError::Unrecognized),
            }
        }
        match Self::from_path(Path::new(input)) {
            Err(SourceParseError::Unrecognized) if is_gecko_id(input) => Ok(InstallSource::Amo { slug_or_guid: input.to_owned() }),
            other => other,
        }
    }

    pub fn web_store(store: CrxStore, id: ExtensionId) -> InstallSource {
        match store {
            CrxStore::ChromeWebStore => InstallSource::ChromeWebStore { id },
            CrxStore::EdgeAddons => InstallSource::EdgeAddons { id },
        }
    }

    /// From a file picker or drag-and-drop.
    pub fn from_path(path: &Path) -> Result<InstallSource, SourceParseError> {
        if path.as_os_str().is_empty() {
            return Err(SourceParseError::Unrecognized);
        }
        let abs = absolute_normalized(path).ok_or(SourceParseError::Unrecognized)?;
        let extension = abs.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("crx") => Ok(InstallSource::CrxFile { path: abs }),
            Some("xpi") => Ok(InstallSource::XpiFile { path: abs }),
            _ if abs.file_name().is_some_and(|n| n == "manifest.json") && abs.is_file() => {
                let dir = abs.parent().ok_or(SourceParseError::Unrecognized)?;
                Ok(InstallSource::Unpacked { dir: dir.to_path_buf() })
            }
            _ if abs.join("manifest.json").is_file() => Ok(InstallSource::Unpacked { dir: abs }),
            _ => Err(SourceParseError::Unrecognized),
        }
    }

    fn from_url(url: &Url) -> Result<InstallSource, SourceParseError> {
        if url.scheme() == "file" {
            let path = url.to_file_path().map_err(|_| SourceParseError::Unrecognized)?;
            return Self::from_path(&path);
        }
        let host = url.host_str().unwrap_or_default().trim_end_matches('.').to_ascii_lowercase();
        let segments: Vec<&str> = url.path_segments().into_iter().flatten().filter(|s| !s.is_empty()).collect();
        match host.as_str() {
            "chromewebstore.google.com" => crx_store_id_after(&segments, &["detail"], CrxStore::ChromeWebStore),
            "chrome.google.com" => crx_store_id_after(&segments, &["webstore", "detail"], CrxStore::ChromeWebStore),
            "microsoftedge.microsoft.com" => crx_store_id_after(&segments, &["addons", "detail"], CrxStore::EdgeAddons),
            "addons.mozilla.org" => {
                let at = segments.iter().position(|s| *s == "addon").ok_or(SourceParseError::Unrecognized)?;
                let slug =
                    segments.get(at + 1).and_then(|s| percent_decode(s)).filter(|s| is_amo_slug(s)).ok_or(SourceParseError::BadId)?;
                Ok(InstallSource::Amo { slug_or_guid: slug })
            }
            _ => Err(SourceParseError::Unrecognized),
        }
    }

    /// Only store sources produce a synced `ExtensionRecord`.
    pub fn store(&self) -> Option<StoreRef> {
        match self {
            InstallSource::ChromeWebStore { .. } => Some(StoreRef::ChromeWebStore),
            InstallSource::EdgeAddons { .. } => Some(StoreRef::EdgeAddons),
            InstallSource::Amo { .. } => Some(StoreRef::Amo),
            _ => None,
        }
    }

    /// The `extension_installs.source_kind` column value (same spelling as the serde tag).
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            InstallSource::ChromeWebStore { .. } => "chrome_web_store",
            InstallSource::EdgeAddons { .. } => "edge_addons",
            InstallSource::Amo { .. } => "amo",
            InstallSource::CrxFile { .. } => "crx_file",
            InstallSource::XpiFile { .. } => "xpi_file",
            InstallSource::Unpacked { .. } => "unpacked",
        }
    }

    /// `https://clients2.google.com/service/update2/crx?response=redirect&prodversion=<v>&acceptformat=crx2,crx3&x=id%3D<id>%26uc`
    pub fn cws_download_url(id: &ExtensionId, chrome_version: &str) -> Url {
        let version: String = url::form_urlencoded::byte_serialize(chrome_version.as_bytes()).collect();
        let url = format!(
            "https://clients2.google.com/service/update2/crx?response=redirect&prodversion={version}&acceptformat=crx2,crx3&x=id%3D{}%26uc",
            id.as_str()
        );
        Url::parse(&url).expect("the id charset and the encoded version keep the URL valid")
    }

    /// `https://addons.mozilla.org/api/v5/addons/addon/<slug_or_guid>/`. The response's
    /// `guid`, `current_version.file.url` and `current_version.file.hash`
    /// ("sha256:<hex>") drive the XPI fetch.
    pub fn amo_api_url(slug_or_guid: &str) -> Url {
        let mut url = Url::parse("https://addons.mozilla.org/api/v5/addons/addon/").expect("a valid constant URL");
        url.path_segments_mut().expect("an https URL has a path").pop_if_empty().push(slug_or_guid).push("");
        url
    }
}

/// Absolute, with `.` and `..` resolved lexically. `std::path::absolute` already does
/// this on Windows but keeps `..` on POSIX, and an unpacked dir's id is a hash of its
/// path, so every spelling of a dir has to reach the same path.
fn absolute_normalized(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in std::path::absolute(path).ok()?.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    Some(out)
}

/// `/detail/<slug>/<id>` or `/detail/<id>`, possibly followed by more segments.
fn crx_store_id_after(segments: &[&str], prefix: &[&str], store: CrxStore) -> Result<InstallSource, SourceParseError> {
    let rest = segments.strip_prefix(prefix).ok_or(SourceParseError::Unrecognized)?;
    let id =
        rest.iter().take(2).find_map(|s| ExtensionId::parse(s).ok().filter(ExtensionId::is_chrome_style)).ok_or(SourceParseError::BadId)?;
    Ok(InstallSource::web_store(store, id))
}

fn is_amo_slug(s: &str) -> bool {
    (1..=200).contains(&s.len()) && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._@{}-".contains(&b))
}

/// `name@domain` or `{uuid}`: unambiguous gecko ids, never a Chrome id or a path.
fn is_gecko_id(s: &str) -> bool {
    ExtensionId::parse(s).is_ok() && (s.contains('@') || (s.starts_with('{') && s.ends_with('}')))
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[derive(Debug, thiserror::Error)]
pub enum SourceParseError {
    #[error("not a Chrome Web Store, Edge Add-ons or addons.mozilla.org extension URL, an extension id, or an extension file")]
    Unrecognized,
    #[error("the store URL does not contain a valid extension id")]
    BadId,
}

/// Who wants this install. See `Extensions::commit` for why it matters.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// The user asked for it: commit updates the synced desired state.
    User,
    /// `reconcile()` is realizing desired state from another device: commit never
    /// changes desired state, and discards the result if it is no longer wanted.
    Reconcile,
}

/// Where a running job is, for the install UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallPhase {
    /// AMO and Edge Add-ons: asking the store which file to download.
    Resolving,
    /// Reported after every received chunk. `total` is the server's `Content-Length`.
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    Verifying,
    Unpacking,
    /// Validating and localizing `manifest.json`.
    ReadingManifest,
}

/// `Send`, no DB handle, no `Profile` borrow. Built by `Extensions::prepare_install` or
/// `reconcile()`, run on any thread.
#[derive(Debug)]
pub struct InstallJob {
    pub(crate) source: InstallSource,
    pub(crate) intent: Intent,
    pub(crate) staging: StagingDir,
    pub(crate) chrome_version: String,
    pub(crate) ui_locale: String,
    /// The id the job must produce: the requested Chrome Web Store or Edge Add-ons id, or
    /// the synced record's id for a reconcile job.
    pub(crate) expected_id: Option<ExtensionId>,
}

impl InstallJob {
    pub fn source(&self) -> &InstallSource {
        &self.source
    }

    /// Blocking. Call off the UI thread (`gio::spawn_blocking`, `std::thread::spawn`).
    /// `progress` is called on the calling thread.
    ///
    /// - CWS: GET the update2 URL (redirects followed; 204/empty = `NotAvailable`),
    ///   `crx::parse` + `crx::verify(WebStore { store, expected: id })`.
    /// - Edge Add-ons: POST the update check, GET its codebase, check its sha256, then
    ///   verify as CWS does, with the Edge publisher key.
    /// - AMO: GET the API JSON, GET `file.url`, check `file.hash`. Id = manifest gecko id,
    ///   which must equal the API `guid` when both exist, else the API `guid`.
    /// - CrxFile: `crx::verify(AnyDeveloperKey)`. The id comes from the developer key.
    /// - XpiFile: no signature check (Mozilla's PKCS#7/COSE is out of scope). Id = the
    ///   manifest gecko id, else derived from the file's path.
    /// - Unpacked: `Manifest::load(dir)` only. Nothing is copied.
    ///
    /// An XPI's gecko id (or AMO `guid`) may not be Chrome-style. Then `unpack_zip` into
    /// `staging/root`, set `key` (CRX: the verified one; XPI: none), load the manifest, and
    /// check the id against `expected_id`.
    pub fn run(self, progress: &mut dyn FnMut(InstallPhase)) -> Result<StagedInstall, InstallError> {
        let InstallJob { source, intent, staging, chrome_version, ui_locale, expected_id } = self;
        let (id, manifest, files, verification) = match fetch(&source, &chrome_version, progress)? {
            Fetched::InPlace(dir) => {
                progress(InstallPhase::ReadingManifest);
                let manifest = Manifest::load(&dir, &ui_locale)?;
                (unpacked_id(&dir, &manifest), manifest, StagedFiles::InPlace { dir }, Verification::Unpacked)
            }
            Fetched::Package(package) => {
                fs::create_dir_all(&staging.0)?;
                let (id, manifest, verification) = stage(&package, &staging.root(), &ui_locale, progress)?;
                let dir_name = format!("{}_{}", manifest.version, hex(&package.sha256[..16]));
                (id, manifest, StagedFiles::Staged { staging, dir_name }, verification)
            }
        };
        if let Some(expected) = expected_id
            && expected != id
        {
            return Err(InstallError::IdMismatch { expected: expected.as_str().to_owned(), actual: id.as_str().to_owned() });
        }
        Ok(StagedInstall { version: manifest.version.clone(), id, source, intent, files, manifest, verification })
    }
}

enum Fetched {
    /// A downloaded or read package, not yet verified.
    Package(Package),
    /// An unpacked developer dir: nothing to fetch.
    InPlace(PathBuf),
}

struct Package {
    bytes: Vec<u8>,
    sha256: [u8; 32],
    format: Format,
}

enum Format {
    Crx(VerifyPolicy),
    Xpi(XpiId),
}

/// Where an XPI's id comes from when its manifest has no gecko id. It also says where
/// the XPI came from, which is all the verification an XPI gets.
enum XpiId {
    /// AMO's `guid`. It must also match the manifest's gecko id when that exists.
    Store(ExtensionId),
    /// A local file without a gecko id is keyed like an unpacked dir: by its path.
    FromPath(PathBuf),
}

impl Package {
    fn new(bytes: Vec<u8>, format: Format) -> Self {
        let sha256 = Sha256::digest(&bytes).into();
        Package { bytes, sha256, format }
    }
}

fn fetch(source: &InstallSource, chrome_version: &str, progress: &mut dyn FnMut(InstallPhase)) -> Result<Fetched, InstallError> {
    Ok(Fetched::Package(match source {
        InstallSource::Unpacked { dir } => return Ok(Fetched::InPlace(dir.clone())),
        InstallSource::ChromeWebStore { id } => {
            let bytes = download(&agent(), InstallSource::cws_download_url(id, chrome_version).as_str(), progress)?;
            Package::new(bytes, Format::Crx(VerifyPolicy::WebStore { store: CrxStore::ChromeWebStore, expected: id.clone() }))
        }
        InstallSource::EdgeAddons { id } => {
            progress(InstallPhase::Resolving);
            let agent = agent();
            let (url, expected_sha256) = edge_package(&agent, id, chrome_version)?;
            let bytes = download(&agent, &url, progress)?;
            let package = Package::new(bytes, Format::Crx(VerifyPolicy::WebStore { store: CrxStore::EdgeAddons, expected: id.clone() }));
            if package.sha256 != expected_sha256 {
                return Err(InstallError::HashMismatch);
            }
            package
        }
        InstallSource::Amo { slug_or_guid } => {
            progress(InstallPhase::Resolving);
            let agent = agent();
            let addon = amo_addon(&agent, slug_or_guid)?;
            let expected_sha256 = parse_sha256(&addon.current_version.file.hash)?;
            let guid = xpi_id(&addon.guid).ok_or_else(|| InstallError::BadStoreResponse(format!("guid {:?}", addon.guid)))?;
            let bytes = download(&agent, &addon.current_version.file.url, progress)?;
            let package = Package::new(bytes, Format::Xpi(XpiId::Store(guid)));
            if package.sha256 != expected_sha256 {
                return Err(InstallError::HashMismatch);
            }
            package
        }
        InstallSource::CrxFile { path } => Package::new(read_package(path)?, Format::Crx(VerifyPolicy::AnyDeveloperKey)),
        InstallSource::XpiFile { path } => Package::new(read_package(path)?, Format::Xpi(XpiId::FromPath(path.clone()))),
    }))
}

/// Verify, unpack into `root`, inject the key, load the manifest.
fn stage(
    package: &Package,
    root: &Path,
    ui_locale: &str,
    progress: &mut dyn FnMut(InstallPhase),
) -> Result<(ExtensionId, Manifest, Verification), InstallError> {
    match &package.format {
        Format::Crx(policy) => {
            progress(InstallPhase::Verifying);
            let crx = crx::parse(&package.bytes)?;
            let verified = crx::verify(&crx, policy)?;
            progress(InstallPhase::Unpacking);
            unpack_zip(crx.zip, root)?;
            set_key(root, Some(&verified.developer_key))?;
            progress(InstallPhase::ReadingManifest);
            let manifest = Manifest::load(root, ui_locale)?;
            let verification = match policy {
                VerifyPolicy::WebStore { store: CrxStore::ChromeWebStore, .. } => {
                    Verification::ChromeWebStore { publisher_verified: verified.publisher_verified }
                }
                VerifyPolicy::WebStore { store: CrxStore::EdgeAddons, .. } => Verification::EdgeAddons,
                VerifyPolicy::AnyDeveloperKey => Verification::LocalCrx,
            };
            Ok((verified.id, manifest, verification))
        }
        Format::Xpi(id) => {
            progress(InstallPhase::Unpacking);
            unpack_zip(&package.bytes, root)?;
            set_key(root, None)?;
            progress(InstallPhase::ReadingManifest);
            let manifest = Manifest::load(root, ui_locale)?;
            let gecko_id = manifest
                .gecko_id
                .as_deref()
                .map(|g| xpi_id(g).ok_or(ManifestError::Field("browser_specific_settings.gecko.id")))
                .transpose()?;
            let resolved = match (gecko_id, id) {
                (Some(g), XpiId::Store(guid)) if g != *guid => {
                    return Err(InstallError::IdMismatch { expected: guid.as_str().to_owned(), actual: g.as_str().to_owned() });
                }
                (Some(g), _) => g,
                (None, XpiId::Store(guid)) => guid.clone(),
                (None, XpiId::FromPath(path)) => ExtensionId::for_unpacked_dir(path),
            };
            let verification = match id {
                XpiId::Store(_) => Verification::AmoHash,
                XpiId::FromPath(_) => Verification::LocalXpi,
            };
            Ok((resolved, manifest, verification))
        }
    }
}

/// A gecko id from an XPI manifest or AMO. Chrome-style ids are refused: they are hashes of
/// a developer key, and nothing ties an XPI to one.
fn xpi_id(s: &str) -> Option<ExtensionId> {
    ExtensionId::parse(s).ok().filter(|id| !id.is_chrome_style())
}

/// An unpacked dir's id: from the manifest `key` if it has one, else from its path.
pub(crate) fn unpacked_id(dir: &Path, manifest: &Manifest) -> ExtensionId {
    manifest.key_id().unwrap_or_else(|| ExtensionId::for_unpacked_dir(dir))
}

/// The UI language for `__MSG_*__` in manifests, from `LC_ALL` / `LC_MESSAGES` / `LANG`
/// (`uk_UA.UTF-8` -> `uk_UA`). Where none is set (usual on Windows) it is `en`, and
/// lookups fall back to each extension's `default_locale`.
pub(crate) fn ui_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .and_then(|v| v.split(['.', '@']).next().map(str::to_owned))
        .filter(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .unwrap_or_else(|| "en".to_owned())
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_recv_body(Some(Duration::from_secs(600)))
        .build()
        .into()
}

fn network_error(e: ureq::Error) -> InstallError {
    match e {
        ureq::Error::StatusCode(code) => InstallError::Http(code),
        other => InstallError::Network(other.to_string()),
    }
}

/// Reads the whole response into memory, capped at [`MAX_ARCHIVE_BYTES`].
fn download(agent: &ureq::Agent, url: &str, progress: &mut dyn FnMut(InstallPhase)) -> Result<Vec<u8>, InstallError> {
    let response = agent.get(url).call().map_err(network_error)?;
    if response.status() == 204 {
        return Err(InstallError::NotAvailable);
    }
    let total = response.body().content_length();
    if total.is_some_and(|t| t > MAX_ARCHIVE_BYTES) {
        return Err(InstallError::TooLarge(MAX_ARCHIVE_BYTES));
    }
    progress(InstallPhase::Downloading { received: 0, total });
    let mut reader = response.into_body().into_reader();
    let mut bytes = Vec::with_capacity(usize::try_from(total.unwrap_or(0)).unwrap_or(0));
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(InstallError::Network(e.to_string())),
        };
        if (bytes.len() + n) as u64 > MAX_ARCHIVE_BYTES {
            return Err(InstallError::TooLarge(MAX_ARCHIVE_BYTES));
        }
        bytes.extend_from_slice(&chunk[..n]);
        progress(InstallPhase::Downloading { received: bytes.len() as u64, total });
    }
    if bytes.is_empty() {
        return Err(InstallError::NotAvailable);
    }
    Ok(bytes)
}

#[derive(Deserialize)]
struct AmoAddon {
    guid: String,
    current_version: AmoVersion,
}

#[derive(Deserialize)]
struct AmoVersion {
    file: AmoFile,
}

#[derive(Deserialize)]
struct AmoFile {
    url: String,
    hash: String,
}

fn amo_addon(agent: &ureq::Agent, slug_or_guid: &str) -> Result<AmoAddon, InstallError> {
    let mut response =
        agent.get(InstallSource::amo_api_url(slug_or_guid).as_str()).header("Accept", "application/json").call().map_err(network_error)?;
    let body = response.body_mut().with_config().limit(MAX_API_RESPONSE_BYTES).read_to_vec().map_err(network_error)?;
    serde_json::from_slice(&body).map_err(|e| InstallError::BadStoreResponse(e.to_string()))
}

/// The Omaha 3.1 update check Edge itself POSTs to [`EDGE_UPDATE_URL`] for a fresh install.
fn edge_update_request(id: &ExtensionId, chrome_version: &str) -> serde_json::Value {
    serde_json::json!({ "request": {
        "protocol": "3.1",
        "acceptformat": "crx3",
        "prodversion": chrome_version,
        "app": [{ "appid": id.as_str(), "installsource": "ondemand", "version": "0.0.0.0", "updatecheck": {} }],
    }})
}

/// The codebase URL and sha256 of the CRX Edge Add-ons serves for `id`.
fn edge_package(agent: &ureq::Agent, id: &ExtensionId, chrome_version: &str) -> Result<(String, [u8; 32]), InstallError> {
    let request = edge_update_request(id, chrome_version).to_string();
    let mut response = agent.post(EDGE_UPDATE_URL).header("Content-Type", "application/json").send(&request).map_err(network_error)?;
    let body = response.body_mut().with_config().limit(MAX_API_RESPONSE_BYTES).read_to_vec().map_err(network_error)?;
    parse_edge_update(&body)
}

/// An Omaha 3.1 JSON response, which starts with the `)]}'` line that guards against JSON
/// hijacking. An app status other than `ok` (`error-unknownApplication` for an id the
/// store does not have) or an update check other than `ok` means nothing to install.
fn parse_edge_update(body: &[u8]) -> Result<(String, [u8; 32]), InstallError> {
    let json = body.strip_prefix(b")]}'").unwrap_or(body);
    let value: serde_json::Value = serde_json::from_slice(json).map_err(|e| InstallError::BadStoreResponse(e.to_string()))?;
    let app = &value["response"]["app"][0];
    if app["status"] != "ok" || app["updatecheck"]["status"] != "ok" {
        return Err(InstallError::NotAvailable);
    }
    let field = |pointer: &str| {
        app.pointer(pointer).and_then(serde_json::Value::as_str).ok_or_else(|| InstallError::BadStoreResponse(format!("no {pointer}")))
    };
    let codebase = field("/updatecheck/urls/url/0/codebase")?;
    let hash = field("/updatecheck/manifest/packages/package/0/hash_sha256")?;
    Ok((codebase.to_owned(), parse_hex_sha256(hash)?))
}

/// `"sha256:<64 hex>"`.
fn parse_sha256(hash: &str) -> Result<[u8; 32], InstallError> {
    parse_hex_sha256(hash.strip_prefix("sha256:").ok_or_else(|| InstallError::BadStoreResponse(format!("file hash {hash:?}")))?)
}

/// 64 hex digits, either case.
fn parse_hex_sha256(hex: &str) -> Result<[u8; 32], InstallError> {
    let bad = || InstallError::BadStoreResponse(format!("file hash {hex:?}"));
    if hex.len() != 64 {
        return Err(bad());
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(2 * i..2 * i + 2).ok_or_else(bad)?, 16).map_err(|_| bad())?;
    }
    Ok(out)
}

fn read_package(path: &Path) -> Result<Vec<u8>, InstallError> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(MAX_ARCHIVE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(InstallError::TooLarge(MAX_ARCHIVE_BYTES));
    }
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Staging
// ---------------------------------------------------------------------------

/// Owns `<root>/staging/<uuid>`. `Drop` removes it (best effort), so a failed or
/// abandoned job leaves nothing behind. `Profile::open` also wipes `staging/` for the
/// crash case. `commit` renames `root` out of it before it drops.
#[derive(Debug)]
pub(crate) struct StagingDir(pub(crate) PathBuf);

impl StagingDir {
    pub(crate) fn root(&self) -> PathBuf {
        self.0.join("root")
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Proof that the files under `root` were fetched, verified and parsed. Fields are
/// private and only `InstallJob::run` constructs it, so `commit` cannot be handed
/// unverified content.
#[derive(Debug)]
pub struct StagedInstall {
    pub(crate) id: ExtensionId,
    pub(crate) version: String,
    pub(crate) source: InstallSource,
    pub(crate) intent: Intent,
    pub(crate) files: StagedFiles,
    pub(crate) manifest: Manifest,
    pub(crate) verification: Verification,
}

#[derive(Debug)]
pub(crate) enum StagedFiles {
    /// `staging.root()`, to be renamed to `extensions/<id>/<version>_<hash32>`.
    /// `hash32` = the first 32 hex chars (128 bits) of the archive's SHA-256, so identical
    /// bytes map to the same dir (idempotent) and different bytes with the same version
    /// never touch an existing dir (WebView2 drops extensions whose files change). A
    /// shorter prefix could be matched on purpose by grinding a zip comment.
    Staged {
        staging: StagingDir,
        dir_name: String,
    },
    InPlace {
        dir: PathBuf,
    },
}

impl StagedInstall {
    pub fn id(&self) -> &ExtensionId {
        &self.id
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn verification(&self) -> &Verification {
        &self.verification
    }
}

/// Extraction rules (the checks that keep a hostile archive inside `dest`):
/// - reject absolute paths, `..`, drive prefixes, backslashes, NUL, symlink entries,
///   characters Windows cannot store, trailing dots or spaces, names differing only in
///   case, and Windows reserved names (`CON`, `NUL`, `COM1`, ...) on every OS, so a dir
///   is portable
/// - skip `_metadata/` (store signature data) and `__MACOSX/`. Chromium and WebView2
///   refuse to load an unpacked dir containing other top-level `_`-prefixed entries,
///   so reject any except `_locales` and `_platform_specific`
/// - at most [`MAX_ENTRIES`] entries, and at most [`MAX_UNPACKED_BYTES`] (and
///   [`MAX_RATIO`] times the archive size) written in total, counted on the bytes
///   actually inflated rather than the sizes the archive claims
/// - `manifest.json` must be at the archive root
pub(crate) fn unpack_zip(zip_bytes: &[u8], dest: &Path) -> Result<(), InstallError> {
    let zip_error = |e: zip::result::ZipError| InstallError::Zip(e.to_string());
    let mut archive = zip::ZipArchive::new(io::Cursor::new(zip_bytes)).map_err(zip_error)?;
    if archive.len() > MAX_ENTRIES {
        return Err(InstallError::Zip(format!("more than {MAX_ENTRIES} entries")));
    }
    let budget = MAX_UNPACKED_BYTES.min((zip_bytes.len() as u64).saturating_mul(MAX_RATIO).saturating_add(RATIO_ALLOWANCE));
    if archive.decompressed_size().is_some_and(|claimed| claimed > u128::from(budget)) {
        return Err(InstallError::TooLargeUnpacked(budget));
    }

    fs::create_dir_all(dest)?;
    let mut written = 0u64;
    let mut seen = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(zip_error)?;
        let name = entry.name().map_err(zip_error)?.into_owned();
        let Some(rel) = entry_path(&name)? else { continue };
        if entry.is_symlink() {
            return Err(InstallError::UnsafePath(name));
        }
        if entry.encrypted() {
            return Err(InstallError::Zip(format!("{name:?} is encrypted")));
        }
        let target = rel.split('/').fold(dest.to_path_buf(), |p, segment| p.join(segment));
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if !seen.insert(rel.to_lowercase()) {
            return Err(InstallError::UnsafePath(name));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create_new(&target)?;
        let remaining = budget - written;
        written += io::copy(&mut (&mut entry).take(remaining + 1), &mut out).map_err(|e| InstallError::Zip(format!("{name}: {e}")))?;
        if written > budget {
            return Err(InstallError::TooLargeUnpacked(budget));
        }
    }
    if !dest.join("manifest.json").is_file() {
        return Err(InstallError::Zip("manifest.json is not at the archive root".into()));
    }
    Ok(())
}

/// The validated `/`-separated relative path of a zip entry, `None` for entries that
/// are skipped.
fn entry_path(name: &str) -> Result<Option<String>, InstallError> {
    let unsafe_path = || InstallError::UnsafePath(name.to_owned());
    let path = name.strip_suffix('/').unwrap_or(name);
    if path.is_empty() || path.starts_with('/') || path.contains(['\\', '\0', ':']) {
        return Err(unsafe_path());
    }
    let segments: Vec<&str> = path.split('/').collect();
    if !segments.iter().all(|s| is_portable_segment(s)) {
        return Err(unsafe_path());
    }
    match segments[0] {
        "_metadata" | "__MACOSX" => Ok(None),
        "_locales" | "_platform_specific" => Ok(Some(path.to_owned())),
        first if first.starts_with('_') => Err(unsafe_path()),
        _ => Ok(Some(path.to_owned())),
    }
}

fn is_portable_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.ends_with(['.', ' '])
        && !s.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        && !is_windows_reserved_name(s)
}

/// Rewrite `manifest.json` so its `key` is the one this install verified, because WebView2
/// derives an unpacked dir's id from it:
/// - CRX: `base64(spki_der)`, so the unpacked copy keeps the CRX id under WebView2 and
///   under our own runtime. Any `key` already there is replaced, as Chromium's installer
///   does: the CRX's developer key is the one that was verified.
/// - XPI (`None`): no `key`. An archive's `key` is just a public key, and with it the
///   engine would load the XPI as, and in place of, the extension that key belongs to.
///
/// Parses with the tolerant reader in `manifest.rs` (Chrome accepts comments in
/// manifest.json), and leaves the file as it is when there is nothing to change.
pub(crate) fn set_key(root: &Path, spki_der: Option<&[u8]>) -> Result<(), InstallError> {
    let path = root.join("manifest.json");
    let mut value = manifest::parse_tolerant_json(&manifest::read_text(&path)?)?;
    let obj = value.as_object_mut().ok_or_else(|| ManifestError::Json("the top level is not an object".into()))?;
    let changed = match spki_der {
        Some(der) => {
            obj.insert("key".into(), base64::engine::general_purpose::STANDARD.encode(der).into());
            true
        }
        None => obj.remove("key").is_some(),
    };
    if changed {
        fs::write(&path, serde_json::to_vec_pretty(&value).expect("a JSON value always serializes"))?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("network: {0}")]
    Network(String),
    #[error("server returned HTTP {0}")]
    Http(u16),
    #[error("the store has no version of this extension for this browser")]
    NotAvailable,
    #[error("unexpected store response: {0}")]
    BadStoreResponse(String),
    #[error("download larger than the {0} byte limit")]
    TooLarge(u64),
    #[error("archive expands to more than {0} bytes")]
    TooLargeUnpacked(u64),
    #[error(transparent)]
    Crx(#[from] CrxError),
    #[error("downloaded file does not match the store's sha256")]
    HashMismatch,
    #[error("archive: {0}")]
    Zip(String),
    #[error("archive entry with unsafe path {0:?}")]
    UnsafePath(String),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("extension id {actual} does not match the expected {expected}")]
    IdMismatch { expected: String, actual: String },
    #[error("only unpacked extensions can be reloaded")]
    NotUnpacked,
    #[error("extension {0} comes from a signed package, and an unverified copy cannot take its id")]
    VerifiedIdTaken(String),
    #[error("extension id {id} differs only in letter case from the installed {installed}")]
    IdCaseConflict { id: String, installed: String },
    #[error("{} is not a valid Unicode path", .0.display())]
    PathNotUnicode(PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_paths() {
        assert_eq!(entry_path("manifest.json").unwrap().as_deref(), Some("manifest.json"));
        assert_eq!(entry_path("js/a.js").unwrap().as_deref(), Some("js/a.js"));
        assert_eq!(entry_path("_locales/en/messages.json").unwrap().as_deref(), Some("_locales/en/messages.json"));
        assert_eq!(entry_path("js/").unwrap().as_deref(), Some("js"));
        assert_eq!(entry_path("_metadata/verified_contents.json").unwrap(), None);
        assert_eq!(entry_path("__MACOSX/._manifest.json").unwrap(), None);
        for bad in [
            "../evil.js",
            "a/../../evil.js",
            "/etc/passwd",
            "C:/Windows/evil.dll",
            "a\\b.js",
            "a//b.js",
            "./a.js",
            "con.js",
            "js/NUL",
            "a.",
            "a /b",
            "a?.js",
            "_private/x.js",
            "_generated.js",
            "",
            "/",
        ] {
            assert!(entry_path(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn download_url_shapes() {
        let id = ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap();
        assert_eq!(
            InstallSource::cws_download_url(&id, "150.0.7000.1").as_str(),
            "https://clients2.google.com/service/update2/crx?response=redirect&prodversion=150.0.7000.1&acceptformat=crx2,crx3&x=id%3Dddkjiahejlhfcafbddmgiahcphecmpfh%26uc"
        );
        let id = ExtensionId::parse("gcllgfdnfnllodcaambdaknbipemelie").unwrap();
        assert_eq!(
            edge_update_request(&id, "150.0.7000.1").to_string(),
            r#"{"request":{"acceptformat":"crx3","app":[{"appid":"gcllgfdnfnllodcaambdaknbipemelie","installsource":"ondemand","updatecheck":{},"version":"0.0.0.0"}],"prodversion":"150.0.7000.1","protocol":"3.1"}}"#
        );
        assert_eq!(InstallSource::amo_api_url("ublock-origin").as_str(), "https://addons.mozilla.org/api/v5/addons/addon/ublock-origin/");
        assert_eq!(
            InstallSource::amo_api_url("{d10d0bf8-f5b5-c8b4-a8b2-2b9879e08c5d}").as_str(),
            "https://addons.mozilla.org/api/v5/addons/addon/%7Bd10d0bf8-f5b5-c8b4-a8b2-2b9879e08c5d%7D/"
        );
    }

    #[test]
    fn edge_update_responses() {
        const XSSI: &str = ")]}'
";
        let hash = "B652C20CED279839FAE5A9B2C935BFB9FC32654496ECD9163615B8402D7892EF";
        let ok = serde_json::json!({ "response": { "app": [{ "status": "ok", "updatecheck": {
            "status": "ok",
            "urls": { "url": [{ "codebase": "https://cdn.example/f?P1=1" }] },
            "manifest": { "packages": { "package": [{ "hash_sha256": hash }] } },
        }}]}});
        let (url, sha256) = parse_edge_update(format!("{XSSI}{ok}").as_bytes()).unwrap();
        assert_eq!(url, "https://cdn.example/f?P1=1");
        assert_eq!(hex(&sha256), hash.to_ascii_lowercase());

        let unknown = br#"{"response":{"app":[{"appid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"error-unknownApplication"}]}}"#;
        assert!(matches!(parse_edge_update(unknown), Err(InstallError::NotAvailable)));
        let no_update = br#"{"response":{"app":[{"status":"ok","updatecheck":{"status":"noupdate"}}]}}"#;
        assert!(matches!(parse_edge_update(no_update), Err(InstallError::NotAvailable)));
        let no_hash = br#"{"response":{"app":[{"status":"ok","updatecheck":{"status":"ok","urls":{"url":[{"codebase":"https://x/"}]}}}]}}"#;
        assert!(matches!(parse_edge_update(no_hash), Err(InstallError::BadStoreResponse(_))));
        assert!(matches!(parse_edge_update(b"<html>"), Err(InstallError::BadStoreResponse(_))));
    }

    #[test]
    fn amo_hashes() {
        let hex64 = "ab".repeat(32);
        assert_eq!(parse_sha256(&format!("sha256:{hex64}")).unwrap(), [0xab; 32]);
        for bad in [hex64.clone(), format!("sha1:{hex64}"), "sha256:abcd".to_owned(), format!("sha256:{}", "zz".repeat(32))] {
            assert!(parse_sha256(&bad).is_err(), "{bad}");
        }
    }
}
