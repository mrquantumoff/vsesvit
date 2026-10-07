//! Extensions: desired state (synced) kept apart from actual state (per device).
//!
//! - `extensions` table ([`ExtensionRecord`], synced): "the user wants store extension X,
//!   enabled or not". Only store-origin extensions (Chrome Web Store, Edge Add-ons, AMO)
//!   have one, because only those can be installed on another device.
//! - `extension_installs` table (local): "this device has version V of X in dir D".
//!   Unpacked and file-installed extensions exist only here.
//!
//! Every in-flight state (downloading, verifying, unpacking) lives in memory as a value
//! ([`InstallJob`], [`StagedInstall`]) and is disposable. After a crash nothing is
//! half-recorded. `reconcile()` compares desired against actual and returns the work
//! still to do. That is the whole install state machine, and it is idempotent from any
//! crash point:
//!
//! ```text
//!             prepare_install()          job.run()  [worker thread]         commit()  [UI thread]
//!   source ───────────────────▶ InstallJob ──────────────────────▶ StagedInstall ─────────────────▶ installed
//!                                   │ Err(InstallError)                  │ dropped / desired changed      │
//!                                   ▼                                    ▼                                │ set_enabled / uninstall
//!                          nothing persisted              staging dir removed (Drop, or at next open)     ▼
//!
//!   desired.installed && no local row          ──reconcile()──▶ InstallJob (Intent::Reconcile)
//!     (or one holding the id less firmly)
//!   !desired.installed && local row from store ──reconcile()──▶ uninstalled locally (no new stamp)
//!   store row, newer version in the store      ──prepare_update_check()──▶ InstallJob (Intent::Update)
//! ```
//!
//! An update that Chrome would warn about anew ([`permissions::update_warnings`]) is
//! installed, but stays off on this device until the user re-enables it
//! ([`InstalledExtension::withheld`]).

mod sync_table;
pub(crate) use sync_table::ExtensionsTable;
pub mod commands;
pub mod crx;
mod install;
pub mod manifest;
pub mod notifications;
pub mod permissions;
pub mod private;
pub mod toolbar;
mod update;

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use install::{
    InstallError, InstallJob, InstallPhase, InstallSource, Intent, MAX_ARCHIVE_BYTES, MAX_ENTRIES, MAX_UNPACKED_BYTES, SourceParseError,
    StagedInstall, Stores, ui_locale,
};
use install::{StagedFiles, StagingDir};
use manifest::{Manifest, cmp_versions};
use permissions::{GRANTED_PERMISSIONS, PermissionMessage, PermissionSet};
pub use update::{FIRST_CHECK_DELAY, UPDATE_INTERVAL, UpdateCheck, UpdateReport, Updates};

use crate::crdt::{Extra, Lww};
use crate::db::Tx;
use crate::sync::SyncTable;
use crate::{Error, Profile};

/// Sent as `prodversion` to the Chrome Web Store. The store serves the newest version
/// compatible with it and returns nothing for a version it considers too old, so bump
/// this with releases. Windows overrides it with the WebView2 runtime's real version.
pub const DEFAULT_CHROME_VERSION: &str = "150.0.0.0";

/// Migration v11: what the user approved, as JSON [`PermissionSet`]: what the last version
/// they approved showed in its prompt, and what they granted it since. Kept only while the
/// installed version can do more (an update added warnings). NULL, as in every row before
/// it, is everything the installed version can do.
pub(crate) const SCHEMA_GRANTED: &str = "ALTER TABLE extension_installs ADD COLUMN granted TEXT;";

/// Validated extension id. It is also a directory name, so the charset is restricted
/// here, once: `[A-Za-z0-9._@{}-]`, 1..=80 chars, not `.`/`..`, no trailing `.`, not a
/// Windows reserved name.
///
/// - Chrome-style: 32 chars `a..p`, derived from a public key ([`ExtensionId::from_public_key`]).
/// - Gecko-style: `{uuid}` or `name@domain`, from `browser_specific_settings.gecko.id` or AMO.
/// - Unpacked without a `key`: Chrome-style id derived from the absolute path (as Chromium does).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ExtensionId(String);

impl ExtensionId {
    pub fn parse(s: &str) -> Result<Self, InvalidExtensionId> {
        let valid = (1..=80).contains(&s.len())
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._@{}-".contains(&b))
            && !s.ends_with('.')
            && !is_windows_reserved_name(s);
        if valid { Ok(ExtensionId(s.to_owned())) } else { Err(InvalidExtensionId) }
    }

    /// First 16 bytes of SHA-256(SPKI DER), each nibble mapped `0..f` -> `a..p`.
    pub fn from_public_key(spki_der: &[u8]) -> Self {
        Self::from_hash_input(spki_der)
    }

    /// The same nibble mapping applied to the CRX3 `SignedData.crx_id` bytes.
    pub fn from_crx_id(id: [u8; 16]) -> Self {
        ExtensionId(id.iter().flat_map(|b| [b >> 4, b & 0x0f]).map(|n| char::from(b'a' + n)).collect())
    }

    /// Chromium's `GenerateIdForPath`: the hash of the path's native encoding, which on
    /// Windows is UTF-16LE with an upper-case drive letter. Decided by the path's shape
    /// rather than the build target, so the result does not depend on where it runs.
    pub fn for_unpacked_dir(abs_path: &Path) -> Self {
        let path = abs_path.to_string_lossy();
        match path.as_bytes() {
            [drive, b':', ..] if drive.is_ascii_alphabetic() => {
                let mut native = path.into_owned();
                native[..1].make_ascii_uppercase();
                let utf16le: Vec<u8> = native.encode_utf16().flat_map(u16::to_le_bytes).collect();
                Self::from_hash_input(&utf16le)
            }
            bytes => Self::from_hash_input(bytes),
        }
    }

    fn from_hash_input(input: &[u8]) -> Self {
        let hash = Sha256::digest(input);
        let mut id = [0u8; 16];
        id.copy_from_slice(&hash[..16]);
        Self::from_crx_id(id)
    }

    pub fn is_chrome_style(&self) -> bool {
        self.0.len() == 32 && self.0.bytes().all(|b| (b'a'..=b'p').contains(&b))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ExtensionId {
    type Error = InvalidExtensionId;
    fn try_from(s: String) -> Result<Self, InvalidExtensionId> {
        ExtensionId::parse(&s)
    }
}

impl From<ExtensionId> for String {
    fn from(id: ExtensionId) -> String {
        id.0
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid extension id")]
pub struct InvalidExtensionId;

/// `CON`, `nul.txt`, `Com1`: names Windows maps to devices, whatever the extension.
/// Rejected on every OS so a profile or an unpacked dir is portable.
pub(crate) fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => {
            let b = stem.as_bytes();
            b.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && b[3].is_ascii_digit()
        }
    }
}

/// The synced part of an install source: which store another device should fetch from.
/// The id is the record key, and AMO's API accepts the gecko id in place of the slug.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreRef {
    ChromeWebStore,
    EdgeAddons,
    Amo,
}

impl StoreRef {
    /// The `extensions.store` column value (same spelling as serde).
    fn column(&self) -> &'static str {
        match self {
            StoreRef::ChromeWebStore => "chrome_web_store",
            StoreRef::EdgeAddons => "edge_addons",
            StoreRef::Amo => "amo",
        }
    }

    fn from_column(s: &str) -> Option<StoreRef> {
        [StoreRef::ChromeWebStore, StoreRef::EdgeAddons, StoreRef::Amo].into_iter().find(|store| store.column() == s)
    }

    /// How firmly this store's downloads hold their ids.
    fn id_hold(&self) -> IdHold {
        match self {
            StoreRef::ChromeWebStore | StoreRef::EdgeAddons => IdHold::Key,
            StoreRef::Amo => IdHold::Store,
        }
    }
}

/// Desired state, synced. Natural key: the extension id. Presence is an LWW register
/// (`installed`), not a tombstone, so uninstall-then-reinstall works.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionRecord {
    pub id: ExtensionId,
    pub store: Lww<StoreRef>,
    pub installed: Lww<bool>,
    pub enabled: Lww<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// How this device established that an install's files are what they claim to be.
