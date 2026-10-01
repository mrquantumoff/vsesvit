//! SQLite plumbing: pragmas, migrations, and the one transaction type every mutation uses.
//!
//! Durability: WAL + `synchronous=NORMAL`. Every public mutation is one transaction, so
//! a crash mid-write leaves the previous committed state (atomic, never corrupt). An
//! application crash loses nothing that was committed. A power loss can drop the last
//! few committed transactions, which is acceptable for browser data and keeps the fsync
//! off the UI thread's per-navigation path.

use std::path::Path;

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, OptionalExtension, params};

use crate::crdt::{Clock, DeviceId, Hlc, Seq, Stamp};
use crate::{Error, OpenError};

pub(crate) const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;
pub(crate) const SCHEMA_V1: &str = include_str!("schema.sql");
/// Extension tables, owned by `extensions`. Applied after `SCHEMA_V1` in the same transaction.
pub(crate) const SCHEMA_V1_EXTENSIONS: &str = include_str!("extensions/schema.sql");
/// Rebuilds both extension tables so their store CHECKs accept `edge_addons`.
pub(crate) const SCHEMA_V4_EXTENSIONS: &str = include_str!("extensions/schema_v4.sql");

/// Migration `i` brings a profile to `user_version = i + 1`, running its scripts in order in
/// one transaction. Append only.
const MIGRATIONS: &[&[&str]] = &[
    &[SCHEMA_V1, SCHEMA_V1_EXTENSIONS],
    &[crate::favicons::SCHEMA],
    &[crate::downloads::SCHEMA],
    &[SCHEMA_V4_EXTENSIONS],
    &[crate::favicons::FAILURES_SCHEMA],
    &[crate::permissions::SCHEMA],
    &[crate::zoom::SCHEMA],
    &[crate::vault::SCHEMA, crate::sync::SECRETS_SCHEMA],
];

/// `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=0`
/// (a single process holds the profile, so contention is a bug, not a wait), and
/// `unicode_lower(text)`: Rust's lowercasing, as SQLite's `lower` and `LIKE` fold only ASCII.
pub(crate) fn open(path: &Path) -> Result<Connection, OpenError> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::ZERO)?;
    let mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(OpenError::Io(std::io::Error::other(format!("journal_mode is {mode}, not WAL"))));
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.create_scalar_function("unicode_lower", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
        Ok(ctx.get::<String>(0)?.to_lowercase())
    })?;
    Ok(conn)
}

/// Forward-only migrations keyed on `PRAGMA user_version`, each in its own transaction.
/// A database newer than this build fails with `OpenError::TooNew` and is never written.
pub(crate) fn migrate(conn: &mut Connection) -> Result<(), OpenError> {
    let found: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if found > SCHEMA_VERSION {
        return Err(OpenError::TooNew { found, supported: SCHEMA_VERSION });
    }
    for (version, scripts) in (1..).zip(MIGRATIONS) {
        if found < version {
            let tx = conn.transaction()?;
            for sql in *scripts {
                tx.execute_batch(sql)?;
            }
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
        }
    }
    Ok(())
}

/// `meta` rows: `device_id`, `clock_last`, `next_seq`, `created_ms`.
pub(crate) struct Meta {
    pub device: DeviceId,
    pub clock_last: Hlc,
    pub next_seq: u64,
    /// This call inserted the rows: the profile did not exist before.
    pub created: bool,
}

fn meta_get(conn: &Connection, key: &str) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()
}

pub(crate) fn load_or_init_meta(
    conn: &mut Connection,
    new_device: Option<DeviceId>,
    now_ms: u64,
) -> Result<Meta, OpenError> {
    if let Some(device) = meta_get(conn, "device_id")? {
        let clock_last = meta_get(conn, "clock_last")?.unwrap_or(0);
        let next_seq = meta_get(conn, "next_seq")?.unwrap_or(1);
        return Ok(Meta {
            device: DeviceId(device as u64),
            clock_last: Hlc(clock_last as u64),
            next_seq: (next_seq as u64).max(1),
            created: false,
        });
    }
    let device = match new_device {
        Some(d) if d.0 != 0 => d,
        _ => DeviceId::random(),
    };
    let tx = conn.transaction()?;
    for (key, value) in [
        ("device_id", device.0 as i64),
        ("clock_last", 0i64),
        ("next_seq", 1i64),
        ("created_ms", now_ms as i64),
    ] {
        tx.execute("INSERT INTO meta (key, value) VALUES (?1, ?2)", params![key, value])?;
    }
    tx.commit()?;
    Ok(Meta { device, clock_last: Hlc::ZERO, next_seq: 1, created: true })
}

