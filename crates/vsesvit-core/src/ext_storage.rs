//! `chrome.storage.local` and `chrome.storage.sync` backing for the Linux WebExtensions
//! runtime. (On Windows, WebView2 owns `chrome.storage` inside its own profile, and these
//! tables stay empty there. See DESIGN.md "Open questions".)
//!
//! - `local`: a plain key/value table. No stamps, never synced.
//! - `sync`: one LWW register per `(extension, key)`, holding canonical JSON (`None` =
//!   removed). Per-key last-writer-wins is Chrome's own `storage.sync` semantics.
//!   `clear()` writes `None` for every key this device knows. A key set concurrently on
//!   another device survives, as in Chrome.
//!
//! Quotas (`QUOTA_BYTES`, `QUOTA_BYTES_PER_ITEM`, `MAX_ITEMS`) are enforced on local
//! writes only. A remote apply is never rejected for quota: whether a record fits depends
//! on local state, and rejecting on that basis would let devices diverge.
//!
//! Every write returns the `StorageChange`s the runtime needs to fire
//! `storage.onChanged`. Remote changes arrive in `sync::ApplyReport::changed.ext_storage`.

use std::collections::BTreeMap;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crdt::{JsonText, Lattice, Lww, Seq, Stamp};
use crate::db::{json_col, opt_json_col, seq_col, stamp_col};
use crate::extensions::ExtensionId;
use crate::sync::{Kind, SyncTable, changed_rows};
use crate::{Error, Profile};

pub const SYNC_QUOTA_BYTES: usize = 102_400;
pub const SYNC_QUOTA_BYTES_PER_ITEM: usize = 8_192;
pub const SYNC_MAX_ITEMS: usize = 512;
pub const LOCAL_QUOTA_BYTES: usize = 10_485_760;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Area {
    Local,
    Sync,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StorageChange {
    pub key: String,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("QUOTA_BYTES quota exceeded")]
    QuotaBytes,
    #[error("QUOTA_BYTES_PER_ITEM quota exceeded")]
    QuotaBytesPerItem,
    #[error("MAX_ITEMS quota exceeded")]
    MaxItems,
}

/// Sync record for one `(extension, key)`. The wire id is
/// `"{ext}:{hex(sha256(key))[..32]}"`: stable and bounded length. The body carries the
/// real key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncItemRecord {
    pub ext: ExtensionId,
    pub key: String,
    #[serde(with = "crate::crdt::json_register")]
    pub value: Lww<Option<JsonText>>,
}

impl Lattice for SyncItemRecord {
    fn join(&mut self, other: Self) {
        self.value.join(other.value);
    }
}

/// Chrome's measure: the key length plus the JSON text length of the value.
fn item_bytes(key: &str, value: &JsonText) -> usize {
    key.len() + value.as_str().len()
}

fn key_hash(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

pub struct ExtStorage<'p> {
    pub(crate) p: &'p mut Profile,
}

