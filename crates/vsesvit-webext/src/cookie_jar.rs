//! `chrome.cookies` over the network session's `CookieManager` (see [`crate::cookies`] for
//! what each call reaches). As in Chrome, the API needs the `cookies` permission, and a call
//! reaches a cookie only where the extension has host permissions for it: for the call's
//! `url`, and for each cookie `getAll` lists or `onChanged` reports. `activeTab` grants none.
//!
//! The shell's cookie rules come first: an extension cannot set a cookie for a site the user
//! blocked ([`crate::TabHost::cookies_blocked`]), which Chrome answers as any cookie it cannot
//! store. Third-party blocking does not apply, as it does not to Chrome's API.

use std::rc::Rc;

use serde_json::{Value, json};
use webkit::{glib, soup};

use crate::bridge::{self, Reply};
use crate::cookies::{self, Cookie, SameSite, Store};
use crate::extension::Extension;
use crate::protocol::{Call, Method};
use crate::runtime::Inner;

pub(crate) fn call(inner: &Rc<Inner>, ext: &Rc<Extension>, call: &Call, reply: Reply) {
    if !ext.has_permission("cookies") {
        return reply.finish(Err(format!("{} requires the \"cookies\" permission", call.method)));
    }
    if call.method == Method::CookiesGetAllCookieStores {
        return reply.finish(Ok(Some(stores(inner))));
    }
    let (inner, ext, method, details) = (inner.clone(), ext.clone(), call.method, call.arg(0).clone());
    glib::spawn_future_local(async move { reply.finish(run(&inner, &ext, method, &details).await) });
}

/// Each store with an open tab, as Chrome lists them. Private windows' store ("1") joins with
/// their tabs for an extension allowed in them, as [`manager`] reaches it.
fn stores(inner: &Inner) -> Value {
    let tabs: Vec<u32> = inner.host.tabs().iter().map(|t| t.id.0).collect();
    if tabs.is_empty() {
        return json!([]);
    }
    json!([{ "id": Store::Normal.id(), "tabIds": tabs }])
}

/// The cookie manager behind `store`. Private windows have none yet; theirs is to answer for
/// an extension allowed in them while one is open, and be an invalid store id otherwise, as
/// in Chrome.
fn manager(inner: &Inner, store: Store) -> Result<webkit::CookieManager, String> {
    match store {
        Store::Normal => inner.session.cookie_manager(),
        Store::Private => None,
    }
    .ok_or_else(|| cookies::invalid_store(store.id()))
}

/// The store's cookies, each with WebKit's own to delete it by.
async fn read(manager: &webkit::CookieManager) -> Result<Vec<(soup::Cookie, Cookie)>, String> {
    let jar = manager.all_cookies_future().await.map_err(|e| format!("cookies: {e}"))?;
    Ok(jar.into_iter().map(|mut c| (c.clone(), from_soup(&mut c))).collect())
}

fn from_soup(c: &mut soup::Cookie) -> Cookie {
    let text = |s: Option<glib::GString>| s.map(String::from).unwrap_or_default();
    Cookie {
        name: text(c.name()),
        value: text(c.value()),
        domain: text(c.domain()),
        path: text(c.path()),
        secure: c.is_secure(),
        http_only: c.is_http_only(),
        same_site: match c.same_site_policy() {
            soup::SameSitePolicy::None => SameSite::NoRestriction,
            soup::SameSitePolicy::Strict => SameSite::Strict,
            _ => SameSite::Lax,
        },
        expires: c.expires().map(|at| at.to_unix() as f64),
    }
}

fn to_soup(c: &Cookie) -> soup::Cookie {
    let mut cookie = soup::Cookie::new(&c.name, &c.value, &c.domain, &c.path, -1);
    cookie.set_secure(c.secure);
    cookie.set_http_only(c.http_only);
    cookie.set_same_site_policy(match c.same_site {
        SameSite::NoRestriction => soup::SameSitePolicy::None,
        SameSite::Lax => soup::SameSitePolicy::Lax,
        SameSite::Strict => soup::SameSitePolicy::Strict,
    });
    if let Some(at) = c.expires.and_then(|at| glib::DateTime::from_unix_utc(at as i64).ok()) {
        cookie.set_expires(&at);
    }
    cookie
}

