//! Cookie controls (`vsesvit_core::cookies`) in WebView2, which has no API for them: each tab
//! applies them through its DevTools sessions, the page's own and each attached frame's (see
//! `shortcuts::AUTO_ATTACH`), set up before their first document runs.
//!
//! - Third-party cookies: `Network.setCookieControls` in every session, which acts only with
//!   the Network domain on. In the page's session alone it blocks only HTTP cookies; frame
//!   sessions block `document.cookie` in third-party frames too. The main frame sets it again as
//!   each navigation starts, for the site it goes to, so a site set to Allow keeps them.
//! - A site set to Block: WebView2 cannot keep a response from storing cookies (`Fetch` hides
//!   `Set-Cookie` from response headers and ignores a request's removed `Cookie`), so the site's
//!   documents run `cookies::block_script`, and its cookies are deleted when the rule is set,
//!   after each page load, and at startup. A cookie it sets over HTTP lives until then.
//! - Clear on exit: the data of the sites to clear goes at startup, before any page loads (see
//!   `Engine::set_up_profile`), and when the last window closes, as far as the engine gets before
//!   the process ends.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use vsesvit_core::cookies::{self, Browsing, SiteRules};
use vsesvit_core::permissions::Origin;
use windows_core::{HSTRING, Result};
use windows_future::IAsyncOperation;

use crate::bindings::*;
use crate::browser::Browser;
use crate::exec;
use crate::tab::{call_in, devtools_in};

/// The Network domain on, buffering no bodies: nothing here reads them.
const NETWORK_ENABLE: &str = r#"{"maxTotalBufferSize":0,"maxResourceBufferSize":0}"#;

/// How long closing the browser waits for the engine to clear sites.
const EXIT_CLEARING: Duration = Duration::from_secs(2);

fn cookie_controls(blocked: bool) -> String {
    json!({ "enableThirdPartyCookieRestriction": blocked }).to_string()
}

/// Each DevTools session of a tab (`""` for the page's own) and its block script.
type Scripts = Rc<RefCell<HashMap<String, Script>>>;

/// The block script a session's documents run (`cookies::block_script`).
#[derive(Default)]
struct Script {
    /// Counts replacements, so an add answered after a later one knows it is out of date.
    generation: u64,
    /// The engine's identifier of the script, once its add has answered; `None` for none.
    identifier: Option<String>,
}

/// Cookie controls in one tab's DevTools sessions.
#[derive(Default)]
pub(crate) struct Sessions {
    /// Whether third-party cookies are blocked on the page, as the sessions were last told.
    blocked: Cell<bool>,
    scripts: Scripts,
}

impl Sessions {
    /// Sets up `session` before its documents run: the Network domain, the page's third-party
    /// choice, and `script` (`cookies::block_script`).
    pub async fn set_up(
        &self,
        core: &CoreWebView2,
        session: &str,
        script: Option<&str>,
    ) -> Result<()> {
        self.scripts
            .borrow_mut()
            .insert(session.to_owned(), Script::default());
        devtools_in(core, session, "Network.enable", NETWORK_ENABLE).await?;
        let controls = cookie_controls(self.blocked.get());
        devtools_in(core, session, "Network.setCookieControls", &controls).await?;
        replace_script(&self.scripts, core, session, script)?.await
    }

    pub fn detached(&self, session: &str) {
        self.scripts.borrow_mut().remove(session);
    }

    /// The main frame starts loading a page on which third-party cookies are `blocked` or not.
    /// The calls go out before the engine's event returns, ahead of the page's requests.
    pub fn navigation_starting(&self, core: &CoreWebView2, blocked: bool) {
        if self.blocked.get() != blocked {
            self.apply(core, blocked, None);
        }
    }

    /// Applies a changed choice or rules in every session, from its next document: third-party
    /// cookies `blocked` or not, and `script` as the block script. The calls go out at once,
    /// ahead of a reload that follows.
    pub fn apply_all(&self, core: &CoreWebView2, blocked: bool, script: Option<&str>) {
        self.apply(core, blocked, Some(script));
    }

    /// Tells every session whether third-party cookies are `blocked`, and replaces its block
    /// script with `script` when that is `Some`.
    fn apply(&self, core: &CoreWebView2, blocked: bool, script: Option<Option<&str>>) {
        self.blocked.set(blocked);
        let controls = cookie_controls(blocked);
        let sessions: Vec<String> = self.scripts.borrow().keys().cloned().collect();
        for session in sessions {
            let calls =
                call_in(core, &session, "Network.setCookieControls", &controls).and_then(|sent| {
                    let replaced = script
                        .map(|source| replace_script(&self.scripts, core, &session, source))
                        .transpose()?;
                    Ok((sent, replaced))
                });
            exec::spawn(async move {
                let applied = async {
                    let (sent, replaced) = calls?;
                    sent.await?;
                    if let Some(replaced) = replaced {
                        replaced.await?;
                    }
                    Ok::<_, windows_core::Error>(())
                }
                .await;
                // A frame's session can end meanwhile.
                if let Err(e) = applied {
                    log::debug!("cookie controls in session {session:?}: {e}");
                }
            });
        }
    }
}

