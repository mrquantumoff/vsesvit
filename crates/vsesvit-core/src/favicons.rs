//! Favicons of bookmarked sites, for the bookmarks bar and the Bookmarks view.
//!
//! The shells hand over the PNG a page shows once it has loaded. It is kept when that page,
//! or another page of the same site, is bookmarked, so a bookmark shows its own page's icon,
//! or its site's icon until that page has been visited (imported bookmarks, for one). Icons
//! of sites with no bookmark left are dropped when the profile opens.
//!
//! LOCAL: derived from what this device loads, so never synced.

use rusqlite::{OptionalExtension, params};

use crate::{Error, Profile, Url};

/// Larger icons are not kept; a 32x32 PNG is a few KiB.
pub const MAX_BYTES: usize = 64 * 1024;

pub(crate) const SCHEMA: &str = "
CREATE TABLE favicons (                  -- LOCAL
  page_url    TEXT PRIMARY KEY,
  origin      TEXT,                      -- NULL for an opaque origin (file:, data:)
  png         BLOB NOT NULL,
  updated_ms  INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX favicons_origin ON favicons(origin, updated_ms);
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
        let site = origin(page);
        let bookmarked = self.p.bookmarks().is_bookmarked(page)
            || site.as_deref().is_some_and(|o| self.p.bookmarks.tree.has_origin(o));
        if !bookmarked || png.is_empty() || png.len() > MAX_BYTES {
            return Ok(false);
        }
        let stored: Option<Vec<u8>> = self
            .p
            .conn
            .query_row("SELECT png FROM favicons WHERE page_url = ?1", [page.as_str()], |r| r.get(0))
            .optional()?;
        if stored.as_deref() == Some(png) {
            return Ok(false);
        }
        let now = self.p.clock.now_ms() as i64;
        self.p.conn.execute(
            "INSERT INTO favicons (page_url, origin, png, updated_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (page_url) DO UPDATE SET origin = ?2, png = ?3, updated_ms = ?4",
            params![page.as_str(), site, png, now],
        )?;
        Ok(true)
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
        Ok(stale.len())
    }
}
