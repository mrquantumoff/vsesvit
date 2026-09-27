//! History.
//!
//! Two synced kinds, both grow-only:
//!
//! - [`PageRecord`], keyed by URL (a natural key, so two devices visiting the same page
//!   merge into one record): an LWW title plus a grow-only set of [`Visit`]s. Visits are
//!   facts with wall-clock times, not edits, so they are never stamped.
//! - [`DeletionDirective`]: an immutable record "forget visits to `url` (or all urls) in
//!   `[from_ms, to_ms]`". Directives form a grow-only set.
//!
//! The effective history is `union(visits) - covered_by(union(directives))`, collapsed to
//! one visit per `(at_ms, device)` and capped to the newest [`MAX_VISITS`] per page. That
//! is a deterministic function of the two unions, so it converges. Deletion propagates as a directive, and a visit made later
//! on another device (outside the range) correctly survives. "Clear all history" is one
//! directive with `url: None`. There is no retention cutoff: history stays until the
//! user clears it (a per-device cutoff inside the join would make devices echo forever).
//!
//! [`normalize`] prunes *before* it caps. Capping the raw union first would let a stale
//! record full of since-deleted visits push live ones out of the top 64.
//!
//! Local tables also carry derived per-page stats (visit_count, typed_count,
//! last_visit_ms, frecency) for omnibox ranking. Exactly one function,
//! `refresh_stats(url)`, writes them. It is called from every page store.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crdt::{DeviceId, Extra, Lattice, Lww, Seq, Stamp, extra_max_stamp, join_extra};
use crate::db::{extra_col, extra_text, seq_col, stamp_col, uuid_col};
use crate::sync::{Kind, SyncTable, changed_rows};
use crate::{Error, Profile, Url};

/// A page record keeps only its newest visits. The top 64 of a union depends only on the
/// top 64 of each side, so the cap keeps the join a semilattice and bounds record size.
pub const MAX_VISITS: usize = 64;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    Link,
    Typed,
    Bookmark,
    Reload,
    Redirect,
    FormSubmit,
}

impl Transition {
    fn code(self) -> i64 {
        match self {
            Transition::Link => 0,
            Transition::Typed => 1,
            Transition::Bookmark => 2,
            Transition::Reload => 3,
            Transition::Redirect => 4,
            Transition::FormSubmit => 5,
        }
    }

    fn from_code(c: i64) -> Option<Transition> {
        Some(match c {
            0 => Transition::Link,
            1 => Transition::Typed,
            2 => Transition::Bookmark,
            3 => Transition::Reload,
            4 => Transition::Redirect,
            5 => Transition::FormSubmit,
            _ => return None,
        })
    }
}

