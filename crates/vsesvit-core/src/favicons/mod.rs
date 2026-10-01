//! Favicons of bookmarked sites, for the bookmarks bar and the Bookmarks view.
//!
//! The shells hand over the PNG a page shows once it has loaded. It is kept when that page,
//! or another page of the same site, is bookmarked, so a bookmark shows its own page's icon,
//! or its site's icon until that page has been visited (imported bookmarks, for one). Icons
//! of sites with no bookmark left are dropped when the profile opens.
//!
//! Bookmarks whose site has no icon yet (imported ones, say) get one without a visit:
//!
//! ```text
//!   missing(limit)  [UI thread]      FaviconFetch::run()  [worker thread]      commit_fetched()  [UI thread]
//!   pages ───────────────────▶ FaviconFetch ───────────────────────▶ Vec<Fetched> ──────────────────▶ stored icons
//!                                                                                                  + failed origins
//! ```
//!
//! A site whose icon could not be fetched is not asked for again for [`RETRY_AFTER_MS`], or
//! for [`UNREACHABLE_RETRY_MS`] when it did not answer at all (the device was offline, say).
//!
//! LOCAL: derived from what this device loads, so never synced.

mod fetch;

use std::collections::{HashSet, VecDeque};

use rusqlite::{Connection, OptionalExtension, params};

pub use fetch::{FaviconFetch, Fetched, Outcome};
#[cfg(feature = "testkit")]
pub use fetch::allow_local_hosts;

use crate::bookmarks::{BookmarkId, NodeKind, Tree};
use crate::{Error, Profile, Url};

/// Larger icons are not kept; a 32x32 PNG is a few KiB.
pub const MAX_BYTES: usize = 64 * 1024;

/// How long a site whose icon could not be fetched is left alone: 7 days.
pub const RETRY_AFTER_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// How long a site that did not answer at all is left alone: an hour.
pub const UNREACHABLE_RETRY_MS: i64 = 60 * 60 * 1000;

pub(crate) const SCHEMA: &str = "
CREATE TABLE favicons (                  -- LOCAL
  page_url    TEXT PRIMARY KEY,
  origin      TEXT,                      -- NULL for an opaque origin (file:, data:)
  png         BLOB NOT NULL,
  updated_ms  INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX favicons_origin ON favicons(origin, updated_ms);
";

/// Sites [`FaviconFetch`] found no icon for, so [`Favicons::missing`] skips them for a while.
pub(crate) const FAILURES_SCHEMA: &str = "
CREATE TABLE favicon_failures (          -- LOCAL
  origin     TEXT PRIMARY KEY,
  failed_ms  INTEGER NOT NULL
) WITHOUT ROWID;
";

pub struct Favicons<'p> {
    pub(crate) p: &'p mut Profile,
}

