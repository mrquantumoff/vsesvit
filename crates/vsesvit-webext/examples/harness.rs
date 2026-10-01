//! End-to-end check of the runtime under a real WebKitGTK, mirroring the self-test's
//! `content_script`, `dnr_blocked` and `popup` checks (docs/design/self-test.md) and
//! covering what the self-test's single fixture cannot:
//!
//! 1. serve `tests/fixtures/site/` from a local HTTP server that logs request paths;
//! 2. install three extensions through the real pipeline into a fresh profile: the probe
//!    (`tests/fixtures/extensions/probe/`, a signed CRX), the *twin* (an XPI with a Gecko
//!    id, two `content_scripts` entries, an options page, `scripting` + `activeTab` and a
//!    host permission for the fixture server only) and the *widget* (a popup that frames
//!    a fixture page, no host permissions);
//! 3. open a tab on `/index.html`: the probe's content script round-trips to its
//!    background; both twin entries run in one world, its `"world": "MAIN"` entry in the
//!    page's;
//! 4. the server saw `/allowed.png` and never `/vsesvit-blocked/pixel.png`;
//! 5. the probe popup shows `visits=N`; page APIs and events work in it; its
//!    `scripting.executeScript` is refused (no `scripting` permission); its `<all_urls>`
//!    reaches no `file:` page (no content script, no tab URL);
//! 6. the widget popup's iframe loads in place instead of being blanked and opened as a tab;
//! 7. the twin popup opens the options page in a tab, where `chrome.*` works
//!    (storage, `runtime.getURL` on the hashed host, messaging both ways, `tabs.getCurrent`,
//!    `tabs.onUpdated`) and its CSP holds; `runtime.sendMessage` reaches every page;
//!    `scripting.executeScript` injects the content-script API into a tab without a
//!    manifest content script, accepts `/`-prefixed files, is refused for a tab outside the
//!    host permissions until `activeTab` grants it and for one still showing such a page while it loads another;
//!    `tabs.query` hides that tab's URL; `action.setPopup(getURL(..))` and `setIcon('/..')`
//!    resolve; `tabs.create` resolves relative URLs and `tabs.update` refuses
//!    `javascript:` and `file:`; a web page cannot navigate a tab to the options page, with
//!    or without a Referer, nor get it by a reload, while going back to it still works;
//! 8. lifecycle: the first load fires `onInstalled(install)`, a re-enable fires nothing,
//!    and an uninstall followed by a reinstall fires `onInstalled(install)` again.
//!
//! Prints every observation and exits non-zero on failure. Runs under WSLg; the window
//! is created but not presented unless `--show` is given, so nothing steals focus.
//!
//! ```text
//! bash scripts/wsl.sh run -p vsesvit-webext --example harness [-- --show]
//! ```

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the vsesvit-webext harness runs on Linux only");
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    linux::main()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::cell::{Cell, RefCell};
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use serde_json::Value;
    use vsesvit_core::ext_storage::Area;
    use vsesvit_core::extensions::{ExtensionId, InstallSource, InstalledExtension};
    use vsesvit_core::{OpenOptions, Profile};
    use vsesvit_webext::{LoadReason, Runtime, TabHost, TabId, TabInfo};
    use webkit::glib;
    use webkit::prelude::*;

    const TIMEOUT: Duration = Duration::from_secs(20);
    const TWIN_ID: &str = "twin@vsesvit.test";

    pub fn main() -> ExitCode {
        let show = std::env::args().any(|a| a == "--show");
        let _ = log::set_logger(&STDOUT_LOG).map(|()| log::set_max_level(log::LevelFilter::Debug));
        // WSLg has no working DMA-BUF path for WebKit's compositor; harmless elsewhere.
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            // SAFETY: called before any other thread exists.
            unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
        }
        gtk::init().expect("gtk::init");

        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let site = repo.join("tests/fixtures/site");
        let server = FixtureServer::start(site);
        println!("[harness] fixture server on 127.0.0.1:{}", server.port);

        let out_dir = std::env::temp_dir().join(format!("vsesvit-webext-harness-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out_dir);
        let profile = Profile::open(&out_dir.join("profile"), OpenOptions::default()).expect("open profile");
        let paths = profile.paths().clone();
        let profile = Rc::new(RefCell::new(profile));
        let session = webkit::NetworkSession::new(paths.engine_data.to_str(), paths.engine_cache.to_str());

        let window = gtk::Window::new();
        window.set_default_size(800, 600);
        window.set_title(Some("vsesvit-webext harness"));
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.set_child(Some(&container));
        if show {
            window.present();
        }

        let host = Rc::new(Host::new(session.clone(), container));
        let runtime = Runtime::new(profile.clone(), &session, host.clone());
        *host.runtime.borrow_mut() = Some(runtime.clone());

        let probe_crx = out_dir.join("probe.crx");
        std::fs::write(&probe_crx, vsesvit_core::testkit::probe_crx()).expect("write probe.crx");
        let twin_xpi = out_dir.join("twin.xpi");
        write_xpi(&twin_xpi, &twin_files());
        let widget_xpi = out_dir.join("widget.xpi");
        write_xpi(&widget_xpi, &widget_files(server.port));

        let probe = install(&profile, &probe_crx);
        assert_eq!(probe.id.as_str(), vsesvit_core::testkit::PROBE_ID);
        let twin = install(&profile, &twin_xpi);
        assert_eq!(twin.id.as_str(), TWIN_ID);
        let widget = install(&profile, &widget_xpi);
        for ext in [&probe, &twin, &widget] {
            if let Err(e) = runtime.load(ext) {
                println!("[harness] FAIL: Runtime::load({}): {e}", ext.id.as_str());
                return ExitCode::FAILURE;
            }
        }
        println!("[harness] loaded {:?}; pending filters = {}", runtime.loaded().iter().map(|i| i.as_str()).collect::<Vec<_>>(), runtime.pending_filters());

        let tab = host.create_tab("about:blank", true).expect("first tab");
        let view = host.web_view(tab).expect("first tab view");

        let main_loop = glib::MainLoop::new(None, false);
        let outcome = Rc::new(Cell::new(false));
        let checks = Checks {
            runtime: runtime.clone(),
            host: host.clone(),
            profile: profile.clone(),
            view,
            tab,
            server_port: server.port,
            hits: server.hits.clone(),
            probe,
            twin: RefCell::new(twin),
            twin_xpi,
            widget_id: widget.id.clone(),
            window: window.clone(),
            results: RefCell::new(Vec::new()),
        };
        glib::spawn_future_local({
            let (main_loop, outcome) = (main_loop.clone(), outcome.clone());
            async move {
                let ok = checks.run().await;
                outcome.set(ok);
                main_loop.quit();
            }
        });
        glib::timeout_add_local_once(TIMEOUT * 6, {
            let main_loop = main_loop.clone();
            move || {
                println!("[harness] FAIL: global timeout");
                main_loop.quit();
            }
        });
        main_loop.run();

        let ok = outcome.get();
        println!("[harness] RESULT: {}", if ok { "PASS" } else { "FAIL" });
        window.destroy();
        drop(runtime);
        drop(host);
        let _ = std::fs::remove_dir_all(&out_dir);
        if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
    }

    struct Checks {
        runtime: Runtime,
        host: Rc<Host>,
        profile: Rc<RefCell<Profile>>,
        view: webkit::WebView,
        tab: TabId,
        server_port: u16,
        hits: Arc<Mutex<Vec<String>>>,
        probe: InstalledExtension,
        twin: RefCell<InstalledExtension>,
        twin_xpi: PathBuf,
        widget_id: ExtensionId,
        window: gtk::Window,
        results: RefCell<Vec<(&'static str, bool)>>,
    }

    impl Checks {
        fn note(&self, name: &'static str, ok: bool, detail: impl std::fmt::Display) {
            println!("[harness] {name}: {detail} -> {}", if ok { "ok" } else { "FAIL" });
            self.results.borrow_mut().push((name, ok));
        }

        async fn run(&self) -> bool {
            let started = Instant::now();
            // Filters compile asynchronously; the self-test must navigate only after they
            // are attached, and so does the harness.
            let ready = Rc::new(Cell::new(false));
            let flag = ready.clone();
            self.runtime.on_filters_ready(move || flag.set(true));
            if !wait_until(|| ready.get(), TIMEOUT).await {
                println!("[harness] FAIL: filters never became ready");
                return false;
            }
            println!("[harness] filters ready after {} ms", started.elapsed().as_millis());

            self.view.load_uri(&self.url("/index.html"));

            // 1. content script -> background -> reply (probe), and both twin entries.
            let probe = self.wait_for_js(&self.view, "document.documentElement.dataset.vsesvitProbe", None, |v| v == "background-replied").await;
            let visits = self.eval(&self.view, "document.documentElement.dataset.vsesvitVisits", None).await;
            self.note("content_script", probe.as_deref() == Some("background-replied"), format!("dataset.vsesvitProbe = {probe:?}, visits = {visits:?} after {} ms", started.elapsed().as_millis()));
            let second = self.wait_for_js(&self.view, "document.documentElement.dataset.twinSecond || ''", None, |v| !v.is_empty()).await;
            let first = self.eval(&self.view, "document.documentElement.dataset.twinFirst || ''", None).await;
            self.note("second_content_script", first.as_deref() == Some("1") && second.as_deref() == Some(TWIN_ID), format!("twinFirst = {first:?}, twinSecond = {second:?}"));
            // A `"world": "MAIN"` entry shares the page's globals and gets no extension API.
            let main_world = self.eval(&self.view, "String(window.__twinMain)", None).await;
            self.note("main_world_content_script", main_world.as_deref() == Some("page"), format!("window.__twinMain in the page's world = {main_world:?}"));

            // 2. declarativeNetRequest: control image requested, blocked image never
            glib::timeout_future(Duration::from_millis(1000)).await;
            let hits = self.hits.lock().unwrap().clone();
            let allowed = hits.iter().any(|p| p == "/allowed.png");
            let blocked = hits.iter().any(|p| p == "/vsesvit-blocked/pixel.png");
            self.note("dnr_blocked", allowed && !blocked, format!("server saw {hits:?}"));

            // 3. the probe's action popup and the page APIs in it
            self.probe_popup().await;

            // 4. an http iframe inside a popup (widget: no host permissions)
            self.widget_popup().await;

            // 5. the twin: options page in a tab, scripting, permissions, action paths
            self.twin_popup().await;

            // 6. lifecycle events
            self.lifecycle().await;

            for id in self.runtime.loaded() {
                self.runtime.unload(&id);
            }
            self.note("unload", self.runtime.loaded().is_empty() && self.runtime.actions().is_empty(), format!("loaded() = {:?}, actions() = {:?}", self.runtime.loaded(), self.runtime.actions()));

            let results = self.results.borrow();
            let failed: Vec<&str> = results.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
            println!("[harness] {} checks, failed: {failed:?}", results.len());
            failed.is_empty()
        }

        async fn probe_popup(&self) {
            let actions = self.runtime.actions();
            println!("[harness] actions: {actions:?}");
            let Some(popup) = self.runtime.activate_action(&self.probe.id, Some(self.tab)) else {
                self.note("popup", false, "activate_action returned no popup view");
                return;
            };
            let _window = self.park(&popup);
            let title = wait_for_value(|| popup.title().map(String::from).filter(|t| t.starts_with("visits=")), TIMEOUT).await;
            let n = title.as_deref().and_then(|t| t.strip_prefix("visits=")).and_then(|n| n.parse::<u64>().ok());
            self.note("popup", n.is_some_and(|n| n >= 1), format!("title = {title:?}"));

            let query = self.eval_async(&popup, "return chrome.tabs.query({ active: true });").await;
            let query_ok = query.as_ref().and_then(|v| v.as_array()).is_some_and(|tabs| tabs.len() == 1 && tabs[0]["url"].as_str().is_some_and(|u| u.ends_with("/index.html")));
            self.note("popup_tabs_query", query_ok, format!("chrome.tabs.query({{active:true}}) = {}", query.map(|v| v.to_string()).unwrap_or_default()));
            let badge = self
                .eval_async(&popup, "await chrome.action.setBadgeText({ text: '7' }); return chrome.runtime.getURL('x/y.png') + ' ' + chrome.i18n.getUILanguage();")
                .await;
            let badge_state = self.runtime.actions().iter().find(|a| a.extension == self.probe.id).map(|a| a.badge_text.clone());
            let badge_ok = badge_state.as_deref() == Some("7") && badge.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with(&format!("chrome-extension://{}/x/y.png ", self.probe.id.as_str())));
            self.note("popup_badge_and_get_url", badge_ok, format!("badge_text = {badge_state:?}; getURL/i18n = {badge:?}"));
            let onchanged = self
                .eval_async(
                    &popup,
                    "return await new Promise((resolve) => { chrome.storage.onChanged.addListener((c, area) => resolve({ area, keys: Object.keys(c), nv: c.harness && c.harness.newValue })); chrome.storage.local.set({ harness: 42 }); });",
                )
                .await;
            self.note("popup_storage_onchanged", onchanged.as_ref().is_some_and(|v| v["area"] == "local" && v["nv"] == 42), format!("{onchanged:?}"));
            let no_receiver = self.eval_async(&popup, "try { await chrome.tabs.sendMessage(1, { ping: 1 }); return 'replied'; } catch (e) { return String(e.message); }").await;
            self.note("popup_no_receiver", no_receiver.as_ref().and_then(Value::as_str).is_some_and(|s| s.contains("Receiving end does not exist")), format!("{no_receiver:?}"));
            // The probe has host permissions for everything but not `scripting`.
            let refused = self.eval_async(&popup, "try { await chrome.scripting.executeScript({ target: { tabId: 1 }, func: () => 1 }); return 'ran'; } catch (e) { return String(e.message); }").await;
            self.note("scripting_permission_required", refused.as_ref().and_then(Value::as_str).is_some_and(|s| s.contains("\"scripting\" permission")), format!("{refused:?}"));

            // `<all_urls>` does not reach local files without the user's file-access grant,
            // which Vsesvit does not offer: no content script there, and the URL stays hidden.
            let page = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/site/index.html").canonicalize().expect("fixture page");
            let file_url = format!("file://{}", page.display());
            let local = self.host.create_tab(&file_url, false).expect("file tab");
            let local_view = self.host.web_view(local).expect("file tab view");
            wait_until(|| local_view.title().as_deref() == Some("Vsesvit fixture"), TIMEOUT).await;
            glib::timeout_future(Duration::from_millis(1000)).await;
            let injected = self.eval(&local_view, "String(document.documentElement.dataset.vsesvitProbe)", None).await;
            let listing = self.eval_async(&popup, "return await chrome.tabs.query({});").await;
            let seen = listing.as_ref().and_then(Value::as_array).and_then(|tabs| tabs.iter().find(|t| t["id"] == local.0).cloned()).unwrap_or(Value::Null);
            self.note("no_file_access", injected.as_deref() == Some("undefined") && seen["id"] == local.0 && seen.get("url").is_none(), format!("content script on {file_url}: {injected:?}; tabs.query sees {seen}"));
            self.host.remove_tab(local);
        }

        async fn widget_popup(&self) {
            let before = self.hits.lock().unwrap().iter().filter(|p| *p == "/page2.html").count();
            let Some(popup) = self.runtime.activate_action(&self.widget_id, Some(self.tab)) else {
                self.note("iframe_in_popup", false, "activate_action returned no popup view");
                return;
            };
            let _window = self.park(&popup);
            let framed = wait_until(|| self.hits.lock().unwrap().iter().filter(|p| *p == "/page2.html").count() > before, Duration::from_secs(5)).await;
            glib::timeout_future(Duration::from_millis(300)).await;
            let opened: Vec<String> = self.host.created.borrow().iter().filter(|u| u.contains("/page2.html")).cloned().collect();
            self.note("iframe_in_popup", framed && opened.is_empty(), format!("server got the frame = {framed}; tabs opened for it = {opened:?}"));
        }

        async fn twin_popup(&self) {
            let twin_id = self.twin.borrow().id.clone();
            let Some(popup) = self.runtime.activate_action(&twin_id, Some(self.tab)) else {
                self.note("twin_popup", false, "activate_action returned no popup view");
                return;
            };
            let _window = self.park(&popup);
            let title = wait_for_value(|| popup.title().map(String::from).filter(|t| t.starts_with("twin-popup:")), TIMEOUT).await;
            self.note("twin_popup", title.as_deref() == Some(&format!("twin-popup:{TWIN_ID}")), format!("title = {title:?}"));

            // Options page in a tab.
            let opened = self.eval_async(&popup, "await chrome.runtime.openOptionsPage(); return true;").await;
            let options_tab = wait_for_value(|| self.host.tabs().into_iter().find(|t| t.url.ends_with("/options.html")), TIMEOUT).await;
            let Some(options_tab) = options_tab else {
                self.note("options_page", false, format!("openOptionsPage = {opened:?}; no tab shows options.html: {:?}", self.host.tabs()));
                return;
            };
            let options_view = self.host.web_view(options_tab.id).expect("options tab view");
            let report = self.wait_for_js(&options_view, "JSON.stringify(window.__twinOptions || null)", None, |v| v.contains("\"done\":true")).await;
            let report: Value = report.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or(Value::Null);
            let url_host = options_tab.url.trim_start_matches("chrome-extension://").split('/').next().unwrap_or_default().to_owned();
            let get_url = report["getURL"].as_str().unwrap_or_default();
            let options_ok = report["id"] == TWIN_ID
                && report["storage"] == "visited"
                && report["ping"]["pong"] == true
                && report["ping"]["fromTab"] == true
                && report["data"]["twin"] == true
                && report["relay"]["options"] == "pong"
                && report["current"]["id"] == options_tab.id.0
                && report["host"] == url_host
                && report.get("error").is_none();
            self.note("options_page", options_ok, format!("tab {} at {}; page report = {report}", options_tab.id.0, options_tab.url));
            let hashed = url_host.len() == 32 && url_host.bytes().all(|b| b.is_ascii_hexdigit()) && url_host != TWIN_ID;
            self.note("gecko_id_get_url", hashed && get_url == format!("chrome-extension://{url_host}/data.json"), format!("getURL = {get_url}, URL host = {url_host}"));
            // The extension's CSP applies in a tab as in its own views: no inline script.
            let inline = self.eval(&options_view, "String(window.__twinInline)", None).await;
            self.note("tab_page_csp", inline.as_deref() == Some("undefined"), format!("inline script in the options page ran: window.__twinInline = {inline:?}"));

            // Events reach a tab-hosted page.
            self.runtime.tab_updated(self.tab);
            let updated = self.wait_for_js(&options_view, "JSON.stringify((window.__twinOptions && window.__twinOptions.updated) || [])", None, |v| v.contains(&self.tab.0.to_string())).await;
            self.note("tab_page_events", updated.as_deref().is_some_and(|u| u.contains(&self.tab.0.to_string())), format!("tabs.onUpdated ids seen in the options page = {updated:?}"));

            // runtime.sendMessage reaches every page, not just the first with a listener:
            // the background listens but does not answer this one, the options page does.
            let broadcast = self.eval_async(&popup, "return await chrome.runtime.sendMessage({ type: 'to-options' });").await;
            self.note("send_message_reaches_every_page", broadcast.as_ref().is_some_and(|v| v["options"] == "pong"), format!("popup -> runtime.sendMessage(to-options) = {broadcast:?}"));

            // More tabs: one the twin may not touch, one it may but has no content script in.
            let other = self.host.create_tab("data:text/html,<title>Vsesvit other</title>", false).expect("data tab");
            let plain = self.host.create_tab(&self.url("/page2.html"), false).expect("page2 tab");
            let other_view = self.host.web_view(other).expect("data tab view");
            let plain_view = self.host.web_view(plain).expect("page2 tab view");
            let loaded = wait_until(|| other_view.title().as_deref() == Some("Vsesvit other") && plain_view.title().as_deref() == Some("Vsesvit fixture 2"), TIMEOUT).await;
            println!("[harness] tabs {} (data:) and {} (page2) loaded = {loaded}", other.0, plain.0);
            let no_content_script = self.eval(&plain_view, "String(document.documentElement.dataset.twinSecond)", None).await;
            println!("[harness] page2 has no twin content script: dataset.twinSecond = {no_content_script:?}");

            let func = self.eval_async(&popup, &format!("return await chrome.scripting.executeScript({{ target: {{ tabId: {} }}, func: () => (typeof chrome === 'object' && chrome.runtime) ? chrome.runtime.id : 'no-chrome' }});", plain.0)).await;
            self.note("execute_script_has_chrome", func.as_ref().is_some_and(|v| v[0]["result"] == TWIN_ID), format!("func injection into tab {} = {func:?}", plain.0));
            let files = self.eval_async(&popup, &format!("return await chrome.scripting.executeScript({{ target: {{ tabId: {} }}, files: ['/inject.js'] }});", plain.0)).await;
            self.note("execute_script_files_leading_slash", files.as_ref().is_some_and(|v| v[0]["result"] == format!("injected:{TWIN_ID}")), format!("files injection = {files:?}"));
            let css = self.eval_async(&popup, &format!("try {{ await chrome.scripting.insertCSS({{ target: {{ tabId: {} }}, css: 'body {{ color: red }}' }}); return 'ok'; }} catch (e) {{ return String(e.message); }}", plain.0)).await;
            self.note("insert_css", css.as_ref().and_then(Value::as_str) == Some("ok"), format!("{css:?}"));

            let denied = self.eval_async(&popup, &format!("try {{ await chrome.scripting.executeScript({{ target: {{ tabId: {} }}, func: () => 1 }}); return 'ran'; }} catch (e) {{ return String(e.message); }}", other.0)).await;
            self.note("host_permission_denied", denied.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("Cannot access contents of url")), format!("tab {} = {denied:?}", other.0));
            let listing = self.eval_async(&popup, "return await chrome.tabs.query({});").await;
            let tabs = listing.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
            let entry = |id: TabId| tabs.iter().find(|t| t["id"] == id.0).cloned().unwrap_or(Value::Null);
            let redacted = tabs.len() == 4 && entry(self.tab)["url"].as_str().is_some_and(|u| u.ends_with("/index.html")) && entry(other).get("url").is_none() && entry(other).get("title").is_none() && entry(other)["id"] == other.0;
            self.note("tabs_url_redacted", redacted, format!("tabs.query({{}}) = {listing:?}"));

            // activeTab: the user invokes the action on the data: tab.
            drop(self.runtime.activate_action(&twin_id, Some(other)));
            let granted = self.eval_async(&popup, &format!("try {{ return await chrome.scripting.executeScript({{ target: {{ tabId: {} }}, func: () => document.title }}); }} catch (e) {{ return String(e.message); }}", other.0)).await;
            self.note("active_tab_grant", granted.as_ref().is_some_and(|v| v[0]["result"] == "Vsesvit other"), format!("after activate_action on tab {} = {granted:?}", other.0));

            // Injection follows the committed document: navigating a tab the twin may not
            // touch to one of its own pages does not let a script into the page still shown.
            let third = self.host.create_tab("data:text/html,<title>Vsesvit third</title>", false).expect("third tab");
            let third_view = self.host.web_view(third).expect("third tab view");
            wait_until(|| third_view.title().as_deref() == Some("Vsesvit third"), TIMEOUT).await;
            let pending = self
                .eval_async(
                    &popup,
                    &format!("chrome.tabs.update({0}, {{ url: chrome.runtime.getURL('popup.html') }}); try {{ const r = await chrome.scripting.executeScript({{ target: {{ tabId: {0} }}, world: 'MAIN', func: () => location.href }}); return r[0].result; }} catch (e) {{ return String(e.message); }}", third.0),
                )
                .await;
            let followed = pending.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("Cannot access contents") || s.starts_with("chrome-extension://"));
            self.note("scripting_follows_committed_document", followed, format!("executeScript on tab {} while it loads the twin's page = {pending:?}", third.0));

            // Chrome-valid resource references.
            let paths = self
                .eval_async(&popup, "await chrome.action.setPopup({ popup: chrome.runtime.getURL('popup.html') }); await chrome.action.setIcon({ path: '/icon.png' }); return [await chrome.action.getPopup({}), chrome.runtime.getURL('popup.html')];")
                .await;
            let icon = self.runtime.actions().iter().find(|a| a.extension == twin_id).and_then(|a| a.icon.clone());
            let paths_ok = paths.as_ref().and_then(Value::as_array).is_some_and(|p| p.len() == 2 && p[0] == p[1] && p[0].as_str().is_some_and(|s| s.ends_with("/popup.html") && !s.contains("chrome-extension://chrome-extension")))
                && icon.as_ref().is_some_and(|i| i.ends_with("icon.png"));
            self.note("action_resource_paths", paths_ok, format!("getPopup vs getURL = {paths:?}; icon = {icon:?}"));

            // tabs.create resolves a relative URL against the extension; tabs.update refuses
            // javascript: and file: URLs.
            let navigation = self
                .eval_async(
                    &popup,
                    "const tab = await chrome.tabs.create({ url: 'data.json', active: false }); const refused = []; for (const url of ['javascript:document.title=\"hijacked\"', 'file:///etc/hostname']) { try { await chrome.tabs.update(tab.id, { url }); refused.push('navigated'); } catch (e) { refused.push(String(e.message)); } } return refused;",
                )
                .await;
            let created = self.host.created.borrow().last().cloned().unwrap_or_default();
            let refused = navigation.as_ref().and_then(Value::as_array).is_some_and(|r| r.len() == 2 && r.iter().all(|m| m.as_str().is_some_and(|m| m != "navigated")));
            self.note("tabs_url_resolved_and_gated", created.starts_with("chrome-extension://") && created.ends_with("/data.json") && refused, format!("created {created:?}; javascript:/file: updates = {navigation:?}"));

            // A web page cannot drive the options page (not web-accessible) through its URL.
            let lure = self.host.create_tab(&self.url("/page2.html"), false).expect("lure tab");
            let lure_view = self.host.web_view(lure).expect("lure tab view");
            wait_until(|| lure_view.title().as_deref() == Some("Vsesvit fixture 2"), TIMEOUT).await;
            let target = format!("chrome-extension://{url_host}/options.html?from=web");
            self.eval(&lure_view, &format!("location.href = {}; 'navigating'", Value::String(target.clone())), None).await;
            glib::timeout_future(Duration::from_millis(1500)).await;
            let ran = self.eval(&lure_view, "String(window.__twinOptions && window.__twinOptions.id)", None).await;
            self.note("web_page_cannot_open_extension_page", ran.is_some() && ran.as_deref() != Some(TWIN_ID), format!("options.js in tab {} after a web page navigated it to {target} = {ran:?}", lure.0));
            // Nor by sending no Referer, which the page's own referrer policy decides, and not
            // when the user then reloads the tab, which the browser starts.
            let quiet = self.host.create_tab(&self.url("/page2.html"), false).expect("no-referrer lure tab");
            let quiet_view = self.host.web_view(quiet).expect("no-referrer lure tab view");
            wait_until(|| quiet_view.title().as_deref() == Some("Vsesvit fixture 2"), TIMEOUT).await;
            let lure_script = format!("document.head.insertAdjacentHTML('beforeend', '<meta name=\"referrer\" content=\"no-referrer\">'); location.href = {}; 'navigating'", Value::String(target.clone()));
            self.eval(&quiet_view, &lure_script, None).await;
            glib::timeout_future(Duration::from_millis(1500)).await;
            quiet_view.reload();
            glib::timeout_future(Duration::from_millis(1500)).await;
            let ran = self.eval(&quiet_view, "String(window.__twinOptions && window.__twinOptions.id)", None).await;
            self.note("web_page_without_referrer_cannot_open_extension_page", ran.is_some() && ran.as_deref() != Some(TWIN_ID), format!("options.js in tab {} after a no-referrer page navigated it to {target} and it reloaded = {ran:?}", quiet.0));
            // The browser's own navigations still reach the extension's pages: back to the
            // options page from a site it linked to.
            let back = self.host.create_tab(&format!("chrome-extension://{url_host}/options.html"), false).expect("options tab");
            let back_view = self.host.web_view(back).expect("options tab view");
            let options_title = format!("options:{TWIN_ID}");
            wait_until(|| back_view.title().as_deref() == Some(options_title.as_str()), TIMEOUT).await;
            self.eval(&back_view, &format!("location.href = {}; 'leaving'", Value::String(self.url("/page2.html"))), None).await;
            wait_until(|| back_view.title().as_deref() == Some("Vsesvit fixture 2"), TIMEOUT).await;
            back_view.go_back();
            let returned = wait_until(|| back_view.title().as_deref() == Some(options_title.as_str()), TIMEOUT).await;
            self.note("browser_navigates_back_to_extension_page", returned, format!("tab {} title after going back = {:?}", back.0, back_view.title()));
        }

        async fn lifecycle(&self) {
            let id = self.twin.borrow().id.clone();
            let lives = wait_for_value(|| {
                let lives = self.twin_lives();
                lives.iter().any(|l| l.iter().any(|e| e == "installed:install")).then_some(lives)
            }, TIMEOUT).await;
            self.note("first_load_fires_installed", lives.as_ref().is_some_and(|l| l.len() == 1), format!("background lives = {lives:?}"));

            self.runtime.unload(&id);
            if let Err(e) = self.runtime.load_with(&self.twin.borrow(), LoadReason::Enable) {
                self.note("enable_fires_nothing", false, format!("load_with(Enable): {e}"));
                return;
            }
            let lives = wait_for_value(|| {
                let lives = self.twin_lives();
                (lives.len() == 2).then_some(lives)
            }, TIMEOUT).await;
            glib::timeout_future(Duration::from_millis(700)).await;
            let lives = if lives.is_some() { Some(self.twin_lives()) } else { None };
            let enable_ok = lives.as_ref().is_some_and(|l| l.len() == 2 && l.iter().any(|life| life == &["alive".to_owned()]));
            self.note("enable_fires_nothing", enable_ok, format!("background lives after load_with(Enable) = {lives:?}"));

            self.runtime.unload(&id);
            if let Err(e) = self.profile.borrow_mut().extensions().uninstall(&id) {
                self.note("reinstall_fires_installed", false, format!("core uninstall: {e}"));
                return;
            }
            let wiped = self.twin_lives();
            let reinstalled = install(&self.profile, &self.twin_xpi);
            if let Err(e) = self.runtime.load(&reinstalled) {
                self.note("reinstall_fires_installed", false, format!("load after reinstall: {e}"));
                return;
            }
            *self.twin.borrow_mut() = reinstalled;
            let lives = wait_for_value(|| {
                let lives = self.twin_lives();
                lives.iter().any(|l| l.iter().any(|e| e == "installed:install" || e == "startup")).then_some(lives)
            }, TIMEOUT).await;
            let reinstall_ok = wiped.is_empty() && lives.as_ref().is_some_and(|l| l.len() == 1 && l[0].iter().any(|e| e == "installed:install"));
            self.note("reinstall_fires_installed", reinstall_ok, format!("storage after uninstall = {wiped:?}; lives after reinstall = {lives:?}"));
        }

        /// The event log of every life of the twin's background page, from its
        /// `storage.local` (`life:<random>` -> [events]).
        fn twin_lives(&self) -> Vec<Vec<String>> {
            let id = self.twin.borrow().id.clone();
            let mut profile = self.profile.borrow_mut();
            let items = profile.ext_storage().get(&id, Area::Local, None).unwrap_or_default();
            items
                .iter()
                .filter(|(k, _)| k.starts_with("life:"))
                .map(|(_, v)| v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect())
                .collect()
        }

        fn url(&self, path: &str) -> String {
            format!("http://127.0.0.1:{}{path}", self.server_port)
        }

        /// A window for a popup view (never presented), so it renders like the shell's popover.
        fn park(&self, popup: &webkit::WebView) -> gtk::Window {
            let window = gtk::Window::new();
            window.set_transient_for(Some(&self.window));
            window.set_child(Some(popup));
            window
        }

        async fn eval(&self, view: &webkit::WebView, script: &str, world: Option<&str>) -> Option<String> {
            match view.evaluate_javascript_future(script, world, None).await {
                Ok(v) if v.is_string() => Some(v.to_str().to_string()),
                Ok(v) => v.to_json(0).map(|j| j.to_string()),
                Err(e) => {
                    println!("[harness] eval error: {e}");
                    None
                }
            }
        }

        async fn eval_async(&self, view: &webkit::WebView, body: &str) -> Option<Value> {
            match view.call_async_javascript_function_future(body, None, None, None).await {
                Ok(v) => v.to_json(0).and_then(|j| serde_json::from_str(&j).ok()),
                Err(e) => {
                    println!("[harness] eval_async error: {e}");
                    None
                }
            }
        }

        async fn wait_for_js(&self, view: &webkit::WebView, script: &str, world: Option<&str>, done: impl Fn(&str) -> bool) -> Option<String> {
            let deadline = Instant::now() + TIMEOUT;
            let mut last = None;
            while Instant::now() < deadline {
                last = self.eval(view, script, world).await;
                if last.as_deref().is_some_and(&done) {
                    break;
                }
                glib::timeout_future(Duration::from_millis(100)).await;
            }
            last
        }
    }

    async fn wait_until(cond: impl Fn() -> bool, timeout: Duration) -> bool {
        wait_for_value(|| cond().then_some(()), timeout).await.is_some()
    }

    async fn wait_for_value<T>(probe: impl Fn() -> Option<T>, timeout: Duration) -> Option<T> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(v) = probe() {
                return Some(v);
            }
            if Instant::now() >= deadline {
                return None;
            }
            glib::timeout_future(Duration::from_millis(50)).await;
        }
    }

    struct StdoutLog;
    static STDOUT_LOG: StdoutLog = StdoutLog;

    impl log::Log for StdoutLog {
        fn enabled(&self, m: &log::Metadata) -> bool {
            m.target().starts_with("vsesvit")
        }
        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                println!("[log] {} {}: {}", record.level(), record.target(), record.args());
            }
        }
        fn flush(&self) {}
    }

    // --- the shell side: one window, tabs stacked in a box ---------------------------------

    struct Tab {
        id: TabId,
        view: webkit::WebView,
        /// The document on screen, as the GTK shell reports it: set on commit, never the
        /// URL still loading.
        committed: Rc<RefCell<String>>,
    }

    /// Builds every tab the way the GTK shell does: a WebView on the runtime's
    /// `UserContentManager` for that tab id.
    struct Host {
        session: webkit::NetworkSession,
        container: gtk::Box,
        tabs: RefCell<Vec<Tab>>,
        runtime: RefCell<Option<Runtime>>,
        /// Every URL `create_tab` was asked to open, for checks that expect none.
        created: RefCell<Vec<String>>,
        next_id: Cell<u32>,
    }

    impl Host {
        fn new(session: webkit::NetworkSession, container: gtk::Box) -> Host {
            Host { session, container, tabs: RefCell::new(Vec::new()), runtime: RefCell::new(None), created: RefCell::new(Vec::new()), next_id: Cell::new(1) }
        }
    }

    impl TabHost for Host {
        fn tabs(&self) -> Vec<TabInfo> {
            self.tabs
                .borrow()
                .iter()
                .enumerate()
                .map(|(i, t)| TabInfo {
                    id: t.id,
                    window_id: 1,
                    index: i as u32,
                    url: t.committed.borrow().clone(),
                    title: t.view.title().map(String::from).unwrap_or_default(),
                    active: i == 0,
                })
                .collect()
        }

        fn create_tab(&self, url: &str, _active: bool) -> Option<TabId> {
            let runtime = self.runtime.borrow().clone()?;
            let id = TabId(self.next_id.get());
            self.next_id.set(id.0 + 1);
            let view = webkit::WebView::builder().network_session(&self.session).user_content_manager(&runtime.user_content_manager(id)).build();
            self.container.append(&view);
            // The navigation gate every shell installs (lib.rs).
            view.connect_decide_policy(move |view, decision, kind| {
                if !matches!(kind, webkit::PolicyDecisionType::NavigationAction | webkit::PolicyDecisionType::NewWindowAction) {
                    return false;
                }
                let target = decision.downcast_ref::<webkit::NavigationPolicyDecision>().and_then(|d| d.navigation_action()).and_then(|a| a.request()).and_then(|r| r.uri());
                let source = view.uri().map(String::from).unwrap_or_default();
                if target.is_none_or(|target| runtime.may_navigate(&source, &target)) {
                    return false;
                }
                println!("[harness] host: refused a navigation from {source}");
                decision.ignore();
                true
            });
            let committed = Rc::new(RefCell::new(String::new()));
            view.connect_load_changed({
                let committed = committed.clone();
                move |view, event| {
                    if event == webkit::LoadEvent::Committed {
                        *committed.borrow_mut() = view.uri().map(String::from).unwrap_or_default();
                    }
                }
            });
            self.tabs.borrow_mut().push(Tab { id, view: view.clone(), committed });
            self.created.borrow_mut().push(url.to_owned());
            println!("[harness] host: create_tab({url}) -> tab {}", id.0);
            view.load_uri(url);
            Some(id)
        }

        fn update_tab(&self, tab: TabId, url: Option<&str>, _active: Option<bool>) -> bool {
            let tabs = self.tabs.borrow();
            let Some(t) = tabs.iter().find(|t| t.id == tab) else { return false };
            if let Some(u) = url {
                t.view.load_uri(u);
            }
            true
        }

        fn remove_tab(&self, tab: TabId) -> bool {
            let removed = {
                let mut tabs = self.tabs.borrow_mut();
                let Some(i) = tabs.iter().position(|t| t.id == tab) else { return false };
                tabs.remove(i)
            };
            self.container.remove(&removed.view);
            if let Some(runtime) = self.runtime.borrow().clone() {
                runtime.tab_closed(tab);
            }
            true
        }

        fn web_view(&self, tab: TabId) -> Option<webkit::WebView> {
            self.tabs.borrow().iter().find(|t| t.id == tab).map(|t| t.view.clone())
        }
    }

    // --- fixtures ------------------------------------------------------------------------

    /// Installs through the real pipeline: parse the source, verify, unpack, commit.
    fn install(profile: &Rc<RefCell<Profile>>, path: &Path) -> InstalledExtension {
        let source = InstallSource::from_path(path).expect("install source");
        let job = profile.borrow_mut().extensions().prepare_install(source).expect("prepare install");
        let staged = job.run(&mut |_| {}).unwrap_or_else(|e| panic!("install {}: {e}", path.display()));
        profile.borrow_mut().extensions().commit(staged).expect("commit").expect("installed")
    }

    fn write_xpi(path: &Path, files: &[(&str, String)]) {
        let borrowed: Vec<(&str, &[u8])> = files.iter().map(|(n, c)| (*n, c.as_bytes())).collect();
        std::fs::write(path, vsesvit_core::testkit::zip_files(&borrowed)).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }

    /// An MV3 add-on with a Gecko id. Its host permission covers the fixture server only,
    /// its content scripts match `/index.html` only, and its background (a classic service
    /// worker that imports its helpers with `importScripts`) logs lifecycle events per life
    /// into `storage.local`.
    fn twin_files() -> Vec<(&'static str, String)> {
        vec![
            (
                "manifest.json",
                serde_json::json!({
                    "manifest_version": 3,
                    "name": "Vsesvit Twin",
                    "version": "1.0.0",
                    "browser_specific_settings": { "gecko": { "id": TWIN_ID } },
                    "permissions": ["storage", "scripting", "activeTab"],
                    "host_permissions": ["http://127.0.0.1/*"],
                    "background": { "service_worker": "background.js" },
                    "content_scripts": [
                        { "matches": ["http://127.0.0.1/index.html"], "js": ["first.js"], "run_at": "document_start" },
                        { "matches": ["http://127.0.0.1/index.html"], "js": ["second.js"], "run_at": "document_end" },
                        { "matches": ["http://127.0.0.1/index.html"], "js": ["main.js"], "run_at": "document_start", "world": "MAIN" }
                    ],
                    "options_page": "options.html",
                    "action": { "default_title": "Vsesvit Twin", "default_popup": "popup.html" }
                })
                .to_string(),
            ),
            ("first.js", "document.documentElement.dataset.twinFirst = \"1\";\n".to_owned()),
            (
                "second.js",
                "document.documentElement.dataset.twinSecond = (typeof chrome === \"object\" && chrome.runtime) ? chrome.runtime.id : \"no-chrome\";\n".to_owned(),
            ),
            ("main.js", "window.__twinMain = (typeof chrome === \"object\" && chrome.runtime && chrome.runtime.id) ? \"api\" : \"page\";\n".to_owned()),
            (
                "background.js",
                r#"importScripts("lib/life.js");
const log = (e) => { events.push(e); return chrome.storage.local.set({ [life]: events.slice() }); };
chrome.runtime.onInstalled.addListener((d) => log("installed:" + d.reason));
chrome.runtime.onStartup.addListener(() => log("startup"));
chrome.runtime.onMessage.addListener((m, sender, respond) => {
  if (m && m.type === "ping") { respond({ pong: true, fromTab: !!sender.tab, url: sender.url }); return false; }
  if (m && m.type === "relay") { chrome.runtime.sendMessage({ type: "to-options" }).then(respond, (e) => respond({ error: String(e) })); return true; }
  return false;
});
log("alive");
"#
                .to_owned(),
            ),
            ("lib/life.js", "importScripts('lib/events.js');\nself.life = \"life:\" + Math.random().toString(36).slice(2);\n".to_owned()),
            ("lib/events.js", "self.events = [];\n".to_owned()),
            ("popup.html", "<!doctype html><html><head><meta charset=\"utf-8\"><title>twin</title></head><body><script src=\"popup.js\"></script></body></html>".to_owned()),
            ("popup.js", "document.title = \"twin-popup:\" + chrome.runtime.id;\n".to_owned()),
            ("options.html", "<!doctype html><html><head><meta charset=\"utf-8\"><title>Twin options</title></head><body><script>window.__twinInline = true;</script><script src=\"options.js\"></script></body></html>".to_owned()),
            (
                "options.js",
                r#"(async () => {
  const out = { id: chrome.runtime.id, host: location.host, origin: location.origin, updated: [] };
  window.__twinOptions = out;
  chrome.tabs.onUpdated.addListener((tabId) => { out.updated.push(tabId); });
  chrome.runtime.onMessage.addListener((m, sender, respond) => { if (m && m.type === "to-options") { respond({ options: "pong" }); } return false; });
  try {
    await chrome.storage.local.set({ options: "visited" });
    out.storage = (await chrome.storage.local.get("options")).options;
    out.ping = await chrome.runtime.sendMessage({ type: "ping" });
    out.relay = await chrome.runtime.sendMessage({ type: "relay" });
    out.getURL = chrome.runtime.getURL("data.json");
    const res = await fetch(out.getURL);
    out.data = res.ok ? await res.json() : "status " + res.status;
    out.current = await chrome.tabs.getCurrent();
  } catch (e) { out.error = String(e); }
  out.done = true;
  document.title = "options:" + out.id;
})();
"#
                .to_owned(),
            ),
            ("data.json", "{\"twin\":true}".to_owned()),
            (
                "inject.js",
                "document.documentElement.dataset.twinInjected = (typeof chrome === \"object\" && chrome.runtime) ? chrome.runtime.id : \"no-chrome\";\n\"injected:\" + document.documentElement.dataset.twinInjected;\n".to_owned(),
            ),
        ]
    }

    /// A popup that frames a fixture page. No host permissions, so the frame may load.
    fn widget_files(port: u16) -> Vec<(&'static str, String)> {
        vec![
            (
                "manifest.json",
                serde_json::json!({
                    "manifest_version": 3,
                    "name": "Vsesvit Widget",
                    "version": "1.0.0",
                    "action": { "default_title": "Vsesvit Widget", "default_popup": "popup.html" }
                })
                .to_string(),
            ),
            (
                "popup.html",
                format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>widget</title></head><body><iframe src=\"http://127.0.0.1:{port}/page2.html\"></iframe></body></html>"),
            ),
        ]
    }

    // --- fixture HTTP server --------------------------------------------------------------

    struct FixtureServer {
        port: u16,
        hits: Arc<Mutex<Vec<String>>>,
    }

    impl FixtureServer {
        fn start(dir: PathBuf) -> FixtureServer {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
            let port = listener.local_addr().expect("local addr").port();
            let hits = Arc::new(Mutex::new(Vec::new()));
            let hits_for_thread = hits.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let (dir, hits) = (dir.clone(), hits_for_thread.clone());
                    std::thread::spawn(move || serve(stream, &dir, &hits));
                }
            });
            FixtureServer { port, hits }
        }
    }

    fn serve(mut stream: TcpStream, dir: &Path, hits: &Mutex<Vec<String>>) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok() && line != "\r\n" && !line.is_empty() {
            line.clear();
        }
        let path = request_line.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/").to_owned();
        hits.lock().unwrap().push(path.clone());
        let safe = path.trim_start_matches('/');
        let file = if safe.split('/').any(|seg| seg == "..") { None } else { Some(dir.join(safe)) };
        let (status, body, mime) = match file.filter(|f| f.is_file()).and_then(|f| std::fs::read(&f).ok()) {
            Some(bytes) => ("200 OK", bytes, vsesvit_webext::mime::for_path(&path)),
            None => ("404 Not Found", b"not found".to_vec(), "text/plain"),
        };
        let head = format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    }
}
