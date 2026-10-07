//! `vsesvit --self-test OUT_DIR [--network]`: the end-to-end contract of docs/design/self-test.md.
//!
//! It drives the same code paths as the UI: the address box submits the fixture URL, the star
//! button bookmarks it, installs go through the Extensions dialog's pipeline, the tab layout is
//! changed through the setting and read back from widget geometry, and the popup opens from the
//! toolbar button. The window is never activated and gets no OS input. Every check is bounded;
//! a timeout reports the last value the check saw.

mod cookie_checks;
mod extension_update_checks;
mod memory_saver_checks;
mod omnibox_checks;
mod page_checks;
mod private_checks;
mod profile_checks;
mod search_engine_checks;
mod download_checks;
mod shortcut_checks;
mod sync_checks;
mod tab_menu_checks;
mod tab_search_checks;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use vsesvit_core::Url;
use vsesvit_core::bookmarks::ImportItem;
use vsesvit_core::downloads::State;
use vsesvit_core::extensions::{ExtensionId, InstallSource, Verification};
use vsesvit_core::https_only::{self, Reach};
use vsesvit_core::import;
use vsesvit_core::permissions::{Origin, Permission};
use vsesvit_core::prefs::{TabsPosition, keys};
use vsesvit_core::private::Browsing;
use vsesvit_core::search::NavTarget;
use vsesvit_core::testkit::report::{Check, Report};
use vsesvit_core::testkit::{self, FixtureServer};
use vsesvit_core::trackers::{self, Category, TrackerList};

use windows_core::{IInspectable, Interface};

use crate::automation::label;
use crate::bindings::{Button, ItemsControl, Panel};
use crate::browser::{Browser, PASSWORDS_PURGED};
use crate::dialogs::{self, Dialog};
use crate::layout;
use crate::popup::Activation;
use crate::report::expected;
use crate::shortcuts::Command;
use crate::tab::{Tab, TabId};
use crate::window::BrowserWindow;
use crate::{app, engine, exec, xaml, zoom};

pub(crate) use sync_checks::WithoutPassphrase;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
/// WebView2 validates and registers an extension on first load.
const ENGINE_LOAD_TIMEOUT: Duration = Duration::from_secs(60);
/// A 10 MB download, signature checks, and the engine indexing large rulesets.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(240);
const POLL: Duration = Duration::from_millis(100);
const FIXTURE_TITLE: &str = "Vsesvit fixture";
const PAGE2_TITLE: &str = "Vsesvit fixture 2";
/// A page on no site, whose zoom is remembered for none.
const OFF_THE_WEB: &str = "data:text/html,<title>Off the web</title>";
/// What the fixture server sends for `/download.bin`.
const DOWNLOAD_FIXTURE: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/site/download.bin"));
/// The new tab page's tile links, once its search box is there.
const NEW_TAB_PAGE_PROBE: &str = "document.querySelector('form input') ? [...document.querySelectorAll('.tile')].map(a => a.href).join(' ') : 'no search box'";
/// Gives the page a new favicon four times, 300 ms apart, as pages that badge their icon do.
const FAVICON_SWAPS: &str = "(() => { let n = 0; const swap = () => { const c = document.createElement('canvas'); c.width = c.height = 16; const g = c.getContext('2d'); g.fillStyle = ['#d33', '#3a3', '#33d', '#da3'][n]; g.fillRect(0, 0, 16, 16); let link = document.querySelector('link[rel=icon]'); if (!link) { link = document.createElement('link'); link.rel = 'icon'; document.head.append(link); } link.href = c.toDataURL(); if (++n < 4) setTimeout(swap, 300); }; swap(); return 'swapping'; })()";
/// uBlock Origin Lite.
const CWS_ID: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

/// Starts from a fresh profile: removes what an earlier run left in `out_dir`.
pub(crate) fn prepare(out_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    for dir in ["profile", "downloads"] {
        absent(std::fs::remove_dir_all(out_dir.join(dir)))?;
    }
    for file in [
        "report.json",
        "window.png",
        "downloads.png",
        "settings-shortcuts.png",
        "shortcut-capture.png",
        "omnibox-inline.png",
        "new-tab.png",
        "saved-page.mhtml",
        "bookmarks.html",
        "probe.crx",
        "vsesvit.log",
        "profiles.json",
        "profile-menu.png",
        "profiles-manage.png",
    ] {
        absent(std::fs::remove_file(out_dir.join(file)))?;
    }
    Ok(())
}

