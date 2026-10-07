//! The `download_pause` and `download_safety` checks: a download pauses, resumes and is
//! cancelled through the engine, and a script waits under its unconfirmed name, marked as from
//! the Internet, until the warning's Keep moves it to its name or Discard deletes it.

use std::path::Path;

use vsesvit_core::Url;
use vsesvit_core::downloads::{Download, DownloadId, State, unconfirmed_path, zone_identifier};
use vsesvit_core::private::Browsing;
use vsesvit_core::testkit::STALLED_SENT;
use windows_core::Interface;

use super::{Probe, until};
use crate::automation::invoke;
use crate::bindings::{Button, UIElement};
use crate::browser::Browser;
use crate::downloads::Indicator;
use crate::tab::Tab;
use crate::window::BrowserWindow;
use crate::xaml;

/// What the fixture server sends for `/dangerous.bat`.
const SCRIPT: &str = "@echo Vsesvit fixture\r\n";

pub(super) async fn download_pause(
    browser: &Browser,
    tab: &Tab,
    stalled: &Url,
    p: &Probe,
) -> Result<String, String> {
    tab.navigate(stalled.as_str());
    let id = until(p, |p| {
        let entry = browser
            .download_list(Browsing::Normal)
            .into_iter()
            .find(|d| d.url == stalled.as_str())?;
        let live = browser.download_progress(entry.id);
        p.observe(format!("{:?}, live counts {live:?}", entry.state));
        live.is_some_and(|l| l.received >= STALLED_SENT as u64)
            .then_some(entry.id)
    })
    .await;
    browser.pause_download(id);
    reaches(browser, id, State::Paused, p).await;
    let paused = (
        browser.downloads_indicator(Browsing::Normal),
        browser.download_progress(id),
    );
    browser.resume_download(id);
    reaches(browser, id, State::InProgress, p).await;
    let resumed = browser.downloads_indicator(Browsing::Normal);
    browser.cancel_download(id);
    reaches(browser, id, State::Cancelled, p).await;
    let detail = format!(
        "Paused with toolbar {:?} and live counts {:?}; resumed with toolbar {resumed:?}; cancelled",
        paused.0, paused.1
    );
    let ok = paused.0 == Indicator::Idle
        && paused.1.is_some_and(|l| l.received >= STALLED_SENT as u64)
        && resumed == Indicator::Busy;
    ok.then_some(detail.clone()).ok_or(detail)
}

pub(super) async fn download_safety(
    browser: &Browser,
    window: &BrowserWindow,
    tab: &Tab,
    script: &Url,
    dir: &Path,
    p: &Probe,
) -> Result<String, String> {
    tab.navigate(script.as_str());
    let first = unconfirmed(browser, script, None, p).await;
    let waiting = unconfirmed_path(&first.path);
    let held =
        std::fs::read_to_string(&waiting).map_err(|e| format!("{}: {e}", waiting.display()))?;
    let held_at_name = first.path.exists();
    let warning = until(p, |p| {
        p.observe("waiting for the warning under the downloads button");
        // A flyout is open before its content is in the tree, where its buttons can be invoked.
        window
            .download_warning()
            .filter(|w| w.cast::<UIElement>().and_then(|w| w.XamlRoot()).is_ok())
    })
    .await;
    let keep = xaml::find::<Button>(&warning, "WarningKeep").map_err(|e| format!("Keep: {e}"))?;
    invoke(&keep).map_err(|e| format!("Keep: {e}"))?;
    reaches(browser, first.id, State::Completed, p).await;
    let kept = std::fs::read_to_string(&first.path)
        .map_err(|e| format!("{}: {e}", first.path.display()))?;
    let mark = std::fs::read_to_string(format!("{}:Zone.Identifier", first.path.display()))
        .map_err(|e| format!("the kept file's Zone.Identifier: {e}"))?;

    tab.navigate(script.as_str());
    let second = unconfirmed(browser, script, Some(first.id), p).await;
    let warning = until(p, |p| {
        p.observe("waiting for the second warning");
        window
            .download_warning()
            .filter(|w| w.cast::<UIElement>().and_then(|w| w.XamlRoot()).is_ok())
    })
    .await;
    let discard =
        xaml::find::<Button>(&warning, "WarningDiscard").map_err(|e| format!("Discard: {e}"))?;
    invoke(&discard).map_err(|e| format!("Discard: {e}"))?;
    until(p, |p| {
        let listed = browser
            .download_list(Browsing::Normal)
            .iter()
            .any(|d| d.id == second.id);
        p.observe(format!("the discarded entry listed: {listed}"));
        (!listed).then_some(())
    })
    .await;
    let discarded_gone = !unconfirmed_path(&second.path).exists() && !second.path.exists();

    let detail = format!(
        "{} waited as {} ({held:?}, at its name: {held_at_name}); Keep made it {kept:?} marked {mark:?} \
         by {}; the second, {}, was discarded (files gone: {discarded_gone})",
        first.path.display(),
        waiting.display(),
        if mark == zone_identifier(script.as_str()) {
            "Vsesvit"
        } else {
            "WebView2"
        },
        second.path.display(),
    );
    let ok = first.path == dir.join("dangerous.bat")
        && held == SCRIPT
        && !held_at_name
        && kept == SCRIPT
        && mark.contains("ZoneId=3")
        && second.path == dir.join("dangerous (1).bat")
        && discarded_gone;
    ok.then_some(detail.clone()).ok_or(detail)
}

/// The download of `url` that waits for the user, other than `seen`.
async fn unconfirmed(
    browser: &Browser,
    url: &Url,
    seen: Option<DownloadId>,
    p: &Probe,
) -> Download {
    until(p, |p| {
        let list = browser.download_list(Browsing::Normal);
        let entry = list
            .into_iter()
            .find(|d| d.url == url.as_str() && Some(d.id) != seen)?;
        p.observe(format!("{:?} {}", entry.state, entry.path.display()));
        (entry.state == State::Unconfirmed).then_some(entry)
    })
    .await
}

async fn reaches(browser: &Browser, id: DownloadId, state: State, p: &Probe) {
    until(p, |p| {
        let now = browser
            .download_list(Browsing::Normal)
            .into_iter()
            .find(|d| d.id == id)
            .map(|d| d.state);
        p.observe(format!(
            "download {} is {now:?}, waiting for {state:?}",
            id.0
        ));
        (now == Some(state)).then_some(())
    })
    .await;
}