/// The only way core writes. One transaction, at most one stamp, at most one seq.
/// `commit` writes `clock_last` and `next_seq` back to `meta` before committing.
pub(crate) struct Tx<'a> {
    pub(crate) sql: rusqlite::Transaction<'a>,
    pub(crate) clock: &'a mut Clock,
    pub(crate) device: DeviceId,
    pub(crate) next_seq: &'a mut u64,
    clock_at_start: Hlc,
    stamp: Option<Stamp>,
    seq: Option<Seq>,
}

impl<'a> Tx<'a> {
    pub(crate) fn begin(
        conn: &'a mut Connection,
        clock: &'a mut Clock,
        device: DeviceId,
        next_seq: &'a mut u64,
    ) -> Result<Tx<'a>, Error> {
        let sql = conn.transaction()?;
        let clock_at_start = clock.last();
        Ok(Tx { sql, clock, device, next_seq, clock_at_start, stamp: None, seq: None })
    }

    /// The stamp for every field this transaction writes. Minted lazily, so a
    /// transaction that turns out to be a no-op mints nothing.
    pub(crate) fn stamp(&mut self) -> Stamp {
        match self.stamp {
            Some(s) => s,
            None => {
                let s = Stamp { hlc: self.clock.tick(), device: self.device };
                self.stamp = Some(s);
                s
            }
        }
    }

    /// The change-sequence value for every row this transaction marks dirty.
    pub(crate) fn seq(&mut self) -> Seq {
        match self.seq {
            Some(s) => s,
            None => {
                let s = Seq(*self.next_seq);
                *self.next_seq += 1;
                self.seq = Some(s);
                s
            }
        }
    }

    /// Wall time as this transaction sees it (for visit times, `added_ms`, stats).
    pub(crate) fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    /// Record a remote stamp so later local stamps order after it.
    pub(crate) fn observe(&mut self, seen: Stamp) {
        self.clock.observe(seen.hlc);
    }

    pub(crate) fn commit(self) -> Result<(), Error> {
        if self.clock.last() != self.clock_at_start {
            self.sql.execute(
                "UPDATE meta SET value = ?1 WHERE key = 'clock_last'",
                [self.clock.last().0 as i64],
            )?;
        }
        if self.seq.is_some() {
            self.sql.execute("UPDATE meta SET value = ?1 WHERE key = 'next_seq'", [*self.next_seq as i64])?;
        }
        self.sql.commit()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Column helpers shared by every table module
// ---------------------------------------------------------------------------

pub(crate) fn stamp_col(row: &rusqlite::Row<'_>, idx: usize) -> Result<Stamp, rusqlite::Error> {
    let bytes: Vec<u8> = row.get(idx)?;
    Stamp::from_slice(&bytes).map_err(|_| bad_column(idx, "stamp"))
}

pub(crate) fn opt_stamp_col(row: &rusqlite::Row<'_>, idx: usize) -> Result<Option<Stamp>, rusqlite::Error> {
    let bytes: Option<Vec<u8>> = row.get(idx)?;
    bytes.map(|b| Stamp::from_slice(&b).map_err(|_| bad_column(idx, "stamp"))).transpose()
}

pub(crate) fn seq_col(row: &rusqlite::Row<'_>, idx: usize) -> Result<Seq, rusqlite::Error> {
    let v: i64 = row.get(idx)?;
    Ok(Seq(v as u64))
}

pub(crate) fn uuid_col(row: &rusqlite::Row<'_>, idx: usize) -> Result<uuid::Uuid, rusqlite::Error> {
    let bytes: Vec<u8> = row.get(idx)?;
    uuid::Uuid::from_slice(&bytes).map_err(|_| bad_column(idx, "uuid"))
}

pub(crate) fn extra_col(row: &rusqlite::Row<'_>, idx: usize) -> Result<crate::crdt::Extra, rusqlite::Error> {
    let text: String = row.get(idx)?;
    serde_json::from_str(&text).map_err(|_| bad_column(idx, "extra"))
}

pub(crate) fn extra_text(extra: &crate::crdt::Extra) -> String {
    serde_json::to_string(extra).expect("Extra serializes")
}

pub(crate) fn bad_column(idx: usize, what: &'static str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        idx,
        rusqlite::types::Type::Blob,
        Box::new(std::io::Error::other(format!("malformed {what} column"))),
    )
}
