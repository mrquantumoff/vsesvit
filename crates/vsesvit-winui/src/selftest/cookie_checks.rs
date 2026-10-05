//! The `cookies` check, on the fixture page whose frame comes from `localhost`, another site
//! than the page's 127.0.0.1: the frame's cookie by default, gone when Settings blocks
//! third-party cookies and back when the page's site is set to Allow; a site set to Block whose
//! documents read no cookies and whose cookies, a response's included, are deleted after each
//! load; and Clear on exit's clearing.

use std::rc::Rc;

use vsesvit_core::cookies::{self as core_cookies, ThirdPartyCookies};
use vsesvit_core::permissions::{Origin, Setting};
use vsesvit_core::prefs::keys;
use vsesvit_core::testkit::FixtureServer;

use super::{POLL, Probe, eval, load, until};
use crate::bindings::CoreWebView2;
use crate::browser::Browser;
use crate::tab::Tab;
use crate::window::BrowserWindow;
use crate::{cookies, exec, permissions};

/// The fixture page's title once its frame has answered.
const READY: &str = "cookies:ready";
/// The fixture page's host.
const PAGE_HOST: &str = "127.0.0.1";
/// The fixture frame's host.
const FRAME_HOST: &str = "localhost";

pub(super) async fn cookies(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    p: &Probe,
) -> Result<String, String> {
    let page = server.url("/cookies.html");
    let site = Origin::parse(page.as_str()).ok_or("no fixture origin")?;
    let tab = window
        .open_url_tab(page.as_str(), true)
        .map_err(|e| e.to_string())?;
    until(p, |p| ready(&tab, p)).await;
    let core = tab.core().cloned().ok_or("the tab has no engine view")?;
    let (_, frame) = read(&tab).await?;
    if !frame.contains("third=1") {
        return Err(format!(
            "by default the {FRAME_HOST} frame read {frame:?}, not its third=1"
        ));
    }

    cookies::delete_cookies(&core, |site| site == FRAME_HOST)
        .await
        .map_err(|e| e.to_string())?;
    browser.write_pref(&keys::THIRD_PARTY_COOKIES, &ThirdPartyCookies::Block);
    cookies::changed(browser);
    reload(&tab, p).await?;
    let (page_cookies, frame) = read(&tab).await?;
    if frame.contains("third=1") || !page_cookies.contains("first=1") {
        return Err(format!(
            "with third-party cookies blocked the page read {page_cookies:?} and the frame {frame:?}"
        ));
    }

    set_rule(browser, &site, Some(Setting::Allow))?;
    reload(&tab, p).await?;
    let (_, frame) = read(&tab).await?;
    if !frame.contains("third=1") {
        return Err(format!(
            "with {} set to Allow the frame read {frame:?}",
            site.as_str()
        ));
    }

    set_rule(browser, &site, Some(Setting::Block))?;
    reload(&tab, p).await?;
    let (page_cookies, _) = read(&tab).await?;
    if !page_cookies.is_empty() {
        return Err(format!(
            "with {} set to Block the page read {page_cookies:?}",
            site.as_str()
        ));
    }
    until_none(&core, p).await?;
    let served = server.url("/set-cookie");
    load(&tab, served.as_str(), p).await;
    until_none(&core, p).await?;

    set_rule(browser, &site, Some(Setting::ClearOnExit))?;
    load(&tab, page.as_str(), p).await;
    until(p, |p| ready(&tab, p)).await;
    let before = names_on(&core).await?;
    if before.is_empty() {
        return Err(format!(
            "with {} set to Clear on exit the page stored no cookie",
            site.as_str()
        ));
    }
    let rules = browser.core(core_cookies::site_rules);
    cookies::clear_sites(&core, &rules)
        .await
        .map_err(|e| e.to_string())?;
    let after = names_on(&core).await?;
    let detail = format!(
        "the {FRAME_HOST} frame got its cookie by default, not with third-party cookies blocked, and again with {} set to Allow; set to Block, the page read none and its cookies, /set-cookie's included, were deleted; set to Clear on exit, clearing deleted {before:?}, leaving {after:?}",
        site.as_str()
    );
    after.is_empty().then_some(detail.clone()).ok_or(detail)
}

/// Back to the default choice and no rule for the fixture site, whether or not the check passed.
pub(super) fn restore(browser: &Rc<Browser>, server: &FixtureServer) {
    if let Err(e) = browser.core(|c| c.prefs().reset(&keys::THIRD_PARTY_COOKIES)) {
        log::warn!("{}: {e}", keys::THIRD_PARTY_COOKIES.key);
    }
    match Origin::parse(&server.origin()) {
        Some(site) => {
            if let Err(e) = set_rule(browser, &site, None) {
                log::warn!("{e}");
            }
        }
        None => cookies::changed(browser),
    }
}

fn set_rule(browser: &Rc<Browser>, site: &Origin, setting: Option<Setting>) -> Result<(), String> {
    browser
        .core(|c| core_cookies::set(c, site, setting))
        .map_err(|e| format!("cookies for {}: {e}", site.as_str()))?;
    permissions::settings_changed(browser);
    Ok(())
}

fn ready(tab: &Tab, p: &Probe) -> Option<()> {
    let s = tab.state();
    p.observe(format!("at {:?}, titled {:?}", s.url, s.title));
    (s.title == READY && !s.loading()).then_some(())
}

/// Loads the page again, under the cookie controls in effect.
async fn reload(tab: &Rc<Tab>, p: &Probe) -> Result<(), String> {
    eval(tab, "document.title = 'reloading'").await?;
    until(p, |_| (tab.state().title == "reloading").then_some(())).await;
    tab.reload();
    until(p, |p| ready(tab, p)).await;
    Ok(())
}

/// The cookies the page and its frame read.
async fn read(tab: &Tab) -> Result<(String, String), String> {
    let json = eval(tab, "[cookieResults.page, cookieResults.frame]").await?;
    serde_json::from_str(&json).map_err(|e| format!("cookieResults {json}: {e}"))
}

/// The names of the cookies on the fixture page's host.
async fn names_on(core: &CoreWebView2) -> Result<Vec<String>, String> {
    let all = cookies::all_cookies(core)
        .await
        .map_err(|e| e.to_string())?;
    Ok(all
        .into_iter()
        .filter(|c| c.site() == PAGE_HOST)
        .map(|c| c.name)
        .collect())
}

/// Waits until the fixture page's host has no cookies.
async fn until_none(core: &CoreWebView2, p: &Probe) -> Result<(), String> {
    loop {
        let names = names_on(core).await?;
        if names.is_empty() {
            return Ok(());
        }
        p.observe(format!("{PAGE_HOST} still has cookies {names:?}"));
        exec::sleep(POLL).await;
    }
}
