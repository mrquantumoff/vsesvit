//! `chrome.cookies` as Chrome's API shows a cookie store. Pure: the runtime reads WebKit's
//! cookies into [`Cookie`]s, and this module says which ones a call reaches, what
//! `cookies.set` stores (Chrome's `CanonicalCookie::CreateSanitizedCookie` rules) and which
//! `cookies.onChanged` events two readings of the store make.
//!
//! WebKitGTK keeps no partitioned cookies, so a `partitionKey` naming a top-level site
//! reaches none, and it has no `SameSite` "unspecified": such a cookie is stored as `lax`,
//! which is how Chrome treats it.

use serde_json::{Value, json};
use url::Url;

/// Chrome's `kCookieSetFailedError`: what `cookies.set` answers for a cookie it cannot store.
pub fn set_failed(name: &str) -> String {
    format!("Failed to parse or set cookie named \"{name}\".")
}

/// Chrome caps a cookie's lifetime at 400 days.
const MAX_AGE_SECONDS: f64 = 400.0 * 24.0 * 60.0 * 60.0;
/// Chrome's limit on a cookie's name and value together, and on its path and domain.
const MAX_NAME_VALUE: usize = 4096;
const MAX_ATTRIBUTE: usize = 1024;

/// A cookie store: the normal windows' one ("0"), or private windows' ("1") for an extension
/// allowed in them.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Store {
    Normal,
    Private,
}

impl Store {
    pub const fn id(self) -> &'static str {
        match self {
            Store::Normal => "0",
            Store::Private => "1",
        }
    }

    /// The store a call's `details.storeId` names: `None` for the caller's own, which for
    /// every extension context here is the normal one.
    pub fn of(details: &Value) -> Result<Option<Store>, String> {
        match &details["storeId"] {
            Value::Null => Ok(None),
            Value::String(id) if id == "0" => Ok(Some(Store::Normal)),
            Value::String(id) if id == "1" => Ok(Some(Store::Private)),
            other => Err(invalid_store(other.as_str().unwrap_or_default())),
        }
    }
}

/// Chrome's `kInvalidStoreIdError`, also for a store that exists but the caller cannot reach.
pub fn invalid_store(id: &str) -> String {
    format!("Invalid cookie store id: \"{id}\".")
}

/// The URL a call names, which Chrome requires to parse.
pub fn parse_url(raw: &str) -> Result<Url, String> {
    Url::parse(raw).map_err(|_| format!("Invalid url: \"{raw}\"."))
}

/// Chrome's `kNoHostPermissionsError`.
pub fn no_host_permissions(url: &Url) -> String {
    format!("No host permissions for cookies at url: \"{url}\".")
}

/// Whether a call's `partitionKey` reaches unpartitioned cookies, the only ones WebKitGTK
/// keeps: without one, or without a top-level site, it does; naming a site, it reaches only
/// that site's partitioned cookies. Errors are Chrome's for a malformed key.
pub fn reaches_unpartitioned(key: &Value) -> Result<bool, String> {
    if key.is_null() {
        return Ok(true);
    }
    let cross_site = key.get("hasCrossSiteAncestor").and_then(Value::as_bool);
    match key.get("topLevelSite").and_then(Value::as_str) {
        None if cross_site.is_some() => Err("CookiePartitionKey.topLevelSite unexpectedly not present.".into()),
        None => Ok(true),
        Some("") if cross_site == Some(true) => Err("partitionKey with empty topLevelSite unexpectedly has a cross-site ancestor value of true.".into()),
        Some("") => Ok(true),
        Some(site) => match Url::parse(site) {
            Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => Ok(false),
            _ => Err("Invalid value for CookiePartitionKey.topLevelSite.".into()),
        },
    }
}

/// `cookies.SameSiteStatus`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SameSite {
    NoRestriction,
    Lax,
    Strict,
}

impl SameSite {
    pub const fn name(self) -> &'static str {
        match self {
            SameSite::NoRestriction => "no_restriction",
            SameSite::Lax => "lax",
            SameSite::Strict => "strict",
        }
    }
}

/// One cookie, as WebKit stores it and `chrome.cookies.Cookie` shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// With a leading dot for a domain cookie, the bare host for a host-only one.
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
    /// Seconds since the epoch; `None` for a session cookie.
    pub expires: Option<f64>,
}

impl Cookie {
    pub fn host_only(&self) -> bool {
        !self.domain.starts_with('.')
    }

    fn host(&self) -> &str {
        self.domain.trim_start_matches('.')
    }