/// `removed`, with "it was not there" counted as removed.
pub(crate) fn absent(removed: std::io::Result<()>) -> std::io::Result<()> {
    match removed {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

/// The profile did not open, so nothing else can run.
pub(crate) fn report_failed_start(out_dir: &Path, network: bool, ms: u128, error: &str) {
    let mut report = Report::new("windows", expected(network));
    report.push(Check {
        name: "profile_open",
        ok: false,
        ms,
        detail: error.to_owned(),
    });
    report.complete("the profile did not open");
    if let Err(e) = report.write(out_dir) {
        log::error!("report.json: {e}");
    }
}

pub(crate) async fn run(browser: Rc<Browser>, out_dir: PathBuf, network: bool) {
    let mut report = Report::new("windows", expected(network));
    if let Err(e) = checks(&browser, &out_dir, network, &mut report).await {
        log::error!("self-test stopped: {e}");
        report.complete(&e);
    }
    let ok = report.ok();
    match report.write(&out_dir) {
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
    // Text typed into the address box would otherwise go to the default engine, on the internet.
    browser.write_pref(&keys::SEARCH_SUGGESTIONS, &false);
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

    check(report, "extension_port", DEFAULT_TIMEOUT, async |p| {
        loop {
            let value = eval(
                &tab,
                "document.documentElement.dataset.vsesvitProbePort || null",
            )
            .await;
            p.observe(format!("dataset.vsesvitProbePort = {value:?}"));
            if value.as_deref() == Ok("\"pong:probe\"") {
                return Ok("dataset.vsesvitProbePort = pong:probe".to_owned());
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

    check(report, "dnr_site_allowed", DEFAULT_TIMEOUT, async |p| {
        let pixels = || {
            server
                .hits()
                .iter()
                .filter(|h| *h == "/vsesvit-blocked/pixel.png")
                .count()
        };
        let ask = async |want: &str, rules: &str| {
            eval(
                &tab,
                &format!("document.documentElement.dataset.vsesvitDnr = '{want}'"),
            )
            .await?;
            loop {
                let value = eval(
                    &tab,
                    "document.documentElement.dataset.vsesvitProbeDnr || null",
                )
                .await?;
                p.observe(format!("dataset.vsesvitProbeDnr = {value}"));
                if let Ok(Some(answer)) = serde_json::from_str::<Option<String>>(&value) {
                    return if answer == rules {
                        Ok(answer)
                    } else {
                        Err(format!("asked to {want}, WebView2 gives the probe {answer}"))
                    };
                }
                exec::sleep(POLL).await;
            }
        };
        let reload = async || {
            eval(&tab, "window.stale = true").await?;
            tab.reload();
            loop {
                let ready = eval(
                    &tab,
                    "!window.stale && document.readyState == 'complete' \
                     && document.documentElement.dataset.vsesvitProbe == 'background-replied'",
                )
                .await;
                p.observe(format!("the reloaded page is ready: {ready:?}"));
                if ready.as_deref() == Ok("true") {
                    return Ok::<_, String>(());
                }
                exec::sleep(POLL).await;
            }
        };

        let allowed = ask("allow", r#"{"rules":[1]}"#).await?;
        let before = pixels();
        reload().await?;
        let seen = until(p, |p| {
            let seen = pixels();
            p.observe(format!(
                "with the allow rule, the reloaded page left /vsesvit-blocked/pixel.png at {seen} request(s)"
            ));
            (seen > before).then_some(seen)
        })
        .await;
        let cleared = ask("clear", r#"{"rules":[]}"#).await?;
        reload().await?;
        exec::sleep(Duration::from_secs(1)).await;
        let after = pixels();
        if after != seen {
            return Err(format!(
                "after the probe cleared its rule ({cleared}), a reload requested /vsesvit-blocked/pixel.png again ({seen} -> {after})"
            ));
        }
        Ok(format!(
            "the probe's allowAllRequests rule for the site ({allowed}) let a reload request /vsesvit-blocked/pixel.png ({before} -> {seen}); after it cleared the rule ({cleared}) a reload left it blocked"
        ))
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

    check(report, "bookmarks_bar_icons", DEFAULT_TIMEOUT, async |p| {
        let before = bar_entries(&window);
        eval(&tab, FAVICON_SWAPS).await?;
        let mut icons: Vec<Vec<u8>> = Vec::new();
        until(p, |p| {
            let icon = window
                .bookmarks_bar_items()
                .iter()
                .find_map(|item| match item {
                    crate::bookmarks_bar::BarItem::Link { url, icon, .. }
                        if url == index.as_str() =>
                    {
                        icon.clone()
                    }
                    _ => None,
                });
            if let Some(icon) = icon.filter(|i| !icons.contains(i)) {
                icons.push(icon);
            }
            p.observe(format!("{} distinct icons on the bar item", icons.len()));
            (icons.len() >= 3).then_some(())
        })
        .await;
        let after = bar_entries(&window);
        let kept = before.len() == after.len()
            && before
                .iter()
                .zip(&after)
                .all(|(a, b)| xaml::same_object(a, b));
        let detail = format!(
            "{} distinct icons shown; {} entries before, {} after, same entries={kept}",
            icons.len(),
            before.len(),
            after.len()
        );
        kept.then_some(detail.clone()).ok_or(detail)
    })
    .await;

    check(report, "bookmark_export", DEFAULT_TIMEOUT, async |_| {
        let preview = dialogs::preview(&window, Dialog::Bookmarks)
            .map_err(|e| format!("Bookmarks view: {e}"))?;
        let button = preview
            .find::<Button>("ExportRun")
            .map(|b| label(&b))
            .map_err(|e| format!("Bookmarks view: {e}"))?;
        let path = out_dir.join("bookmarks.html");
        let status = dialogs::export_bookmarks(browser, path.clone()).await;
        drop(preview);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let found = has_link(&import::parse_html(&text), index.as_str());
        let detail = format!(
            "button {button:?}; status {status:?}; {} ({} bytes) links the fixture page: {found}",
            path.display(),
            text.len()
        );
        (button == "Export bookmarks\u{2026}" && found)
            .then_some(detail.clone())
            .ok_or(detail)
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

    let windows = browser.windows();
    check(report, "tab_menu", DEFAULT_TIMEOUT, async |p| {
        tab_menu_checks::tab_menu(browser, &window, &tab, &server, p).await
    })
    .await;
    tab_menu_checks::tidy(browser, &windows, &window, &tab);

    check(report, "tab_search", DEFAULT_TIMEOUT, async |p| {
        tab_search_checks::tab_search(browser, &window, &tab, &server, p).await
    })
    .await;
    tab_menu_checks::tidy(browser, &windows, &window, &tab);

    check(report, "memory_saver", DEFAULT_TIMEOUT, async |p| {
        memory_saver_checks::memory_saver(browser, &window, &tab, &server, p).await
    })
    .await;
    browser.write_pref(&keys::MEMORY_SAVER, &true);
    tab_menu_checks::tidy(browser, &windows, &window, &tab);

    check(report, "zoom_is_remembered_per_site", DEFAULT_TIMEOUT, async |p| {
        let page2 = server.url("/page2.html");
        let remembered = |url: &Url| browser.core(|c| c.site_zoom(Browsing::Normal).get(url)).unwrap_or(-1.0);
        let scale = window.scale();
        let mut seen = Vec::new();

        load(&tab, index.as_str(), p).await;
        emulate_zoom(&tab, scale, Some(1.25)).await?;
        until(p, |p| {
            let (shown, stored) = (tab.state().zoom, remembered(&index));
            p.observe(format!("zoomed to {} on {index}, remembered {stored}", shown.label()));
            (shown == zoom::Level(125) && stored == 1.25).then_some(())
        })
        .await;
        seen.push("125% on index.html remembered for the site".to_owned());

        load(&tab, page2.as_str(), p).await;
        let same_site = (remembered(&page2), tab.wanted_zoom());
        seen.push(format!("page2.html: remembered {}, wants {:?}", same_site.0, same_site.1));

        // A page off the web: back at 100% there, which is remembered for no site.
        load(&tab, OFF_THE_WEB, p).await;
        emulate_zoom(&tab, scale, None).await?;
        until(p, |p| {
            p.observe(format!("off the web at {}", tab.state().zoom.label()));
            tab.state().zoom.is_default().then_some(())
        })
        .await;
        // A change is remembered once it settles.
        exec::sleep(zoom::SETTLE).await;
        let kept = remembered(&index);
        seen.push(format!("100% off the web left the site at {kept}"));

        // The site's page starts at 100% and asks for its site's level; a scripted run sends
        // no key presses, so it stays asking.
        load(&tab, index.as_str(), p).await;
        let wanted = until(p, |p| {
            p.observe(format!("index.html at {}, wants {:?}", tab.state().zoom.label(), tab.wanted_zoom()));
            tab.wanted_zoom()
        })
        .await;
        seen.push(format!("index.html opened at {}, wants {}", tab.state().zoom.label(), wanted.label()));

        // Back to 100% forgets the site.
        emulate_zoom(&tab, scale, Some(1.25)).await?;
        until(p, |_| (tab.state().zoom == zoom::Level(125)).then_some(())).await;
        emulate_zoom(&tab, scale, None).await?;
        until(p, |p| {
            let stored = remembered(&index);
            p.observe(format!("at {}, remembered {stored}", tab.state().zoom.label()));
            (tab.state().zoom.is_default() && stored == 1.0).then_some(())
        })
        .await;
        load(&tab, page2.as_str(), p).await;
        let after_reset = (remembered(&page2), tab.wanted_zoom());
        seen.push(format!("reset forgot it: page2.html remembered {}, wants {:?}", after_reset.0, after_reset.1));
        load(&tab, index.as_str(), p).await;

        let detail = seen.join("; ");
        (same_site == (1.25, None) && kept == 1.25 && wanted == zoom::Level(125) && after_reset == (1.0, None))
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

    check(report, "search_suggestions", DEFAULT_TIMEOUT, async |p| {
        omnibox_checks::search_suggestions(browser, &window, &tab, &server, out_dir, p).await
    })
    .await;

    check(report, "selection_search", DEFAULT_TIMEOUT, async |p| {
        omnibox_checks::selection_search(browser, &window, &tab, &server, p).await
    })
    .await;

    check(report, "search_engines", DEFAULT_TIMEOUT, async |p| {
        let search_url = format!("http://127.0.0.1:{}/search?q={{searchTerms}}", server.port());
        search_engine_checks::search_engines(&window, &search_url, index.as_str(), out_dir, p)
            .await
            .map_err(|e| format!("{e} (at: {})", p.last()))
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

    check(report, "private_window", DEFAULT_TIMEOUT, async |p| {
        private_checks::private_window(browser, &server, &out_dir.join("downloads"), p).await
    })
    .await;

    check(report, "new_tab_page", DEFAULT_TIMEOUT, async |p| {
        let origin = server.url("/");
        let open = tab_ids(&window);
        window.run(Command::NewTab);
        let ntp = until(p, |p| {
            let tabs = window.tabs_in_order();
            p.observe(format!("{} tabs", tabs.len()));
            tabs.into_iter().find(|t| !open.contains(&t.id))
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

    let open = tab_ids(&window);
    let page = server.url("/trackers.html");
    let site = Origin::parse(page.as_str()).ok_or("no fixture origin")?;
    browser.set_trackers(
        TrackerList::bundled()
            .clone()
            .with_tracker("localhost", Category::Analytics),
    );
    check(report, "tracking_protection", DEFAULT_TIMEOUT, async |p| {
        let pixel = || server.hits().iter().any(|h| h == "/tracker/pixel.png");
        let protected = window
            .open_url_tab(page.as_str(), true)
            .map_err(|e| e.to_string())?;
        until(p, |p| {
            let s = protected.state();
            p.observe(format!(
                "title {:?}, blocked {:?}",
                s.title,
                protected.blocked_trackers()
            ));
            (s.title == "tracker blocked" && !s.loading()).then_some(())
        })
        .await;
        let blocked = protected.blocked_trackers();
        if pixel() || blocked != ["localhost"] {
            return Err(format!(
                "blocked {blocked:?}; server saw {:?}",
                server.hits()
            ));
        }
        protected.set_tracking_protection(false);
        until(p, |p| {
            let s = protected.state();
            p.observe(format!(
                "off for the site: title {:?}, server saw the image: {}",
                s.title,
                pixel()
            ));
            (s.title == "tracker loaded" && pixel()).then_some(())
        })
        .await;
        let allowed = browser.core(|c| trackers::allowed(c, &site));
        let detail = format!(
            "blocked {blocked:?} unseen by the server; off for {} (stored: {allowed}), it loaded",
            site.as_str()
        );
        allowed.then_some(detail.clone()).ok_or(detail)
    })
    .await;
    if let Err(e) = browser.core(|c| trackers::set_allowed(c, &site, false)) {
        log::warn!("tracking protection for {}: {e}", site.as_str());
    }
    for id in tab_ids(&window).into_iter().filter(|id| !open.contains(id)) {
        window.close_tab(id);
    }
    browser.set_trackers(TrackerList::bundled().clone());

    let open = tab_ids(&window);
    let site = Origin::of(&index).ok_or("no fixture origin")?;
    browser.write_pref(&keys::HTTPS_ONLY, &true);
    browser.set_https_reach(Reach::Everywhere);
    check(report, "https_only", DEFAULT_TIMEOUT, async |p| {
        let tab = window
            .open_url_tab(index.as_str(), true)
            .map_err(|e| e.to_string())?;
        let at = |title: &str, p: &Probe| {
            let s = tab.state();
            p.observe(format!("at {:?}, titled {:?}", s.url, s.title));
            (s.url == index.as_str() && s.title == title && !s.loading()).then_some(())
        };
        until(p, |p| at(https_only::WARNING_TITLE, p)).await;
        let link = eval(&tab, "document.getElementById('continue').href").await?;
        eval(&tab, "document.getElementById('continue').click()").await?;
        until(p, |p| at(FIXTURE_TITLE, p)).await;
        let allowed = browser.core(|c| https_only::allowed(c, Browsing::Normal, &site));
        let detail = format!(
            "{index} failed over https and showed {:?} at its own address; its Continue link {link} loaded the page over http and stored the exception ({allowed})",
            https_only::WARNING_TITLE
        );
        allowed.then_some(detail.clone()).ok_or(detail)
    })
    .await;
    if let Err(e) = browser.core(|c| c.site_permissions().set(&site, Permission::Http, None)) {
        log::warn!("HTTPS-only exception for {}: {e}", site.as_str());
    }
    browser.write_pref(&keys::HTTPS_ONLY, &false);
    browser.set_https_reach(Reach::Public);
    for id in tab_ids(&window).into_iter().filter(|id| !open.contains(id)) {
        window.close_tab(id);
    }

    let open = tab_ids(&window);
    check(report, "cookies", Duration::from_secs(60), async |p| {
        cookie_checks::cookies(browser, &window, &server, p).await
    })
    .await;
    cookie_checks::restore(browser, &server);
    for id in tab_ids(&window).into_iter().filter(|id| !open.contains(id)) {
        window.close_tab(id);
    }

    check(report, "passwords_purged", DEFAULT_TIMEOUT, async |p| {
        until(p, |p| {
            let purged = browser.core(|c| c.prefs().get(&PASSWORDS_PURGED));
            p.observe(format!("{} = {purged}", PASSWORDS_PURGED.key));
            purged.then_some(())
        })
        .await;
        Ok(format!("{} is set", PASSWORDS_PURGED.key))
    })
    .await;

    check(report, "shortcuts", Duration::from_secs(90), async |p| {
        shortcut_checks::shortcuts(&window, &tab, out_dir, p)
            .await
            .map_err(|e| format!("{e} (at: {})", p.last()))
    })
    .await;
    restore_shortcuts(&window, browser);

    check(report, "shortcuts_sync", DEFAULT_TIMEOUT, async |p| {
        shortcut_checks::shortcuts_sync(&window, &tab, p)
            .await
            .map_err(|e| format!("{e} (at: {})", p.last()))
    })
    .await;
    restore_shortcuts(&window, browser);

    check(report, "sync_passphrase", Duration::from_secs(60), async |p| {
        sync_checks::sync_passphrase(&window, p)
            .await
            .map_err(|e| format!("{e} (at: {})", p.last()))
    })
    .await;

    check(report, "profiles", DEFAULT_TIMEOUT, async |p| {
        profile_checks::profiles(browser, &window, out_dir, p)
            .await
            .map_err(|e| format!("{e} (at: {})", p.last()))
    })
    .await;

    check(report, "save_page", DEFAULT_TIMEOUT, async |p| {
        shortcut_checks::save_page(&tab, out_dir, p).await
    })
    .await;

    check(report, "page_commands", DEFAULT_TIMEOUT, async |p| {
        page_checks::page_commands(&window, &tab, p).await
    })
    .await;

    check(report, "context_menus", DEFAULT_TIMEOUT, async |p| {
        page_checks::extension_items(&tab, p).await
    })
    .await;

    check(report, "extension_commands", Duration::from_secs(40), async |p| {
        shortcut_checks::extension_commands(&window, &tab, p)
            .await
            .map_err(|e| format!("{e} (at: {})", p.last()))
    })
    .await;
    window.close_scripted_dialog();

    check(report, "extension_notifications", DEFAULT_TIMEOUT, async |p| {
        eval(&tab, "document.documentElement.dataset.vsesvitNotify = 'clear'").await?;
        loop {
            let value = eval(&tab, "document.documentElement.dataset.vsesvitProbeNotified || null").await?;
            p.observe(format!("dataset.vsesvitProbeNotified = {value}"));
            if let Ok(Some(answer)) = serde_json::from_str::<Option<String>>(&value) {
                return Ok(format!("WebView2 gives the probe {answer}"));
            }
            exec::sleep(POLL).await;
        }
    })
    .await;

    let open = tab_ids(&window);
    check(report, "extension_update", 3 * ENGINE_LOAD_TIMEOUT, async |p| {
        extension_update_checks::extension_update(browser, &window, &server, &index, p).await
    })
    .await;
    extension_update_checks::restore(browser).await;
    for id in tab_ids(&window).into_iter().filter(|id| !open.contains(id)) {
        window.close_tab(id);
    }

    check(report, "download", DEFAULT_TIMEOUT, async |p| {
        let dir = out_dir.join("downloads");
        browser.set_download_dir(Some(&dir));
        let url = server.url("/download.bin");
        tab.navigate(url.as_str());
        let entry = until(p, |p| {
            let list = browser.download_list(Browsing::Normal);
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

    check(report, "download_pause", DEFAULT_TIMEOUT, async |p| {
        download_checks::download_pause(browser, &tab, &server.url("/stalled.bin"), p).await
    })
    .await;

    check(report, "download_safety", DEFAULT_TIMEOUT, async |p| {
        let (script, dir) = (server.url("/dangerous.bat"), out_dir.join("downloads"));
        download_checks::download_safety(browser, &window, &tab, &script, &dir, p).await
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
                && ext.verification == Verification::ChromeWebStore
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

/// The window's tabs, to tell a tab opened after this from one open before.
fn tab_ids(window: &BrowserWindow) -> Vec<TabId> {
    window.tabs_in_order().iter().map(|t| t.id).collect()
}

/// Whether `items` hold a link to `url`, at any depth.
pub(crate) fn has_link(items: &[ImportItem], url: &str) -> bool {
    items.iter().any(|item| match item {
        ImportItem::Url { url: link, .. } => link.as_str() == url,
        ImportItem::Folder { children, .. } => has_link(children, url),
        ImportItem::Separator => false,
    })
}

/// The bookmarks bar's list entries, in order.
fn bar_entries(window: &BrowserWindow) -> Vec<IInspectable> {
    let Ok(items) = window
        .bookmarks_bar_list()
        .cast::<ItemsControl>()
        .and_then(|list| list.Items())
    else {
        return Vec::new();
    };
    (0..items.Size().unwrap_or(0))
        .filter_map(|i| items.GetAt(i).ok())
        .collect()
}

/// Back to the default shortcuts with no dialog open, whether or not a shortcuts check passed.
fn restore_shortcuts(window: &BrowserWindow, browser: &Browser) {
    window.close_scripted_dialog();
    if browser.core(|c| c.prefs().keymap()) != vsesvit_core::shortcuts::Keymap::default() {
        browser.edit_keymap(vsesvit_core::shortcuts::Keymap::reset_all);
    }
}

/// Navigates `tab` to `url`, unless it is there, and waits until it settles there.
async fn load(tab: &Rc<Tab>, url: &str, probe: &Probe) {
    if tab.state().url != url {
        tab.navigate(url);
    }
    until(probe, |p| {
        let s = tab.state();
        p.observe(format!("loading {url}: at {} loading={}", s.url, s.loading()));
        (s.url == url && !s.loading()).then_some(())
    })
    .await;
}

/// Emulates the page's pixel ratio at `factor` times the window's scale, which is what zoom
/// changes, or ends that with `None`, and fires the resize a real zoom brings. A scripted run
/// sends no OS input, so it cannot zoom as a person would.
async fn emulate_zoom(tab: &Tab, scale: f64, factor: Option<f64>) -> Result<(), String> {
    let (method, params) = match factor {
        Some(factor) => (
            "Emulation.setDeviceMetricsOverride",
            serde_json::json!({ "width": 0, "height": 0, "deviceScaleFactor": scale * factor, "mobile": false }),
        ),
        None => ("Emulation.clearDeviceMetricsOverride", serde_json::json!({})),
    };
    tab.devtools(method, &params.to_string())
        .await
        .map_err(|e| format!("{method}: {e}"))?;
    eval(tab, "dispatchEvent(new Event('resize')), 0").await?;
    Ok(())
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
    use super::{ImportItem, Url, absent, has_link, prepare, visits};
    use std::io::{Error, ErrorKind};

    #[test]
    fn a_link_is_found_inside_folders() {
        let link = |url| ImportItem::Url {
            title: "Page".into(),
            url: Url::parse(url).unwrap(),
            added_ms: None,
        };
        let items = vec![
            ImportItem::Separator,
            link("http://127.0.0.1/other.html"),
            ImportItem::Folder {
                title: "Bookmarks bar".into(),
                children: vec![ImportItem::Folder {
                    title: "Nested".into(),
                    children: vec![link("http://127.0.0.1/index.html")],
                }],
            },
        ];
        assert!(has_link(&items, "http://127.0.0.1/index.html"));
        assert!(!has_link(&items, "http://127.0.0.1/page2.html"));
    }

    #[test]
    fn popup_titles() {
        assert_eq!(visits("visits=3"), Some(3));
        assert_eq!(visits("visits=0"), Some(0));
        assert_eq!(visits("Vsesvit Probe"), None);
        assert_eq!(visits("visits="), None);
    }

    #[test]
    fn prepare_removes_what_an_earlier_run_left() {
        let dir =
            std::env::temp_dir().join(format!("vsesvit-winui-test-prepare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        prepare(&dir).unwrap();
        std::fs::create_dir_all(dir.join("profile/x")).unwrap();
        std::fs::create_dir_all(dir.join("downloads")).unwrap();
        let files = [
            "report.json",
            "window.png",
            "downloads.png",
            "settings-shortcuts.png",
            "shortcut-capture.png",
            "omnibox-inline.png",
            "new-tab.png",
            "saved-page.mhtml",
            "bookmarks.html",
            "probe.crx",
            "vsesvit.log",
        ];
        for file in files {
            std::fs::write(dir.join(file), "old").unwrap();
        }
        prepare(&dir).unwrap();
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(left.is_empty(), "left behind: {left:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn absent_counts_only_a_missing_file_as_removed() {
        assert!(absent(Err(Error::from(ErrorKind::NotFound))).is_ok());
        let denied = absent(Err(Error::from(ErrorKind::PermissionDenied)));
        assert_eq!(denied.unwrap_err().kind(), ErrorKind::PermissionDenied);
    }
}
