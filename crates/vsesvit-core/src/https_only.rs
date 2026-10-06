//! HTTPS-only, Chrome's HTTPS-First mode ("Always use secure connections", stored in
//! [`keys::HTTPS_ONLY`]): an http navigation loads over https instead, and when the site
//! doesn't support that, the tab shows [`warning_page`] rather than the insecure page.
//!
//! Both shells feed each tab's [`Upgrades`] the main frame's navigation events and do what it
//! answers. "Continue to site" on the warning stores an exception for the site
//! ([`Permission::Http`] set to Allow), synced with the other site settings. Unlike Chrome's,
//! which lapse after 15 days, exceptions stay until the user removes them in Settings' site list.
//! One made in a private window lasts for the private session ([`crate::private`]).

use std::net::Ipv4Addr;

use url::Host;

use crate::html::escape;
use crate::permissions::{Origin, Permission, Setting};
use crate::prefs::keys;
use crate::private::Browsing;
use crate::{Error, Profile, Url};

/// The title of the Settings row (sentence case; GTK title-cases it itself).
pub const TITLE: &str = "Always use secure connections";
/// Under it in Settings. Chrome's wording.
pub const DESCRIPTION: &str = "Upgrade navigations to HTTPS and warn you before loading sites that don't support it";
/// The warning page's `<title>`.
pub const WARNING_TITLE: &str = "Connection is not secure";

/// Which http URLs get upgraded.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Reach {
    /// Sites on the internet, as Chrome: the default port, and no localhost, private or
    /// link-local address, or local name.
    #[default]
    Public,
    /// Every http URL, for the self-tests' local server.
    Everywhere,
}

/// Top-level names that only resolve on a local network, or never.
const LOCAL_TLDS: &[&str] = &["local", "localhost", "internal", "lan", "home", "corp", "test", "invalid", "example"];

/// The https URL `url` upgrades to when `reach` covers it.
pub fn upgraded(url: &Url, reach: Reach) -> Option<Url> {
    let host = url.host()?;
    let covered = match reach {
        Reach::Public => url.port().is_none() && public(host),
        Reach::Everywhere => true,
    };
    if url.scheme() != "http" || !covered {
        return None;
    }
    let mut https = url.clone();
    https.set_scheme("https").ok()?;
    Some(https)
}

fn public(host: Host<&str>) -> bool {
    match host {
        Host::Domain(domain) => {
            let domain = domain.strip_suffix('.').unwrap_or(domain);
            let Some((_, tld)) = domain.rsplit_once('.') else {
                return false;
            };
            !LOCAL_TLDS.contains(&tld) && domain != "home.arpa" && !domain.ends_with(".home.arpa")
        }
        Host::Ipv4(ip) => !(ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified() || shared(ip)),
        Host::Ipv6(ip) => !(ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local() || ip.is_unspecified()),
    }
}

/// 100.64.0.0/10, carrier-grade NAT.
fn shared(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && b & 0xc0 == 64
}

/// What a navigation to `url` loads instead: [`upgraded`], when the user turned HTTPS-only on
/// and has no exception for `url`'s site: stored, or in private, kept by the private session.
pub fn upgrade(p: &mut Profile, browsing: Browsing, url: &Url, reach: Reach) -> Option<Url> {
    if !p.prefs().get(&keys::HTTPS_ONLY) {
        return None;
    }
    let https = upgraded(url, reach)?;
    let origin = Origin::of(url)?;
    (!allowed(p, browsing, &origin)).then_some(https)
}

/// Remembers the exception for `url`'s site: what "Continue to site" does.
pub fn allow(p: &mut Profile, browsing: Browsing, url: &Url) -> Result<(), Error> {
    let Some(origin) = Origin::of(url) else {
        return Ok(());
    };
    p.site_permissions_in(browsing).set(&origin, Permission::Http, Some(Setting::Allow))
}

/// Whether the user made an exception for `origin`.
pub fn allowed(p: &mut Profile, browsing: Browsing, origin: &Origin) -> bool {
    p.site_permissions_in(browsing).get(origin, Permission::Http) == Some(Setting::Allow)
}

/// How a main-frame navigation started.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The page's own: a link the user followed (the warning's Continue is one).
    Link,
    /// A server redirect of the navigation under way.
    Redirect,
    /// The browser's: typed, reload, back/forward, ...
    Other,
}