/// Replaces the block script of `session` with `source`, if any. The add and the old script's
/// removal go out at once, in that order, so the next document runs exactly the new one; the
/// returned future records the new script's identifier, or removes the script again if a later
/// replacement or the session's end came first.
fn replace_script(
    scripts: &Scripts,
    core: &CoreWebView2,
    session: &str,
    source: Option<&str>,
) -> Result<impl Future<Output = Result<()>> + 'static> {
    let added = source
        .map(|source| {
            let params = json!({ "source": source }).to_string();
            call_in(
                core,
                session,
                "Page.addScriptToEvaluateOnNewDocument",
                &params,
            )
        })
        .transpose()?;
    let (generation, old) = match scripts.borrow_mut().get_mut(session) {
        Some(slot) => {
            slot.generation += 1;
            (slot.generation, slot.identifier.take())
        }
        None => (0, None),
    };
    let removed = old
        .map(|old| remove_script(core, session, &old))
        .transpose()?;
    let (scripts, core, session) = (scripts.clone(), core.clone(), session.to_owned());
    Ok(async move {
        if let Some(removed) = removed {
            removed.await?;
        }
        let Some(added) = added else {
            return Ok(());
        };
        let Some(identifier) = serde_json::from_str::<Value>(&added.await?.to_string_lossy())
            .ok()
            .and_then(|v| v["identifier"].as_str().map(str::to_owned))
        else {
            return Ok(());
        };
        let stale = match scripts.borrow_mut().get_mut(&session) {
            Some(slot) if slot.generation == generation => {
                slot.identifier = Some(identifier.clone());
                false
            }
            Some(_) => true,
            None => false,
        };
        if stale {
            remove_script(&core, &session, &identifier)?.await?;
        }
        Ok(())
    })
}

fn remove_script(
    core: &CoreWebView2,
    session: &str,
    identifier: &str,
) -> Result<IAsyncOperation<HSTRING>> {
    let params = json!({ "identifier": identifier }).to_string();
    call_in(
        core,
        session,
        "Page.removeScriptToEvaluateOnNewDocument",
        &params,
    )
}

/// Whether third-party cookies are blocked on a page of `top`.
pub(crate) fn third_party_blocked(browser: &Browser, top: Option<&Origin>) -> bool {
    browser.core(|p| cookies::third_party_blocked(p, Browsing::Normal, top))
}

/// The script the documents of every session run (`cookies::block_script`), if any site is set
/// to Block.
pub(crate) fn block_script(browser: &Browser) -> Option<String> {
    browser.core(|p| cookies::block_script(cookies::site_rules(p).blocked_hosts()))
}

/// Applies the stored choice and rules again in every tab, from each page's next load, and
/// deletes the cookies of the sites set to Block: after a change here, in Settings or from a
/// sync.
pub(crate) fn changed(browser: &Browser) {
    let rules = browser.core(cookies::site_rules);
    let script = cookies::block_script(rules.blocked_hosts());
    for tab in browser.windows().iter().flat_map(|w| w.tabs_in_order()) {
        tab.apply_cookies(
            third_party_blocked(browser, tab.origin().as_ref()),
            script.as_deref(),
        );
    }
    delete_blocked(browser, rules);
}

/// Deletes the cookies of the sites `rules` sets to Block, if any.
pub(crate) fn delete_blocked(browser: &Browser, rules: SiteRules) {
    if rules.blocked_hosts().is_empty() {
        return;
    }
    let Some(core) = any_core(browser) else {
        return;
    };
    exec::spawn(async move {
        match delete_cookies(&core, |domain| rules.blocks_cookie(domain)).await {
            Ok(0) => {}
            Ok(n) => log::info!("deleted {n} cookie(s) of sites set to Block"),
            Err(e) => log::warn!("deleting the cookies of sites set to Block: {e}"),
        }
    });
}

/// The engine view of any open tab, for calls that reach the whole profile.
pub(crate) fn any_core(browser: &Browser) -> Option<CoreWebView2> {
    browser
        .windows()
        .iter()
        .flat_map(|w| w.tabs_in_order())
        .find_map(|t| t.core().cloned())
}

/// A cookie as `Network.getAllCookies` lists it.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Cookie {
    pub name: String,
    pub domain: String,
    pub path: String,
    /// Set for a partitioned cookie, which deleting must name.
    #[serde(rename = "partitionKey")]
    pub partition_key: Option<Value>,
}

