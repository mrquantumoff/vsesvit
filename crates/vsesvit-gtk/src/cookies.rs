//! Cookie controls on Linux (`vsesvit_core::cookies`). WebKitGTK has one cookie accept policy
//! per network session, so Settings' third-party cookies choice blocks them on every site or
//! none, and a site's Allow cannot lift it: site info offers Allow only when sync brought it.
//!
//! A site set to Block gets a content-blocker rule that keeps requests to it from sending or
//! storing cookies, attached to every tab's `UserContentManager` beside tracking protection's,
//! and core's script that hides `document.cookie` from its documents. The data of sites set to
//! Block or Clear on exit is deleted when Vsesvit closes and again when it starts.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::cookies::{self, Browsing, SiteRules};
use vsesvit_core::permissions::Origin;
use vsesvit_core::{Profile, Url};

use crate::blocker::Blocker;
use crate::browser::Browser;
use crate::profile::Core;
use crate::tab::Tab;

/// How long quitting waits for the data of sites set to Clear on exit to be deleted.
const EXIT_CLEAR_WAIT: Duration = Duration::from_secs(3);

/// Cheap to clone; every clone is the same state.
#[derive(Clone)]
pub(crate) struct Cookies(Rc<Inner>);

struct Inner {
    core: Core,
    session: webkit::NetworkSession,
    blocker: Blocker,
    script: RefCell<Option<webkit::UserScript>>,
    /// The deletion [`Cookies::clear_at_start`] began is still running.
    clearing: Cell<bool>,
    waiters: RefCell<Vec<Box<dyn FnOnce()>>>,
}

/// The accept policy for a network session of `browsing` windows. Private windows' ephemeral
/// session passes [`Browsing::Private`].
pub(crate) fn accept_policy(profile: &mut Profile, browsing: Browsing) -> webkit::CookieAcceptPolicy {
    if cookies::third_party_blocked(profile, browsing, None) {
        webkit::CookieAcceptPolicy::NoThirdParty
    } else {
        webkit::CookieAcceptPolicy::Always
    }
}

impl Cookies {
    /// The compiled blocker is kept in the profile's `cookies` folder.
    pub(crate) fn new(core: Core, session: &webkit::NetworkSession) -> Cookies {
        let dir = core.borrow().paths().root.join("cookies");
        Cookies(Rc::new(Inner {
            core,
            session: session.clone(),
            blocker: Blocker::new(&dir, "vsesvit-cookie-rules"),
            script: RefCell::new(None),
            clearing: Cell::new(false),
            waiters: RefCell::new(Vec::new()),
        }))
    }

    /// A new tab's manager: it gets the rules now and every change after.
    pub(crate) fn attach(&self, manager: &webkit::UserContentManager) {
        self.0.blocker.attach(manager);
        if let Some(script) = self.0.script.borrow().as_ref() {
            manager.add_script(script);
        }
    }

    /// Brings the session's accept policy and every tab's rules in line with the profile, and
    /// deletes the cookies sites set to Block still have. Compiling the rules is asynchronous;
    /// [`Cookies::when_applied`] runs once they are on the tabs.
    pub(crate) fn apply(&self) {
        let (policy, rules) = {
            let mut profile = self.0.core.borrow_mut();
            (accept_policy(&mut profile, Browsing::Normal), cookies::site_rules(&mut profile))
        };
        if let Some(manager) = self.0.session.cookie_manager() {
            manager.set_accept_policy(policy);
        }
        self.0.blocker.apply(content_blocker(rules.blocked_hosts()));
        let script = cookies::block_script(rules.blocked_hosts()).map(|source| {
            webkit::UserScript::new(&source, webkit::UserContentInjectedFrames::AllFrames, webkit::UserScriptInjectionTime::Start, &[], &[])
        });
        let old = self.0.script.replace(script.clone());
        for manager in self.0.blocker.managers() {
            if let Some(old) = &old {
                manager.remove_script(old);
            }
            if let Some(script) = &script {
                manager.add_script(script);
            }
        }
        if !rules.blocked_hosts().is_empty() {
            glib::spawn_future_local(delete_blocked_cookies(self.0.session.clone(), rules));
        }
    }

    /// Runs `f` once the latest [`Cookies::apply`] is on every tab and the deletion at start is
    /// done (at once when both are).
    pub(crate) fn when_applied(&self, f: impl FnOnce() + 'static) {
        let weak = Rc::downgrade(&self.0);
        self.0.blocker.when_applied(move || match weak.upgrade() {
            Some(inner) if inner.clearing.get() => inner.waiters.borrow_mut().push(Box::new(f)),
            _ => f(),
        });
    }