async fn run(inner: &Inner, ext: &Extension, method: Method, details: &Value) -> Result<Option<Value>, String> {
    let url = details.get("url").and_then(Value::as_str).map(cookies::parse_url).transpose()?;
    // getAll judges each cookie by its own URL instead.
    if method != Method::CookiesGetAll
        && let Some(url) = &url
        && !ext.host_access(url.as_str(), None)
    {
        return Err(cookies::no_host_permissions(url));
    }
    let store = Store::of(details)?.unwrap_or(Store::Normal);
    let manager = manager(inner, store)?;
    let unpartitioned = cookies::reaches_unpartitioned(&details["partitionKey"])?;
    if method == Method::CookiesGetAll {
        let jar = if unpartitioned { read(&manager).await? } else { Vec::new() };
        let reached = jar
            .iter()
            .map(|(_, c)| c)
            .filter(|c| url.as_ref().is_none_or(|u| c.applies_to(u)) && cookies::matches_filter(c, details) && ext.host_access(&c.url(), None));
        return Ok(Some(Value::Array(reached.map(|c| c.to_json(store)).collect())));
    }
    let url = url.ok_or_else(|| format!("{method}: details.url must be a string"))?;
    let name = details.get("name").and_then(Value::as_str);
    match method {
        Method::CookiesGet => {
            let name = name.ok_or("cookies.get: details.name must be a string")?;
            let jar: Vec<Cookie> = if unpartitioned { read(&manager).await?.into_iter().map(|(_, c)| c).collect() } else { Vec::new() };
            Ok(Some(cookies::first_named(&jar, &url, name).map_or(Value::Null, |c| c.to_json(store))))
        }
        Method::CookiesSet => {
            let failed = || cookies::set_failed(name.unwrap_or_default());
            let now = bridge::now_ms() / 1000.0;
            let cookie = Cookie::to_set(&url, details, now).filter(|c| unpartitioned && !inner.host.cookies_blocked(&c.domain)).ok_or_else(failed)?;
            manager.add_cookie_future(&to_soup(&cookie)).await.map_err(|_| failed())?;
            // An expired cookie deletes the one it replaces and is not kept.
            if cookie.expired(now) {
                return Ok(None);
            }
            let stored = read(&manager).await?.into_iter().map(|(_, c)| c).find(|c| c.key() == cookie.key());
            stored.map(|c| Some(c.to_json(store))).ok_or_else(failed)
        }
        Method::CookiesRemove => {
            let name = name.ok_or("cookies.remove: details.name must be a string")?;
            let jar = if unpartitioned { read(&manager).await? } else { Vec::new() };
            for (soup, _) in jar.iter().filter(|(_, c)| c.name == name && c.applies_to(&url)) {
                manager.delete_cookie_future(soup).await.map_err(|e| format!("cookies: {e}"))?;
            }
            let mut removed = json!({ "name": name, "url": url.as_str(), "storeId": store.id() });
            if let Some(key) = details.get("partitionKey").filter(|k| !k.is_null()) {
                removed["partitionKey"] = key.clone();
            }
            Ok(Some(removed))
        }
        _ => unreachable!("not a cookies call"),
    }
}

/// What `cookies.onChanged` was last told from. WebKit says only that the store changed, so
/// the runtime reads it again and fires the difference; changes while it reads are read
/// together once it is done.
#[derive(Default)]
pub(crate) struct Watch {
    known: Option<Vec<Cookie>>,
    reading: bool,
    again: bool,
}

/// The normal store changed, or an extension that may hear of it loaded. With no such
/// extension loaded nothing is read, and the last reading is forgotten: the first reading
/// after one loads fires nothing.
pub(crate) fn changed(inner: &Rc<Inner>) {
    let listening = inner.loaded_extensions().iter().any(|e| e.has_permission("cookies"));
    {
        let mut watch = inner.cookie_watch.borrow_mut();
        if !listening {
            watch.known = None;
            return;
        }
        if watch.reading {
            watch.again = true;
            return;
        }
        watch.reading = true;
    }
    let Some(manager) = inner.session.cookie_manager() else { return };
    let weak = Rc::downgrade(inner);
    glib::spawn_future_local(async move {
        let jar = read(&manager).await;
        let Some(inner) = weak.upgrade() else { return };
        let (changes, again) = {
            let mut watch = inner.cookie_watch.borrow_mut();
            watch.reading = false;
            let changes = match jar {
                Ok(jar) => {
                    let now: Vec<Cookie> = jar.into_iter().map(|(_, c)| c).collect();
                    let before = watch.known.replace(now);
                    let after = watch.known.as_deref().unwrap_or_default();
                    before.map(|before| cookies::changes(&before, after, bridge::now_ms() / 1000.0)).unwrap_or_default()
                }
                Err(e) => {
                    log::warn!("{e}");
                    Vec::new()
                }
            };
            (changes, std::mem::take(&mut watch.again))
        };
        for change in changes {
            let (url, args) = (change.cookie.url(), [change.to_json(Store::Normal)]);
            for ext in inner.loaded_extensions().iter().filter(|e| e.has_permission("cookies") && e.host_access(&url, None)) {
                bridge::emit_to_pages(&inner, ext, "cookies.onChanged", &args);
            }
        }
        if again {
            changed(&inner);
        }
    });
}