/// What the shell does with a navigation that starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// Let it load.
    Load,
    /// Stop it and load this https URL instead.
    Upgrade(Url),
    /// Stop it and show [`warning_page`] for this http URL.
    Warn(Url),
    /// The warning's "Continue to site": remember the exception ([`allow`]) and let it load.
    Allow(Url),
}

/// The navigation the tab upgraded.
#[derive(Debug)]
struct Upgrading {
    /// The last http URL it upgraded.
    http: Url,
    /// What loads instead.
    https: Url,
    /// Every host upgraded along its redirects.
    hosts: Vec<String>,
}

/// The warning the tab shows, or (not `shown`) is about to show.
#[derive(Debug)]
struct Warning {
    url: Url,
    shown: bool,
}

/// One tab's HTTPS-only state, fed the main frame's navigation events by the shell.
#[derive(Debug, Default)]
pub struct Upgrades {
    upgrading: Option<Upgrading>,
    warning: Option<Warning>,
}

impl Upgrades {
    /// A main-frame navigation to `url` starts (also each redirect). `upgrade`: what
    /// [`upgrade`] says for `url`.
    pub fn starting(&mut self, url: &Url, cause: Cause, upgrade: Option<Url>) -> Next {
        if let Some(warning) = self.warning.as_ref().filter(|w| w.url == *url) {
            if !warning.shown {
                // The engine loading the warning page under the URL it warns about.
                return Next::Load;
            }
            if cause == Cause::Link {
                *self = Upgrades::default();
                return Next::Allow(url.clone());
            }
        }
        let upgrading = self.upgrading.take().filter(|u| cause == Cause::Redirect || u.https == *url);
        let Some(https) = upgrade else {
            self.upgrading = upgrading;
            return Next::Load;
        };
        let mut hosts = upgrading.map(|u| u.hosts).unwrap_or_default();
        let host = url.host_str().unwrap_or_default().to_owned();
        if hosts.contains(&host) {
            self.warning = Some(Warning { url: url.clone(), shown: false });
            return Next::Warn(url.clone());
        }
        hosts.push(host);
        self.upgrading = Some(Upgrading { http: url.clone(), https: https.clone(), hosts });
        Next::Upgrade(https)
    }

    /// The navigation under way ended: `ok` when it loaded. A failed upgrade gives the http URL
    /// to warn about.
    pub fn finished(&mut self, ok: bool) -> Option<Url> {
        let failed = self.upgrading.take().filter(|_| !ok)?;
        self.warning = Some(Warning { url: failed.http.clone(), shown: false });
        Some(failed.http)
    }

    /// A new document committed in the tab.
    pub fn committed(&mut self) {
        match &mut self.warning {
            Some(warning) if !warning.shown => warning.shown = true,
            _ => self.warning = None,
        }
    }

    /// The browser takes the tab elsewhere (typed, reload, back, forward): Continue on the
    /// warning no longer applies.
    pub fn leave(&mut self) {
        self.warning = None;
    }

    /// The http URL of the warning page the tab shows.
    pub fn warning(&self) -> Option<&Url> {
        self.warning.as_ref().filter(|w| w.shown).map(|w| &w.url)
    }
}

/// The warning page for `url`, an http URL. Engines load it under `url`, so it carries no
/// script and every interpolated string is escaped.
pub fn warning_page(url: &Url) -> String {
    let host = url.host_str().map_or_else(|| url.to_string(), crate::address::readable_host);
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="color-scheme" content="light dark">
<title>{title}</title>
<style>
  :root {{ font-family: system-ui, sans-serif; color: #1e1e1e; background: #fafafb; }}
  @media (prefers-color-scheme: dark) {{ :root {{ color: #eeeeee; background: #222226; }} }}
  body {{ max-width: 36rem; margin: 12vh auto; padding: 0 1.5rem; line-height: 1.5; }}
  h1 {{ font-size: 1.6rem; font-weight: 800; }}
  a.button {{ display: inline-block; margin-top: 1rem; padding: 0.5rem 1.1rem; border-radius: 6px;
             background: #3584e4; color: white; text-decoration: none; font-weight: 700; }}
</style>
</head>
<body>
<h1>This site doesn't support a secure connection</h1>
<p>Attackers might be able to see or change what you send to or receive from <strong>{host}</strong>.</p>
<p>You're seeing this warning because Always use secure connections is on, and the site doesn't offer a secure connection.</p>
<a id="continue" class="button" href="{url}">Continue to site</a>
</body>
</html>"#,
        title = escape(WARNING_TITLE),
        host = escape(&host),
        url = escape(url.as_str()),
    )
}
