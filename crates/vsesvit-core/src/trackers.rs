//! Tracking protection: the bundled tracker list ([`TrackerList`]), the level the user picks in
//! Settings > Privacy ([`TrackingProtection`], stored in [`keys::TRACKING_PROTECTION`]) and the
//! sites they turned it off for ([`Permission::Trackers`] set to Allow, synced with the other
//! site settings).
//!
//! The list (`trackers.json`) groups tracker domains by the company that runs them. A
//! company's trackers are first party on the sites it runs, so `facebook.net` loads on
//! `facebook.com`. Windows asks [`TrackerList::blocks`] about each request that may be a
//! tracker; Linux compiles [`TrackerList::dnr_rules`] into a WebKit content blocker. The one
//! difference: WebKit judges a frame's requests by the frame's own site, Windows by the tab's
//! page, so a Google frame on another site may load Google's trackers on Linux only.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::address::covers;
use crate::permissions::{Origin, Permission, Setting};
use crate::prefs::keys;
use crate::{Error, Profile};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackingProtection {
    Off,
    Standard,
    Strict,
}

impl TrackingProtection {
    pub const ALL: [TrackingProtection; 3] = [Self::Off, Self::Standard, Self::Strict];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Standard => "Standard",
            Self::Strict => "Strict",
        }
    }

    /// What the level blocks, for Settings.
    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "Sites can load trackers",
            Self::Standard => "Blocks known advertising and analytics trackers on other sites",
            Self::Strict => "Also blocks social media trackers, such as Like and Share buttons. Some sites may not work",
        }
    }

    fn blocks(self, category: Category) -> bool {
        match self {
            Self::Off => false,
            Self::Standard => category != Category::Social,
            Self::Strict => true,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Category {
    Advertising,
    Analytics,
    /// Share buttons, embedded posts and sign-in widgets that report the pages they are on.
    Social,
}

#[derive(Deserialize)]
struct ListFile {
    entities: BTreeMap<String, EntityFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityFile {
    #[serde(default)]
    sites: Vec<String>,
    #[serde(default)]
    advertising: Vec<String>,
    #[serde(default)]
    analytics: Vec<String>,
    #[serde(default)]
    social: Vec<String>,
}

#[derive(Clone, Debug)]
struct Entity {
    /// Where its trackers are first party: its sites and its tracker domains.
    first_party: Vec<String>,
    trackers: Vec<(String, Category)>,
}

/// Tracker domains by company. A domain covers its subdomains.
#[derive(Clone, Debug)]
pub struct TrackerList {
    entities: Vec<Entity>,
    /// Tracker domain -> (index into `entities`, category).
    index: HashMap<String, (usize, Category)>,
}

impl TrackerList {
    /// The list built into this release.
    pub fn bundled() -> &'static TrackerList {
        static LIST: OnceLock<TrackerList> = OnceLock::new();
        LIST.get_or_init(|| TrackerList::parse(include_str!("trackers.json")).expect("the bundled tracker list parses"))
    }

    pub fn parse(json: &str) -> Result<TrackerList, serde_json::Error> {
        let file: ListFile = serde_json::from_str(json)?;
        let mut list = TrackerList { entities: Vec::new(), index: HashMap::new() };
        for entity in file.entities.into_values() {
            let trackers: Vec<(String, Category)> = [
                (entity.advertising, Category::Advertising),
                (entity.analytics, Category::Analytics),
                (entity.social, Category::Social),
            ]
            .into_iter()
            .flat_map(|(domains, category)| domains.into_iter().map(move |d| (d, category)))
            .collect();
            let mut first_party = entity.sites;
            for (domain, _) in &trackers {
                if !first_party.contains(domain) {
                    first_party.push(domain.clone());
                }
            }
            list.push(Entity { first_party, trackers });
        }
        Ok(list)
    }

    /// Adds a company that runs `tracker` and no site, as the self-tests do with `localhost`.
    pub fn with_tracker(mut self, tracker: &str, category: Category) -> TrackerList {
        self.push(Entity { first_party: vec![tracker.to_owned()], trackers: vec![(tracker.to_owned(), category)] });
        self
    }

    /// Every tracker domain, for engines that pick the requests to ask [`blocks`](Self::blocks) about.
    pub fn domains(&self) -> impl Iterator<Item = &str> {
        self.index.keys().map(String::as_str)
    }

    fn push(&mut self, entity: Entity) {
        for (domain, category) in &entity.trackers {
            self.index.insert(domain.clone(), (self.entities.len(), *category));
        }
        self.entities.push(entity);
    }

    /// The list domain `request_host` matches when `level` blocks it on a page at `page_host`;
    /// `None` when it is no tracker, the level lets its category load, or its company runs the
    /// page's site.
    pub fn blocks(&self, level: TrackingProtection, page_host: &str, request_host: &str) -> Option<&str> {
        let (domain, &(entity, category)) = suffixes(request_host).find_map(|d| self.index.get_key_value(d))?;
        let first_party = self.entities[entity].first_party.iter().any(|site| covers(site, page_host));
        (level.blocks(category) && !first_party).then_some(domain.as_str())
    }

    /// The same blocking as [`blocks`](Self::blocks) as declarativeNetRequest rules (a
    /// `rules.json`), for engines that take content blockers. Pages of the `allowed` sites
    /// load everything.
    pub fn dnr_rules(&self, level: TrackingProtection, allowed: &[Origin]) -> String {
        let mut rules = Vec::new();
        for entity in &self.entities {
            let blocked: Vec<&str> = entity.trackers.iter().filter(|(_, c)| level.blocks(*c)).map(|(d, _)| d.as_str()).collect();
            if !blocked.is_empty() {
                rules.push(json!({
                    "id": rules.len() + 1,
                    "action": { "type": "block" },
                    "condition": { "requestDomains": blocked, "excludedInitiatorDomains": entity.first_party },
                }));
            }
        }
        if !rules.is_empty() {
            for origin in allowed {
                rules.push(json!({
                    "id": rules.len() + 1,
                    "priority": 2,
                    "action": { "type": "allowAllRequests" },
                    "condition": { "urlFilter": format!("|{}/", origin.as_str()), "resourceTypes": ["main_frame"] },
                }));
            }
        }
        serde_json::Value::Array(rules).to_string()
    }
}

/// `a.b.example.com`, `b.example.com`, `example.com`, `com`.
fn suffixes(host: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(host), |h| h.split_once('.').map(|(_, rest)| rest))
}

/// The title of tracking protection's row in Settings and of its switch in the site-info popup.
pub const TITLE: &str = "Tracking protection";

/// Under the site-info popup's switch: whether protection is `on` for the site and, where the
/// engine reports it, how many tracker domains it `blocked` on the page.
pub fn site_status(on: bool, blocked: Option<usize>) -> String {
    match (on, blocked) {
        (false, _) => "Off for this site".to_owned(),
        (true, None) => "Blocking known trackers on this site".to_owned(),
        (true, Some(0)) => "No trackers blocked on this page".to_owned(),
        (true, Some(1)) => "1 tracker blocked on this page".to_owned(),
        (true, Some(n)) => format!("{n} trackers blocked on this page"),
    }
}

/// The protection pages of `origin` get: the user's level, or Off on a site they turned it off
/// for. A page with no origin gets the level.
pub fn level_for(p: &mut Profile, origin: Option<&Origin>) -> TrackingProtection {
    match origin {
        Some(origin) if allowed(p, origin) => TrackingProtection::Off,
        _ => p.prefs().get(&keys::TRACKING_PROTECTION),
    }
}

/// Whether the user turned tracking protection off for `origin`.
pub fn allowed(p: &mut Profile, origin: &Origin) -> bool {
    p.site_permissions().get(origin, Permission::Trackers) == Some(Setting::Allow)
}

/// Turns tracking protection off for `origin` (`allowed`), or back on.
pub fn set_allowed(p: &mut Profile, origin: &Origin, allowed: bool) -> Result<(), Error> {
    p.site_permissions().set(origin, Permission::Trackers, allowed.then_some(Setting::Allow))
}

/// Every site the user turned tracking protection off for.
pub fn allowed_sites(p: &mut Profile) -> Vec<Origin> {
    p.site_permissions()
        .all()
        .into_iter()
        .filter(|s| s.permission == Permission::Trackers && s.setting == Setting::Allow)
        .map(|s| s.origin)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each domain is a lowercase host, and each tracker is one company's.
    #[test]
    fn the_bundled_list_is_well_formed() {
        let file: ListFile = serde_json::from_str(include_str!("trackers.json")).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        for (name, entity) in &file.entities {
            let trackers = entity.advertising.iter().chain(&entity.analytics).chain(&entity.social);
            for domain in entity.sites.iter().chain(trackers.clone()) {
                let host = url::Host::parse(domain).map(|h| h.to_string());
                assert_eq!(host.as_deref(), Ok(domain.as_str()), "{name}: {domain} is a lowercase host");
                assert!(domain.contains('.'), "{name}: {domain}");
            }
            for domain in trackers {
                assert!(seen.insert(domain.clone()), "{name}: {domain} is listed twice");
            }
        }
        assert!(TrackerList::bundled().entities.len() > 50);
    }
}
