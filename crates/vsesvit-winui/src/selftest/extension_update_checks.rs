//! The `extension_update` check: a Chrome Web Store extension updates in place through the
//! Extensions dialog's Update path, an update that asks for new permissions stays off until
//! the user re-enables it, and the page shows whether WebView2 kept the extension's chrome.storage
//! when it loaded each new version over the old one.

use std::rc::Rc;

use vsesvit_core::Url;
use vsesvit_core::extensions::{ExtensionId, InstallSource, Stores, UpdateReport};
use vsesvit_core::testkit::{CrxKey, FixtureServer, FixtureStore, update_probe_files};

use super::{DEFAULT_TIMEOUT, POLL, Probe, eval};
use crate::browser::Browser;
use crate::tab::Tab;
use crate::window::BrowserWindow;
use crate::{engine, exec};

pub(super) async fn extension_update(
    browser: &Rc<Browser>,
    window: &BrowserWindow,
    server: &FixtureServer,
    page: &Url,
    p: &Probe,
) -> Result<String, String> {
    let store = FixtureStore::start(server);
    browser.core(|c| c.set_stores(store.stores()));
    let developer = CrxKey::second();
    let mut seen = Vec::new();
    // What the page read wrong; the check goes on, so one run shows every version's answer.
    let mut problems = Vec::new();

    let id = store.publish_crx(&update_probe_files("1.0", &["storage"]), &developer);
    p.observe(format!("installing {} 1.0 from the store", id.as_str()));
    let ext = browser
        .install_extension(
            InstallSource::ChromeWebStore { id: id.clone() },
            &|progress| p.observe(&progress.text),
        )
        .await?;
    let engine = engine_state(browser, &id).await?;
    seen.push(format!(
        "installed {} {} ({:?}), {engine}",
        ext.id.as_str(),
        ext.version,
        ext.verification
    ));
    if ext.version != "1.0" || !engine.loaded(&id, true) {
        return Err(context(&seen, "1.0 is not running in the engine"));
    }
    let tab = window
        .open_url_tab(page.as_str(), true)
        .map_err(|e| context(&seen, e))?;
    let first = written(&tab, "1.0:", p).await;
    seen.push(format!("1.0 wrote {first}"));
    if first != "1.0:1.0" {
        problems.push("1.0 did not start its storage");
    }

    store.publish_crx(&update_probe_files("2.0", &["storage"]), &developer);
    p.observe("Update: checking the store");
    let report = browser
        .update_extensions()
        .await
        .map_err(|e| context(&seen, e))?;
    let engine = engine_state(browser, &id).await?;
    seen.push(format!(
        "Update to 2.0: {}; {engine}",
        describe(&report, &id)
    ));
    if !updated(&report, &id, "2.0", true) || !engine.loaded(&id, true) {
        return Err(context(&seen, "2.0 is not running in the engine"));
    }
    tab.reload();
    let second = written(&tab, "2.0:", p).await;
    seen.push(format!("after a reload 2.0 wrote {second}"));
    if second == "2.0:2.0" {
        problems
            .push("WebView2 dropped the extension's chrome.storage when it loaded 2.0 over 1.0");
    } else if second != "2.0:1.0" {
        problems.push("2.0 wrote neither 2.0:1.0 nor 2.0:2.0");
    }

    store.publish_crx(&update_probe_files("3.0", &["storage", "tabs"]), &developer);
    p.observe("Update: checking the store");
    let report = browser
        .update_extensions()
        .await
        .map_err(|e| context(&seen, e))?;
    let engine = engine_state(browser, &id).await?;
    seen.push(format!(
        "Update to 3.0: {}; {engine}",
        describe(&report, &id)
    ));
    let withheld: Vec<&str> = report
        .updated
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.withheld.iter().map(|w| w.text.as_str()).collect())
        .unwrap_or_default();
    if !updated(&report, &id, "3.0", false)
        || withheld != ["Read your browsing history"]
        || !engine.loaded(&id, false)
    {
        return Err(context(
            &seen,
            "3.0 is not held off in the engine until it is re-enabled",
        ));
    }

    p.observe("re-enabling 3.0");
    browser
        .approve_extension_permissions(&id)
        .await
        .map_err(|e| context(&seen, e))?;
    let approved = browser
        .core(|c| c.extensions().get(&id))
        .map_err(|e| e.to_string())?
        .map(|e| (e.enabled, e.withheld.is_empty()));
    let engine = engine_state(browser, &id).await?;
    seen.push(format!(
        "re-enabled: (enabled, nothing withheld) = {approved:?}; {engine}"
    ));
    if approved != Some((true, true)) || !engine.loaded(&id, true) {
        return Err(context(&seen, "Re-enable did not turn 3.0 back on"));
    }
    tab.reload();
    let third = written(&tab, "3.0:", p).await;
    seen.push(format!("after a reload 3.0 wrote {third}"));
    if third != "3.0:1.0" {
        problems.push("3.0 did not keep the storage 1.0 started");
    }

    browser
        .uninstall_extension(&id)
        .await
        .map_err(|e| context(&seen, e))?;
    let installed = browser
        .core(|c| c.extensions().get(&id))
        .map_err(|e| e.to_string())?
        .is_some();
    let engine = engine_state(browser, &id).await?;
    seen.push(format!("uninstalled: still in core {installed}, {engine}"));
    if installed || engine.listed.is_some() {
        problems.push("uninstalling left it behind");
    }
    if problems.is_empty() {
        Ok(seen.join("; "))
    } else {
        Err(context(&seen, problems.join("; ")))
    }
}

