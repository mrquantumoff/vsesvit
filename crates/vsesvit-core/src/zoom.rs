//! Page zoom remembered per site, as Chrome does: one level per host, whatever the scheme or
//! port. This device's only (a LOCAL table), never synced. A site with no row is at
//! [`DEFAULT`], and setting the default forgets the site. Zoom set in a private window stays
//! in the private session ([`crate::private`]).

use rusqlite::{OptionalExtension, params};

use crate::private::Browsing;
use crate::{Error, Profile, Url};

pub(crate) const SCHEMA: &str = "
CREATE TABLE site_zoom (                 -- LOCAL
  host     TEXT PRIMARY KEY,             -- lowercase, as Url::host_str
  percent  INTEGER NOT NULL
) WITHOUT ROWID;
";

/// A page at 100%.
pub const DEFAULT: f64 = 1.0;

/// Chrome's zoom levels, in percent: the presets zooming in and out steps through. Whole
/// percents, as the store keeps them, so a level read back from it is still a preset.
pub const PRESETS: [u32; 17] = [25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Step {
    In,
    Out,
}

/// The next preset level from `current`, which need not be a preset itself. At the last
/// preset, `current` stays.
pub fn step(current: f64, step: Step) -> f64 {
    const EPSILON: f64 = 0.001;
    let mut presets = PRESETS.iter().map(|&percent| f64::from(percent) / 100.0);
    match step {
        Step::In => presets.find(|&l| l > current + EPSILON),
        Step::Out => presets.rev().find(|&l| l < current - EPSILON),
    }
    .unwrap_or(current)
}

/// `level` as the zoom chip and the zoom bubble show it: "125%".
pub fn label(level: f64) -> String {
    format!("{:.0}%", level * 100.0)
}

/// The zoom a tab of one [`Browsing`] kind goes by: in private, what the private session kept
/// for a site wins over what is stored.
pub struct SiteZoom<'p> {
    pub(crate) p: &'p mut Profile,
    pub(crate) browsing: Browsing,
}

/// The site a zoom belongs to: the host of a web page. Files, `about:` and the like have none.
fn host(url: &Url) -> Option<&str> {
    matches!(url.scheme(), "http" | "https").then(|| url.host_str()).flatten()
}

fn to_percent(level: f64) -> i64 {
    (level * 100.0).round() as i64
}

/// The percent remembered for `level`; `None` for the default, which is not remembered.
fn remembered(level: f64) -> Option<i64> {
    let percent = to_percent(level);
    (percent != to_percent(DEFAULT) && percent > 0).then_some(percent)
}

impl SiteZoom<'_> {
    /// The level to show `url` at: what was set for its site, else [`DEFAULT`].
    pub fn get(&mut self, url: &Url) -> Result<f64, Error> {
        let Some(host) = host(url) else { return Ok(DEFAULT) };
        let kept = match self.browsing {
            Browsing::Normal => None,
            Browsing::Private => self.p.private.zoom.get(host).copied(),
        };
        let percent = match kept {
            Some(percent) => percent,
            None => self.p.conn.query_row("SELECT percent FROM site_zoom WHERE host = ?1", [host], |r| r.get(0)).optional()?,
        };
        Ok(percent.map_or(DEFAULT, |percent: i64| percent as f64 / 100.0))
    }

    /// Remembers `level` for the site of `url`; a page that is not on a site is not remembered.
    pub fn set(&mut self, url: &Url, level: f64) -> Result<(), Error> {
        let Some(host) = host(url) else { return Ok(()) };
        match (self.browsing, remembered(level)) {
            (Browsing::Private, percent) => {
                self.p.private.zoom.insert(host.to_owned(), percent);
            }
            (Browsing::Normal, None) => {
                self.p.conn.execute("DELETE FROM site_zoom WHERE host = ?1", [host])?;
            }
            (Browsing::Normal, Some(percent)) => {
                self.p.conn.execute(
                    "INSERT INTO site_zoom (host, percent) VALUES (?1, ?2)
                     ON CONFLICT (host) DO UPDATE SET percent = ?2",
                    params![host, percent],
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_walk_the_presets_and_stop_at_the_ends() {
        assert_eq!(step(1.0, Step::In), 1.1);
        assert_eq!(step(1.0, Step::Out), 0.9);
        assert_eq!(step(5.0, Step::In), 5.0);
        assert_eq!(step(0.25, Step::Out), 0.25);
    }

    #[test]
    fn zoom_out_follows_chromes_presets_below_half() {
        assert_eq!(step(0.5, Step::Out), 0.33);
        assert_eq!(step(0.33, Step::Out), 0.25);
        assert_eq!(step(0.25, Step::In), 0.33);
        assert_eq!(step(0.67, Step::Out), 0.5, "a level read back from the store's whole percents is still a preset");
    }

    #[test]
    fn off_preset_levels_snap_to_the_neighbouring_preset() {
        assert_eq!(step(1.05, Step::In), 1.1);
        assert_eq!(step(1.05, Step::Out), 1.0);
        assert_eq!(step(9.0, Step::Out), 5.0);
    }

    #[test]
    fn labels_are_rounded_percents() {
        assert_eq!(label(1.0), "100%");
        assert_eq!(label(0.67), "67%");
        assert_eq!(label(1.25), "125%");
        for percent in PRESETS {
            assert_eq!(label(f64::from(percent) / 100.0), format!("{percent}%"));
        }
    }

    #[test]
    fn presets_rise_through_100_and_survive_the_store() {
        assert!(PRESETS.windows(2).all(|w| w[0] < w[1]));
        assert!(PRESETS.contains(&100));
        for percent in PRESETS {
            assert_eq!(remembered(f64::from(percent) / 100.0).unwrap_or(100), i64::from(percent));
        }
    }
}
