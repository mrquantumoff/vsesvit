//! `vsesvit --self-test OUT_DIR [--network]`: the scripted end-to-end check from
//! `docs/design/self-test.md`, on a fresh profile at `OUT_DIR/profile`. It builds the real
//! browser (`Browser`, windows, address bar, star, toolbar buttons) around that profile and
//! drives those widgets, writing `report.json` and `window.png` and exiting non-zero if any
//! check fails.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::downloads::{State, status_line};
use vsesvit_core::extensions::{ExtensionId, InstallPhase, InstallSource, Verification};
use vsesvit_core::https_only::{self, Reach};
use vsesvit_core::permissions::{Answer, Origin, Permission, Setting};
use vsesvit_core::prefs::{DEFAULT_SYNC_SERVER, TabsPosition, Theme, keys};
use vsesvit_core::search::{EngineForm, NavTarget, SearchEngineId};
use vsesvit_core::shortcuts::{Chord, Command, Keymap};
use vsesvit_core::suggest::DEBOUNCE;
use vsesvit_core::testkit::report::{Check, Report};
use vsesvit_core::testkit::{self, FixtureServer};
use vsesvit_core::trackers::{self, Category, TrackerList, TrackingProtection};
use vsesvit_core::{OpenOptions, Profile};
use vsesvit_webext::menus::Target;
use webkit::prelude::*;

use crate::address_bar::Security;
use crate::browser::Browser;
use crate::dialogs::settings::{HTTPS_ONLY_ROW, PASSWORDS_NOTICE, SECURE_DNS_ROW, TRACKING_PROTECTION_ROW};
use crate::dialogs::{Windowed, shortcut_settings};
use crate::keymap;
use crate::tab::Tab;
use crate::window::{BrowserWindow, Focus, classify_layout};

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// The Web Store install downloads about 10 MB; it gets longer than the default.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(50);
/// The welcome check captures each page twice, in light and dark.
const WELCOME_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a popover gets to open and draw before it is captured.
const POPOVER_SETTLE: Duration = Duration::from_millis(400);
/// uBlock Origin Lite.
const CWS_EXTENSION: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";
/// How many distinct colours the screenshot check samples before it is satisfied.
const COLOR_SAMPLE_CAP: usize = 64;
/// What the fixture server sends for `/download.bin`.
const DOWNLOAD_FIXTURE: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/site/download.bin"));
/// Asks for the location; the page records the outcome: `ok`, or the error code (1 is
/// PERMISSION_DENIED).
const ASK_LOCATION: &str = "delete document.documentElement.dataset.location; navigator.geolocation.getCurrentPosition(() => document.documentElement.dataset.location = 'ok', e => document.documentElement.dataset.location = String(e.code)); 'asked'";
const LOCATION_OUTCOME: &str = "document.documentElement.dataset.location || ''";
const QUERY_LOCATION: &str = "delete document.documentElement.dataset.locationState; navigator.permissions.query({name: 'geolocation'}).then(s => document.documentElement.dataset.locationState = s.state); 'asked'";
const LOCATION_STATE: &str = "document.documentElement.dataset.locationState || ''";
const ASK_CAMERA_AND_MICROPHONE: &str = "delete document.documentElement.dataset.capture; navigator.mediaDevices.getUserMedia({video: true, audio: true}).then(s => { window.captured = s; document.documentElement.dataset.capture = s.getTracks().map(t => t.kind + ':' + t.readyState).sort().join(' '); }, e => document.documentElement.dataset.capture = e.name); 'asked'";
const CAPTURE_OUTCOME: &str = "document.documentElement.dataset.capture || ''";
const CAPTURED_TRACKS: &str = "window.captured.getTracks().map(t => t.kind + ':' + t.readyState).sort().join(' ')";
/// The new tab page's tile links, once its search box is there.
const NEW_TAB_PAGE_PROBE: &str = "document.querySelector('form input') ? [...document.querySelectorAll('.tile')].map(a => a.href).join(' ') : 'no search box'";
/// Selects the fixture page's heading and returns the selected text.
const SELECT_HEADING: &str = "getSelection().selectAllChildren(document.querySelector('h1')); String(getSelection())";

/// Every check the self-test runs, in order; a run that misses one fails.
const CHECKS: [&str; 42] = [
    "profile_open",
    "install_crx",
    "engine_loaded_extension",
    "favicon_preload",
    "navigate",
    "history_recorded",
    "content_script",
    "extension_port",
    "dnr_blocked",
    "bookmark",
    "star_bubble",
    "tabs",
    "tab_animation",
    "tab_layout",
    "tab_menu",
    "popup",
    "extension_toolbar",
    "context_menus",
    "extension_commands",
    "omnibox",
    "address_completion",
    "search_suggestions",
    "selection_search",
    "session",
    "download",
    "new_tab_page",
    "address_progress",
    "settings",
    "search_engines",
    "bookmarks_bar_menus",
    "ctrl_s_toggles_sidebar",
    "shortcuts",
    "save_page",
    "page_commands",
    "zoom_indicator",
    "zoom_is_remembered_per_site",
    "connection_info",
    "site_permissions",
    "tracking_protection",
    "https_only",
    "capture_in_use",
    "welcome",
    "screenshot",
];

/// Only with `--network`.
const NETWORK_CHECK: &str = "cws_install";

fn expected(network: bool) -> Vec<&'static str> {
    let mut names = CHECKS.to_vec();
    if network {
        names.push(NETWORK_CHECK);
    }
    names
}

/// Records one check and prints it as it happens.
fn record(report: &RefCell<Report>, name: &'static str, ok: bool, started: Instant, detail: String) {
    let ms = started.elapsed().as_millis();
    println!("[self-test] {name}: {} ({ms} ms) {detail}", if ok { "ok" } else { "FAIL" });
    report.borrow_mut().push(Check { name, ok, ms, detail });
}

/// The last value a check observed while waiting, reported when it times out.
#[derive(Clone, Default)]
struct Last(Rc<RefCell<String>>);

impl Last {
    fn set(&self, observed: impl Into<String>) {
        *self.0.borrow_mut() = observed.into();
    }

    fn get(&self) -> String {
        self.0.borrow().clone()
    }
}

struct Context {
    out_dir: PathBuf,
    crx_path: PathBuf,
    server: FixtureServer,
    network: bool,
    report: Rc<RefCell<Report>>,
}

