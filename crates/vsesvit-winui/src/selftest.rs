//! `vsesvit --self-test OUT_DIR [--network]`: the end-to-end contract of docs/design/self-test.md.
//!
//! It drives the same code paths as the UI: the address box submits the fixture URL, the star
//! button bookmarks it, installs go through the Extensions dialog's pipeline, the tab layout is
//! changed through the setting and read back from widget geometry, and the popup opens from the
//! toolbar button. The window is never activated and gets no OS input. Every check is bounded;
//! a timeout reports the last value the check saw.

mod omnibox_checks;
mod shortcut_checks;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use vsesvit_core::Url;
use vsesvit_core::downloads::State;
use vsesvit_core::extensions::{ExtensionId, InstallSource, Verification};
use vsesvit_core::prefs::{TabsPosition, keys};
use vsesvit_core::search::NavTarget;
use vsesvit_core::testkit::{self, FixtureServer};

use crate::bindings::Panel;
use crate::browser::Browser;
use crate::dialogs::{self, Dialog};
use crate::layout;
use crate::popup::Activation;
use crate::report::{Check, Report};
use crate::shortcuts::Command;
use crate::tab::Tab;
use crate::window::BrowserWindow;
use crate::{app, engine, exec};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
/// WebView2 validates and registers an extension on first load.
const ENGINE_LOAD_TIMEOUT: Duration = Duration::from_secs(60);
/// A 10 MB download, signature checks, and the engine indexing large rulesets.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(240);
const POLL: Duration = Duration::from_millis(100);
const FIXTURE_TITLE: &str = "Vsesvit fixture";
const PAGE2_TITLE: &str = "Vsesvit fixture 2";
/// What the fixture server sends for `/download.bin`.
const DOWNLOAD_FIXTURE: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/site/download.bin"));
/// The new tab page's tile links, once its search box is there.
const NEW_TAB_PAGE_PROBE: &str = "document.querySelector('form input') ? [...document.querySelectorAll('.tile')].map(a => a.href).join(' ') : 'no search box'";
/// uBlock Origin Lite.
const CWS_ID: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