/// Recorded per install, so the UI can say where an extension came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Verification {
    /// Downloaded from the Chrome Web Store: every CRX3 proof verified, the developer
    /// key derives to the requested id, and the Chrome Web Store publisher key signed it.
    /// That proof is required. Rows written when this carried a `publisher_verified` flag
    /// still load: the extra field is ignored.
    ChromeWebStore,
    /// Downloaded from Microsoft Edge Add-ons, matching the update service's sha256: every
    /// CRX3 proof verified, the developer key derives to the requested id, and the Edge
    /// Add-ons publisher key signed it. That proof is required.
    EdgeAddons,
    /// Downloaded from addons.mozilla.org over TLS, matching the API's sha256. Mozilla's
    /// own signature is not checked.
    AmoHash,
    /// A local `.crx`: every proof verified, id derived from the developer key.
    LocalCrx,
    /// A local `.xpi`: no signature check.
    LocalXpi,
    /// A developer directory, loaded in place.
    Unpacked,
}

impl Verification {
    /// The id is the hash of a developer key this device saw sign the package. A manifest
    /// `key` proves nothing: it is a public key.
    fn binds_key(&self) -> bool {
        matches!(self, Verification::ChromeWebStore | Verification::EdgeAddons | Verification::LocalCrx)
    }

    /// Where the install came from and how it was checked, as both shells' extensions pages say it.
    pub fn label(&self) -> &'static str {
        match self {
            Verification::ChromeWebStore => "Chrome Web Store, publisher verified",
            Verification::EdgeAddons => "Edge Add-ons, publisher verified",
            Verification::AmoHash => "Firefox Add-ons, hash checked",
            Verification::LocalCrx => "Local CRX, signature verified",
            Verification::LocalXpi => "Local XPI, not verified",
            Verification::Unpacked => "Unpacked folder, not verified",
        }
    }

    fn id_hold(&self) -> IdHold {
        match self {
            _ if self.binds_key() => IdHold::Key,
            Verification::AmoHash => IdHold::Store,
            _ => IdHold::Nothing,
        }
    }
}

/// What ties an install to its id, weakest first. An install may not take an id held
/// more firmly (see `check_id`).
#[derive(PartialEq, PartialOrd)]
enum IdHold {
    /// A local `.xpi` or an unpacked dir: its manifest names the id, and anyone can write one.
    Nothing,
    /// AMO served it under that id, and AMO gives each gecko id to one developer.
    Store,
    /// The id is the hash of a developer key that signed it.
    Key,
}

impl IdHold {
    /// From the `extension_installs.source_kind` column ([`InstallSource::kind`]), which
    /// fixes the verification, so a row whose `verification` does not decode still has
    /// one. A kind this build does not know keeps its id.
    fn of_source_kind(kind: &str) -> IdHold {
        match kind {
            "amo" => IdHold::Store,
            "xpi_file" | "unpacked" => IdHold::Nothing,
            _ => IdHold::Key,
        }
    }
}

/// What a shell loads into its engine.
#[derive(Clone, Debug)]
pub struct InstalledExtension {
    pub id: ExtensionId,
    pub version: String,
    /// Absolute. Managed installs: `<root>/extensions/<id>/<version>_<hash32>`, immutable,
    /// since WebView2 drops an extension whose files change. Unpacked: the developer's dir.
    pub dir: PathBuf,
    /// Parsed and localized at install time. The dir is immutable, so this never goes stale.
    pub manifest: Manifest,
    /// Whether the engine runs it: the user's choice, owned by the synced record for store
    /// installs and by the local row otherwise, and nothing [`withheld`](Self::withheld).
    pub enabled: bool,
    /// Chrome's warnings for what this version can do beyond what the user approved: an
    /// update added them. While this is not empty the extension is off on this device,
    /// whatever the user's choice, until the user re-enables it
    /// (`Extensions::approve_permissions`). Local: another device decides for itself.
    pub withheld: Vec<PermissionMessage>,
    pub source: InstallSource,
    pub verification: Verification,
    /// The id WebView2 assigned when it loaded *this* `dir`
    /// (`CoreWebView2BrowserExtension.Id`), recorded by the Windows shell.
    /// `None` means the engine has not loaded the current dir yet: a fresh install,
    /// or an update. `commit` clears it whenever `dir` changes, except for an update that is
    /// [`withheld`](Self::withheld): the engine keeps the version before, off, and loads this
    /// one only once `approve_permissions` clears the id. It equals `id` when the
    /// manifest carries `key` (every CRX install) and differs for XPI (whose `key` is
    /// removed at install) and keyless unpacked installs, which is why it is stored.
    pub engine_id: Option<String>,
}

/// Work `reconcile()` found.
pub struct Reconcile {
    /// Desired but missing on this device. Run each off the UI thread, then `commit`.
    pub install: Vec<InstallJob>,
    /// Removed locally because another device uninstalled them. Already gone from
    /// `list()`, and their dirs are left for the GC at next open, because the engine may
    /// still hold their files. The shell unloads them from its engine.
    pub removed: Vec<ExtensionId>,
}

pub struct Extensions<'p> {
    pub(crate) p: &'p mut Profile,
}