    /// Deletes the data of the sites set to Block or Clear on exit, in case the last run did not
    /// close cleanly. Pages wait for it through [`Cookies::when_applied`].
    pub(crate) fn clear_at_start(&self) {
        self.0.clearing.set(true);
        let weak = Rc::downgrade(&self.0);
        let clear = self.clear();
        glib::spawn_future_local(async move {
            clear.await;
            let Some(inner) = weak.upgrade() else { return };
            inner.clearing.set(false);
            for waiter in std::mem::take(&mut *inner.waiters.borrow_mut()) {
                waiter();
            }
        });
    }

    /// Deletes the data of the sites set to Block or Clear on exit as Vsesvit closes, waiting up
    /// to [`EXIT_CLEAR_WAIT`] for WebKit.
    pub(crate) fn clear_at_exit(&self) {
        let clear = self.clear();
        if glib::MainContext::default().block_on(glib::future_with_timeout(EXIT_CLEAR_WAIT, clear)).is_err() {
            log::info!("cookies: the data of sites to clear on exit was not deleted in {EXIT_CLEAR_WAIT:?}");
        }
    }

    /// Deletes every kind of data WebKit keeps for the sites core's rules clear.
    pub(crate) fn clear(&self) -> impl Future<Output = ()> + use<> {
        let rules = cookies::site_rules(&mut self.0.core.borrow_mut());
        let manager = self.0.session.website_data_manager();
        async move {
            let Some(manager) = manager.filter(|_| !rules.to_clear().is_empty()) else { return };
            let records = match manager.fetch_future(webkit::WebsiteDataTypes::ALL).await {
                Ok(records) => records,
                Err(e) => {
                    log::warn!("cookies: listing site data: {e}");
                    return;
                }
            };
            let cleared: Vec<webkit::WebsiteData> = records.into_iter().filter(|r| r.name().is_some_and(|name| rules.clears(&name))).collect();
            if let Err(e) = remove_data(&manager, webkit::WebsiteDataTypes::ALL, &cleared).await {
                log::warn!("cookies: deleting site data: {e}");
            }
        }
    }
}

/// Deletes the cookies that reach the hosts `rules` blocks, which a site had before it was set to
/// Block.
async fn delete_blocked_cookies(session: webkit::NetworkSession, rules: SiteRules) {
    let Some(manager) = session.cookie_manager() else { return };
    let cookies = match manager.all_cookies_future().await {
        Ok(cookies) => cookies,
        Err(e) => {
            log::warn!("cookies: listing cookies: {e}");
            return;
        }
    };
    for mut cookie in cookies {
        let blocked = cookie.domain().is_some_and(|domain| rules.blocks_cookie(&domain));
        if blocked && let Err(e) = manager.delete_cookie_future(&cookie).await {
            log::warn!("cookies: deleting a cookie: {e}");
        }
    }
}

/// `WebsiteDataManager::remove`, awaited. Nothing to remove succeeds at once.
pub(crate) async fn remove_data(manager: &webkit::WebsiteDataManager, types: webkit::WebsiteDataTypes, records: &[webkit::WebsiteData]) -> Result<(), glib::Error> {
    if records.is_empty() {
        return Ok(());
    }
    let (done, removed) = futures_channel::oneshot::channel();
    let records: Vec<&webkit::WebsiteData> = records.iter().collect();
    manager.remove(types, &records, None::<&gio::Cancellable>, move |result| {
        let _ = done.send(result);
    });
    removed.await.unwrap_or(Ok(()))
}

/// WebKit's content-blocker JSON keeping requests to `hosts` and their subdomains from sending
/// or storing cookies, one rule per host: WebKit's patterns have no alternation. `None` when
/// nothing is blocked.
fn content_blocker(hosts: &[String]) -> Option<String> {
    if hosts.is_empty() {
        return None;
    }
    let rules: Vec<serde_json::Value> = hosts
        .iter()
        .map(|host| {
            let host: String = host.chars().flat_map(|c| if matches!(c, '.' | '[' | ']') { vec!['\\', c] } else { vec![c] }).collect();
            serde_json::json!({
                "trigger": { "url-filter": format!("^[^:]+://([^/:]*\\.)?{host}[:/]") },
                "action": { "type": "block-cookies" },
            })
        })
        .collect();
    Some(serde_json::Value::Array(rules).to_string())
}

