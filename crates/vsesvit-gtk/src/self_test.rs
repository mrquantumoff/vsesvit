//! `vsesvit --self-test OUT_DIR [--network]`: the scripted end-to-end check from
//! `docs/design/self-test.md`, on a fresh profile at `OUT_DIR/profile`. It builds the real
//! browser (`Browser`, windows, address bar, star, toolbar buttons) around that profile and
//! drives those widgets, writing `report.json` and `window.png` and exiting non-zero if any
//! check fails.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::downloads::{State, status_line};
use vsesvit_core::extensions::{ExtensionId, InstallSource, Verification};
use vsesvit_core::permissions::{Answer, Origin, Permission, Setting};
use vsesvit_core::prefs::{DEFAULT_SYNC_SERVER, TabsPosition, Theme, keys};
use vsesvit_core::search::{NavTarget, SearchEngineId, UrlTemplate};
use vsesvit_core::shortcuts::{Chord, Command, Keymap};
use vsesvit_core::testkit::{self, FixtureServer};
use vsesvit_core::{OpenOptions, Profile};
use webkit::prelude::*;

use crate::browser::Browser;
use crate::dialogs::{Windowed, shortcut_settings};
use crate::extensions::describe_phase;
use crate::keymap;
use crate::window::{Focus, classify_layout};

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

struct Check {
    name: &'static str,
    ok: bool,
    ms: u128,
    detail: String,
}

#[derive(Default)]
struct Report {
    checks: Vec<Check>,
}

impl Report {
    fn push(&mut self, name: &'static str, ok: bool, started: Instant, detail: String) {
        let ms = started.elapsed().as_millis();
        println!("[self-test] {name}: {} ({ms} ms) {detail}", if ok { "ok" } else { "FAIL" });
        self.checks.push(Check { name, ok, ms, detail });
    }

    fn ok(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "platform": "linux",
            "ok": self.ok(),
            "checks": self.checks.iter().map(|c| serde_json::json!({
                "name": c.name,
                "ok": c.ok,
                "ms": u64::try_from(c.ms).unwrap_or(u64::MAX),
                "detail": c.detail,
            })).collect::<Vec<_>>(),
        })
    }
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
    let report = Rc::new(RefCell::new(Report::default()));

    let started = Instant::now();
    let profile = match Profile::open(&profile_dir, OpenOptions::default()) {
        Ok(profile) => {
            report.borrow_mut().push("profile_open", true, started, format!("root={}", profile_dir.display()));
            profile
        }
        Err(e) => {
            report.borrow_mut().push("profile_open", false, started, e.to_string());
            return finish(out_dir, &report);
        }
    };
    let server = match FixtureServer::start() {
        Ok(server) => {
            // `favicon_preload` has core fetch a bookmark's icon from the loopback server.
            vsesvit_core::favicons::allow_local_hosts();
            server
        }
        Err(e) => {
            report.borrow_mut().push("fixture_server", false, started, e.to_string());
            return finish(out_dir, &report);
        }
    };
    let crx_path = out_dir.join("probe.crx");
    if let Err(e) = std::fs::write(&crx_path, testkit::probe_crx()) {
        report.borrow_mut().push("probe_crx", false, started, e.to_string());
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
    let report = report.borrow();
    let json = report.to_json();
    let path = out_dir.join("report.json");
    match serde_json::to_string_pretty(&json) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                eprintln!("vsesvit: cannot write {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        }
        Err(e) => {
            eprintln!("vsesvit: cannot serialize the report: {e}");
            return ExitCode::FAILURE;
        }
    }
    let passed = report.checks.iter().filter(|c| c.ok).count();
    println!(
        "[self-test] {}: {passed}/{} checks passed; report at {}",
        if report.ok() { "PASS" } else { "FAIL" },
        report.checks.len(),
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
        self.report.borrow_mut().push(name, ok, started, detail);
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
        ctx.report.borrow_mut().push("window", false, Instant::now(), "startup opened no window".into());
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

    ctx.check("selection_search", CHECK_TIMEOUT, |last| async move {
        let template = UrlTemplate(format!("{}/search?q={{searchTerms}}", ctx.server.origin()));
        let engine = browser.core().borrow_mut().search_engines().add("Fixture Search", None, template).map_err(|e| e.to_string())?;
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
            "engine (pop-ups, smooth, GPU) = {engine:?}; Home opened {page2_url}; home-button.png and {} written",
            shots.join(", ")
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
            let capture = shortcut_settings::capture(&dialog, browser, Command::ShowHistory, || {});
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
                move |phase| last.set(describe_phase(&phase))
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
            if ext.verification == (Verification::ChromeWebStore { publisher_verified: true }) { Ok(detail) } else { Err(detail) }
        })
        .await;
    }
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

/// How many different pixel values the image has, stopping at `cap`.
fn char_len(text: &str) -> i32 {
    i32::try_from(text.chars().count()).unwrap_or(i32::MAX)
}

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
