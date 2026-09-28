//! Typed preferences.
//!
//! One LWW register per key, holding canonical JSON (`None` = reset to default). Keys
//! are declared in code as typed [`Pref<T>`] constants, so callers never handle strings
//! or JSON. Each pref declares whether it syncs. Keys this build does not know (written
//! by a newer build on another device) are stored and re-synced untouched.
//!
//! A stored value that no longer decodes as `T` (a newer build changed its shape) reads
//! as the default. That is boundary validation at read time; the stored value is left
//! alone for the newer build.

use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::crdt::{JsonText, Lattice, Lww, Seq, Stamp};
use crate::db::{seq_col, stamp_col};
use crate::search::SearchEngineId;
use crate::sync::{Kind, SyncTable, changed_rows};
use crate::{Error, Profile};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Synced,
    /// Stored with stamps like everything else but never exported (window geometry,
    /// device name, download dir).
    Local,
}

pub struct Pref<T: 'static> {
    pub key: &'static str,
    pub scope: Scope,
    pub default: fn() -> T,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// Where the tab list lives. Vertical tabs (a sidebar) are the default.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabsPosition {
    Left,
    Right,
    Top,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Startup {
    RestoreSession,
    Homepage,
    NewTab,
}

pub mod keys {
    use super::*;

    pub const HOMEPAGE: Pref<String> = Pref { key: "homepage", scope: Scope::Synced, default: || "about:home".to_owned() };
    /// Exactly one default engine: a single register, not an `is_default` flag on each
    /// engine record (which two devices could set on two different engines).
    pub const DEFAULT_SEARCH_ENGINE: Pref<SearchEngineId> =
        Pref { key: "search.default", scope: Scope::Synced, default: SearchEngineId::builtin_default };
    pub const THEME: Pref<Theme> = Pref { key: "theme", scope: Scope::Synced, default: || Theme::System };
    pub const STARTUP: Pref<Startup> = Pref { key: "startup", scope: Scope::Synced, default: || Startup::RestoreSession };
    pub const TABS_POSITION: Pref<TabsPosition> = Pref { key: "tabs.position", scope: Scope::Synced, default: || TabsPosition::Left };
    pub const SHOW_BOOKMARKS_BAR: Pref<bool> = Pref { key: "bookmarks_bar.visible", scope: Scope::Synced, default: || true };
    pub const DEVICE_NAME: Pref<String> = Pref { key: "device.name", scope: Scope::Local, default: || String::new() };
    /// Local: whether an installation checks for and downloads updates is a property of that
    /// installation, not of the user's other devices.
    pub const UPDATES_AUTOMATIC: Pref<bool> = Pref { key: "updates.automatic", scope: Scope::Local, default: || true };
    /// Local: a folder on this device's disk. `None` = the platform's Downloads folder, which
    /// only the shell knows.
    pub const DOWNLOADS_DIR: Pref<Option<PathBuf>> = Pref { key: "downloads.directory", scope: Scope::Local, default: || None };
    /// Whether each download opens a save dialog instead of going straight to [`DOWNLOADS_DIR`].
    pub const DOWNLOADS_ASK: Pref<bool> = Pref { key: "downloads.ask", scope: Scope::Synced, default: || false };
}

/// Sync record: one per key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefRecord {
    pub key: String,
    #[serde(with = "crate::crdt::json_register")]
    pub value: Lww<Option<JsonText>>,
}

impl Lattice for PrefRecord {
    fn join(&mut self, other: Self) {
        self.value.join(other.value);
    }
}

pub struct Prefs<'p> {
    pub(crate) p: &'p mut Profile,
}

