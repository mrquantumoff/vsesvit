//! Page zoom remembered per site, as Chrome does: one level per host, whatever the scheme or
//! port. This device's only (a LOCAL table), never synced. A site with no row is at
//! [`DEFAULT`], and setting the default forgets the site.

use rusqlite::{OptionalExtension, params};

use crate::{Error, Profile, Url};

pub(crate) const SCHEMA: &str = "
CREATE TABLE site_zoom (                 -- LOCAL
  host     TEXT PRIMARY KEY,             -- lowercase, as Url::host_str
  percent  INTEGER NOT NULL
) WITHOUT ROWID;
";

/// A page at 100%.
pub const DEFAULT: f64 = 1.0;

pub struct SiteZoom<'p> {
    pub(crate) p: &'p mut Profile,
}

/// The site a zoom belongs to: the host of a web page. Files, `about:` and the like have none.
fn host(url: &Url) -> Option<&str> {
    matches!(url.scheme(), "http" | "https").then(|| url.host_str()).flatten()
}

fn to_percent(level: f64) -> i64 {
    (level * 100.0).round() as i64
}

impl SiteZoom<'_> {
    /// The level to show `url` at: what was set for its site, else [`DEFAULT`].
    pub fn get(&mut self, url: &Url) -> Result<f64, Error> {
        let Some(host) = host(url) else { return Ok(DEFAULT) };
        let stored: Option<i64> =
            self.p.conn.query_row("SELECT percent FROM site_zoom WHERE host = ?1", [host], |r| r.get(0)).optional()?;
        Ok(stored.map_or(DEFAULT, |percent| percent as f64 / 100.0))
    }

    /// Remembers `level` for the site of `url`; a page that is not on a site is not remembered.
    pub fn set(&mut self, url: &Url, level: f64) -> Result<(), Error> {
        let Some(host) = host(url) else { return Ok(()) };
        let percent = to_percent(level);
        if percent == to_percent(DEFAULT) || percent <= 0 {
            self.p.conn.execute("DELETE FROM site_zoom WHERE host = ?1", [host])?;
        } else {
            self.p.conn.execute(
                "INSERT INTO site_zoom (host, percent) VALUES (?1, ?2)
                 ON CONFLICT (host) DO UPDATE SET percent = ?2",
                params![host, percent],
            )?;
        }
        Ok(())
    }
}
