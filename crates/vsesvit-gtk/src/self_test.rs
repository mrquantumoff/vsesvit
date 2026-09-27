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
use vsesvit_core::extensions::{ExtensionId, InstallSource, Verification};
use vsesvit_core::prefs::TabsPosition;
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
/// uBlock Origin Lite.
const CWS_EXTENSION: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";
/// How many distinct colours the screenshot check samples before it is satisfied.
const COLOR_SAMPLE_CAP: usize = 64;
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