impl Extensions<'_> {
    /// Access pattern 4. Reads rows only; no disk scan. In install order, which an
    /// update keeps. Skips a row this build cannot decode (damaged, or written by another
    /// version); `on_open` keeps its files.
    pub fn list(&mut self) -> Result<Vec<InstalledExtension>, Error> {
        let sql = format!("{SELECT_INSTALLED} ORDER BY i.installed_ms, i.id");
        let mut stmt = self.p.conn.prepare(&sql)?;
        let mut installed = Vec::new();
        for row in stmt.query_map([], LoadedRow::from_sql)? {
            match row {
                Ok(row) => installed.push(row.into_installed(&self.p.paths.extensions)),
                Err(e @ rusqlite::Error::FromSqlConversionFailure(..)) => log::warn!("skipping an installed extension: {e}"),
                Err(e) => return Err(e.into()),
            }
        }
        Ok(installed)
    }

    pub fn get(&mut self, id: &ExtensionId) -> Result<Option<InstalledExtension>, Error> {
        Ok(self.loaded(id)?.map(|r| r.into_installed(&self.p.paths.extensions)))
    }

    /// Cheap and synchronous: allocates a staging dir name and captures the paths and
    /// `chrome_version` the job needs. Performs no I/O beyond that. `Intent::User`.
    /// Refuses a path that is not Unicode: the source is stored as JSON, which cannot spell it.
    pub fn prepare_install(&mut self, source: InstallSource) -> Result<InstallJob, Error> {
        if let InstallSource::CrxFile { path } | InstallSource::XpiFile { path } | InstallSource::Unpacked { dir: path } = &source
            && path.to_str().is_none()
        {
            return Err(InstallError::PathNotUnicode(path.clone()).into());
        }
        Ok(self.job(source, Intent::User))
    }

    /// UI thread, one transaction plus one rename. Idempotent:
    ///
    /// 1. `Intent::Reconcile` and `Intent::Update`: returns `Ok(None)` and discards the
    ///    files if the desired record no longer wants this extension (it was uninstalled on
    ///    another device while we downloaded). These commits never touch desired state. An
    ///    update is also dropped when this device no longer has the extension from the
    ///    source it was checked against (uninstalled, or replaced by a local copy).
    /// 2. Refuse an id this install may not take (see `check_id`).
    /// 3. If a staged package is older than the installed one, keep the installed one,
    ///    unless that holds its id less firmly (`IdHold`): a store copy replaces an
    ///    unverified one. An unpacked dir is the developer's own source of truth, so it
    ///    replaces the installed one at any version.
    /// 4. If `extensions/<id>/<version>_<hash32>` exists, it is complete (only a finished
    ///    staging dir is ever renamed into place, and dirs leave by a rename too), so drop
    ///    the staged copy. Otherwise rename the staging root into place (same volume, atomic).
    /// 5. `Intent::User` with a store source: `installed := true`, `store := ..`. A new
    ///    or previously uninstalled record starts `enabled := true`, as Chrome brings a
    ///    reinstalled extension back enabled; re-installing an installed one keeps its
    ///    enabled state. Unchanged registers mint no stamp, so re-running an install of
    ///    the same version changes nothing and causes no sync traffic.
    /// 6. Upsert the `extension_installs` row. Crash after the rename but before the
    ///    commit leaves an unreferenced dir that the next open GCs or the next install reuses.
    ///    An update keeps what the user approved before (`granted`), so a version Chrome
    ///    warns about anew is [`InstalledExtension::withheld`]; any other install is
    ///    approved as is.
    pub fn commit(&mut self, staged: StagedInstall) -> Result<Option<InstalledExtension>, Error> {
        let StagedInstall { id, source, intent, files, manifest, verification } = staged;
        if matches!(intent, Intent::Reconcile | Intent::Update) && !self.desired_installed(&id)? {
            return Ok(None);
        }
        let wanted_store = source.store().filter(|_| intent == Intent::User);
        let existing = self.row(&id)?;
        if intent == Intent::Update && existing.as_ref().is_none_or(|r| r.source != source) {
            return Ok(None);
        }
        self.check_id(&id, &verification, existing.as_ref())?;
        if let Some(row) = &existing
            && matches!(files, StagedFiles::Staged { .. })
            && cmp_versions(&manifest.version, &row.version) == Ordering::Less
            && verification.id_hold() <= row.verification.id_hold()
        {
            if let Some(store) = wanted_store {
                self.p.write(|tx| want_installed(tx, &id, store))?;
            }
            return self.get(&id);
        }

        // Managed: `<id>/<version>_<hash32>`, relative to `<root>/extensions`.
        let dir_text = match files {
            StagedFiles::Staged { staging, dir_name } => {
                place(staging, &self.p.paths.extensions.join(id.as_str()), &dir_name)?;
                format!("{}/{dir_name}", id.as_str())
            }
            // `prepare_install` already refused a dir that is not Unicode.
            StagedFiles::InPlace { dir } => dir.to_str().ok_or_else(|| InstallError::PathNotUnicode(dir.clone()))?.to_owned(),
        };
        let same_dir = existing.as_ref().is_some_and(|r| r.dir == dir_text);
        let granted = existing.as_ref().filter(|_| intent == Intent::Update).and_then(|r| {
            let since = self.p.prefs().get(&GRANTED_PERMISSIONS).remove(&id).unwrap_or_default();
            let approved = r.granted.clone().unwrap_or_else(|| permissions::prompted(&r.manifest)).union(&since);
            (!permissions::added_warnings(&approved, &permissions::prompted(&manifest)).is_empty()).then_some(approved)
        });
        let row = InstallRow {
            local_enabled: match source.store() {
                Some(_) => None,
                None => Some(existing.as_ref().and_then(|r| r.local_enabled).unwrap_or(true)),
            },
            // A withheld version is not loaded until approved: the engine keeps the one before.
            engine_id: existing.as_ref().filter(|_| same_dir || granted.is_some()).and_then(|r| r.engine_id.clone()),
            installed_ms: existing.as_ref().map_or_else(|| self.p.clock.now_ms() as i64, |r| r.installed_ms),
            id,
            version: manifest.version.clone(),
            dir: dir_text,
            source,
            verification,
            manifest,
            granted,
        };
        self.p.write(|tx| {
            if let Some(store) = wanted_store {
                want_installed(tx, &row.id, store)?;
            }
            row.upsert(tx)
        })?;
        self.get(&row.id)
    }

    /// Store installs: sets the synced `enabled` register. Local installs: sets the row.
    pub fn set_enabled(&mut self, id: &ExtensionId, enabled: bool) -> Result<(), Error> {
        let row = self.row(id)?;
        self.p.write(|tx| {
            if let Some(row) = row.as_ref().filter(|r| r.source.store().is_none()) {
                tx.sql.execute("UPDATE extension_installs SET local_enabled = ?2 WHERE id = ?1", params![row.id.as_str(), enabled])?;
                return Ok(());
            }
            let mut rec =
                ExtensionsTable::load(&tx.sql, id.as_str())?.filter(|rec| row.is_some() || rec.installed.v).ok_or(Error::NotFound)?;
            if tx.set_register(&mut rec.enabled, enabled) {
                let seq = tx.seq();
                ExtensionsTable::store(&tx.sql, &rec, seq)?;
            }
            Ok(())
        })
    }

    /// Store installs: `installed := false` (synced). A local install that took a store
    /// extension's id never wrote the synced record, so removing it leaves the record
    /// alone, and `reconcile` brings the store copy back. Deletes the local row and
    /// `storage.local`, and keeps `storage.sync` (Chrome keeps it too, so a reinstall gets
    /// its settings back). Dir removal is best effort. If the engine still holds files
    /// open, GC at next open removes the dir. A developer's unpacked dir is never touched.
    pub fn uninstall(&mut self, id: &ExtensionId) -> Result<(), Error> {
        let row = self.row(id)?;
        let owns_desired = row.as_ref().is_none_or(|r| r.source.store().is_some());
        self.p.write(|tx| {
            let mut was_wanted = false;
            if owns_desired && let Some(mut rec) = ExtensionsTable::load(&tx.sql, id.as_str())? {
                was_wanted = rec.installed.v;
                if tx.set_register(&mut rec.installed, false) {
                    let seq = tx.seq();
                    ExtensionsTable::store(&tx.sql, &rec, seq)?;
                }
            }
            if row.is_none() && !was_wanted {
                return Err(Error::NotFound);
            }
            delete_local(tx, id)
        })?;
        if row.is_some_and(|r| r.is_managed()) {
            remove_whole(&self.p.paths.extensions.join(id.as_str()), &self.p.paths.staging);
        }
        self.forget(id);
        Ok(())
    }

    /// What this device kept about an extension it no longer has, beside its row.
    fn forget(&mut self, id: &ExtensionId) {
        if let Err(e) = self.forget_permissions(id) {
            log::warn!("{}: cannot forget its granted permissions: {e}", id.as_str());
        }
    }

    /// Called at startup and whenever `ApplyReport::changed.extensions` is set.
    /// desired ∖ actual -> install jobs; actual-from-store ∖ desired -> removed locally.
    /// A local row that holds the id less firmly than the wanted store would (`IdHold`:
    /// an unpacked dir carrying a store extension's public key) does not count as actual.
    pub fn reconcile(&mut self) -> Result<Reconcile, Error> {
        let desired: HashMap<ExtensionId, (StoreRef, bool)> = {
            let mut stmt = self.p.conn.prepare("SELECT id, store, installed FROM extensions")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?)))?;
            let mut desired = HashMap::new();
            for row in rows {
                let (id, store, installed) = row?;
                if let (Ok(id), Some(store)) = (ExtensionId::parse(&id), StoreRef::from_column(&store)) {
                    desired.insert(id, (store, installed));
                }
            }
            desired
        };
        let actual: HashMap<ExtensionId, (bool, IdHold)> = {
            let mut stmt = self
                .p
                .conn
                .prepare("SELECT id, source_kind IN ('chrome_web_store', 'edge_addons', 'amo'), source_kind FROM extension_installs")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, String>(2)?)))?;
            let mut actual = HashMap::new();
            for row in rows {
                let (id, from_store, source_kind) = row?;
                actual.insert(ExtensionId::parse(&id).map_err(|e| conversion_error(0, e))?, (from_store, IdHold::of_source_kind(&source_kind)));
            }
            actual
        };

        let mut wanted: Vec<(&ExtensionId, &StoreRef)> = desired
            .iter()
            .filter(|(id, (store, installed))| *installed && actual.get(*id).is_none_or(|(_, hold)| *hold < store.id_hold()))
            .map(|(id, (store, _))| (id, store))
            .collect();
        wanted.sort();
        let install = wanted
            .into_iter()
            .map(|(id, store)| {
                let source = match store {
                    StoreRef::ChromeWebStore => InstallSource::ChromeWebStore { id: id.clone() },
                    StoreRef::EdgeAddons => InstallSource::EdgeAddons { id: id.clone() },
                    StoreRef::Amo => InstallSource::Amo { slug_or_guid: id.as_str().to_owned() },
                };
                self.job_expecting(source, Intent::Reconcile, Some(id.clone()))
            })
            .collect();

        let mut removed: Vec<ExtensionId> = actual
            .iter()
            .filter(|(id, (from_store, _))| *from_store && !desired.get(*id).is_some_and(|(_, installed)| *installed))
            .map(|(id, _)| id.clone())
            .collect();
        removed.sort();
        if !removed.is_empty() {
            self.p.write(|tx| removed.iter().try_for_each(|id| delete_local(tx, id)))?;
            for id in &removed {
                self.forget(id);
            }
        }
        Ok(Reconcile { install, removed })
    }

    /// Chrome's "Re-enable": the user approved what the installed version can do, so nothing
    /// is [`withheld`](InstalledExtension::withheld) any more and it runs again if the user's
    /// choice has it enabled. What the update added is granted, as Chrome grants it. Local to
    /// this device, like the withholding.
    pub fn approve_permissions(&mut self, id: &ExtensionId) -> Result<(), Error> {
        let row = self.row(id)?.ok_or(Error::NotFound)?;
        if let Some(approved) = &row.granted {
            self.grant_permissions(id, &permissions::required(&row.manifest).beyond(approved))?;
        }
        self.p.write(|tx| {
            let n = tx.sql.execute(
                "UPDATE extension_installs SET engine_id = CASE WHEN granted IS NULL THEN engine_id END, granted = NULL WHERE id = ?1",
                [id.as_str()],
            )?;
            if n == 0 { Err(Error::NotFound) } else { Ok(()) }
        })
    }

    /// Windows shell only: remember WebView2's id for this extension's current dir.
    pub fn set_engine_id(&mut self, id: &ExtensionId, engine_id: &str) -> Result<(), Error> {
        self.p.write(|tx| {
            let n = tx.sql.execute("UPDATE extension_installs SET engine_id = ?2 WHERE id = ?1", params![id.as_str(), engine_id])?;
            if n == 0 { Err(Error::NotFound) } else { Ok(()) }
        })
    }

    /// Unpacked dev extensions: re-read the manifest in place, after the developer
    /// clicks "reload". The id must not change (a `key` added or removed changes it; that
    /// is a new extension). The developer approves whatever it now asks for.
    pub fn reload_unpacked(&mut self, id: &ExtensionId) -> Result<InstalledExtension, Error> {
        let row = self.row(id)?.ok_or(Error::NotFound)?;
        let InstallSource::Unpacked { dir } = &row.source else {
            return Err(InstallError::NotUnpacked.into());
        };
        let manifest = Manifest::load(dir, &install::ui_locale()).map_err(InstallError::from)?;
        let reloaded = install::unpacked_id(dir, &manifest);
        if reloaded != *id {
            return Err(InstallError::IdMismatch { expected: id.as_str().to_owned(), actual: reloaded.as_str().to_owned() }.into());
        }
        let manifest_json = to_json(&manifest);
        self.p.write(|tx| {
            tx.sql.execute(
                "UPDATE extension_installs SET version = ?2, manifest = ?3, granted = NULL WHERE id = ?1",
                params![id.as_str(), manifest.version, manifest_json],
            )?;
            Ok(())
        })?;
        self.get(id)?.ok_or(Error::NotFound)
    }

    fn job(&self, source: InstallSource, intent: Intent) -> InstallJob {
        let expected = match &source {
            InstallSource::ChromeWebStore { id } | InstallSource::EdgeAddons { id } => Some(id.clone()),
            _ => None,
        };
        self.job_expecting(source, intent, expected)
    }

    fn job_expecting(&self, source: InstallSource, intent: Intent, expected_id: Option<ExtensionId>) -> InstallJob {
        InstallJob {
            source,
            intent,
            staging: StagingDir(self.p.paths.staging.join(uuid::Uuid::new_v4().simple().to_string())),
            chrome_version: self.p.chrome_version.clone(),
            ui_locale: install::ui_locale(),
            stores: self.p.stores.clone(),
            expected_id,
        }
    }

    fn loaded(&self, id: &ExtensionId) -> Result<Option<LoadedRow>, Error> {
        let sql = format!("{SELECT_INSTALLED} WHERE i.id = ?1");
        Ok(self.p.conn.query_row(&sql, [id.as_str()], LoadedRow::from_sql).optional()?)
    }

    fn row(&self, id: &ExtensionId) -> Result<Option<InstallRow>, Error> {
        Ok(self.loaded(id)?.map(|loaded| loaded.row))
    }

    fn desired_installed(&self, id: &ExtensionId) -> Result<bool, Error> {
        let installed =
            self.p.conn.query_row("SELECT installed FROM extensions WHERE id = ?1", [id.as_str()], |r| r.get::<_, bool>(0)).optional()?;
        Ok(installed.unwrap_or(false))
    }

    /// An install may not take:
    /// - an id held more firmly than it would hold it ([`IdHold`]), by an install here or
    ///   by the store a synced record wants it from. So a signature-verified package's id
    ///   goes only to a package that verified a developer key too (the id is that key's
    ///   hash, so it is the same key), and an AMO extension's id is not a local `.xpi`'s
    ///   or an unpacked dir's. The id owns the extension's storage and engine identity.
    /// - an id that differs from an installed one only in letter case, since on Windows
    ///   both would share `extensions/<id>`.
    fn check_id(&self, id: &ExtensionId, verification: &Verification, existing: Option<&InstallRow>) -> Result<(), Error> {
        let hold = verification.id_hold();
        let wanted_from: Option<String> = self
            .p
            .conn
            .query_row("SELECT store FROM extensions WHERE id = ?1 AND installed", [id.as_str()], |r| r.get(0))
            .optional()?;
        let wanted_hold = wanted_from.as_deref().and_then(StoreRef::from_column).map(|store| store.id_hold());
        if wanted_hold.is_some_and(|h| h > hold) || existing.is_some_and(|r| r.verification.id_hold() > hold) {
            return Err(InstallError::VerifiedIdTaken(id.as_str().to_owned()).into());
        }
        let other: Option<String> = self
            .p
            .conn
            .query_row("SELECT id FROM extension_installs WHERE id = ?1 COLLATE NOCASE AND id <> ?1", [id.as_str()], |r| r.get(0))
            .optional()?;
        match other {
            Some(installed) => Err(InstallError::IdCaseConflict { id: id.as_str().to_owned(), installed }.into()),
            None => Ok(()),
        }
    }
}