    /// The URL Chrome judges host permissions for the cookie by (`CookieOriginToURL`).
    pub fn url(&self) -> String {
        format!("{}://{}/", if self.secure { "https" } else { "http" }, self.host())
    }

    /// Whether a request to `url` would send the cookie: its domain, path and `secure` match.
    pub fn applies_to(&self, url: &Url) -> bool {
        let Some(host) = url.host_str() else { return false };
        let domain_matches = if self.host_only() { host == self.domain } else { host == self.host() || host.ends_with(&self.domain) };
        let path = url.path();
        let path_matches = path == self.path || (path.starts_with(&self.path) && (self.path.ends_with('/') || path[self.path.len()..].starts_with('/')));
        domain_matches && path_matches && (!self.secure || is_secure(url))
    }

    /// Two readings of the store hold the same cookie when these agree; any other difference
    /// is a new cookie in its place.
    pub fn key(&self) -> (&str, &str, &str) {
        (&self.name, &self.domain, &self.path)
    }

    pub fn expired(&self, now: f64) -> bool {
        self.expires.is_some_and(|at| at <= now)
    }

    /// `chrome.cookies.Cookie` in `store`.
    pub fn to_json(&self, store: Store) -> Value {
        let mut cookie = json!({
            "name": self.name,
            "value": self.value,
            "domain": self.domain,
            "hostOnly": self.host_only(),
            "path": self.path,
            "secure": self.secure,
            "httpOnly": self.http_only,
            "sameSite": self.same_site.name(),
            "session": self.expires.is_none(),
            "storeId": store.id(),
        });
        if let Some(at) = self.expires {
            cookie["expirationDate"] = json!(at);
        }
        cookie
    }

    /// The cookie `cookies.set(details)` stores for `url` at `now`, or `None` where Chrome's
    /// `CreateSanitizedCookie` refuses one (answered with [`set_failed`]).
    pub fn to_set(url: &Url, details: &Value, now: f64) -> Option<Cookie> {
        let text = |key: &str| details.get(key).and_then(Value::as_str).unwrap_or_default();
        let flag = |key: &str| details.get(key).and_then(Value::as_bool).unwrap_or(false);
        let (name, value) = (text("name"), text("value"));
        let valid = |s: &str, forbidden: &[char]| !s.chars().any(|c| c.is_ascii_control() || forbidden.contains(&c));
        if !valid(name, &[';', '=']) || !valid(value, &[';']) || (name.is_empty() && value.is_empty()) || name.len() + value.len() > MAX_NAME_VALUE {
            return None;
        }
        if !matches!(url.scheme(), "http" | "https" | "ws" | "wss") {
            return None;
        }
        let host = url.host_str()?;
        let domain = match text("domain").trim_start_matches('.').to_ascii_lowercase() {
            given if given.is_empty() => host.to_owned(),
            given if given.len() > MAX_ATTRIBUTE => return None,
            // An IP address has no subdomains, so its cookie stays host-only.
            given if matches!(url.host(), Some(url::Host::Ipv4(_) | url::Host::Ipv6(_))) => (given == host).then(|| host.to_owned())?,
            given if host == given || host.ends_with(&format!(".{given}")) => format!(".{given}"),
            _ => return None,
        };
        let path = match text("path") {
            "" => default_path(url),
            given if given.starts_with('/') && given.len() <= MAX_ATTRIBUTE => given.to_owned(),
            _ => return None,
        };
        let secure = flag("secure");
        let same_site = match details.get("sameSite").and_then(Value::as_str) {
            None | Some("unspecified") | Some("lax") => SameSite::Lax,
            Some("no_restriction") => SameSite::NoRestriction,
            Some("strict") => SameSite::Strict,
            Some(_) => return None,
        };
        let lower = name.to_ascii_lowercase();
        let host_prefix = lower.starts_with("__host-");
        let needs_secure = same_site == SameSite::NoRestriction || host_prefix || lower.starts_with("__secure-");
        if (secure && !is_secure(url)) || (needs_secure && !secure) || (host_prefix && (!text("domain").is_empty() || path != "/")) {
            return None;
        }
        let expires = details.get("expirationDate").and_then(Value::as_f64).map(|at| at.min(now + MAX_AGE_SECONDS));
        Some(Cookie { name: name.to_owned(), value: value.to_owned(), domain, path, secure, http_only: flag("httpOnly"), same_site, expires })
    }
}

fn is_secure(url: &Url) -> bool {
    matches!(url.scheme(), "https" | "wss")
}