/// The site-info popover's choice of what `tab`'s site may do with cookies, with what it means
/// on this page under it. `None` for a page that is not from a website.
pub(crate) fn site_info_section(browser: &Browser, tab: &Tab) -> Option<gtk::ListBox> {
    let url = tab.committed_uri().and_then(|uri| Url::parse(&uri).ok())?;
    let origin = Origin::of(&url).filter(|_| matches!(url.scheme(), "http" | "https"))?;
    let (current, blocked) = {
        let mut profile = browser.core().borrow_mut();
        (cookies::setting(&mut profile, &origin), cookies::third_party_blocked(&mut profile, Browsing::Normal, None))
    };
    let choices = cookies::site_choices(current, false);
    let labels: Vec<&str> = choices.iter().map(|&c| cookies::choice_label(c)).collect();
    let row = adw::ComboRow::builder()
        .title(cookies::SITE_TITLE)
        .subtitle(cookies::site_status(blocked, current))
        .model(&gtk::StringList::new(&labels))
        .selected(choices.iter().position(|&c| c == current).and_then(|i| u32::try_from(i).ok()).unwrap_or(0))
        .build();
    row.connect_selected_notify(glib::clone!(
        #[strong]
        browser,
        #[weak]
        tab,
        move |row| {
            let Some(&chosen) = choices.get(row.selected() as usize) else { return };
            row.set_subtitle(cookies::site_status(blocked, chosen));
            if let Err(e) = cookies::set(&mut browser.core().borrow_mut(), &origin, chosen) {
                log::warn!("cookies: {e}");
                return;
            }
            browser.cookies().apply();
            // A reload would send the request again as the old rules shaped it.
            browser.cookies().when_applied(glib::clone!(
                #[weak]
                tab,
                move || {
                    if let Some(uri) = tab.committed_uri() {
                        tab.load(&uri);
                    }
                }
            ));
        }
    ));
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
    list.append(&row);
    Some(list)
}

/// What a site-data record holds, as the list in Settings says it: "Cookies, site storage".
pub(crate) fn holds(types: webkit::WebsiteDataTypes) -> String {
    use webkit::WebsiteDataTypes as T;
    let kinds = [
        (T::COOKIES, "cookies"),
        (T::LOCAL_STORAGE | T::SESSION_STORAGE | T::INDEXEDDB_DATABASES | T::DOM_CACHE | T::OFFLINE_APPLICATION_CACHE, "site storage"),
        (T::SERVICE_WORKER_REGISTRATIONS, "service workers"),
        (T::DISK_CACHE | T::MEMORY_CACHE, "cached files"),
    ];
    let held: Vec<&str> = kinds.iter().filter(|(kind, _)| types.intersects(*kind)).map(|(_, name)| *name).collect();
    let text = if held.is_empty() { "other site data".to_owned() } else { held.join(", ") };
    text[..1].to_uppercase() + &text[1..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_say_what_they_hold() {
        use webkit::WebsiteDataTypes as T;
        assert_eq!(holds(T::COOKIES | T::LOCAL_STORAGE), "Cookies, site storage");
        assert_eq!(holds(T::INDEXEDDB_DATABASES | T::DISK_CACHE), "Site storage, cached files");
        assert_eq!(holds(T::HSTS_CACHE), "Other site data");
    }

    #[gtk::test]
    fn each_blocked_host_compiles_to_a_rule_of_its_own() {
        assert_eq!(content_blocker(&[]), None);
        let hosts = ["127.0.0.1", "example.com", "[::1]"].map(str::to_owned);
        let json = content_blocker(&hosts).expect("hosts are blocked");
        let rules: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(rules[1]["trigger"]["url-filter"], r"^[^:]+://([^/:]*\.)?example\.com[:/]");
        assert_eq!(rules[2]["trigger"]["url-filter"], r"^[^:]+://([^/:]*\.)?\[::1\][:/]");

        let store = webkit::UserContentFilterStore::new(&crate::test_support::scratch_dir("cookie-rules").to_string_lossy());
        let bytes = glib::Bytes::from_owned(json.into_bytes());
        let compiled = glib::MainContext::default().block_on(store.save_future("vsesvit-cookie-rules", &bytes));
        assert!(compiled.is_ok(), "{compiled:?}");
    }
}
