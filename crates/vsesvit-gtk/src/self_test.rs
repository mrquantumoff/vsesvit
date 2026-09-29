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
use vsesvit_core::prefs::{TabsPosition, keys};
use vsesvit_core::search::NavTarget;
use vsesvit_core::testkit::{self, FixtureServer};
use vsesvit_core::{OpenOptions, Profile};
use webkit::prelude::*;

use crate::browser::Browser;
use crate::extensions::describe_phase;
use crate::window::{Focus, classify_layout};

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// The Web Store install downloads about 10 MB; it gets longer than the default.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(50);
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
        Ok(server) => server,
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
            let on_bar = window.bookmarks_bar().shows(index_url.as_str());
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
        let dialog = window.visible_dialog().ok_or_else(|| "win.show-downloads opened no dialog".to_owned())?;
        glib::timeout_future(Duration::from_millis(500)).await;
        let shot = crate::screenshot::save_png(window, &ctx.out_dir.join("downloads.png")).await;
        dialog.close();
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
        for name in ["general", "appearance", "search", "privacy"] {
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