/// RFC 6265's default path: the URL's path up to its last `/`, or `/`.
fn default_path(url: &Url) -> String {
    match url.path().rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(i) => url.path()[..i].to_owned(),
    }
}

/// The cookie `cookies.get` answers with from `cookies` that a request to `url` sends: the
/// first named `name` in Chrome's order (longest path first).
pub fn first_named<'a>(cookies: &'a [Cookie], url: &Url, name: &str) -> Option<&'a Cookie> {
    let mut sent: Vec<&Cookie> = cookies.iter().filter(|c| c.name == name && c.applies_to(url)).collect();
    sent.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
    sent.first().copied()
}

/// `cookies.getAll(details)`'s filter, besides its `url` and host permissions.
pub fn matches_filter(cookie: &Cookie, details: &Value) -> bool {
    let text = |key: &str| details.get(key).and_then(Value::as_str);
    let flag = |key: &str| details.get(key).and_then(Value::as_bool);
    text("name").is_none_or(|n| n == cookie.name)
        && text("domain").is_none_or(|d| {
            let d = d.trim_start_matches('.');
            cookie.host() == d || cookie.host().ends_with(&format!(".{d}"))
        })
        && text("path").is_none_or(|p| p == cookie.path)
        && flag("secure").is_none_or(|s| s == cookie.secure)
        && flag("session").is_none_or(|s| s == cookie.expires.is_none())
}

/// `cookies.OnChangedCause`, as far as two readings of the store can tell it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    Explicit,
    Overwrite,
    Expired,
}

impl Cause {
    pub const fn name(self) -> &'static str {
        match self {
            Cause::Explicit => "explicit",
            Cause::Overwrite => "overwrite",
            Cause::Expired => "expired",
        }
    }
}

/// One `cookies.onChanged` event.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub removed: bool,
    pub cause: Cause,
    pub cookie: Cookie,
}

impl Change {
    pub fn to_json(&self, store: Store) -> Value {
        json!({ "removed": self.removed, "cause": self.cause.name(), "cookie": self.cookie.to_json(store) })
    }
}