/// The desired-state half of a user install: `installed := true`, `store := store`,
/// and `enabled := true` for a record that did not exist yet or was uninstalled.
fn want_installed(tx: &mut Tx<'_>, id: &ExtensionId, store: StoreRef) -> Result<(), Error> {
    let record = match ExtensionsTable::load(&tx.sql, id.as_str())? {
        None => {
            let at = tx.stamp();
            Some(ExtensionRecord {
                id: id.clone(),
                store: Lww::new(store, at),
                installed: Lww::new(true, at),
                enabled: Lww::new(true, at),
                extra: Extra::new(),
            })
        }
        Some(mut rec) => {
            let reinstalled = tx.set_register(&mut rec.installed, true);
            let enabled = reinstalled && tx.set_register(&mut rec.enabled, true);
            let store_changed = tx.set_register(&mut rec.store, store);
            (reinstalled || enabled || store_changed).then_some(rec)
        }
    };
    if let Some(record) = record {
        let seq = tx.seq();
        ExtensionsTable::store(&tx.sql, &record, seq)?;
    }
    Ok(())
}

fn delete_local(tx: &mut Tx<'_>, id: &ExtensionId) -> Result<(), Error> {
    tx.sql.execute("DELETE FROM extension_installs WHERE id = ?1", [id.as_str()])?;
    tx.sql.execute("DELETE FROM ext_storage_local WHERE ext = ?1", [id.as_str()])?;
    Ok(())
}

