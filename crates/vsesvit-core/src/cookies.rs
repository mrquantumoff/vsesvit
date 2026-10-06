//! Cookie controls: which pages get third-party cookies ([`ThirdPartyCookies`], chosen in
//! Settings > Privacy and stored in [`keys::THIRD_PARTY_COOKIES`]) and the user's rule for a
//! site ([`Permission::Cookies`] set to Allow, Block or Clear on exit, synced with the other
//! site settings).
//!
//! With no rule a site uses cookies as the Settings choice says. Allow also lets the sites it
//! embeds use theirs, where the engine can make that exception for one site. Block keeps a site
//! from using cookies at all: the shells keep its requests from sending or storing them and run
//! [`block_script`] in its documents. The data of sites set to Block or Clear on exit
//! ([`SiteRules::to_clear`]) is deleted when Vsesvit closes and again when it starts, in case it
//! did not close cleanly.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::address::covers;
use crate::permissions::{Origin, Permission, Setting};
use crate::prefs::keys;
use crate::{Error, Profile, Url};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThirdPartyCookies {
    Allow,
    BlockInPrivate,
    Block,
}

impl ThirdPartyCookies {
    pub const ALL: [ThirdPartyCookies; 3] = [Self::Allow, Self::BlockInPrivate, Self::Block];

    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow third-party cookies",
            Self::BlockInPrivate => "Block third-party cookies in private windows",
            Self::Block => "Block third-party cookies",
        }
    }

    /// What the choice does, for Settings.
    pub fn description(self) -> &'static str {
        match self {
            Self::Allow => "Sites can use cookies from other sites they embed, which can track you across sites",
            Self::BlockInPrivate => "In private windows, sites can't use cookies from other sites they embed",
            Self::Block => "Sites can't use cookies from other sites they embed. Some sites may not work",
        }
    }

    pub fn blocks(self, browsing: Browsing) -> bool {
        match self {
            Self::Allow => false,
            Self::BlockInPrivate => browsing == Browsing::Private,
            Self::Block => true,
        }
    }
}

/// Normal or private browsing. Vsesvit has no private windows yet; when it does, they pass
/// [`Browsing::Private`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Browsing {
    Normal,
    Private,
}

/// The title of the site-info popup's section, the same as [`Permission::Cookies`]' label.
pub const SITE_TITLE: &str = "Cookies and site data";

/// Whether third-party cookies are blocked on pages of `top` in `browsing` windows: the
/// Settings choice, lifted on a site whose cookies are set to Allow. `top: None` asks for every
/// site at once, as WebKitGTK sets one policy for all, so no site's rule counts. This is the
/// hook private windows call with [`Browsing::Private`].
pub fn third_party_blocked(p: &mut Profile, browsing: Browsing, top: Option<&Origin>) -> bool {
    p.prefs().get(&keys::THIRD_PARTY_COOKIES).blocks(browsing) && !top.is_some_and(|o| setting(p, o) == Some(Setting::Allow))
}

/// The site's stored rule; `None` = the default.
pub fn setting(p: &mut Profile, origin: &Origin) -> Option<Setting> {
    p.site_permissions().get(origin, Permission::Cookies)
}

pub fn set(p: &mut Profile, origin: &Origin, setting: Option<Setting>) -> Result<(), Error> {
    p.site_permissions().set(origin, Permission::Cookies, setting)
}

/// The site-info choices in order: `None` (the default) first, then Allow, Block and Clear on
/// exit. `exceptions`: the engine can lift third-party blocking for one site. Without it
/// (Linux) Allow means the same as the default, so it is offered only when it is `current`,
/// synced from another device.
pub fn site_choices(current: Option<Setting>, exceptions: bool) -> Vec<Option<Setting>> {
    let mut choices = vec![None];
    if exceptions || current == Some(Setting::Allow) {
        choices.push(Some(Setting::Allow));
    }
    choices.extend([Some(Setting::Block), Some(Setting::ClearOnExit)]);
    choices
}

pub fn choice_label(choice: Option<Setting>) -> &'static str {
    choice.map_or("Default", Setting::label)
}

/// Under the site-info choice: what it means on this page. `third_party_blocked`: whether the
/// shell blocks third-party cookies on it.
pub fn site_status(third_party_blocked: bool, setting: Option<Setting>) -> &'static str {
    match (third_party_blocked, setting) {
        (_, Some(Setting::Block)) => "This site can't use cookies",
        (_, Some(Setting::ClearOnExit)) => "This site's cookies and data are deleted when you close Vsesvit",
        (false, None | Some(Setting::Allow)) => "This site and sites it embeds can use cookies",
        (true, None) => "Third-party cookies are blocked on this site",
        (true, Some(Setting::Allow)) => "This site can use cookies. Third-party cookies are blocked on it",
    }
}