/// The events between two readings of the store, as Chrome fires them: a cookie replaced by
/// another of its name, domain and path is removed (`overwrite`) and then added
/// (`explicit`); a cookie gone is removed, `expired` when its time had come.
pub fn changes(before: &[Cookie], after: &[Cookie], now: f64) -> Vec<Change> {
    let find = |list: &[Cookie], c: &Cookie| list.iter().position(|o| o.key() == c.key());
    let mut out = Vec::new();
    for old in before {
        match find(after, old) {
            Some(i) if after[i] == *old => {}
            Some(_) => out.push(Change { removed: true, cause: Cause::Overwrite, cookie: old.clone() }),
            None => out.push(Change { removed: true, cause: if old.expired(now) { Cause::Expired } else { Cause::Explicit }, cookie: old.clone() }),
        }
    }
    for new in after {
        if find(before, new).is_none_or(|i| before[i] != *new) {
            out.push(Change { removed: false, cause: Cause::Explicit, cookie: new.clone() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_800_000_000.0;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn cookie(name: &str, domain: &str, path: &str) -> Cookie {
        Cookie { name: name.into(), value: "v".into(), domain: domain.into(), path: path.into(), secure: false, http_only: false, same_site: SameSite::Lax, expires: None }
    }

    fn set(u: &str, details: Value) -> Option<Cookie> {
        Cookie::to_set(&url(u), &details, NOW)
    }

    #[test]
    fn set_fills_in_chromes_defaults() {
        let c = set("https://www.example.com/a/b?q", json!({ "name": "n", "value": "v" })).unwrap();
        assert_eq!((c.domain.as_str(), c.path.as_str(), c.secure, c.http_only, c.same_site, c.expires), ("www.example.com", "/a", false, false, SameSite::Lax, None));
        assert!(c.host_only());
        assert_eq!(set("http://x.test", json!({ "name": "n" })).unwrap().path, "/");
        assert_eq!(set("http://x.test/a", json!({ "name": "n" })).unwrap().path, "/");
        assert_eq!(set("http://x.test/a/", json!({ "name": "n" })).unwrap().path, "/a");
    }

    #[test]
    fn set_takes_a_domain_the_url_is_in_and_makes_it_a_domain_cookie() {
        let c = set("https://www.example.com/", json!({ "name": "n", "domain": "example.com", "path": "/p" })).unwrap();
        assert_eq!((c.domain.as_str(), c.path.as_str(), c.host_only()), (".example.com", "/p", false));
        assert_eq!(set("https://www.example.com/", json!({ "name": "n", "domain": ".WWW.example.com" })).unwrap().domain, ".www.example.com");
        assert_eq!(set("https://www.example.com/", json!({ "name": "n", "domain": "other.com" })), None);
        assert_eq!(set("https://example.com/", json!({ "name": "n", "domain": "www.example.com" })), None);
        let ip = set("http://127.0.0.1:8080/", json!({ "name": "n", "domain": "127.0.0.1" })).unwrap();
        assert_eq!(ip.domain, "127.0.0.1", "an IP address's cookie is host-only");
        assert_eq!(set("http://127.0.0.1/", json!({ "name": "n", "domain": "0.0.1" })), None);
    }

    #[test]
    fn set_refuses_what_chrome_refuses() {
        for (u, details) in [
            ("https://x.test/", json!({})),
            ("https://x.test/", json!({ "name": "a;b" })),
            ("https://x.test/", json!({ "name": "a=b" })),
            ("https://x.test/", json!({ "name": "n", "value": "a;b" })),
            ("https://x.test/", json!({ "name": "n", "value": "a\u{7}" })),
            ("https://x.test/", json!({ "name": "n", "value": "x".repeat(4096) })),
            ("https://x.test/", json!({ "name": "n", "path": "relative" })),
            ("https://x.test/", json!({ "name": "n", "sameSite": "sideways" })),
            ("http://x.test/", json!({ "name": "n", "secure": true })),
            ("https://x.test/", json!({ "name": "n", "sameSite": "no_restriction" })),
            ("https://x.test/", json!({ "name": "__Secure-n" })),
            ("https://x.test/", json!({ "name": "__Host-n", "secure": true, "domain": "x.test" })),
            ("https://x.test/a/", json!({ "name": "__host-n", "secure": true })),
            ("ftp://x.test/", json!({ "name": "n" })),
            ("data:text/plain,x", json!({ "name": "n" })),
        ] {
            assert_eq!(set(u, details.clone()), None, "{u} {details}");
        }
        assert!(set("https://x.test/", json!({ "name": "n", "sameSite": "no_restriction", "secure": true })).is_some());
        assert!(set("https://x.test/", json!({ "name": "__Host-n", "secure": true })).is_some());
        assert!(set("https://x.test/", json!({ "value": "only" })).is_some(), "a nameless cookie with a value is allowed");
    }

    #[test]
    fn set_keeps_the_expiry_within_400_days() {
        let c = set("https://x.test/", json!({ "name": "n", "expirationDate": NOW + 60.0, "sameSite": "strict", "httpOnly": true, "secure": true })).unwrap();
        assert_eq!((c.expires, c.same_site, c.http_only, c.secure), (Some(NOW + 60.0), SameSite::Strict, true, true));
        assert_eq!(set("https://x.test/", json!({ "name": "n", "expirationDate": NOW * 2.0 })).unwrap().expires, Some(NOW + MAX_AGE_SECONDS));
        assert!(set("https://x.test/", json!({ "name": "n", "expirationDate": 0 })).unwrap().expired(NOW));
    }

    #[test]
    fn cookies_apply_to_their_domain_path_and_scheme() {
        let host_only = cookie("n", "example.com", "/");
        assert!(host_only.applies_to(&url("http://example.com/x")));
        assert!(!host_only.applies_to(&url("http://www.example.com/")));
        let domain = cookie("n", ".example.com", "/docs");
        assert!(domain.applies_to(&url("http://www.example.com/docs")));
        assert!(domain.applies_to(&url("http://example.com/docs/a")));
        assert!(!domain.applies_to(&url("http://example.com/docsx")));
        assert!(!domain.applies_to(&url("http://example.com/")));
        assert!(!domain.applies_to(&url("http://badexample.com/docs")));
        let secure = Cookie { secure: true, ..cookie("n", "example.com", "/") };
        assert!(!secure.applies_to(&url("http://example.com/")));
        assert!(secure.applies_to(&url("https://example.com/")));
        assert_eq!(secure.url(), "https://example.com/");
        assert_eq!(domain.url(), "http://example.com/");
    }

    #[test]
    fn get_answers_the_longest_path_first() {
        let list = [cookie("n", "x.test", "/"), cookie("n", "x.test", "/a"), cookie("m", "x.test", "/a/b")];
        assert_eq!(first_named(&list, &url("http://x.test/a/b"), "n").unwrap().path, "/a");
        assert_eq!(first_named(&list, &url("http://x.test/"), "n").unwrap().path, "/");
        assert_eq!(first_named(&list, &url("http://y.test/"), "n"), None);
    }

    #[test]
    fn get_all_filters_as_chrome_does() {
        let c = Cookie { expires: Some(NOW), secure: true, ..cookie("n", ".www.example.com", "/p") };
        assert!(matches_filter(&c, &json!({})));
        assert!(matches_filter(&c, &json!({ "domain": "example.com", "name": "n", "path": "/p", "secure": true, "session": false })));
        assert!(matches_filter(&c, &json!({ "domain": ".www.example.com" })));
        assert!(!matches_filter(&c, &json!({ "domain": "sub.www.example.com" })));
        assert!(!matches_filter(&c, &json!({ "domain": "ample.com" })));
        assert!(!matches_filter(&c, &json!({ "path": "/" })));
        assert!(!matches_filter(&c, &json!({ "session": true })));
        assert!(!matches_filter(&c, &json!({ "secure": false })));
    }

    #[test]
    fn cookies_show_as_chrome_cookies() {
        let session = cookie("n", "x.test", "/").to_json(Store::Normal);
        assert_eq!(
            session,
            json!({ "name": "n", "value": "v", "domain": "x.test", "hostOnly": true, "path": "/", "secure": false, "httpOnly": false, "sameSite": "lax", "session": true, "storeId": "0" })
        );
        let lasting = Cookie { expires: Some(NOW), ..cookie("n", ".x.test", "/") }.to_json(Store::Private);
        assert_eq!((lasting["expirationDate"].as_f64(), &lasting["session"], &lasting["hostOnly"], &lasting["storeId"]), (Some(NOW), &json!(false), &json!(false), &json!("1")));
    }

    #[test]
    fn stores_and_partition_keys_parse_as_chrome_parses_them() {
        assert_eq!(Store::of(&json!({})), Ok(None));
        assert_eq!(Store::of(&json!({ "storeId": "0" })), Ok(Some(Store::Normal)));
        assert_eq!(Store::of(&json!({ "storeId": "1" })), Ok(Some(Store::Private)));
        assert_eq!(Store::of(&json!({ "storeId": "2" })), Err("Invalid cookie store id: \"2\".".into()));
        assert_eq!(parse_url("nope"), Err("Invalid url: \"nope\".".into()));

        assert_eq!(reaches_unpartitioned(&Value::Null), Ok(true));
        assert_eq!(reaches_unpartitioned(&json!({})), Ok(true));
        assert_eq!(reaches_unpartitioned(&json!({ "topLevelSite": "" })), Ok(true));
        assert_eq!(reaches_unpartitioned(&json!({ "topLevelSite": "https://x.test" })), Ok(false));
        assert!(reaches_unpartitioned(&json!({ "hasCrossSiteAncestor": false })).unwrap_err().contains("topLevelSite unexpectedly not present"));
        assert!(reaches_unpartitioned(&json!({ "topLevelSite": "", "hasCrossSiteAncestor": true })).unwrap_err().contains("empty topLevelSite"));
        assert!(reaches_unpartitioned(&json!({ "topLevelSite": "not a site" })).unwrap_err().contains("topLevelSite"));
    }

    #[test]
    fn changes_are_chromes_events() {
        let a = cookie("a", "x.test", "/");
        let b = cookie("b", "x.test", "/");
        let b2 = Cookie { value: "w".into(), ..b.clone() };
        let gone = Cookie { expires: Some(NOW - 1.0), ..cookie("c", "x.test", "/") };
        let ev = |removed, cause, cookie: &Cookie| Change { removed, cause, cookie: cookie.clone() };
        let just_a = std::slice::from_ref(&a);
        assert_eq!(changes(just_a, just_a, NOW), vec![]);
        assert_eq!(changes(&[], just_a, NOW), vec![ev(false, Cause::Explicit, &a)]);
        assert_eq!(changes(&[a.clone(), gone.clone()], &[], NOW), vec![ev(true, Cause::Explicit, &a), ev(true, Cause::Expired, &gone)]);
        assert_eq!(changes(&[a.clone(), b.clone()], &[b2.clone(), a.clone()], NOW), vec![ev(true, Cause::Overwrite, &b), ev(false, Cause::Explicit, &b2)]);
        assert_eq!(ev(true, Cause::Overwrite, &b).to_json(Store::Normal), json!({ "removed": true, "cause": "overwrite", "cookie": b.to_json(Store::Normal) }));
    }
}