/// Moves a finished staging root to `<id_dir>/<dir_name>` unless that dir already exists.
/// Windows can briefly refuse to rename a directory whose new files an antivirus is
/// still scanning, so an access-denied rename is retried a few times.
fn place(staging: StagingDir, id_dir: &Path, dir_name: &str) -> Result<(), Error> {
    let target = id_dir.join(dir_name);
    if target.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(id_dir)?;
    let mut attempt = 0;
    loop {
        match fs::rename(staging.root(), &target) {
            Ok(()) => return Ok(()),
            Err(_) if target.is_dir() => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && attempt < 10 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Best-effort delete of a dir under `extensions/` that never leaves part of it behind,
/// because `place` takes an existing content-addressed dir as complete. Windows deletes
/// file by file and stops at one another process holds open, so the dir is first renamed
/// into `staging/`, which every open wipes. A rename Windows refuses (a file inside is
/// open) leaves the dir whole, and the GC at next open tries again.
fn remove_whole(path: &Path, staging: &Path) {
    let trash = staging.join(uuid::Uuid::new_v4().simple().to_string());
    if fs::rename(path, &trash).is_ok() {
        let _ = fs::remove_dir_all(&trash).or_else(|_| fs::remove_file(&trash));
    }
}

fn conversion_error(column: usize, e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(e))
}

/// Every `extension_installs` column, then the user's enabled choice: the local column
/// for local installs, the synced register for store installs.
const SELECT_INSTALLED: &str = "SELECT i.id, i.version, i.dir, i.source, i.verification, i.manifest, i.local_enabled, \
     i.engine_id, i.installed_ms, i.granted, COALESCE(i.local_enabled, e.enabled, 0) \
     FROM extension_installs i LEFT JOIN extensions e ON e.id = i.id";

/// One `extension_installs` row.
struct InstallRow {
    id: ExtensionId,
    version: String,
    dir: String,
    source: InstallSource,
    verification: Verification,
    manifest: Manifest,
    local_enabled: Option<bool>,
    engine_id: Option<String>,
    /// When this extension was first installed on this device; `list` orders by it.
    installed_ms: i64,
    /// What the user approved, while `manifest` can do more (`SCHEMA_GRANTED`).
    granted: Option<PermissionSet>,
}

/// A row as `SELECT_INSTALLED` reads it.
struct LoadedRow {
    row: InstallRow,
    /// The user's choice, before anything is withheld.
    enabled: bool,
    /// The stored manifest predates [`Manifest::commands`].
    without_commands: bool,
}

impl LoadedRow {
    fn from_sql(r: &rusqlite::Row<'_>) -> rusqlite::Result<LoadedRow> {
        fn json<T: serde::de::DeserializeOwned>(r: &rusqlite::Row<'_>, col: usize) -> rusqlite::Result<T> {
            serde_json::from_str(&r.get::<_, String>(col)?).map_err(|e| conversion_error(col, e))
        }
        let manifest: serde_json::Value = json(r, 5)?;
        let without_commands = manifest.get("commands").is_none();
        let row = InstallRow {
            id: ExtensionId::parse(&r.get::<_, String>(0)?).map_err(|e| conversion_error(0, e))?,
            version: r.get(1)?,
            dir: r.get(2)?,
            source: json(r, 3)?,
            verification: json(r, 4)?,
            manifest: serde_json::from_value(manifest).map_err(|e| conversion_error(5, e))?,
            local_enabled: r.get(6)?,
            engine_id: r.get(7)?,
            installed_ms: r.get(8)?,
            granted: r
                .get::<_, Option<String>>(9)?
                .map(|text| serde_json::from_str(&text).map_err(|e| conversion_error(9, e)))
                .transpose()?,
        };
        Ok(LoadedRow { row, enabled: r.get(10)?, without_commands })
    }

    fn into_installed(self, extensions_root: &Path) -> InstalledExtension {
        let LoadedRow { row, enabled, without_commands } = self;
        let dir = resolve_dir(&row.dir, row.is_managed(), extensions_root);
        let mut manifest = row.manifest;
        if without_commands {
            manifest.commands = manifest.commands_from_raw(&dir, &install::ui_locale());
        }
        let withheld = row.granted.map(|approved| permissions::added_warnings(&approved, &permissions::prompted(&manifest))).unwrap_or_default();
        InstalledExtension {
            dir,
            id: row.id,
            version: row.version,
            manifest,
            enabled: enabled && withheld.is_empty(),
            withheld,
            source: row.source,
            verification: row.verification,
            engine_id: row.engine_id,
        }
    }
}

impl InstallRow {
    fn is_managed(&self) -> bool {
        !matches!(self.source, InstallSource::Unpacked { .. })
    }

    fn upsert(&self, tx: &mut Tx<'_>) -> Result<(), Error> {
        tx.sql.execute(
            "INSERT OR REPLACE INTO extension_installs \
             (id, version, dir, source_kind, source, verification, manifest, local_enabled, engine_id, installed_ms, granted) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                self.id.as_str(),
                self.version,
                self.dir,
                self.source.kind(),
                to_json(&self.source),
                to_json(&self.verification),
                to_json(&self.manifest),
                self.local_enabled,
                self.engine_id,
                self.installed_ms,
                self.granted.as_ref().map(to_json),
            ],
        )?;
        Ok(())
    }
}

/// The `dir` column as a path: under `extensions_root` for a managed install.
fn resolve_dir(dir: &str, managed: bool, extensions_root: &Path) -> PathBuf {
    if managed { dir.split('/').fold(extensions_root.to_path_buf(), |p, seg| p.join(seg)) } else { PathBuf::from(dir) }
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("install metadata always serializes")
}

/// Idempotent housekeeping, called by `Profile::open` after migrations: wipe `staging/`,
/// drop `extension_installs` rows whose dir is missing, delete version dirs no row
/// references, except those of a withheld update's extension, since the engine still holds
/// the version before it. Everything here is best effort: a file the OS will not let go of is
/// retried at the next open, and never stops the profile from opening.
pub(crate) fn on_open(p: &mut Profile) -> Result<(), Error> {
    let _ = fs::remove_dir_all(&p.paths.staging);
    fs::create_dir_all(&p.paths.staging)?;

    // (id, dir, managed, withheld). Only plain columns, so a row whose JSON this build cannot
    // decode still keeps its dir.
    let rows: Vec<(String, String, bool, bool)> = {
        let mut stmt = p.conn.prepare("SELECT id, dir, source_kind <> 'unpacked', granted IS NOT NULL FROM extension_installs")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?
    };
    let (present, vanished): (Vec<_>, Vec<_>) =
        rows.into_iter().partition(|(_, dir, managed, _)| resolve_dir(dir, *managed, &p.paths.extensions).is_dir());
    if !vanished.is_empty() {
        p.write(|tx| {
            for (id, _, _, _) in &vanished {
                tx.sql.execute("DELETE FROM extension_installs WHERE id = ?1", [id])?;
            }
            Ok(())
        })?;
    }

    // Compared ignoring case: on Windows `extensions/<id>` may be an existing dir spelled in
    // another case, and `commit` keeps installed ids distinct ignoring case.
    let referenced: HashSet<String> =
        present.iter().filter(|(_, _, managed, _)| *managed).map(|(_, dir, _, _)| dir.to_ascii_lowercase()).collect();
    let withheld: HashSet<String> =
        present.iter().filter(|(_, _, managed, withheld)| *managed && *withheld).map(|(id, _, _, _)| id.to_ascii_lowercase()).collect();
    let Ok(id_dirs) = fs::read_dir(&p.paths.extensions) else { return Ok(()) };
    for id_dir in id_dirs.flatten() {
        let id_name = id_dir.file_name().to_string_lossy().into_owned();
        if withheld.contains(&id_name.to_ascii_lowercase()) {
            continue;
        }
        if let Ok(versions) = fs::read_dir(id_dir.path()) {
            for version in versions.flatten() {
                let rel = format!("{id_name}/{}", version.file_name().to_string_lossy());
                if !referenced.contains(&rel.to_ascii_lowercase()) {
                    remove_whole(&version.path(), &p.paths.staging);
                }
            }
        }
        // Succeeds only when empty, which is exactly when no version is left.
        let _ = fs::remove_dir(id_dir.path());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_id_charset_and_reserved_names() {
        for good in ["ddkjiahejlhfcafbddmgiahcphecmpfh", "uBlock0@raymondhill.net", "{d10d0bf8-f5b5-c8b4-a8b2-2b9879e08c5d}", "a.b-c_d"] {
            assert!(ExtensionId::parse(good).is_ok(), "{good}");
        }
        let long = "a".repeat(81);
        for bad in ["", ".", "..", "a.", "a/b", "a\\b", "CON", "nul.txt", "Com1", "lpt9.x", long.as_str(), "a b", "a:b"] {
            assert!(ExtensionId::parse(bad).is_err(), "{bad:?}");
        }
        assert!(ExtensionId::parse("console").is_ok());
        assert!(ExtensionId::parse("com10").is_ok());
    }

    #[test]
    fn verification_labels() {
        assert_eq!(Verification::ChromeWebStore.label(), "Chrome Web Store, publisher verified");
        assert_eq!(Verification::EdgeAddons.label(), "Edge Add-ons, publisher verified");
        assert_eq!(Verification::Unpacked.label(), "Unpacked folder, not verified");
    }

    #[test]
    fn legacy_chrome_web_store_rows_still_load() {
        let legacy = r#"{"kind":"chrome_web_store","publisher_verified":true}"#;
        assert_eq!(serde_json::from_str::<Verification>(legacy).unwrap(), Verification::ChromeWebStore);
        assert_eq!(serde_json::to_string(&Verification::ChromeWebStore).unwrap(), r#"{"kind":"chrome_web_store"}"#);
    }

    #[test]
    fn chrome_ids_map_nibbles_to_a_through_p() {
        let id = ExtensionId::from_crx_id([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0, 0, 0, 0, 0, 0, 0, 0xff]);
        assert_eq!(id.as_str(), "abcdefghijklmnopaaaaaaaaaaaaaapp");
        assert!(id.is_chrome_style());
        assert!(!ExtensionId::parse("uBlock0@raymondhill.net").unwrap().is_chrome_style());
    }

    #[test]
    fn unpacked_ids_follow_chromium_path_hashing() {
        // Windows: UTF-16LE with the drive letter upper-cased, so both spellings agree.
        let lower = ExtensionId::for_unpacked_dir(Path::new(r"c:\dev\ext"));
        let upper = ExtensionId::for_unpacked_dir(Path::new(r"C:\dev\ext"));
        assert_eq!(lower, upper);
        let utf16: Vec<u8> = r"C:\dev\ext".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(upper, ExtensionId::from_public_key(&utf16));
        // POSIX: the UTF-8 bytes.
        assert_eq!(ExtensionId::for_unpacked_dir(Path::new("/home/dev/ext")), ExtensionId::from_public_key(b"/home/dev/ext"));
    }
}

/// Store installs and reconcile, offline: a verified probe install is relabelled as a
/// Chrome Web Store download, and "another device" edits the synced record directly.
#[cfg(all(test, feature = "testkit"))]
mod store_tests {
    use super::*;
    use crate::testkit;
    use crate::{OpenOptions, Profile};

    struct TempProfile {
        dir: PathBuf,
        p: Option<Profile>,
    }

    impl TempProfile {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("vsesvit-store-{}", uuid::Uuid::new_v4().simple()));
            let p = Profile::open(&dir.join("profile"), OpenOptions::default()).unwrap();
            TempProfile { dir, p: Some(p) }
        }
        fn p(&mut self) -> &mut Profile {
            self.p.as_mut().unwrap()
        }
    }

    impl Drop for TempProfile {
        fn drop(&mut self) {
            self.p = None;
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn probe_id() -> ExtensionId {
        ExtensionId::parse(testkit::PROBE_ID).unwrap()
    }

    fn staged_from_store(t: &mut TempProfile, intent: Intent) -> StagedInstall {
        let crx = t.dir.join("probe.crx");
        fs::write(&crx, testkit::probe_crx()).unwrap();
        let mut job = t.p().extensions().prepare_install(InstallSource::CrxFile { path: crx }).unwrap();
        job.intent = intent;
        let mut staged = job.run(&mut |_| {}).unwrap();
        staged.source = InstallSource::ChromeWebStore { id: probe_id() };
        staged.verification = Verification::ChromeWebStore;
        staged
    }

    fn desired(t: &mut TempProfile) -> Option<(ExtensionRecord, i64)> {
        let rec = t.p().write(|tx| ExtensionsTable::load(&tx.sql, testkit::PROBE_ID)).unwrap()?;
        let seq = t.p().conn.query_row("SELECT seq FROM extensions WHERE id = ?1", [testkit::PROBE_ID], |r| r.get(0)).unwrap();
        Some((rec, seq))
    }

    /// What a sync apply from another device does: a newer stamp on `installed`.
    fn remote_set_installed(t: &mut TempProfile, installed: bool) {
        t.p()
            .write(|tx| {
                let mut rec = ExtensionsTable::load(&tx.sql, testkit::PROBE_ID)?.unwrap();
                rec.installed = Lww::new(installed, tx.stamp());
                let seq = tx.seq();
                ExtensionsTable::store(&tx.sql, &rec, seq)
            })
            .unwrap();
    }

    #[test]
    fn a_user_store_install_writes_desired_state_once() {
        let mut t = TempProfile::new();
        let staged = staged_from_store(&mut t, Intent::User);
        let ext = t.p().extensions().commit(staged).unwrap().unwrap();
        assert!(ext.enabled);
        assert_eq!(ext.verification, Verification::ChromeWebStore);
        let (rec, seq) = desired(&mut t).unwrap();
        assert_eq!((rec.store.v.clone(), rec.installed.v, rec.enabled.v), (StoreRef::ChromeWebStore, true, true));

        let staged = staged_from_store(&mut t, Intent::User);
        t.p().extensions().commit(staged).unwrap().unwrap();
        assert_eq!(desired(&mut t).unwrap(), (rec.clone(), seq), "a same-version re-install mints nothing");

        t.p().extensions().set_enabled(&ext.id, false).unwrap();
        let (disabled, disabled_seq) = desired(&mut t).unwrap();
        assert!(!disabled.enabled.v && disabled.enabled.at > rec.enabled.at && disabled_seq > seq);
        assert!(!t.p().extensions().get(&ext.id).unwrap().unwrap().enabled, "store installs read the synced register");
        t.p().extensions().set_enabled(&ext.id, false).unwrap();
        assert_eq!(desired(&mut t).unwrap(), (disabled.clone(), disabled_seq), "an unchanged value mints nothing");

        let staged = staged_from_store(&mut t, Intent::User);
        assert!(!t.p().extensions().commit(staged).unwrap().unwrap().enabled, "re-install keeps the synced enabled state");

        let tabs = permissions::PermissionSet { apis: ["tabs".to_owned()].into(), ..Default::default() };
        t.p().extensions().grant_permissions(&ext.id, &tabs).unwrap();
        t.p().extensions().uninstall(&ext.id).unwrap();
        let (gone, _) = desired(&mut t).unwrap();
        assert!(!gone.installed.v, "uninstall is synced");
        assert!(t.p().prefs().get(&permissions::GRANTED_PERMISSIONS).is_empty(), "its granted permissions go with it");
        assert!(t.p().extensions().list().unwrap().is_empty());
        let work = t.p().extensions().reconcile().unwrap();
        assert!(work.install.is_empty() && work.removed.is_empty());
    }

    #[test]
    fn only_a_users_install_of_a_package_asks_first() {
        let mut t = TempProfile::new();
        assert!(staged_from_store(&mut t, Intent::User).needs_approval());
        assert!(!staged_from_store(&mut t, Intent::Reconcile).needs_approval(), "another device's install");
        let dir = t.dir.join("unpacked");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.json"), r#"{"manifest_version":3,"name":"u","version":"1"}"#).unwrap();
        let job = t.p().extensions().prepare_install(InstallSource::from_path(&dir).unwrap()).unwrap();
        assert!(!job.run(&mut |_| {}).unwrap().needs_approval(), "a developer's folder");
    }

    #[test]
    fn a_store_reinstall_after_uninstall_comes_back_enabled() {
        let mut t = TempProfile::new();
        let staged = staged_from_store(&mut t, Intent::User);
        let ext = t.p().extensions().commit(staged).unwrap().unwrap();
        t.p().extensions().set_enabled(&ext.id, false).unwrap();
        t.p().extensions().uninstall(&ext.id).unwrap();

        let staged = staged_from_store(&mut t, Intent::User);
        assert!(t.p().extensions().commit(staged).unwrap().unwrap().enabled, "as Chrome brings it back");
        let (rec, _) = desired(&mut t).unwrap();
        assert!(rec.installed.v && rec.enabled.v, "other devices install it enabled too");
    }

    #[test]
    fn reconcile_follows_other_devices() {
        let mut t = TempProfile::new();
        let staged = staged_from_store(&mut t, Intent::User);
        let ext = t.p().extensions().commit(staged).unwrap().unwrap();

        remote_set_installed(&mut t, false);
        let (before, before_seq) = desired(&mut t).unwrap();
        let work = t.p().extensions().reconcile().unwrap();
        assert_eq!(work.removed, [probe_id()]);
        assert!(work.install.is_empty());
        assert!(t.p().extensions().list().unwrap().is_empty());
        assert!(ext.dir.is_dir(), "files stay until the next open: the engine may still hold them");
        assert_eq!(desired(&mut t).unwrap(), (before, before_seq), "reconcile never writes desired state");

        remote_set_installed(&mut t, true);
        let work = t.p().extensions().reconcile().unwrap();
        assert!(work.removed.is_empty());
        let [job] = <[InstallJob; 1]>::try_from(work.install).ok().unwrap();
        assert_eq!(job.source(), &InstallSource::ChromeWebStore { id: probe_id() });
        assert_eq!((job.intent, job.expected_id.clone()), (Intent::Reconcile, Some(probe_id())));

        let (wanted, wanted_seq) = desired(&mut t).unwrap();
        let staged = staged_from_store(&mut t, Intent::Reconcile);
        let back = t.p().extensions().commit(staged).unwrap().expect("still wanted");
        assert_eq!(back.dir, ext.dir);
        assert_eq!(desired(&mut t).unwrap(), (wanted, wanted_seq), "a reconcile commit never writes desired state");

        // Uninstalled elsewhere while this device was downloading: the result is dropped.
        let staged = staged_from_store(&mut t, Intent::Reconcile);
        t.p().extensions().uninstall(&probe_id()).unwrap();
        assert!(t.p().extensions().commit(staged).unwrap().is_none());
        assert!(t.p().extensions().list().unwrap().is_empty());
    }

    #[test]
    fn reconcile_builds_amo_jobs_from_the_gecko_id() {
        let mut t = TempProfile::new();
        t.p()
            .write(|tx| {
                let at = tx.stamp();
                let rec = ExtensionRecord {
                    id: ExtensionId::parse("uBlock0@raymondhill.net").unwrap(),
                    store: Lww::new(StoreRef::Amo, at),
                    installed: Lww::new(true, at),
                    enabled: Lww::new(true, at),
                    extra: Extra::new(),
                };
                let seq = tx.seq();
                ExtensionsTable::store(&tx.sql, &rec, seq)
            })
            .unwrap();
        let work = t.p().extensions().reconcile().unwrap();
        assert_eq!(work.install.len(), 1);
        assert_eq!(work.install[0].source(), &InstallSource::Amo { slug_or_guid: "uBlock0@raymondhill.net".into() });
        assert!(t.p().extensions().set_enabled(&ExtensionId::parse("uBlock0@raymondhill.net").unwrap(), false).is_ok());
    }

    #[test]
    fn removing_a_local_copy_leaves_the_store_extension_wanted() {
        let mut t = TempProfile::new();
        let staged = staged_from_store(&mut t, Intent::User);
        t.p().extensions().commit(staged).unwrap().unwrap();
        // The same extension from a local .crx: same developer key, so it may replace the store row.
        let crx = InstallSource::CrxFile { path: t.dir.join("probe.crx") };
        let staged = t.p().extensions().prepare_install(crx.clone()).unwrap().run(&mut |_| {}).unwrap();
        assert_eq!(t.p().extensions().commit(staged).unwrap().unwrap().source, crx);

        let before = desired(&mut t).unwrap();
        t.p().extensions().uninstall(&probe_id()).unwrap();
        assert_eq!(desired(&mut t).unwrap(), before, "a local copy does not own the synced record");
        let work = t.p().extensions().reconcile().unwrap();
        assert!(work.removed.is_empty());
        let [job] = <[InstallJob; 1]>::try_from(work.install).ok().unwrap();
        assert_eq!(job.source(), &InstallSource::ChromeWebStore { id: probe_id() }, "the store copy comes back");
    }

    /// A developer copy of the probe at `<dir>/dev`, version 9.0, whose manifest `key` is
    /// the probe's developer key, so it claims the store extension's id.
    fn unpacked_with_probe_key(t: &mut TempProfile) -> StagedInstall {
        use base64::Engine as _;
        let dev = t.dir.join("dev");
        fs::create_dir_all(&dev).unwrap();
        for (name, bytes) in testkit::PROBE_FILES {
            fs::write(dev.join(name), bytes).unwrap();
        }
        let key = base64::engine::general_purpose::STANDARD.encode(testkit::CrxKey::probe().public_key_der());
        let manifest = String::from_utf8(testkit::PROBE_FILES[0].1.to_vec()).unwrap();
        fs::write(dev.join("manifest.json"), manifest.replacen('{', &format!("{{\"key\": \"{key}\","), 1).replace("\"1.0.0\"", "\"9.0\""))
            .unwrap();
        let job = t.p().extensions().prepare_install(InstallSource::Unpacked { dir: dev }).unwrap();
        let staged = job.run(&mut |_| {}).unwrap();
        assert_eq!(staged.id, probe_id());
        staged
    }

    #[test]
    fn an_unpacked_dir_with_a_store_extension_key_cannot_take_its_id() {
        let mut t = TempProfile::new();

        // Wanted from the Chrome Web Store (another device installed it), not here yet.
        t.p()
            .write(|tx| {
                let at = tx.stamp();
                let rec = ExtensionRecord {
                    id: probe_id(),
                    store: Lww::new(StoreRef::ChromeWebStore, at),
                    installed: Lww::new(true, at),
                    enabled: Lww::new(true, at),
                    extra: Extra::new(),
                };
                let seq = tx.seq();
                ExtensionsTable::store(&tx.sql, &rec, seq)
            })
            .unwrap();
        let staged = unpacked_with_probe_key(&mut t);
        assert!(matches!(t.p().extensions().commit(staged), Err(Error::Install(InstallError::VerifiedIdTaken(_)))));
        assert!(t.p().extensions().list().unwrap().is_empty());

        // Installed from the store here.
        let staged = staged_from_store(&mut t, Intent::Reconcile);
        t.p().extensions().commit(staged).unwrap().unwrap();
        let staged = unpacked_with_probe_key(&mut t);
        assert!(matches!(t.p().extensions().commit(staged), Err(Error::Install(InstallError::VerifiedIdTaken(_)))));
        let ext = t.p().extensions().get(&probe_id()).unwrap().unwrap();
        assert_eq!((ext.version.as_str(), &ext.verification), ("1.0.0", &Verification::ChromeWebStore));
    }

    #[test]
    fn a_store_record_arriving_after_an_unpacked_copy_still_installs_the_store_copy() {
        let mut t = TempProfile::new();
        let staged = unpacked_with_probe_key(&mut t);
        t.p().extensions().commit(staged).unwrap().unwrap();

        remote_want(&mut t, &probe_id(), StoreRef::ChromeWebStore);
        let work = t.p().extensions().reconcile().unwrap();
        assert!(work.removed.is_empty());
        let [job] = <[InstallJob; 1]>::try_from(work.install).expect("the unverified copy does not count as installed");
        assert_eq!(job.source(), &InstallSource::ChromeWebStore { id: probe_id() });

        let staged = staged_from_store(&mut t, Intent::Reconcile);
        let ext = t.p().extensions().commit(staged).unwrap().unwrap();
        assert_eq!(
            (ext.version.as_str(), &ext.verification),
            ("1.0.0", &Verification::ChromeWebStore),
            "the store copy replaces a newer unverified one"
        );
        let work = t.p().extensions().reconcile().unwrap();
        assert!(work.install.is_empty() && work.removed.is_empty());
    }

    /// What a sync apply does when another device installs `id` from `store`.
    fn remote_want(t: &mut TempProfile, id: &ExtensionId, store: StoreRef) {
        t.p()
            .write(|tx| {
                let at = tx.stamp();
                let rec = ExtensionRecord {
                    id: id.clone(),
                    store: Lww::new(store, at),
                    installed: Lww::new(true, at),
                    enabled: Lww::new(true, at),
                    extra: Extra::new(),
                };
                let seq = tx.seq();
                ExtensionsTable::store(&tx.sql, &rec, seq)
            })
            .unwrap();
    }

    /// A local `.xpi` whose manifest claims `gecko_id`.
    fn local_xpi(t: &mut TempProfile, gecko_id: &ExtensionId, version: &str) -> StagedInstall {
        let manifest = format!(
            r#"{{"manifest_version": 2, "name": "X", "version": "{version}", "browser_specific_settings": {{"gecko": {{"id": "{}"}}}}}}"#,
            gecko_id.as_str()
        );
        let path = t.dir.join(format!("{version}.xpi"));
        fs::write(&path, testkit::zip_files(&[("manifest.json", manifest.as_bytes())])).unwrap();
        let staged = t.p().extensions().prepare_install(InstallSource::XpiFile { path }).unwrap().run(&mut |_| {}).unwrap();
        assert_eq!((&staged.id, &staged.verification), (gecko_id, &Verification::LocalXpi));
        staged
    }

    /// The same files as AMO's download of `gecko_id`, as a reconcile job stages them.
    fn from_amo(t: &mut TempProfile, gecko_id: &ExtensionId, version: &str) -> StagedInstall {
        let mut staged = local_xpi(t, gecko_id, version);
        staged.source = InstallSource::Amo { slug_or_guid: gecko_id.as_str().to_owned() };
        staged.verification = Verification::AmoHash;
        staged.intent = Intent::Reconcile;
        staged
    }

    #[test]
    fn a_local_xpi_cannot_take_the_gecko_id_of_an_amo_extension() {
        let mut t = TempProfile::new();
        let id = ExtensionId::parse("victim@example.org").unwrap();
        let taken = |r: Result<Option<InstalledExtension>, Error>| matches!(r, Err(Error::Install(InstallError::VerifiedIdTaken(_))));

        // Wanted from AMO (another device installed it), not here yet.
        remote_want(&mut t, &id, StoreRef::Amo);
        let staged = local_xpi(&mut t, &id, "9.0");
        assert!(taken(t.p().extensions().commit(staged)));
        assert!(t.p().extensions().list().unwrap().is_empty());

        // Installed from AMO here, with data of its own.
        let staged = from_amo(&mut t, &id, "1.0");
        t.p().extensions().commit(staged).unwrap().unwrap();
        t.p().conn.execute("INSERT INTO ext_storage_local (ext, key, value) VALUES (?1, 'secret', '1')", [id.as_str()]).unwrap();
        let staged = local_xpi(&mut t, &id, "9.0");
        assert!(taken(t.p().extensions().commit(staged)));
        let ext = t.p().extensions().get(&id).unwrap().unwrap();
        assert_eq!((ext.version.as_str(), &ext.verification), ("1.0", &Verification::AmoHash));
        let stored: u32 = t.p().conn.query_row("SELECT COUNT(*) FROM ext_storage_local WHERE ext = ?1", [id.as_str()], |r| r.get(0)).unwrap();
        assert_eq!(stored, 1);

        // AMO still updates its own extension.
        let staged = from_amo(&mut t, &id, "2.0");
        let ext = t.p().extensions().commit(staged).unwrap().unwrap();
        assert_eq!((ext.version.as_str(), &ext.verification), ("2.0", &Verification::AmoHash));
    }

    #[test]
    fn a_manifest_stored_before_commands_lists_them_localized() {
        let mut t = TempProfile::new();
        let dev = t.dir.join("dev");
        fs::create_dir_all(dev.join("_locales").join("en")).unwrap();
        fs::write(
            dev.join("manifest.json"),
            r#"{"manifest_version": 3, "name": "A", "version": "1", "default_locale": "en",
                "commands": {"run": {"suggested_key": "Alt+Shift+R", "description": "__MSG_run__"}}}"#,
        )
        .unwrap();
        fs::write(dev.join("_locales").join("en").join("messages.json"), r#"{"run": {"message": "Run it"}}"#).unwrap();
        let staged = t.p().extensions().prepare_install(InstallSource::Unpacked { dir: dev }).unwrap().run(&mut |_| {}).unwrap();
        let commands = t.p().extensions().commit(staged).unwrap().unwrap().manifest.commands;
        assert_eq!((commands[0].description.as_str(), commands[0].suggested_key.map(|k| k.to_string())), ("Run it", Some("Alt+Shift+R".into())));

        t.p().conn.execute("UPDATE extension_installs SET manifest = json_remove(manifest, '$.commands')", []).unwrap();
        let stored: String = t.p().conn.query_row("SELECT manifest FROM extension_installs", [], |r| r.get(0)).unwrap();
        assert!(!stored.contains("\"commands\":[") && stored.contains("__MSG_run__"), "as an older build stored it");
        let [listed] = <[InstalledExtension; 1]>::try_from(t.p().extensions().list().unwrap()).ok().unwrap();
        assert_eq!(listed.manifest.commands, commands);
    }

    fn schema(conn: &rusqlite::Connection) -> Vec<(String, String, Option<String>)> {
        let mut stmt = conn.prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name").unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<Result<_, _>>().unwrap()
    }

    /// A v3 profile (store CHECKs without 'edge_addons') with a Chrome Web Store record and
    /// an unpacked install: migration v4 keeps both rows, ends at the fresh schema, and the
    /// rebuilt tables take an Edge Add-ons install through its lifecycle.
    #[test]
    fn a_v3_profile_migrates_and_takes_an_edge_install() {
        let fresh_schema = schema(&TempProfile::new().p().conn);
        let dir = std::env::temp_dir().join(format!("vsesvit-store-{}", uuid::Uuid::new_v4().simple()));
        let root = dir.join("profile");
        let dev = dir.join("unpacked");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&dev).unwrap();
        for (name, bytes) in testkit::PROBE_FILES {
            fs::write(dev.join(name), bytes).unwrap();
        }
        let manifest = Manifest::load(&dev, "en").unwrap();
        let unpacked_id = install::unpacked_id(&dev, &manifest);
        let at = crate::crdt::Stamp { hlc: crate::crdt::Hlc(1), device: crate::crdt::DeviceId(7) };
        let cws_record = ExtensionRecord {
            id: ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap(),
            store: Lww::new(StoreRef::ChromeWebStore, at),
            installed: Lww::new(true, at),
            enabled: Lww::new(false, at),
            extra: Extra::new(),
        };
        {
            let mut conn = crate::db::open(&root.join("vsesvit.db")).unwrap();
            let tx = conn.transaction().unwrap();
            for sql in [crate::db::SCHEMA_V1, crate::db::SCHEMA_V1_EXTENSIONS, crate::favicons::SCHEMA, crate::downloads::SCHEMA] {
                tx.execute_batch(sql).unwrap();
            }
            tx.pragma_update(None, "user_version", 3).unwrap();
            ExtensionsTable::store(&tx, &cws_record, crate::crdt::Seq(1)).unwrap();
            tx.execute(
                "INSERT INTO extension_installs \
                 (id, version, dir, source_kind, source, verification, manifest, local_enabled, engine_id, installed_ms) \
                 VALUES (?1, '1.0.0', ?2, 'unpacked', ?3, ?4, ?5, 0, 'engine', 5)",
                params![
                    unpacked_id.as_str(),
                    dev.to_str().unwrap(),
                    to_json(&InstallSource::Unpacked { dir: dev.clone() }),
                    to_json(&Verification::Unpacked),
                    to_json(&manifest),
                ],
            )
            .unwrap();
            assert!(tx.execute("UPDATE extensions SET store = 'edge_addons'", []).is_err(), "v3 refuses the new store");
            tx.commit().unwrap();
        }

        let mut t = TempProfile { dir, p: Some(Profile::open(&root, OpenOptions::default()).unwrap()) };
        let version: u32 = t.p().conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, crate::db::SCHEMA_VERSION);
        assert_eq!(schema(&t.p().conn), fresh_schema, "a migrated profile has the fresh schema");
        let kept_record = t.p().write(|tx| ExtensionsTable::load(&tx.sql, cws_record.id.as_str())).unwrap().unwrap();
        assert_eq!(kept_record, cws_record);
        let [kept] = <[InstalledExtension; 1]>::try_from(t.p().extensions().list().unwrap()).ok().unwrap();
        assert_eq!((&kept.id, kept.enabled, kept.engine_id.as_deref()), (&unpacked_id, false, Some("engine")));

        let mut staged = staged_from_store(&mut t, Intent::User);
        staged.source = InstallSource::EdgeAddons { id: probe_id() };
        staged.verification = Verification::EdgeAddons;
        let ext = t.p().extensions().commit(staged).unwrap().unwrap();
        assert_eq!((ext.verification, ext.enabled), (Verification::EdgeAddons, true));
        let (rec, _) = desired(&mut t).unwrap();
        assert_eq!((rec.store.v, rec.installed.v), (StoreRef::EdgeAddons, true));

        remote_set_installed(&mut t, false);
        let work = t.p().extensions().reconcile().unwrap();
        assert_eq!(work.removed, [probe_id()], "an Edge install is a store install");

        remote_set_installed(&mut t, true);
        let work = t.p().extensions().reconcile().unwrap();
        let mut jobs: Vec<(InstallSource, Option<ExtensionId>)> =
            work.install.iter().map(|job| (job.source().clone(), job.expected_id.clone())).collect();
        jobs.sort_by_key(|(_, id)| id.clone());
        let mut expected = [
            (InstallSource::EdgeAddons { id: probe_id() }, Some(probe_id())),
            (InstallSource::ChromeWebStore { id: kept_record.id.clone() }, Some(kept_record.id.clone())),
        ];
        expected.sort_by_key(|(_, id)| id.clone());
        assert_eq!(jobs, expected, "the Edge record, and the migrated Chrome Web Store record");
        let staged = unpacked_with_probe_key(&mut t);
        assert!(
            matches!(t.p().extensions().commit(staged), Err(Error::Install(InstallError::VerifiedIdTaken(_)))),
            "wanted from Edge Add-ons, so an unverified copy cannot take the id"
        );
        let job = t.p().extensions().prepare_install(InstallSource::EdgeAddons { id: probe_id() }).unwrap();
        assert_eq!(job.expected_id, Some(probe_id()), "a user install checks the id too");
    }
}