/// The per-site rules the shells enforce.
#[derive(Clone, Debug, Default)]
pub struct SiteRules {
    blocked_hosts: Vec<String>,
    to_clear: Vec<Origin>,
    clear_hosts: Vec<String>,
}

pub fn site_rules(p: &mut Profile) -> SiteRules {
    let mut rules = SiteRules::default();
    for s in p.site_permissions().all().into_iter().filter(|s| s.permission == Permission::Cookies) {
        let host = host(&s.origin);
        match s.setting {
            Setting::Allow => continue,
            Setting::Block => rules.blocked_hosts.push(host.clone()),
            Setting::ClearOnExit => {}
        }
        rules.clear_hosts.push(host);
        rules.to_clear.push(s.origin);
    }
    rules.blocked_hosts.sort_unstable();
    rules.blocked_hosts.dedup();
    rules
}

impl SiteRules {
    /// Hosts of the sites set to Block, deduplicated and sorted. A rule covers the host and its
    /// subdomains.
    pub fn blocked_hosts(&self) -> &[String] {
        &self.blocked_hosts
    }

    /// Whether a cookie under `domain` reaches a site set to Block or is one of its
    /// subdomains': a domain cookie (`.example.com`) reaches `www.example.com`, a host-only one
    /// (`example.com`) only `example.com`.
    pub fn blocks_cookie(&self, domain: &str) -> bool {
        self.blocked_hosts.iter().any(|h| belongs(domain, h))
    }

    /// Sites whose data is deleted at exit and at the next start, those set to Block and to
    /// Clear on exit, in order.
    pub fn to_clear(&self) -> &[Origin] {
        &self.to_clear
    }

    /// Whether a cookie under `domain` belongs to a site to clear, as [`SiteRules::blocks_cookie`]
    /// matches blocked sites.
    pub fn clears_cookie(&self, domain: &str) -> bool {
        self.clear_hosts.iter().any(|h| belongs(domain, h))
    }

    /// Whether the data WebKit keeps under website-data record `name` (a registrable domain such
    /// as `example.com`) holds a site to clear's: the name covers one of those sites' hosts, or
    /// one of them covers it.
    pub fn clears(&self, name: &str) -> bool {
        self.clear_hosts.iter().any(|h| covers(name, h) || covers(h, name))
    }
}

/// Whether a cookie under `domain` (a leading dot for a domain cookie) reaches `host`, or is
/// set by one of its subdomains.
fn belongs(domain: &str, host: &str) -> bool {
    match domain.strip_prefix('.') {
        Some(parent) => covers(parent, host) || covers(host, parent),
        None => covers(host, domain),
    }
}

/// The ASCII host: `example.com`, `xn--e1afmkfd.xn--j1amh`, `[::1]`.
fn host(origin: &Origin) -> String {
    let url = Url::parse(origin.as_str()).expect("an origin is a URL");
    url.host_str().unwrap_or_default().to_owned()
}

const BLOCK_SCRIPT: &str = r#"(() => {
  const hosts = HOSTS;
  let host;
  try { host = new URL(self.origin).hostname; } catch { return; }
  if (!hosts.some(d => host === d || host.endsWith('.' + d))) return;
  Object.defineProperty(Document.prototype, 'cookie', { get: () => '', set: () => {}, configurable: true });
  delete self.cookieStore;
  delete Window.prototype.cookieStore;
})();"#;

/// The script both shells run at the start of every document in every frame: on a host
/// `blocked_hosts` covers, `document.cookie` reads `""` and ignores writes, and `cookieStore`
/// is gone. It goes by the document's origin, so an `about:blank` frame of a blocked site is
/// covered too. `None` when nothing is blocked.
pub fn block_script(blocked_hosts: &[String]) -> Option<String> {
    if blocked_hosts.is_empty() {
        return None;
    }
    let hosts = serde_json::to_string(blocked_hosts).expect("JSON of strings");
    Some(BLOCK_SCRIPT.replace("HOSTS", &hosts))
}

/// One row of the cookies list in Settings, for an engine that lists cookies one by one
/// (Windows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteCookies {
    pub site: String,
    pub count: usize,
}

/// Cookies grouped by their domain without its leading dot, sorted by site.
pub fn group_by_site<'a>(domains: impl IntoIterator<Item = &'a str>) -> Vec<SiteCookies> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for domain in domains {
        *counts.entry(domain.strip_prefix('.').unwrap_or(domain)).or_default() += 1;
    }
    counts.into_iter().map(|(site, count)| SiteCookies { site: site.to_owned(), count }).collect()
}