/// `scheme://host:port`, or `None` for an opaque origin, which no two pages share.
fn origin(url: &Url) -> Option<String> {
    let origin = url.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

impl Favicons<'_> {
    /// Keeps `png` as the icon of `page` when the page or its site is bookmarked. Returns
    /// whether the stored icon changed, which is when bookmarks need repainting.
    pub fn record(&mut self, page: &Url, png: &[u8]) -> Result<bool, Error> {
        let now = self.p.clock.now_ms() as i64;
        store(&self.p.conn, &self.p.bookmarks.tree, now, page, png)
    }

    /// Bookmarked pages with no stored icon for the page or its site, one per origin, skipping
    /// origins that failed within [`RETRY_AFTER_MS`]. At most `limit`. Bookmarks bar items come
    /// first, then the other roots, then each level of folders below them.
    pub fn missing(&mut self, limit: usize) -> Result<Vec<Url>, Error> {
        let since = self.p.clock.now_ms() as i64 - RETRY_AFTER_MS;
        let mut skip: HashSet<String> = {
            let mut stmt = self.p.conn.prepare(
                "SELECT origin FROM favicons WHERE origin IS NOT NULL
                 UNION SELECT origin FROM favicon_failures WHERE failed_ms > ?1",
            )?;
            stmt.query_map([since], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        let mut pages = Vec::new();
        let mut folders = VecDeque::from([BookmarkId::TOOLBAR, BookmarkId::OTHER, BookmarkId::MOBILE]);
        let bookmarks = self.p.bookmarks();
        while let Some(folder) = folders.pop_front() {
            for node in bookmarks.children(folder) {
                match (node.kind, node.url) {
                    (NodeKind::Folder, _) => folders.push_back(node.id),
                    (NodeKind::Url, Some(url)) if matches!(url.scheme(), "http" | "https") => {
                        if pages.len() == limit {
                            return Ok(pages);
                        }
                        if origin(&url).is_some_and(|o| skip.insert(o)) {
                            pages.push(url);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(pages)
    }

    /// Stores fetched icons (same rules as [`Favicons::record`]) and marks the sites of the
    /// pages that got none as failed (see [`Outcome`]). Returns whether any stored icon changed.
    pub fn commit_fetched(&mut self, results: Vec<Fetched>) -> Result<bool, Error> {
        let now = self.p.clock.now_ms() as i64;
        let Profile { conn, bookmarks, .. } = &mut *self.p;
        let tx = conn.transaction()?;
        let mut changed = false;
        for Fetched { page, outcome } in results {
            let Some(site) = origin(&page) else { continue };
            let failed_ms = match outcome {
                Outcome::Icon(png) => {
                    changed |= store(&tx, &bookmarks.tree, now, &page, &png)?;
                    tx.execute("DELETE FROM favicon_failures WHERE origin = ?1", [site])?;
                    continue;
                }
                Outcome::NoIcon => now,
                // Dated so that `missing` skips the site for UNREACHABLE_RETRY_MS only.
                Outcome::Unreachable => now - RETRY_AFTER_MS + UNREACHABLE_RETRY_MS,
            };
            tx.execute(
                "INSERT INTO favicon_failures (origin, failed_ms) VALUES (?1, ?2)
                 ON CONFLICT (origin) DO UPDATE SET failed_ms = ?2",
                params![site, failed_ms],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// The icon of `page`, or else the latest icon of its site.
    pub fn get(&mut self, page: &Url) -> Result<Option<Vec<u8>>, Error> {
        let conn = &self.p.conn;
        let exact =
            conn.query_row("SELECT png FROM favicons WHERE page_url = ?1", [page.as_str()], |r| r.get(0)).optional()?;
        if exact.is_some() {
            return Ok(exact);
        }
        let Some(site) = origin(page) else {
            return Ok(None);
        };
        Ok(conn
            .query_row("SELECT png FROM favicons WHERE origin = ?1 ORDER BY updated_ms DESC LIMIT 1", [site], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Drops the icons of pages and sites that have no bookmark any more.
    pub(crate) fn prune(&mut self) -> Result<usize, Error> {
        let rows: Vec<(String, Option<String>)> = {
            let mut stmt = self.p.conn.prepare("SELECT page_url, origin FROM favicons")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
        };
        let tree = &self.p.bookmarks.tree;
        let stale: Vec<String> = rows
            .into_iter()
            .filter(|(page, site)| {
                let page_kept = Url::parse(page).is_ok_and(|u| !tree.ids_for_url(&u).is_empty());
                let site_kept = site.as_deref().is_some_and(|o| tree.has_origin(o));
                !page_kept && !site_kept
            })
            .map(|(page, _)| page)
            .collect();
        let mut delete = self.p.conn.prepare("DELETE FROM favicons WHERE page_url = ?1")?;
        for page in &stale {
            delete.execute([page])?;
        }
        let since = self.p.clock.now_ms() as i64 - RETRY_AFTER_MS;
        self.p.conn.execute("DELETE FROM favicon_failures WHERE failed_ms <= ?1", [since])?;
        Ok(stale.len())
    }
}

/// [`Favicons::record`] on a connection or an open transaction.
fn store(conn: &Connection, tree: &Tree, now_ms: i64, page: &Url, png: &[u8]) -> Result<bool, Error> {
    let site = origin(page);
    let bookmarked = !tree.ids_for_url(page).is_empty() || site.as_deref().is_some_and(|o| tree.has_origin(o));
    if !bookmarked || png.is_empty() || png.len() > MAX_BYTES {
        return Ok(false);
    }
    let stored: Option<Vec<u8>> =
        conn.query_row("SELECT png FROM favicons WHERE page_url = ?1", [page.as_str()], |r| r.get(0)).optional()?;
    if stored.as_deref() == Some(png) {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO favicons (page_url, origin, png, updated_ms) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (page_url) DO UPDATE SET origin = ?2, png = ?3, updated_ms = ?4",
        params![page.as_str(), site, png, now_ms],
    )?;
    Ok(true)
}
