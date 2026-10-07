//! The `private_window` check: a page in a private window leaves no history, session entry or
//! zoom behind, runs in WebView2's InPrivate profile without the probe extension, stays out of a
//! normal window's tab search, opens no extension popup, by default gets no third-party cookies
//! and no cookie rule in site info, and its download shows in private windows only; closing the
//! window ends its private session, and the log names none of its addresses.

use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use vsesvit_core::Url;
use vsesvit_core::cookies::ThirdPartyCookies;
use vsesvit_core::downloads::State;
use vsesvit_core::prefs::keys;
use vsesvit_core::private::Browsing;
use vsesvit_core::tab_search::Hit;
use vsesvit_core::testkit::{self, FixtureServer};
use windows_core::Interface;

use super::{DEFAULT_TIMEOUT, FIXTURE_TITLE, Probe, cookie_checks, emulate_zoom, eval, until};
use crate::bindings::ICoreWebView2_13;
use crate::browser::Browser;
use crate::popup::Activation;
use crate::session::{TabPlan, WindowPlan};
use crate::shortcuts::{self, Command};
use crate::{engine, exec, zoom};

/// In the query of every address the check loads in the private window, so the log can be
/// searched for them.
const MARK: &str = "private-window";

/// The probe's content script runs at `document_end`; this is ample for it to have marked the
/// page, had it run.
const CONTENT_SCRIPT_WAIT: Duration = Duration::from_millis(1500);

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(super) async fn private_window(
    browser: &Rc<Browser>,
    server: &FixtureServer,
    downloads: &Path,
    p: &Probe,
) -> Result<String, String> {
    let mut detail = Vec::new();
    let url = server.url(&format!("/index.html?{MARK}"));
    let plan = WindowPlan::with_tabs(vec![TabPlan::url(url.to_string())]);
    let private = browser
        .open_window(Browsing::Private, &plan, browser.show_mode())
        .map_err(err)?;
    let tab = until(p, |p| {
        p.observe("the private window has no tab yet");
        private.active_tab()
    })
    .await;
    until(p, |p| {
        let s = tab.state();
        p.observe(format!(
            "private tab at {:?} titled {:?}, loading={}",
            s.url,
            s.title,
            s.loading()
        ));
        (s.url == url.as_str() && s.title == FIXTURE_TITLE && !s.loading()).then_some(())
    })
    .await;

    let profile = tab
        .core()
        .ok_or("the private tab has no engine view")?
        .cast::<ICoreWebView2_13>()
        .and_then(|c| c.Profile())
        .map_err(err)?;
    let in_private = profile.IsInPrivateModeEnabled().map_err(err)?;
    let listed = match exec::timeout(DEFAULT_TIMEOUT, engine::extensions(&profile)).await {
        Some(Ok(list)) => format!(
            "{:?}",
            list.iter()
                .map(|e| format!("{} ({}, enabled={})", e.name, e.id, e.enabled))
                .collect::<Vec<_>>()
        ),
        Some(Err(e)) => format!("error {e}"),
        None => "no answer".to_owned(),
    };
    detail.push(format!(
        "InPrivate profile {in_private}; its GetBrowserExtensionsAsync lists {listed}"
    ));

    exec::sleep(CONTENT_SCRIPT_WAIT).await;
    let marked = eval(&tab, "document.documentElement.dataset.vsesvitProbe || null").await?;
    detail.push(format!("dataset.vsesvitProbe = {marked}"));

    let bindings = shortcuts::current();
    let shortcut = (0..)
        .map_while(|index| bindings.extension_action(index))
        .position(|id| id == testkit::PROBE_ID);
    if let Some(index) = shortcut {
        private.run(Command::ExtensionAction(index));
    }
    let shortcut_popup = private.extension_popup().is_some();
    let opened = private.open_extension_popup(testkit::PROBE_ID, Activation::Keep);
    let refusal = opened.as_ref().err().map(|e| e.message());
    if let Ok(popup) = &opened {
        popup.hide();
    }
    detail.push(format!(
        "the probe's action shortcut ({shortcut:?}) there opened a popup {shortcut_popup}; \
         opening its popup there: {refusal:?}"
    ));

    let in_history = browser
        .core(|c| c.history().visits_between(0, i64::MAX, 1000))
        .map_err(err)?
        .iter()
        .any(|(entry, _)| entry.url == url);
    let saved = browser.save_session_now();
    let in_session = browser
        .core(|c| c.session().restore())
        .map_err(err)?
        .is_some_and(|s| {
            s.windows
                .iter()
                .flat_map(|w| &w.tabs)
                .any(|t| t.url.as_str() == url.as_str())
        });
    detail.push(format!(
        "in history {in_history}; session saved {saved}, with the page {in_session}"
    ));

    let zoom_in =
        |b: Browsing, url: &Url| browser.core(|c| c.site_zoom(b).get(url)).unwrap_or(-1.0);
    emulate_zoom(&tab, private.scale(), Some(1.25)).await?;
    until(p, |p| {
        let (shown, kept) = (tab.state().zoom, zoom_in(Browsing::Private, &url));
        p.observe(format!("private tab zoomed to {}, its session keeps {kept}", shown.label()));
        (shown == zoom::Level(125) && kept == 1.25).then_some(())
    })
    .await;
    let stored_zoom = zoom_in(Browsing::Normal, &url);
    detail.push(format!("125% in the private tab; the stored zoom stays {stored_zoom}"));

    let hits = |b: Browsing| {
        browser
            .search_tabs(b, "fixture")
            .into_iter()
            .map(|(row, _)| row.hit)
            .collect::<Vec<_>>()
    };
    let normal_lists = hits(Browsing::Normal).contains(&Hit::Open(tab.id));
    let private_lists = hits(Browsing::Private).contains(&Hit::Open(tab.id));
    detail.push(format!(
        "tab search lists it in a normal window {normal_lists}, in the private one {private_lists}"
    ));

    let choice = browser.core(|c| c.prefs().get(&keys::THIRD_PARTY_COOKIES));
    tab.navigate(server.url("/cookies.html").as_str());
    until(p, |p| cookie_checks::ready(&tab, p)).await;
    let (_, frame) = cookie_checks::read(&tab).await?;
    let site_rule = private.cookies_status(&tab).is_some();
    detail.push(format!(
        "with {choice:?} its third-party frame read {frame:?}; a site-info cookie rule {site_rule}"
    ));

    let normal_windows = browser.windows_of(Browsing::Normal);
    let normal_button = || normal_windows.iter().any(|w| w.downloads_button_shown());
    let button_before = normal_button();
    let dir_before = browser.custom_download_dir();
    browser.set_download_dir(Some(downloads));
    tab.navigate(server.url(&format!("/download.bin?{MARK}")).as_str());
    let download = until(p, |p| {
        let row = browser
            .download_list(Browsing::Private)
            .into_iter()
            .find(|d| d.id.0 < 0);
        p.observe(format!("the private download's row is {row:?}"));
        row.filter(|d| d.state == State::Completed)
    })
    .await;
    browser.set_download_dir(dir_before.as_deref());
    let normal_lists = browser
        .download_list(Browsing::Normal)
        .iter()
        .any(|d| d.id == download.id);
    let (private_button, button_after) = (private.downloads_button_shown(), normal_button());
    detail.push(format!(
        "its download {} is listed for normal windows {normal_lists}; the downloads button \
         shows in it {private_button}, in normal windows {button_after} (before: {button_before})",
        download.path.display()
    ));

    let closed_before = closed_in(browser, Browsing::Normal);
    private.close_tab(tab.id);
    until(p, |p| {
        p.observe("the private window is still open");
        browser.windows_of(Browsing::Private).is_empty().then_some(())
    })
    .await;
    let closed_after = closed_in(browser, Browsing::Normal);
    let private_closed = browser.can_reopen_closed_tab(Browsing::Private);
    let zoom_after = zoom_in(Browsing::Private, &url);
    let gained = closed_after.difference(&closed_before).count();
    let download_rows = browser
        .download_list(Browsing::Private)
        .iter()
        .filter(|d| d.id.0 < 0)
        .count();
    let file_kept = download.path.is_file();
    detail.push(format!(
        "closed: private closed tabs left {private_closed}, normal ones gained {gained}, \
         its zoom now {zoom_after}, {download_rows} private download rows, its file kept {file_kept}"
    ));
    let log = std::fs::read_to_string(browser.config().log_file()).map_err(err)?;
    let logged: Vec<&str> = log.lines().filter(|line| line.contains(MARK)).collect();
    detail.push(format!("log lines naming its addresses: {logged:?}"));

    let ok = in_private
        && marked == "null"
        && shortcut.is_some()
        && !shortcut_popup
        && refusal.is_some()
        && !in_history
        && saved
        && !in_session
        && stored_zoom != 1.25
        && !normal_lists
        && private_lists
        && choice == ThirdPartyCookies::BlockInPrivate
        && !frame.contains("third=1")
        && !site_rule
        && !normal_lists
        && private_button
        && button_after == button_before
        && !private_closed
        && closed_after == closed_before
        && zoom_after == stored_zoom
        && download_rows == 0
        && file_kept
        && logged.is_empty();
    let detail = detail.join("; ");
    ok.then_some(detail.clone()).ok_or(detail)
}

/// The closed tabs a window of `browsing`'s kind lists, by when they closed.
fn closed_in(browser: &Browser, browsing: Browsing) -> HashSet<u64> {
    browser
        .search_tabs(browsing, "")
        .into_iter()
        .filter_map(|(row, _)| match row.hit {
            Hit::Closed(at) => Some(at),
            Hit::Open(_) => None,
        })
        .collect()
}
