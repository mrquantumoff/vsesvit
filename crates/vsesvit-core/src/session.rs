//! Open windows and tabs.
//!
//! Per-actor state. Each device writes exactly one record, keyed by its own
//! `DeviceId`, and only reads the others. With one writer per record there are no
//! conflicts to resolve: the record is a single `Lww<Option<SessionSnapshot>>`
//! (`None` = device forgotten).
//!
//! This device's record is also its crash-restore state, so there is one source of
//! truth. The engine's opaque back/forward blob per tab (WebKit
//! `webkit_web_view_get_session_state`) is local-only and lives in `tab_restore_state`,
//! never on the wire, and never in record equality: a save whose only difference is a
//! restore blob mints no stamp.

use std::cmp::Ordering;
use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crdt::{DeviceId, Lattice, Lww, Seq, Stamp};
use crate::db::{seq_col, stamp_col, uuid_col};
use crate::sync::{Kind, SyncTable, changed_rows};
use crate::{Error, Profile, Url};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TabId(pub Uuid);

impl TabId {
    pub fn new() -> Self {
        TabId(Uuid::new_v4())
    }
}

impl Default for TabId {
    fn default() -> Self {
        TabId::new()
    }
}

/// What the shell hands over and gets back on restore. "Which tab is active" is an index
/// (one value), not a flag on each tab (which could end up set on two tabs).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub device_name: String,
    pub windows: Vec<WindowSnapshot>,
    pub active_window: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub tabs: Vec<TabSnapshot>,
    pub active_tab: usize,
    /// Restore-only. Other devices ignore it.
    pub bounds: Option<(i32, i32, u32, u32)>,
    pub maximized: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabSnapshot {
    pub id: TabId,
    pub url: Url,
    pub title: String,
    pub pinned: bool,
    pub last_active_ms: i64,
    /// Engine back/forward state. Stored in `tab_restore_state`, never serialized, and
    /// not part of equality or ordering.
    #[serde(skip)]
    pub restore_state: Option<Vec<u8>>,
}

impl TabSnapshot {
    fn key(&self) -> (TabId, &Url, &str, bool, i64) {
        (self.id, &self.url, &self.title, self.pinned, self.last_active_ms)
    }
}

impl PartialEq for TabSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for TabSnapshot {}

impl PartialOrd for TabSnapshot {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TabSnapshot {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSessionRecord {
    pub device: DeviceId,
    pub session: Lww<Option<SessionSnapshot>>,
}

impl Lattice for DeviceSessionRecord {
    fn join(&mut self, other: Self) {
        self.session.join(other.session);
    }
}

/// Another device's tabs, for a "tabs from other devices" menu.
#[derive(Clone, Debug)]
pub struct DeviceTabs {
    pub device: DeviceId,
    pub device_name: String,
    pub updated_ms: i64,
    pub windows: Vec<WindowSnapshot>,
}

const OTHER_DEVICE_MAX_AGE_MS: u64 = 14 * 24 * 60 * 60 * 1000;

pub struct Session<'p> {
    pub(crate) p: &'p mut Profile,
}

impl Session<'_> {
    /// The shell calls this debounced (about 2 s after any tab change) and on clean
    /// shutdown. One transaction writes the own `device_sessions` row (new stamp and seq
    /// only if the snapshot changed) and replaces `tab_restore_state` for the listed tabs.
    pub fn save(&mut self, snapshot: &SessionSnapshot) -> Result<(), Error> {
        let device = self.p.device;
        self.p.write(|tx| {
            let mut rec = load_record(&tx.sql, device)?
                .unwrap_or(DeviceSessionRecord { device, session: Lww::new(None, Stamp::ZERO) });
            if rec.session.v.as_ref() != Some(snapshot) {
                let at = tx.stamp();
                rec.session.set(Some(snapshot.clone()), at);
                let seq = tx.seq();
                store_record(&tx.sql, &rec, seq)?;
            }
            let wanted: HashMap<TabId, &[u8]> = snapshot
                .windows
                .iter()
                .flat_map(|w| &w.tabs)
                .filter_map(|t| t.restore_state.as_deref().map(|s| (t.id, s)))
                .collect();
            let current = load_restore_state(&tx.sql)?;
            let same = current.len() == wanted.len()
                && wanted.iter().all(|(id, s)| current.get(id).is_some_and(|c| c.as_slice() == *s));
            if !same {
                tx.sql.execute("DELETE FROM tab_restore_state", [])?;
                let mut ins = tx.sql.prepare_cached("INSERT INTO tab_restore_state (tab_id, state) VALUES (?1, ?2)")?;
                for (id, state) in wanted {
                    ins.execute(params![id.0.as_bytes().as_slice(), state])?;
                }
            }
            Ok(())
        })
    }

    /// At startup: the last saved snapshot with restore blobs re-attached.
    pub fn restore(&mut self) -> Result<Option<SessionSnapshot>, Error> {
        let Some(rec) = load_record(&self.p.conn, self.p.device)? else { return Ok(None) };
        let Some(mut snapshot) = rec.session.v else { return Ok(None) };
        let mut blobs = load_restore_state(&self.p.conn)?;
        for tab in snapshot.windows.iter_mut().flat_map(|w| w.tabs.iter_mut()) {
            tab.restore_state = blobs.remove(&tab.id);
        }
        Ok(Some(snapshot))
    }