impl ExtStorage<'_> {
    /// `keys: None` = everything (`storage.get(null)`).
    pub fn get(&mut self, ext: &ExtensionId, area: Area, keys: Option<&[String]>) -> Result<BTreeMap<String, serde_json::Value>, Error> {
        let live = self.live(ext, area)?;
        Ok(live
            .into_iter()
            .filter(|(k, _)| keys.is_none_or(|ks| ks.contains(k)))
            .map(|(k, v)| (k, v.to_value()))
            .collect())
    }

    /// All-or-nothing: quota is checked for the whole batch before anything is written.
    pub fn set(&mut self, ext: &ExtensionId, area: Area, items: BTreeMap<String, serde_json::Value>) -> Result<Vec<StorageChange>, Error> {
        let items: Vec<(String, JsonText)> = items.into_iter().map(|(k, v)| (k, JsonText::from_value(&v))).collect();
        let current = self.live(ext, area)?;
        let mut after = current.clone();
        after.extend(items.iter().cloned());
        check_quota(area, &after)?;
        let changes: Vec<(String, Option<JsonText>, JsonText)> = items
            .into_iter()
            .map(|(k, v)| {
                let old = current.get(&k).cloned();
                (k, old, v)
            })
            .filter(|(_, old, new)| old.as_ref() != Some(new))
            .collect();
        if changes.is_empty() {
            return Ok(Vec::new());
        }
        let writes: Vec<(String, Option<JsonText>)> = changes.iter().map(|(k, _, v)| (k.clone(), Some(v.clone()))).collect();
        self.write(ext, area, writes)?;
        Ok(changes
            .into_iter()
            .map(|(key, old, new)| StorageChange { key, old_value: old.map(|j| j.to_value()), new_value: Some(new.to_value()) })
            .collect())
    }

    pub fn remove(&mut self, ext: &ExtensionId, area: Area, keys: &[String]) -> Result<Vec<StorageChange>, Error> {
        let current = self.live(ext, area)?;
        let gone: Vec<(String, JsonText)> = keys.iter().filter_map(|k| current.get(k).map(|v| (k.clone(), v.clone()))).collect();
        if gone.is_empty() {
            return Ok(Vec::new());
        }
        self.write(ext, area, gone.iter().map(|(k, _)| (k.clone(), None)).collect())?;
        Ok(gone.into_iter().map(|(key, old)| StorageChange { key, old_value: Some(old.to_value()), new_value: None }).collect())
    }

    pub fn clear(&mut self, ext: &ExtensionId, area: Area) -> Result<Vec<StorageChange>, Error> {
        let keys: Vec<String> = self.live(ext, area)?.into_keys().collect();
        self.remove(ext, area, &keys)
    }

    pub fn bytes_in_use(&mut self, ext: &ExtensionId, area: Area, keys: Option<&[String]>) -> Result<usize, Error> {
        Ok(self
            .live(ext, area)?
            .iter()
            .filter(|(k, _)| keys.is_none_or(|ks| ks.contains(k)))
            .map(|(k, v)| item_bytes(k, v))
            .sum())
    }

    /// Every present key of one area, as canonical JSON.
    fn live(&mut self, ext: &ExtensionId, area: Area) -> Result<BTreeMap<String, JsonText>, Error> {
        let sql = match area {
            Area::Local => "SELECT key, value FROM ext_storage_local WHERE ext = ?1",
            Area::Sync => "SELECT key, value FROM ext_storage_sync WHERE ext = ?1 AND value IS NOT NULL",
        };
        let mut stmt = self.p.conn.prepare_cached(sql)?;
        let rows = stmt.query_map([ext.as_str()], |row| Ok((row.get::<_, String>(0)?, json_col(row, 1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// One transaction. `None` removes (local) or tombstones (sync) the key.
    fn write(&mut self, ext: &ExtensionId, area: Area, items: Vec<(String, Option<JsonText>)>) -> Result<(), Error> {
        let ext = ext.clone();
        self.p.write(|tx| {
            match area {
                Area::Local => {
                    for (key, value) in items {
                        match value {
                            Some(v) => tx.sql.execute(
                                "INSERT OR REPLACE INTO ext_storage_local (ext, key, value) VALUES (?1, ?2, ?3)",
                                params![ext.as_str(), key, v.as_str()],
                            )?,
                            None => tx.sql.execute(
                                "DELETE FROM ext_storage_local WHERE ext = ?1 AND key = ?2",
                                params![ext.as_str(), key],
                            )?,
                        };
                    }
                }
                Area::Sync => {
                    let at = tx.stamp();
                    let seq = tx.seq();
                    for (key, value) in items {
                        let mut rec = load_item(&tx.sql, &ext, &key)?
                            .unwrap_or(SyncItemRecord { ext: ext.clone(), key: key.clone(), value: Lww::new(None, Stamp::ZERO) });
                        if rec.value.set(value, at) {
                            store_item(&tx.sql, &rec, seq)?;
                        }
                    }
                }
            }
            Ok(())
        })
    }
}

fn check_quota(area: Area, after: &BTreeMap<String, JsonText>) -> Result<(), StorageError> {
    let total: usize = after.iter().map(|(k, v)| item_bytes(k, v)).sum();
    match area {
        Area::Local => {
            if total > LOCAL_QUOTA_BYTES {
                return Err(StorageError::QuotaBytes);
            }
        }
        Area::Sync => {
            if after.iter().any(|(k, v)| item_bytes(k, v) > SYNC_QUOTA_BYTES_PER_ITEM) {
                return Err(StorageError::QuotaBytesPerItem);
            }
            if after.len() > SYNC_MAX_ITEMS {
                return Err(StorageError::MaxItems);
            }
            if total > SYNC_QUOTA_BYTES {
                return Err(StorageError::QuotaBytes);
            }
        }
    }
    Ok(())
}

const COLUMNS: &str = "ext, key, value, value_at, seq";

fn row_item(row: &rusqlite::Row<'_>) -> Result<(Seq, SyncItemRecord), rusqlite::Error> {
    let ext: String = row.get(0)?;
    let ext = ExtensionId::parse(&ext).map_err(|_| crate::db::bad_column(0, "extension id"))?;
    let key: String = row.get(1)?;
    let value = opt_json_col(row, 2)?;
    let at = stamp_col(row, 3)?;
    Ok((seq_col(row, 4)?, SyncItemRecord { ext, key, value: Lww::new(value, at) }))
}

fn load_item(conn: &rusqlite::Connection, ext: &ExtensionId, key: &str) -> Result<Option<SyncItemRecord>, Error> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM ext_storage_sync WHERE ext = ?1 AND key = ?2"))?;
    let mut rows = stmt.query_map(params![ext.as_str(), key], row_item)?;
    Ok(rows.next().transpose()?.map(|(_, r)| r))
}

fn store_item(conn: &rusqlite::Connection, rec: &SyncItemRecord, seq: Seq) -> Result<(), Error> {
    conn.execute(
        &format!("INSERT OR REPLACE INTO ext_storage_sync ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5)"),
        params![rec.ext.as_str(), rec.key, rec.value.v.as_ref().map(JsonText::as_str), rec.value.at.to_vec(), seq.0 as i64],
    )?;
    Ok(())
}

pub(crate) struct StorageTable;

impl SyncTable for StorageTable {
    const KIND: Kind = Kind::ExtStorageSync;
    type Record = SyncItemRecord;

    fn wire_id(rec: &SyncItemRecord) -> String {
        format!("{}:{}", rec.ext.as_str(), key_hash(&rec.key))
    }

    fn max_stamp(rec: &SyncItemRecord) -> Option<Stamp> {
        Some(rec.value.at)
    }

    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<SyncItemRecord>, Error> {
        let Some((ext, hash)) = wire_id.split_once(':') else { return Ok(None) };
        let Ok(ext) = ExtensionId::parse(ext) else { return Ok(None) };
        let mut stmt = tx.prepare_cached(&format!("SELECT {COLUMNS} FROM ext_storage_sync WHERE ext = ?1"))?;
        let rows = stmt.query_map([ext.as_str()], row_item)?;
        for r in rows {
            let (_, rec) = r?;
            if key_hash(&rec.key) == hash {
                return Ok(Some(rec));
            }
        }
        Ok(None)
    }

    /// By primary key: [`load`](Self::load) hashes every key of the extension, and a remote
    /// batch (not bound by the item quota) would make that quadratic.
    fn load_for(tx: &rusqlite::Transaction<'_>, incoming: &SyncItemRecord) -> Result<Option<SyncItemRecord>, Error> {
        load_item(tx, &incoming.ext, &incoming.key)
    }

    fn store(tx: &rusqlite::Transaction<'_>, rec: &SyncItemRecord, seq: Seq) -> Result<(), Error> {
        store_item(tx, rec, seq)
    }

    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<(Vec<(Seq, SyncItemRecord)>, bool), Error> {
        changed_rows(conn, "ext_storage_sync", COLUMNS, "1", since, limit, row_item)
    }
}
