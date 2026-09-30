//! What a future sync engine calls. Nothing in core calls it, and no engine is built now.
//!
//! The engine (a later crate `vsesvit-sync` that depends on this one) needs exactly
//! four things:
//!
//! 1. `changes_since(kind, cursor, limit)`: local changes to upload.
//! 2. `apply(records)`: merge downloaded records. Idempotent, order-independent,
//!    any batch split.
//! 3. `engine_state` / `set_engine_state`: a place to keep its cursors; `secret_state` /
//!    `set_secret_state` for its tokens, sealed by the [`vault`](crate::vault).
//! 4. `ApplyReport::changed`: what the shell must refresh afterwards.
//!
//! There is no trait. There is one implementation (this) and one consumer (the engine),
//! so the "interface" is these concrete methods.
//!
//! Threading follows the same pattern as extension installs: network I/O runs on a
//! worker thread with owned `Vec<WireRecord>`s; `changes_since` and `apply` run on the UI
//! thread. The engine should apply in batches of a few hundred records, each batch one
//! transaction of a few milliseconds.
//!
//! Server model this is designed for: the dumbest one. The server stores the last
//! uploaded body per `(kind, id)` and never merges. Correctness needs one rule, in
//! `apply_one`: **if our merged state differs from the incoming record, mark it dirty so
//! it is uploaded again.** That repairs a server copy that a concurrent or stale upload
//! overwrote. When merged == incoming nothing is marked, so records do not echo back
//! and forth. `tests/convergence.rs` runs exactly this server.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::bookmarks::{BookmarkRecord, BookmarksTable};
use crate::crdt::{Lattice, Seq, Stamp};
use crate::db::Tx;
use crate::ext_storage::{StorageChange, StorageTable, SyncItemRecord};
use crate::extensions::{ExtensionId, ExtensionsTable};
use crate::history::{DeletionDirective, DeletionsTable, PagesTable};
use crate::permissions::SitePermissionsTable;
use crate::prefs::{PrefRecord, PrefsTable};
use crate::search::EnginesTable;
use crate::session::SessionsTable;
use crate::{Error, Profile, vault};

/// Values the engine keeps secret, apart from `sync_state` so a sealed value is never read as
/// a plain one.
pub(crate) const SECRETS_SCHEMA: &str = "
CREATE TABLE sync_secrets (              -- LOCAL
  key    TEXT PRIMARY KEY,
  value  BLOB NOT NULL                   -- sealed by vault::seal
) WITHOUT ROWID;
";

/// Stable numeric codes, so an engine can store them. Never reuse a retired code.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum Kind {
    Bookmarks = 1,
    HistoryPages = 2,
    HistoryDeletions = 3,
    Sessions = 4,
    Extensions = 5,
    ExtStorageSync = 6,
    Prefs = 7,
    SearchEngines = 8,
    // 9 = ReadingList (retired before release; never reuse), 10 = Passwords, 11 = Autofill: reserved (DESIGN.md "Passwords and autofill").
    SitePermissions = 12,
}

impl Kind {
    pub const ALL: &'static [Kind] = &[
        Kind::Bookmarks,
        Kind::HistoryPages,
        Kind::HistoryDeletions,
        Kind::Sessions,
        Kind::Extensions,
        Kind::ExtStorageSync,
        Kind::Prefs,
        Kind::SearchEngines,
        Kind::SitePermissions,
    ];

    pub fn code(self) -> u8 {
        self as u8
    }

    /// Unknown codes (from a newer build) return `None`. The engine leaves such records
    /// on the server untouched.
    pub fn from_code(code: u8) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| k.code() == code)
    }
}

/// What a person turns sync on or off for, as Chrome's "Customize sync" lists it. Each is one
/// or more [`Kind`]s.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    Bookmarks,
    History,
    /// Open tabs, which other devices list as "Tabs from other devices".
    Tabs,
    /// Installed extensions and their `storage.sync`.
    Extensions,
    /// Preferences, search engines and site permissions.
    Settings,
}

impl DataType {
    pub const ALL: [DataType; 5] = [DataType::Bookmarks, DataType::History, DataType::Tabs, DataType::Extensions, DataType::Settings];