/// Identity in the table is `(at_ms, device)`: two navigations in the same millisecond on
/// one device count as one visit. The set orders by the whole triple so that the union
/// join stays a semilattice; [`normalize`] collapses duplicate keys before every store.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Visit {
    pub at_ms: i64,
    pub device: DeviceId,
    pub transition: Transition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRecord {
    pub url: Url,
    pub title: Lww<String>,
    pub visits: BTreeSet<Visit>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl PageRecord {
    fn new(url: Url) -> PageRecord {
        PageRecord { url, title: Lww::new(String::new(), Stamp::ZERO), visits: BTreeSet::new(), extra: Extra::default() }
    }

    /// Drop everything but the newest `MAX_VISITS` visits.
    pub fn cap(&mut self) {
        while self.visits.len() > MAX_VISITS {
            self.visits.pop_first();
        }
    }
}

impl Lattice for PageRecord {
    /// title: LWW. visits: union. Pruning by directives and the cap are applied by
    /// [`normalize`] before every store, and are deterministic functions of the joined state.
    fn join(&mut self, other: Self) {
        self.title.join(other.title);
        self.visits.extend(other.visits);
        join_extra(&mut self.extra, other.extra);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletionDirective {
    pub id: Uuid,
    /// `None` = every url.
    pub url: Option<Url>,
    pub from_ms: i64,
    pub to_ms: i64,
}

impl Lattice for DeletionDirective {
    /// Immutable: two records with the same id are equal, so join is identity.
    fn join(&mut self, _other: Self) {}
}

pub fn covers(d: &DeletionDirective, url: &Url, v: &Visit) -> bool {
    d.url.as_ref().is_none_or(|u| u == url) && (d.from_ms..=d.to_ms).contains(&v.at_ms)
}

/// Drop covered visits, collapse to one visit per `(at_ms, device)`, then cap. Returns
/// `None` when no visits remain; the page row is then deleted. It is not a tombstone:
/// page existence is derived from visits.
pub(crate) fn normalize(mut page: PageRecord, directives: &[DeletionDirective]) -> Option<PageRecord> {
    let url = page.url.clone();
    page.visits.retain(|v| !directives.iter().any(|d| covers(d, &url, v)));
    dedupe(&mut page.visits);
    page.cap();
    (!page.visits.is_empty()).then_some(page)
}

/// One visit per `(at_ms, device)`, the `history_visits` key. The join is a plain union
/// over the whole triple, so a record from a peer (or a copied profile sharing our device
/// id) can carry two transitions for one key. The greatest transition wins: a rule every
/// device applies alike, so the stored form never depends on arrival order. Equal keys
/// are adjacent in the set's order with the greatest transition last.
fn dedupe(visits: &mut BTreeSet<Visit>) {
    let same_key = |a: &Visit, b: &Visit| a.at_ms == b.at_ms && a.device == b.device;
    if !visits.iter().zip(visits.iter().skip(1)).any(|(a, b)| same_key(a, b)) {
        return;
    }
    let mut kept = BTreeSet::new();
    let mut run = visits.iter().peekable();
    while let Some(v) = run.next() {
        if !run.peek().is_some_and(|next| same_key(v, next)) {
            kept.insert(*v);
        }
    }
    *visits = kept;
}

/// `http`, `https`, `file` only. Internal pages (`about:`, `vsesvit:`, `data:`, extension pages) are not history.
pub fn is_recordable(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
}

/// Lowercased url without scheme and a leading `www.`: what the omnibox prefix-matches.
pub(crate) fn url_key(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    rest.to_lowercase()
}

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub url: Url,
    pub title: String,
    pub last_visit_ms: i64,
    pub visit_count: u32,
    pub typed_count: u32,
}

pub struct History<'p> {
    pub(crate) p: &'p mut Profile,
}

impl History<'_> {
    /// Access pattern 1. One transaction: upsert page, insert visit, refresh stats, mark
    /// dirty. A single WAL append (well under a millisecond), fine on the UI thread.
    /// Ignores non-recordable urls.
    pub fn record_visit(&mut self, url: &Url, transition: Transition) -> Result<(), Error> {
        if !is_recordable(url) {
            return Ok(());
        }
        let url = url.clone();
        self.p.write(|tx| {
            let visit = Visit { at_ms: tx.now_ms() as i64, device: tx.device, transition };
            let mut page = load_page(&tx.sql, &url)?.unwrap_or_else(|| PageRecord::new(url.clone()));
            if page.visits.iter().any(|v| v.at_ms == visit.at_ms && v.device == visit.device) {
                return Ok(());
            }
            page.visits.insert(visit);
            let directives = directives_for(&tx.sql, Some(&url))?;
            if let Some(page) = normalize(page, &directives) {
                let seq = tx.seq();
                store_page(&tx.sql, &page, seq)?;
            }
            Ok(())
        })
    }

    /// Titles arrive after commit (WebView2 `DocumentTitleChanged`, WebKit `notify::title`).
    /// No-op if unchanged or if the page has no visit yet.
    pub fn set_title(&mut self, url: &Url, title: &str) -> Result<(), Error> {
        let url = url.clone();
        self.p.write(|tx| {
            let Some(mut page) = load_page(&tx.sql, &url)? else { return Ok(()) };
            if page.title.v == title {
                return Ok(());
            }
            let at = tx.stamp();
            page.title.set(title.to_owned(), at);
            let seq = tx.seq();
            store_page(&tx.sql, &page, seq)
        })
    }

    /// Omnibox source: host/url prefix match (indexed `url_key` column, see schema) plus
    /// title substring, ordered by frecency.
    pub fn search(&mut self, text: &str, limit: usize) -> Result<Vec<HistoryEntry>, Error> {
        let needle = text.trim().to_lowercase();
        if needle.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let prefix = format!("{}%", like_escape(&url_key(&needle)));
        let substring = format!("%{}%", like_escape(&needle));
        let mut stmt = self.p.conn.prepare_cached(
            "SELECT url, title, last_visit_ms, visit_count, typed_count FROM history_pages \
             WHERE url_key LIKE ?1 ESCAPE '\\' OR title LIKE ?2 ESCAPE '\\' \
             ORDER BY frecency DESC, last_visit_ms DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![prefix, substring, limit as i64], row_entry)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// History page: visits newest first in `[from_ms, to_ms)`.
    pub fn visits_between(&mut self, from_ms: i64, to_ms: i64, limit: usize) -> Result<Vec<(HistoryEntry, Visit)>, Error> {
        let mut stmt = self.p.conn.prepare_cached(
            "SELECT p.url, p.title, p.last_visit_ms, p.visit_count, p.typed_count, v.at_ms, v.device, v.transition \
             FROM history_visits v JOIN history_pages p ON p.url = v.url \
             WHERE v.at_ms >= ?1 AND v.at_ms < ?2 ORDER BY v.at_ms DESC, v.device LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![from_ms, to_ms, limit as i64], |row| {
            let entry = row_entry(row)?;
            let visit = Visit {
                at_ms: row.get(5)?,
                device: DeviceId(row.get::<_, i64>(6)? as u64),
                transition: Transition::from_code(row.get(7)?).ok_or_else(|| crate::db::bad_column(7, "transition"))?,
            };
            Ok((entry, visit))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Writes a directive `{url: Some(url), 0..=now}` and applies it locally.
    pub fn delete_url(&mut self, url: &Url) -> Result<(), Error> {
        let url = url.clone();
        self.p.write(|tx| {
            let d = DeletionDirective { id: Uuid::new_v4(), url: Some(url), from_ms: 0, to_ms: tx.now_ms() as i64 };
            let seq = tx.seq();
            store_directive(&tx.sql, &d, seq)?;
            apply_directive(&tx.sql, &d)
        })
    }

    /// Writes a directive `{url: None, from..=to}` and applies it locally.
    pub fn delete_range(&mut self, from_ms: i64, to_ms: i64) -> Result<(), Error> {
        self.p.write(|tx| {
            let d = DeletionDirective { id: Uuid::new_v4(), url: None, from_ms, to_ms };
            let seq = tx.seq();
            store_directive(&tx.sql, &d, seq)?;
            apply_directive(&tx.sql, &d)
        })
    }
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn row_entry(row: &rusqlite::Row<'_>) -> Result<HistoryEntry, rusqlite::Error> {
    let url: String = row.get(0)?;
    Ok(HistoryEntry {
        url: Url::parse(&url).map_err(|_| crate::db::bad_column(0, "url"))?,
        title: row.get(1)?,
        last_visit_ms: row.get(2)?,
        visit_count: row.get::<_, i64>(3)?.max(0) as u32,
        typed_count: row.get::<_, i64>(4)?.max(0) as u32,
    })
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

fn load_visits(conn: &rusqlite::Connection, url: &str) -> Result<BTreeSet<Visit>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached("SELECT at_ms, device, transition FROM history_visits WHERE url = ?1")?;
    let rows = stmt.query_map([url], |row| {
        Ok(Visit {
            at_ms: row.get(0)?,
            device: DeviceId(row.get::<_, i64>(1)? as u64),
            transition: Transition::from_code(row.get(2)?).ok_or_else(|| crate::db::bad_column(2, "transition"))?,
        })
    })?;
    rows.collect()
}

fn load_page_by_text(conn: &rusqlite::Connection, url: &str) -> Result<Option<(Seq, PageRecord)>, Error> {
    let head = conn
        .query_row("SELECT title, title_at, extra, seq FROM history_pages WHERE url = ?1", [url], |row| {
            Ok((row.get::<_, String>(0)?, stamp_col(row, 1)?, extra_col(row, 2)?, seq_col(row, 3)?))
        })
        .optional()?;
    let Some((title, title_at, extra, seq)) = head else { return Ok(None) };
    let parsed = Url::parse(url).map_err(|_| crate::db::bad_column(0, "url"))?;
    let visits = load_visits(conn, url)?;
    Ok(Some((seq, PageRecord { url: parsed, title: Lww::new(title, title_at), visits, extra })))
}

pub(crate) fn load_page(conn: &rusqlite::Connection, url: &Url) -> Result<Option<PageRecord>, Error> {
    Ok(load_page_by_text(conn, url.as_str())?.map(|(_, p)| p))
}

/// Directives that can cover visits to `url` (`None` = every directive).
fn directives_for(conn: &rusqlite::Connection, url: Option<&Url>) -> Result<Vec<DeletionDirective>, Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, url, from_ms, to_ms, seq FROM history_deletions WHERE (?1 IS NULL) OR url IS NULL OR url = ?1",
    )?;
    let rows = stmt.query_map([url.map(Url::as_str)], row_directive)?;
    Ok(rows.into_iter().map(|r| r.map(|(_, d)| d)).collect::<Result<Vec<_>, _>>()?)
}

/// Frecency: a pure function of the visit set, so it needs no clock and every store
/// computes the same value. Recency dominates (10 points per day of the newest visit),
/// then volume and typed navigations.
fn frecency(visits: &BTreeSet<Visit>) -> i64 {
    let last_day = visits.iter().map(|v| v.at_ms).max().unwrap_or(0).max(0) / 86_400_000;
    let typed = visits.iter().filter(|v| v.transition == Transition::Typed).count() as i64;
    last_day * 10 + visits.len() as i64 * 100 + typed * 200
}

/// The one writer of the derived stats columns. Recomputes them from `history_visits`.
pub(crate) fn refresh_stats(conn: &rusqlite::Connection, url: &str) -> Result<(), Error> {
    let visits = load_visits(conn, url)?;
    let typed = visits.iter().filter(|v| v.transition == Transition::Typed).count() as i64;
    let last = visits.iter().map(|v| v.at_ms).max().unwrap_or(0);
    conn.execute(
        "UPDATE history_pages SET visit_count = ?2, typed_count = ?3, last_visit_ms = ?4, frecency = ?5 WHERE url = ?1",
        params![url, visits.len() as i64, typed, last, frecency(&visits)],
    )?;
    Ok(())
}

fn write_visits(conn: &rusqlite::Connection, url: &str, visits: &BTreeSet<Visit>) -> Result<(), Error> {
    conn.execute("DELETE FROM history_visits WHERE url = ?1", [url])?;
    let mut ins = conn.prepare_cached("INSERT INTO history_visits (url, at_ms, device, transition) VALUES (?1, ?2, ?3, ?4)")?;
    for v in visits {
        ins.execute(params![url, v.at_ms, v.device.0 as i64, v.transition.code()])?;
    }
    Ok(())
}

/// Upsert a page (already normalized) with its visits and stats.
pub(crate) fn store_page(conn: &rusqlite::Connection, page: &PageRecord, seq: Seq) -> Result<(), Error> {
    let url = page.url.as_str();
    conn.execute(
        "INSERT INTO history_pages (url, url_key, title, title_at, extra, seq, visit_count, typed_count, last_visit_ms, frecency) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0, 0) \
         ON CONFLICT(url) DO UPDATE SET title = excluded.title, title_at = excluded.title_at, extra = excluded.extra, seq = excluded.seq",
        params![url, url_key(url), page.title.v, page.title.at.to_vec(), extra_text(&page.extra), seq.0 as i64],
    )?;
    write_visits(conn, url, &page.visits)?;
    refresh_stats(conn, url)
}

fn delete_page(conn: &rusqlite::Connection, url: &str) -> Result<(), Error> {
    conn.execute("DELETE FROM history_pages WHERE url = ?1", [url])?;
    Ok(())
}

/// Re-derive one stored page after the directive set grew. Keeps the row's seq: every
/// device prunes on its own once it holds the directive, so nothing needs re-uploading.
fn renormalize_page(conn: &rusqlite::Connection, url: &Url) -> Result<(), Error> {
    let Some(page) = load_page(conn, url)? else { return Ok(()) };
    let before = page.visits.clone();
    match normalize(page, &directives_for(conn, Some(url))?) {
        None => delete_page(conn, url.as_str()),
        Some(p) if p.visits != before => {
            write_visits(conn, url.as_str(), &p.visits)?;
            refresh_stats(conn, url.as_str())
        }
        Some(_) => Ok(()),
    }
}

/// Apply a new directive to the pages it can touch (local write or sync apply).
pub(crate) fn apply_directive(conn: &rusqlite::Connection, d: &DeletionDirective) -> Result<(), Error> {
    let urls: Vec<Url> = match &d.url {
        Some(u) => vec![u.clone()],
        None => {
            let mut stmt = conn.prepare_cached("SELECT DISTINCT url FROM history_visits WHERE at_ms >= ?1 AND at_ms <= ?2")?;
            let rows = stmt.query_map(params![d.from_ms, d.to_ms], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).filter_map(|s| Url::parse(&s).ok()).collect()
        }
    };
    for u in &urls {
        renormalize_page(conn, u)?;
    }
    Ok(())
}

fn row_directive(row: &rusqlite::Row<'_>) -> Result<(Seq, DeletionDirective), rusqlite::Error> {
    let id = uuid_col(row, 0)?;
    let url: Option<String> = row.get(1)?;
    let url = match url {
        Some(u) => Some(Url::parse(&u).map_err(|_| crate::db::bad_column(1, "url"))?),
        None => None,
    };
    Ok((seq_col(row, 4)?, DeletionDirective { id, url, from_ms: row.get(2)?, to_ms: row.get(3)? }))
}

fn store_directive(conn: &rusqlite::Connection, d: &DeletionDirective, seq: Seq) -> Result<(), Error> {
    conn.execute(
        "INSERT OR REPLACE INTO history_deletions (id, url, from_ms, to_ms, seq) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![d.id.as_bytes().as_slice(), d.url.as_ref().map(Url::as_str), d.from_ms, d.to_ms, seq.0 as i64],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Sync plumbing
// ---------------------------------------------------------------------------

pub(crate) struct PagesTable;

impl SyncTable for PagesTable {
    const KIND: Kind = Kind::HistoryPages;
    type Record = PageRecord;

    fn wire_id(rec: &PageRecord) -> String {
        rec.url.to_string()
    }

    fn max_stamp(rec: &PageRecord) -> Option<Stamp> {
        Some(extra_max_stamp(&rec.extra).map_or(rec.title.at, |e| e.max(rec.title.at)))
    }

    fn settle(tx: &rusqlite::Transaction<'_>, merged: PageRecord) -> Result<Option<PageRecord>, Error> {
        let directives = directives_for(tx, Some(&merged.url))?;
        Ok(normalize(merged, &directives))
    }

    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<PageRecord>, Error> {
        Ok(load_page_by_text(tx, wire_id)?.map(|(_, p)| p))
    }

    fn store(tx: &rusqlite::Transaction<'_>, rec: &PageRecord, seq: Seq) -> Result<(), Error> {
        store_page(tx, rec, seq)
    }

    fn delete(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<(), Error> {
        delete_page(tx, wire_id)
    }

    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<(Vec<(Seq, PageRecord)>, bool), Error> {
        let (heads, more) = changed_rows(conn, "history_pages", "url, title, title_at, extra, seq", "1", since, limit, |row| {
            Ok((seq_col(row, 4)?, (row.get::<_, String>(0)?, row.get::<_, String>(1)?, stamp_col(row, 2)?, extra_col(row, 3)?)))
        })?;
        let mut out = Vec::with_capacity(heads.len());
        for (seq, (url, title, title_at, extra)) in heads {
            let visits = load_visits(conn, &url)?;
            let url = Url::parse(&url).map_err(|_| crate::db::bad_column(0, "url"))?;
            out.push((seq, PageRecord { url, title: Lww::new(title, title_at), visits, extra }));
        }
        Ok((out, more))
    }
}

pub(crate) struct DeletionsTable;

impl SyncTable for DeletionsTable {
    const KIND: Kind = Kind::HistoryDeletions;
    type Record = DeletionDirective;

    fn wire_id(rec: &DeletionDirective) -> String {
        rec.id.to_string()
    }

    fn max_stamp(_rec: &DeletionDirective) -> Option<Stamp> {
        None
    }

    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<DeletionDirective>, Error> {
        let Ok(id) = Uuid::parse_str(wire_id) else { return Ok(None) };
        let d = tx
            .query_row(
                "SELECT id, url, from_ms, to_ms, seq FROM history_deletions WHERE id = ?1",
                [id.as_bytes().as_slice()],
                row_directive,
            )
            .optional()?;
        Ok(d.map(|(_, d)| d))
    }

    fn store(tx: &rusqlite::Transaction<'_>, rec: &DeletionDirective, seq: Seq) -> Result<(), Error> {
        store_directive(tx, rec, seq)
    }

    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<(Vec<(Seq, DeletionDirective)>, bool), Error> {
        changed_rows(conn, "history_deletions", "id, url, from_ms, to_ms, seq", "1", since, limit, row_directive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visit(at: i64, dev: u64) -> Visit {
        Visit { at_ms: at, device: DeviceId(dev), transition: Transition::Link }
    }

    #[test]
    fn normalize_prunes_before_capping() {
        let url = Url::parse("https://a.example/").unwrap();
        let mut page = PageRecord::new(url.clone());
        page.visits.extend((0..100).map(|i| visit(i, 1)));
        let d = DeletionDirective { id: Uuid::nil(), url: None, from_ms: 36, to_ms: 100 };
        let kept = normalize(page, &[d]).unwrap();
        assert_eq!(kept.visits.len(), 36);
        assert!(kept.visits.iter().all(|v| v.at_ms < 36));
    }

    #[test]
    fn normalize_caps_and_drops_empty() {
        let url = Url::parse("https://a.example/").unwrap();
        let mut page = PageRecord::new(url.clone());
        page.visits.extend((0..100).map(|i| visit(i, 1)));
        let kept = normalize(page.clone(), &[]).unwrap();
        assert_eq!(kept.visits.len(), MAX_VISITS);
        assert_eq!(kept.visits.first().unwrap().at_ms, 36);
        let all = DeletionDirective { id: Uuid::nil(), url: Some(url), from_ms: 0, to_ms: 1000 };
        assert!(normalize(page, &[all]).is_none());
    }

    #[test]
    fn normalize_keeps_one_visit_per_key_with_the_greatest_transition() {
        let url = Url::parse("https://a.example/").unwrap();
        let mut page = PageRecord::new(url.clone());
        page.visits.insert(visit(5, 1));
        page.visits.insert(Visit { at_ms: 5, device: DeviceId(1), transition: Transition::Typed });
        page.visits.insert(visit(5, 2));
        let kept = normalize(page, &[]).unwrap();
        assert_eq!(kept.visits.len(), 2);
        assert_eq!(kept.visits.iter().find(|v| v.device == DeviceId(1)).unwrap().transition, Transition::Typed);

        // duplicates never eat into the cap
        let mut page = PageRecord::new(url);
        for i in 0..70 {
            page.visits.insert(visit(i, 1));
            page.visits.insert(Visit { at_ms: i, device: DeviceId(1), transition: Transition::Reload });
        }
        let kept = normalize(page, &[]).unwrap();
        assert_eq!(kept.visits.len(), MAX_VISITS);
        assert_eq!(kept.visits.first().unwrap().at_ms, 6);
        assert!(kept.visits.iter().all(|v| v.transition == Transition::Reload));
    }

    #[test]
    fn url_key_strips_scheme_and_www() {
        assert_eq!(url_key("https://www.Example.com/A?b=1"), "example.com/a?b=1");
        assert_eq!(url_key("file:///C:/x"), "/c:/x");
        assert_eq!(url_key("example.com"), "example.com");
    }
}