/// What the check saw, then what went wrong.
fn context(seen: &[String], problem: impl std::fmt::Display) -> String {
    format!("{}; {problem}", seen.join("; "))
}

/// Uninstalls the update probe if the check left it installed, and goes back to the real stores.
pub(super) async fn restore(browser: &Rc<Browser>) {
    let id = CrxKey::second().extension_id();
    if browser
        .core(|c| c.extensions().get(&id))
        .ok()
        .flatten()
        .is_some()
        && let Err(e) = browser.uninstall_extension(&id).await
    {
        log::warn!("uninstalling the update probe: {e}");
    }
    browser.core(|c| c.set_stores(Stores::default()));
}

fn updated(report: &UpdateReport, id: &ExtensionId, version: &str, enabled: bool) -> bool {
    report
        .updated
        .iter()
        .any(|e| e.id == *id && e.version == version && e.enabled == enabled)
}

fn describe(report: &UpdateReport, id: &ExtensionId) -> String {
    let ext = report.updated.iter().find(|e| e.id == *id);
    format!(
        "{:?}, {}",
        report.summary(),
        ext.map_or("not among the updated".to_owned(), |e| format!(
            "{} enabled={} withheld={:?}",
            e.version,
            e.enabled,
            e.withheld.iter().map(|w| &w.text).collect::<Vec<_>>()
        ))
    )
}

/// The engine id core recorded for an extension, and how WebView2 lists that extension.
struct EngineState {
    recorded: Option<String>,
    /// Whether it is enabled, if WebView2 lists it.
    listed: Option<bool>,
    error: Option<String>,
}

impl EngineState {
    /// Core recorded the extension's own id (every CRX install's) and WebView2 runs it, or not.
    fn loaded(&self, id: &ExtensionId, enabled: bool) -> bool {
        self.recorded.as_deref() == Some(id.as_str())
            && self.listed == Some(enabled)
            && self.error.is_none()
    }
}

impl std::fmt::Display for EngineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "engine id {:?}, ", self.recorded)?;
        match self.listed {
            Some(enabled) => write!(f, "WebView2 lists it enabled={enabled}")?,
            None => f.write_str("WebView2 lists nothing with that id")?,
        }
        match &self.error {
            Some(error) => write!(f, ", engine error: {error}"),
            None => Ok(()),
        }
    }
}

async fn engine_state(browser: &Browser, id: &ExtensionId) -> Result<EngineState, String> {
    let recorded = browser
        .core(|c| c.extensions().get(id))
        .map_err(|e| e.to_string())?
        .and_then(|e| e.engine_id);
    let profile = browser.engine_profile().await.ok_or("no engine profile")?;
    let loaded = exec::timeout(DEFAULT_TIMEOUT, engine::extensions(&profile))
        .await
        .ok_or("listing WebView2's extensions timed out")?
        .map_err(|e| e.to_string())?;
    Ok(EngineState {
        recorded,
        listed: loaded
            .iter()
            .find(|e| e.id == id.as_str())
            .map(|e| e.enabled),
        error: browser.extensions.engine_error(id),
    })
}

/// What the probe's content script wrote into the page, once it is from a version that starts
/// with `prefix`: a reload replaces what the version before wrote.
async fn written(tab: &Tab, prefix: &str, p: &Probe) -> String {
    loop {
        let value = eval(
            tab,
            "document.documentElement.dataset.vsesvitUpdateProbe || null",
        )
        .await;
        p.observe(format!("dataset.vsesvitUpdateProbe = {value:?}"));
        if let Ok(Ok(Some(answer))) = value.map(|v| serde_json::from_str::<Option<String>>(&v))
            && answer.starts_with(prefix)
        {
            return answer;
        }
        exec::sleep(POLL).await;
    }
}