    pub fn kinds(self) -> &'static [Kind] {
        match self {
            DataType::Bookmarks => &[Kind::Bookmarks],
            DataType::History => &[Kind::HistoryPages, Kind::HistoryDeletions],
            DataType::Tabs => &[Kind::Sessions],
            DataType::Extensions => &[Kind::Extensions, Kind::ExtStorageSync],
            DataType::Settings => &[Kind::Prefs, Kind::SearchEngines, Kind::SitePermissions],
        }
    }

    pub fn of(kind: Kind) -> DataType {
        DataType::ALL.into_iter().find(|t| t.kinds().contains(&kind)).expect("every kind has a data type")
    }

    pub fn label(self) -> &'static str {
        match self {
            DataType::Bookmarks => "Bookmarks",
            DataType::History => "History",
            DataType::Tabs => "Open tabs",
            DataType::Extensions => "Extensions",
            DataType::Settings => "Settings",
        }
    }
}

/// One record in transit. `body` is the kind's record type as UTF-8 JSON. The engine
/// treats it as opaque bytes (it will encrypt them end-to-end). `id` is the record's
/// server key: stable, unique within the kind, never reused with a different meaning.
///
/// | kind             | id                                  | body type                               |
/// |------------------|-------------------------------------|-----------------------------------------|
/// | Bookmarks        | uuid                                | `bookmarks::BookmarkRecord`             |
/// | HistoryPages     | url                                 | `history::PageRecord`                   |
/// | HistoryDeletions | uuid                                | `history::DeletionDirective`            |
/// | Sessions         | device id (16 hex)                  | `session::DeviceSessionRecord`          |
/// | Extensions       | extension id                        | `extensions::ExtensionRecord`           |
/// | ExtStorageSync   | `{ext}:{sha256(key)[..32]}`         | `ext_storage::SyncItemRecord`           |
/// | Prefs            | pref key                            | `prefs::PrefRecord`                     |
/// | SearchEngines    | engine id                           | `search::EngineRecord`                  |
/// | SitePermissions  | `{permission key}\|{origin}`        | `permissions::SitePermissionRecord`     |
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireRecord {
    pub kind: Kind,
    pub id: String,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct ChangeBatch {
    /// Ordered by seq, oldest first.
    pub records: Vec<WireRecord>,
    /// The cursor to store once these are uploaded, and to pass as `since` next time.
    /// If the process dies before that, the same records are uploaded again, which is
    /// harmless: apply is idempotent.
    pub upto: Seq,
    pub more: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ApplyReport {
    /// Merged and changed local state.
    pub merged: usize,
    /// Already known (incoming ⊑ local). Includes our own uploads echoed back.
    pub unchanged: usize,
    /// Failed boundary validation (bad JSON, kind mismatch, root id, invalid url). They
    /// are skipped, never partially applied. The engine logs them and does not retry.
    pub rejected: Vec<Rejected>,
    pub changed: Changed,
}

#[derive(Clone, Debug)]
pub struct Rejected {
    pub kind: Kind,
    pub id: String,
    pub reason: String,
}

/// What the shell refreshes after an apply. Core has no observer/event system: local
/// mutations return what they changed, and remote ones are summarized here.
#[derive(Clone, Debug, Default)]
pub struct Changed {
    pub bookmarks: bool,
    pub history: bool,
    pub sessions: bool,
    /// Call `extensions().reconcile()`.
    pub extensions: bool,
    /// For `storage.onChanged` in the Linux runtime.
    pub ext_storage: Vec<(ExtensionId, Vec<StorageChange>)>,
    pub prefs: Vec<String>,
    pub search_engines: bool,
    pub site_permissions: bool,
}

pub struct SyncStore<'p> {
    pub(crate) p: &'p mut Profile,
}

impl SyncStore<'_> {
    /// Records of `kind` with `seq >= since`, oldest first, about `limit` of them (a batch
    /// never ends inside a group of rows that share one seq, so it can run a little
    /// over). Local-only data never appears: `Scope::Local` prefs, `storage.local`,
    /// unpacked extensions, restore blobs. `since = Seq::ZERO` exports everything (first
    /// sync; the test's state comparison), including rows received from sync and never
    /// edited since, which sit at `Seq::ZERO`.
    pub fn changes_since(&mut self, kind: Kind, since: Seq, limit: usize) -> Result<ChangeBatch, Error> {
        let conn = &self.p.conn;
        match kind {
            Kind::Bookmarks => batch::<BookmarksTable>(conn, since, limit),
            Kind::HistoryPages => batch::<PagesTable>(conn, since, limit),
            Kind::HistoryDeletions => batch::<DeletionsTable>(conn, since, limit),
            Kind::Sessions => batch::<SessionsTable>(conn, since, limit),
            Kind::Extensions => batch::<ExtensionsTable>(conn, since, limit),
            Kind::ExtStorageSync => batch::<StorageTable>(conn, since, limit),
            Kind::Prefs => batch::<PrefsTable>(conn, since, limit),
            Kind::SearchEngines => batch::<EnginesTable>(conn, since, limit),
            Kind::SitePermissions => batch::<SitePermissionsTable>(conn, since, limit),
        }
    }

    /// Merge a batch in one transaction. Records may be of mixed kinds, in any order,
    /// duplicated, or older than what we hold. Per record:
    /// validate -> observe stamps into the HLC -> `apply_one`. After the batch, one
    /// post-step per touched kind: re-materialize bookmarks, refresh history stats and
    /// prune by directives, collect `storage.onChanged` diffs.
    pub fn apply(&mut self, records: Vec<WireRecord>) -> Result<ApplyReport, Error> {
        let mut report = ApplyReport::default();
        let mut effects = Effects::default();
        self.p.write(|tx| {
            for wire in &records {
                apply_wire(tx, wire, &mut report, &mut effects)?;
            }
            for d in &effects.new_directives {
                crate::history::apply_directive(&tx.sql, d)?;
            }
            Ok(())
        })?;
        if !effects.bookmarks.is_empty() {
            for rec in effects.bookmarks {
                self.p.bookmarks.records.insert(rec.id, rec);
            }
            self.p.bookmarks.rematerialize();
            report.changed.bookmarks = true;
        }
        report.changed.ext_storage = effects.ext_storage.into_iter().collect();
        Ok(report)
    }

    /// Opaque key/value storage for the engine (server cursors, tokens), in the
    /// `sync_state` table. It need not be written in the same transaction as `apply`:
    /// re-downloading after a crash is harmless.
    pub fn engine_state(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        Ok(self
            .p
            .conn
            .query_row("SELECT value FROM sync_state WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_engine_state(&mut self, key: &str, value: &[u8]) -> Result<(), Error> {
        self.p.conn.execute(
            "INSERT INTO sync_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Like [`SyncStore::engine_state`], for tokens: sealed with the profile's vault key. A value
    /// that does not open (another key, altered) is an error, never `None`. The first call
    /// in a process may wait on an OS keyring prompt.
    pub fn secret_state(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        let sealed: Option<Vec<u8>> =
            self.p.conn.query_row("SELECT value FROM sync_secrets WHERE key = ?1", [key], |r| r.get(0)).optional()?;
        let Some(sealed) = sealed else {
            return Ok(None);
        };
        let vault_key = self.p.vault_key()?;
        Ok(Some(vault::open(&vault_key, &secret_aad(key), &sealed)?))
    }

    /// An empty `value` removes the key, and needs no vault key, so signing out works with the
    /// keyring locked.
    pub fn set_secret_state(&mut self, key: &str, value: &[u8]) -> Result<(), Error> {
        if value.is_empty() {
            self.p.conn.execute("DELETE FROM sync_secrets WHERE key = ?1", [key])?;
            return Ok(());
        }
        let vault_key = self.p.vault_key()?;
        let sealed = vault::seal(&vault_key, &secret_aad(key), value);
        self.p.conn.execute(
            "INSERT INTO sync_secrets (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, sealed],
        )?;
        Ok(())
    }
}

fn secret_aad(key: &str) -> Vec<u8> {
    format!("sync_secrets:{key}").into_bytes()
}

// ---------------------------------------------------------------------------
// Internal: one impl per kind, one generic apply loop
// ---------------------------------------------------------------------------

/// The per-kind glue between a typed record and its table. Internal: it exists so
/// `apply_one` and `changes_since` are written once rather than once per kind. It is not
/// an extension point.
pub(crate) trait SyncTable {
    const KIND: Kind;
    type Record: Lattice + Serialize + DeserializeOwned;

    fn wire_id(rec: &Self::Record) -> String;
    /// The greatest stamp in the record, observed into the clock before merging.
    fn max_stamp(rec: &Self::Record) -> Option<Stamp>;
    /// Boundary check between an existing local record and an incoming one for the same
    /// id (bookmarks: the kind is immutable). `Err` rejects the incoming record.
    fn compatible(_local: &Self::Record, _incoming: &Self::Record) -> Result<(), &'static str> {
        Ok(())
    }
    /// Kind-specific settling of a merged record before it is compared and stored
    /// (history prunes by deletion directives). `None` means the record ceases to exist.
    fn settle(_tx: &rusqlite::Transaction<'_>, merged: Self::Record) -> Result<Option<Self::Record>, Error> {
        Ok(Some(merged))
    }
    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<Self::Record>, Error>;
    /// Upsert. `seq` is the row's new change sequence: a fresh `tx.seq()` when the row
    /// must be uploaded, `Seq::ZERO` when it holds nothing the server lacks.
    fn store(tx: &rusqlite::Transaction<'_>, rec: &Self::Record, seq: Seq) -> Result<(), Error>;
    /// Only kinds whose `settle` can return `None` need this.
    fn delete(_tx: &rusqlite::Transaction<'_>, _wire_id: &str) -> Result<(), Error> {
        Ok(())
    }
    /// Rows with `seq >= since` in seq order (see [`changed_rows`]), and whether more follow.
    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<ChangedRows<Self::Record>, Error>;
}

/// `(rows oldest first, more)`: one page of a table's changes.
pub(crate) type ChangedRows<R> = (Vec<(Seq, R)>, bool);

pub(crate) enum Applied<R> {
    Unchanged,
    /// Local state changed: `before` is what we held (if anything), `after` the result.
    Merged { before: Option<R>, after: Option<R> },
    Rejected(&'static str),
}

/// ```text
/// merged = local ⊔ incoming            (local = ⊥ if absent)
/// if merged != local    -> store(merged)             local state changed
/// if merged != incoming -> mark dirty (new seq)      the sender/server holds less than we do: re-upload
/// ```
pub(crate) fn apply_one<T: SyncTable>(tx: &mut Tx<'_>, incoming: T::Record) -> Result<Applied<T::Record>, Error> {
    let id = T::wire_id(&incoming);
    let local = T::load(&tx.sql, &id)?;
    let joined = match &local {
        Some(l) => {
            if let Err(reason) = T::compatible(l, &incoming) {
                return Ok(Applied::Rejected(reason));
            }
            let mut m = l.clone();
            m.join(incoming.clone());
            m
        }
        None => incoming.clone(),
    };
    let Some(merged) = T::settle(&tx.sql, joined)? else {
        return Ok(match local {
            Some(_) => {
                T::delete(&tx.sql, &id)?;
                Applied::Merged { before: local, after: None }
            }
            None => Applied::Unchanged,
        });
    };
    let changed = local.as_ref() != Some(&merged);
    let dirty = merged != incoming;
    if !changed && !dirty {
        return Ok(Applied::Unchanged);
    }
    let seq = if dirty { tx.seq() } else { Seq::ZERO };
    T::store(&tx.sql, &merged, seq)?;
    Ok(if changed { Applied::Merged { before: local, after: Some(merged) } } else { Applied::Unchanged })
}

/// Side effects of one batch, gathered inside the transaction and applied after it.
#[derive(Default)]
struct Effects {
    bookmarks: Vec<BookmarkRecord>,
    new_directives: Vec<DeletionDirective>,
    ext_storage: BTreeMap<ExtensionId, Vec<StorageChange>>,
}

fn apply_wire(tx: &mut Tx<'_>, wire: &WireRecord, report: &mut ApplyReport, effects: &mut Effects) -> Result<(), Error> {
    match wire.kind {
        Kind::Bookmarks => {
            if let Some((_, Some(after))) = apply_typed::<BookmarksTable>(tx, wire, report)? {
                effects.bookmarks.push(after);
            }
        }
        Kind::HistoryPages => {
            if apply_typed::<PagesTable>(tx, wire, report)?.is_some() {
                report.changed.history = true;
            }
        }
        Kind::HistoryDeletions => {
            if let Some((_, Some(after))) = apply_typed::<DeletionsTable>(tx, wire, report)? {
                report.changed.history = true;
                effects.new_directives.push(after);
            }
        }
        Kind::Sessions => {
            if apply_typed::<SessionsTable>(tx, wire, report)?.is_some() {
                report.changed.sessions = true;
            }
        }
        Kind::Extensions => {
            if apply_typed::<ExtensionsTable>(tx, wire, report)?.is_some() {
                report.changed.extensions = true;
            }
        }
        Kind::ExtStorageSync => {
            if let Some((before, Some(after))) = apply_typed::<StorageTable>(tx, wire, report)? {
                let change = storage_change(before.as_ref(), &after);
                effects.ext_storage.entry(after.ext.clone()).or_default().push(change);
            }
        }
        Kind::Prefs => {
            if let Some((_, Some(after))) = apply_typed::<PrefsTable>(tx, wire, report)? {
                let PrefRecord { key, .. } = after;
                report.changed.prefs.push(key);
            }
        }
        Kind::SearchEngines => {
            if apply_typed::<EnginesTable>(tx, wire, report)?.is_some() {
                report.changed.search_engines = true;
            }
        }
        Kind::SitePermissions => {
            if apply_typed::<SitePermissionsTable>(tx, wire, report)?.is_some() {
                report.changed.site_permissions = true;
            }
        }
    }
    Ok(())
}

type MergedPair<R> = Option<(Option<R>, Option<R>)>;

/// Boundary: parse and validate the body, check the id, observe stamps, merge. Returns
/// `(before, after)` when local state changed.
fn apply_typed<T: SyncTable>(tx: &mut Tx<'_>, wire: &WireRecord, report: &mut ApplyReport) -> Result<MergedPair<T::Record>, Error> {
    let mut reject = |reason: String| {
        report.rejected.push(Rejected { kind: wire.kind, id: wire.id.clone(), reason });
    };
    let rec: T::Record = match serde_json::from_slice(&wire.body) {
        Ok(r) => r,
        Err(e) => {
            reject(format!("invalid body: {e}"));
            return Ok(None);
        }
    };
    if T::wire_id(&rec) != wire.id {
        reject("body id does not match the record id".to_owned());
        return Ok(None);
    }
    if let Some(s) = T::max_stamp(&rec) {
        tx.observe(s);
    }
    match apply_one::<T>(tx, rec)? {
        Applied::Unchanged => {
            report.unchanged += 1;
            Ok(None)
        }
        Applied::Merged { before, after } => {
            report.merged += 1;
            Ok(Some((before, after)))
        }
        Applied::Rejected(reason) => {
            reject(reason.to_owned());
            Ok(None)
        }
    }
}

fn storage_change(before: Option<&SyncItemRecord>, after: &SyncItemRecord) -> StorageChange {
    StorageChange {
        key: after.key.clone(),
        old_value: before.and_then(|b| b.value.v.as_ref()).map(|j| j.to_value()),
        new_value: after.value.v.as_ref().map(|j| j.to_value()),
    }
}

fn batch<T: SyncTable>(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<ChangeBatch, Error> {
    let (rows, more) = T::changed_since(conn, since, limit)?;
    let upto = rows.last().map_or(since, |(s, _)| Seq(s.0 + 1));
    let records = rows
        .into_iter()
        .map(|(_, rec)| WireRecord {
            kind: T::KIND,
            id: T::wire_id(&rec),
            body: serde_json::to_vec(&rec).expect("records serialize"),
        })
        .collect();
    Ok(ChangeBatch { records, upto, more })
}

/// Shared paging for every `changed_since`: rows with `seq >= since` in seq order, cut
/// at a seq boundary (a transaction that touched many rows gave them all one seq, and a
/// batch must not split them, or the rows past the cut would be skipped forever). The cut
/// moves back to the start of the group it would split; a group larger than `limit` is
/// returned whole.
pub(crate) fn changed_rows<R>(
    conn: &rusqlite::Connection,
    table: &str,
    columns: &str,
    filter: &str,
    since: Seq,
    limit: usize,
    mut map: impl FnMut(&rusqlite::Row<'_>) -> Result<(Seq, R), rusqlite::Error>,
) -> Result<ChangedRows<R>, Error> {
    let probe = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(&format!("SELECT {columns} FROM {table} WHERE {filter} AND seq >= ?1 ORDER BY seq LIMIT ?2"))?;
    let mut rows: Vec<(Seq, R)> = stmt.query_map(params![since.0 as i64, probe], &mut map)?.collect::<Result<_, _>>()?;
    if limit == 0 {
        return Ok((Vec::new(), !rows.is_empty()));
    }
    if rows.len() <= limit {
        return Ok((rows, false));
    }
    let last = rows[limit - 1].0;
    if rows[limit].0 != last {
        rows.truncate(limit);
        return Ok((rows, true));
    }
    let group_start = rows.iter().position(|(s, _)| *s == last).expect("last is in rows");
    if group_start > 0 {
        rows.truncate(group_start);
        return Ok((rows, true));
    }
    let mut stmt =
        conn.prepare(&format!("SELECT {columns} FROM {table} WHERE {filter} AND seq >= ?1 AND seq <= ?2 ORDER BY seq"))?;
    let rows: Vec<(Seq, R)> =
        stmt.query_map(params![since.0 as i64, last.0 as i64], &mut map)?.collect::<Result<_, _>>()?;
    let more: bool = conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {filter} AND seq > ?1)"),
        [last.0 as i64],
        |r| r.get(0),
    )?;
    Ok((rows, more))
}