impl Prefs<'_> {
    /// Primary-key lookup. Falls back to `default()` if the key is missing, reset, or undecodable.
    pub fn get<T: DeserializeOwned>(&mut self, pref: &Pref<T>) -> T {
        let stored: Option<Option<String>> = self
            .p
            .conn
            .query_row("SELECT value FROM prefs WHERE key = ?1", [pref.key], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        stored
            .flatten()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| (pref.default)())
    }

    /// No-op (no stamp) if the canonical JSON is unchanged. Setting the default value
    /// explicitly stores it, so the choice syncs.
    pub fn set<T: Serialize>(&mut self, pref: &Pref<T>, value: &T) -> Result<(), Error> {
        let json = serde_json::to_value(value).map_err(|e| Error::Io(std::io::Error::other(e)))?;
        self.write(pref.key, pref.scope, Some(JsonText::from_value(&json)))
    }

    /// Writes `None`: "use the default", which also syncs.
    pub fn reset<T>(&mut self, pref: &Pref<T>) -> Result<(), Error> {
        self.write(pref.key, pref.scope, None)
    }

    fn write(&mut self, key: &str, scope: Scope, value: Option<JsonText>) -> Result<(), Error> {
        self.p.write(|tx| {
            let mut rec = load_record(&tx.sql, key)?
                .map(|(r, _)| r)
                .unwrap_or(PrefRecord { key: key.to_owned(), value: Lww::new(None, Stamp::ZERO) });
            if rec.value.v == value {
                return Ok(());
            }
            let at = tx.stamp();
            rec.value.set(value, at);
            let seq = tx.seq();
            store_record(&tx.sql, &rec, seq, Some(scope == Scope::Synced))
        })
    }
}

const COLUMNS: &str = "key, value, value_at, synced, seq";

fn row_record(row: &rusqlite::Row<'_>) -> Result<(Seq, (PrefRecord, bool)), rusqlite::Error> {
    let key: String = row.get(0)?;
    let value: Option<String> = row.get(1)?;
    let value = match value {
        Some(text) => Some(JsonText::parse(&text).ok_or_else(|| crate::db::bad_column(1, "json"))?),
        None => None,
    };
    let at = stamp_col(row, 2)?;
    let synced: i64 = row.get(3)?;
    Ok((seq_col(row, 4)?, (PrefRecord { key, value: Lww::new(value, at) }, synced == 1)))
}

fn load_record(conn: &rusqlite::Connection, key: &str) -> Result<Option<(PrefRecord, bool)>, Error> {
    let rec = conn.query_row(&format!("SELECT {COLUMNS} FROM prefs WHERE key = ?1"), [key], row_record).optional()?;
    Ok(rec.map(|(_, r)| r))
}

/// `synced: None` keeps an existing row's flag (remote writes never change scope) and
/// defaults a new row to synced.
fn store_record(conn: &rusqlite::Connection, rec: &PrefRecord, seq: Seq, synced: Option<bool>) -> Result<(), Error> {
    let flag = synced.map(i64::from);
    conn.execute(
        &format!(
            "INSERT INTO prefs ({COLUMNS}) VALUES (?1, ?2, ?3, COALESCE(?4, 1), ?5) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, value_at = excluded.value_at, \
             synced = COALESCE(?4, prefs.synced), seq = excluded.seq"
        ),
        params![rec.key, rec.value.v.as_ref().map(JsonText::as_str), rec.value.at.to_vec(), flag, seq.0 as i64],
    )?;
    Ok(())
}

pub(crate) struct PrefsTable;

impl SyncTable for PrefsTable {
    const KIND: Kind = Kind::Prefs;
    type Record = PrefRecord;

    fn wire_id(rec: &PrefRecord) -> String {
        rec.key.clone()
    }

    fn max_stamp(rec: &PrefRecord) -> Option<Stamp> {
        Some(rec.value.at)
    }

    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<PrefRecord>, Error> {
        Ok(load_record(tx, wire_id)?.map(|(r, _)| r))
    }

    fn store(tx: &rusqlite::Transaction<'_>, rec: &PrefRecord, seq: Seq) -> Result<(), Error> {
        store_record(tx, rec, seq, None)
    }

    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<(Vec<(Seq, PrefRecord)>, bool), Error> {
        let (rows, more) = changed_rows(conn, "prefs", COLUMNS, "synced = 1", since, limit, row_record)?;
        Ok((rows.into_iter().map(|(s, (r, _))| (s, r)).collect(), more))
    }
}