/// Starts from a fresh profile: removes what an earlier run left in `out_dir`.
pub(crate) fn prepare(out_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    for dir in ["profile", "downloads"] {
        match std::fs::remove_dir_all(out_dir.join(dir)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    for file in [
        "report.json",
        "window.png",
        "downloads.png",
        "settings-shortcuts.png",
        "shortcut-capture.png",
        "omnibox-inline.png",
        "saved-page.mhtml",
        "probe.crx",
        "vsesvit.log",
    ] {
        match std::fs::remove_file(out_dir.join(file)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// The profile did not open, so nothing else can run.
pub(crate) fn report_failed_start(out_dir: &Path, network: bool, ms: u128, error: &str) {
    let mut report = Report::default();
    report.push(Check {
        name: "profile_open",
        ok: false,
        ms,
        detail: error.to_owned(),
    });
    report.complete(network, "the profile did not open");
    if let Err(e) = report.write(out_dir, network) {
        log::error!("report.json: {e}");
    }
}

pub(crate) async fn run(browser: Rc<Browser>, out_dir: PathBuf, network: bool) {
    let mut report = Report::default();
    if let Err(e) = checks(&browser, &out_dir, network, &mut report).await {
        log::error!("self-test stopped: {e}");
        report.complete(network, &e);
    }
    let ok = report.ok(network);
    match report.write(&out_dir, network) {
        Ok(()) => log::info!("self-test: ok={ok}, report in {}", out_dir.display()),
        Err(e) => log::error!("self-test: writing report.json: {e}"),
    }
    app::exit(if ok { 0 } else { 1 });
}

/// The last value a check observed, reported if it times out.
#[derive(Clone, Default)]
struct Probe(Rc<RefCell<String>>);

impl Probe {
    fn observe(&self, value: impl std::fmt::Display) {
        *self.0.borrow_mut() = value.to_string();
    }

    fn last(&self) -> String {
        self.0.borrow().clone()
    }
}

async fn check<F>(report: &mut Report, name: &'static str, limit: Duration, f: F)
where
    F: AsyncFnOnce(&Probe) -> Result<String, String>,
{
    let probe = Probe::default();
    let started = Instant::now();
    let outcome = exec::timeout(limit, f(&probe)).await;
    let (ok, detail) = match outcome {
        Some(Ok(detail)) => (true, detail),
        Some(Err(detail)) => (false, detail),
        None => (
            false,
            format!(
                "timed out after {} s; last observed: {}",
                limit.as_secs(),
                probe.last()
            ),
        ),
    };
    let check = Check {
        name,
        ok,
        ms: started.elapsed().as_millis(),
        detail,
    };
    log::info!(
        "self-test {name}: ok={} ({} ms) {}",
        check.ok,
        check.ms,
        check.detail
    );
    report.push(check);
}

/// Polls `f` until it returns `Some`, recording what it saw.
async fn until<T>(probe: &Probe, mut f: impl FnMut(&Probe) -> Option<T>) -> T {
    loop {
        if let Some(value) = f(probe) {
            return value;
        }
        exec::sleep(POLL).await;
    }
}

async fn checks(
    browser: &Rc<Browser>,
    out_dir: &Path,
    network: bool,
    report: &mut Report,
) -> Result<(), String> {
    let server = FixtureServer::start().map_err(|e| format!("fixture server: {e}"))?;
    let window = browser.windows().into_iter().next().ok_or("no window")?;
    let probe_id = ExtensionId::parse(testkit::PROBE_ID).map_err(|e| e.to_string())?;
    let index = server.url("/index.html");

    report.push(Check {
        name: "profile_open",
        ok: true,
        ms: browser.profile_open_ms(),
        detail: format!("opened {}", browser.profile_dir().display()),
    });

    check(report, "install_crx", DEFAULT_TIMEOUT, async |p| {
        let crx = out_dir.join("probe.crx");
        std::fs::write(&crx, testkit::probe_crx())
            .map_err(|e| format!("{}: {e}", crx.display()))?;
        let source = InstallSource::parse(&crx.to_string_lossy()).map_err(|e| e.to_string())?;
        let ext = browser
            .install_package(source, &|progress| p.observe(&progress.text))
            .await?;
        let detail = format!(
            "id={} version={} verification={:?} dir={}",
            ext.id.as_str(),
            ext.version,
            ext.verification,
            ext.dir.display()
        );
        (ext.id == probe_id && ext.verification == Verification::LocalCrx)
            .then_some(detail.clone())
            .ok_or(detail)
    })
    .await;

    check(
        report,
        "engine_loaded_extension",
        ENGINE_LOAD_TIMEOUT,
        async |p| {
            p.observe("syncing WebView2's extensions");
            browser.sync_extensions().await?;
            let recorded = browser
                .core(|c| c.extensions().get(&probe_id))
                .map_err(|e| e.to_string())?
                .and_then(|e| e.engine_id);
            let profile = browser.engine_profile().await.ok_or("no engine profile")?;
            let loaded = exec::timeout(DEFAULT_TIMEOUT, engine::extensions(&profile))
                .await
                .ok_or("listing WebView2's extensions timed out")?
                .map_err(|e| e.to_string())?;
            let in_engine = loaded.iter().find(|e| e.id == testkit::PROBE_ID);
            let detail = format!(
                "AddBrowserExtensionAsync id={recorded:?}; engine lists {}",
                in_engine.map_or("nothing with that id".to_owned(), |e| format!(
                    "{} ({}, enabled={})",
                    e.id, e.name, e.enabled
                ))
            );
            let ok = recorded.as_deref() == Some(testkit::PROBE_ID)
                && in_engine.is_some_and(|e| e.enabled);
            if let Some(error) = browser.extensions.engine_error(&probe_id) {
                return Err(format!("{detail}; engine error: {error}"));
            }
            ok.then_some(detail.clone()).ok_or(detail)
        },
    )
    .await;

    let tab = window.active_tab().ok_or("no active tab")?;
    check(report, "navigate", DEFAULT_TIMEOUT, async |p| {
        wait_ready(&tab, p).await;
        window.address_submitted(index.as_str());
        let state = until(p, |p| {
            let s = tab.state();
            p.observe(format!(
                "url={} title={:?} loading={}",
                s.url, s.title, s.loading()
            ));
            (s.url == index.as_str() && s.title == FIXTURE_TITLE && !s.loading()).then_some(s)
        })
        .await;
        Ok(format!("title={} url={}", state.title, state.url))
    })
    .await;
    let loaded_at = Instant::now();

    check(report, "history_recorded", DEFAULT_TIMEOUT, async |p| {
        let entry = until(p, |p| {
            let visits = browser
                .core(|c| c.history().visits_between(0, i64::MAX, 100))
                .unwrap_or_default();
            p.observe(format!("{} visits", visits.len()));
            visits.into_iter().find(|(entry, _)| entry.url == index)
        })
        .await;
        Ok(format!(
            "visit at {} ms ({:?}), title {:?}, {} visit(s)",
            entry.1.at_ms, entry.1.transition, entry.0.title, entry.0.visit_count
        ))
    })
    .await;

    check(report, "content_script", DEFAULT_TIMEOUT, async |p| {
        loop {
            let value = eval(
                &tab,
                "document.documentElement.dataset.vsesvitProbe || null",
            )
            .await;
            p.observe(format!("dataset.vsesvitProbe = {value:?}"));
            if value.as_deref() == Ok("\"background-replied\"") {
                let visits = eval(
                    &tab,
                    "document.documentElement.dataset.vsesvitVisits || null",
                )
                .await;
                return Ok(format!(
                    "dataset.vsesvitProbe = background-replied, visits = {visits:?}"
                ));
            }
            exec::sleep(POLL).await;
        }
    })
    .await;

    check(report, "dnr_blocked", DEFAULT_TIMEOUT, async |_| {
        let settle = Duration::from_secs(1).saturating_sub(loaded_at.elapsed());
        exec::sleep(settle).await;
        let hits = server.hits();
        let allowed = hits.iter().any(|h| h == "/allowed.png");
        let blocked = hits.iter().any(|h| h == "/vsesvit-blocked/pixel.png");
        let detail = format!("server saw {hits:?}");
        (allowed && !blocked)
            .then_some(detail.clone())
            .ok_or(detail)
    })
    .await;

    check(report, "bookmark", DEFAULT_TIMEOUT, async |p| {
        window.star_clicked();
        let seen = until(p, |p| {
            let bookmarked = browser.is_bookmarked(index.as_str());
            let in_bar = window.bookmarks_bar_items().iter().any(|item| match item {
                crate::bookmarks_bar::BarItem::Link { url, .. } => url == index.as_str(),
                crate::bookmarks_bar::BarItem::Folder { .. } => false,
            });
            let buttons = window.bookmarks_bar_buttons();
            let starred = tab.state().starred;
            p.observe(format!(
                "is_bookmarked={bookmarked} bar_item={in_bar} bar_buttons={buttons} bar_shown={} star={starred}",
                window.bookmarks_bar_shown()
            ));
            (bookmarked && in_bar && buttons >= 1 && window.bookmarks_bar_shown() && starred).then(|| p.last())
        })
        .await;
        if let Some(editor) = window.bookmark_editor() {
            editor.close();
        }
        Ok(seen)
    })
    .await;

    check(report, "tabs", DEFAULT_TIMEOUT, async |p| {
        let page2 = server.url("/page2.html");
        window.run(Command::NewTab);
        let second = until(p, |p| {
            let tabs = window.tabs_in_order();
            p.observe(format!("{} tabs", tabs.len()));
            tabs.into_iter().find(|t| t.id != tab.id)
        })
        .await;
        wait_ready(&second, p).await;
        window.address_submitted(page2.as_str());
        until(p, |p| {
            let s = second.state();
            p.observe(format!("second tab url={} title={:?}", s.url, s.title));
            (s.title == PAGE2_TITLE && !s.loading()).then_some(())
        })
        .await;
        let opened = window.tab_count();
        window.run(Command::SelectTab(0));
        let back = window.active_tab().is_some_and(|t| t.id == tab.id);
        window.close_tab(second.id);
        let remaining = window.tabs_in_order();
        let detail = format!(
            "opened {opened} tabs, switched back: {back}, after close: {} tab(s) at {:?}",
            remaining.len(),
            remaining.iter().map(|t| t.state().url).collect::<Vec<_>>()
        );
        let ok = opened == 2
            && back
            && remaining.len() == 1
            && remaining[0].id == tab.id
            && remaining[0].state().url == index.as_str();
        ok.then_some(detail.clone()).ok_or(detail)
    })
    .await;

    check(report, "tab_layout", DEFAULT_TIMEOUT, async |p| {
        let mut seen = Vec::new();
        let default = browser.tabs_position();
        seen.push(expect_layout(&window, TabsPosition::Left, p).await);
        for position in [TabsPosition::Right, TabsPosition::Top, TabsPosition::Left] {
            browser.set_tabs_position(position);
            seen.push(expect_layout(&window, position, p).await);
        }
        let stored = browser.core(|c| c.prefs().get(&keys::TABS_POSITION));
        let detail = format!(
            "default {default:?}; {}; stored {stored:?}",
            seen.join("; ")
        );
        (default == TabsPosition::Left && stored == TabsPosition::Left)
            .then_some(detail.clone())
            .ok_or(detail)
    })
    .await;

    check(report, "popup", DEFAULT_TIMEOUT, async |p| {
        let action = until(p, |p| {
            let actions = browser.extension_actions();
            p.observe(format!(
                "toolbar actions: {:?}",
                actions.iter().map(|a| &a.extension_id).collect::<Vec<_>>()
            ));
            actions
                .into_iter()
                .find(|a| a.extension_id == testkit::PROBE_ID)
        })
        .await;
        let popup = window
            .open_extension_popup(&action.extension_id, Activation::Keep)
            .map_err(|e| format!("opening the popup: {e}"))?;
        let title = until(p, |p| {
            let title = popup.title();
            p.observe(format!("popup title {title:?}"));
            title.filter(|t| visits(t).is_some_and(|n| n >= 1))
        })
        .await;
        popup.hide();
        Ok(format!(
            "{} opened; document title {title:?}",
            action.popup_url().unwrap_or_default()
        ))
    })
    .await;

    check(report, "omnibox", DEFAULT_TIMEOUT, async |_| {
        let (search, direct, default) = browser.core(|c| {
            let search = c.omnibox().resolve("vsesvit fixture");
            let direct = c
                .omnibox()
                .resolve(&format!("127.0.0.1:{}/page2.html", server.port()));
            let default = c.search_engines().default_engine().map(|e| e.id);
            (search, direct, default)
        });
        let (search, direct, default) = (
            search.map_err(|e| e.to_string())?,
            direct.map_err(|e| e.to_string())?,
            default.map_err(|e| e.to_string())?,
        );
        let expected = Url::parse(&format!("http://127.0.0.1:{}/page2.html", server.port()))
            .map_err(|e| e.to_string())?;
        let search_ok =
            matches!(&search, Some(NavTarget::Search { engine, .. }) if *engine == default);
        let direct_ok = direct == Some(NavTarget::Url(expected.clone()));
        let box_ok = browser
            .resolve_input(&format!("127.0.0.1:{}/page2.html", server.port()))
            .as_deref()
            == Some(expected.as_str());
        let detail = format!(
            "\"vsesvit fixture\" -> {} (default engine {}); \"127.0.0.1:{}/page2.html\" -> {}",
            describe(search.as_ref()),
            default.0,
            server.port(),
            describe(direct.as_ref())
        );
        (search_ok && direct_ok && box_ok)
            .then_some(detail.clone())
            .ok_or(detail)
    })
    .await;

    check(report, "address_completion", DEFAULT_TIMEOUT, async |p| {
        omnibox_checks::address_completion(&window, &tab, &server, out_dir, p).await
    })
    .await;

    check(report, "selection_search", DEFAULT_TIMEOUT, async |p| {
        omnibox_checks::selection_search(browser, &window, &tab, &server, p).await
    })
    .await;

    check(report, "session", DEFAULT_TIMEOUT, async |_| {
        if !browser.save_session_now() {
            return Err("session save failed".into());
        }
        let restored = browser
            .core(|c| c.session().restore())
            .map_err(|e| e.to_string())?
            .ok_or("restore() returned nothing")?;
        let tabs: Vec<Vec<String>> = restored
            .windows
            .iter()
            .map(|w| w.tabs.iter().map(|t| t.url.to_string()).collect())
            .collect();
        let detail = format!("restored {} window(s): {tabs:?}", restored.windows.len());
        (!restored.windows.is_empty() && restored.windows.iter().any(|w| !w.tabs.is_empty()))
            .then_some(detail.clone())
            .ok_or(detail)
    })
    .await;

    check(report, "new_tab_page", DEFAULT_TIMEOUT, async |p| {
        let origin = server.url("/");
        window.run(Command::NewTab);
        let ntp = until(p, |p| {
            let tabs = window.tabs_in_order();
            p.observe(format!("{} tabs", tabs.len()));
            tabs.into_iter().find(|t| t.id != tab.id)
        })
        .await;
        wait_ready(&ntp, p).await;
        let tiles = loop {
            let tiles = eval(&ntp, NEW_TAB_PAGE_PROBE).await?;
            if tiles.contains(origin.as_str()) {
                break tiles;
            }
            p.observe(format!("page reports {tiles}"));
            exec::sleep(POLL).await;
        };
        exec::sleep(Duration::from_millis(500)).await;
        let shot = window.capture().await.map_err(|e| format!("capture: {e}"))?;
        let path = out_dir.join("new-tab.png");
        std::fs::write(&path, &shot.png).map_err(|e| format!("{}: {e}", path.display()))?;
        let s = ntp.state();
        let detail = format!("url={:?} title={:?} tiles {tiles}", s.url, s.title);
        window.close_tab(ntp.id);
        (s.url == "about:blank" && s.title == "New tab")
            .then_some(detail.clone())
            .ok_or(detail)
    })
    .await;

    check(report, "shortcuts", Duration::from_secs(90), async |p| {
        shortcut_checks::shortcuts(&window, &tab, out_dir, p)
            .await
            .map_err(|e| format!("{e} (at: {})", p.last()))
    })
    .await;
    window.close_scripted_dialog();
    if browser.core(|c| c.prefs().keymap()) != vsesvit_core::shortcuts::Keymap::default() {
        browser.edit_keymap(vsesvit_core::shortcuts::Keymap::reset_all);
    }

    check(report, "save_page", DEFAULT_TIMEOUT, async |p| {
        shortcut_checks::save_page(&tab, out_dir, p).await
    })
    .await;

    check(report, "download", DEFAULT_TIMEOUT, async |p| {
        let dir = out_dir.join("downloads");
        browser.set_download_dir(Some(&dir));
        let url = server.url("/download.bin");
        tab.navigate(url.as_str());
        let entry = until(p, |p| {
            let list = browser.download_list();
            p.observe(format!(
                "list {:?}",
                list.iter()
                    .map(|d| format!("{:?} {}", d.state, d.path.display()))
                    .collect::<Vec<_>>()
            ));
            list.into_iter()
                .find(|d| d.url == url.as_str() && d.state == State::Completed)
        })
        .await;
        let bytes =
            std::fs::read(&entry.path).map_err(|e| format!("{}: {e}", entry.path.display()))?;
        let button = window.downloads_button_shown();
        let preview = dialogs::preview(&window, Dialog::Downloads)
            .map_err(|e| format!("Downloads view: {e}"))?;
        exec::sleep(Duration::from_millis(500)).await;
        let rows = preview
            .find::<Panel>("DownloadRows")
            .and_then(|rows| rows.Children()?.Size())
            .map_err(|e| format!("Downloads view rows: {e}"))?;
        let shot = window.capture().await.map_err(|e| format!("capture: {e}"))?;
        drop(preview);
        let path = out_dir.join("downloads.png");
        std::fs::write(&path, &shot.png).map_err(|e| format!("{}: {e}", path.display()))?;
        let detail = format!(
            "{} ({} bytes, {} recorded) in {}; toolbar button shown: {button}; the view lists {rows}",
            entry.path.display(),
            bytes.len(),
            entry.received,
            dir.display()
        );
        let ok = entry.path.parent() == Some(dir.as_path())
            && bytes == DOWNLOAD_FIXTURE
            && bytes.len() as u64 == entry.received
            && button
            && rows == 1;
        ok.then_some(detail.clone()).ok_or(detail)
    })
    .await;

    check(report, "screenshot", DEFAULT_TIMEOUT, async |_| {
        exec::sleep(Duration::from_millis(500)).await;
        let shot = window.capture().await.map_err(|e| format!("capture: {e}"))?;
        let path = out_dir.join("window.png");
        std::fs::write(&path, &shot.png).map_err(|e| format!("{}: {e}", path.display()))?;
        let detail = format!(
            "{} ({}x{}, {} bytes, flat={})",
            path.display(),
            shot.width,
            shot.height,
            shot.png.len(),
            shot.flat
        );
        (!shot.flat).then_some(detail.clone()).ok_or(detail)
    })
    .await;

    if network {
        check(report, "cws_install", NETWORK_TIMEOUT, async |p| {
            let source = InstallSource::parse(CWS_ID).map_err(|e| e.to_string())?;
            let ext = browser
                .install_package(source, &|progress| p.observe(&progress.text))
                .await?;
            p.observe(format!(
                "installed {} {}; loading it into WebView2",
                ext.manifest.name, ext.version
            ));
            browser.sync_extensions().await?;
            let engine_id = browser
                .core(|c| c.extensions().get(&ext.id))
                .map_err(|e| e.to_string())?
                .and_then(|e| e.engine_id);
            let error = browser.extensions.engine_error(&ext.id);
            let detail = format!(
                "{} {} verification={:?} engine id={engine_id:?}{}",
                ext.manifest.name,
                ext.version,
                ext.verification,
                error
                    .map(|e| format!(" engine error: {e}"))
                    .unwrap_or_default()
            );
            let ok = ext.id.as_str() == CWS_ID
                && ext.verification
                    == (Verification::ChromeWebStore {
                        publisher_verified: true,
                    })
                && engine_id.as_deref() == Some(CWS_ID);
            ok.then_some(detail.clone()).ok_or(detail)
        })
        .await;
    }
    Ok(())
}

/// The layout the window shows, once it matches `want`; described for the report.
async fn expect_layout(window: &Rc<BrowserWindow>, want: TabsPosition, probe: &Probe) -> String {
    until(probe, |p| {
        let (pane, strip, view) = window.layout_geometry();
        let seen = view.and_then(|view| layout::observed(pane, strip, view));
        p.observe(format!(
            "want {want:?}, geometry pane={pane:?} strip={strip:?} view={view:?}"
        ));
        (seen == Some(want)).then(|| {
            let part = match want {
                TabsPosition::Top => {
                    strip.map(|s| format!("strip y={:.0}..{:.0}", s.y, s.y + s.height))
                }
                _ => pane.map(|s| format!("pane x={:.0}..{:.0}", s.x, s.x + s.width)),
            };
            let view =
                view.map(|v| format!("view x={:.0}..{:.0} y={:.0}", v.x, v.x + v.width, v.y));
            format!(
                "{want:?}: {} {}",
                part.unwrap_or_default(),
                view.unwrap_or_default()
            )
        })
    })
    .await
}

/// A tab's engine view exists and its first navigation has settled.
async fn wait_ready(tab: &Rc<Tab>, probe: &Probe) {
    until(probe, |p| {
        let s = tab.state();
        p.observe(format!(
            "tab ready={} url={:?} loading={}",
            tab.is_ready(),
            s.url,
            s.loading()
        ));
        (tab.is_ready() && !s.loading() && !s.url.is_empty()).then_some(())
    })
    .await;
}

async fn eval(tab: &Tab, script: &str) -> Result<String, String> {
    exec::timeout(Duration::from_secs(5), tab.eval(script))
        .await
        .ok_or_else(|| "script timed out".to_owned())?
        .map_err(|e| e.to_string())
}

fn describe(target: Option<&NavTarget>) -> String {
    match target {
        Some(NavTarget::Url(url)) => format!("URL {url}"),
        Some(NavTarget::Search { engine, url }) => format!("search on {} ({url})", engine.0),
        None => "nothing".to_owned(),
    }
}

/// `N` from a popup title `visits=N`.
fn visits(title: &str) -> Option<u32> {
    title.strip_prefix("visits=")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::visits;

    #[test]
    fn popup_titles() {
        assert_eq!(visits("visits=3"), Some(3));
        assert_eq!(visits("visits=0"), Some(0));
        assert_eq!(visits("Vsesvit Probe"), None);
        assert_eq!(visits("visits="), None);
    }
}