impl Cookie {
    /// The domain without its leading dot.
    pub fn site(&self) -> &str {
        self.domain.strip_prefix('.').unwrap_or(&self.domain)
    }
}

/// `Network.getAllCookies`' result.
fn parse_cookies(json: &str) -> Vec<Cookie> {
    #[derive(Deserialize)]
    struct All {
        cookies: Vec<Cookie>,
    }
    serde_json::from_str::<All>(json).map_or_else(|_| Vec::new(), |all| all.cookies)
}

/// Every cookie in the profile.
pub(crate) async fn all_cookies(core: &CoreWebView2) -> Result<Vec<Cookie>> {
    Ok(parse_cookies(
        &devtools_in(core, "", "Network.getAllCookies", "{}").await?,
    ))
}

/// Deletes the cookies whose site (`Cookie::site`) `which` picks; how many it deleted.
pub(crate) async fn delete_cookies(
    core: &CoreWebView2,
    which: impl Fn(&str) -> bool,
) -> Result<usize> {
    let picked: Vec<Cookie> = all_cookies(core)
        .await?
        .into_iter()
        .filter(|c| which(c.site()))
        .collect();
    for cookie in &picked {
        let mut params =
            json!({ "name": cookie.name, "domain": cookie.domain, "path": cookie.path });
        if let Some(key) = &cookie.partition_key {
            params["partitionKey"] = key.clone();
        }
        devtools_in(core, "", "Network.deleteCookies", &params.to_string()).await?;
    }
    Ok(picked.len())
}

/// Starts deleting the cookies and storage of each origin `rules` clears
/// (`SiteRules::to_clear`); the calls go out at once.
fn start_clearing(core: &CoreWebView2, rules: &SiteRules) -> Result<Vec<IAsyncOperation<HSTRING>>> {
    rules
        .to_clear()
        .iter()
        .map(|origin| {
            let params = json!({ "origin": origin.as_str(), "storageTypes": "all" });
            call_in(core, "", "Storage.clearDataForOrigin", &params.to_string())
        })
        .collect()
}

/// Deletes the data of the sites `rules` clears: each origin's, then the cookies under their
/// domains that leaves, such as a subdomain's.
pub(crate) async fn clear_sites(core: &CoreWebView2, rules: &SiteRules) -> Result<()> {
    if rules.to_clear().is_empty() {
        return Ok(());
    }
    for call in start_clearing(core, rules)? {
        call.await?;
    }
    delete_cookies(core, |domain| rules.clears(domain)).await?;
    log::info!("cleared the data of {} site(s)", rules.to_clear().len());
    Ok(())
}

/// Clearing the sites to clear as the browser closes, started through a tab still open.
#[derive(Default)]
pub(crate) struct ExitClearing(Vec<IAsyncOperation<HSTRING>>);

impl ExitClearing {
    pub fn start(browser: &Browser) -> Self {
        let rules = browser.core(cookies::site_rules);
        let Some(core) = any_core(browser).filter(|_| !rules.to_clear().is_empty()) else {
            return Self::default();
        };
        log::info!(
            "clearing the data of {} site(s) on exit",
            rules.to_clear().len()
        );
        Self(start_clearing(&core, &rules).unwrap_or_else(|e| {
            log::warn!("clearing site data on exit: {e}");
            Vec::new()
        }))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Waits for the engine to finish, a bounded while: its web views may be gone already.
    pub async fn finish(self) {
        let finished = exec::timeout(EXIT_CLEARING, async {
            for call in self.0 {
                call.await?;
            }
            Ok::<_, windows_core::Error>(())
        })
        .await;
        match finished {
            Some(Ok(())) => log::info!("cleared site data on exit"),
            Some(Err(e)) => log::warn!("clearing site data on exit: {e}"),
            None => log::warn!("clearing site data on exit: no answer from the engine"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engines_cookie_list_is_read() {
        let json = r#"{"cookies":[
            {"name":"a","value":"1","domain":".e.test","path":"/","secure":false},
            {"name":"b","value":"2","domain":"e.test","path":"/x","partitionKey":{"topLevelSite":"https://t.test","hasCrossSiteAncestor":true}}]}"#;
        let cookies = parse_cookies(json);
        assert_eq!(cookies.len(), 2);
        assert_eq!(
            (cookies[0].site(), cookies[0].path.as_str()),
            ("e.test", "/")
        );
        assert!(cookies[0].partition_key.is_none());
        assert_eq!(cookies[1].site(), "e.test");
        assert!(cookies[1].partition_key.is_some());
        assert!(parse_cookies("not json").is_empty());
    }
}