    /// Other devices' sessions updated within the last 14 days, newest first. Staleness
    /// is derived at read time. There is no expiry write to conflict on.
    pub fn other_devices(&mut self) -> Result<Vec<DeviceTabs>, Error> {
        let now = self.p.clock.now_ms();
        let own = self.p.device;
        let mut stmt = self.p.conn.prepare_cached(
            "SELECT device, snapshot, snapshot_at, seq FROM device_sessions WHERE device <> ?1 AND snapshot IS NOT NULL",
        )?;
        let rows = stmt.query_map([own.0 as i64], row_record)?;
        let mut out = Vec::new();
        for r in rows {
            let (_, rec) = r?;
            let updated = rec.session.at.hlc.wall_ms();
            if updated + OTHER_DEVICE_MAX_AGE_MS < now {
                continue;
            }
            if let Some(s) = rec.session.v {
                out.push(DeviceTabs { device: rec.device, device_name: s.device_name, updated_ms: updated as i64, windows: s.windows });
            }
        }
        out.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then(a.device.cmp(&b.device)));
        Ok(out)
    }
}

fn load_restore_state(conn: &rusqlite::Connection) -> Result<HashMap<TabId, Vec<u8>>, Error> {
    let mut stmt = conn.prepare_cached("SELECT tab_id, state FROM tab_restore_state")?;
    let rows = stmt.query_map([], |row| Ok((TabId(uuid_col(row, 0)?), row.get::<_, Vec<u8>>(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

const COLUMNS: &str = "device, snapshot, snapshot_at, seq";

fn row_record(row: &rusqlite::Row<'_>) -> Result<(Seq, DeviceSessionRecord), rusqlite::Error> {
    let device = DeviceId(row.get::<_, i64>(0)? as u64);
    let snapshot: Option<String> = row.get(1)?;
    let snapshot = match snapshot {
        Some(text) => Some(serde_json::from_str(&text).map_err(|_| crate::db::bad_column(1, "snapshot"))?),
        None => None,
    };
    let at = stamp_col(row, 2)?;
    Ok((seq_col(row, 3)?, DeviceSessionRecord { device, session: Lww::new(snapshot, at) }))
}

fn load_record(conn: &rusqlite::Connection, device: DeviceId) -> Result<Option<DeviceSessionRecord>, Error> {
    let rec = conn
        .query_row(&format!("SELECT {COLUMNS} FROM device_sessions WHERE device = ?1"), [device.0 as i64], row_record)
        .optional()?;
    Ok(rec.map(|(_, r)| r))
}

fn store_record(conn: &rusqlite::Connection, rec: &DeviceSessionRecord, seq: Seq) -> Result<(), Error> {
    let snapshot = rec.session.v.as_ref().map(|s| serde_json::to_string(s).expect("snapshot serializes"));
    conn.execute(
        &format!("INSERT OR REPLACE INTO device_sessions ({COLUMNS}) VALUES (?1, ?2, ?3, ?4)"),
        params![rec.device.0 as i64, snapshot, rec.session.at.to_vec(), seq.0 as i64],
    )?;
    Ok(())
}

pub(crate) struct SessionsTable;

impl SyncTable for SessionsTable {
    const KIND: Kind = Kind::Sessions;
    type Record = DeviceSessionRecord;

    fn wire_id(rec: &DeviceSessionRecord) -> String {
        rec.device.to_hex()
    }

    fn max_stamp(rec: &DeviceSessionRecord) -> Option<Stamp> {
        Some(rec.session.at)
    }

    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<DeviceSessionRecord>, Error> {
        let Ok(device) = u64::from_str_radix(wire_id, 16) else { return Ok(None) };
        load_record(tx, DeviceId(device))
    }

    fn store(tx: &rusqlite::Transaction<'_>, rec: &DeviceSessionRecord, seq: Seq) -> Result<(), Error> {
        store_record(tx, rec, seq)
    }

    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<(Vec<(Seq, DeviceSessionRecord)>, bool), Error> {
        changed_rows(conn, "device_sessions", COLUMNS, "1", since, limit, row_record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_equality_ignores_restore_state() {
        let url = Url::parse("https://a.example/").unwrap();
        let a = TabSnapshot { id: TabId(Uuid::from_u128(7)), url: url.clone(), title: "t".into(), pinned: false, last_active_ms: 1, restore_state: None };
        let b = TabSnapshot { restore_state: Some(vec![1, 2, 3]), ..a.clone() };
        assert_eq!(a, b);
        assert_eq!(a.cmp(&b), Ordering::Equal);
        let c = TabSnapshot { title: "u".into(), ..a.clone() };
        assert_ne!(a, c);
        assert!(serde_json::to_string(&b).unwrap().contains("\"title\":\"t\""));
        assert!(!serde_json::to_string(&b).unwrap().contains("restore_state"));
    }

    #[test]
    fn restore_state_rows_need_a_whole_tab_id() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE tab_restore_state (tab_id BLOB PRIMARY KEY, state BLOB NOT NULL)", []).unwrap();
        let id = TabId(Uuid::from_u128(7));
        conn.execute("INSERT INTO tab_restore_state VALUES (?1, ?2)", params![id.0.as_bytes().to_vec(), vec![9u8]]).unwrap();
        assert_eq!(load_restore_state(&conn).unwrap(), HashMap::from([(id, vec![9u8])]));
        conn.execute("INSERT INTO tab_restore_state VALUES (?1, ?2)", params![vec![1u8, 2, 3], vec![9u8]]).unwrap();
        assert!(load_restore_state(&conn).is_err());
    }
}