pub(crate) fn run(out_dir: &Path, network: bool) -> ExitCode {
    if let Err(e) = std::fs::create_dir_all(out_dir) {
        eprintln!("vsesvit: cannot create {}: {e}", out_dir.display());
        return ExitCode::FAILURE;
    }
    let profile_dir = out_dir.join("profile");
    let _ = std::fs::remove_dir_all(&profile_dir);
    crate::SCRIPTED.set(true);
    let report = Rc::new(RefCell::new(Report::new("linux", expected(network))));

    let started = Instant::now();
    let mut profile = match Profile::open(&profile_dir, OpenOptions::default()) {
        Ok(profile) => {
            record(&report, "profile_open", true, started, format!("root={}", profile_dir.display()));
            profile
        }
        Err(e) => {
            record(&report, "profile_open", false, started, e.to_string());
            return finish(out_dir, &report);
        }
    };
    // Typed searches must not reach the real engine; `search_suggestions` uses a fixture one.
    if let Err(e) = profile.prefs().set(&keys::SEARCH_SUGGESTIONS, &false) {
        record(&report, "search_suggestions_off", false, started, e.to_string());
        return finish(out_dir, &report);
    }
    let server = match FixtureServer::start() {
        Ok(server) => {
            // `favicon_preload` has core fetch a bookmark's icon from the loopback server.
            vsesvit_core::favicons::allow_local_hosts();
            server
        }
        Err(e) => {
            record(&report, "fixture_server", false, started, e.to_string());
            return finish(out_dir, &report);
        }
    };
    let crx_path = out_dir.join("probe.crx");
    if let Err(e) = std::fs::write(&crx_path, testkit::probe_crx()) {
        record(&report, "probe_crx", false, started, e.to_string());
        return finish(out_dir, &report);
    }
    println!("[self-test] fixture server on {}", server.origin());

    let ctx = Rc::new(Context { out_dir: out_dir.to_path_buf(), crx_path, server, network, report: report.clone() });
    let app = adw::Application::builder()
        .application_id("dev.mrquantumoff.vsesvit.SelfTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let pending = Rc::new(RefCell::new(Some(profile)));
    let slot: crate::app::Slot = Rc::default();
    app.connect_startup(glib::clone!(
        #[strong]
        slot,
        #[strong]
        ctx,
        move |app| {
            let Some(profile) = pending.take() else { return };
            crate::app::setup(app, &slot);
            let browser = Browser::new(app, profile);
            browser.start();
            browser.open_startup_windows(&[]);
            slot.replace(Some(browser.clone()));
            let hold = app.hold();
            let app = app.clone();
            let ctx = ctx.clone();
            glib::spawn_future_local(async move {
                run_checks(&ctx, &browser).await;
                for window in browser.windows() {
                    window.close();
                }
                drop(hold);
                app.quit();
            });
        }
    ));
    app.connect_activate(|_| {});
    app.run_with_args::<&str>(&[]);
    slot.take();
    finish(out_dir, &report)
}

fn finish(out_dir: &Path, report: &Rc<RefCell<Report>>) -> ExitCode {
    report.borrow_mut().complete("the self-test stopped before it");
    let report = report.borrow();
    let path = out_dir.join("report.json");
    if let Err(e) = report.write(out_dir) {
        eprintln!("vsesvit: cannot write {}: {e}", path.display());
        return ExitCode::FAILURE;
    }
    let passed = report.checks().iter().filter(|c| c.ok).count();
    println!(
        "[self-test] {}: {passed}/{} checks passed; report at {}",
        if report.ok() { "PASS" } else { "FAIL" },
        report.checks().len(),
        path.display()
    );
    if report.ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

impl Context {
    /// Runs one check under its timeout and records it. The body reports progress through
    /// [`Last`], which becomes the detail when the timeout wins.
    async fn check<F, Fut>(&self, name: &'static str, timeout: Duration, body: F) -> bool
    where
        F: FnOnce(Last) -> Fut,
        Fut: Future<Output = Result<String, String>>,
    {
        let last = Last::default();
        let started = Instant::now();
        let outcome = glib::future_with_timeout(timeout, body(last.clone())).await;
        let (ok, detail) = match outcome {
            Ok(Ok(detail)) => (true, detail),
            Ok(Err(detail)) => (false, detail),
            Err(_) => (false, format!("timed out after {} s; last observed: {}", timeout.as_secs(), last.get())),
        };
        record(&self.report, name, ok, started, detail);
        ok
    }
}

/// Polls `probe` until it succeeds, keeping the last failure in `last`. The check's
/// timeout bounds it.
async fn wait_for<T>(last: &Last, mut probe: impl FnMut() -> Result<T, String>) -> T {
    loop {
        match probe() {
            Ok(value) => return value,
            Err(observed) => {
                last.set(observed);
                glib::timeout_future(POLL).await;
            }
        }
    }
}

async fn eval_js(view: &webkit::WebView, script: &str) -> Result<String, String> {
    match view.evaluate_javascript_future(script, None, None).await {
        Ok(value) if value.is_string() => Ok(value.to_str().to_string()),
        Ok(value) => Ok(value.to_json(0).map(|j| j.to_string()).unwrap_or_default()),
        Err(e) => Err(e.to_string()),
    }
}

fn title_of(view: &webkit::WebView) -> String {
    view.title().map(String::from).unwrap_or_default()
}

async fn run_checks(ctx: &Rc<Context>, browser: &Browser) {
    let Some(window) = browser.windows().into_iter().next() else {
        record(&ctx.report, "window", false, Instant::now(), "startup opened no window".into());
        return;
    };
    let probe_id = ExtensionId::parse(testkit::PROBE_ID).expect("the probe id is valid");
    let index_url = ctx.server.url("/index.html");
    let page2_url = ctx.server.url("/page2.html");
    // Every check's future captures references, so the checks share these values.
    let (window, index_url, page2_url, probe_id) = (&window, &index_url, &page2_url, &probe_id);

    ctx.check("install_crx", CHECK_TIMEOUT, |_| async move {
        let source = InstallSource::from_path(&ctx.crx_path).map_err(|e| e.to_string())?;
        let ext = browser
            .install(source, |_| {})
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "the install committed nothing".to_owned())?;
        let detail = format!("id={} verification={:?} dir={}", ext.id.as_str(), ext.verification, ext.dir.display());
        if ext.id == *probe_id && ext.verification == Verification::LocalCrx { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("engine_loaded_extension", CHECK_TIMEOUT, |last| async move {
        let loaded: Vec<String> = browser.runtime().loaded().iter().map(|id| id.as_str().to_owned()).collect();
        if !loaded.iter().any(|id| id == testkit::PROBE_ID) {
            return Err(format!("runtime loaded {loaded:?}"));
        }
        // The DNR ruleset compiles in the background; navigating before it is attached
        // would make `dnr_blocked` a race.
        wait_for(&last, || {
            let pending = browser.runtime().pending_filters();
            if pending == 0 { Ok(()) } else { Err(format!("{pending} content blocker(s) still compiling")) }
        })
        .await;
        Ok(format!("runtime loaded {loaded:?}, content blockers attached"))
    })
    .await;

    ctx.check("favicon_preload", CHECK_TIMEOUT, |last| async move {
        let url = ctx.server.url("/icon.html");
        let id = browser.core().borrow_mut().bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Icon page", &url).map_err(|e| e.to_string())?;
        browser.bookmarks_changed();
        wait_for(&last, || {
            let stored = browser.core().borrow_mut().favicons().get(&url).map_err(|e| e.to_string())?.is_some();
            let shown = window.bookmarks_bar().shows_favicon(url.as_str());
            if stored && shown { Ok(()) } else { Err(format!("icon stored={stored}, shown on the bar={shown}")) }
        })
        .await;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("favicon-preload.png")).await.map_err(|e| e.to_string())?;
        let visited = ctx.server.hits().iter().any(|path| path == "/icon.html");
        browser.core().borrow_mut().bookmarks().remove(id).map_err(|e| e.to_string())?;
        browser.bookmarks_changed();
        Ok(format!("the bar shows the icon of {url}, fetched without a tab (page fetched by core: {visited}); favicon-preload.png"))
    })
    .await;

    ctx.check("navigate", CHECK_TIMEOUT, |last| async move {
        window.address_bar().submit_text(index_url.as_str());
        let tab = wait_for(&last, || window.selected_tab().ok_or_else(|| "no selected tab".to_owned())).await;
        wait_for(&last, || {
            let title = title_of(tab.web_view());
            let uri = tab.committed_uri().unwrap_or_default();
            if title == "Vsesvit fixture" && uri == index_url.as_str() {
                Ok(())
            } else {
                Err(format!("title={title:?} committed={uri:?}"))
            }
        })
        .await;
        Ok("title=Vsesvit fixture".to_owned())
    })
    .await;

    ctx.check("history_recorded", CHECK_TIMEOUT, |last| async move {
        let entry = wait_for(&last, || {
            let found = browser.core().borrow_mut().history().search(index_url.as_str(), 20);
            let found = found.map_err(|e| e.to_string())?;
            found
                .into_iter()
                .find(|e| e.url == *index_url)
                .ok_or_else(|| "no history entry for the fixture url yet".to_owned())
        })
        .await;
        Ok(format!("url={} visits={} title={:?}", entry.url, entry.visit_count, entry.title))
    })
    .await;

    ctx.check("content_script", CHECK_TIMEOUT, |last| async move {
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        loop {
            let value = eval_js(tab.web_view(), "document.documentElement.dataset.vsesvitProbe")
                .await
                .unwrap_or_else(|e| format!("error: {e}"));
            if value == "background-replied" {
                let visits = eval_js(tab.web_view(), "document.documentElement.dataset.vsesvitVisits").await.unwrap_or_default();
                return Ok(format!("vsesvitProbe={value} vsesvitVisits={visits}"));
            }
            last.set(format!("vsesvitProbe={value:?}"));
            glib::timeout_future(POLL * 2).await;
        }
    })
    .await;

    ctx.check("extension_port", CHECK_TIMEOUT, |last| async move {
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        loop {
            let value = eval_js(tab.web_view(), "String(document.documentElement.dataset.vsesvitProbePort)").await.unwrap_or_else(|e| format!("error: {e}"));
            if value == "pong:probe" {
                return Ok(format!("vsesvitProbePort={value}"));
            }
            last.set(format!("vsesvitProbePort={value:?}"));
            glib::timeout_future(POLL * 2).await;
        }
    })
    .await;

    ctx.check("dnr_blocked", CHECK_TIMEOUT, |_| async move {
        glib::timeout_future(Duration::from_secs(1)).await;
        let hits = ctx.server.hits();
        let allowed = hits.iter().any(|p| p == "/allowed.png");
        let blocked = hits.iter().any(|p| p == "/vsesvit-blocked/pixel.png");
        let detail = format!("server saw {hits:?}");
        if allowed && !blocked { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("bookmark", CHECK_TIMEOUT, |last| async move {
        gio::prelude::ActionGroupExt::activate_action(window, "bookmark-page", None);
        wait_for(&last, || {
            let starred = browser.core().borrow_mut().bookmarks().is_bookmarked(index_url);
            let on_bar = window.bookmarks_bar().button_for(index_url.as_str()).is_some();
            let star = window.action_state("bookmark-page").and_then(|v| v.get::<bool>()).unwrap_or(false);
            if starred && on_bar && star {
                Ok(())
            } else {
                Err(format!("is_bookmarked={starred} bar_shows_it={on_bar} star_active={star}"))
            }
        })
        .await;
        Ok("is_bookmarked=true, the bar shows the item, the star is active".to_owned())
    })
    .await;

    ctx.check("star_bubble", CHECK_TIMEOUT, |last| async move {
        let address = window.address_bar();
        let bubble = address.bubble().ok_or_else(|| "the star opened no bubble".to_owned())?;
        let heading = heading_of(&bubble);
        if heading.as_deref() != Some("Bookmark added") {
            return Err(format!("the bubble is titled {heading:?}"));
        }
        glib::timeout_future(POPOVER_SETTLE).await;
        // Scripted popovers take no keyboard focus (see `crate::popup`), so this checks the
        // selection that comes with focusing the Name field.
        let name = find::<gtk::Entry>(bubble.upcast_ref(), |_| true).ok_or_else(|| "the bubble has no Name field".to_owned())?;
        let selected = name.selection_bounds().is_some_and(|(start, end)| start == 0 && end == i32::from(name.text_length()));
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&bubble), &ctx.out_dir.join("star-bubble.png"))
            .await
            .map_err(|e| e.to_string())?;
        name.set_text("Renamed fixture");
        button_labelled(bubble.upcast_ref(), "_Done").ok_or_else(|| "no Done button".to_owned())?.emit_clicked();
        wait_for(&last, || {
            let titles: Vec<String> = browser.core().borrow_mut().bookmarks().find_by_url(index_url).into_iter().map(|n| n.title).collect();
            if titles == ["Renamed fixture"] { Ok(()) } else { Err(format!("the bookmark is titled {titles:?}")) }
        })
        .await;
        gio::prelude::ActionGroupExt::activate_action(window, "bookmark-page", None);
        let again = address.bubble().as_ref().and_then(heading_of);
        if let Some(bubble) = address.bubble() {
            bubble.popdown();
        }
        match (selected, again.as_deref()) {
            (true, Some("Edit bookmark")) => Ok("\"Bookmark added\" with the name selected (star-bubble.png); Done saved the new name; the star then opens \"Edit bookmark\"".to_owned()),
            _ => Err(format!("name selected={selected}, second click opened {again:?}")),
        }
    })
    .await;

    ctx.check("tabs", CHECK_TIMEOUT, |last| async move {
        let first = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let second = window.open_tab(Some(page2_url.as_str()), None, Focus::Foreground);
        wait_for(&last, || {
            let title = title_of(second.web_view());
            if title == "Vsesvit fixture 2" && window.selected_tab().as_ref() == Some(&second) {
                Ok(())
            } else {
                Err(format!("second tab title={title:?} tabs={}", window.tabs().len()))
            }
        })
        .await;
        window.select_tab(&first);
        window.close_tab(&second);
        wait_for(&last, || {
            let tabs = window.tabs();
            let selected = window.selected_tab();
            let uri = selected.as_ref().and_then(|t| t.committed_uri()).unwrap_or_default();
            if tabs.len() == 1 && selected.as_ref() == Some(&first) && uri == index_url.as_str() {
                Ok(())
            } else {
                Err(format!("tabs={} selected shows {uri:?}", tabs.len()))
            }
        })
        .await;
        Ok("second tab opened, switched back, closed; one tab remains on index.html".to_owned())
    })
    .await;

    ctx.check("tab_animation", CHECK_TIMEOUT, |last| async move {
        let (list, observed) = (window.tab_list(), &last);
        let settled = move || {
            wait_for(observed, move || match list.row_counts() {
                (live, 0, true) if live == window.tabs().len() => Ok(()),
                (live, leaving, settled) => Err(format!("{live} rows for {} tabs, {leaving} leaving, settled={settled}", window.tabs().len())),
            })
        };
        let animations = gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations());
        settled().await;
        let first = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let before = window.tabs().len();

        let tab = window.open_tab(Some(page2_url.as_str()), None, Focus::Foreground);
        let opened = (window.tabs().len(), list.row_counts().0, window.tab_row_opacity(&tab));
        settled().await;
        let shown = window.tab_row_opacity(&tab);
        wait_for(&last, || match tab.committed_uri() {
            Some(uri) if uri == page2_url.as_str() => Ok(()),
            uri => Err(format!("the new tab is at {uri:?}")),
        })
        .await;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("tab-opened.png")).await.map_err(|e| e.to_string())?;

        window.select_tab(&first);
        window.close_tab(&tab);
        let (tabs_at_close, (live_at_close, leaving_at_close, _)) = (window.tabs().len(), list.row_counts());
        browser.reopen_closed_tab(window);
        let reopened = window.selected_tab().filter(|t| *t != first).ok_or_else(|| "reopening selected no tab".to_owned())?;
        let tabs_reopened = window.tabs().len();
        wait_for(&last, || match reopened.committed_uri() {
            Some(uri) if uri == page2_url.as_str() => Ok(()),
            uri => Err(format!("the reopened tab is at {uri:?}")),
        })
        .await;
        window.select_tab(&first);
        window.close_tab(&reopened);
        let tabs_after = window.tabs().len();
        settled().await;
        let rows_after = list.row_counts().0;

        let detail = format!(
            "animations enabled={animations}; opening: tabs {before} -> {}, rows {}, the new row's opacity {:?} at once and {shown:?} settled (tab-opened.png); closing: tabs {tabs_at_close} and rows {live_at_close} at once, {leaving_at_close} row leaving; reopening gave {tabs_reopened} tabs on page2.html; closed again: {tabs_after} tabs, {rows_after} rows, all opaque",
            opened.0, opened.1, opened.2
        );
        let grows = !animations || opened.2.is_some_and(|o| o < 1.0);
        let leaves = leaving_at_close == usize::from(animations);
        let ok = opened.0 == before + 1
            && opened.1 == before + 1
            && grows
            && shown == Some(1.0)
            && tabs_at_close == before
            && live_at_close == before
            && leaves
            && tabs_reopened == before + 1
            && tabs_after == before
            && rows_after == before;
        if ok { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("tab_layout", CHECK_TIMEOUT, |last| async move {
        let mut details = Vec::new();
        let expect = |position: TabsPosition, last: &Last| {
            let last = last.clone();
            async move {
                wait_for(&last, || {
                    let probe = window.layout_probe();
                    let seen = classify_layout(&probe);
                    if seen == Some(position) { Ok(probe) } else { Err(format!("expected {position:?}, geometry shows {seen:?}: {probe}")) }
                })
                .await
            }
        };
        let probe = expect(TabsPosition::Left, &last).await;
        details.push(format!("fresh profile -> left ({probe})"));
        for position in [TabsPosition::Right, TabsPosition::Top, TabsPosition::Left] {
            browser.set_tabs_position(position);
            let probe = expect(position, &last).await;
            details.push(format!("{position:?} ({probe})"));
        }
        let stored = browser.tabs_position();
        if stored != TabsPosition::Left {
            return Err(format!("tabs.position ended as {stored:?}"));
        }
        Ok(details.join("; "))
    })
    .await;

    ctx.check("tab_menu", CHECK_TIMEOUT, |last| async move {
        let first = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let tracked = format!("{page2_url}?utm_source=self-test&a=1");
        let clean = format!("{page2_url}?a=1");
        let second = window.open_tab(Some(&tracked), None, Focus::Background);
        wait_for(&last, || match second.committed_uri() {
            Some(uri) if uri == tracked => Ok(()),
            uri => Err(format!("the second tab is at {uri:?}")),
        })
        .await;
        let choose = |tab: &Tab, action: &str| -> Result<(), String> {
            let menu = window.open_tab_menu(tab).ok_or_else(|| "the row opened no menu".to_owned())?;
            menu.popdown();
            WidgetExt::activate_action(window, &format!("tab.{action}"), None).map_err(|e| format!("tab.{action}: {e}"))
        };
        let copied = || async { window.clipboard().read_text_future().await.ok().flatten().map(String::from).unwrap_or_default() };
        let mut details = Vec::new();

        let menu = window.open_tab_menu(&first).ok_or_else(|| "the first tab's row opened no menu".to_owned())?;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), &[menu.clone().upcast()], &ctx.out_dir.join("tab-menu.png"))
            .await
            .map_err(|e| e.to_string())?;
        menu.popdown();
        let lines = window.tab_menu_lines();
        let reopen = browser.can_reopen_closed_tab();
        let wanted = [
            vec![("_New Tab Below", true), ("Move Tab to New _Window", true)],
            vec![("_Reload", true), ("_Duplicate", true), ("P_in Tab", true), ("_Mute Tab", true), ("Copy _Link", true)],
            vec![("_Close Tab", true), ("Close _Other Tabs", true), ("Close Tabs _Below", true), ("R_eopen Closed Tab", reopen)],
        ];
        let wanted: Vec<Vec<(String, bool)>> = wanted.iter().map(|s| s.iter().map(|&(l, e)| (l.to_owned(), e)).collect()).collect();
        if lines != wanted {
            return Err(format!("the menu reads {lines:?}"));
        }
        details.push("the menu has Chrome's items, with Below in the sidebar (tab-menu.png)".to_owned());

        choose(&second, "copy-link")?;
        let from_menu = copied().await;
        let accels = browser.app().accels_for_action("win.copy-clean-link");
        window.select_tab(&second);
        gio::prelude::ActionGroupExt::activate_action(window, "copy-link", None);
        let whole = copied().await;
        gio::prelude::ActionGroupExt::activate_action(window, "copy-clean-link", None);
        let from_key = copied().await;
        let page_menu = webkit::ContextMenu::new();
        page_menu.append(&webkit::ContextMenuItem::from_stock_action(webkit::ContextMenuAction::CopyLinkToClipboard));
        page_menu.append(&webkit::ContextMenuItem::from_stock_action(webkit::ContextMenuAction::InspectElement));
        let item = crate::page_menu::add_link_item(&second, &page_menu, &tracked);
        let at = page_menu.items().iter().position(|i| i == &item);
        item.gaction().ok_or_else(|| "the link item has no action".to_owned())?.activate(None);
        let from_page = copied().await;
        window.select_tab(&first);
        if from_menu != clean || whole != tracked || from_key != clean || from_page != clean || at != Some(1) || !accels.iter().any(|a| gtk::accelerator_parse(a) == gtk::accelerator_parse("<Control><Shift>c")) {
            return Err(format!(
                "Copy Link gave {from_menu:?}, win.copy-link {whole:?}, win.copy-clean-link ({accels:?}) {from_key:?}, the link's item at {at:?} {from_page:?}"
            ));
        }
        details.push(format!("Copy Link, Ctrl+Shift+C and the link's Copy Link Without Tracking copy {clean}"));

        choose(&first, "duplicate")?;
        let copy = window.selected_tab().filter(|t| *t != first).ok_or_else(|| "duplicating selected no new tab".to_owned())?;
        wait_for(&last, || match (window.tabs(), copy.committed_uri()) {
            (tabs, Some(uri)) if tabs == [first.clone(), copy.clone(), second.clone()] && uri == index_url.as_str() => Ok(()),
            (tabs, uri) => Err(format!("{} tabs; the duplicate is at {uri:?}", tabs.len())),
        })
        .await;
        details.push("Duplicate opened the page again next to it".to_owned());

        choose(&second, "pin")?;
        let row = window.tab_row_buttons(&second);
        let saved = crate::session::snapshot(browser).windows.iter().flat_map(|w| w.tabs.clone()).find(|t| t.id == second.session_id()).map(|t| t.pinned);
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("tab-pinned.png")).await.map_err(|e| e.to_string())?;
        window.open_tab_menu(&second).ok_or_else(|| "the pinned row opened no menu".to_owned())?.popdown();
        let unpin = window.tab_menu_lines().concat().iter().any(|(label, _)| label == "Unp_in Tab");
        let leads = window.tabs().first() == Some(&second);
        if !leads || !window.is_pinned(&second) || row != Some((false, true)) || saved != Some(true) || !unpin {
            return Err(format!("after Pin Tab: first={leads} row (close, pin)={row:?} saved pinned={saved:?} menu offers Unpin={unpin}"));
        }
        details.push("Pin Tab moved it first, swapped its close button for a pin, and the session saves it pinned (tab-pinned.png)".to_owned());

        choose(&second, "mute")?;
        let muted = (second.web_view().is_muted(), window.tab_indicator(&second));
        let clicked = window.click_tab_indicator(&second);
        let unmuted = (second.web_view().is_muted(), window.tab_indicator(&second));
        if muted != (true, Some(("audio-volume-muted-symbolic".to_owned(), "Unmute Tab".to_owned()))) || !clicked || unmuted != (false, None) {
            return Err(format!("muted {muted:?}; clicking the speaker ({clicked}) left {unmuted:?}"));
        }
        details.push("Mute Tab muted it and showed the muted speaker, which unmutes it".to_owned());

        choose(&first, "close-others")?;
        let after_others = window.tabs();
        choose(&first, "reopen-closed")?;
        let reopened = window.selected_tab().filter(|t| *t != first).ok_or_else(|| "reopening selected no tab".to_owned())?;
        let after_reopen = window.tabs();
        wait_for(&last, || match reopened.committed_uri() {
            Some(uri) if uri == index_url.as_str() => Ok(()),
            uri => Err(format!("the reopened tab is at {uri:?}")),
        })
        .await;
        choose(&first, "close-after")?;
        let after_below = window.tabs();
        choose(&first, "new-tab-next")?;
        let new = window.selected_tab().filter(|t| *t != first).ok_or_else(|| "New Tab Below selected no new tab".to_owned())?;
        let new_at = window.tabs().iter().position(|t| *t == new);
        let blank = new.is_blank();
        window.close_tab(&new);
        let pair = [second.clone(), first.clone()];
        if after_others != pair || after_reopen != [second.clone(), first.clone(), reopened.clone()] || after_below != pair || new_at != Some(2) || !blank {
            return Err(format!(
                "tabs after Close Other Tabs {}, Reopen Closed Tab {}, Close Tabs Below {}; New Tab Below at {new_at:?}, blank={blank}",
                after_others.len(),
                after_reopen.len(),
                after_below.len()
            ));
        }
        details.push("Close Other Tabs kept the pinned tab, Reopen Closed Tab brought the closed one back, Close Tabs Below closed it, New Tab Below opened under the tab".to_owned());

        let windows_before = browser.windows().len();
        choose(&second, "move-to-new-window")?;
        let moved = browser.windows().into_iter().find(|w| w != window).ok_or_else(|| "no new window".to_owned())?;
        let moved_tabs = (moved.tabs() == [second.clone()], moved.is_pinned(&second), window.tabs() == [first.clone()]);
        moved.close();
        wait_for(&last, || {
            let open = browser.windows().len();
            if open == windows_before { Ok(()) } else { Err(format!("{open} windows after closing the moved tab's")) }
        })
        .await;
        window.select_tab(&first);
        if moved_tabs != (true, true, true) {
            return Err(format!("after Move Tab to New Window (alone there, pinned, left alone here): {moved_tabs:?}"));
        }
        details.push("Move Tab to New Window moved it, still pinned".to_owned());
        Ok(details.join("; "))
    })
    .await;

    ctx.check("popup", CHECK_TIMEOUT, |last| async move {
        let button = wait_for(&last, || {
            window.extension_action_button(probe_id).ok_or_else(|| "no toolbar button for the probe".to_owned())
        })
        .await;
        button.emit_clicked();
        let view = wait_for(&last, || window.extension_popup_view().ok_or_else(|| "no popup view".to_owned())).await;
        let title = wait_for(&last, || {
            let title = title_of(&view);
            if title.starts_with("visits=") { Ok(title) } else { Err(format!("popup title={title:?}")) }
        })
        .await;
        let visits: u64 = title.strip_prefix("visits=").and_then(|n| n.parse().ok()).unwrap_or(0);
        window.close_extension_popup();
        if visits >= 1 { Ok(format!("popup title={title}")) } else { Err(format!("popup title={title}")) }
    })
    .await;

    ctx.check("extension_toolbar", CHECK_TIMEOUT, |last| async move {
        let saved_pin = || {
            let entries = browser.core().borrow_mut().prefs().get(&vsesvit_core::extensions::toolbar::TOOLBAR);
            entries.into_iter().find(|e| e.id == probe_id.as_str()).map(|e| e.pinned)
        };
        let pinned_at_start = window.extension_action_button(probe_id).is_some();

        let context = window.open_extension_context_menu(probe_id).ok_or_else(|| "the probe has no toolbar button".to_owned())?;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), &[context.clone().upcast()], &ctx.out_dir.join("extensions-context-menu.png"))
            .await
            .map_err(|e| e.to_string())?;
        context.popdown();

        let menu = window.open_extensions_menu().ok_or_else(|| "the puzzle piece opened no menu".to_owned())?;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&menu), &ctx.out_dir.join("extensions-menu.png"))
            .await
            .map_err(|e| e.to_string())?;
        let pin = find::<gtk::Button>(menu.upcast_ref(), |b| b.icon_name().as_deref() == Some("view-pin-symbolic"))
            .ok_or_else(|| "the menu has no pin toggle".to_owned())?;
        pin.emit_clicked();
        wait_for(&last, || match (window.extension_action_button(probe_id), saved_pin()) {
            (None, Some(false)) => Ok(()),
            (button, saved) => Err(format!("after unpinning: button shown={}, saved pin={saved:?}", button.is_some())),
        })
        .await;
        let open = find::<gtk::Button>(menu.upcast_ref(), |b| b.label().is_none() && b.icon_name().is_none())
            .ok_or_else(|| "the menu has no row for the probe".to_owned())?;
        open.emit_clicked();
        let view = wait_for(&last, || window.extension_popup_view().ok_or_else(|| "no popup from the menu".to_owned())).await;
        let popup = view.ancestor(gtk::Popover::static_type()).and_downcast::<gtk::Popover>().ok_or_else(|| "the popup page is in no popover".to_owned())?;
        let from_puzzle = popup.parent().is_some_and(|p| p.tooltip_text().as_deref() == Some("Extensions"));
        glib::timeout_future(Duration::from_millis(800)).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), &[popup], &ctx.out_dir.join("extensions-unpinned-popup.png"))
            .await
            .map_err(|e| e.to_string())?;
        window.close_extension_popup();

        gio::prelude::ActionGroupExt::activate_action(window, "extension-pin", Some(&(probe_id.as_str(), true).to_variant()));
        wait_for(&last, || match (window.extension_action_button(probe_id), saved_pin()) {
            (Some(_), Some(true)) => Ok(()),
            (button, saved) => Err(format!("after pinning again: button shown={}, saved pin={saved:?}", button.is_some())),
        })
        .await;
        let detail = format!("pinned at start={pinned_at_start}; the menu's pin toggle unpinned it (saved in toolbar.extensions); its row opened the popup from the puzzle piece={from_puzzle}; pinned again; extensions-*.png");
        if pinned_at_start && from_puzzle { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("context_menus", CHECK_TIMEOUT, |last| async move {
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        if tab.committed_uri().as_deref() != Some(index_url.as_str()) {
            return Err(format!("the selected tab shows {:?}", tab.committed_uri()));
        }
        let clicked = async |what: &str| {
            let click = |seen: &str| serde_json::from_str::<serde_json::Value>(seen).unwrap_or_default();
            let seen = wait_js(&last, tab.web_view(), "String(document.documentElement.dataset.vsesvitProbeMenu)", |seen| click(seen)["id"] == what).await;
            click(&seen)
        };
        let listed = |menu: &webkit::ContextMenu| -> Vec<String> {
            menu.items()
                .iter()
                .map(|item| match item.stock_action() {
                    _ if item.is_separator() => "-".to_owned(),
                    webkit::ContextMenuAction::Custom => item.title().map(String::from).unwrap_or_default(),
                    stock => format!("{stock:?}"),
                })
                .collect()
        };

        let menu = webkit::ContextMenu::new();
        menu.append(&webkit::ContextMenuItem::from_stock_action(webkit::ContextMenuAction::Reload));
        menu.append(&webkit::ContextMenuItem::new_separator());
        menu.append(&webkit::ContextMenuItem::from_stock_action(webkit::ContextMenuAction::InspectElement));
        let page = Target { page_url: index_url.to_string(), ..Target::default() };
        let added = crate::page_menu::add_extension_items(&tab, &menu, &page);
        let on_page = listed(&menu);
        if on_page != ["Reload", "-", "Vsesvit Probe page item", "-", "InspectElement"] {
            return Err(format!("the page's context menu lists {on_page:?}"));
        }
        let link = Target { link_url: Some(page2_url.to_string()), ..page.clone() };
        let on_link = crate::page_menu::add_extension_items(&tab, &webkit::ContextMenu::new(), &link).len();
        if on_link != 0 {
            return Err(format!("a link's context menu got {on_link} extension items"));
        }
        added.get(1).and_then(|item| item.gaction()).ok_or_else(|| "the probe's item has no action".to_owned())?.activate(None);
        let from_page = clicked("probe-page").await;
        if from_page["pageUrl"] != index_url.as_str() || from_page["tab"] != tab.id().0 {
            return Err(format!("contextMenus.onClicked got {from_page}"));
        }

        let action_menu = window.open_extension_context_menu(probe_id).ok_or_else(|| "the probe has no toolbar button".to_owned())?;
        let model = action_menu.menu_model().ok_or_else(|| "the action's menu has no model".to_owned())?;
        let first = model.item_link(0, gio::MENU_LINK_SECTION).ok_or_else(|| "the action's menu has no sections".to_owned())?;
        let label = first.item_attribute_value(0, gio::MENU_ATTRIBUTE_LABEL, None).and_then(|v| v.get::<String>());
        let action = first.item_attribute_value(0, gio::MENU_ATTRIBUTE_ACTION, None).and_then(|v| v.get::<String>());
        if label.as_deref() != Some("Vsesvit Probe action item") {
            return Err(format!("the action's menu starts with {label:?}"));
        }
        let action = action.ok_or_else(|| "the action item has no action".to_owned())?;
        WidgetExt::activate_action(&action_menu, &action, None).map_err(|e| e.to_string())?;
        action_menu.popdown();
        let from_action = clicked("probe-action").await;
        if from_action["pageUrl"] != serde_json::Value::Null || from_action["tab"] != tab.id().0 {
            return Err(format!("contextMenus.onClicked from the action's menu got {from_action}"));
        }
        Ok(format!("the page's context menu lists {on_page:?}, a link's none of the probe's; choosing the item fired onClicked with {from_page}; the action's menu starts with {label:?}, which fired {from_action}"))
    })
    .await;

    ctx.check("extension_commands", CHECK_TIMEOUT, |last| async move {
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        if tab.committed_uri().as_deref() != Some(index_url.as_str()) {
            return Err(format!("the selected tab shows {:?}", tab.committed_uri()));
        }
        let app = browser.app();
        let (action, named) = (keymap::extension_action(probe_id, "_execute_action"), keymap::extension_action(probe_id, "probe-command"));
        let accels = |detailed: &str| -> Vec<String> { app.accels_for_action(detailed).iter().map(|a| a.to_string()).collect() };
        let has = |detailed: &str, want: &str| accels(detailed).iter().map(|a| gtk::accelerator_parse(a.as_str())).eq([gtk::accelerator_parse(want)]);
        if !has(&action, "<Alt><Shift>p") || !has(&named, "<Alt><Shift>k") {
            return Err(format!("{action} has {:?} and {named} has {:?}", accels(&action), accels(&named)));
        }
        let _restore = Cleanup(|| {
            browser.edit_keymap(|keymap| {
                let extensions = browser.extension_shortcuts(keymap);
                keymap.reset_extension(&extensions, probe_id, "probe-command");
            });
        });

        gio::prelude::ActionGroupExt::activate_action(window, "extension-command", Some(&(probe_id.as_str(), "probe-command").to_variant()));
        let seen = wait_js(&last, tab.web_view(), "String(document.documentElement.dataset.vsesvitProbeCommand)", |seen| {
            serde_json::from_str::<serde_json::Value>(seen).is_ok_and(|command| command["name"] == "probe-command")
        })
        .await;
        let command: serde_json::Value = serde_json::from_str(&seen).unwrap_or_default();
        if command["tab"] != tab.id().0 || command["url"] != index_url.as_str() {
            return Err(format!("commands.onCommand got {command}"));
        }

        gio::prelude::ActionGroupExt::activate_action(window, "extension-command", Some(&(probe_id.as_str(), "_execute_action").to_variant()));
        let popup = wait_for(&last, || window.extension_popup_view().ok_or_else(|| "no popup from _execute_action".to_owned())).await;
        let popup_url = popup.uri().map(String::from).unwrap_or_default();
        window.close_extension_popup();
        if !popup_url.ends_with("/popup.html") || window.extension_popup_view().is_some() {
            return Err(format!("_execute_action opened {popup_url:?}; still open after closing: {}", window.extension_popup_view().is_some()));
        }

        gio::prelude::ActionGroupExt::activate_action(window, "show-settings", None);
        let dialog = window
            .visible_dialog()
            .and_downcast::<adw::PreferencesDialog>()
            .ok_or_else(|| "win.show-settings opened no preferences dialog".to_owned())?;
        let edited = async {
            dialog.set_visible_page_name("shortcuts");
            let page = dialog.visible_page().ok_or_else(|| "the shortcuts page is not shown".to_owned())?;
            let row = find::<adw::ActionRow>(page.upcast_ref(), |row| row.title() == "Vsesvit Probe command" && row.subtitle().as_deref() == Some("Vsesvit Probe"))
                .ok_or_else(|| "Settings > Shortcuts has no row for the probe's command".to_owned())?;
            let shown = || find::<adw::ShortcutLabel>(row.upcast_ref(), |_| true).map(|label| label.accelerator().to_string());
            let listed = shown();
            if listed.as_deref() != Some("<Alt><Shift>k") {
                return Err(format!("the probe's command row shows {listed:?}"));
            }
            glib::timeout_future(Duration::from_millis(300)).await;
            if let Some(scrolled) = row.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>() {
                let at = scrolled.vadjustment();
                at.set_value(at.upper() - at.page_size());
            }
            glib::timeout_future(Duration::from_millis(300)).await;
            crate::screenshot::save_png(window, &ctx.out_dir.join("extension-shortcuts.png")).await.map_err(|e| e.to_string())?;
            let alt_shift_j = "Alt+Shift+J".parse::<Chord>().map_err(|e| e.to_string())?;
            browser.edit_keymap(|keymap| {
                let extensions = browser.extension_shortcuts(keymap);
                keymap.assign_extension(&extensions, probe_id, "probe-command", Some(alt_shift_j));
            });
            let moved = (accels(&named), app.actions_for_accel("<Alt><Shift>k").len());
            if !has(&named, "<Alt><Shift>j") || moved.1 != 0 {
                return Err(format!("after assigning Alt+Shift+J, {named} has {:?} and Alt+Shift+K runs {} actions", moved.0, moved.1));
            }
            let reset = find::<gtk::Button>(row.upcast_ref(), |b| b.tooltip_text().as_deref() == Some("Reset to Default"))
                .ok_or_else(|| "the probe's command row has no Reset to Default button".to_owned())?;
            reset.emit_clicked();
            let back = shown();
            if !has(&named, "<Alt><Shift>k") || back.as_deref() != Some("<Alt><Shift>k") || reset.is_visible() {
                return Err(format!("after Reset to Default, {named} has {:?}, the row shows {back:?}, its reset button shown={}", accels(&named), reset.is_visible()));
            }
            Ok(format!("its Settings row lists {listed:?} (extension-shortcuts.png); Alt+Shift+J moved it to {:?}; the row's Reset to Default brought back {back:?}", moved.0))
        }
        .await;
        dialog.close();
        let edited = edited?;
        Ok(format!("{action} is Alt+Shift+P and {named} Alt+Shift+K; probe-command fired onCommand with {command}; _execute_action opened {popup_url}, closed again; {edited}"))
    })
    .await;

    ctx.check("omnibox", CHECK_TIMEOUT, |_| async move {
        let typed_url = format!("127.0.0.1:{}/page2.html", ctx.server.port());
        let (search, url, default) = {
            let mut profile = browser.core().borrow_mut();
            let default = profile.search_engines().default_engine().map_err(|e| e.to_string())?;
            let mut omnibox = profile.omnibox();
            let search = omnibox.resolve("vsesvit fixture").map_err(|e| e.to_string())?;
            let url = omnibox.resolve(&typed_url).map_err(|e| e.to_string())?;
            (search, url, default)
        };
        match (search, url) {
            (Some(NavTarget::Search { engine, url: search_url }), Some(NavTarget::Url(resolved)))
                if engine == default.id && resolved == *page2_url =>
            {
                Ok(format!("\"vsesvit fixture\" -> search on {} ({search_url}); \"{typed_url}\" -> {resolved}", default.name))
            }
            (search, url) => Err(format!("search={search:?} url={url:?} default={}", default.name)),
        }
    })
    .await;

    ctx.check("address_completion", CHECK_TIMEOUT, |last| async move {
        let address = window.address_bar();
        let none = gdk::ModifierType::empty();
        let typed = "127.0.0";
        let completed = format!("127.0.0.1:{}", ctx.server.port());
        let completion = Some((char_len(typed), char_len(&completed)));
        let root = ctx.server.url("/");
        address.focus_for_typing();
        address.type_text(typed);
        wait_for(&last, || {
            let seen = address.observe();
            if seen.open && seen.text == completed && seen.selection == completion && seen.highlighted == Some(0) {
                Ok(())
            } else {
                Err(format!("after typing {typed:?}: {seen:?}"))
            }
        })
        .await;
        glib::timeout_future(POPOVER_SETTLE).await;
        let list = address.suggestions_popover();
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&list), &ctx.out_dir.join("address-completion.png"))
            .await
            .map_err(|e| e.to_string())?;
        if list.width() < address.width() {
            return Err(format!("the list is {} px wide under a {} px address bar", list.width(), address.width()));
        }
        address.press(gdk::Key::Down, none);
        let down = address.observe();
        if down.highlighted != Some(1) || down.fills.get(1) != Some(&down.text) || down.selection.is_some() {
            return Err(format!("after Down: {down:?}"));
        }
        address.press(gdk::Key::Escape, none);
        let escaped = address.observe();
        if escaped.highlighted != Some(0) || escaped.text != completed || escaped.selection != completion {
            return Err(format!("after Escape: {escaped:?}"));
        }
        address.press(gdk::Key::Return, none);
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        wait_for(&last, || {
            let uri = tab.committed_uri().unwrap_or_default();
            if uri == root.as_str() { Ok(()) } else { Err(format!("after Enter the tab shows {uri:?}")) }
        })
        .await;
        address.submit_text(index_url.as_str());
        wait_for(&last, || {
            let uri = tab.committed_uri().unwrap_or_default();
            if uri == index_url.as_str() { Ok(()) } else { Err(format!("back to the fixture page: {uri:?}")) }
        })
        .await;
        Ok(format!(
            "typing {typed:?} read {completed:?} with {:?} selected on row 0; Down showed row 1's {:?}; Escape brought the completion back; Enter opened {root}; address-completion.png",
            &completed[typed.len()..],
            down.text
        ))
    })
    .await;

    ctx.check("search_suggestions", CHECK_TIMEOUT, |last| async move {
        let form = EngineForm { name: "Fixture Search".into(), keyword: "fixture".into(), url: format!("{}/search?q=%s", ctx.server.origin()) };
        let engine = browser.core().borrow_mut().search_engines().save(None, &form).map_err(|e| e.to_string())?;
        let _engine = Cleanup(|| {
            let mut profile = browser.core().borrow_mut();
            let mut engines = profile.search_engines();
            let reset = engines.set_default(&SearchEngineId::builtin_default()).and_then(|()| engines.remove(&engine));
            if let Err(e) = reset.and_then(|()| profile.prefs().set(&keys::SEARCH_SUGGESTIONS, &false)) {
                log::warn!("self-test: removing the fixture engine: {e}");
            }
        });
        {
            let mut profile = browser.core().borrow_mut();
            let mut engines = profile.search_engines();
            let suggest_url = format!("{}/suggest?q={{searchTerms}}", ctx.server.origin());
            engines.set_suggest_url(&engine, Some(&suggest_url)).and_then(|()| engines.set_default(&engine)).map_err(|e| e.to_string())?;
            profile.prefs().set(&keys::SEARCH_SUGGESTIONS, &true).map_err(|e| e.to_string())?;
        }
        let address = window.address_bar();
        let none = gdk::ModifierType::empty();
        let asked = || ctx.server.hits().iter().filter(|path| *path == "/suggest").count();
        let shows_suggestions = |fills: &[String], typed: &str| fills.iter().any(|fill| *fill == format!("{typed} one"));
        let wanted = ["vsesvit", "vsesvit one", "vsesvit two"];
        let before = asked();
        address.focus_for_typing();
        address.type_text("vsesvit");
        let shown = wait_for(&last, || {
            let seen = address.observe();
            if seen.open && seen.text == "vsesvit" && seen.highlighted == Some(0) && seen.fills.iter().take(wanted.len()).eq(&wanted) {
                Ok(seen.fills)
            } else {
                Err(format!("after typing \"vsesvit\": {seen:?}"))
            }
        })
        .await;
        if asked() - before != 1 {
            return Err(format!("the engine was asked {} times for {shown:?}", asked() - before));
        }
        glib::timeout_future(POPOVER_SETTLE).await;
        let list = address.suggestions_popover();
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&list), &ctx.out_dir.join("search-suggestions.png"))
            .await
            .map_err(|e| e.to_string())?;
        address.press(gdk::Key::Down, none);
        let down = address.observe();
        if down.text != "vsesvit one" || down.highlighted != Some(1) {
            return Err(format!("after Down: {down:?}"));
        }
        address.press(gdk::Key::Return, none);
        let searched = ctx.server.url("/search?q=vsesvit+one");
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        wait_for(&last, || {
            let uri = tab.committed_uri().unwrap_or_default();
            if uri == searched.as_str() { Ok(()) } else { Err(format!("after Enter the tab shows {uri:?}")) }
        })
        .await;
        address.submit_text(index_url.as_str());
        wait_for(&last, || {
            let uri = tab.committed_uri().unwrap_or_default();
            if uri == index_url.as_str() { Ok(()) } else { Err(format!("back to the fixture page: {uri:?}")) }
        })
        .await;

        let typed_url = format!("127.0.0.1:{}/page2.html", ctx.server.port());
        address.focus_for_typing();
        address.type_text(&typed_url);
        glib::timeout_future(DEBOUNCE + POPOVER_SETTLE).await;
        let url = address.observe();
        browser.core().borrow_mut().prefs().set(&keys::SEARCH_SUGGESTIONS, &false).map_err(|e| e.to_string())?;
        address.focus_for_typing();
        address.type_text("vsesvit off");
        glib::timeout_future(DEBOUNCE + POPOVER_SETTLE).await;
        let off = address.observe();
        address.press(gdk::Key::Escape, none);
        let asked_since = asked() - before - 1;
        if asked_since != 0 || shows_suggestions(&url.fills, &typed_url) || shows_suggestions(&off.fills, "vsesvit off") {
            return Err(format!("asked the engine {asked_since} more times; typing {typed_url:?} listed {:?}; \"vsesvit off\" listed {:?}", url.fills, off.fills));
        }
        Ok(format!(
            "typing \"vsesvit\" asked the engine once and listed {shown:?} (search-suggestions.png); Down read \"vsesvit one\" and Enter opened {searched}; typing {typed_url:?}, and \"vsesvit off\" with the setting off, asked nothing"
        ))
    })
    .await;

    ctx.check("selection_search", CHECK_TIMEOUT, |last| async move {
        let form = EngineForm { name: "Fixture Search".into(), keyword: "fixture".into(), url: format!("{}/search?q=%s", ctx.server.origin()) };
        let engine = browser.core().borrow_mut().search_engines().save(None, &form).map_err(|e| e.to_string())?;
        let _engine = Cleanup(|| {
            let mut profile = browser.core().borrow_mut();
            let mut engines = profile.search_engines();
            let reset = engines.set_default(&SearchEngineId::builtin_default()).and_then(|()| engines.remove(&engine));
            if let Err(e) = reset {
                log::warn!("self-test: removing the fixture engine: {e}");
            }
        });
        browser.core().borrow_mut().search_engines().set_default(&engine).map_err(|e| e.to_string())?;
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let selected = eval_js(tab.web_view(), SELECT_HEADING).await?;
        wait_for(&last, || {
            let tracked = tab.selection();
            if tracked == selected { Ok(()) } else { Err(format!("the page selected {selected:?}; the tab has {tracked:?}")) }
        })
        .await;
        let menu = webkit::ContextMenu::new();
        menu.append(&webkit::ContextMenuItem::from_stock_action(webkit::ContextMenuAction::Copy));
        menu.append(&webkit::ContextMenuItem::from_stock_action(webkit::ContextMenuAction::InspectElement));
        let item = crate::page_menu::add_selection_item(&tab, &menu).ok_or_else(|| "no item for the selection".to_owned())?;
        let label = item.title().map(String::from).unwrap_or_default();
        let position = menu.items().iter().position(|i| i == &item);
        let wanted_label = format!("Search Fixture Search for \u{201c}{selected}\u{201d}");
        if selected != "Vsesvit fixture page" || label != wanted_label || position != Some(1) {
            return Err(format!("selected {selected:?}: the item reads {label:?} at {position:?}"));
        }
        let before = window.tabs().len();
        item.gaction().ok_or_else(|| "the item has no action".to_owned())?.activate(None);
        let wanted = format!("{}/search?q=Vsesvit+fixture+page", ctx.server.origin());
        let opened = wait_for(&last, || {
            let tabs = window.tabs();
            let selected = window.selected_tab().filter(|t| t != &tab).ok_or_else(|| "no new tab selected".to_owned())?;
            let next_to = tabs.iter().position(|t| t == &tab).map(|i| i + 1) == tabs.iter().position(|t| t == &selected);
            let uri = selected.web_view().uri().map(String::from).unwrap_or_default();
            if tabs.len() == before + 1 && next_to && uri == wanted {
                Ok(selected)
            } else {
                Err(format!("tabs={} next to the page={next_to} requested {uri:?}", tabs.len()))
            }
        })
        .await;
        window.close_tab(&opened);
        window.select_tab(&tab);
        Ok(format!("{label:?} follows Copy and opened {wanted} in a new tab next to the page"))
    })
    .await;

    ctx.check("session", CHECK_TIMEOUT, |_| async move {
        browser.save_session_now();
        let restored = browser.core().borrow_mut().session().restore().map_err(|e| e.to_string())?;
        match restored {
            Some(snapshot) if snapshot.windows.iter().any(|w| !w.tabs.is_empty()) => Ok(format!(
                "windows={} tabs={} first={}",
                snapshot.windows.len(),
                snapshot.windows.iter().map(|w| w.tabs.len()).sum::<usize>(),
                snapshot.windows[0].tabs[0].url
            )),
            other => Err(format!("restore returned {other:?}")),
        }
    })
    .await;

    ctx.check("download", CHECK_TIMEOUT, |last| async move {
        let dir = ctx.out_dir.join("downloads");
        let _ = std::fs::remove_dir_all(&dir);
        browser.core().borrow_mut().prefs().set(&keys::DOWNLOADS_DIR, &Some(dir.clone())).map_err(|e| e.to_string())?;
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        tab.load(ctx.server.url("/download.bin").as_str());
        let expected = dir.join("download.bin");
        let entry = wait_for(&last, || {
            let entry = browser.downloads().list().into_iter().find(|d| d.path == expected);
            match entry {
                Some(entry) if entry.state == State::Completed => Ok(entry),
                Some(entry) => Err(format!("the list entry is {:?}", entry.state)),
                None => Err(format!("no list entry for {}", expected.display())),
            }
        })
        .await;
        let bytes = std::fs::read(&expected).map_err(|e| format!("{}: {e}", expected.display()))?;
        if bytes != DOWNLOAD_FIXTURE {
            return Err(format!("{} has {} bytes, not the {} served", expected.display(), bytes.len(), DOWNLOAD_FIXTURE.len()));
        }
        if !window.shows_downloads_button() {
            return Err("the header shows no downloads button".to_owned());
        }
        gio::prelude::ActionGroupExt::activate_action(window, "show-downloads", None);
        let downloads = browser.windowed(Windowed::Downloads).ok_or_else(|| "win.show-downloads opened no window".to_owned())?;
        glib::timeout_future(Duration::from_millis(500)).await;
        let shot = crate::screenshot::save_png(&downloads, &ctx.out_dir.join("downloads.png")).await;
        downloads.close();
        shot.map_err(|e| e.to_string())?;
        Ok(format!(
            "{} matches the {} bytes served; list entry Completed, reading {:?}; the header shows the downloads button; downloads.png written",
            expected.display(),
            bytes.len(),
            status_line(&entry, None, true)
        ))
    })
    .await;

    ctx.check("new_tab_page", CHECK_TIMEOUT, |last| async move {
        let first = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        window.new_tab();
        let tab = window.selected_tab().filter(|t| *t != first).ok_or_else(|| "no new tab selected".to_owned())?;
        let origin = ctx.server.url("/");
        let tiles = loop {
            let loaded = !tab.web_view().is_loading() && tab.committed_uri().as_deref() == Some("about:blank");
            let tiles = if loaded { eval_js(tab.web_view(), NEW_TAB_PAGE_PROBE).await? } else { String::new() };
            if tiles.contains(origin.as_str()) {
                break tiles;
            }
            last.set(format!("uri={:?} page reports {tiles:?}", tab.committed_uri()));
            glib::timeout_future(POLL).await;
        };
        let title = tab.display_title();
        if !tab.is_blank() || title != "New Tab" {
            return Err(format!("is_blank={} title={title:?} tiles {tiles}", tab.is_blank()));
        }
        glib::timeout_future(Duration::from_millis(500)).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("new-tab.png")).await.map_err(|e| e.to_string())?;
        window.select_tab(&first);
        window.close_tab(&tab);
        Ok(format!("tiles {tiles}; the tab reads as blank, titled {title:?}"))
    })
    .await;

    // The page waits on an image from a listener that never answers, until the listener closes.
    ctx.check("address_progress", CHECK_TIMEOUT, |last| async move {
        let stall = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = stall.local_addr().map_err(|e| e.to_string())?.port();
        let page = format!("data:text/html,<title>Slow page</title><h1>Slow page</h1><img src='http://127.0.0.1:{port}/stall.png'>");
        let first = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let tab = window.open_tab(Some(&page), None, Focus::Foreground);
        let bar = window.address_bar();
        wait_for(&last, || match tab.display_title() {
            title if title == "Slow page" => Ok(()),
            title => Err(format!("the slow page is titled {title:?}")),
        })
        .await;
        glib::timeout_future(Duration::from_millis(1000)).await;
        let during = (tab.web_view().is_loading(), bar.progress());
        let shot = crate::screenshot::save_png(window, &ctx.out_dir.join("address-progress.png")).await;
        drop(stall);
        wait_for(&last, || match (tab.web_view().is_loading(), bar.progress()) {
            (false, 0.0) => Ok(()),
            now => Err(format!("after the image failed, (loading, line) = {now:?}")),
        })
        .await;
        window.close_tab(&tab);
        window.select_tab(&first);
        shot.map_err(|e| e.to_string())?;
        if !(during.0 && during.1 > 0.0 && during.1 < 1.0) {
            return Err(format!("while the page loaded, (loading, line) = {during:?}"));
        }
        Ok(format!("while the page loaded the line was at {:.2} (address-progress.png); it went once the load ended", during.1))
    })
    .await;

    ctx.check("settings", CHECK_TIMEOUT, |last| async move {
        let settings = browser.engine().settings();
        let engine = (
            settings.is_javascript_can_open_windows_automatically(),
            settings.enables_smooth_scrolling(),
            settings.hardware_acceleration_policy(),
        );
        if engine != (false, true, webkit::HardwareAccelerationPolicy::Always) {
            return Err(format!("a fresh profile's engine has (pop-ups, smooth, GPU) = {engine:?}"));
        }
        browser.core().borrow_mut().prefs().set(&keys::HOMEPAGE, &page2_url.to_string()).map_err(|e| e.to_string())?;
        browser.set_home_button_visible(true);
        let home = window.home_button();
        wait_for(&last, || if home.is_mapped() { Ok(()) } else { Err("the home button is not shown".to_owned()) }).await;
        glib::timeout_future(Duration::from_millis(300)).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("home-button.png")).await.map_err(|e| e.to_string())?;
        home.emit_clicked();
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        wait_for(&last, || match tab.committed_uri() {
            Some(uri) if uri == page2_url.as_str() => Ok(()),
            uri => Err(format!("after Home the tab is at {uri:?}")),
        })
        .await;

        gio::prelude::ActionGroupExt::activate_action(window, "show-settings", None);
        let dialog = window
            .visible_dialog()
            .and_downcast::<adw::PreferencesDialog>()
            .ok_or_else(|| "win.show-settings opened no preferences dialog".to_owned())?;
        let mut shots = Vec::new();
        for name in ["general", "sync", "appearance", "search", "privacy", "shortcuts"] {
            dialog.set_visible_page_name(name);
            if dialog.visible_page_name().as_deref() != Some(name) {
                dialog.close();
                return Err(format!("Settings has no {name:?} page"));
            }
            glib::timeout_future(Duration::from_millis(500)).await;
            if name == "privacy" && find::<adw::ActionRow>(dialog.upcast_ref(), |r| r.title() == PASSWORDS_NOTICE && r.is_mapped()).is_none() {
                dialog.close();
                return Err(format!("the Privacy page shows no {PASSWORDS_NOTICE:?} row"));
            }
            let file = format!("settings-{name}.png");
            let shot = crate::screenshot::save_png(window, &ctx.out_dir.join(&file)).await;
            if let Err(e) = shot {
                dialog.close();
                return Err(format!("{file}: {e}"));
            }
            shots.push(file);
        }
        dialog.close();
        browser.set_home_button_visible(false);
        browser.core().borrow_mut().prefs().reset(&keys::HOMEPAGE).map_err(|e| e.to_string())?;
        Ok(format!(
            "engine (pop-ups, smooth, GPU) = {engine:?}; Home opened {page2_url}; Privacy shows {PASSWORDS_NOTICE:?}; home-button.png and {} written",
            shots.join(", ")
        ))
    })
    .await;

    ctx.check("search_engines", CHECK_TIMEOUT, |last| async move {
        let engine_named = |name: &str| browser.core().borrow_mut().search_engines().list().ok().and_then(|list| list.into_iter().find(|e| e.name == name));
        let _engine = Cleanup(|| {
            if let Some(engine) = engine_named("Fixture Engine")
                && let Err(e) = browser.core().borrow_mut().search_engines().remove(&engine.id)
            {
                log::warn!("self-test: removing the fixture engine: {e}");
            }
        });
        let (settings, add) = open_search_engines(window, &last).await?;
        let settings_open = Cleanup(|| {
            settings.close();
        });
        let row_titled = |title: &str| find::<adw::ActionRow>(settings.upcast_ref(), |r| r.title() == title && r.is_mapped());
        let default_row = row_titled("DuckDuckGo (Default)").ok_or_else(|| "the list has no \"DuckDuckGo (Default)\"".to_owned())?;
        if WidgetExt::activate_action(&default_row, "engine.remove", None).is_ok() {
            return Err("the default engine's menu offers Remove".to_owned());
        }

        add.emit_clicked();
        let editor = wait_for(&last, || window.visible_dialog().and_downcast::<adw::AlertDialog>().ok_or_else(|| "Add opened no editor".to_owned())).await;
        let fill = |title: &str, text: &str| {
            find::<adw::EntryRow>(editor.upcast_ref(), |r| r.title() == title).map(|row| row.set_text(text)).ok_or_else(|| format!("the editor has no {title:?} box"))
        };
        fill("Name", "Fixture Engine")?;
        fill("Shortcut", "w")?;
        let taken = find::<gtk::Label>(editor.upcast_ref(), |l| l.has_css_class("error") && l.is_visible()).map(|l| l.label().to_string());
        let refused = !editor.is_response_enabled("save");
        fill("Shortcut", "fx")?;
        fill("URL with %s in Place of Query", &format!("{}/search?q=%s", ctx.server.origin()))?;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("search-engine-editor.png")).await.map_err(|e| e.to_string())?;
        if taken.as_deref() != Some("Another search engine has this shortcut") || !refused || !editor.is_response_enabled("save") {
            editor.close();
            return Err(format!("the shortcut w said {taken:?} with Add enabled={}; filled in, Add enabled={}", !refused, editor.is_response_enabled("save")));
        }
        button_labelled(editor.upcast_ref(), "_Add").ok_or_else(|| "the editor has no Add button".to_owned())?.emit_clicked();
        let added = wait_for(&last, || engine_named("Fixture Engine").ok_or_else(|| "core has no Fixture Engine".to_owned())).await;
        wait_for(&last, || row_titled("Fixture Engine").ok_or_else(|| "the list shows no Fixture Engine".to_owned())).await;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("settings-search-engines.png")).await.map_err(|e| e.to_string())?;
        drop(settings_open);

        let address = window.address_bar();
        address.focus_for_typing();
        address.type_text("fx vsesvit fixture");
        address.press(gdk::Key::Return, gdk::ModifierType::empty());
        let searched = ctx.server.url("/search?q=vsesvit+fixture");
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        wait_for(&last, || {
            let uri = tab.committed_uri().unwrap_or_default();
            if uri == searched.as_str() { Ok(()) } else { Err(format!("\"fx vsesvit fixture\" opened {uri:?}")) }
        })
        .await;

        let (settings, _) = open_search_engines(window, &last).await?;
        let _settings_open = Cleanup(|| {
            settings.close();
        });
        let fixture_row = find::<adw::ActionRow>(settings.upcast_ref(), |r| r.title() == "Fixture Engine").ok_or_else(|| "Settings opened again lists no Fixture Engine".to_owned())?;
        WidgetExt::activate_action(&fixture_row, "engine.remove", None).map_err(|e| e.to_string())?;
        wait_for(&last, || {
            let listed = find::<adw::ActionRow>(settings.upcast_ref(), |r| r.title() == "Fixture Engine").is_some();
            if engine_named("Fixture Engine").is_none() && !listed { Ok(()) } else { Err(format!("after Remove, core has it={}, the list shows it={listed}", engine_named("Fixture Engine").is_some())) }
        })
        .await;
        address.submit_text(index_url.as_str());
        wait_for(&last, || {
            let uri = tab.committed_uri().unwrap_or_default();
            if uri == index_url.as_str() { Ok(()) } else { Err(format!("back to the fixture page: {uri:?}")) }
        })
        .await;
        Ok(format!(
            "the default's menu has no Remove; the shortcut w was refused ({taken:?}); Add saved {} with {}; \"fx vsesvit fixture\" opened {searched}; Remove in Settings opened again deleted it; search-engine-editor.png, settings-search-engines.png",
            added.name, added.search_url.0
        ))
    })
    .await;

    ctx.check("bookmarks_bar_menus", CHECK_TIMEOUT, |last| async move {
        let folder = {
            let mut profile = browser.core().borrow_mut();
            let icon = gdk::MemoryTexture::new(16, 16, gdk::MemoryFormat::R8g8b8a8, &glib::Bytes::from_owned([220u8, 60, 40, 255].repeat(256)), 64);
            profile.favicons().record(index_url, &icon.save_to_png_bytes()).map_err(|e| e.to_string())?;
            let mut bookmarks = profile.bookmarks();
            let folder = bookmarks.add_folder(BookmarkId::TOOLBAR, InsertAt::Index(0), "Fixture folder").map_err(|e| e.to_string())?;
            bookmarks.add_url(folder, InsertAt::End, "Fixture index", index_url).map_err(|e| e.to_string())?;
            bookmarks.add_url(folder, InsertAt::End, "A page whose title is far too long to fit in any menu without being cut short", page2_url).map_err(|e| e.to_string())?;
            let inner = bookmarks.add_folder(folder, InsertAt::End, "Inner folder").map_err(|e| e.to_string())?;
            bookmarks.add_url(inner, InsertAt::End, "Inner page", page2_url).map_err(|e| e.to_string())?;
            for i in 0..16 {
                let url = vsesvit_core::Url::parse(&format!("https://site{i}.example/")).map_err(|e| e.to_string())?;
                bookmarks.add_url(BookmarkId::TOOLBAR, InsertAt::End, &format!("Example site {i}"), &url).map_err(|e| e.to_string())?;
            }
            folder
        };
        browser.bookmarks_changed();
        window.set_default_size(900, 700);
        let bar = window.bookmarks_bar();
        let (shown, hidden) = wait_for(&last, || match bar.overflow() {
            (shown, hidden) if hidden > 0 && bar.chevron().is_mapped() => Ok((shown, hidden)),
            (shown, hidden) => Err(format!("window {} px wide, {shown} items shown, {hidden} in the chevron menu", window.width())),
        })
        .await;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("bookmarks-overflow.png")).await.map_err(|e| e.to_string())?;

        bar.chevron().emit_clicked();
        let overflow = wait_for(&last, || popover_of(bar.chevron().upcast_ref()).ok_or_else(|| "the chevron opened no menu".to_owned())).await;
        let overflow_rows = count::<gtk::Button>(overflow.upcast_ref(), |b| b.has_css_class("bookmark-menu-row"));
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&overflow), &ctx.out_dir.join("bookmarks-overflow-menu.png"))
            .await
            .map_err(|e| e.to_string())?;
        overflow.popdown();

        let (item, folder_button) = bar.item(0).ok_or_else(|| "the bar has no first item".to_owned())?;
        if item.node.id != folder {
            return Err(format!("the first item is {:?}", item.node.title));
        }
        folder_button.downcast_ref::<gtk::Button>().ok_or_else(|| "the folder is not a button".to_owned())?.emit_clicked();
        let menu = wait_for(&last, || popover_of(&folder_button).ok_or_else(|| "the folder opened no menu".to_owned())).await;
        let favicons = count::<gtk::Image>(menu.upcast_ref(), |i| i.storage_type() == gtk::ImageType::Paintable);
        let inner = button_labelled_in_row(menu.upcast_ref(), "Inner folder").ok_or_else(|| "no Inner folder row".to_owned())?;
        inner.emit_clicked();
        let submenu = wait_for(&last, || popover_of(inner.upcast_ref()).ok_or_else(|| "the inner folder opened no menu".to_owned())).await;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), &[menu.clone(), submenu.clone()], &ctx.out_dir.join("bookmarks-folder-menu.png"))
            .await
            .map_err(|e| e.to_string())?;
        let long = button_labelled_in_row(menu.upcast_ref(), "A page whose title is far too long to fit in any menu without being cut short")
            .and_then(|row| find::<gtk::Label>(row.upcast_ref(), |_| true))
            .is_some_and(|label| label.layout().is_ellipsized());
        menu.popdown();

        let (link, link_button) = bar.item(1).ok_or_else(|| "the bar has no second item".to_owned())?;
        let context = crate::bookmark_menu::open_context_menu(&link_button, &crate::bookmark_menu::Target::Node(link.clone()), None);
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), &[context.clone().upcast()], &ctx.out_dir.join("bookmarks-context-menu.png"))
            .await
            .map_err(|e| e.to_string())?;
        WidgetExt::activate_action(&context, "bookmark.edit", None).map_err(|e| e.to_string())?;
        let dialog = wait_for(&last, || window.visible_dialog().ok_or_else(|| "Edit… opened no dialog".to_owned())).await;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("bookmarks-edit-dialog.png")).await.map_err(|e| e.to_string())?;
        let url_field = find::<gtk::Entry>(dialog.upcast_ref(), |e| e.text().starts_with("https://") || e.text().starts_with("http://")).map(|e| e.text().to_string());
        dialog.close();

        {
            let mut profile = browser.core().borrow_mut();
            let mut bookmarks = profile.bookmarks();
            let added: Vec<BookmarkId> = bookmarks.children(BookmarkId::TOOLBAR).into_iter().filter(|n| n.id == folder || n.title.starts_with("Example site")).map(|n| n.id).collect();
            for id in added {
                bookmarks.remove(id).map_err(|e| e.to_string())?;
            }
        }
        browser.bookmarks_changed();
        window.set_default_size(1280, 820);
        let detail = format!(
            "at {} px {shown} items shown and {hidden} in the » menu ({overflow_rows} rows); folder menu has {favicons} favicons, a submenu, long title ellipsized={long}; Edit… showed URL {url_field:?}; screenshots bookmarks-*.png",
            window.width()
        );
        if overflow_rows == hidden && favicons > 0 && long && url_field.as_deref() == link.node.url.as_ref().map(|u| u.as_str()) { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("ctrl_s_toggles_sidebar", CHECK_TIMEOUT, |last| async move {
        let bound: Vec<String> = browser.app().actions_for_accel("<Control>s").iter().map(|a| a.to_string()).collect();
        if bound != ["win.toggle-tab-sidebar"] {
            return Err(format!("Ctrl+S runs {bound:?}"));
        }
        let shown = || window.layout_probe().sidebar.is_some();
        if !shown() {
            return Err("the sidebar is hidden before Ctrl+S".to_owned());
        }
        gio::prelude::ActionGroupExt::activate_action(window, "toggle-tab-sidebar", None);
        wait_for(&last, || if shown() { Err("the sidebar is still shown".to_owned()) } else { Ok(()) }).await;
        glib::timeout_future(Duration::from_millis(500)).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("sidebar-hidden.png")).await.map_err(|e| e.to_string())?;
        gio::prelude::ActionGroupExt::activate_action(window, "toggle-tab-sidebar", None);
        wait_for(&last, || if shown() { Ok(()) } else { Err("the sidebar did not come back".to_owned()) }).await;
        Ok("Ctrl+S is bound to win.toggle-tab-sidebar only; it hid the sidebar (sidebar-hidden.png) and showed it again".to_owned())
    })
    .await;

    ctx.check("shortcuts", CHECK_TIMEOUT, |_| async move {
        let _defaults = Cleanup(|| browser.edit_keymap(Keymap::reset_all));
        let app = browser.app();
        // GTK hands accelerators back in its own spelling, so they are compared parsed.
        let accels = |action: &str| -> Vec<String> { app.accels_for_action(action).iter().map(|a| a.to_string()).collect() };
        let has = |action: &str, want: &[&str]| {
            let parse = |accels: &[&str]| accels.iter().map(|a| gtk::accelerator_parse(*a)).collect::<Vec<_>>();
            let set = accels(action);
            parse(&set.iter().map(String::as_str).collect::<Vec<_>>()) == parse(want)
        };
        let runs = |accel: &str| -> Vec<String> { app.actions_for_accel(accel).iter().map(|a| a.to_string()).collect() };
        let chord = |text: &str| text.parse::<Chord>().map_err(|e| e.to_string());
        let (ctrl_shift_y, ctrl_j) = (chord("Ctrl+Shift+Y")?, chord("Ctrl+J")?);

        browser.edit_keymap(|keymap| keymap.assign(Command::ShowHistory, [ctrl_shift_y]));
        let (history, ctrl_h) = (accels("win.show-history"), runs("<Control>h"));
        if !has("win.show-history", &["<Control><Shift>y"]) || !ctrl_h.is_empty() {
            return Err(format!("after Ctrl+Shift+Y, win.show-history has {history:?} and Ctrl+H runs {ctrl_h:?}"));
        }
        let taken = browser.edit_keymap(|keymap| keymap.assign(Command::ShowHistory, [ctrl_j]));
        let taken_from: Vec<&str> = taken.iter().map(|t| t.from.title()).collect();
        let (downloads, ctrl_j_runs) = (accels("win.show-downloads"), runs("<Control>j"));
        if taken_from != ["Downloads"] || !downloads.is_empty() || ctrl_j_runs != ["win.show-history"] {
            return Err(format!("after Ctrl+J, taken from {taken_from:?}; win.show-downloads has {downloads:?}; Ctrl+J runs {ctrl_j_runs:?}"));
        }

        gio::prelude::ActionGroupExt::activate_action(window, "show-settings", None);
        let dialog = window
            .visible_dialog()
            .and_downcast::<adw::PreferencesDialog>()
            .ok_or_else(|| "win.show-settings opened no preferences dialog".to_owned())?;
        let shown = async {
            dialog.set_visible_page_name("shortcuts");
            if dialog.visible_page_name().as_deref() != Some("shortcuts") {
                return Err("Settings has no \"shortcuts\" page".to_owned());
            }
            // Only the Shortcuts page: the Sync page has a "History" row of its own.
            let page = dialog.visible_page().ok_or_else(|| "the shortcuts page is not shown".to_owned())?;
            let row_shows = |title: &str| {
                find::<adw::ActionRow>(page.upcast_ref(), |row| row.title() == title)
                    .and_then(|row| find::<adw::ShortcutLabel>(row.upcast_ref(), |_| true))
                    .map(|label| label.accelerator().to_string())
            };
            let rows = (row_shows("History"), row_shows("Downloads"));
            if rows != (Some("<Control>j".to_owned()), Some(String::new())) {
                return Err(format!("the History and Downloads rows show {rows:?}"));
            }
            glib::timeout_future(Duration::from_millis(300)).await;
            let rows = find::<adw::ActionRow>(page.upcast_ref(), |row| row.title() == "History");
            if let Some(scrolled) = rows.and_then(|row| row.ancestor(gtk::ScrolledWindow::static_type())).and_downcast::<gtk::ScrolledWindow>() {
                let at = scrolled.vadjustment();
                at.set_value(at.upper() - at.page_size());
            }
            glib::timeout_future(Duration::from_millis(300)).await;
            crate::screenshot::save_png(window, &ctx.out_dir.join("shortcuts-edited.png")).await.map_err(|e| e.to_string())?;
            let capture = shortcut_settings::capture(&dialog, browser, shortcut_settings::Target::Browser(Command::ShowHistory), || {});
            capture.press(keymap::pressed(gdk::Key::t, gdk::ModifierType::CONTROL_MASK, gdk::ModifierType::empty(), Some(gdk::Key::t)));
            let note = capture.note();
            glib::timeout_future(POPOVER_SETTLE).await;
            let shot = crate::screenshot::save_png(window, &ctx.out_dir.join("shortcut-capture.png")).await;
            capture.close();
            shot.map_err(|e| e.to_string())?;
            if note != "Also used by New tab. Saving moves it here." {
                return Err(format!("capturing Ctrl+T for History says {note:?}"));
            }
            Ok(note)
        }
        .await;
        dialog.close();
        let note = shown?;

        browser.edit_keymap(Keymap::reset_all);
        let drifted: Vec<&str> = keymap::actions()
            .filter(|(cmd, action)| {
                let defaults: Vec<String> = cmd.defaults().iter().map(|&chord| keymap::accelerator(chord)).collect();
                !has(action, &defaults.iter().map(String::as_str).collect::<Vec<_>>())
            })
            .map(|(_, action)| action)
            .collect();
        let (save, ctrl_s) = (accels("win.save-page"), runs("<Control>s"));
        if !drifted.is_empty() || !has("win.save-page", &["<Control><Shift>s"]) || ctrl_s != ["win.toggle-tab-sidebar"] {
            return Err(format!("after Reset All, {drifted:?} are off their defaults; win.save-page has {save:?}; Ctrl+S runs {ctrl_s:?}"));
        }
        Ok(format!(
            "Ctrl+Shift+Y moved History off Ctrl+H; Ctrl+J moved to History from {taken_from:?}, leaving win.show-downloads none; Settings rows show it (shortcuts-edited.png); capturing Ctrl+T said {note:?} (shortcut-capture.png); Reset All restored every default, Ctrl+Shift+S on win.save-page, Ctrl+S on win.toggle-tab-sidebar"
        ))
    })
    .await;

    ctx.check("save_page", CHECK_TIMEOUT, |last| async move {
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        tab.load(index_url.as_str());
        wait_for(&last, || {
            let (title, uri, loading) = (title_of(tab.web_view()), tab.committed_uri().unwrap_or_default(), tab.web_view().is_loading());
            if title == "Vsesvit fixture" && uri == index_url.as_str() && !loading { Ok(()) } else { Err(format!("title={title:?} committed={uri:?} loading={loading}")) }
        })
        .await;
        let path = ctx.out_dir.join("saved-page.mhtml");
        let _ = std::fs::remove_file(&path);
        crate::save_page::save(browser, tab.web_view(), &path).await?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let detail = format!("{} has {} bytes", path.display(), bytes.len());
        if String::from_utf8_lossy(&bytes).contains("Vsesvit fixture") { Ok(format!("{detail}, containing \"Vsesvit fixture\"")) } else { Err(format!("{detail}, none of them \"Vsesvit fixture\"")) }
    })
    .await;

    ctx.check("page_commands", CHECK_TIMEOUT, |last| async move {
        let app = browser.app();
        let parse = |accels: &[&str]| accels.iter().map(|a| gtk::accelerator_parse(*a)).collect::<Vec<_>>();
        for (action, want) in [
            ("win.print", &["<Control>p"][..]),
            ("win.view-source", &["<Control>u"]),
            ("win.developer-tools", &["<Control><Shift>i", "F12"]),
            ("win.javascript-console", &["<Control><Shift>j"]),
        ] {
            let held: Vec<String> = app.accels_for_action(action).iter().map(|a| a.to_string()).collect();
            if parse(&held.iter().map(String::as_str).collect::<Vec<_>>()) != parse(want) {
                return Err(format!("{action} has {held:?}, not {want:?}"));
            }
        }

        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let _tabs = Cleanup(|| {
            for other in window.tabs().into_iter().filter(|t| t != &tab) {
                window.close_tab(&other);
            }
            window.select_tab(&tab);
            if tab.inspector_open() {
                gio::prelude::ActionGroupExt::activate_action(window, "developer-tools", None);
            }
        });
        let source_url = format!("view-source:{index_url}");
        let fetches = || ctx.server.hits().iter().filter(|path| *path == "/index.html").count();
        let shows_source = |source: &crate::tab::Tab| {
            let (uri, title, loading) = (source.committed_uri().unwrap_or_default(), title_of(source.web_view()), source.web_view().is_loading());
            if uri == source_url && title == source_url && !loading { Ok(()) } else { Err(format!("committed={uri:?} title={title:?} loading={loading}")) }
        };
        let source_text = async |source: &crate::tab::Tab| {
            let text = eval_js(source.web_view(), "document.body.innerText").await?;
            if text.contains("<title>Vsesvit fixture</title>") { Ok(()) } else { Err(format!("{source_url} reads {text:?}")) }
        };

        let before = fetches();
        gio::prelude::ActionGroupExt::activate_action(window, "view-source", None);
        let source = wait_for(&last, || {
            let tabs = window.tabs();
            let source = window.selected_tab().filter(|t| t != &tab).ok_or_else(|| "no new tab selected".to_owned())?;
            let next_to = tabs.iter().position(|t| t == &tab).map(|i| i + 1) == tabs.iter().position(|t| t == &source);
            shows_source(&source).and_then(|()| if next_to { Ok(source) } else { Err("the source tab is not next to the page".to_owned()) })
        })
        .await;
        source_text(&source).await?;
        let from_tab = fetches() - before;
        window.close_tab(&source);

        tab.load(page2_url.as_str());
        wait_for(&last, || {
            let uri = tab.committed_uri().unwrap_or_default();
            if uri == page2_url.as_str() && !tab.web_view().is_loading() { Ok(()) } else { Err(format!("the page tab shows {uri:?}")) }
        })
        .await;
        let before = fetches();
        let fresh = window.open_tab(Some(&source_url), None, Focus::Foreground);
        wait_for(&last, || shows_source(&fresh)).await;
        source_text(&fresh).await?;
        let from_hidden = fetches() - before;
        window.close_tab(&fresh);
        if from_tab != 0 || from_hidden != 1 {
            return Err(format!("the server sent the page {from_tab} time(s) for the open tab's source and {from_hidden} for the hidden view's"));
        }
        tab.load(index_url.as_str());
        wait_for(&last, || {
            let uri = tab.committed_uri().unwrap_or_default();
            if uri == index_url.as_str() && !tab.web_view().is_loading() { Ok(()) } else { Err(format!("back on the page: {uri:?}")) }
        })
        .await;

        let menu = webkit::ContextMenu::new();
        menu.append(&webkit::ContextMenuItem::from_stock_action(webkit::ContextMenuAction::Reload));
        menu.append(&webkit::ContextMenuItem::from_stock_action(webkit::ContextMenuAction::InspectElement));
        crate::page_menu::add_page_items(&tab, &menu);
        let listed: Vec<String> = menu
            .items()
            .iter()
            .map(|item| match item.stock_action() {
                webkit::ContextMenuAction::Custom => item.title().map(String::from).unwrap_or_default(),
                stock => format!("{stock:?}"),
            })
            .collect();
        if listed != ["Reload", "_Print…", "View Page _Source", "InspectElement"] {
            return Err(format!("the page's context menu lists {listed:?}"));
        }

        let inspector = tab.web_view().inspector().ok_or_else(|| "the page has no inspector".to_owned())?;
        let placed = Rc::new(Cell::new(""));
        inspector.connect_attach(glib::clone!(
            #[strong]
            placed,
            move |_| {
                placed.set("attached to the page");
                false
            }
        ));
        inspector.connect_open_window(glib::clone!(
            #[strong]
            placed,
            move |_| {
                placed.set("in a window");
                false
            }
        ));
        gio::prelude::ActionGroupExt::activate_action(window, "developer-tools", None);
        wait_for(&last, || match (tab.inspector_open(), placed.get()) {
            (true, placed) if !placed.is_empty() => Ok(()),
            (open, placed) => Err(format!("after the first win.developer-tools the inspector is open={open}, placed {placed:?}")),
        })
        .await;
        let inspector_placed = placed.get();
        gio::prelude::ActionGroupExt::activate_action(window, "developer-tools", None);
        wait_for(&last, || if tab.inspector_open() { Err("the second win.developer-tools left the inspector open".to_owned()) } else { Ok(()) }).await;

        let pdf = ctx.out_dir.join("print.pdf");
        let _ = std::fs::remove_file(&pdf);
        let settings = gtk::PrintSettings::new();
        settings.set_printer("Print to File");
        settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
        settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(gio::File::for_path(&pdf).uri().as_str()));
        let operation = webkit::PrintOperation::new(tab.web_view());
        operation.set_print_settings(&settings);
        let outcome: Rc<RefCell<Option<Result<(), String>>>> = Rc::default();
        operation.connect_failed(glib::clone!(
            #[strong]
            outcome,
            move |_, error| *outcome.borrow_mut() = Some(Err(error.to_string()))
        ));
        operation.connect_finished(glib::clone!(
            #[strong]
            outcome,
            move |_| {
                outcome.borrow_mut().get_or_insert(Ok(()));
            }
        ));
        operation.print();
        wait_for(&last, || outcome.borrow().clone().ok_or_else(|| "printing has not finished".to_owned())).await?;
        let bytes = std::fs::read(&pdf).map_err(|e| format!("{}: {e}", pdf.display()))?;
        if !bytes.starts_with(b"%PDF") {
            return Err(format!("{} has {} bytes starting {:?}", pdf.display(), bytes.len(), String::from_utf8_lossy(&bytes[..bytes.len().min(8)])));
        }
        Ok(format!(
            "Ctrl+P, Ctrl+U, Ctrl+Shift+I and F12, Ctrl+Shift+J on their actions; win.view-source opened {source_url} next to the page from the open tab ({from_tab} fetches), and a fresh tab showed it through a hidden view ({from_hidden} fetch); the context menu lists {listed:?}; win.developer-tools opened the inspector ({inspector_placed}) and closed it; printing to file wrote {} ({} bytes)",
            pdf.display(),
            bytes.len()
        ))
    })
    .await;

    ctx.check("zoom_indicator", CHECK_TIMEOUT, |last| async move {
        let address = window.address_bar();
        let at_100 = address.shown_zoom();
        gio::prelude::ActionGroupExt::activate_action(window, "zoom-in", None);
        wait_for(&last, || match address.shown_zoom() {
            Some(level) if level == "110%" => Ok(()),
            other => Err(format!("after zoom-in the address bar shows {other:?}")),
        })
        .await;
        address.click_zoom();
        let bubble = address.bubble().ok_or_else(|| "clicking the zoom level opened no bubble".to_owned())?;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&bubble), &ctx.out_dir.join("zoom.png"))
            .await
            .map_err(|e| format!("{e}; bubble mapped={} realized={} surface={:?}", bubble.is_mapped(), bubble.is_realized(), bubble.surface()))?;
        gio::prelude::ActionGroupExt::activate_action(window, "zoom-reset", None);
        wait_for(&last, || match address.shown_zoom() {
            None => Ok(()),
            Some(level) => Err(format!("after zoom-reset the address bar still shows {level}")),
        })
        .await;
        bubble.popdown();
        match at_100 {
            None => Ok("hidden at 100%, 110% after zoom-in with its bubble open (zoom.png), hidden after reset".to_owned()),
            Some(level) => Err(format!("the address bar showed {level} at 100%")),
        }
    })
    .await;

    ctx.check("zoom_is_remembered_per_site", CHECK_TIMEOUT, |last| async move {
        let load = |url: &vsesvit_core::Url| window.address_bar().submit_text(url.as_str());
        let here = ctx.server.url("/index.html");
        let mut elsewhere = here.clone();
        elsewhere.set_host(Some("localhost")).map_err(|e| e.to_string())?;
        let shown = || window.selected_tab().map(|tab| tab.web_view().zoom_level());
        let at = |url: &vsesvit_core::Url, level: f64| {
            let committed = window.selected_tab().and_then(|tab| tab.committed_uri()).unwrap_or_default();
            if committed == url.as_str() && shown() == Some(level) {
                Ok(())
            } else {
                Err(format!("{committed} at {:?}, wanted {url} at {level}", shown()))
            }
        };

        load(&here);
        wait_for(&last, || at(&here, 1.0)).await;
        gio::prelude::ActionGroupExt::activate_action(window, "zoom-in", None);
        gio::prelude::ActionGroupExt::activate_action(window, "zoom-in", None);
        wait_for(&last, || at(&here, 1.25)).await;

        let other_page = ctx.server.url("/page2.html");
        load(&other_page);
        wait_for(&last, || at(&other_page, 1.25)).await;
        load(&elsewhere);
        wait_for(&last, || at(&elsewhere, 1.0)).await;
        load(&here);
        wait_for(&last, || at(&here, 1.25)).await;

        let stored = browser.core().borrow_mut().site_zoom().get(&here).map_err(|e| e.to_string())?;
        gio::prelude::ActionGroupExt::activate_action(window, "zoom-reset", None);
        wait_for(&last, || at(&here, 1.0)).await;
        let forgotten = browser.core().borrow_mut().site_zoom().get(&here).map_err(|e| e.to_string())?;
        if stored == 1.25 && forgotten == 1.0 {
            Ok("125% followed the site to another page and came back to it, another host stayed at 100%; reset forgot it".to_owned())
        } else {
            Err(format!("stored {stored} after zooming, {forgotten} after reset"))
        }
    })
    .await;

    ctx.check("connection_info", if ctx.network { NETWORK_TIMEOUT } else { CHECK_TIMEOUT }, |last| async move {
        let address = window.address_bar();
        let open = |file: &'static str| async move {
            address.click_security();
            let bubble = address.bubble().ok_or_else(|| "the security icon opened no popover".to_owned())?;
            glib::timeout_future(POPOVER_SETTLE).await;
            crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&bubble), &ctx.out_dir.join(file))
                .await
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(bubble)
        };
        let http = open("connection-http.png").await?;
        let http_heading = heading_of(&http);
        let http_certificate = find::<gtk::Label>(http.upcast_ref(), |l| l.label() == "Certificate").is_some();
        http.popdown();
        if http_heading.as_deref() != Some("Connection is not secure") || http_certificate {
            return Err(format!("on HTTP the popover says {http_heading:?}, certificate section shown={http_certificate}"));
        }
        if !ctx.network {
            return Ok("HTTP: \"Connection is not secure\", no certificate (connection-http.png); HTTPS needs --network".to_owned());
        }
        let first = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let tab = window.open_tab(Some("https://example.com/"), None, Focus::Foreground);
        wait_for(&last, || match tab.committed_uri() {
            Some(uri) if uri.starts_with("https://example.com") && !tab.web_view().is_loading() => Ok(()),
            uri => Err(format!("example.com is at {uri:?}")),
        })
        .await;
        let https = open("connection-https.png").await?;
        let heading = heading_of(&https);
        let fingerprint = find::<gtk::Label>(https.upcast_ref(), |l| l.label().len() == 95 && l.label().chars().filter(|&c| c == ':').count() == 31)
            .map(|l| l.label().to_string());
        let names = find::<gtk::Expander>(https.upcast_ref(), |_| true).map(|e| e.label().unwrap_or_default().to_string());
        let chain = find::<gtk::Label>(https.upcast_ref(), |l| l.label() == "Certificate chain").is_some();
        https.popdown();
        window.select_tab(&first);
        window.close_tab(&tab);
        let detail = format!("HTTP: {http_heading:?}; example.com: {heading:?}, SHA-256 {fingerprint:?}, {names:?}, chain shown={chain} (connection-http.png, connection-https.png)");
        if heading.as_deref() == Some("Connection is secure") && fingerprint.is_some() && names.is_some() && chain { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("site_permissions", CHECK_TIMEOUT, |last| async move {
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let view = tab.web_view();
        let origin = Origin::of(&ctx.server.url("/")).ok_or_else(|| "the fixture server has no origin".to_owned())?;
        let _cleanup = Cleanup(|| {
            if let Some(bubble) = window.address_bar().bubble() {
                bubble.popdown();
            }
            if let Some(dialog) = window.visible_dialog() {
                dialog.close();
            }
            if let Err(e) = browser.core().borrow_mut().site_permissions().reset_site(&origin) {
                log::warn!("site permissions: {e}");
            }
        });
        let stored = || browser.core().borrow_mut().site_permissions().get(&origin, Permission::Location);
        let address = window.address_bar();
        // WebKit keeps a document's first answer, so each request after it comes from a fresh load.
        let reload = || async {
            eval_js(view, "window.stale = true").await?;
            view.reload();
            wait_js(&last, view, "String(!window.stale && document.readyState == 'complete')", |s| s == "true").await;
            Ok::<_, String>(())
        };
        let query = || async {
            eval_js(view, QUERY_LOCATION).await?;
            Ok::<_, String>(wait_js(&last, view, LOCATION_STATE, |s| !s.is_empty()).await)
        };

        eval_js(view, ASK_LOCATION).await?;
        let prompt = wait_for(&last, || address.prompt().ok_or_else(|| "no permission prompt".to_owned())).await;
        let heading = heading_of(&prompt);
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&prompt), &ctx.out_dir.join("permission-prompt.png"))
            .await
            .map_err(|e| e.to_string())?;
        glib::timeout_future(crate::permissions::PROMPT_GUARD).await;
        button_labelled(prompt.upcast_ref(), Answer::AllowWhileVisiting.label())
            .ok_or_else(|| "the prompt has no Allow while visiting".to_owned())?
            .emit_clicked();
        wait_for(&last, || if stored() == Some(Setting::Allow) { Ok(()) } else { Err(format!("location is stored as {:?}", stored())) }).await;
        let state = query().await?;

        reload().await?;
        let state_after_reload = query().await?;
        eval_js(view, ASK_LOCATION).await?;
        glib::timeout_future(Duration::from_secs(1)).await;
        let asked_again = address.prompt().is_some();
        let second = eval_js(view, LOCATION_OUTCOME).await?;

        address.click_security();
        let info = address.bubble().ok_or_else(|| "the security icon opened no popover".to_owned())?;
        let row = find::<adw::ComboRow>(info.upcast_ref(), |r| r.title() == Permission::Location.label())
            .ok_or_else(|| "site info has no Location row".to_owned())?;
        select(&row, "Block")?;
        wait_for(&last, || if stored() == Some(Setting::Block) { Ok(()) } else { Err(format!("after Block, location is stored as {:?}", stored())) }).await;
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&info), &ctx.out_dir.join("site-info-permissions.png"))
            .await
            .map_err(|e| e.to_string())?;
        info.popdown();

        reload().await?;
        eval_js(view, ASK_LOCATION).await?;
        let blocked = wait_js(&last, view, LOCATION_OUTCOME, |s| !s.is_empty()).await;
        let asked_when_blocked = address.prompt().is_some();

        gio::prelude::ActionGroupExt::activate_action(window, "show-settings", None);
        let dialog = window
            .visible_dialog()
            .and_downcast::<adw::PreferencesDialog>()
            .ok_or_else(|| "win.show-settings opened no preferences dialog".to_owned())?;
        dialog.set_visible_page_name("privacy");
        find::<adw::ActionRow>(dialog.upcast_ref(), |r| r.title() == "Site Permissions")
            .ok_or_else(|| "the Privacy page has no Site Permissions row".to_owned())?
            .emit_by_name::<()>("activated", &[]);
        glib::timeout_future(Duration::from_millis(600)).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("settings-site-permissions.png")).await.map_err(|e| e.to_string())?;
        let listed = find::<adw::ComboRow>(dialog.upcast_ref(), |r| r.title() == Permission::Location.label() && r.is_mapped()).map(|r| selected_label(&r));
        find::<gtk::Button>(dialog.upcast_ref(), |b| b.tooltip_text().as_deref() == Some("Remove") && b.is_mapped())
            .ok_or_else(|| "the Location row has no Remove button".to_owned())?
            .emit_clicked();
        wait_for(&last, || if stored().is_none() { Ok(()) } else { Err(format!("after Remove, location is stored as {:?}", stored())) }).await;
        glib::timeout_future(POPOVER_SETTLE).await;
        let empty = find::<adw::StatusPage>(dialog.upcast_ref(), |p| p.is_mapped()).and_then(|p| p.description()).map(String::from);
        crate::screenshot::save_png(window, &ctx.out_dir.join("settings-site-permissions-empty.png")).await.map_err(|e| e.to_string())?;

        let detail = format!(
            "prompt {heading:?} (permission-prompt.png); Allow while visiting stored Allow and the page's permissions.query reads {state:?}, after a reload {state_after_reload:?}; the reloaded page's request prompted={asked_again} (outcome {second:?}; without GeoClue the position never arrives); Block in site info stored Block (site-info-permissions.png) and the next request failed with code {blocked:?}, prompted={asked_when_blocked}; Settings lists Location as {listed:?} (settings-site-permissions.png); Remove there went back to Ask and left {empty:?} (settings-site-permissions-empty.png)"
        );
        let ok = heading.as_deref() == Some("Know your location?")
            && state == "granted"
            && state_after_reload == "granted"
            && !asked_again
            && second != "1"
            && blocked == "1"
            && !asked_when_blocked
            && listed.as_deref() == Some("Block")
            && empty.as_deref() == Some("Sites you allow or block show here.");
        if ok { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("tracking_protection", CHECK_TIMEOUT, |last| async move {
        let url = ctx.server.url("/trackers.html");
        let origin = Origin::of(&url).ok_or_else(|| "the fixture server has no origin".to_owned())?;
        let first = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let opened: RefCell<Option<Tab>> = RefCell::new(None);
        let _cleanup = Cleanup(|| {
            if let Some(bubble) = window.address_bar().bubble() {
                bubble.popdown();
            }
            if let Some(dialog) = window.visible_dialog() {
                dialog.close();
            }
            if let Some(tab) = opened.take() {
                window.select_tab(&first);
                window.close_tab(&tab);
            }
            browser.reset_pref(&keys::TRACKING_PROTECTION);
            if let Err(e) = trackers::set_allowed(&mut browser.core().borrow_mut(), &origin, false) {
                log::warn!("tracking protection: {e}");
            }
            browser.trackers().use_list(Cow::Borrowed(TrackerList::bundled()));
        });
        let tracker_loaded = || ctx.server.hits().iter().any(|path| path == "/tracker/pixel.png");
        let title_is = |view: &webkit::WebView, title: &str, loaded: bool| {
            let (shown, seen) = (title_of(view), tracker_loaded());
            if shown == title && seen == loaded { Ok(()) } else { Err(format!("title {shown:?}, the server saw the tracker: {seen}")) }
        };

        browser.trackers().use_list(Cow::Owned(TrackerList::bundled().clone().with_tracker("localhost", Category::Analytics)));
        tracking_applied(browser).await;
        let tab = window.open_tab(Some(url.as_str()), None, Focus::Foreground);
        opened.replace(Some(tab.clone()));
        let view = tab.web_view();
        wait_for(&last, || title_is(view, "tracker blocked", false)).await;

        let address = window.address_bar();
        address.click_security();
        let info = address.bubble().ok_or_else(|| "the security icon opened no popover".to_owned())?;
        let switch = find::<adw::SwitchRow>(info.upcast_ref(), |r| r.title() == trackers::TITLE)
            .ok_or_else(|| "site info has no tracking protection switch".to_owned())?;
        let shown_on = (switch.is_active(), switch.subtitle().unwrap_or_default().to_string());
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&info), &ctx.out_dir.join("site-info-trackers.png"))
            .await
            .map_err(|e| e.to_string())?;
        switch.set_active(false);
        let shown_off = switch.subtitle().unwrap_or_default().to_string();
        info.popdown();
        wait_for(&last, || title_is(view, "tracker loaded", true)).await;
        let exception = trackers::allowed(&mut browser.core().borrow_mut(), &origin);

        gio::prelude::ActionGroupExt::activate_action(window, "show-settings", None);
        let dialog = window
            .visible_dialog()
            .and_downcast::<adw::PreferencesDialog>()
            .ok_or_else(|| "win.show-settings opened no preferences dialog".to_owned())?;
        dialog.set_visible_page_name("privacy");
        let row = find::<adw::ComboRow>(dialog.upcast_ref(), |r| r.title() == TRACKING_PROTECTION_ROW)
            .ok_or_else(|| "the Privacy page has no tracking protection row".to_owned())?;
        let listed = (selected_label(&row), row.subtitle().unwrap_or_default().to_string());
        select(&row, TrackingProtection::Strict.label())?;
        let strict = (browser.pref(&keys::TRACKING_PROTECTION), row.subtitle().unwrap_or_default().to_string());
        dialog.close();

        let standard = TrackingProtection::Standard;
        let detail = format!(
            "with localhost a tracker, /trackers.html at 127.0.0.1 showed \"tracker blocked\" and the server never saw /tracker/pixel.png; site info's switch was {shown_on:?} (site-info-trackers.png), switched off it read {shown_off:?}, stored the exception ({exception}) and the reloaded page loaded the tracker; Settings > Privacy shows {listed:?}, and choosing Strict there stored {strict:?}"
        );
        let ok = shown_on == (true, trackers::site_status(true, None))
            && shown_off == trackers::site_status(false, None)
            && exception
            && listed == (standard.label().to_owned(), standard.description().to_owned())
            && strict == (TrackingProtection::Strict, TrackingProtection::Strict.description().to_owned());
        if ok { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("https_only", CHECK_TIMEOUT, |last| async move {
        let origin = Origin::of(index_url).ok_or_else(|| "the fixture server has no origin".to_owned())?;
        let first = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let opened: RefCell<Option<Tab>> = RefCell::new(None);
        let _cleanup = Cleanup(|| {
            if let Some(dialog) = window.visible_dialog() {
                dialog.close();
            }
            if let Some(tab) = opened.take() {
                window.select_tab(&first);
                window.close_tab(&tab);
            }
            browser.reset_pref(&keys::HTTPS_ONLY);
            browser.set_https_reach(Reach::Public);
            if let Err(e) = browser.core().borrow_mut().site_permissions().set(&origin, Permission::Http, None) {
                log::warn!("HTTPS-only exception: {e}");
            }
        });

        gio::prelude::ActionGroupExt::activate_action(window, "show-settings", None);
        let dialog = window
            .visible_dialog()
            .and_downcast::<adw::PreferencesDialog>()
            .ok_or_else(|| "win.show-settings opened no preferences dialog".to_owned())?;
        dialog.set_visible_page_name("privacy");
        let switch = find::<adw::SwitchRow>(dialog.upcast_ref(), |r| r.title() == HTTPS_ONLY_ROW)
            .ok_or_else(|| "the Privacy page has no HTTPS-only switch".to_owned())?;
        let dns = find::<adw::ActionRow>(dialog.upcast_ref(), |r| r.title() == SECURE_DNS_ROW)
            .map(|r| r.subtitle().unwrap_or_default().to_string())
            .ok_or_else(|| "the Privacy page has no Secure DNS row".to_owned())?;
        let off = (switch.is_active(), browser.pref(&keys::HTTPS_ONLY));
        switch.set_active(true);
        let on = browser.pref(&keys::HTTPS_ONLY);
        dialog.close();

        browser.set_https_reach(Reach::Everywhere);
        let tab = window.open_tab(Some(index_url.as_str()), None, Focus::Foreground);
        opened.replace(Some(tab.clone()));
        let at = |title: &str| {
            let (shown, uri) = (title_of(tab.web_view()), tab.committed_uri().unwrap_or_default());
            if shown == title && uri == index_url.as_str() && !tab.web_view().is_loading() {
                Ok(())
            } else {
                Err(format!("at {uri:?}, titled {shown:?}"))
            }
        };
        wait_for(&last, || at(https_only::WARNING_TITLE)).await;
        let insecure = tab.security() == Security::Insecure;
        crate::screenshot::save_png(window, &ctx.out_dir.join("https-only-warning.png")).await.map_err(|e| e.to_string())?;
        eval_js(tab.web_view(), "document.getElementById('continue').click()").await?;
        wait_for(&last, || at("Vsesvit fixture")).await;
        let allowed = https_only::allowed(&mut browser.core().borrow_mut(), &origin);

        let detail = format!(
            "Settings > Privacy's {HTTPS_ONLY_ROW:?} switch and preference were {off:?}, switched on stored {on}; Secure DNS reads {dns:?}; {index_url} failed over https and showed {:?} at its own address, marked insecure ({insecure}) (https-only-warning.png); Continue to site loaded it over http and stored the exception ({allowed})",
            https_only::WARNING_TITLE
        );
        let ok = off == (false, false) && on && !dns.is_empty() && insecure && allowed;
        if ok { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("capture_in_use", CHECK_TIMEOUT, |last| async move {
        let tab = window.selected_tab().ok_or_else(|| "no selected tab".to_owned())?;
        let view = tab.web_view();
        let origin = Origin::of(&ctx.server.url("/")).ok_or_else(|| "the fixture server has no origin".to_owned())?;
        let settings = browser.engine().settings();
        let (mock_devices, position) = (settings.enables_mock_capture_devices(), browser.tabs_position());
        let _cleanup = Cleanup(|| {
            for permission in [Permission::Camera, Permission::Microphone, Permission::ScreenShare] {
                crate::permissions::stop(view, permission);
            }
            if let Some(bubble) = window.address_bar().bubble() {
                bubble.popdown();
            }
            browser.set_tabs_position(position);
            settings.set_enable_mock_capture_devices(mock_devices);
            if let Err(e) = browser.core().borrow_mut().site_permissions().reset_site(&origin) {
                log::warn!("site permissions: {e}");
            }
        });
        settings.set_enable_mock_capture_devices(true);
        let address = window.address_bar();

        eval_js(view, ASK_CAMERA_AND_MICROPHONE).await?;
        let prompt = wait_for(&last, || address.prompt().ok_or_else(|| "no permission prompt".to_owned())).await;
        let heading = heading_of(&prompt);
        let buttons: Vec<String> = all::<gtk::Button>(prompt.upcast_ref()).iter().filter_map(|b| b.label()).map(String::from).collect();
        let expected: Vec<&str> = [Answer::AllowWhileVisiting, Answer::AllowThisTime, Answer::NeverAllow, Answer::Dismiss].map(Answer::label).to_vec();
        glib::timeout_future(crate::permissions::PROMPT_GUARD).await;
        button_labelled(prompt.upcast_ref(), Answer::AllowThisTime.label())
            .ok_or_else(|| "the prompt has no Allow this time".to_owned())?
            .emit_clicked();
        let tracks = wait_js(&last, view, CAPTURE_OUTCOME, |s| !s.is_empty()).await;
        let in_use = wait_for(&last, || match address.shown_in_use() {
            Some(tooltip) if tab.capturing().camera && tab.capturing().microphone => Ok(tooltip),
            shown => Err(format!("capturing {:?}, the address bar shows {shown:?}", tab.capturing())),
        })
        .await;
        let indicator = window.tab_indicator(&tab);
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("capture-in-use.png")).await.map_err(|e| e.to_string())?;
        browser.set_tabs_position(TabsPosition::Top);
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png(window, &ctx.out_dir.join("capture-in-use-top.png")).await.map_err(|e| e.to_string())?;
        browser.set_tabs_position(position);

        address.click_in_use();
        let info = address.bubble().ok_or_else(|| "the in-use button opened no popover".to_owned())?;
        let row = |info: &gtk::Popover, permission: Permission| find::<adw::ComboRow>(info.upcast_ref(), |r| r.title() == permission.label());
        let stop_of = |row: &adw::ComboRow| find::<gtk::Button>(row.upcast_ref(), |b| b.label().as_deref() == Some("Stop") && b.is_visible());
        let before: Vec<String> = [Permission::Camera, Permission::Microphone]
            .into_iter()
            .filter_map(|p| row(&info, p))
            .map(|r| format!("{}: {}, Stop={}", r.title(), selected_label(&r), stop_of(&r).is_some()))
            .collect();
        glib::timeout_future(POPOVER_SETTLE).await;
        crate::screenshot::save_png_with_popovers(window.upcast_ref(), std::slice::from_ref(&info), &ctx.out_dir.join("site-info-capture.png"))
            .await
            .map_err(|e| e.to_string())?;
        let camera = row(&info, Permission::Camera).ok_or_else(|| "site info has no Camera row".to_owned())?;
        select(&camera, "Block")?;
        wait_for(&last, || if tab.capturing().camera { Err("blocking the camera left it capturing".to_owned()) } else { Ok(()) }).await;
        let camera_stored = browser.core().borrow_mut().site_permissions().get(&origin, Permission::Camera);
        let microphone_on = tab.capturing().microphone;
        let stop = wait_for(&last, || {
            row(&info, Permission::Microphone).and_then(|r| stop_of(&r)).ok_or_else(|| "no Stop on the Microphone row".to_owned())
        })
        .await;
        stop.emit_clicked();
        wait_for(&last, || match (tab.capturing().any(), address.shown_in_use(), window.tab_indicator(&tab)) {
            (false, None, None) => Ok(()),
            (capturing, shown, indicator) => Err(format!("after Stop: capturing={capturing}, the address bar shows {shown:?}, the tab {indicator:?}")),
        })
        .await;
        info.popdown();
        let ended = wait_js(&last, view, CAPTURED_TRACKS, |s| s == "audio:ended video:ended").await;

        let detail = format!(
            "prompt {heading:?} with buttons {buttons:?}; Allow this time gave the page {tracks:?}; the address bar said {in_use:?} and the tab {indicator:?} (capture-in-use.png, capture-in-use-top.png); site info showed {before:?} (site-info-capture.png); Block on Camera stored {camera_stored:?} and stopped it, microphone still on={microphone_on}; Stop ended the rest and the indicators, tracks now {ended:?}; screen sharing needs a user gesture and is not scripted"
        );
        let ok = heading.as_deref() == Some("Use your camera and microphone?")
            && buttons == expected
            && tracks == "audio:live video:live"
            && in_use == "Using your camera and microphone"
            && indicator == Some(("camera-web-symbolic".to_owned(), "Using your camera and microphone".to_owned()))
            && before == ["Camera: Allowed this time, Stop=true", "Microphone: Allowed this time, Stop=true"]
            && camera_stored == Some(Setting::Block)
            && microphone_on;
        if ok { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("welcome", if ctx.network { WELCOME_TIMEOUT + NETWORK_TIMEOUT } else { WELCOME_TIMEOUT }, |last| async move {
        use crate::dialogs::welcome::{STEPS, Step};
        let theme = browser.theme();
        let engines = browser.core().borrow_mut().search_engines().list().map_err(|e| e.to_string())?;
        let engine_before = browser.core().borrow_mut().search_engines().default_engine().map_err(|e| e.to_string())?;
        let found = vsesvit_core::import::installed_browsers();
        browser.core().borrow_mut().prefs().reset(&keys::ONBOARDING_DONE).map_err(|e| e.to_string())?;
        let dialog = crate::dialogs::welcome::present(window);
        let _cleanup = Cleanup(|| {
            if window.visible_dialog().as_ref() == Some(&dialog) {
                dialog.force_close();
            }
            browser.set_theme(theme);
            if let Err(e) = browser.core().borrow_mut().search_engines().set_default(&engine_before.id) {
                log::warn!("search engines: {e}");
            }
        });
        let carousel = find::<adw::Carousel>(dialog.upcast_ref(), |_| true).ok_or_else(|| "the welcome has no carousel".to_owned())?;
        let (carousel, observed) = (&carousel, &last);
        let at = move |index: usize| {
            wait_for(observed, move || match carousel.position() {
                p if (p - index as f64).abs() < 1e-3 => Ok(()),
                p => Err(format!("the carousel is at {p}, waiting for page {index}")),
            })
        };
        let click = |label: &str| button_labelled(dialog.upcast_ref(), label).filter(|b| b.is_mapped()).ok_or_else(|| format!("no {label:?} button")).map(|b| b.emit_clicked());
        let mut notes = Vec::new();
        for (index, step) in STEPS.into_iter().enumerate() {
            at(index).await;
            let page = carousel.nth_page(index as u32);
            match step {
                Step::Search => {
                    let checks = all::<gtk::CheckButton>(&page);
                    let checked: Vec<String> = checks
                        .iter()
                        .filter(|c| c.is_active())
                        .filter_map(|c| c.ancestor(adw::ActionRow::static_type()).and_downcast::<adw::ActionRow>())
                        .map(|r| r.title().to_string())
                        .collect();
                    if checks.len() != engines.len() || checked != [engine_before.name.clone()] {
                        return Err(format!("{} engine rows for {} engines, checked {checked:?}, default {}", checks.len(), engines.len(), engine_before.name));
                    }
                    let other = engines.iter().find(|e| e.id != engine_before.id).ok_or_else(|| "only one search engine".to_owned())?;
                    let row = find::<adw::ActionRow>(&page, |r| r.title() == other.name).ok_or_else(|| format!("no row for {}", other.name))?;
                    ActionRowExt::activate(&row);
                    wait_for(&last, || match browser.core().borrow_mut().search_engines().default_engine() {
                        Ok(now) if now.id == other.id => Ok(()),
                        now => Err(format!("after choosing {}, the default is {:?}", other.name, now.map(|e| e.name))),
                    })
                    .await;
                    notes.push(format!("{} engine rows, {} checked; choosing {} made it core's default", checks.len(), engine_before.name, other.name));
                }
                Step::Import => {
                    wait_for(&last, || match find::<adw::ActionRow>(&page, |r| r.title() == "Looking for other browsers…") {
                        Some(_) => Err("still looking for other browsers".to_owned()),
                        None => Ok(()),
                    })
                    .await;
                    let imports = count::<gtk::Button>(&page, |b| b.label().as_deref() == Some("Import"));
                    let none = find::<adw::ActionRow>(&page, |r| r.title() == "No other browsers found").is_some();
                    let file = button_labelled(&page, "Choose File…").is_some();
                    if imports != found.len() || none != found.is_empty() || !file {
                        return Err(format!("{imports} Import buttons for {} browsers found, \"No other browsers found\" shown={none}, Choose File… shown={file}", found.len()));
                    }
                    notes.push(format!("{} browsers to import from, and Choose File…", found.len()));
                }
                Step::Extensions => {
                    let mut rows = Vec::new();
                    for recommended in vsesvit_core::onboarding::RECOMMENDED_EXTENSIONS {
                        let row = find::<adw::ActionRow>(&page, |r| r.title() == recommended.name).ok_or_else(|| format!("no row for {}", recommended.name))?;
                        let ready = button_labelled(row.upcast_ref(), "Install").is_some_and(|b| b.is_mapped());
                        let installed = find::<gtk::Label>(row.upcast_ref(), |l| l.label() == "Installed" && l.is_mapped()).is_some();
                        if ready == installed {
                            return Err(format!("{}: Install shown={ready}, Installed shown={installed}", recommended.name));
                        }
                        rows.push(format!("{} ({})", recommended.name, if ready { "Install" } else { "Installed" }));
                    }
                    notes.push(rows.join(", "));
                    if ctx.network {
                        let recommended = vsesvit_core::onboarding::RECOMMENDED_EXTENSIONS.iter().find(|r| r.name == "Bitwarden").ok_or_else(|| "Bitwarden is not recommended".to_owned())?;
                        let row = find::<adw::ActionRow>(&page, |r| r.title() == recommended.name).ok_or_else(|| "no Bitwarden row".to_owned())?;
                        button_labelled(row.upcast_ref(), "Install").ok_or_else(|| "no Install on the Bitwarden row".to_owned())?.emit_clicked();
                        wait_for(&last, || {
                            let installed = find::<gtk::Label>(row.upcast_ref(), |l| l.label() == "Installed" && l.is_mapped()).is_some();
                            let stored = browser.core().borrow_mut().extensions().get(&recommended.id()).ok().flatten().is_some();
                            match (installed, stored, row.has_css_class("error")) {
                                (true, true, false) => Ok(()),
                                (_, _, true) => Err(format!("the install failed: {:?}", row.subtitle())),
                                now => Err(format!("(Installed shown, in the profile, error) = {now:?}")),
                            }
                        })
                        .await;
                        notes.push("Install on Bitwarden installed it from the Web Store".to_owned());
                    }
                }
                Step::DefaultBrowser => {
                    let heading = page.downcast_ref::<adw::StatusPage>().map(|p| (p.title(), p.description().unwrap_or_default())).unwrap_or_default();
                    let status = find::<gtk::Label>(&page, |l| {
                        l.wraps() && !l.label().is_empty() && l.label() != heading.0 && l.label() != heading.1 && !l.has_css_class("error")
                    })
                        .map(|l| l.label().to_string())
                        .ok_or_else(|| "the page shows no status".to_owned())?;
                    let button = button_labelled(&page, "Make Vsesvit the Default Browser").is_some_and(|b| b.is_visible());
                    notes.push(format!("default browser: {status:?}, button shown={button}"));
                }
                Step::Sync => {
                    let server = browser.core().borrow_mut().prefs().get(&keys::SYNC_SERVER);
                    let row = find::<adw::EntryRow>(&page, |r| r.title() == "Sync Server").ok_or_else(|| "no Sync Server row".to_owned())?;
                    let sign_in = button_labelled(&page, "Sign In").is_some_and(|b| b.is_visible());
                    if row.text() != server || !sign_in {
                        return Err(format!("Sync Server shows {:?} for {server:?}, Sign In shown={sign_in}", row.text()));
                    }
                    notes.push(format!("sync: Sign In and the server {server}"));
                    row.set_text("https://sync.example.com/");
                    row.emit_by_name::<()>("apply", &[]);
                    let custom = browser.core().borrow_mut().prefs().get(&keys::SYNC_SERVER);
                    row.set_text("");
                    row.emit_by_name::<()>("apply", &[]);
                    let cleared = browser.core().borrow_mut().prefs().get(&keys::SYNC_SERVER);
                    if custom != "https://sync.example.com" || cleared != DEFAULT_SYNC_SERVER || row.text() != DEFAULT_SYNC_SERVER || row.has_css_class("error") {
                        return Err(format!("Sync Server stored {custom:?}, then cleared stored {cleared:?} and showed {:?}, error={}", row.text(), row.has_css_class("error")));
                    }
                    notes.push("clearing Sync Server went back to the default server".to_owned());
                }
                Step::Welcome | Step::Done => {}
            }
            glib::timeout_future(POPOVER_SETTLE).await;
            let name = step.name();
            crate::screenshot::save_png(window, &ctx.out_dir.join(format!("welcome-{name}.png"))).await.map_err(|e| e.to_string())?;
            browser.set_theme(Theme::Dark);
            glib::timeout_future(POPOVER_SETTLE).await;
            crate::screenshot::save_png(window, &ctx.out_dir.join(format!("welcome-{name}-dark.png"))).await.map_err(|e| e.to_string())?;
            browser.set_theme(theme);
            if step == Step::Import {
                click("Back")?;
                at(index - 1).await;
                click(STEPS[index - 1].next_label())?;
                at(index).await;
                notes.push("Back and Next returned to the import page".to_owned());
            }
            click(step.next_label())?;
        }
        wait_for(&last, || if window.visible_dialog().is_none() { Ok(()) } else { Err("the welcome is still open".to_owned()) }).await;
        let done = browser.core().borrow_mut().prefs().get(&keys::ONBOARDING_DONE);
        let detail = format!("{}; Start Browsing closed it, onboarding.done={done}; welcome-*.png in light and dark", notes.join("; "));
        if done { Ok(detail) } else { Err(detail) }
    })
    .await;

    ctx.check("screenshot", CHECK_TIMEOUT, |_| async move {
        let path = ctx.out_dir.join("window.png");
        crate::screenshot::save_png(window, &path).await.map_err(|e| e.to_string())?;
        let texture = gdk::Texture::from_filename(&path).map_err(|e| e.to_string())?;
        let colors = distinct_colors(&texture, COLOR_SAMPLE_CAP);
        let detail = format!(
            "{} ({}x{} px, {colors} distinct colours sampled, cap {COLOR_SAMPLE_CAP})",
            path.display(),
            texture.width(),
            texture.height()
        );
        if colors >= 2 { Ok(detail) } else { Err(detail) }
    })
    .await;

    if ctx.network {
        ctx.check("cws_install", NETWORK_TIMEOUT, |last| async move {
            let id = ExtensionId::parse(CWS_EXTENSION).map_err(|e| e.to_string())?;
            let progress = {
                let last = last.clone();
                move |phase: InstallPhase| last.set(phase.describe())
            };
            let ext = browser
                .install(InstallSource::ChromeWebStore { id: id.clone() }, progress)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "the install committed nothing".to_owned())?;
            wait_for(&last, || {
                if browser.runtime().loaded().contains(&id) { Ok(()) } else { Err(format!("runtime has not loaded {CWS_EXTENSION}")) }
            })
            .await;
            let detail = format!("{} {} verification={:?}, loaded by the runtime", ext.manifest.name, ext.version, ext.verification);
            if ext.verification == Verification::ChromeWebStore { Ok(detail) } else { Err(detail) }
        })
        .await;
    }
}

/// Waits until tracking protection's latest blocker is on every tab.
async fn tracking_applied(browser: &Browser) {
    let (done, applied) = futures_channel::oneshot::channel();
    browser.trackers().when_applied(move || {
        let _ = done.send(());
    });
    let _ = applied.await;
}

/// The first widget of type `W` under `root` (itself included) that `matches`.
fn find<W: IsA<gtk::Widget>>(root: &gtk::Widget, matches: impl Fn(&W) -> bool + Copy) -> Option<W> {
    if let Some(widget) = root.downcast_ref::<W>()
        && matches(widget)
    {
        return Some(widget.clone());
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(found) = find(&widget, matches) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

fn count<W: IsA<gtk::Widget>>(root: &gtk::Widget, matches: impl Fn(&W) -> bool + Copy) -> usize {
    let mut n = usize::from(root.downcast_ref::<W>().is_some_and(matches));
    let mut child = root.first_child();
    while let Some(widget) = child {
        n += count(&widget, matches);
        child = widget.next_sibling();
    }
    n
}

fn button_labelled(root: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    find::<gtk::Button>(root, |b| b.label().as_deref() == Some(label))
}

/// A bookmark menu row showing `text`.
fn button_labelled_in_row(root: &gtk::Widget, text: &str) -> Option<gtk::Button> {
    find::<gtk::Button>(root, |b| b.has_css_class("bookmark-menu-row") && find::<gtk::Label>(b.upcast_ref(), |l| l.label() == text).is_some())
}

/// The popover `widget` holds, if one is open.
fn popover_of(widget: &gtk::Widget) -> Option<gtk::Popover> {
    let mut child = widget.first_child();
    while let Some(c) = child {
        if let Some(popover) = c.downcast_ref::<gtk::Popover>().filter(|p| p.is_visible()) {
            return Some(popover.clone());
        }
        child = c.next_sibling();
    }
    None
}

/// Runs its closure when dropped, so a check's cleanup also runs when it fails or times out.
struct Cleanup<F: FnMut()>(F);

impl<F: FnMut()> Drop for Cleanup<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// Settings over `window`, on the Search page's Search Engines subpage, and the subpage's Add button.
async fn open_search_engines(window: &BrowserWindow, last: &Last) -> Result<(adw::PreferencesDialog, gtk::Button), String> {
    gio::prelude::ActionGroupExt::activate_action(window, "show-settings", None);
    let settings = window
        .visible_dialog()
        .and_downcast::<adw::PreferencesDialog>()
        .ok_or_else(|| "win.show-settings opened no preferences dialog".to_owned())?;
    settings.set_visible_page_name("search");
    let manage = find::<adw::ActionRow>(settings.upcast_ref(), |r| r.title() == "Manage Search Engines");
    let Some(manage) = manage else {
        settings.close();
        return Err("the Search page has no Manage Search Engines row".to_owned());
    };
    ActionRowExt::activate(&manage);
    let add = wait_for(last, || {
        find::<adw::ButtonContent>(settings.upcast_ref(), |c| c.label() == "_Add" && c.is_mapped())
            .and_then(|content| content.parent().and_downcast::<gtk::Button>())
            .ok_or_else(|| "the Search Engines subpage shows no Add button".to_owned())
    })
    .await;
    Ok((settings, add))
}

/// Every widget of type `W` under `root`, in order.
fn all<W: IsA<gtk::Widget>>(root: &gtk::Widget) -> Vec<W> {
    let mut found: Vec<W> = root.downcast_ref::<W>().cloned().into_iter().collect();
    let mut child = root.first_child();
    while let Some(widget) = child {
        found.extend(all(&widget));
        child = widget.next_sibling();
    }
    found
}

/// Picks the choice labelled `label` in a combo row, as the user would from its list.
fn select(row: &adw::ComboRow, label: &str) -> Result<(), String> {
    let model = row.model().and_downcast::<gtk::StringList>().ok_or_else(|| format!("{} has no choices", row.title()))?;
    let at = (0..model.n_items()).find(|&i| model.string(i).as_deref() == Some(label)).ok_or_else(|| format!("{} offers no {label:?}", row.title()))?;
    row.set_selected(at);
    Ok(())
}

fn selected_label(row: &adw::ComboRow) -> String {
    row.selected_item().and_downcast::<gtk::StringObject>().map(|s| s.string().to_string()).unwrap_or_default()
}

/// Polls `script` until `done` accepts what it returns.
async fn wait_js(last: &Last, view: &webkit::WebView, script: &str, done: impl Fn(&str) -> bool) -> String {
    loop {
        let value = eval_js(view, script).await.unwrap_or_else(|e| format!("error: {e}"));
        if done(&value) {
            return value;
        }
        last.set(format!("{script} -> {value:?}"));
        glib::timeout_future(POLL).await;
    }
}

fn heading_of(bubble: &gtk::Popover) -> Option<String> {
    find::<gtk::Label>(bubble.upcast_ref(), |l| l.has_css_class("heading")).map(|l| l.label().into())
}

/// The text's length in characters, as GTK's `i32` text positions count it.
fn char_len(text: &str) -> i32 {
    i32::try_from(text.chars().count()).unwrap_or(i32::MAX)
}

/// How many different pixel values the image has, stopping at `cap`.
fn distinct_colors(texture: &gdk::Texture, cap: usize) -> usize {
    let downloader = gdk::TextureDownloader::new(texture);
    let (bytes, stride) = downloader.download_bytes();
    let (width, height) = (texture.width().max(0).cast_unsigned() as usize, texture.height().max(0).cast_unsigned() as usize);
    let mut seen: HashSet<[u8; 4]> = HashSet::new();
    for y in 0..height {
        for x in 0..width {
            let at = y * stride + x * 4;
            let Some(pixel) = bytes.get(at..at + 4) else { continue };
            let mut color = [0u8; 4];
            color.copy_from_slice(pixel);
            seen.insert(color);
            if seen.len() >= cap {
                return seen.len();
            }
        }
    }
    seen.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_that_stops_early_fails() {
        let report = RefCell::new(Report::new("linux", expected(false)));
        record(&report, "profile_open", true, Instant::now(), String::new());
        let mut report = report.into_inner();
        assert!(!report.ok());
        report.complete("stopped");
        assert_eq!(report.checks()[1].name, "install_crx");
        assert!(!report.checks()[1].ok);
        assert!(!report.ok());
    }

    #[test]
    fn checks_lists_every_check_run_checks_runs() {
        let source = include_str!("self_test.rs");
        let run: Vec<&str> = source
            .split("ctx.check(\"")
            .skip(1)
            .filter_map(|rest| rest.split_once('"').map(|(name, _)| name))
            .collect();
        let mut listed = expected(true);
        listed.remove(0);
        assert_eq!(run, listed);
    }
}
