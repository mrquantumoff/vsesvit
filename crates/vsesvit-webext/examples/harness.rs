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
//!    reaches no `file:` page (no content script, no tab URL, not in `permissions.contains`);
//! 6. the widget popup's iframe loads in place instead of being blanked and opened as a tab;
//! 7. the twin popup answers `permissions.contains` by pattern coverage and Chrome's
//!    predefined `@@` messages, substitutes `getMessage` placeholders in one pass, and
//!    opens the options page in a tab, where `chrome.*` works
//!    (storage, `runtime.getURL` on the hashed host, messaging both ways, `tabs.getCurrent`,
//!    `tabs.onUpdated`) and its CSP holds; `runtime.sendMessage` reaches every page;
//!    `scripting.executeScript` injects the content-script API into a tab without a
//!    manifest content script, accepts `/`-prefixed files, is refused for a tab outside the
//!    host permissions until `activeTab` grants it and for one still showing such a page while it loads another;
//!    `tabs.query` hides that tab's URL; `action.setPopup(getURL(..))` and `setIcon('/..')`
//!    resolve; `tabs.create` resolves relative URLs and `tabs.update` refuses
//!    `javascript:` and `file:` and without a tab id updates the active tab; a web page cannot
//!    navigate a tab to the options page, with or without a Referer, nor get it by a reload,
//!    nor a site by redirecting a load the browser started there, nor a page through a window
//!    it opened at the twin's web-accessible page, while going back to it still works;
//! 8. ports (`tests/fixtures/extensions/ports/`, with `ports-friend/` beside it): a content
//!    script's `runtime.connect` reaches the background and back, `tabs.connect` reaches a
//!    content script, a connection nobody takes ends with Chrome's error, a closed popup
//!    and a tab that navigates away disconnect their ports, a disconnected port refuses to
//!    post, another extension messages and connects where `externally_connectable` lets
//!    it and not elsewhere, and `getBackgroundPage` answers in the background page;
//! 9. context menus (`tests/fixtures/extensions/menus/`, a service worker, and the *classic*,
//!    an MV2 extension without a background): the page menu shows an extension's several
//!    items under its name and a lone one on its own, by context, `documentUrlPatterns` (a
//!    frame's own URL) and `targetUrlPatterns`, with `%s` as the selection; the worker cannot
//!    reuse an id or leave one out; a click flips a checkbox and fires `onClicked` with the
//!    click and the tab, from the page menu and the action's; `update` and `remove` work from
//!    a popup; the items outlive a restart of the worker's extension until `removeAll`; the
//!    classic's popup makes an item with a generated id whose `onclick` it runs;
//! 10. keyboard commands (`tests/fixtures/extensions/commands/`, a service worker):
//!     `commands.getAll` lists every command, the action's too, from the background and a
//!     popup, with the shortcuts core resolves (none for a key the browser holds or for a
//!     command without one), and the new one after the user assigns it; a shortcut fires
//!     `onCommand` with the name and the tab, whose URL `activeTab` now shows; an extension
//!     without `commands` in its manifest has no `chrome.commands`;
//! 11. notifications (`tests/fixtures/extensions/notifications/`, a service worker): a popup
//!     creates notifications with an id and without one (a UUID), lists them with `getAll`,
//!     and the runtime shows each as Chrome does on the Linux portal (title, body, two
//!     buttons at most, priority, the icon as PNG scaled down to 128 pixels); an update
//!     merges and answers whether there was one; every refusal has Chrome's message; a
//!     click and a button reach the background's listeners, a button the notification lacks
//!     does nothing; `clear` fires `onClosed`; the user's switch in core turns the API off
//!     and back on, closing the notifications; an extension without the permission has no
//!     `chrome.notifications`;
//! 12. lifecycle: the first load fires `onInstalled(install)`, a re-enable fires nothing,
//!     `runtime.reload()` from a page restarts the background and drops its alarms, and an
//!     uninstall followed by a reinstall fires `onInstalled(install)` again.
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
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use serde_json::Value;
    use vsesvit_core::ext_storage::Area;
    use vsesvit_core::extensions::{ExtensionId, InstallSource, InstalledExtension};
    use vsesvit_core::shortcuts::Chord;
    use vsesvit_core::testkit::FixtureServer;
    use vsesvit_core::{OpenOptions, Profile};
    use vsesvit_webext::menus::{Entry, ItemId, Target};
    use vsesvit_webext::notifications::{Activation, Priority, Shown};
    use vsesvit_webext::{Gate, LoadReason, Runtime, TabHost, TabId, TabInfo};
    use webkit::prelude::*;
    use webkit::{gio, glib};

    const TIMEOUT: Duration = Duration::from_secs(20);
    const TWIN_ID: &str = "twin@vsesvit.test";
    const PORTS_ID: &str = "ports@vsesvit.test";
    const FRIEND_ID: &str = "friend@vsesvit.test";
    const MENUS_ID: &str = "menus@vsesvit.test";
    const COMMANDS_ID: &str = "commands@vsesvit.test";
    const NOTIFICATIONS_ID: &str = "notifications@vsesvit.test";
    const DNR_ID: &str = "dnr@vsesvit.test";

    pub fn main() -> ExitCode {
        let show = std::env::args().any(|a| a == "--show");
        let _ = log::set_logger(&STDOUT_LOG).map(|()| log::set_max_level(log::LevelFilter::Debug));
        // WSLg has no working DMA-BUF path for WebKit's compositor; harmless elsewhere.
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            // SAFETY: called before any other thread exists.
            unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
        }
        gtk::init().expect("gtk::init");

        let server = FixtureServer::start().expect("fixture server");
        println!("[harness] fixture server on 127.0.0.1:{}", server.port());

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
        *host.me.borrow_mut() = Rc::downgrade(&host);
        let runtime = Runtime::new(profile.clone(), &session, host.clone());
        *host.runtime.borrow_mut() = Some(runtime.clone());

        let probe_crx = out_dir.join("probe.crx");
        std::fs::write(&probe_crx, vsesvit_core::testkit::probe_crx()).expect("write probe.crx");
        let twin_xpi = out_dir.join("twin.xpi");
        write_xpi(&twin_xpi, &twin_files());
        let widget_xpi = out_dir.join("widget.xpi");
        write_xpi(&widget_xpi, &widget_files(server.port()));
        let ports_xpi = out_dir.join("ports.xpi");
        write_xpi(&ports_xpi, &fixture_files("ports"));
        let friend_xpi = out_dir.join("friend.xpi");
        write_xpi(&friend_xpi, &fixture_files("ports-friend"));
        let menus_xpi = out_dir.join("menus.xpi");
        write_xpi(&menus_xpi, &fixture_files("menus"));
        let classic_xpi = out_dir.join("classic.xpi");
        write_xpi(&classic_xpi, &classic_files());
        let commands_xpi = out_dir.join("commands.xpi");
        write_xpi(&commands_xpi, &fixture_files("commands"));
        let notifications_xpi = out_dir.join("notifications.xpi");
        write_xpi(&notifications_xpi, &fixture_files("notifications"));
        let dnr_xpi = out_dir.join("dnr.xpi");
        write_xpi(&dnr_xpi, &fixture_files("dnr"));

        let probe = install(&profile, &probe_crx);
        assert_eq!(probe.id.as_str(), vsesvit_core::testkit::PROBE_ID);
        let twin = install(&profile, &twin_xpi);
        assert_eq!(twin.id.as_str(), TWIN_ID);
        let widget = install(&profile, &widget_xpi);
        let ports = install(&profile, &ports_xpi);
        let friend = install(&profile, &friend_xpi);
        let menus = install(&profile, &menus_xpi);
        assert_eq!(menus.id.as_str(), MENUS_ID);
        let classic = install(&profile, &classic_xpi);
        let commands = install(&profile, &commands_xpi);
        assert_eq!(commands.id.as_str(), COMMANDS_ID);
        let notifications = install(&profile, &notifications_xpi);
        assert_eq!(notifications.id.as_str(), NOTIFICATIONS_ID);
        let dnr = install(&profile, &dnr_xpi);
        assert_eq!(dnr.id.as_str(), DNR_ID);
        for ext in [&probe, &twin, &widget, &ports, &friend, &menus, &classic, &commands, &notifications, &dnr] {
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
            server,
            probe,
            twin: RefCell::new(twin),
            twin_xpi,
            widget_id: widget.id.clone(),
            menus,
            classic_id: classic.id.clone(),
            commands_id: commands.id.clone(),
            notifications_id: notifications.id.clone(),
            dnr,
            out_dir: out_dir.clone(),
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
        server: FixtureServer,
        probe: InstalledExtension,
        twin: RefCell<InstalledExtension>,
        twin_xpi: PathBuf,
        widget_id: ExtensionId,
        menus: InstalledExtension,
        classic_id: ExtensionId,
        commands_id: ExtensionId,
        notifications_id: ExtensionId,
        dnr: InstalledExtension,
        out_dir: PathBuf,
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
            let hits = self.server.hits();
            let allowed = hits.iter().any(|p| p == "/allowed.png");
            let blocked = hits.iter().any(|p| p == "/vsesvit-blocked/pixel.png");
            self.note("dnr_blocked", allowed && !blocked, format!("server saw {hits:?}"));

            // 3. the probe's action popup and the page APIs in it
            self.probe_popup().await;

            // 4. an http iframe inside a popup (widget: no host permissions)
            self.widget_popup().await;

            // 5. the twin: options page in a tab, scripting, permissions, action paths
            self.twin_popup().await;

            // 6. ports and messages between extensions
            self.ports().await;

            // 7. context menus
            self.menus().await;

            // 8. keyboard commands
            self.commands().await;

            // 9. notifications
            self.notifications().await;

            // 10. declarativeNetRequest dynamic and session rules
            self.declarative_net_request().await;

            // 11. lifecycle events
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
            let Some(popup) = self.popup(&self.probe.id, self.tab).await else {
                self.note("popup", false, "no popup view");
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
            let contains = self.eval_async(&popup, "return [await chrome.permissions.contains({ origins: ['https://example.com/*'] }), await chrome.permissions.contains({ origins: ['file:///*'] })];").await;
            self.note("all_urls_contains", contains == Some(serde_json::json!([true, false])), format!("permissions.contains(https://example.com/*, file:///*) under <all_urls> = {contains:?}"));
            self.host.remove_tab(local);
        }

        async fn widget_popup(&self) {
            let before = self.server.hits().iter().filter(|p| *p == "/page2.html").count();
            let Some(popup) = self.popup(&self.widget_id, self.tab).await else {
                self.note("iframe_in_popup", false, "no popup view");
                return;
            };
            let _window = self.park(&popup);
            let framed = wait_until(|| self.server.hits().iter().filter(|p| *p == "/page2.html").count() > before, Duration::from_secs(5)).await;
            glib::timeout_future(Duration::from_millis(300)).await;
            let opened: Vec<String> = self.host.created.borrow().iter().filter(|u| u.contains("/page2.html")).cloned().collect();
            self.note("iframe_in_popup", framed && opened.is_empty(), format!("server got the frame = {framed}; tabs opened for it = {opened:?}"));
        }

        async fn twin_popup(&self) {
            let twin_id = self.twin.borrow().id.clone();
            let Some(popup) = self.popup(&twin_id, self.tab).await else {
                self.note("twin_popup", false, "no popup view");
                return;
            };
            let _window = self.park(&popup);
            let title = wait_for_value(|| popup.title().map(String::from).filter(|t| t.starts_with("twin-popup:")), TIMEOUT).await;
            self.note("twin_popup", title.as_deref() == Some(&format!("twin-popup:{TWIN_ID}")), format!("title = {title:?}"));
            // Substituted values are inserted as they are, `$` signs included.
            let cost = self.eval_async(&popup, "return chrome.i18n.getMessage('cost', ['$5.00 $$']);").await;
            self.note("i18n_substitution_single_pass", cost.as_ref().and_then(Value::as_str) == Some("Total: $5.00 $$ $5.00 $$"), format!("getMessage(cost, ['$5.00 $$']) = {cost:?}"));
            // permissions.contains and request answer by pattern coverage, as in Chrome.
            let contains = self
                .eval_async(&popup, "const ask = (o) => chrome.permissions.contains({ origins: [o] }); return [await ask('http://127.0.0.1/foo/*'), await ask('http://127.0.0.1:8080/*'), await ask('http://127.0.0.2/*'), await ask('*://127.0.0.1/*'), await chrome.permissions.request({ origins: ['http://127.0.0.1/a*'] })];")
                .await;
            self.note("permissions_contains_patterns", contains == Some(serde_json::json!([true, true, false, false, true])), format!("contains/request under http://127.0.0.1/* = {contains:?}"));

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
            let predefined = self
                .eval_async(&popup, "return ['@@extension_id', '@@ui_locale', '@@bidi_dir', '@@bidi_reversed_dir', '@@bidi_start_edge', '@@bidi_end_edge'].map((m) => chrome.i18n.getMessage(m));")
                .await;
            let p = |i: usize| predefined.as_ref().and_then(|v| v[i].as_str()).unwrap_or_default().to_owned();
            let ltr = [p(2), p(3), p(4), p(5)] == ["ltr", "rtl", "left", "right"] || [p(2), p(3), p(4), p(5)] == ["rtl", "ltr", "right", "left"];
            self.note("i18n_predefined_messages", p(0) == url_host && !p(1).is_empty() && ltr, format!("@@ messages = {predefined:?}, URL host = {url_host}"));
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
            drop(self.popup(&twin_id, other).await);
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
            let defaulted = self.eval_async(&popup, "const [active] = await chrome.tabs.query({ active: true }); const updated = await chrome.tabs.update({}); return [active.id, updated.id];").await;
            let to_active = defaulted.as_ref().and_then(Value::as_array).is_some_and(|ids| ids.len() == 2 && ids[0].is_number() && ids[0] == ids[1]);
            self.note("tabs_update_defaults_to_the_active_tab", to_active, format!("[active tab, tabs.update({{}}) tab] = {defaulted:?}"));

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
            self.host.reload(quiet);
            glib::timeout_future(Duration::from_millis(1500)).await;
            let ran = self.eval(&quiet_view, "String(window.__twinOptions && window.__twinOptions.id)", None).await;
            self.note("web_page_without_referrer_cannot_open_extension_page", ran.is_some() && ran.as_deref() != Some(TWIN_ID), format!("options.js in tab {} after a no-referrer page navigated it to {target} and it reloaded = {ran:?}", quiet.0));
            // Nor a site that redirects there, even in a load the browser started.
            let bounce = bounce_to(format!("chrome-extension://{url_host}/options.html?bounced"));
            let bounced = self.host.create_tab(&bounce, false).expect("bounced tab");
            let bounced_view = self.host.web_view(bounced).expect("bounced tab view");
            glib::timeout_future(Duration::from_millis(1500)).await;
            let title = bounced_view.title().map(String::from);
            self.note("redirect_cannot_open_extension_page", title.as_deref() != Some(format!("options:{TWIN_ID}").as_str()), format!("tab {} title after {bounce} redirected it to the options page = {title:?}", bounced.0));
            // The browser's own navigations still reach the extension's pages: back to the
            // options page from a site it linked to.
            let back = self.host.create_tab(&format!("chrome-extension://{url_host}/options.html"), false).expect("options tab");
            let back_view = self.host.web_view(back).expect("options tab view");
            let options_title = format!("options:{TWIN_ID}");
            wait_until(|| back_view.title().as_deref() == Some(options_title.as_str()), TIMEOUT).await;
            self.eval(&back_view, &format!("location.href = {}; 'leaving'", Value::String(self.url("/page2.html"))), None).await;
            wait_until(|| back_view.title().as_deref() == Some("Vsesvit fixture 2"), TIMEOUT).await;
            self.host.go_back(back);
            let returned = wait_until(|| back_view.title().as_deref() == Some(options_title.as_str()), TIMEOUT).await;
            self.note("browser_navigates_back_to_extension_page", returned, format!("tab {} title after going back = {:?}", back.0, back_view.title()));
            // Nor through a window a web page opened at the twin's web-accessible page and keeps.
            let opener = self.host.create_tab(&self.url("/page2.html"), false).expect("opener tab");
            let opener_view = self.host.web_view(opener).expect("opener tab view");
            if let Some(settings) = WebViewExt::settings(&opener_view) {
                settings.set_javascript_can_open_windows_automatically(true);
            }
            wait_until(|| opener_view.title().as_deref() == Some("Vsesvit fixture 2"), TIMEOUT).await;
            let public = format!("chrome-extension://{url_host}/public.html");
            self.eval(&opener_view, &format!("window.held = window.open({}, 'held'); 0", Value::String(public.clone())), None).await;
            let held = wait_for_value(|| self.host.tabs().into_iter().find(|t| t.url == public), TIMEOUT).await;
            let target = format!("chrome-extension://{url_host}/options.html?held");
            self.eval(&opener_view, &format!("held.location = {}; 0", Value::String(target.clone())), None).await;
            let held_url = || held.as_ref().and_then(|h| self.host.tabs().into_iter().find(|t| t.id == h.id)).map(|t| t.url);
            let judged = wait_until(|| self.host.refused.borrow().contains(&target) || held_url().is_some_and(|u| u != public), TIMEOUT).await;
            let refused = judged && held_url().as_deref() == Some(public.as_str());
            self.note("held_window_cannot_open_extension_page", refused, format!("the window a web page opened at {public} = {held:?}; at {:?} after the page sent it to {target}", held_url()));
        }

        async fn ports(&self) {
            let ports_id = ExtensionId::parse(PORTS_ID).expect("ports id");
            let tab = self.host.create_tab(&self.url("/index.html"), false).expect("ports tab");
            let view = self.host.web_view(tab).expect("ports tab view");
            let world = Some(PORTS_ID);
            let reply = self.wait_for_js(&view, "String(document.documentElement.dataset.portsReply)", world, |v| v == "pong").await;
            self.note("port_round_trip", reply.as_deref() == Some("pong"), format!("content script port -> background -> {reply:?}"));

            let Some(popup) = self.popup(&ports_id, self.tab).await else {
                self.note("port_popup", false, "no popup view");
                return;
            };
            let window = self.park(&popup);
            let title = wait_for_value(|| popup.title().map(String::from).filter(|t| t.starts_with("ports-popup:")), TIMEOUT).await;
            self.note("port_popup", title.as_deref() == Some("ports-popup:pong"), format!("title = {title:?}"));
            window.destroy();
            drop(window);
            drop(popup);

            let Some(popup) = self.popup(&ports_id, self.tab).await else {
                self.note("port_closed_popup_disconnects", false, "no popup view");
                return;
            };
            let _window = self.park(&popup);
            let log = || self.eval_async(&popup, "return await chrome.runtime.sendMessage('log');");
            let disconnected = wait_for_async(|| async move { log().await.filter(|l| l.as_array().is_some_and(|l| l.iter().any(|e| e == "disconnect:popup"))) }, TIMEOUT).await;
            self.note("port_closed_popup_disconnects", disconnected.is_some(), format!("background log after the popup closed = {:?}", log().await));

            let to_tab = self
                .eval_async(&popup, &format!("const port = chrome.tabs.connect({}, {{ name: 'to-tab' }}); return await new Promise((resolve) => {{ port.onMessage.addListener(resolve); port.postMessage('hi'); }});", tab.0))
                .await;
            self.note("tabs_connect", to_tab.as_ref().is_some_and(|v| v["echo"] == "hi" && v["name"] == "to-tab"), format!("tabs.connect -> content script echo = {to_tab:?}"));

            let other = self.host.create_tab(&self.url("/page2.html"), false).expect("page2 tab");
            let other_view = self.host.web_view(other).expect("page2 tab view");
            wait_until(|| other_view.title().as_deref() == Some("Vsesvit fixture 2"), TIMEOUT).await;
            let nobody = self
                .eval_async(&popup, &format!("const port = chrome.tabs.connect({}); return await new Promise((resolve) => port.onDisconnect.addListener(() => resolve(chrome.runtime.lastError && chrome.runtime.lastError.message)));", other.0))
                .await;
            self.note("port_without_receiver", nobody.as_ref().and_then(Value::as_str).is_some_and(|s| s.contains("Receiving end does not exist")), format!("tabs.connect to a tab without a content script: lastError = {nobody:?}"));

            let closed = self
                .eval_async(&popup, "const port = chrome.runtime.connect({ name: 'closed' }); port.disconnect(); try { port.postMessage(1); return 'posted'; } catch (e) { return e.message; }")
                .await;
            self.note("port_disconnected_refuses_to_post", closed.as_ref().and_then(Value::as_str) == Some("Attempting to use a disconnected port object"), format!("{closed:?}"));

            let count = |log: Option<Value>| log.as_ref().and_then(Value::as_array).map_or(0, |l| l.iter().filter(|e| *e == "disconnect:content").count());
            let before = count(log().await);
            self.host.update_tab(tab, Some(&self.url("/page2.html")), None);
            let after = wait_for_async(|| async move { Some(count(log().await)).filter(|n| *n > before) }, TIMEOUT).await;
            self.note("port_navigation_disconnects", after.is_some(), format!("disconnect:content in the background log {before} -> {after:?} after the tab left the page"));

            let in_background = self.eval_async(&popup, "return await chrome.runtime.sendMessage('background-page');").await;
            let in_popup = self.eval_async(&popup, "const page = await chrome.runtime.getBackgroundPage(); return page === chrome.extension.getBackgroundPage() && page.portsLog().includes('connect:popup:page');").await;
            self.note("get_background_page", in_background == Some(Value::Bool(true)) && in_popup == Some(Value::Bool(true)), format!("runtime.getBackgroundPage in the background page = {in_background:?}, in the popup it opened = {in_popup:?}"));

            let Some(friend) = self.popup(&ExtensionId::parse(FRIEND_ID).expect("friend id"), self.tab).await else {
                self.note("external_messages", false, "no friend popup");
                return;
            };
            let _friend_window = self.park(&friend);
            wait_until(|| friend.title().as_deref() == Some("Vsesvit Ports Friend"), TIMEOUT).await;
            let external = self
                .eval_async(
                    &friend,
                    &format!("const id = {}; const port = chrome.runtime.connect(id, {{ name: 'external' }}); const echo = await new Promise((resolve) => {{ port.onMessage.addListener(resolve); port.postMessage('hi'); }}); return [await chrome.runtime.sendMessage(id, 'hello'), echo];", Value::from(PORTS_ID)),
                )
                .await;
            let external_ok = external.as_ref().is_some_and(|v| v[0]["external"] == "hello" && v[0]["from"] == FRIEND_ID && v[1]["echo"] == "hi" && v[1]["from"] == FRIEND_ID && v[1]["name"] == "external");
            self.note("external_messages", external_ok, format!("friend -> ports: [sendMessage, connect] = {external:?}"));

            let twin_id = self.twin.borrow().id.clone();
            let Some(stranger) = self.popup(&twin_id, self.tab).await else {
                self.note("external_refused", false, "no twin popup");
                return;
            };
            let _stranger_window = self.park(&stranger);
            wait_until(|| stranger.title().is_some_and(|t| t.starts_with("twin-popup:")), TIMEOUT).await;
            let refused = self
                .eval_async(
                    &stranger,
                    &format!("const id = {}; let message; try {{ await chrome.runtime.sendMessage(id, 'hello'); message = 'answered'; }} catch (e) {{ message = e.message; }} const port = chrome.runtime.connect(id); const connected = await new Promise((resolve) => port.onDisconnect.addListener(() => resolve(chrome.runtime.lastError && chrome.runtime.lastError.message))); return [message, connected];", Value::from(PORTS_ID)),
                )
                .await;
            let worker = self.eval_async(&stranger, "try { await chrome.runtime.getBackgroundPage(); return 'page'; } catch (e) { return e.message; }").await;
            self.note("get_background_page_service_worker", worker.as_ref().and_then(Value::as_str) == Some("You do not have a background page."), format!("runtime.getBackgroundPage with a service worker = {worker:?}"));
            let refused_ok = refused.as_ref().and_then(Value::as_array).is_some_and(|r| r.len() == 2 && r.iter().all(|m| m.as_str().is_some_and(|m| m.contains("Receiving end does not exist"))));
            self.note("external_refused", refused_ok, format!("twin (not in externally_connectable) -> ports: [sendMessage, connect] = {refused:?}"));
            self.host.remove_tab(tab);
            self.host.remove_tab(other);
        }

        async fn menus(&self) {
            let id = self.menus.id.clone();
            let top = Target { page_url: self.url("/index.html"), ..Target::default() };
            let ours = |target: &Target| self.runtime.page_menu(target).into_iter().find(|(e, _)| *e == id).map(|(_, entry)| entry);
            let item = |id: &str, title: &str, checked: Option<bool>| Entry::Item { id: ItemId::Str(id.to_owned()), title: title.to_owned(), enabled: true, checked };
            let submenu = |title: &str, children: Vec<Entry>| Entry::Submenu { title: title.to_owned(), enabled: true, children };

            let on_page = wait_for_value(|| ours(&top), TIMEOUT).await;
            let order: Vec<ExtensionId> = self.runtime.page_menu(&top).into_iter().map(|(e, _)| e).collect();
            let expected = submenu("Vsesvit Menus", vec![item("parent", "Menus parent", None), item("check", "Menus check", Some(true))]);
            self.note("menus_page", on_page.as_ref() == Some(&expected) && order.first() == Some(&id), format!("{on_page:?}; extensions by name: {order:?}"));
            let link = Target { link_url: Some(self.url("/page2.html")), selection: "hello".to_owned(), ..top.clone() };
            let on_link = ours(&link);
            let expected = submenu("Menus parent", vec![item("link", "Link with hello", None), item("selection", "Find \u{201c}hello\u{201d}", None)]);
            self.note("menus_link_and_selection", on_link.as_ref() == Some(&expected), format!("{on_link:?}"));
            let frame = Target { frame_url: Some(self.url("/page2.html")), ..top.clone() };
            let in_frame = ours(&frame);
            let expected = submenu("Vsesvit Menus", vec![item("parent", "Menus parent", None), item("frame", "Menus frame", None), item("check", "Menus check", Some(true))]);
            self.note("menus_frame", in_frame.as_ref() == Some(&expected), format!("{in_frame:?}"));
            let errors = wait_for_value(|| {
                let created: Vec<Value> = self.menus_log().into_iter().filter(|e| e.get("created").is_some()).collect();
                (created.len() == 2).then_some(created)
            }, TIMEOUT).await;
            let error_of = |what: &str| errors.iter().flatten().find(|e| e["created"] == what).and_then(|e| e["error"].as_str().map(str::to_owned)).unwrap_or_default();
            let refused = error_of("duplicate") == "Cannot create item with duplicate id parent" && error_of("generated").contains("must pass an id parameter");
            self.note("menus_create_errors", refused, format!("{errors:?}"));

            self.runtime.menu_clicked(&id, &ItemId::Str("check".to_owned()), Some(self.tab), Some(&top));
            let click = wait_for_value(|| self.menus_log().into_iter().find(|e| e["clicked"]["menuItemId"] == "check"), TIMEOUT).await.unwrap_or_default();
            let info = serde_json::json!({ "menuItemId": "check", "editable": false, "wasChecked": true, "checked": false, "pageUrl": top.page_url, "frameId": 0 });
            let flipped = ours(&top).is_some_and(|e| matches!(e, Entry::Submenu { children, .. } if children.contains(&item("check", "Menus check", Some(false)))));
            self.note("menus_clicked", click["clicked"] == info && click["tab"] == self.tab.0 && flipped, format!("{click}; unchecked after = {flipped}"));
            let action_menu = self.runtime.action_menu(&id);
            self.runtime.menu_clicked(&id, &ItemId::Str("action".to_owned()), Some(self.tab), None);
            let click = wait_for_value(|| self.menus_log().into_iter().find(|e| e["clicked"]["menuItemId"] == "action"), TIMEOUT).await.unwrap_or_default();
            let action_ok = action_menu == [item("parent", "Menus parent", None), item("action", "Menus action", None)] && click["clicked"] == serde_json::json!({ "menuItemId": "action", "editable": false }) && click["tab"] == self.tab.0;
            self.note("menus_action", action_ok, format!("action menu = {action_menu:?}; {click}"));

            let Some(popup) = self.popup(&id, self.tab).await else {
                self.note("menus_update_and_remove", false, "no popup view");
                return;
            };
            let window = self.park(&popup);
            wait_until(|| popup.title().as_deref() == Some("Vsesvit Menus"), TIMEOUT).await;
            let edits = self.eval_async(&popup, "await chrome.contextMenus.update('check', { title: 'Menus check 2' }); try { await chrome.contextMenus.remove('nope'); return 'removed'; } catch (e) { return e.message; }").await;
            let renamed = ours(&top);
            let expected = submenu("Vsesvit Menus", vec![item("parent", "Menus parent", None), item("check", "Menus check 2", Some(false))]);
            self.note("menus_update_and_remove", edits.as_ref().and_then(Value::as_str) == Some("Cannot find menu item with id nope") && renamed.as_ref() == Some(&expected), format!("remove('nope') -> {edits:?}; {renamed:?}"));
            window.destroy();
            drop(window);
            drop(popup);

            self.runtime.unload(&id);
            if let Err(e) = self.runtime.load(&self.menus) {
                self.note("menus_kept", false, format!("load: {e}"));
                return;
            }
            let kept = ours(&top);
            let Some(popup) = self.popup(&id, self.tab).await else {
                self.note("menus_kept", false, "no popup view after the restart");
                return;
            };
            let _window = self.park(&popup);
            wait_until(|| popup.title().as_deref() == Some("Vsesvit Menus"), TIMEOUT).await;
            let cleared = self.eval_async(&popup, "await chrome.contextMenus.removeAll(); return 'cleared';").await;
            let left = ours(&top);
            self.note("menus_kept", kept.as_ref() == Some(&expected) && cleared.is_some() && left.is_none(), format!("after a restart: {kept:?}; after removeAll: {left:?}"));

            let Some(classic) = self.popup(&self.classic_id, self.tab).await else {
                self.note("menus_onclick", false, "no classic popup");
                return;
            };
            let _classic_window = self.park(&classic);
            wait_until(|| classic.title().as_deref() == Some("classic"), TIMEOUT).await;
            let made = self.eval_async(&classic, "return chrome.contextMenus.create({ title: 'Classic item', onclick: (info) => { document.title = 'clicked:' + info.menuItemId; } });").await;
            let made = made.as_ref().and_then(Value::as_i64).map(ItemId::Int);
            let entry = wait_for_value(|| self.runtime.page_menu(&top).into_iter().find(|(e, _)| *e == self.classic_id).map(|(_, e)| e), TIMEOUT).await;
            if let Some(made) = &made {
                self.runtime.menu_clicked(&self.classic_id, made, Some(self.tab), Some(&top));
            }
            let title = wait_for_value(|| classic.title().map(String::from).filter(|t| t.starts_with("clicked:")), TIMEOUT).await;
            let wanted = made.as_ref().map(|id| Entry::Item { id: id.clone(), title: "Classic item".to_owned(), enabled: true, checked: None });
            let onclick_ok = entry.is_some() && entry == wanted && title == made.as_ref().map(|id| format!("clicked:{id}"));
            self.note("menus_onclick", onclick_ok, format!("create() returned {made:?}; page menu entry = {entry:?}; popup title = {title:?}"));
        }

        /// What the menus fixture's background logged into its `storage.local`.
        fn menus_log(&self) -> Vec<Value> {
            let mut profile = self.profile.borrow_mut();
            let items = profile.ext_storage().get(&self.menus.id, Area::Local, Some(&["log".to_owned()])).unwrap_or_default();
            items.get("log").and_then(Value::as_array).cloned().unwrap_or_default()
        }

        async fn commands(&self) {
            let id = self.commands_id.clone();
            let listed = |shortcuts: [&str; 4]| {
                serde_json::json!([
                    { "name": "_execute_action", "description": "", "shortcut": shortcuts[0] },
                    { "name": "free", "description": "Free key", "shortcut": shortcuts[1] },
                    { "name": "keyless", "description": "No key", "shortcut": shortcuts[2] },
                    { "name": "taken", "description": "The browser's key", "shortcut": shortcuts[3] },
                ])
            };
            let resolved = listed(["Alt+Shift+A", "Alt+Shift+F", "", ""]);
            let from_background = wait_for_value(|| self.commands_storage("all"), TIMEOUT).await;
            self.note("commands_get_all_background", from_background.as_ref() == Some(&resolved), format!("{from_background:?}"));

            let page2 = self.url("/page2.html");
            let other = self.host.create_tab(&page2, false).expect("second tab");
            let other_view = self.host.web_view(other).expect("second tab view");
            self.wait_for_js(&other_view, "document.readyState", None, |s| s == "complete").await;
            let Some(popup) = self.popup(&id, self.tab).await else {
                self.note("commands_get_all_popup", false, "no popup view");
                return;
            };
            let _window = self.park(&popup);
            wait_until(|| popup.title().as_deref() == Some("Vsesvit Commands"), TIMEOUT).await;
            let from_popup = self.eval_async(&popup, "return chrome.commands.getAll();").await;
            self.note("commands_get_all_popup", from_popup.as_ref() == Some(&resolved), format!("{from_popup:?}"));

            let url_of_other = format!("return (await chrome.tabs.get({})).url ?? null;", other.0);
            let hidden = self.eval_async(&popup, &url_of_other).await;
            self.runtime.command(&id, "free", Some(other));
            let fired = wait_for_value(|| self.commands_storage("command"), TIMEOUT).await;
            let shown = self.eval_async(&popup, &url_of_other).await;
            let fired_ok = fired == Some(serde_json::json!({ "name": "free", "tab": other.0, "url": page2 }));
            let granted = hidden == Some(Value::Null) && shown == Some(Value::String(page2.clone()));
            self.note("commands_on_command", fired_ok && granted, format!("onCommand got {fired:?}; the tab's URL before = {hidden:?}, after = {shown:?}"));

            let alt_shift_f: Chord = "Alt+Shift+F".parse().expect("a chord");
            let stored = {
                let mut profile = self.profile.borrow_mut();
                profile.extension_shortcuts().and_then(|shortcuts| {
                    let mut keymap = profile.prefs().keymap();
                    keymap.assign_extension(&shortcuts, &id, "keyless", Some(alt_shift_f));
                    profile.prefs().set_keymap(&keymap)
                })
            };
            let reassigned = self.eval_async(&popup, "return chrome.commands.getAll();").await;
            let moved = listed(["Alt+Shift+A", "", "Alt+Shift+F", ""]);
            self.note("commands_get_all_assigned", stored.is_ok() && reassigned.as_ref() == Some(&moved), format!("stored: {stored:?}; {reassigned:?}"));
            self.host.remove_tab(other);

            let Some(classic) = self.popup(&self.classic_id, self.tab).await else {
                self.note("commands_need_the_manifest_key", false, "no classic popup");
                return;
            };
            let _classic_window = self.park(&classic);
            wait_until(|| classic.title().as_deref() == Some("classic"), TIMEOUT).await;
            let api = self.eval_async(&classic, "return typeof chrome.commands;").await;
            self.note("commands_need_the_manifest_key", api.as_ref().and_then(Value::as_str) == Some("undefined"), format!("typeof chrome.commands without commands = {api:?}"));
        }

        /// What the commands fixture's background stored under `key` in its `storage.local`.
        fn commands_storage(&self, key: &str) -> Option<Value> {
            let mut profile = self.profile.borrow_mut();
            let items = profile.ext_storage().get(&self.commands_id, Area::Local, Some(&[key.to_owned()])).ok()?;
            items.get(key).cloned()
        }

        async fn notifications(&self) {
            let id = self.notifications_id.clone();
            let Some(popup) = self.popup(&id, self.tab).await else {
                self.note("notifications_create", false, "no popup view");
                return;
            };
            let _window = self.park(&popup);
            wait_until(|| popup.title().as_deref() == Some("Vsesvit Notifications"), TIMEOUT).await;

            let created = self
                .eval_async(
                    &popup,
                    r#"const greeting = await chrome.notifications.create("greeting", { type: "basic", iconUrl: "icon.svg", title: "Hello", message: "From the harness", contextMessage: "Vsesvit", priority: 2, buttons: [{ title: "Yes" }, { title: "No" }, { title: "Dropped" }] });
                    const list = await new Promise((resolve) => chrome.notifications.create({ type: chrome.notifications.TemplateType.LIST, iconUrl: "/icon.svg", title: "Inbox", message: "", items: [{ title: "Ann", message: "Lunch?" }, { title: "Bo", message: "Done" }] }, resolve));
                    return { greeting, list, all: await chrome.notifications.getAll() };"#,
                )
                .await
                .unwrap_or_default();
            let list = created["list"].as_str().unwrap_or_default().to_owned();
            let uuid_v4 = list.len() == 36
                && list.char_indices().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_digit() || ('a'..='f').contains(&c) })
                && list.as_bytes()[14] == b'4';
            let all = serde_json::json!({ "greeting": true, list.clone(): true });
            self.note("notifications_create", created["greeting"] == "greeting" && uuid_v4 && created["all"] == all, &created);

            let describe = |shown: &Option<Shown>| format!("{:?}", shown.as_ref().map(|s| (&s.title, &s.body, &s.buttons, s.priority, s.icon.len())));
            // A 256-pixel icon arrives as a PNG of 128 by 128 (IHDR's width and height).
            let png_128 = |icon: &[u8]| icon.starts_with(b"\x89PNG\r\n\x1a\n") && icon.get(16..24) == Some(&[0, 0, 0, 128, 0, 0, 0, 128]);
            let greeting = self.runtime.notification(&id, "greeting");
            let greeting_ok = greeting.as_ref().is_some_and(|s| {
                s.title == "Hello" && s.body.as_deref() == Some("Vsesvit\n\nFrom the harness") && s.buttons == ["Yes", "No"] && s.priority == Priority::Urgent && png_128(&s.icon)
            });
            let inbox = self.runtime.notification(&id, &list);
            let inbox_ok = inbox.as_ref().is_some_and(|s| s.title == "Inbox" && s.body.as_deref() == Some("Ann - Lunch?\nBo - Done") && s.buttons.is_empty() && s.priority == Priority::Normal);
            self.note("notifications_shown", greeting_ok && inbox_ok, format!("greeting = {}; list = {}", describe(&greeting), describe(&inbox)));

            let updated = self
                .eval_async(&popup, r#"return [await chrome.notifications.update("greeting", { type: "progress", progress: 40, title: "Copying" }), await chrome.notifications.update("missing", { title: "x" })];"#)
                .await;
            let progress = self.runtime.notification(&id, "greeting");
            let update_ok = updated == Some(serde_json::json!([true, false])) && progress.as_ref().is_some_and(|s| s.title == "40% - Copying" && s.buttons == ["Yes", "No"]);
            self.note("notifications_update", update_ok, format!("update answered {updated:?}; greeting = {}", describe(&progress)));

            let refused = self
                .eval_async(
                    &popup,
                    r#"const basic = { type: "basic", iconUrl: "icon.svg", title: "t", message: "m" };
                    const attempt = async (...args) => { try { await chrome.notifications.create(...args); return "created"; } catch (e) { return e.message; } };
                    return [
                      await new Promise((resolve) => chrome.notifications.create("lacking", { type: "basic", iconUrl: "icon.svg", title: "t" }, () => resolve(chrome.runtime.lastError && chrome.runtime.lastError.message))),
                      await attempt(Object.assign({}, basic, { priority: -1 })),
                      await attempt(Object.assign({}, basic, { imageUrl: "icon.svg" })),
                      await attempt(Object.assign({}, basic, { type: "progress", progress: 150 })),
                      await attempt(Object.assign({}, basic, { iconUrl: "missing.png" })),
                      await attempt("x".repeat(501), basic),
                      Object.keys(await chrome.notifications.getAll()).length,
                    ];"#,
                )
                .await;
            let chrome_errors = serde_json::json!([
                "Some of the required properties are missing: type, iconUrl, title and message.",
                "Low-priority notifications are deprecated on this platform.",
                "Image resource provided for notification type != image",
                "The progress value should range from 0 to 100",
                "Unable to download all specified images.",
                "The notification's ID should be 500 characters or less",
                2,
            ]);
            self.note("notifications_errors", refused.as_ref() == Some(&chrome_errors), format!("{refused:?}"));

            self.runtime.notification_activated(&id, "greeting", Activation::Click);
            self.runtime.notification_activated(&id, "greeting", Activation::Button(5));
            self.runtime.notification_activated(&id, "greeting", Activation::Button(1));
            self.runtime.notification_activated(&id, "greeting", Activation::Settings);
            let log = wait_for_value(|| Some(self.notifications_log()).filter(|log| log.len() >= 2), TIMEOUT).await;
            let listed = self.runtime.notification(&id, "greeting").is_some();
            let clicked = serde_json::json!([["clicked", "greeting"], ["button", "greeting", 1]]);
            self.note("notifications_clicked", log.as_ref() == clicked.as_array() && listed, format!("background log = {log:?}; still listed = {listed}"));

            let cleared = self.eval_async(&popup, r#"return [await chrome.notifications.clear("greeting"), await chrome.notifications.clear("greeting"), await chrome.notifications.getAll()];"#).await;
            let closed = wait_for_value(|| self.notifications_log().get(2).cloned(), TIMEOUT).await;
            let clear_ok = cleared == Some(serde_json::json!([true, false, { list.clone(): true }])) && closed == Some(serde_json::json!(["closed", "greeting", false]));
            self.note("notifications_clear", clear_ok, format!("clear answered {cleared:?}; the background got {closed:?}"));

            let switch = |allowed: bool| {
                let stored = self.profile.borrow_mut().extensions().set_notifications_allowed(&id, allowed);
                self.runtime.notification_permission_changed(&id);
                stored
            };
            let off = switch(false);
            let denied = wait_for_value(|| Some(self.notifications_log()).filter(|log| log.len() >= 5).map(|log| log[3..].to_vec()), TIMEOUT).await;
            let refused = self
                .eval_async(
                    &popup,
                    r#"const level = await chrome.notifications.getPermissionLevel();
                    try { await chrome.notifications.create({ type: "basic", iconUrl: "icon.svg", title: "t", message: "m" }); return [level, "created"]; } catch (e) { return [level, e.message]; }"#,
                )
                .await;
            let off_ok = off.is_ok()
                && denied == Some(vec![serde_json::json!(["closed", list.clone(), false]), serde_json::json!(["permission", "denied"])])
                && refused == Some(serde_json::json!(["denied", "Notifications are turned off for this extension."]))
                && self.runtime.notification(&id, &list).is_none();
            self.note("notifications_turned_off", off_ok, format!("stored: {off:?}; the background got {denied:?}; the popup got {refused:?}"));
            let on = switch(true);
            let granted = wait_for_value(|| self.notifications_log().get(5).cloned(), TIMEOUT).await;
            let level = self.eval_async(&popup, "return chrome.notifications.getPermissionLevel();").await;
            let on_ok = on.is_ok() && granted == Some(serde_json::json!(["permission", "granted"])) && level == Some(serde_json::json!("granted"));
            self.note("notifications_turned_on", on_ok, format!("stored: {on:?}; the background got {granted:?}; getPermissionLevel = {level:?}"));

            let Some(classic) = self.popup(&self.classic_id, self.tab).await else {
                self.note("notifications_need_the_permission", false, "no classic popup");
                return;
            };
            let _classic_window = self.park(&classic);
            wait_until(|| classic.title().as_deref() == Some("classic"), TIMEOUT).await;
            let api = self.eval_async(&classic, "return typeof chrome.notifications;").await;
            self.note("notifications_need_the_permission", api.as_ref().and_then(Value::as_str) == Some("undefined"), format!("typeof chrome.notifications without the permission = {api:?}"));
        }

        /// The events the notifications fixture's background logged in its `storage.local`.
        fn notifications_log(&self) -> Vec<Value> {
            let mut profile = self.profile.borrow_mut();
            let items = profile.ext_storage().get(&self.notifications_id, Area::Local, Some(&["log".to_owned()])).unwrap_or_default();
            items.get("log").and_then(Value::as_array).cloned().unwrap_or_default()
        }

        async fn declarative_net_request(&self) {
            // The shell's own content blockers (tracking protection, cookie rules) share each
            // tab's manager with the extensions'; this one blocks every run's browser.png.
            let ucm = self.runtime.user_content_manager(self.tab);
            let store = webkit::UserContentFilterStore::new(&self.out_dir.join("browser-filters").to_string_lossy());
            let compiled = Rc::new(RefCell::new(None));
            let slot = compiled.clone();
            let json = r#"[{"trigger": {"url-filter": "/browser\\.png"}, "action": {"type": "block"}}]"#;
            store.save("vsesvit-tracking-protection", &glib::Bytes::from_static(json.as_bytes()), None::<&gio::Cancellable>, move |r| *slot.borrow_mut() = Some(r));
            let browser_filter = match wait_for_value(|| compiled.borrow_mut().take(), TIMEOUT).await {
                Some(Ok(filter)) => filter,
                other => {
                    self.note("dnr_enabled_rulesets", false, format!("the browser's blocker did not compile: {other:?}"));
                    return;
                }
            };
            ucm.add_filter(&browser_filter);

            let id = self.dnr.id.clone();
            let Some(popup) = self.popup(&id, self.tab).await else {
                self.note("dnr_enabled_rulesets", false, "no popup view");
                return;
            };
            let _window = self.park(&popup);
            wait_until(|| popup.title().as_deref() == Some("Vsesvit DNR"), TIMEOUT).await;
            let mut runs = Vec::new();
            let fixed = self.dnr_run("start").await;
            runs.push(fixed.clone());
            self.note("dnr_static_rulesets", fixed == ["dynamic", "extra", "session"], format!("loaded {fixed:?}: base blocks static.png, extra is off"));

            let enabled = self
                .eval_async(
                    &popup,
                    r#"const d = chrome.declarativeNetRequest;
                    const before = await d.getEnabledRulesets();
                    await d.updateEnabledRulesets({ disableRulesetIds: ["base"], enableRulesetIds: ["extra"] });
                    let refused;
                    try { await d.updateEnabledRulesets({ enableRulesetIds: ["nope"] }); } catch (e) { refused = e.message; }
                    return { before, after: await d.getEnabledRulesets(), refused, available: await d.getAvailableStaticRuleCount(), limit: d.MAX_NUMBER_OF_ENABLED_STATIC_RULESETS };"#,
                )
                .await;
            let swapped = self.dnr_run("swapped").await;
            runs.push(swapped.clone());
            let expected = serde_json::json!({ "before": ["base"], "after": ["extra"], "refused": "Invalid ruleset id: nope.", "available": 329_999, "limit": 50 });
            self.note("dnr_enabled_rulesets", enabled.as_ref() == Some(&expected) && swapped == ["dynamic", "session", "static"], format!("{enabled:?}; then loaded {swapped:?}"));

            let added = self
                .eval_async(
                    &popup,
                    r#"const d = chrome.declarativeNetRequest;
                    const block = (id, path) => ({ id, action: { type: "block" }, condition: { urlFilter: path, resourceTypes: ["image"] } });
                    await d.updateDynamicRules({ addRules: [block(1, "/dynamic.png")] });
                    await new Promise((resolve) => d.updateSessionRules({ addRules: [block(1, "/session.png")] }, resolve));
                    let refused;
                    try { await d.updateDynamicRules({ addRules: [block(1, "/again.png")] }); } catch (e) { refused = e.message; }
                    return { dynamic: await d.getDynamicRules(), session: (await d.getSessionRules({ ruleIds: [1, 2] })).map((r) => r.condition.urlFilter), refused };"#,
                )
                .await;
            let blocked = self.dnr_run("added").await;
            runs.push(blocked.clone());
            let expected = serde_json::json!({
                "dynamic": [{ "id": 1, "priority": 1, "action": { "type": "block" }, "condition": { "urlFilter": "/dynamic.png", "resourceTypes": ["image"] } }],
                "session": ["/session.png"],
                "refused": "Rule with id 1 does not have a unique ID.",
            });
            self.note("dnr_dynamic_and_session_rules", added.as_ref() == Some(&expected) && blocked == ["static"], format!("{added:?}; then loaded {blocked:?}"));

            // uBlock Origin Lite's "no filtering" on a site, then everywhere but that site.
            let site = self
                .eval_async(&popup, r#"await chrome.declarativeNetRequest.updateDynamicRules({ addRules: [{ id: 2, priority: 2000000, action: { type: "allowAllRequests" }, condition: { requestDomains: ["127.0.0.1"], resourceTypes: ["main_frame"] } }] }); return "allowed";"#)
                .await;
            let allowed = self.dnr_run("site-off").await;
            runs.push(allowed.clone());
            let reverse = self
                .eval_async(&popup, r#"await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds: [2], addRules: [{ id: 2, priority: 2000000, action: { type: "allowAllRequests" }, condition: { excludedRequestDomains: ["127.0.0.1"], resourceTypes: ["main_frame"] } }] }); return "reversed";"#)
                .await;
            let elsewhere = self.dnr_run("site-on").await;
            runs.push(elsewhere.clone());
            let removed = self.eval_async(&popup, r#"await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds: [2] }); return "removed";"#).await;
            let toggle_ok = (site, reverse, removed) == (Some(serde_json::json!("allowed")), Some(serde_json::json!("reversed")), Some(serde_json::json!("removed")))
                && allowed == ["dynamic", "extra", "session", "static"]
                && elsewhere == ["static"];
            self.note("dnr_allow_all_requests_for_a_site", toggle_ok, format!("with the site's rule loaded {allowed:?}; with every other site's, {elsewhere:?}"));

            let regex = self
                .eval_async(&popup, r#"const d = chrome.declarativeNetRequest; return [await d.isRegexSupported({ regex: "^https?://[a-z]+\.test/" }), await d.isRegexSupported({ regex: "ads|track" })];"#)
                .await;
            let regex_ok = regex == Some(serde_json::json!([{ "isSupported": true }, { "isSupported": false, "reason": "syntaxError" }]));
            self.note("dnr_is_regex_supported", regex_ok, format!("{regex:?}"));

            let session = self
                .eval_async(
                    &popup,
                    r#"const changed = new Promise((resolve) => chrome.storage.onChanged.addListener((changes, area) => area === "session" && resolve(changes.toggle.newValue)));
                    await chrome.storage.session.set({ toggle: "off" });
                    return { changed: await changed, items: await chrome.storage.session.get(null) };"#,
                )
                .await;

            // A restart keeps the dynamic rules and the rulesets the extension chose, and drops
            // the session rules and storage.
            self.runtime.unload(&id);
            if let Err(e) = self.runtime.load(&self.dnr) {
                self.note("dnr_rules_kept", false, format!("load: {e}"));
                return;
            }
            let Some(popup) = self.popup(&id, self.tab).await else {
                self.note("dnr_rules_kept", false, "no popup view after the restart");
                return;
            };
            let _window = self.park(&popup);
            wait_until(|| popup.title().as_deref() == Some("Vsesvit DNR"), TIMEOUT).await;
            let kept = self
                .eval_async(
                    &popup,
                    r#"const d = chrome.declarativeNetRequest;
                    return { dynamic: (await d.getDynamicRules()).map((r) => r.id), session: await d.getSessionRules(), enabled: await d.getEnabledRulesets(), storage: await chrome.storage.session.get(null) };"#,
                )
                .await;
            let restarted = self.dnr_run("restarted").await;
            runs.push(restarted.clone());
            let kept_ok = kept == Some(serde_json::json!({ "dynamic": [1], "session": [], "enabled": ["extra"], "storage": {} })) && restarted == ["session", "static"];
            self.note("dnr_rules_kept", kept_ok, format!("after a restart: {kept:?}; loaded {restarted:?}"));
            let session_ok = session == Some(serde_json::json!({ "changed": "off", "items": { "toggle": "off" } })) && kept.as_ref().is_some_and(|k| k["storage"] == serde_json::json!({}));
            self.note("storage_session", session_ok, format!("{session:?}; after the restart: {:?}", kept.as_ref().map(|k| &k["storage"])));

            let unblocked = runs.iter().any(|run| run.iter().any(|kind| kind == "browser"));
            self.note("dnr_coexists_with_browser_blockers", !unblocked && runs.len() == 6, format!("loaded per run: {runs:?}"));
            ucm.remove_filter(&browser_filter);
        }

        /// Loads the declarativeNetRequest fixture page as `run` in the first tab, once the
        /// rules changed so far are on it, and returns which of its images reached the server.
        async fn dnr_run(&self, run: &str) -> Vec<String> {
            self.view.load_uri(&self.url(&format!("/dnr.html?run={run}")));
            let done = format!("done:{run}");
            wait_until(|| self.view.title().as_deref() == Some(done.as_str()), TIMEOUT).await;
            let prefix = format!("/dnr/{run}/");
            let mut loaded: Vec<String> = self.server.hits().iter().filter_map(|p| p.strip_prefix(&prefix)?.strip_suffix(".png").map(str::to_owned)).collect();
            loaded.sort();
            loaded
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

            // runtime.reload() from a page restarts the whole extension: a new background
            // life (no lifecycle event, as for a packed extension in Chrome), its alarms
            // gone, and the page reloaded with a working API. The page counts its alarms
            // and answers before it reloads, since a reload can tear it down mid-call.
            let Some(options_url) = self.host.tabs().into_iter().map(|t| t.url).find(|u| u.ends_with("/options.html")) else {
                self.note("runtime_reload", false, "no options tab to reload from");
                return;
            };
            let page = self.host.create_tab(&options_url, false).expect("options tab");
            let view = self.host.web_view(page).expect("options tab view");
            self.wait_for_js(&view, "JSON.stringify(window.__twinOptions || null)", None, |v| v.contains("\"done\":true")).await;
            let before = self.twin_lives().len();
            let armed = self.eval_async(&view, "await chrome.alarms.create('before-reload', { delayInMinutes: 5 }); const armed = (await chrome.alarms.getAll()).length; window.__beforeReload = true; setTimeout(() => chrome.runtime.reload()); return armed;").await;
            let lives = wait_for_value(|| {
                let lives = self.twin_lives();
                (lives.len() > before).then_some(lives)
            }, TIMEOUT).await;
            let reloaded = self.wait_for_js(&view, "String(!window.__beforeReload && !!(window.__twinOptions && window.__twinOptions.done))", None, |v| v == "true").await;
            let alarms = self.eval_async(&view, "return (await chrome.alarms.getAll()).map((a) => a.name);").await;
            let reload_ok = armed == Some(serde_json::json!(1)) && lives.as_ref().is_some_and(|l| l.len() == before + 1) && reloaded.as_deref() == Some("true") && alarms == Some(serde_json::json!([]));
            self.note("runtime_reload", reload_ok, format!("alarms before = {armed:?}; lives {before} -> {lives:?}; page reloaded = {reloaded:?}; alarms after = {alarms:?}"));
            self.host.remove_tab(page);

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
            self.server.url(path).to_string()
        }

        /// The action popup `activate_action` shows, once it exists.
        async fn popup(&self, id: &ExtensionId, tab: TabId) -> Option<webkit::WebView> {
            let shown = Rc::new(RefCell::new(None));
            let slot = shown.clone();
            self.runtime.activate_action(id, Some(tab), move |view| *slot.borrow_mut() = Some(view));
            wait_for_value(|| shown.borrow_mut().take(), TIMEOUT).await
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

    async fn wait_for_async<T, F: std::future::Future<Output = Option<T>>>(probe: impl Fn() -> F, timeout: Duration) -> Option<T> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(v) = probe().await {
                return Some(v);
            }
            if Instant::now() >= deadline {
                return None;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
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
        /// The navigation gate every shell keeps per view (lib.rs), as the GTK tab keeps it.
        gate: Rc<RefCell<Gate>>,
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
        /// Every target a gate refused, for checks that wait on one.
        refused: Rc<RefCell<Vec<String>>>,
        next_id: Cell<u32>,
        me: RefCell<std::rc::Weak<Host>>,
    }

    impl Host {
        fn new(session: webkit::NetworkSession, container: gtk::Box) -> Host {
            Host {
                session,
                container,
                tabs: RefCell::new(Vec::new()),
                runtime: RefCell::new(None),
                created: RefCell::new(Vec::new()),
                refused: Rc::new(RefCell::new(Vec::new())),
                next_id: Cell::new(1),
                me: RefCell::new(std::rc::Weak::new()),
            }
        }

        fn next_id(&self) -> TabId {
            let id = TabId(self.next_id.get());
            self.next_id.set(id.0 + 1);
            id
        }

        /// Adds `view` as tab `id`, wired as the GTK shell wires a tab's view: its navigations
        /// and the windows it opens go through `gate`, as `Tab::may_navigate` sends them, and a
        /// window it opens is a tab too.
        fn add(&self, runtime: &Runtime, id: TabId, view: &webkit::WebView, gate: Gate) -> Rc<RefCell<Gate>> {
            self.container.append(view);
            let gate = Rc::new(RefCell::new(gate));
            let committed = Rc::new(RefCell::new(String::new()));
            view.connect_decide_policy({
                let (gate, runtime, refused) = (gate.clone(), runtime.clone(), self.refused.clone());
                move |_, decision, kind| {
                    let new_window = match kind {
                        webkit::PolicyDecisionType::NavigationAction => false,
                        webkit::PolicyDecisionType::NewWindowAction => true,
                        _ => return false,
                    };
                    let Some(action) = decision.downcast_ref::<webkit::NavigationPolicyDecision>().and_then(|d| d.navigation_action()) else { return false };
                    let Some(target) = action.request().and_then(|r| r.uri()) else { return false };
                    if gate.borrow_mut().decide(&runtime, &target, action.is_redirect(), new_window) {
                        return false;
                    }
                    println!("[harness] host: refused a navigation of tab {} to {target}", id.0);
                    refused.borrow_mut().push(target.into());
                    decision.ignore();
                    true
                }
            });
            view.connect_load_changed({
                let (gate, runtime, committed) = (gate.clone(), runtime.clone(), committed.clone());
                move |view, event| {
                    if event == webkit::LoadEvent::Committed {
                        let uri = view.uri().map(String::from).unwrap_or_default();
                        gate.borrow_mut().committed(&runtime, &uri);
                        *committed.borrow_mut() = uri;
                    }
                }
            });
            vsesvit_webext::connect_create(view, {
                let (gate, runtime, me) = (gate.clone(), runtime.clone(), self.me.borrow().clone());
                move |view, action| {
                    let host = me.upgrade()?;
                    let target = action.request().and_then(|r| r.uri()).map(String::from).unwrap_or_default();
                    if !gate.borrow_mut().decide(&runtime, &target, false, true) {
                        println!("[harness] host: refused a window of tab {} at {target}", id.0);
                        host.refused.borrow_mut().push(target);
                        return None;
                    }
                    let popup_id = host.next_id();
                    let popup = webkit::WebView::builder().related_view(view).user_content_manager(&runtime.user_content_manager(popup_id)).build();
                    let opened = Gate::opened_by(&gate.borrow());
                    host.add(&runtime, popup_id, &popup, opened);
                    println!("[harness] host: tab {} opened tab {} at {target}", id.0, popup_id.0);
                    Some(popup)
                }
            });
            self.tabs.borrow_mut().push(Tab { id, view: view.clone(), committed, gate: gate.clone() });
            gate
        }

        /// The browser's reload, of the document on screen, which the tab's gate lets through
        /// as the GTK tab's does.
        fn reload(&self, tab: TabId) {
            let tabs = self.tabs.borrow();
            let Some(t) = tabs.iter().find(|t| t.id == tab) else { return };
            let committed = t.committed.borrow().clone();
            t.gate.borrow_mut().browser_load(&committed);
            t.view.reload();
        }

        /// The browser's back button, likewise.
        fn go_back(&self, tab: TabId) {
            let tabs = self.tabs.borrow();
            let Some(t) = tabs.iter().find(|t| t.id == tab) else { return };
            if let Some(uri) = t.view.back_forward_list().and_then(|l| l.back_item()).and_then(|i| i.uri()) {
                t.gate.borrow_mut().browser_load(&uri);
            }
            t.view.go_back();
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
            let id = self.next_id();
            let view = webkit::WebView::builder().network_session(&self.session).user_content_manager(&runtime.user_content_manager(id)).build();
            let gate = self.add(&runtime, id, &view, Gate::default());
            self.created.borrow_mut().push(url.to_owned());
            println!("[harness] host: create_tab({url}) -> tab {}", id.0);
            gate.borrow_mut().browser_load(url);
            view.load_uri(url);
            Some(id)
        }

        fn update_tab(&self, tab: TabId, url: Option<&str>, _active: Option<bool>) -> bool {
            let tabs = self.tabs.borrow();
            let Some(t) = tabs.iter().find(|t| t.id == tab) else { return false };
            if let Some(u) = url {
                t.gate.borrow_mut().browser_load(u);
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

    /// A site that answers every request with a redirect to `location` and no Referer, served
    /// until the harness exits. Returns its URL.
    fn bounce_to(location: String) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bounce server");
        let url = format!("http://{}/", listener.local_addr().expect("bounce server address"));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                use std::io::{BufRead, Write};
                let mut reader = std::io::BufReader::new(&stream);
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                let reply = format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nReferrer-Policy: no-referrer\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = (&stream).write_all(reply.as_bytes());
            }
        });
        url
    }

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
                    "default_locale": "en",
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
                    "web_accessible_resources": [{ "resources": ["public.html"], "matches": ["<all_urls>"] }],
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
            ("public.html", "<!doctype html><html><head><meta charset=\"utf-8\"><title>Twin public</title></head></html>".to_owned()),
            ("_locales/en/messages.json", r#"{"cost":{"message":"Total: $AMOUNT$ $1","placeholders":{"amount":{"content":"$1"}}}}"#.to_owned()),
            (
                "inject.js",
                "document.documentElement.dataset.twinInjected = (typeof chrome === \"object\" && chrome.runtime) ? chrome.runtime.id : \"no-chrome\";\n\"injected:\" + document.documentElement.dataset.twinInjected;\n".to_owned(),
            ),
        ]
    }

    /// Every file of the fixture extension `tests/fixtures/extensions/<name>/`.
    fn fixture_files(name: &str) -> Vec<(&'static str, String)> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/extensions").join(name);
        std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|entry| {
                let path = entry.expect("fixture entry").path();
                let name = path.file_name().and_then(|n| n.to_str()).expect("fixture file name").to_owned();
                (&*name.leak(), std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
            })
            .collect()
    }

    /// An MV2 extension without a background: its popup makes its own menu items.
    fn classic_files() -> Vec<(&'static str, String)> {
        vec![
            (
                "manifest.json",
                serde_json::json!({
                    "manifest_version": 2,
                    "name": "Vsesvit Classic",
                    "version": "1.0.0",
                    "permissions": ["contextMenus"],
                    "browser_action": { "default_title": "Vsesvit Classic", "default_popup": "popup.html" }
                })
                .to_string(),
            ),
            ("popup.html", "<!doctype html><html><head><meta charset=\"utf-8\"><title>classic</title></head><body></body></html>".to_owned()),
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
}
