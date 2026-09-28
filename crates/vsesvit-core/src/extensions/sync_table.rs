//! Sync glue for the synced desired state (`Kind::Extensions`, table `extensions`).

use rusqlite::{OptionalExtension, params};

use super::{ExtensionId, ExtensionRecord, StoreRef};
use crate::Error;
use crate::crdt::{Lattice, Lww, Seq, Stamp, extra_max_stamp, join_extra};
use crate::db::{extra_col, extra_text, seq_col, stamp_col};
use crate::sync::{Kind, SyncTable, changed_rows};

impl Lattice for ExtensionRecord {
    fn join(&mut self, other: Self) {
        debug_assert_eq!(self.id, other.id, "join is per record");
        self.store.join(other.store);
        self.installed.join(other.installed);
        self.enabled.join(other.enabled);
        join_extra(&mut self.extra, other.extra);
    }
}

const COLUMNS: &str = "id, store, store_at, installed, installed_at, enabled, enabled_at, extra, seq";

fn row_record(row: &rusqlite::Row<'_>) -> Result<(Seq, ExtensionRecord), rusqlite::Error> {
    let id: String = row.get(0)?;
    let id = ExtensionId::parse(&id).map_err(|_| crate::db::bad_column(0, "extension id"))?;
    let store: String = row.get(1)?;
    let store = StoreRef::from_column(&store).ok_or_else(|| crate::db::bad_column(1, "store"))?;
    let rec = ExtensionRecord {
        id,
        store: Lww::new(store, stamp_col(row, 2)?),
        installed: Lww::new(row.get::<_, i64>(3)? != 0, stamp_col(row, 4)?),
        enabled: Lww::new(row.get::<_, i64>(5)? != 0, stamp_col(row, 6)?),
        extra: extra_col(row, 7)?,
    };
    Ok((seq_col(row, 8)?, rec))
}

pub(crate) fn load_record(conn: &rusqlite::Connection, id: &ExtensionId) -> Result<Option<ExtensionRecord>, Error> {
    let rec = conn
        .query_row(&format!("SELECT {COLUMNS} FROM extensions WHERE id = ?1"), [id.as_str()], row_record)
        .optional()?;
    Ok(rec.map(|(_, r)| r))
}

pub(crate) fn store_record(conn: &rusqlite::Connection, rec: &ExtensionRecord, seq: Seq) -> Result<(), Error> {
    conn.execute(
        &format!("INSERT OR REPLACE INTO extensions ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"),
        params![
            rec.id.as_str(),
            rec.store.v.column(),
            rec.store.at.to_vec(),
            i64::from(rec.installed.v),
            rec.installed.at.to_vec(),
            i64::from(rec.enabled.v),
            rec.enabled.at.to_vec(),
            extra_text(&rec.extra),
            seq.0 as i64,
        ],
    )?;
    Ok(())
}

pub(crate) struct ExtensionsTable;

impl SyncTable for ExtensionsTable {
    const KIND: Kind = Kind::Extensions;
    type Record = ExtensionRecord;

    fn wire_id(rec: &ExtensionRecord) -> String {
        rec.id.as_str().to_owned()
    }

    fn max_stamp(rec: &ExtensionRecord) -> Option<Stamp> {
        [rec.store.at, rec.installed.at, rec.enabled.at].into_iter().chain(extra_max_stamp(&rec.extra)).max()
    }

    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<ExtensionRecord>, Error> {
        let Ok(id) = ExtensionId::parse(wire_id) else { return Ok(None) };
        load_record(tx, &id)
    }

    fn store(tx: &rusqlite::Transaction<'_>, rec: &ExtensionRecord, seq: Seq) -> Result<(), Error> {
        store_record(tx, rec, seq)
    }

    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<(Vec<(Seq, ExtensionRecord)>, bool), Error> {
        changed_rows(conn, "extensions", COLUMNS, "1", since, limit, row_record)
    }
}
