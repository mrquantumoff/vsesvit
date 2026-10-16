# Inventory: the Linux self-test (`--self-test`), its helpers and fixtures

Scope: `crates/vsesvit-gtk/src/self_test.rs` (4212 lines), `crates/vsesvit-gtk/src/test_support.rs` (189 lines), `docs/design/self-test.md` (the cross-shell contract), `tests/fixtures/` (site, extensions, keys), plus the parts of `vsesvit_core::testkit` the self-test is built on (`crates/vsesvit-core/src/testkit/{mod,fixture_server,store,crx_writer,report}.rs`) and the GTK hooks the self-test calls.

Read on the `linux-cef-gpui` worktree at `/home/dy/git/vsesvit-cef`, 2026-10-16. Line numbers are from that checkout.

Conventions:

- Everything stated plainly was read in code or docs. Statements marked *(inference)* are my reasoning, not something I verified.
- Classification of engine-touching pieces:
  - **KEEP**: product behavior the CEF shell must reproduce.
  - **WEBKIT-ONLY**: a WebKitGTK workaround, limitation or WebKit-typed assertion. It goes away with Chromium (the assertion has to be rewritten in engine-neutral terms).
  - **NEEDS-CEF**: needs a CEF equivalent. The CEF names given are candidates. I checked that the Rust method exists in `cef-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs` where it says "(in bindings)".
  - **GTK-ONLY**: a GTK/libadwaita/Wayland mechanism with no engine involvement, which the GPUI shell replaces with its own.
- "Engine-neutral" for a step means it exercises only core, sync and shell UI, so it should pass unchanged in meaning on any engine. "WebKit-specific" means the step as written asserts WebKit types or WebKit behavior.

---

## 1. Entry point and command line

### 1.1 Invocation

```
vsesvit --self-test <OUT_DIR> [--network]
```

- Parsed by `crates/vsesvit-gtk/src/cli.rs::parse`. `--self-test=OUT_DIR` also works. `--network` may come before or after. Rules (from the unit tests `self_test_takes_an_output_dir_and_a_network_flag`, `self_test_is_exclusive_and_network_needs_it`):
  - `--self-test` is exclusive. With URLs or `--profile-dir` it is an error: `CliError::Exclusive("--self-test")`, printed as "--self-test cannot be combined with URLs or other options".
  - Repeating it gives `Repeated`. A second `--network` gives `Repeated("--network")`.
  - `--network` without `--self-test` (including with `--check-for-updates`) gives `NetworkWithoutSelfTest`: "--network needs --self-test".
  - An empty value gives `MissingValue`.
  - Parse errors print `vsesvit: <error>\n\n<USAGE>` to stderr and exit with code **2** (`lib.rs::run`).
- Usage text (`cli::USAGE`): `--self-test OUT_DIR   run the end-to-end self-test on a fresh profile and write report.json and window.png to OUT_DIR`; `--network   with --self-test: also install from the Chrome Web Store`.
- The cargo feature `self-test` is on by default (`crates/vsesvit-gtk/Cargo.toml`: `default = ["self-test"]`, `self-test = ["vsesvit-core/testkit"]`). The reason given there: the self-test is the acceptance check and must run on the same optimized binary that ships. A packager can drop it with `--no-default-features`. Without the feature, `--self-test` prints "vsesvit: this build was made without the `self-test` feature" and exits with failure. The Windows shell does the same (`crates/vsesvit-winui/Cargo.toml`).
- Normal runs also accept `--profile-dir <path>` (self-test.md, line 11). That flag is outside this inventory, but the self-test relies on its sibling idea: never touch the user's profile.

### 1.2 Exit codes and stdout

- Exit code: `ExitCode::SUCCESS` (0) only if `Report::ok()`. Otherwise `ExitCode::FAILURE` (1). It is also 1 if `OUT_DIR` cannot be created or `report.json` cannot be written.
- Stdout lines (exact formats from `self_test.rs`):
  - `[self-test] fixture server on http://127.0.0.1:<port>`
  - one per check: `[self-test] <name>: ok|FAIL (<ms> ms) <detail>` (`record`, line 160)
  - final: `[self-test] PASS|FAIL: <passed>/<total> checks passed; report at <OUT_DIR>/report.json`
  - assorted `[self-test] ...` notes. Example: "cannot keep the probe out of private windows: ..." in a cleanup.

### 1.3 What the self-test process looks like (GTK, `self_test::run`, lines 188-274)

1. `create_dir_all(OUT_DIR)`. On failure: eprintln, then exit 1.
2. `remove_dir_all(OUT_DIR/profile)`, so the profile is always fresh.
3. `crate::SCRIPTED.set(true)`, a thread-local flag (`lib.rs:58`). Effects:
   - No first-run welcome at startup (`browser.rs:183`: `!SCRIPTED && onboarding::should_show(..)`).
   - No scheduled extension update checks (`browser.rs:275`), so they cannot race `extension_update`.
   - **GTK-ONLY:** popovers open with `autohide=false` (`lib.rs::popup`). Wayland refuses an autohide popup grab without a real input event, so scripted popovers stay up to be checked and captured, and take no keyboard focus.
4. `Report::new("linux", expected(network))`.
5. `Profile::open(OUT_DIR/profile, OpenOptions::default())` → check **`profile_open`** (detail `root=<dir>`). On failure it records and finishes.
6. `profile.prefs().set(&keys::SEARCH_SUGGESTIONS, &false)`, so typed text never reaches a real search engine. On failure it records a pseudo-check `search_suggestions_off` and finishes.
7. `FixtureServer::start()`, then `vsesvit_core::favicons::allow_local_hosts()` (lets core's favicon fetcher reach loopback hosts, which `favicon_preload` needs). On failure: pseudo-check `fixture_server`.
8. Writes `testkit::probe_crx()` to `OUT_DIR/probe.crx`. On failure: pseudo-check `probe_crx`.
9. Removes `OUT_DIR/profiles.json`, then builds `Home { dir: ProfilesDir::at(OUT_DIR), id: ProfileId::parse("profile") }`. The profile list lives in the output directory, never the user's.
10. GTK app `adw::Application` with id **`dev.mrquantumoff.vsesvit.SelfTest`** and `NON_UNIQUE` (GTK-ONLY detail). On `startup` it runs `crate::app::setup`, `Browser::new(app, profile, Some(home))`, `browser.start()`, `browser.open_startup_windows(&[])`, then `run_checks` in a local future. Afterwards it closes every window and quits.
    - A fresh profile has `keys::STARTUP` = `RestoreSession` with nothing saved, so startup opens one window with a new tab (`browser.rs:460-485`).
11. `finish`: `report.complete("the self-test stopped before it")`, writes `report.json`, prints the summary, returns the exit code.

Pseudo-checks `search_suggestions_off`, `fixture_server`, `probe_crx` and `window` ("startup opened no window", line 343) are not in the expected list. Recording one as failed makes `Report::ok()` false, because every recorded check must be ok.

### 1.4 The check runner (lines 294-335, 4134-4144)

- `Context::check(name, timeout, body)`: runs `body(last)` under `glib::future_with_timeout(timeout, ..)`. `Ok(detail)` passes and `Err(detail)` fails. On timeout the detail is `timed out after <s> s; last observed: <last>`, where `Last` is a shared string the body updates while polling. Every check is recorded with `ms` = elapsed time.
- Checks run strictly in sequence. A failed check does not stop the run. Each check is written to leave the browser as it found it, mostly through a `Cleanup` drop guard that also runs on failure or timeout.
- `wait_for(last, probe)`: polls every `POLL` = 50 ms until `probe()` is `Ok`, and stores each `Err` in `last`.
- `eval_js(view, script)`: `webkit::WebView::evaluate_javascript_future`. A JS string comes back as is. Other values come back as JSON text. **NEEDS-CEF:** evaluating script in a tab's main frame and getting the value back. CEF's `CefFrame::ExecuteJavaScript` returns nothing. Candidates: the DevTools protocol `Runtime.evaluate` through `CefBrowserHost::ExecuteDevToolsMethod` / `SendDevToolsMessage` with `AddDevToolsMessageObserver` (both in bindings), or a render-process `CefV8Context::Eval` answering by `CefProcessMessage`.
- `wait_js(last, view, script, done)`: polls `eval_js` every 50 ms until `done(value)`.
- `wait_cookies(last, jar, url, done)` / `cookie_names(jar, url)`: WebKit `CookieManager::cookies_future`. **NEEDS-CEF:** `CefCookieManager::VisitUrlCookies` (in bindings as `visit_url_cookies`).
- `tracking_applied(browser)`: waits for `browser.trackers().when_applied(..)`, meaning WebKit content blockers compiled and attached to every tab. **WEBKIT-ONLY**.
- UI finders, all GTK-ONLY: `find::<W>`, `count`, `all`, `button_labelled` (by label, mnemonics included, such as `_Done`), `button_labelled_in_row` (CSS class `bookmark-menu-row`), `popover_of`, `select(combo, label)`, `selected_label(combo)`, `heading_of(popover)` (label with CSS class `heading`), `newest_row_reads`, `cancel_download`, `open_search_engines`.
- Image helpers: `peak_alpha(texture)` (highest alpha 0-255), `distinct_colors(texture, cap)` (distinct RGBA values, stops at `cap`), `char_len` (GTK character positions).
- Constants:

| name | value | used by |
|---|---|---|
| `CHECK_TIMEOUT` | 15 s | default per check |
| `CHECK_TIMEOUT * 2` | 30 s | `private_window`, `download_speed` |
| `NETWORK_TIMEOUT` | 120 s | `cws_install`, and `connection_info` with `--network` |
| `WELCOME_TIMEOUT` | 60 s (+120 s with `--network`) | `welcome` |
| `POLL` | 50 ms | every poll |
| `POPOVER_SETTLE` | 400 ms | wait before capturing an opened popover |
| `CWS_EXTENSION` | `ddkjiahejlhfcafbddmgiahcphecmpfh` (uBlock Origin Lite) | `cws_install` |
| `COLOR_SAMPLE_CAP` | 64 | `screenshot` |
| `DOWNLOAD_FIXTURE` | bytes of `tests/fixtures/site/download.bin` (4096 bytes) | `download` |

- Page scripts the self-test runs (exact strings, lines 70-80): `ASK_LOCATION` (geolocation, writes `data-location` = `ok` or the error code), `LOCATION_OUTCOME`, `QUERY_LOCATION` (`navigator.permissions.query({name:'geolocation'})` → `data-location-state`), `LOCATION_STATE`, `ASK_CAMERA_AND_MICROPHONE` (`getUserMedia({video,audio})`, keeps `window.captured`, writes `data-capture` = sorted `kind:readyState` list or the error name), `CAPTURE_OUTCOME`, `CAPTURED_TRACKS`, `NEW_TAB_PAGE_PROBE` (if `form input` exists, the hrefs of `.tile` links, else `no search box`), `SELECT_HEADING` (selects the `h1`, returns the selection text).
- A reload idiom used in many checks: `eval_js("window.stale = true")`, `view.reload()`, then poll until `!window.stale && document.readyState == 'complete' && ...vsesvitProbe == 'background-replied'`. Engine-neutral apart from eval.

### 1.5 Self-tests of the self-test (lines 4184-4212)

- `a_run_that_stops_early_fails`: after one passed check, `ok()` is false. `complete()` adds `install_prompt` as failed.
- `checks_lists_every_check_run_checks_runs`: scans its own source for `ctx.check("` and asserts that the names, in source order, equal `expected(true)` minus `profile_open`. A new shell needs the same guard so the expected list and the run cannot drift apart.

---

## 2. `report.json` and other output

### 2.1 Format (`vsesvit_core::testkit::report`, used by both shells)

```json
{
  "platform": "linux",
  "ok": true,
  "checks": [ { "name": "navigate", "ok": true, "ms": 812, "detail": "title=Vsesvit fixture" } ]
}
```

- `Report::new(platform, expected)`: `platform` is `"windows"` or `"linux"`. `expected` lists every check in run order.
- `push(Check { name: &'static str, ok, ms: u128, detail })`. `ms` is serialized as u64, saturating.
- `complete(reason)`: adds `{ok: false, ms: 0, detail: "not run: <reason>"}` for each expected check not yet recorded. GTK passes "the self-test stopped before it".
- `ok()`: every expected check has a passing entry **and** every recorded entry is ok.
- `write(out_dir)`: pretty-printed JSON at `<out_dir>/report.json`.

### 2.2 Files the Linux run leaves in `OUT_DIR` (KEEP as a reviewer artifact set)

`report.json`, `window.png`, `probe.crx`, `profile/` (the fresh profile), `profiles.json` (the test's profile list), `bookmarks.html`, `saved-page.mhtml`, `print.pdf`, `view-source-lure.html`, `downloads/` (`download.bin`, `dangerous.sh`, a private download of `download.bin`, plus whatever the cancelled `slow.bin`/`stalled.bin` left).

Screenshots, in check order: `install-prompt.png`, `favicon-preload.png`, `star-bubble.png`, `tab-opened.png`, `tab-menu.png`, `tab-pinned.png`, `tab-search.png`, `tab-group-editor.png`, `tab-group-collapsed.png`, `tab-asleep.png`, `extensions-context-menu.png`, `extensions-menu.png`, `extensions-unpinned-popup.png`, `extension-shortcuts.png`, `extension-notifications.png`, `extension-update.png`, `address-completion.png`, `search-suggestions.png`, `downloads.png`, `private-window.png`, `new-tab.png`, `address-progress.png`, `home-button.png`, `settings-general.png`, `settings-sync.png`, `settings-appearance.png`, `settings-search.png`, `settings-privacy.png`, `settings-shortcuts.png`, `settings-spell-check.png`, `search-engine-editor.png`, `settings-search-engines.png`, `bookmarks-overflow.png`, `bookmarks-overflow-menu.png`, `bookmarks-folder-menu.png`, `bookmarks-context-menu.png`, `bookmarks-edit-dialog.png`, `sidebar-hidden.png`, `shortcuts-edited.png`, `shortcut-capture.png`, `zoom.png`, `connection-http.png`, `connection-https.png` (`--network` only), `permission-prompt.png`, `site-info-permissions.png`, `settings-site-permissions.png`, `settings-site-permissions-empty.png`, `site-info-trackers.png`, `https-only-warning.png`, `site-info-cookies.png`, `settings-site-data.png`, `capture-in-use.png`, `capture-in-use-top.png`, `site-info-capture.png`, `sync-passphrase-offer.png`, `sync-passphrase.png`, `profiles-account.png`, `profiles-manage.png`, `profiles.png`, and `welcome-<step>.png` plus `welcome-<step>-dark.png` for each of `welcome`, `search`, `import`, `extensions`, `default-browser`, `sync`, `done`.

### 2.3 Screenshots: how they are taken (`crates/vsesvit-gtk/src/screenshot.rs`)

- `save_png(widget, path)`: renders the widget through its own window's GSK renderer (`gtk::WidgetPaintable` → `gtk::Snapshot` → `renderer.render_texture`) right after the next frame paints (`FrameClock::connect_after_paint`). It retries up to 10 more frames while it gets `NothingDrawn`. Reads nothing outside the app, needs neither focus nor the window on top. This is what self-test.md means by "an in-app capture that does not steal focus".
- `save_png_with_popovers(window, popovers, path)`: popovers are separate Wayland surfaces, so each is captured on its own and composited at its surface position (`gdk::Popup::position_x/y`, summed along the parent chain). The image grows to include popovers that reach past the window.
- Errors: `NotRealized`, `NothingDrawn`, `Save`.
- **KEEP:** in-app PNG capture of a browser window that includes web content and open menus or popovers, without stealing focus.
- **NEEDS-CEF + GPUI:** the pixels of a CEF browser hosted in a GPUI window. *(inference)* If CEF renders into its own native child window (windowed mode, X11 under XWayland), a GPUI-side capture will not contain the page. Off-screen rendering (`CefRenderHandler::OnPaint` / `OnAcceleratedPaint`, in bindings) gives the shell the page pixels. DevTools `Page.captureScreenshot` gives page-only pixels. Popovers drawn by GPUI must be captured by GPUI.

---

## 3. Inputs: `vsesvit_core::testkit` (feature `testkit`)

Everything is embedded at compile time with `include_bytes!`, so it works from any working directory (`testkit/mod.rs` doc).

### 3.1 `FixtureServer` (`testkit/fixture_server.rs`)

- `FixtureServer::start() -> io::Result<FixtureServer>`: binds `127.0.0.1:0`, one accept thread, one thread per connection, one response per connection (`Connection: close`, `Cache-Control: no-store`, so every load hits the server). Idle connections are dropped after 5 s (engines open speculative connections). A connection whose first byte is a TLS handshake (`0x16`) is closed at once, so an https load of the plain server fails fast. `https_only` relies on this.
- `port()`, `origin()` (`http://127.0.0.1:<port>`), `url(path) -> Url`.
- `hits() -> Vec<String>`: every request path in arrival order, **query stripped**, 404s included. Absolute-form targets keep only the path.
- `route(path_prefix, handler)`: added routes come before site files. When two match, the one added last answers. Handlers get `FixtureRequest { method, target, body }` (`path()`, `query()`) and return `FixtureResponse { status, content_type, body }` (`FixtureResponse::ok(ct, body)`, `FixtureResponse::status(code)`).
- Built-in responses:

| path | response |
|---|---|
| `/` | same as `/index.html` |
| `/index.html`, `/page2.html`, `/media.html`, `/trackers.html`, `/cookies.html`, `/cookie-frame.html`, `/dnr.html`, `/frames.html` | the files in `tests/fixtures/site/`, `text/html; charset=utf-8` |
| `/allowed.png`, `/vsesvit-blocked/pixel.png`, `/tracker/pixel.png` | PNG files (69 bytes each) |
| `/download.bin` | 4096 bytes, `application/octet-stream` |
| `/dangerous.sh` | `#!/bin/sh\necho Vsesvit fixture\n`, `application/octet-stream` |
| `/dangerous.bat` | `@echo Vsesvit fixture\r\n` (Windows) |
| `/icon.html` | `<title>Icon</title><link rel="icon" href="allowed.png">` |
| `/suggest?q=<t>` | `application/x-suggestions+json`: `["<t>",["<t> one","<t> two"]]` |
| `/set-cookie` | `<title>Cookie set</title>` with `Set-Cookie: served=1; Path=/` |
| `/stalled.bin` | announces 1,000,000 bytes, sends 1,000 zero bytes (`STALLED_SENT`), then nothing until the client closes |
| `/slow.bin` | announces 10,000,000 bytes, sends 25,000 every 100 ms (about 250 KB/s for 40 s) |
| anything else | `404 not found` |

- Also exported: `reads_speed_and_time_left(line)`. True when the line looks like `"<speed>/s - <x> of <y>, <...> left"` with a speed other than `0 B/s`.

### 3.2 `FixtureStore` (`testkit/store.rs`): stand-in Chrome Web Store, Edge Add-ons and AMO

- `FixtureStore::start(&server)` adds routes `/store/cws/update` (gupdate: `response=redirect` returns the CRX or 204; otherwise XML `<gupdate ...><app appid status="ok"><updatecheck codebase hash_sha256 status="ok" version/>`, or `status="error-unknownApplication"`), `/store/edge/update` (Omaha 3.1 JSON behind `)]}'`), `/store/crx/<id>.crx`, `/store/amo/api/...` and `/store/amo/file/<sha256>.xpi`.
- `stores() -> Stores { cws_update_url, edge_update_url, amo_api_url, cws_publisher_key_sha256, edge_publisher_key_sha256, https_only: false }`. The publisher key is a deterministic P-256 key, seed 91 (`CrxKey::ecdsa(91)`), standing in for Google's and Microsoft's. Installed with `Profile::set_stores(..)`.
- `publish_crx(files, developer: &CrxKey) -> ExtensionId`: a CRX3 signed by the developer key and countersigned by the fixture publisher key. It replaces whatever was published for that id. Every update check answers with the newest published version; deciding whether it is newer is core's job.
- `publish_xpi(guid, files)`: not used by the self-test.

### 3.3 CRX writing (`testkit/crx_writer.rs`) and the probe

- `CrxKey::probe()` is `tests/fixtures/keys/test-only-probe-key.pem` (RSA-2048) → id **`eonajgebgeenbhiiobbhmkafolkeghdb`** = `testkit::PROBE_ID`.
- `CrxKey::second()` is `test-only-second-key.pem` → `kpjcccehndkfpfbbofnjkcelnoonmhja`. It signs the update probe.
- `CrxKey::ecdsa(seed)`, `public_key_der`, `crx_id`, `extension_id`, `key_sha256`, `proof`.
- `zip_files` (deterministic deflate zip), `encode_crx3`, `sign_crx3`, `write_crx3`.
- `testkit::PROBE_FILES`: probe `manifest.json`, `background.js`, `content.js`, `dynamic.js`, `popup.html`, `popup.js`, `rules.json`. Not `_metadata/`, which Chromium writes into an unpacked source directory.
- `testkit::probe_crx()`: `write_crx3(PROBE_FILES, &CrxKey::probe())`. Deterministic, so installing twice is a same-bytes reinstall.
- `testkit::update_probe_files(version, permissions)`: builds the update probe's MV3 manifest (name `Vsesvit update probe`, `background.service_worker`, content script on `http://127.0.0.1/*` at `document_end`) plus `update-probe/background.js` and `content.js`.

### 3.4 The keys (`tests/fixtures/keys/`)

`README.md`: both RSA-2048 keys are committed on purpose and therefore public. They are only for signing CRX3 in tests and self-tests. Format: PKCS#8 PEM with a one-paragraph label before `-----BEGIN`, which `CrxKey::rsa_pem` skips.

---

## 4. Fixtures (`tests/fixtures/`)

### 4.1 Site pages (`tests/fixtures/site/`)

| file | title | what it does | used by |
|---|---|---|---|
| `index.html` | `Vsesvit fixture` | `<h1>Vsesvit fixture page</h1>`; `<img src="/allowed.png">` (control) and `<img src="/vsesvit-blocked/pixel.png">` (blocked by the probe's DNR); link to `/page2.html` | most checks |
| `page2.html` | `Vsesvit fixture 2` | `<h1>Second fixture page</h1>`, link back | tabs, menus, search, groups, private |
| `media.html` | `Media` | canvas plus quiet oscillator as a `<video>` stream, `window.start()` plays it; `mediaSession` metadata `Smoke tone`/`Vsesvit`, `nexttrack` handler sets title `Next track`; moves the player when narrow | `tab_search` (only as a page titled "Media") |
| `trackers.html` | `Trackers` → `tracker loaded` / `tracker blocked` | loads `http://localhost:<port>/tracker/pixel.png` | `tracking_protection` |
| `cookies.html` | `Cookies` → `cookies:ready` | sets `first=1; path=/`, `window.cookieResults = {page: document.cookie}`, embeds `http://localhost:<port>/cookie-frame.html`, sets the title once the frame posts its cookies | `cookies` |
| `cookie-frame.html` | `Cookie frame` | sets `third=1; SameSite=None; Secure; path=/`, posts `{cookieFrame: document.cookie}` to the parent | `cookies` |
| `dnr.html` | `DNR` → `done:<run>` | five images `/dnr/<run>/{static,extra,dynamic,session,browser}.png` | webext harness only |
| `frames.html` | `Vsesvit frames` | same-origin iframe `page2.html` plus a `localhost` iframe | webext harness only |
| `allowed.png`, `tracker/pixel.png`, `vsesvit-blocked/pixel.png` | n/a | tiny PNGs (69 bytes) | DNR, trackers, favicon, account picture |
| `download.bin` | n/a | 4096 bytes | `download`, `private_window` |

### 4.2 The probe extension (`tests/fixtures/extensions/probe/`), installed as CRX `PROBE_ID`

- `manifest.json`: MV3, `"name": "Vsesvit Probe"`, version `1.0.0`. `permissions`: `storage`, `declarativeNetRequest`, `contextMenus`, `notifications`, `scripting`, `webNavigation`, `cookies`. `host_permissions`: `<all_urls>`. Service worker `background.js`. Content script `content.js` on `<all_urls>` at `document_end`. `action` with `default_title` "Vsesvit Probe" and `default_popup` `popup.html`. DNR ruleset `block` (`rules.json`, enabled). `commands`: `_execute_action` with suggested `Alt+Shift+P`, and `probe-command` with suggested `Alt+Shift+K`, description "Vsesvit Probe command".
- `rules.json`: rule 1, priority 1, `block` when `urlFilter: "/vsesvit-blocked/"` matches resource types `image`, `script`, `xmlhttprequest` or `sub_frame`.
- `popup.html` / `popup.js`: reads `storage.local.visits` and sets `document.title = "visits=<n>"`.
- `dynamic.js`: sets `data-vsesvit-probe-dynamic` to `"isolated"` if `chrome.runtime` exists, else `"main"`.
- `content.js`: sets `data-vsesvit-probe="content-script"`, then sends `{type:"hello"}` and on a reply sets `data-vsesvit-probe="background-replied"` and `data-vsesvit-visits`. It mirrors `storage.onChanged` keys `menuClick`, `command` and `notification` into `data-vsesvit-probe-menu`, `-command` and `-notification` (as JSON). It opens `runtime.connect({name:"probe"})`, posts `"ping"`, and sets `data-vsesvit-probe-port` to the reply. It runs one `MutationObserver` per request attribute (table below). Each observer deletes the request attribute, sends a message, and writes the JSON answer (or `{error}`) to the answer attribute.
- `background.js`: on install it creates context menu items `probe-page` ("Vsesvit Probe page item", contexts `["page"]`) and `probe-action` ("Vsesvit Probe action item", contexts `["action"]`). `contextMenus.onClicked` stores `menuClick = {id, pageUrl|null, tab|null, at}`. `commands.onCommand` stores `command = {name, tab|null, url|null, at}`. The notification listeners store `notification = {event: clicked|button|closed|level, ...}`. `runtime.onConnect` replies `"pong:" + port.name` to `"ping"`. The message handlers:

| request attribute set by the self-test | message | background does | answer attribute / JSON |
|---|---|---|---|
| (none, on every load) | `{type:"hello"}` | `storage.local.visits += 1` | `data-vsesvit-probe="background-replied"`, `data-vsesvit-visits=<n>` |
| `data-vsesvit-notify` (`keep` keeps the notification shown) | `{type:"notify", keep}` | `notifications.create("probe-notification", {type:"basic", iconUrl: <16px PNG data URL>, title:"Vsesvit Probe notification", message:"Sent by the probe", buttons:[{title:"Open"}]})`; without `keep` it clears at once | `data-vsesvit-probe-notified`: `{api, created}`, `{api:"undefined"}` or `{api, error}` |
| `data-vsesvit-dnr` = `allow` / `clear` | `{type:"dnr", want}` | `allow`: `updateDynamicRules` adds rule 1, priority 2000000, `allowAllRequests`, condition `{requestDomains:[page host], resourceTypes:["main_frame"]}` (uBlock Origin Lite's "no filtering on this site"); `clear`: removes rule 1 | `data-vsesvit-probe-dnr`: `{"rules":[1]}` / `{"rules":[]}` |
| `data-vsesvit-scripts` = `register` / `unregister` | `{type:"scripts", want}` | `scripting.registerContentScripts([{id:"probe-dynamic", matches:["*://<host>/*"], js:["dynamic.js"], persistAcrossSessions:false}])` / `unregisterContentScripts({ids:["probe-dynamic"]})` | `data-vsesvit-probe-scripts`: `{"scripts":["probe-dynamic"]}` / `{"scripts":[]}` |
| `data-vsesvit-windows` = `open` / `move` / `close` | `{type:"windows", want}` | `open`: clears the event log, `windows.create({url: sender.url})` → `{window, tabs}`; `move`: `tabs.move(probeTab, {windowId: sender.tab.windowId, index:-1})` → `{tab, window, index}`; `close`: `windows.create({tabId: probeTab})`, `windows.remove(w.id)` → `{window, windows: getAll ids, events}`. Events logged: `["windows.onCreated", id]`, `["windows.onRemoved", id]`, `["tabs.onDetached", tabId, info]`, `["tabs.onAttached", tabId, info]` | `data-vsesvit-probe-windows` |
| `data-vsesvit-navigation` = `watch` / `report` | `{type:"navigation", want}` | `watch`: records webNavigation events of the sender's tab (`onBeforeNavigate`, `onCommitted`, `onDOMContentLoaded`, `onCompleted`, `onErrorOccurred`, `onHistoryStateUpdated`, `onReferenceFragmentUpdated`) as `[name, frameId, url, transitionType|null]` → `{tab}`; `report`: waits for top-frame `onCompleted` (5 s cap) → `{events, frames: getAllFrames → [frameId, parentFrameId, url]}` | `data-vsesvit-probe-navigation` |
| `data-vsesvit-cookies` = `set` / `remove` | `{type:"cookies", want}` | `set`: `cookies.set({url, name:"vsesvit-probe", value:"1"})`, `getAllCookieStores()` → `{cookie:[name,value,domain,storeId], inStore: store "0" lists the tab}`; `remove`: `cookies.remove`, `cookies.get` → `{left, events:[[removed, cause, value]...]}` (waits up to 5 s for 2 events) | `data-vsesvit-probe-cookies` |

### 4.3 The update probe (`tests/fixtures/extensions/update-probe/`)

The manifest is generated by `testkit::update_probe_files`. `background.js` answers the message `"update-probe"` with `"<running version>:<first version>"`, keeping `first` in `storage.local`. `content.js` writes the answer to `data-vsesvit-update-probe`. So `"2.0:1.0"` means an update kept the storage, and `"2.0:2.0"` means it lost it.

### 4.4 Other fixture extensions (used by `crates/vsesvit-webext/examples/harness.rs`, not by the self-test)

These cover what the self-test's single probe cannot. They are the most detailed WebExtensions compatibility spec in the repo, so they are useful whichever runtime replaces vsesvit-webext. The harness itself (2915 lines, WebKit-only) is listed in its header (lines 1-117) as 18 sections.

| dir | manifest essentials | harness section it serves |
|---|---|---|
| `commands/` | MV3 "Vsesvit Commands", gecko id `commands@vsesvit.test`, `activeTab`+`storage`; commands `_execute_action` Alt+Shift+A, `free` Alt+Shift+F, `taken` Ctrl+T (the browser's key), `keyless` | 10: `commands.getAll`, resolution against the browser's keys, `onCommand` + `activeTab` |
| `cookies/` | MV3, `cookies`; hosts `http://127.0.0.1/*`, `http://*.blocked.test/*`, `http://open.test/*` | 16: set/get/getAll/remove/getAllCookieStores, errors, `onChanged`, private store "1" |
| `dnr/` | MV3, `declarativeNetRequest`+`storage`, `<all_urls>`; rulesets `base` (on) and `extra` (off) | 12: enable/disable rulesets, dynamic/session rules, `allowAllRequests`, `isRegexSupported`, persistence |
| `menus/` | MV3, `contextMenus`+`storage` | 9: contexts, `documentUrlPatterns`, `targetUrlPatterns`, `%s`, checkboxes, action menu, persistence |
| `navigation/` | MV3, `webNavigation` | 15: event order across frames, filters, `getFrame`/`getAllFrames`, history and fragment navigations, errors |
| `notifications/` | MV3, `notifications`+`storage`, `icon.svg` | 11: create/update/clear/getAll, Chrome's Linux layout, buttons, the per-extension switch |
| `permissions/` | MV3, `scripting`+`storage`; optional `tabs`, `alarms`, `cookies`, optional host `http://127.0.0.1/*`; `mark.js` | 17: `permissions.request/contains/remove`, prompts, grants reaching scripts, cookies and fetch |
| `ports/` | **MV2**, `tabs` + `http://127.0.0.1/*`, background scripts, `browser_action`, content script on `http://127.0.0.1/index.html`, `externally_connectable.ids: ["friend@vsesvit.test"]` | 8: `runtime.connect`, `tabs.connect`, disconnects, cross-extension messaging, `getBackgroundPage` |
| `ports-friend/` | MV3 "Vsesvit Ports Friend", gecko id `friend@vsesvit.test` | 8 |
| `scripts/` | MV3, `scripting`, host `http://127.0.0.1/*`; `isolated.js`, `main.js`, `updated.js`, `style.css`, `inserted.css` | 13: register/update/unregister content scripts, worlds, CSS insert/remove, persistence |
| `windows/` | MV3, no permissions | 14: `windows.*`, `tabs.create`/`move` across windows, Chrome's error and event order |

Compatibility notes on these fixtures *(inference)*: every one carries `browser_specific_settings.gecko.id`, which Chromium ignores (a harmless manifest warning). Chromium gives an unpacked extension without `key` a path-derived 32-letter id. `ports/` uses `externally_connectable.ids: ["friend@vsesvit.test"]`, a Gecko-style id that Chromium can never match. Under Chromium that fixture needs the friend's real id (or a `key`). MV2 (`ports/`) is unsupported in current Chromium builds unless MV2 support is kept on. Check what CEF 154 allows.

---

## 5. Check-by-check inventory, in run order

Order and names come from `CHECKS` (62 entries, `self_test.rs:83-146`) plus `cws_install` with `--network`. `profile_open` is §1.3 step 5. Each entry gives: what it proves for the user; steps and assertions; artifacts; core/sync APIs; shell hooks; state touched and restored; engine classification; delta from self-test.md.

### 5.1 `profile_open` (line 199)
- Proves core opens a new profile. `Profile::open(OUT_DIR/profile, OpenOptions::default())` succeeds. Detail `root=<path>`.
- **Engine-neutral.** KEEP.

### 5.2 `install_prompt` (line 352, 15 s)
- User-visible: installing a package through the Extensions dialog's path shows Chrome's install prompt before anything is committed. Heading `Add “Vsesvit Probe”?`, then the lines `It can:`, `• Read and change all your data on all websites`, `• Display notifications`. Buttons `_Cancel` (default and close response) and `_Add Extension` (suggested), from `dialogs/extension_prompts.rs::install_dialog`.
- Steps: `InstallSource::from_path(probe.crx)`. Spawn `browser.install_asking(&window, source, |_| {})`. Wait for `window.visible_dialog()` to be an `adw::AlertDialog`. Read `extension_prompts::shown_lines` (heading plus each label of the extra child). Wait 400 ms, capture `install-prompt.png`, `dialog.close()`.
- Assert: the lines equal exactly those four, the install returned `Err(InstallFailure::Cancelled)`, and `browser.installed_extensions()` does not contain `PROBE_ID`.
- Core: `vsesvit_core::extensions::{InstallSource::from_path, ExtensionsStore::prepare_install → InstallJob, StagedInstall::needs_approval, StagedInstall::manifest}`, `extensions::permissions::{install_heading, install_warnings, INSTALL_LEAD}` (Chrome's wording and coalescing), `extensions().commit` (not reached), `extensions().list()`.
- Engine: **engine-neutral** apart from the shell dialog. **WEBKIT-ONLY:** the prompt's extra line "Not supported by the Linux runtime: ..." (`extensions::unsupported_notice` via `vsesvit_webext::unsupported_features`) is absent for the probe and disappears with Chromium.
- Doc: matches self-test.md (Linux row; Windows' equivalent lives in its ui-smoke step `15c-extensions-install-prompt`).

### 5.3 `install_crx` (line 374)
- Proves CRX3 parse, verify, unpack, `key` injection and commit through core, then hand-off to the engine. `browser.install(InstallSource::from_path(probe.crx), |_| {})` (no prompt) returns `Some(InstalledExtension)` with `id == PROBE_ID` and `verification == Verification::LocalCrx`. Detail `id=.. verification=.. dir=..`.
- Core: `extensions().prepare_install`, `InstallJob::run` (off the UI thread), `extensions().commit(staged)`, then `apply_keymap` (the extension commands' accelerators) and `load_into_runtime(ext)`. A load failure returns `InstallFailure::Load`, which fails the check.
- Engine: core part **engine-neutral**. Loading the committed directory into the engine is **NEEDS-CEF** (see 5.4).

### 5.4 `engine_loaded_extension` (line 386)
- Linux: `browser.runtime().loaded()` (vsesvit-webext) contains `PROBE_ID`. Then it waits until `runtime().pending_filters() == 0`, meaning the DNR ruleset has compiled into a WebKit content blocker, so that `dnr_blocked` cannot race.
- Windows equivalent: `AddBrowserExtensionAsync` returns id `PROBE_ID` (proving the injected `key`) and the engine lists it as enabled.
- Classification: the runtime list is **NEEDS-CEF**: ask Chromium which extensions it has loaded, and assert the id `PROBE_ID`, which proves core's `key` injection as Windows does. The `pending_filters` wait is **WEBKIT-ONLY**: Chromium compiles DNR itself. *(inference)* CEF 154's bindings have no `CefRequestContext::LoadExtension` or `GetExtensions` (`grep` finds only `get_extensions_for_mime_type`). How an unpacked directory gets into a Chrome-style CEF profile at runtime is an open question for the extensions research (candidates: the `--load-extension` switch at startup, DevTools `Extensions.loadUnpacked` behind `--enable-unsafe-extension-debugging`, the `chrome://extensions` WebUI, or patching CEF).

### 5.5 `favicon_preload` (line 402), not in self-test.md
- User-visible: a bookmark on the bookmarks bar gets its site's icon without a tab ever visiting it. Core fetches the page and its icon.
- Steps: `core.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Icon page", <origin>/icon.html)`, then `browser.bookmarks_changed()`. Wait until `core.favicons().get(url)` is `Some` **and** `window.bookmarks_bar().shows_favicon(url)`. Capture `favicon-preload.png`. Remove the bookmark.
- Detail notes whether the server saw `/icon.html` (fetched by core). Not asserted.
- Core: `bookmarks()`, `favicons().get`, core favicon fetch (`vsesvit_core::favicons::FaviconFetch`, `allow_local_hosts()`), `browser.preload_favicons()` at startup.
- **Engine-neutral.** KEEP.

### 5.6 `navigate` (line 421)
- `window.address_bar().submit_text(<origin>/index.html)` (a test hook: sets the edit text and submits, as if typed plus Enter). Wait until the selected tab's title is `Vsesvit fixture` and its committed URI equals the URL. Detail `title=Vsesvit fixture`.
- Engine: KEEP. **NEEDS-CEF:** load a URL (`CefFrame::LoadURL`), title (`CefDisplayHandler::OnTitleChange`), committed address (`OnAddressChange` / `CefLoadHandler::OnLoadStart` and `OnLoadEnd`, in bindings).

### 5.7 `history_recorded` (line 438)
- Wait until `core.history().search(<url>, 20)` holds an entry whose `url` equals the fixture URL. Detail has `visits` and `title`.
- Core: `vsesvit_core::history` (`search`). The shell records a visit on each committed navigation, with a `vsesvit_core::history::Transition`.
- Engine: KEEP. **NEEDS-CEF:** commit notification plus transition type (`CefNavigationEntry::GetTransitionType` via `CefBrowserHost::GetVisibleNavigationEntry`, or the transition passed to `CefLoadHandler::OnLoadStart`).

### 5.8 `content_script` (line 452)
- Polls `document.documentElement.dataset.vsesvitProbe` every 100 ms until it is `background-replied`. The detail adds `vsesvitVisits`.
- Proves the content script, `runtime.sendMessage`, the MV3 service worker and `storage.local`.
- Engine: KEEP (native in Chromium). Needs the page-eval hook (§1.4).

### 5.9 `extension_port` (line 468)
- Polls `String(dataset.vsesvitProbePort)` until it is `pong:probe` (`runtime.connect` plus `Port` both ways).
- KEEP; native in Chromium.

### 5.10 `dnr_blocked` (line 481)
- Waits 1 s, then asserts the server's hits include `/allowed.png` and not `/vsesvit-blocked/pixel.png`. Detail lists every hit.
- KEEP; **engine-neutral observation** (at the fixture server). Native DNR in Chromium.

### 5.11 `dnr_site_allowed` (line 491)
- Sets `data-vsesvit-dnr='allow'`; the answer must be `{"rules":[1]}`. Reload (stale idiom); wait until the count of `/vsesvit-blocked/pixel.png` hits grows. Sets `'clear'`; the answer must be `{"rules":[]}`. Reload, wait 1 s; the count must not change.
- Proves dynamic rules, and an `allowAllRequests` rule at priority 2000000 above a static block, as uBlock Origin Lite does.
- KEEP; native.

### 5.12 `dynamic_content_script` (line 527)
- `data-vsesvit-scripts='register'` → `{"scripts":["probe-dynamic"]}`; after a reload, `dataset.vsesvitProbeDynamic == "isolated"` (it ran in the extension's world). `'unregister'` → `{"scripts":[]}`; after a reload the attribute is `undefined`.
- KEEP; native. Doc says "Linux so far".

### 5.13 `extension_windows` (line 557)
- `open`: the probe answers `{window: <new id>}`. Wait until the shell window with that id holds exactly one tab committed at `index.html`; remember that tab's id.
- `move`: wait until window `<new>` is gone and the tab is in the main window. Assert the answer equals `{"tab": <tab id>, "window": <main id>, "index": <its index>}`.
- `close`: the answer's `window` is another new id. Wait until that window and the tab are both gone. Reselect the original tab. Assert the answer's `windows` equals the sorted list of the browser's window ids, and its `events` equal exactly:
  `[["windows.onCreated",new],["tabs.onDetached",tab,{"oldWindowId":new,"oldPosition":0}],["tabs.onAttached",tab,{"newWindowId":main,"newPosition":idx}],["windows.onRemoved",new],["windows.onCreated",other],["tabs.onDetached",tab,{"oldWindowId":main,"oldPosition":idx}],["tabs.onAttached",tab,{"newWindowId":other,"newPosition":0}],["windows.onRemoved",other]]`.
- User-visible behavior: `chrome.windows` and `chrome.tabs` operate on the browser's real (shell) windows and tabs. A window emptied by `tabs.move` closes. `windows.create({tabId})` moves an existing tab into a new window.
- Shell hooks: `BrowserWindow::id()` (u32) **is** the extension window id, and `Tab::id().0` **is** the extension tab id. The check compares them directly.
- Engine: KEEP. **NEEDS-CEF, high risk** *(inference)*: Chromium's `chrome.windows` and `chrome.tabs` operate on Chrome `Browser` objects and their `TabStripModel`. A CEF Chrome-style browser in a GPUI window is not a multi-tab Chrome window, and CEF exposes neither Chromium's window/tab session ids nor a hook to implement `tabs.move` between shell windows. Matching shell ids to extension-visible ids is needed by this check and by `extension_web_navigation`, `extension_cookies`, `context_menus` and `extension_commands`, which all compare against `tab.id()`.

### 5.14 `extension_web_navigation` (line 617)
- `watch` → `{tab: <selected tab id>}`. Sets `window.stale`, activates `win.reload`, waits for the fresh load. `report` must equal `{"events":[["onBeforeNavigate",0,url,null],["onCommitted",0,url,"reload"],["onDOMContentLoaded",0,url,null],["onCompleted",0,url,null]],"frames":[[0,-1,url]]}`.
- KEEP. Native webNavigation in Chromium; the shell's Reload must produce transition `reload`. Tab-id mapping as in 5.13.

### 5.15 `extension_cookies` (line 647)
- `set` → `{"cookie":["vsesvit-probe","1",<host 127.0.0.1>,"0"],"inStore":true}`; then the page's `document.cookie` contains `vsesvit-probe=1`. `remove` → `{"left":null,"events":[[false,"explicit","1"],[true,"explicit","1"]]}`; then `document.cookie` lacks it.
- KEEP; native `chrome.cookies` over the profile's cookie store (store id "0"). `getAllCookieStores` listing the tab in store "0" needs the tab mapping (5.13).

### 5.16 `bookmark` (line 673)
- Activates `win.bookmark-page`. Wait until `core.bookmarks().is_bookmarked(index)`, `window.bookmarks_bar().button_for(index)` exists, and the `bookmark-page` action's boolean state is true (star active).
- User-visible: the star bookmarks the page into the bookmarks bar, and the bar shows it at once.
- Core: `bookmarks().is_bookmarked`, `add_url`. **Engine-neutral.**

### 5.17 `star_bubble` (line 690), not in self-test.md
- Right after `bookmark`: `address_bar().bubble()` is open, headed `Bookmark added`. Its Name entry has the whole text selected. Capture `star-bubble.png`. Set the name to `Renamed fixture`, click `_Done`. Wait until `core.bookmarks().find_by_url(index)` titles equal `["Renamed fixture"]`. Activate `win.bookmark-page` again: the bubble is headed `Edit bookmark`. Pop it down.
- **GTK-ONLY note:** the selection is checked instead of focus, because scripted popovers take no keyboard focus.
- **Engine-neutral.** KEEP.

### 5.18 `bookmark_export` (line 724)
- Activates `win.show-bookmarks`; `browser.windowed(Windowed::Bookmarks)` is the Bookmarks window. Its main menu (`MenuButton` with icon `open-menu-symbolic`) model holds exactly `[("_Import Bookmarks…","bookmarks.import"),("_Export Bookmarks…","bookmarks.export")]`.
- Then `dialogs::bookmarks::export_to(browser, OUT_DIR/bookmarks.html)`, the step that runs after the save dialog (`vsesvit_core::export::html(&bookmarks)` written off-thread). Read the file back with `vsesvit_core::import::parse_html`; it must contain `("Renamed fixture", <index url>)`.
- **Engine-neutral.** KEEP.

### 5.19 `tabs` (line 751)
- `window.open_tab(Some(page2), None, Focus::Foreground)`. Wait for title `Vsesvit fixture 2` with that tab selected. `select_tab(first)`, `close_tab(second)`. Wait for 1 tab, the first selected, on `index.html`.
- KEEP (titles come from the engine).

### 5.20 `tab_animation` (line 780)
- User-visible: a tab's row in the sidebar appears at once and fades or grows in when animations are on. A closed tab's row leaves the live list at once while it shrinks out. Reopening restores the page.
- Steps and asserts: wait until settled, meaning `tab_list().row_counts() == (tabs, 0, true)`. Open page2 in the foreground: tabs+1 and live rows+1 immediately, the new row's opacity < 1.0 if `gtk-enable-animations` (any value if not). Settled: its opacity == 1.0. Capture `tab-opened.png`. Close it: tab count and live rows back to the original immediately, 1 row leaving if animations are on (else 0). `browser.reopen_closed_tab(window)` selects a new tab that commits `page2.html`. Close it again; settled rows == tabs.
- **GTK-ONLY** measurement (revealer opacity, the GTK animations setting). The product behavior is KEEP and engine-neutral.

### 5.21 `tab_layout` (line 842)
- A fresh profile classifies as `TabsPosition::Left`. Then `browser.set_tabs_position(Right | Top | Left)`, each confirmed with `classify_layout(window.layout_probe())`. Ends with stored `tabs.position == Left`.
- `LayoutProbe { width, sidebar: Option<Rect>, tab_bar: Option<Rect>, web_view: Option<Rect> }` in window coordinates. `window/layout.rs::classify` uses 0.5 px slack. Left: sidebar at x≈0, its right edge ≤ web view x, web view reaching the window's right edge. Right: mirrored. Top: no sidebar, tab bar bottom ≤ web view top, web view spanning the full width. A frame caught mid-animation classifies as nothing.
- Prefs: `tabs.position` (synced, default `left`).
- KEEP. **NEEDS-CEF:** the web view's bounds inside the GPUI window (the CEF view or child-window rect).

### 5.22 `tab_menu` (line 870)
- Opens a second tab in the background on `/page2.html?utm_source=self-test&a=1`. The clean form is `/page2.html?a=1`.
- First tab's menu (`window.open_tab_menu(tab)`): capture `tab-menu.png`. `window.tab_menu_lines()` must equal three sections:
  1. `_New Tab Below`, `_Add Tab to New Group`, `Move Tab to New _Window`
  2. `_Reload`, `_Duplicate`, `P_in Tab`, `_Mute Tab`, `Copy _Link`
  3. `_Close Tab`, `Close _Other Tabs`, `Close Tabs _Below`, `R_eopen Closed Tab`

  All enabled except Reopen, which is enabled iff `browser.can_reopen_closed_tab(Browsing::Normal)`.
- `tab.copy-link` on the second tab copies the clean URL. `win.copy-link` (second tab selected) copies the whole tracked URL. `win.copy-clean-link` is bound to `<Control><Shift>c` and copies the clean URL. `page_menu::add_link_item(tab, menu, tracked)` inserts `Copy Link _Without Tracking` right after WebKit's `CopyLinkToClipboard` (index 1); activating it copies the clean URL. Read through `window.clipboard().read_text_future()`.
- `tab.duplicate` on the first tab: order becomes `[first, copy, second]`, the copy is selected and commits `index.html`.
- `tab.pin` on the second tab: it moves first, its row shows the pin and not the close button (`tab_row_buttons == (false, true)`), and `session::snapshot(..)` has `pinned: true`. Capture `tab-pinned.png`. Its menu then offers `Unp_in Tab`.
- `tab.mute`: `web_view().is_muted()` is true and the tab indicator is `("audio-volume-muted-symbolic", "Unmute Tab")`. `window.click_tab_indicator` unmutes (false, no indicator).
- `tab.close-others` on the first tab leaves `[second(pinned), first]`. `tab.reopen-closed` brings back a third tab on `index.html`. `tab.close-after` leaves the pair again. `tab.new-tab-next` opens a blank tab at index 2, which is then closed.
- `tab.move-to-new-window` on the pinned tab: a new window holds exactly that tab, still pinned, and this window holds only the first tab. The new window is closed.
- Core: `vsesvit_core::clean_url::clean`, session snapshot types (`vsesvit_core::session::{SessionSnapshot, WindowSnapshot, TabSnapshot{id,url,title,pinned,last_active_ms,group,restore_state}}`).
- Engine: mostly engine-neutral. Mute is **NEEDS-CEF** (`CefBrowserHost::SetAudioMuted` / `IsAudioMuted`, in bindings). Building a `webkit::ContextMenu` by hand to test the link item is **WEBKIT-ONLY test plumbing**. The behavior (Copy Link Without Tracking right after Copy Link Address in a link's menu) is KEEP, with a CEF candidate of `CefContextMenuHandler::OnBeforeContextMenu` inserting into the `CefMenuModel` after Chromium's copy-link item.

### 5.23 `tab_search` (line 1008)
- Precondition: this window has one tab. Builds a second, unshown window (`BrowserWindow::new`) with `page2.html` and `/media.html?vsesvit-tab-search` (Media selected last).
- `win.search-tabs` is on `<Control><Shift>a` and opens `window.tab_search()` anchored to the sidebar's Search Tabs button (`tab_search_buttons().0`).
- Query `vsesvit`: rows are `(section, title, site)`. The first two are both fixture pages, by title, under `Open tabs`. Row 2 is `Media` (an address match) under `Open tabs`. Rows 3+ are under `Recently closed` (tabs closed by earlier checks). Headings sit at indices `[0, 3]` only. Selection starts at row 0; `step(1)` → 1, `step(-1)` → 0, `step(-1)` → the last row (wraps). Capture `tab-search.png`.
- Empty query: exactly 3 open tabs, with Media before page2 (more recently used first). Query `no tab is called this`: 0 rows and `says_no_results()` (the "No results found" text).
- Query `fixture 2`: the top row is page2 under Open tabs. `click(0)` selects page2 in the other window, closes the popover, and this window keeps 1 tab.
- Close Media in the other window. Reopen the search with query `tab-search`: exactly `[("Recently closed","Media","127.0.0.1")]`. `press_enter()` reopens it **in this window**, committing the media URL with title `Media`. Searching again lists it under `Open tabs`. `win.search-tabs` while open closes it.
- With `tabs.position = top`, the search opens from the tab bar's Search Tabs button (`tab_search_buttons().1`). Restored to left.
- Core: `vsesvit_core::tab_search::{self, Listed, Row}` (ranking: title match, then address match, then recency), closed-tab stacks (`crate::closed_tabs::ClosedTabs`, `CLOSED_TABS_KEPT`).
- **Engine-neutral** apart from titles. KEEP.

### 5.24 `tab_groups` (line 1155)
- A second tab on page2. Its menu offers `_Add Tab to New Group`. `tab.add-to-new-group` puts it in a new group, opens the group editor popover (`group_editor_open()`; capture `tab-group-editor.png`), and shows a mapped header `gtk::Button` (`group_header(id)`) with the tab's row visible.
- Clicking the header collapses it: `group.collapsed == true`, the row is hidden, the header stays mapped, and the first tab (visible) is selected. Capture `tab-group-collapsed.png`. Clicking again expands.
- With a third tab and the group collapsed: `select_next_tab(false)` from the first selects the third, and `select_next_tab(true)` goes back to the first (hidden tabs are skipped; these are Ctrl+Tab and Ctrl+Shift+Tab). Closing the selected third selects the first, not the hidden tab, and the group stays collapsed.
- A tab opened from the grouped tab (`open_tab(page2, Some(&grouped), Foreground)`) joins its group. Closed and reopened, it is back in the group right after the grouped tab.
- The session snapshot gives the tab `group.id`. `tab-group.ungroup` (target: the group id as a string) leaves no group, no header and nothing saved.
- Core: `vsesvit_core::tab_groups::{GroupId, ...}`, session `TabSnapshot.group: Option<TabGroup>`.
- **Engine-neutral.** KEEP.

### 5.25 `memory_saver` (line 1262)
- Setup: three background tabs A, B, C on `index.html`. A then loads `/icon.html`, so it has back history and a favicon; wait for title `Icon` and an icon. B gets typed input (an appended `<input>` with value `unsaved`, `value !== defaultValue`). C is pinned (`window.set_pinned`).
- `browser.sleep_idle_tabs(now + 7 h)` (the Memory Saver sweep). Wait until A `is_asleep()` and WebKit's `web-process-terminated` fired. Then wait 1 s and capture `tab-asleep.png`.
- Asserts: the termination reason is `webkit::WebProcessTerminationReason::TerminatedByApi`. A's view released the page (`uri()` empty). B, C and the selected tab are awake with a URI. A's tab still shows title `Icon`, with its icon's peak alpha exactly half the original (`tab_look`). The session snapshot for A keeps `(icon.html, "Icon", restore_state.is_some())`.
- Selecting A wakes it: not asleep, committed `icon.html`, title `Icon`, `can_go_back()`, and its tab icon is the same favicon texture.
- With `performance.memory_saver = false`, a +7 h sweep puts none to sleep. Then reset.
- Settings > General: a `SwitchRow` titled `Memory Saver` (`memory_saver::TITLE`), active. A `ComboRow` `Memory Savings` with choices `[Moderate, Balanced, Maximum]` (`MemorySaverMode::ALL` labels), selected `Balanced`, subtitle `Tabs become inactive after 4 hours in the background`, sensitive. Switching it off stores false and makes the combo insensitive. Then back on.
- Core: `vsesvit_core::memory_saver::{Sweep::new, TabActivity{url,shown,pinned,audible,capturing,related}, UNSAVED_INPUT_SCRIPT, has_unsaved_input, SWEEP_EVERY (60 s), TITLE, MemorySaverMode::{ALL,label,description,delay}}` (Moderate 6 h, Balanced 4 h, Maximum 2 h). The shell sweeps every `SWEEP_EVERY` from `Browser::start`. Prefs `performance.memory_saver` (local, default true) and `performance.memory_saver.mode` (local, default balanced).
- Engine: user-visible behavior KEEP (the tab keeps title, icon and history, its icon shows at half opacity, it wakes on selection). **WEBKIT-ONLY:** `TerminatedByApi`, "the view lets go of the page" via an empty `uri()`, and `restore_state` as WebKit `WebViewSessionState` bytes. **NEEDS-CEF** *(inference)*: releasing a background tab's renderer while keeping its back/forward history. CEF has no public API to serialize and restore a navigation history (`CefBrowserHost::GetNavigationEntries` only reads), so "Back available after waking" is a risk. Candidates: keep the `CefBrowser` and kill its renderer (no direct API; `OnRenderProcessTerminated` reports kills), or close the browser and rebuild history by replaying entries (loses form state and Back unless replayed). The unsaved-input probe needs page eval.

### 5.26 `popup` (line 1380)
- Wait for `window.extension_action_button(PROBE_ID)` (a pinned toolbar button) and click it. Wait for `window.extension_popup_view()` (a WebView in a popover) whose title starts with `visits=`. N ≥ 1. `close_extension_popup()`.
- KEEP. **NEEDS-CEF, risk** *(inference)*: showing an extension's `default_popup` as a real extension popup view anchored to the GPUI toolbar button. In Chrome this is `ExtensionPopup`, tied to Chrome's toolbar; CEF has no API for it. A plain `CefBrowser` on `chrome-extension://<id>/popup.html` is not a popup-type view (`window.close()`, `chrome.extension.getViews({type:"popup"})`, `action.openPopup`, focus-loss closing).

### 5.27 `extension_toolbar` (line 1398), not in self-test.md
- The probe's button is pinned at the start (a fresh install pins).
- Opening the button's context menu (`open_extension_context_menu`) and capturing `extensions-context-menu.png`. Menu content (`window/ext_actions.rs::action_menu`): the extension's own `action` context menu items, then a section with `_Unpin` (`win.extension-pin` with `(id,false)`) and `_Manage Extensions` (`win.show-extensions`).
- The puzzle piece (`open_extensions_menu`, tooltip `Extensions`) opens the Extensions menu; capture `extensions-menu.png`. Its pin toggle (icon `view-pin-symbolic`) unpins: the toolbar button goes away and synced pref `toolbar.extensions` stores `pinned: false` for the probe. The probe's row button (no label, no icon name) opens the popup **anchored to the puzzle piece**. Wait 800 ms, capture `extensions-unpinned-popup.png`, close.
- `win.extension-pin (PROBE_ID, true)`: the button is back and the pref says `pinned: true`.
- Core: `vsesvit_core::extensions::toolbar::{TOOLBAR (key "toolbar.extensions", synced, Vec<Entry{id,pinned}>), ...}`, `browser.extension_toolbar(&available)`.
- Engine: shell UI and core are neutral. The list of actions (title, icon, badge, popup) and popup hosting are **NEEDS-CEF** (as 5.26).

### 5.28 `context_menus` (line 1448)
- The selected tab must be on `index.html`. A `webkit::ContextMenu` is built by hand with `Reload`, a separator and `InspectElement`. `page_menu::add_extension_items(tab, menu, Target{page_url})` must produce `["Reload","-","Vsesvit Probe page item","-","InspectElement"]`. A link target (`link_url` = page2) gets 0 extension items.
- Activating the probe item's action: the probe's `onClicked` gives `{id:"probe-page", pageUrl: index, tab: <tab id>}` (read from `data-vsesvit-probe-menu`).
- The action button's context menu: the first section's first item is labelled `Vsesvit Probe action item`. Activating its action gives `{id:"probe-action", pageUrl:null, tab:<selected tab id>}`.
- Engine: behavior KEEP (extension items in the page menu by context, before Inspect's separator; action items at the top of the toolbar button's menu). The hand-built `webkit::ContextMenu` and `Target` from `vsesvit_webext::menus` are **WEBKIT-ONLY plumbing**. **NEEDS-CEF:** `CefContextMenuHandler::OnBeforeContextMenu` / `OnContextMenuCommand` / `RunContextMenu` (in bindings). *(inference)* With a Chrome-style browser, Chromium's own context menu model already carries extension items. The action-context items have no CEF API (Windows has the same gap: WebView2 gives no way to read them, PLAN.md).

### 5.29 `extension_commands` (line 1509)
- `app.accels_for_action(keymap::extension_action(PROBE_ID, "_execute_action"))` must be exactly `<Alt><Shift>p`, and for `"probe-command"` exactly `<Alt><Shift>k`. The detailed action is `win.extension-command` with target `(id, name)`.
- Activating `win.extension-command (PROBE_ID,"probe-command")`: the probe's `commands.onCommand` gives `{name:"probe-command", tab:<selected tab id>, url: index}`.
- `(PROBE_ID,"_execute_action")` opens the popup (URI ends `/popup.html`), which is then closed.
- Settings > Shortcuts page: an `ActionRow` titled `Vsesvit Probe command`, subtitle `Vsesvit Probe`, whose `ShortcutLabel` shows `<Alt><Shift>k` (scrolled to the bottom; capture `extension-shortcuts.png`). `browser.edit_keymap(|k| k.assign_extension(&shortcuts, PROBE_ID, "probe-command", Some(Alt+Shift+J)))` makes the accel `<Alt><Shift>j`, and no action holds Alt+Shift+K. The row's `Reset to Default` button (tooltip) brings back `<Alt><Shift>k` and then hides itself. A cleanup resets the extension command.
- Core: `vsesvit_core::shortcuts::{Chord, Keymap}`, `vsesvit_core::extensions::commands::{ExtensionShortcuts, Keymap::assign_extension, reset_extension}` (Chrome's rules: browser keys win, then user changes, then suggested keys in install order). `browser.extension_shortcuts(keymap)`. Pref `keyboard.shortcuts` (synced).
- Engine: keymap neutral. Firing `commands.onCommand` with the selected tab (and granting `activeTab`) is **NEEDS-CEF** *(inference)*: Chromium's extension keybinding registry lives in Chrome's browser-window accelerator handling; no CEF API fires a command. Key events reaching GPUI while web content has focus is also NEEDS-CEF (`CefKeyboardHandler::OnPreKeyEvent`, in bindings).

### 5.30 `extension_notifications` (line 1593)
- On `index.html`, setting `data-vsesvit-notify='keep'` must give `{created:"probe-notification"}` in `data-vsesvit-probe-notified`.
- `browser.runtime().notification(PROBE_ID,"probe-notification")` must show title `Vsesvit Probe notification`, body `Sent by the probe`, buttons `["Open"]`, and a PNG icon (starts `\x89PNG`).
- Activating app action `app.extension-notification` (`vsesvit_webext::notifications::ACTION`) with `(id, "probe-notification", "0")` (`Activation::Button(0)`) fires `onButtonClicked` → `{event:"button", id, button:0}`. With `"settings"` (`Activation::Settings`) it opens the `Extensions` `PreferencesDialog`, where the `Notifications` `SwitchRow` is active inside the expanded `ExpanderRow` titled `Vsesvit Probe`; capture `extension-notifications.png`.
- Switching it off fires `onPermissionLevelChanged` `denied`, closes the notification, and makes core's `extensions().notifications_allowed(id)` false. Switching it on fires `granted`. A cleanup restores the setting and calls `runtime.notification_permission_changed`.
- Core: `extensions().{set_notifications_allowed, notifications_allowed}` (PLAN.md: synced as `extensions.notifications_off`).
- Engine: behavior KEEP (desktop notification with Chrome's Linux layout, an Open button, a Settings button opening the Extensions dialog on that extension, a per-extension switch). **NEEDS-CEF, risk** *(inference)*: Chromium shows extension notifications through its own Linux bridge (D-Bus `org.freedesktop.Notifications` or the portal), and its Settings button goes to Chrome settings. CEF exposes no hook to list them, route their activations or flip Chromium's per-extension notifier state, so `onPermissionLevelChanged` would not follow core's switch. Windows' check only records what WebView2 does and does not judge.

### 5.31 `extension_update` (line 1662)
- `FixtureStore::start(&server)`. Publish the update probe 1.0 with `["storage"]`, signed by `CrxKey::second()`, then `core.set_stores(store.stores())`. A cleanup uninstalls it and `set_stores(Stores::default())`.
- `browser.install(InstallSource::ChromeWebStore{id}, ..)` (no prompt). After a reload the page reads `1.0:1.0`.
- Publish 2.0. `browser.update_extensions()` reports `updated == [(id, "2.0", enabled=true)]`. After a reload: `2.0:1.0` (storage kept).
- Publish 3.0 with `["storage","tabs"]`. `win.show-extensions` opens the `Extensions` dialog. Clicking the Update button (widget name `update-extensions`, `dialogs/extensions.rs::UPDATE_BUTTON`) makes it insensitive while it runs, then sensitive again. Then core's record shows version `3.0`, `withheld` texts `["Read your browsing history"]`, `enabled == false`, and the runtime does not run it.
- Its `ExpanderRow` `Vsesvit update probe` holds an `ActionRow` `Enable “Vsesvit update probe”?` whose subtitle is `re_enable_text(withheld)` = `It can now:\n• Read your browsing history`. The `gtk::Switch` (tooltip `Enabled`) is insensitive and off. Capture `extension-update.png`. Clicking `_Re-enable` runs it. After a reload: `3.0:1.0`.
- Core: `extensions().{prepare_install, prepare_update_check → UpdateCheck::run, commit_updates → UpdateReport{updated, summary()}, get(id) → InstalledExtension{version, withheld: Vec<PermissionMessage>, enabled}, approve_permissions(id), uninstall}`, `extensions::permissions::{update_warnings, RE_ENABLE_LEAD = "It can now:"}`, `Profile::set_stores`, `Stores::default()`.
- Engine: core decisions are engine-neutral and KEEP. **NEEDS-CEF** *(inference)*: swap a running extension to a new unpacked version in place, keeping `chrome.storage` (same id), and keep it unloaded while withheld. The way to load and unload in CEF 154 is the open question from 5.4. Chromium's own updater and permission-escalation disabling must not fight core's.

### 5.32 `omnibox` (line 1746)
- `profile.omnibox().resolve("vsesvit fixture")` is `NavTarget::Search{engine: default.id, url}`. `resolve("127.0.0.1:<port>/page2.html")` is `NavTarget::Url(<page2 url>)`.
- Core: `vsesvit_core::search::{NavTarget, SearchEngine}`, `profile.search_engines().default_engine()`, `profile.omnibox().resolve`.
- **Engine-neutral** (core only).

### 5.33 `address_completion` (line 1767)
- `address.focus_for_typing()` (focus plus select all), `type_text("127.0.0")`. Wait until `observe()` reports the popover open, text `127.0.0.1:<port>`, selection `(7, len)` (the inline completion `.1:<port>` selected) and highlighted row 0. Capture `address-completion.png`. The list must be at least as wide as the address bar.
- `press(Down)`: highlighted 1, the text equals `fills[1]`, no selection. `press(Escape)`: back to highlighted 0, completed text and selection. `press(Return)`: the tab commits `http://127.0.0.1:<port>/`. Then `submit_text(index)`.
- `Observed { text, selection, highlighted, fills, open }` (`address_bar.rs:120`).
- Core: `vsesvit_core::omnibox` (history suggestions, inline completion), suggestion rows with `fill`.
- **Engine-neutral** shell behavior plus a navigation. KEEP.

### 5.34 `search_suggestions` (line 1824)
- Save engine `EngineForm{name:"Fixture Search", keyword:"fixture", url:"<origin>/search?q=%s"}`. `set_suggest_url(<origin>/suggest?q={searchTerms})`, `set_default`, `address_bar.suggest.search = true`.
- Typing `vsesvit`: the open list's first fills are `["vsesvit","vsesvit one","vsesvit two"]`, highlighted 0, after **exactly one** `/suggest` hit. Capture `search-suggestions.png`. Down: text `vsesvit one`, highlighted 1. Enter: commits `<origin>/search?q=vsesvit+one`. Back to `index.html`.
- Typing `127.0.0.1:<port>/page2.html`, waiting `DEBOUNCE` (100 ms) + 400 ms: no suggestion rows. With the setting off, typing `vsesvit off`: none. No further `/suggest` hits. Escape.
- A cleanup restores the built-in default (`SearchEngineId::builtin_default()`), removes the engine and turns suggestions off.
- Core: `search_engines().{save, set_suggest_url, set_default, remove}`, `vsesvit_core::suggest::DEBOUNCE`, pref `address_bar.suggest.search` (synced, default true).
- **Engine-neutral** (core fetches suggestions over HTTP itself). KEEP.

### 5.35 `selection_search` (line 1908)
- With the fixture engine as default, `SELECT_HEADING` in the page selects `Vsesvit fixture page`. Wait until `tab.selection()` (the shell's record of the page selection) equals it.
- `page_menu::add_selection_item(tab, menu)` on a hand-built menu `[Copy, InspectElement]` adds `Search Fixture Search for “Vsesvit fixture page”` at index 1 (right after Copy). Activating it opens `<origin>/search?q=Vsesvit+fixture+page` in a new selected tab right after the page. The tab is then closed and the engine restored.
- Core: `profile.omnibox().for_selection(&text) → Option<{url, label}>`.
- Engine: behavior KEEP. **WEBKIT-ONLY:** the selection-tracking user script (`tab.selection()`) and the hand-built `webkit::ContextMenu`. **NEEDS-CEF:** `CefContextMenuParams::GetSelectionText` in `OnBeforeContextMenu`, inserting after Chromium's Copy.

### 5.36 `session` (line 1958)
- `browser.save_session_now()`. `core.session().restore()` is `Some(snapshot)` with some window holding tabs. Detail: window and tab counts, the first URL.
- Core: `vsesvit_core::session::{Session::save/restore, SessionSnapshot}`.
- **Engine-neutral.** KEEP.

### 5.37 `download` (line 1973)
- Removes `OUT_DIR/downloads` and sets local pref `downloads.directory = Some(OUT_DIR/downloads)` while running. `tab.load(<origin>/download.bin)`.
- Wait for an entry in `browser.downloads().list(Browsing::Normal)` with `path == OUT_DIR/downloads/download.bin` and `state == State::Completed`. The file bytes must equal the 4096 served bytes. `window.shows_downloads_button()` must be true.
- `win.show-downloads` opens `browser.windowed(Windowed::Downloads(Normal))`. Wait 500 ms, capture `downloads.png`. Detail includes `status_line(&entry, None, true)`.
- Core: `vsesvit_core::downloads::{Download, DownloadId, State, status_line}`, pref `downloads.directory` (local).
- KEEP. **NEEDS-CEF:** `CefDownloadHandler::OnBeforeDownload` (choose the path; `CefBeforeDownloadCallback::Continue(path, show_dialog=false)`), `OnDownloadUpdated` (`CefDownloadItem` state and bytes), `CanDownload` (all in bindings).

### 5.38 `private_window` (line 2011, 30 s)
- `app.new-private-window` is on `<Control><Shift>n`.
- `page_menu::add_private_window_item(tab, menu, <origin>/page2.html?vsesvit-private)` on a hand-built `[OpenLinkInNewWindow, CopyLinkToClipboard]` menu inserts `Open Link in _Private Window` at index 1. Activating it opens a window of `Browsing::Private` whose tab commits the URL. The window title is `Vsesvit fixture 2 (Private)` and it has CSS class `private`. Capture `private-window.png`.
- **WEBKIT-ONLY asserts:** the private tab's `network_session().is_ephemeral()` and the normal tab's not. With pref `privacy.third_party_cookies` at its default `BlockInPrivate`, the private session's cookie accept policy is `NoThirdParty` and the profile's is `Always`.
- Site info for the private tab offers no cookie rule (`cookies::site_info_section` is None).
- No history row for the URL. After `save_session_now`, the saved session is non-empty and lacks the URL.
- `zoom-in` in the private window: the tab's zoom > 1. Core `site_zoom(Private).get(url)` > 1 and `site_zoom(Normal).get(url) == 1.0`.
- Tab search for `vsesvit-private`: 0 rows from the normal window, 1 from the private one.
- New tab in the private window: once `about:blank` has loaded, its `h1` text is `You're browsing privately` (`vsesvit_core::new_tab::PRIVATE_PAGE`). Then closed.
- After 1 s the probe's content script has not run (`dataset.vsesvitProbe` undefined). `browser.set_extension_allowed_in_private(PROBE_ID, true)` plus a reload: it reports `background-replied`. A cleanup sets it false again.
- `/download.bin` downloaded in private: a `Browsing::Private` row with a negative id, Completed. It is listed (by file name, as an `ActionRow` title) in the Downloads window opened from the private window, not in the one opened from a normal window. No negative-id rows are in the normal list.
- A tab opened and closed in private: `can_reopen_closed_tab(Private)` is true.
- Close the private window. Afterwards: `private_session_lasts()` is false (the engine session ended), the private closed stack is empty, the private zoom is back to 1.0, there are no private download rows, and the downloaded file still exists.
- Core: `vsesvit_core::private::Browsing`, `site_zoom(browsing)`, `history().search`, `session().restore()`, `extensions().set_allowed_in_private`, `new_tab::PRIVATE_PAGE`, `cookies::ThirdPartyCookies::BlockInPrivate`.
- Engine: product behavior KEEP. **NEEDS-CEF:** a separate off-the-record request context for private windows *(inference: `CefRequestContext::CreateContext` with an empty `cache_path` is in-memory)*, third-party cookie blocking in it (Chrome pref or content setting via `CefRequestContext::SetPreference` / `SetContentSetting`, in bindings), per-extension "allowed in private" (no CEF API: risk), and tearing the context down when the last private window closes. The WebKit session and policy assertions become engine-neutral equivalents (for example, the Windows check reads the `localhost` frame's cookie on `/cookies.html`).

### 5.39 `download_safety` (line 2193)
- `/dangerous.sh` (a shell script served as octet-stream) ends `State::Unconfirmed`. The file sits at `unconfirmed_path(path)` = `OUT_DIR/downloads/dangerous.sh.unconfirmed` holding the script, and nothing is at `dangerous.sh`.
- The warning popover under the downloads button (`window.download_warning()`) has `_Keep` and `_Discard`. Keep makes the entry Completed and the file `dangerous.sh` holds the script.
- A second download goes to `dangerous (1).sh`. Discard removes the entry and both files.
- Core: `downloads::{is_dangerous, unconfirmed_path, State::Unconfirmed}` and the unique-name rule.
- KEEP. **NEEDS-CEF:** the target path chosen in `OnBeforeDownload` (write to the `.unconfirmed` name, rename on Keep); deleting on Discard.

### 5.40 `download_speed` (line 2250, 30 s)
- With the normal Downloads window open: `/slow.bin` until the newest row's subtitle satisfies `testkit::reads_speed_and_time_left` (for example `250 KB/s - 500 KB of 10 MB, 38 secs left`). Then cancel (`browser.downloads().cancel(id)` → `State::Cancelled`).
- `/stalled.bin` until the newest row reads exactly `0 B/s - 1.0 KB of 1.0 MB`. Only the view's once-a-second refresh can produce that, since no bytes arrive. Then cancel.
- Core: `downloads::status_line(d, live: Option<Progress{received,total,per_second}>, exists)`.
- KEEP. **NEEDS-CEF:** `CefDownloadItem` received and total bytes (and speed, or the shell computes it), `CefDownloadItemCallback::Cancel`. *(inference)* CEF also offers Pause and Resume, so the Windows-only `download_pause` check becomes possible on Linux. PLAN.md says WebKitGTK cannot pause.

### 5.41 `new_tab_page` (line 2269)
- `window.new_tab()`. Once the tab is not loading and is committed at `about:blank`, `NEW_TAB_PAGE_PROBE` finds the search box and tile hrefs including `http://127.0.0.1:<port>/`. `tab.is_blank()` is true and `display_title()` is `New Tab`. Wait 500 ms, capture `new-tab.png`, close.
- Core: `vsesvit_core::new_tab::{page(p, browsing) → HTML, html(sites, engine), TopSite, TILES = 8}`. The doc says shells load it as HTML at `about:blank`, so the tab reads as blank everywhere (empty address bar, "New tab" title, not in session or history).
- KEEP. **NEEDS-CEF:** HTML shown while the committed URL stays `about:blank` *(inference: load `about:blank`, then `document.open/write` the page by script; or intercept with a `CefResourceHandler` / scheme handler)*. Chrome-style's own `chrome://newtab` must not appear.

### 5.42 `address_progress` (line 2296), not in self-test.md
- A `TcpListener` on `127.0.0.1:0` that never accepts. Opens `data:text/html,<title>Slow page</title><h1>Slow page</h1><img src='http://127.0.0.1:<port>/stall.png'>`. Once the title is `Slow page`, wait 1 s: the tab is loading and the address bar's progress line is in (0, 1). Capture `address-progress.png`. Drop the listener: loading ends and progress returns to 0.0.
- Shell: `AddressBar::{set_progress (never moves back within a load, min PROGRESS_START), finish_progress (fills, holds PROGRESS_HOLD, hides), progress()}`.
- KEEP. **NEEDS-CEF:** `CefDisplayHandler::OnLoadingProgressChange`, `CefLoadHandler::OnLoadingStateChange` (in bindings).

### 5.43 `settings` (line 2327)
- **WEBKIT-ONLY assert:** the engine settings have `javascript_can_open_windows_automatically == false`, smooth scrolling on, and hardware acceleration `Always`. The product meaning (pop-ups blocked, smooth scrolling and GPU on by default) is KEEP; prefs `content.block_popups` (default true), `scrolling.smooth` (default true), `system.hardware_acceleration` (local, default true). **NEEDS-CEF** *(inference)*: pop-up blocking via `CefLifeSpanHandler::OnBeforePopup` or Chrome's popups content setting; smooth scrolling and GPU are Chromium switches (`--enable-smooth-scrolling`/`--disable-smooth-scrolling`, `--disable-gpu`) fixed at startup.
- Sets `homepage = <page2 url>` and shows the home button (`set_home_button_visible(true)`, pref `toolbar.home_button`). Capture `home-button.png`. Clicking it loads page2 in the selected tab.
- `win.show-settings`: pages `general`, `sync`, `appearance`, `search`, `privacy`, `shortcuts` all exist; each is captured as `settings-<page>.png` after 500 ms. Privacy shows a mapped row `Vsesvit Doesn't Save Passwords` (`PASSWORDS_NOTICE`).
- Restores the home button and homepage.
- Doc delta: self-test.md lists General, Appearance, Search, Privacy, Shortcuts. The code also requires and captures `sync`.

### 5.44 `spellcheck` (line 2386)
- Entirely WebKit-typed. `webkit::WebContext::default()` `is_spell_checking_enabled()` and `spell_checking_languages()`.
- Assertions:
  - A fresh profile checks spelling iff `Dictionaries::checked(None)` (installed system dictionaries matching system locales) is non-empty, in exactly those languages.
  - `set_engine_switch(spellcheck.enabled, false)` turns checking off, and on restores it.
  - Toggling the first installed dictionary (`set_spellcheck_language`) checks `dictionaries.checked(Some(&choose(None, lang, on)))`; resetting `spellcheck.languages` restores the system set.
  - `page_menu::name_add_to_dictionary` relabels WebKit's `LearnSpelling` as `_Add to Dictionary`.
  - Settings > General: `Check Spelling` switch on and the `Languages` row sensitive (expanded; capture `settings-spell-check.png`).
- Core: `vsesvit_core::spellcheck::{Dictionaries::{new, installed, checked, choose}, installed_in, system_locales, display_name}`, prefs `spellcheck.enabled` (synced, default true) and `spellcheck.languages` (synced, `Option<Vec<String>>`, None = system).
- Engine: product behavior KEEP (a switch, a language list defaulting to the system's, Add to Dictionary). The WebKit context and `LearnSpelling` relabel are **WEBKIT-ONLY**. **NEEDS-CEF, risk** *(inference)*: Chromium spellcheck uses its own `.bdic` Hunspell dictionaries, normally downloaded from Google, not the system's `/usr/share/hunspell`. It is driven by prefs `browser.enable_spellchecking` and `spellcheck.dictionaries` (`CefRequestContext::SetPreference`). Suggestions and add-word come through `CefContextMenuParams::GetDictionarySuggestions` / `GetMisspelledWord` and `CefBrowserHost::ReplaceMisspelling` / `AddWordToDictionary` (in bindings). Core's "installed dictionaries" concept may not map. Windows has no spellcheck check at all.

### 5.45 `search_engines` (line 2465)
- Settings > Search > `Manage Search Engines` subpage. The `_Add` button is found via `adw::ButtonContent`.
- The row `DuckDuckGo (Default)` exists and activating `engine.remove` on it fails (the default cannot be removed).
- Add opens an `AlertDialog` editor with `EntryRow`s `Name`, `Shortcut`, `URL with %s in Place of Query`. Shortcut `w` shows a visible error label `Another search engine has this shortcut` and response `save` is disabled. With `fx` and `<origin>/search?q=%s` it is enabled; capture `search-engine-editor.png`. `_Add` saves `Fixture Engine` to core, and the list shows it; capture `settings-search-engines.png`.
- Typing `fx vsesvit fixture` + Enter commits `<origin>/search?q=vsesvit+fixture`.
- Settings reopened: the row's `engine.remove` deletes it from core and the list. Back to `index.html`. A cleanup removes the engine if left.
- Core: `search_engines().{list, save, remove, default_engine}`, keyword search in `omnibox`.
- **Engine-neutral.** KEEP.

### 5.46 `bookmarks_bar_menus` (line 2544), not in self-test.md
- Data: records a red 16x16 PNG as `index.html`'s favicon (`favicons().record`). Adds folder `Fixture folder` at toolbar index 0 containing `Fixture index`, a very long-titled page2 link and `Inner folder` (holding `Inner page`), plus 16 `Example site N` links (`https://siteN.example/`).
- At window size 900×700 the bar overflows: `bar.overflow() = (shown, hidden>0)` and the chevron (»), mapped; capture `bookmarks-overflow.png`. The chevron menu has exactly `hidden` rows (`bookmark-menu-row`); capture `bookmarks-overflow-menu.png`.
- Item 0 is the folder. Its menu shows favicons (paintable images > 0). The `Inner folder` row opens a submenu; capture both (`bookmarks-folder-menu.png`). The long title's label is ellipsized.
- Item 1 (the `Renamed fixture` bookmark left by `star_bubble`) context menu via `bookmark_menu::open_context_menu(button, Target::Node(..))`; capture `bookmarks-context-menu.png`. `bookmark.edit` opens a dialog (`bookmarks-edit-dialog.png`) whose URL entry equals the bookmark's URL.
- Cleanup: removes the folder and the Example sites, size back to 1280×820.
- **Engine-neutral.** KEEP.

### 5.47 `ctrl_s_toggles_sidebar` (line 2631), not in self-test.md
- `app.actions_for_accel("<Control>s") == ["win.toggle-tab-sidebar"]`. The sidebar is shown (`layout_probe().sidebar.is_some()`). The action hides it; after 500 ms capture `sidebar-hidden.png`. Again shows it.
- **Engine-neutral.** KEEP (Vsesvit's Ctrl+S is the tab list; Save Page As is Ctrl+Shift+S).

### 5.48 `shortcuts` (line 2650)
- `edit_keymap(|k| k.assign(Command::ShowHistory, [Ctrl+Shift+Y]))`: `win.show-history` has exactly `<Control><Shift>y`, and `<Control>h` runs nothing.
- Assigning `Ctrl+J` to History returns `taken` from `Downloads`. `win.show-downloads` has none and `<Control>j` runs `win.show-history`.
- Settings > Shortcuts page (only that page; Sync has its own "History" row): the `History` row's `ShortcutLabel` is `<Control>j` and `Downloads` is `""` (shown as Disabled). Capture `shortcuts-edited.png`.
- `shortcut_settings::capture(dialog, browser, Target::Browser(ShowHistory))` given Ctrl+T notes `Also used by New tab. Saving moves it here.`; capture `shortcut-capture.png`.
- `Keymap::reset_all`: every `keymap::actions()` accel equals `Command::defaults()`, `win.save-page` is `<Control><Shift>s`, and `<Control>s` runs only `win.toggle-tab-sidebar`. A cleanup resets all.
- Core: `vsesvit_core::shortcuts::{Command::{ShowHistory, title, defaults, ALL}, Chord, Keymap::{assign → Vec<Taken{from}>, reset_all, offer, chords, command_for}}`, pref `keyboard.shortcuts` (synced).
- **Engine-neutral** as written: it only inspects GTK accelerator tables. **NEEDS-CEF** *(inference)*: shortcuts must also work while a CEF page has keyboard focus and must win over the page where Chrome's do. That means `CefKeyboardHandler::OnPreKeyEvent` returning true with `is_keyboard_shortcut`. The Windows check tests exactly this inside the loaded page (Ctrl+Shift+Y opens History, F5 prevented); the Linux one does not.

### 5.49 `save_page` (line 2739)
- Reloads `index.html`, then runs `save_page::save(browser, view, OUT_DIR/saved-page.mhtml)`, the step after the file dialog (`webkit::WebView::save_to_file_future(.., SaveMode::Mhtml)`). The file must contain `Vsesvit fixture`.
- User-visible: `win.save-page` (Ctrl+Shift+S) shows a GTK save dialog titled `Save Page As` in the downloads folder with a suggested name, then a toast `Saved “<name>”` or `Cannot save the page: <e>`. A non-HTML document goes through the downloads list instead.
- KEEP. **NEEDS-CEF** *(inference)*: MHTML via DevTools `Page.captureSnapshot {format:"mhtml"}` (`ExecuteDevToolsMethod`), or Chrome's own save via `CefBrowserHost::ExecuteChromeCommand` (which brings Chrome's dialog).

### 5.50 `page_commands` (line 2756)
- Accels: `win.print` = `<Control>p`; `win.view-source` = `<Control>u`; `win.developer-tools` = `<Control><Shift>i` and `F12`; `win.javascript-console` = `<Control><Shift>j`. **Engine-neutral.**
- `win.view-source` on `index.html` opens a new selected tab right after the page, committed at and titled `view-source:<url>`, whose `body.innerText` contains `<title>Vsesvit fixture</title>`. **WEBKIT-ONLY assert:** served from the open tab with 0 extra fetches of `/index.html`.
- With page2 in the tab, a fresh tab on `view-source:<index>` shows it too. **WEBKIT-ONLY assert:** exactly 1 fetch "through a hidden view". This is the GTK shell's own view-source implementation (`crate::view_source::register`), since WebKitGTK has no view-source. *(inference)* Chromium's native `view-source:` refetches (the fixture sends `no-store`), so the counts differ.
- **KEEP (security):** a local `file://` page (`OUT_DIR/view-source-lure.html`) that embeds an iframe on `view-source:<origin>/lured-source.html` and after 300 ms sets `location.href` to it stays on screen 1.5 s later, and the server never sees `/lured-source.html`. *(inference)* Chromium blocks renderer-initiated `view-source:` navigations natively.
- A hand-built `[Reload, InspectElement]` menu plus `page_menu::add_page_items` becomes `["Reload","_Print…","View Page _Source","InspectElement"]`. Plumbing **WEBKIT-ONLY**; behavior KEEP (Print… and View Page Source right before Inspect Element).
- `win.developer-tools` opens the inspector (WebKit `attach` or `open-window` signal fired; either is fine) and a second activation closes it (`tab.inspector_open()`). **NEEDS-CEF:** `CefBrowserHost::ShowDevTools` / `CloseDevTools` / `HasDevTools` (in bindings).
- Print: a `webkit::PrintOperation` with GTK print settings (printer `Print to File`, format `pdf`, output URI `OUT_DIR/print.pdf`) without a dialog; the file starts with `%PDF`. **NEEDS-CEF:** `CefBrowserHost::PrintToPDF(path, settings, callback)` (in bindings) for the scripted part, and `CefBrowserHost::Print` for the user's dialog.
- A cleanup closes extra tabs and closes the inspector if open.

### 5.51 `zoom_indicator` (line 2929), not in self-test.md
- At 100% the address bar shows no zoom label (`shown_zoom()` None). `win.zoom-in` shows `110%`. Clicking the label opens a bubble; capture `zoom.png`. `win.zoom-reset` hides the label.
- KEEP. **NEEDS-CEF:** zoom changes (`CefBrowserHost::SetZoomLevel` / `GetZoomLevel` / `Zoom(command)` / `CanZoom`, in bindings). *(inference)* CEF zoom levels are logarithmic (factor 1.2^level); Chrome's presets include 110% and 125%.

### 5.52 `zoom_is_remembered_per_site` (line 2958)
- `submit_text(index)` at 1.0. Two `zoom-in`s give 1.25. Page2 (same host) opens at 1.25. `http://localhost:<port>/index.html` (another host) is at 1.0. Index is back at 1.25. Core `site_zoom(Normal).get(index) == 1.25`. `zoom-reset` gives 1.0 and core forgets it (1.0).
- Core: `profile.site_zoom(Browsing) → get(url)` (local, per host).
- KEEP. **NEEDS-CEF** *(inference)*: Chrome-style CEF keeps its own per-host zoom map in Chromium prefs. Either core's value must drive it on each navigation, or the two must be reconciled. Private windows keep their own (5.38).

### 5.53 `connection_info` (line 2999; 120 s with `--network`), not in self-test.md
- On the http fixture page, clicking the security icon (`click_security`) opens a popover headed `Connection is not secure` with no `Certificate` label; capture `connection-http.png`.
- Without `--network`, the check ends there with "HTTPS needs --network".
- With `--network`: open `https://example.com/`. The popover is headed `Connection is secure`, shows a SHA-256 fingerprint label (95 characters, 31 colons), a `gtk::Expander` (names), and a `Certificate chain` label; capture `connection-https.png`.
- KEEP. **NEEDS-CEF:** `CefNavigationEntry::GetSSLStatus` → `CefSSLStatus::GetX509Certificate` → `CefX509Certificate::{GetSubject, GetIssuer, GetDEREncoded (for SHA-256), GetIssuerChainSize, GetDEREncodedIssuerChain}` (in bindings as `sslstatus`, `x509_certificate`, `derencoded`, `issuer_chain_size`).

### 5.54 `site_permissions` (line 3041)
- `ASK_LOCATION` on the fixture page shows the prompt bubble (`address.prompt()`) headed `Know your location?`; capture `permission-prompt.png`. Wait `PROMPT_GUARD` (500 ms; buttons ignore activation before that, as Chrome guards prompts). Click `Allow while visiting the site`: core stores `Setting::Allow` for `Permission::Location`, and `permissions.query` reads `granted`, also after a reload. A new request after the reload shows no prompt and does not fail with code 1. The detail notes that without GeoClue the position never arrives.
- Site info (`click_security`): the `Location` `ComboRow`, set to `Block`, stores Block; capture `site-info-permissions.png`. After a reload, a request fails with code `1` and no prompt.
- Settings > Privacy > `Site Permissions` row (activated): Location listed as `Block`; capture `settings-site-permissions.png`. The `Remove` button (tooltip) resets to Ask (core None), leaving the `StatusPage` description `Sites you allow or block show here.`; capture `settings-site-permissions-empty.png`.
- A cleanup resets the site.
- Core: `vsesvit_core::permissions::{Origin::of, Permission::{Location,label}, Setting, Answer::{AllowWhileVisiting, ...}.label(), prompt(origin, permissions) → Prompt{heading, body, answers}, SiteChoice}`, `site_permissions().{get, set, reset_site}`.
- Engine: KEEP. **WEBKIT-ONLY:** reloading before each request because "WebKit keeps a document's first answer". **NEEDS-CEF:** `CefPermissionHandler::OnShowPermissionPrompt` (geolocation and others; `CefPermissionPromptCallback::Continue`) and `OnDismissPermissionPrompt` (in bindings). *(inference)* `navigator.permissions.query` reflects Chromium's content settings, so core's stored setting has to be mirrored into them (`CefRequestContext::SetContentSetting`, in bindings) for `granted` to read back. Chromium on Linux without a network location provider answers POSITION_UNAVAILABLE (code 2) once allowed, which this check accepts.

### 5.55 `tracking_protection` (line 3144)
- `trackers().use_list(bundled + ("localhost", Category::Analytics))`, wait for `tracking_applied` (**WEBKIT-ONLY**: blocker compile). Open `/trackers.html` at 127.0.0.1: title `tracker blocked` and the server never saw `/tracker/pixel.png`.
- Site info: `SwitchRow` `Tracking protection` (`trackers::TITLE`) is on, subtitle `trackers::site_status(true, None)` = `Blocking known trackers on this site`; capture `site-info-trackers.png`. Off: subtitle `Off for this site`, the exception is stored (`trackers::allowed(core, origin)`), and the page reloads to `tracker loaded` with the pixel fetched.
- Settings > Privacy `Tracking Protection` `ComboRow`: `Standard` with description `Blocks known advertising and analytics trackers on other sites`. Choosing `Strict` stores `privacy.tracking_protection = Strict`, subtitle `Also blocks social media trackers, such as Like and Share buttons. Some sites may not work`.
- A cleanup resets the level, clears the exception (`trackers::set_allowed(core, origin, false)`) and restores the bundled list.
- Core: `vsesvit_core::trackers::{TrackerList::{bundled, with_tracker}, Category, TrackingProtection::{label, description, ALL}, TITLE, site_status(on, blocked: Option<usize>), allowed, set_allowed, level_for}`, pref `privacy.tracking_protection` (synced, default standard).
- KEEP. **NEEDS-CEF:** per-request blocking via `CefRequestHandler::GetResourceRequestHandler` → `CefResourceRequestHandler::OnBeforeResourceLoad` returning `RV_CANCEL`. *(inference)* Chromium lets the shell count blocked trackers, so the expected subtitle could become `1 tracker blocked on this page` (`site_status(true, Some(1))`), as Windows counts. The `None` form is a **WEBKIT-ONLY** limitation (PLAN.md: WebKit reports nothing a blocker matched).

### 5.56 `https_only` (line 3221)
- Settings > Privacy: `Always Use Secure Connections` switch off with pref false. The `Secure DNS` row has non-empty subtitle text. Switching it on stores `privacy.https_only = true`.
- `browser.set_https_reach(Reach::Everywhere)`, a self-test-only hook that makes the upgrade cover loopback. Open `index.html`: the https attempt fails (the fixture server drops TLS), and the tab shows the warning titled `Connection is not secure` (`https_only::WARNING_TITLE`) **at the http address**, not loading, with `tab.security() == Security::Insecure`. Capture `https-only-warning.png`.
- Script-clicking `#continue` loads the page over http (title `Vsesvit fixture`) and stores the exception: `https_only::allowed(core, Normal, origin)`, stored as `Permission::Http` (label `Insecure connections`).
- A cleanup resets the pref, `Reach::Public`, and the site's Http setting.
- Core: `vsesvit_core::https_only::{TITLE, DESCRIPTION, WARNING_TITLE, Reach::{Public, Everywhere}, upgrade(p, browsing, url, reach), allowed}`, the error/warning page HTML (shell `error_page`).
- KEEP. **NEEDS-CEF:** upgrading in `CefRequestHandler::OnBeforeBrowse` (cancel and reload https) or `OnBeforeResourceLoad` redirect; failure in `CefLoadHandler::OnLoadError`; showing the warning while the committed address is the http URL; the `#continue` link reaching the shell (a custom scheme or a JS-to-browser message). *(inference)* Chrome-style has its own HTTPS-First mode, which would need to stay off.

### 5.57 `cookies` (line 3283)
- **WEBKIT-ONLY asserts:** the network session cookie manager's accept policy is `Always` by default, `NoThirdParty` after choosing Block, and `Always` after going back. WebKitGTK has one policy for all sites.
- Settings > Privacy: the three choice rows (labels from `ThirdPartyCookies::label`: `Allow third-party cookies`, `Block third-party cookies in private windows`, `Block third-party cookies`). Only `BlockInPrivate` is checked, with subtitle `In private windows, sites can't use cookies from other sites they embed`. Activating `Block` stores `privacy.third_party_cookies = Block`; back to BlockInPrivate.
- `/cookies.html`: title `cookies:ready`. `cookieResults.page` contains `first=1` and the jar has `first`.
- Site info `Cookies and site data` `ComboRow` (`cookies::SITE_TITLE`): `Default` (`choice_label(None)`), subtitle `site_status(false, None)` = `This site and sites it embeds can use cookies`. Choosing `Block` (the page reloads) reads `This site can't use cookies`; capture `site-info-cookies.png`. The reloaded page's `cookieResults.page` is empty, the site's cookies are deleted, and `/set-cookie` stores no `served`.
- Choosing `Clear on exit`: `/set-cookie` stores `served`. Settings > Privacy > `See All Site Data` lists `127.0.0.1` with a subtitle starting `Cookies`; capture `settings-site-data.png`. `browser.cookies().clear()` (what exit does) leaves no cookies for the site and **WEBKIT-ONLY**: no `WebsiteDataManager` record named `127.0.0.1`.
- A cleanup resets the choice and the site rule, then `cookies().apply()`.
- Core: `vsesvit_core::cookies::{ThirdPartyCookies::{ALL, label, description, blocks}, SITE_TITLE, choice_label, site_status, site_choices(current, exceptions), set, setting, third_party_blocked, SiteRules}`, `permissions::Setting::{Block, ClearOnExit}`.
- KEEP. **NEEDS-CEF:** cookie reads and deletes (`CefCookieManager::VisitUrlCookies` / `DeleteCookies` / `SetCookie`), blocking cookies per site and third-party globally (Chromium content setting COOKIES via `SetContentSetting`, prefs via `SetPreference`), listing site data (*(inference)* no CEF enumeration API; cookies via `VisitAllCookies`, other storage via DevTools `Storage.*`), clearing on exit. *(inference)* Chromium can lift third-party blocking per site (`site_choices(.., exceptions=true)` offers Allow), so the Linux assertions can follow Windows' (the frame reads `third=1` by default; Chromium treats `http://localhost` as potentially trustworthy, so its `Secure` cookie can be set there).

### 5.58 `capture_in_use` (line 3421)
- **WEBKIT-ONLY setup:** `settings.set_enable_mock_capture_devices(true)`, restored by the cleanup.
- `ASK_CAMERA_AND_MICROPHONE` shows one prompt headed `Use your camera and microphone?` with buttons in core's order `[Allow while visiting the site, Allow this time, Never allow, Not now]`. After `PROMPT_GUARD`, click `Allow this time`. The page gets `audio:live video:live`.
- The address bar's in-use button tooltip reads `Using your camera and microphone` (`Capturing::description()`), and `tab.capturing()` has camera and microphone. The tab indicator is `("camera-web-symbolic", "Using your camera and microphone")`. Capture `capture-in-use.png`, then with tabs on top `capture-in-use-top.png`.
- `click_in_use()` opens site info: Camera and Microphone rows both read `Allowed this time` with a visible `Stop` button; capture `site-info-capture.png`. Choosing `Block` on Camera stores Block and stops the camera only (the microphone is still on). `Stop` on Microphone ends the rest: nothing captured, no in-use button, no tab indicator, and the page's tracks end (`audio:ended video:ended`).
- The detail notes that screen sharing needs a user gesture and is not scripted. A cleanup stops every capture (`permissions::stop(view, ...)`), restores the tab position and resets the site.
- Core: `permissions::{prompt, Answer, Permission::{Camera, Microphone, ScreenShare}, Capturing::{any, uses, description}, SiteChoice::AllowedThisTime}`.
- KEEP. **NEEDS-CEF:** `CefPermissionHandler::OnRequestMediaAccessPermission` (`CefMediaAccessCallback::Continue(allowed_permissions)`) and `CefDisplayHandler::OnMediaAccessChange(has_video, has_audio)` for the indicators (in bindings). *(inference)* Fake devices come from Chromium's `--use-fake-device-for-media-stream` (never `--use-fake-ui-for-media-stream`, which auto-accepts). Stopping one device from browser UI has no obvious CEF API: open question.

### 5.59 `sync_passphrase` (line 3512)
- Stores an account (JSON): `sign_in:"self-test"`, `server:"http://127.0.0.1:9"` (refuses at once), `name:"Self-Test"`, limits, `server_keys:"missing"`, `plaintext_trusted:false`. Then `set_secret_state("account.session", b"x")`, `syncer.reload()`. Local pref `sync.passphrase_offered = false`. Cleanups restore the pref and sign out.
- The offer `AlertDialog` comes by itself: heading `Encrypt Your Synced Data?`, body `OFFER.1` verbatim, buttons `Set Passphrase` and `Not Now`. Capture `sync-passphrase-offer.png`. `Not Now` sets `sync.passphrase_offered` and closes. After `syncer.reload()` it does not come back.
- Settings > Sync shows mapped buttons `Encrypt with a Passphrase…` (`Action::SetPassphrase`) and `Sync Now`, and a row whose subtitle contains `Not encrypted`. The encrypt button opens an `AlertDialog` headed `Encrypt Sync with a Passphrase` with two `PasswordEntryRow`s, `Passphrase` and `Confirm passphrase`, and accept `Set Passphrase` (response id `accept`). Accept is disabled with no error when fresh. Mismatched (`correct horse`/`correct hose`) disables it with an error label (`The passphrases don't match`). Short (`horse`) disables it with an error (`Use at least 8 characters`). Valid (`correct horse` twice) enables it with no error. Capture `sync-passphrase.png`. Accepting stores an account with `Encryption::Ready`, and Settings shows `Change Passphrase…`.
- Removing the keyring (`set_secret_state("account.keyring", b"")`, `reload`): the prompt `Enter Your Sync Passphrase` comes by itself with a password field, `Start Syncing` and `Sign Out`. Closing it as Escape does keeps it open (`close()` returns false, `can_close()` false). A wrong passphrase reopens it with the error `The passphrase is wrong`. The right one leaves the account Ready, with only Settings open.
- Removing the keyring again: the prompt's `Sign Out` signs out (`is_signed_in()` false), leaving only Settings open.
- APIs: `vsesvit_sync::{Account::{save, load, encryption}, Encryption::{Off, Enter, Changed, Ready}}`, `vsesvit_sync::status::{OFFER, Action::{SetPassphrase, SyncNow, ChangePassphrase, SignOut}.label(), passphrase_dialog(Encryption) → PassphraseDialog{title, body, field, confirm, accept, dismiss}, check_passphrase, prompt(state, offered) → Prompt::{Offer, Enter}}`, `MIN_PASSPHRASE_CHARS = 8`, `core.sync().set_secret_state`. Shell: `Syncer::{reload, act, is_signed_in}`.
- **Engine-neutral.** KEEP.

### 5.60 `profiles` (line 3684)
- Alone: the profile menu (first section of `window.profile_menu()`) lists `["Person 1"]`, and the window title equals the page title.
- `home.dir.add("Work", ProfileColor::Green)` → `(work_id, registry)`, then `browser.set_profiles(registry)`. The menu lists `["Person 1","Work"]` and the title is `<page> - Person 1`.
- A private window (`BrowserWindow::with_browsing(browser, Private)`) is titled `<tab title> - Person 1 (Private)` with no profile menu (hidden button). It is destroyed (never shown).
- Sign-in details: `sync().take_account_details(AccountDetails{name:"Alex", picture: <origin>/allowed.png})` followed at once by one with no picture. After 1.5 s no picture is shown (the superseded picture is dropped).
- Again with the picture: the menu becomes `["Alex","Work"]` and the profile button shows the picture (`shows_profile_picture()`: the button child is an `adw::Avatar`). Capture `profiles-account.png`.
- Route `/slow-picture.png` (500 ms delay). A sign-in with that picture, waiting until the fetch has started. Then rename and recolour by hand: `browser.edit_profile(home.id, "Tester", ProfileColor::Teal)`. After 1 s the title is `<page> - Tester`. Then a sign-in as `Sam` with no picture: the menu is `["Tester","Work"]` and no picture (hand-set details win, and the new colour replaced the picture).
- `win.manage-profiles` dialog lists `Tester` and `Work` rows; capture `profiles-manage.png`.
- Settings > General `Show Profile Picker at Startup` switch is on. Turning it off stores `show_picker() == false` in `profiles.json`. Capture `profiles.png`.
- `browser.remove_profile(window, &work)`: Work's directory (`home.dir.root(&work)`) is deleted; the menu is `["Tester"]` and the title is back to the page title. No process is started.
- Core: `vsesvit_core::profiles::{Home{dir, id}, ProfilesDir::{at, add, load, opened, picture, root}, Registry::{get, show_picker}, ProfileColor, ProfileId, AccountDetails{name, picture}}`. The picture fetch is core's: `vsesvit_core::profiles::AccountPicture::fetch` (`profiles/account.rs`), HTTPS from a public address only in normal runs (PLAN.md "Details from sync"). The self-test serves it from the loopback fixture server over http, which works because `favicons::allow_local_hosts()` (feature `testkit`; called in §1.3 step 7) also lets account pictures reach loopback hosts over plain HTTP (`favicons/fetch.rs:100-113`, `profiles/account.rs:56`).
- **Engine-neutral.** KEEP.

### 5.61 `welcome` (line 3784; 60 s, +120 s with `--network`)
- Opened directly with `dialogs::welcome::present(window)` after resetting local pref `onboarding.done`. The self-test never shows it at startup. A cleanup restores the theme and the default engine.
- An `adw::Carousel` steps through `STEPS = [Welcome, Search, Import, Extensions, DefaultBrowser, Sync, Done]`. Next labels: Welcome `Get Started`, Sync `Skip`, Done `Start Browsing`, the others `Next`.
  - Search: one `CheckButton` row per engine (`search_engines().list()`), only the default checked. Activating another engine's row makes it core's default.
  - Import: wait until `Looking for other browsers…` is gone. One `Import` button per `vsesvit_core::import::installed_browsers()` entry, `No other browsers found` iff none, plus `Choose File…`. After capture: `Back`, then Next again, returning to the page.
  - Extensions: each `RECOMMENDED_EXTENSIONS` row (`uBlock Origin Lite`, `Bitwarden`, `Proton Pass`) shows exactly one of a mapped `Install` button or an `Installed` label. With `--network`, `Install` on Bitwarden (`nngceckbapebfimnlniiiahkandclblb`) installs it from the Chrome Web Store: `Installed` shown, in core, row not `error`.
  - DefaultBrowser: a wrapping status label other than the status page's title and description. Whether the `Make Vsesvit the Default Browser` button is visible is recorded only.
  - Sync: the `Sync Server` `EntryRow` shows pref `sync.server` and `Sign In` is visible. Applying `https://sync.example.com/` stores `https://sync.example.com` (trailing slash dropped). Applying an empty string stores and shows `DEFAULT_SYNC_SERVER` (`https://vsesvit-service.mrquantumoff.dev`) without the `error` class.
- Each page is captured in light (`welcome-<name>.png`) and, after `set_theme(Theme::Dark)`, in dark (`welcome-<name>-dark.png`); then the theme is restored. `Start Browsing` closes the dialog, and `onboarding.done` is true.
- Core: `vsesvit_core::onboarding::{should_show, RECOMMENDED_EXTENSIONS, Recommended{name, blurb, store, id}}`, `import::installed_browsers`, `search_engines()`, prefs `onboarding.done` (local), `sync.server` (local), `theme` (synced).
- **Engine-neutral** apart from the network install (NEEDS-CEF load). Doc delta: self-test.md omits the Sync page step and the network Bitwarden install.

### 5.62 `screenshot` (line 3933)
- `save_png(window, OUT_DIR/window.png)`. Reloads it as a texture and requires at least 2 distinct colours among the first 64 sampled. Detail: path, size, colour count.
- KEEP. NEEDS-CEF and GPUI capture (§2.3).

### 5.63 `cws_install` (line 3949; only with `--network`, 120 s)
- `browser.install(InstallSource::ChromeWebStore{id: ddkjiahejlhfcafbddmgiahcphecmpfh}, progress)`. Each `InstallPhase::describe()` goes into `Last`. Wait until the runtime has loaded it. Assert `verification == Verification::ChromeWebStore`. Detail: name, version, verification.
- Core pipeline against the real stores (`Stores::default()`): engine-neutral. Engine load: NEEDS-CEF (5.4).

---

## 6. self-test.md versus the Linux code

| topic | self-test.md | `self_test.rs` |
|---|---|---|
| checks present in code, missing from the doc table | n/a | `favicon_preload`, `star_bubble`, `extension_toolbar`, `address_progress`, `bookmarks_bar_menus`, `ctrl_s_toggles_sidebar`, `zoom_indicator`, `connection_info` |
| Windows-only rows (not in Linux `CHECKS`) | `bookmarks_bar_icons`, `download_pause`, `passwords_purged` | n/a. Windows also runs `shortcuts_sync` (`crates/vsesvit-winui/src/report.rs`), which the doc does not list. |
| order | table order | code order differs: `zoom_is_remembered_per_site` runs late (after `zoom_indicator`); `private_window` before `download_safety` and `download_speed`; `search_engines` after `spellcheck`; and so on. The source-scan unit test pins the code order. |
| `settings` pages | General, Appearance, Search, Privacy, Shortcuts | also `sync` (`settings-sync.png`) |
| `welcome` | Search, Import, Extensions, Default Browser, Start Browsing | also the Sync page (server entry and reset) and, with `--network`, a Bitwarden install |
| `--network` | `cws_install` | also `connection_info`'s HTTPS half (example.com) and `welcome`'s Bitwarden install |
| `navigate` detail | "a tab loads ..." | through the address bar (`submit_text`), and also checks the committed URI |
| `bookmark` | `is_bookmarked` and bar item | also the star action's state |
| `memory_saver` | "Settings > General shows the Memory Saver switch and the Memory Savings row" | also asserts the exact choices `[Moderate, Balanced, Maximum]` |

---

## 7. Hooks the self-test drives in the shell (the capabilities a new shell must expose to be testable)

These are `#[cfg(feature = "self-test")]` or `#[cfg(any(test, feature = "self-test"))]` methods in `crates/vsesvit-gtk/src` unless noted. Behavior-level names are given so the list does not depend on GTK types.

- Browser level: `core()` (the `Profile`), `windows()`, `windows_of(Browsing)`, `find_tab(id)`, `windowed(Windowed::{Bookmarks, History, Downloads(Browsing)})`, `install`, `install_asking`, `installed_extensions`, `update_extensions`, `uninstall_extension`, `set_extension_allowed_in_private`, extension runtime queries (`loaded`, `notification(id, nid)`, `notification_permission_changed`), `extension_toolbar`, `extension_shortcuts(keymap)`, `edit_keymap(f) → Vec<Taken>`, `apply_keymap`, `pref` / `set_pref` / `reset_pref` / `switch`, `set_engine_switch`, `set_spellcheck_language`, `engine().apply_prefs`, `set_tabs_position` / `tabs_position`, `set_home_button_visible`, `set_theme` / `theme`, `sleep_idle_tabs(now)`, `reopen_closed_tab(window)`, `can_reopen_closed_tab(Browsing)`, `private_session_lasts()`, `save_session_now()`, `bookmarks_changed()`, `downloads().{list(Browsing), cancel(id)}`, `trackers().{use_list, when_applied}`, `cookies().{apply, clear}`, `set_https_reach(Reach)`, `sync().{take_account_details, act, reload, is_signed_in}`, `home()`, `set_profiles`, `edit_profile`, `remove_profile`, `app()` accelerator tables (`accels_for_action`, `actions_for_accel`).
- Window level: `id()`, `title()`, CSS class `private`, `tabs()`, `selected_tab()`, `select_tab`, `open_tab(url, opener, Focus)`, `open_tabs`, `new_tab()`, `close_tab`, `close()`/`destroy()`, `present()`, `set_default_size`, `is_pinned` / `set_pinned`, `group_of(tab)`, `group_header(id)`, `group_editor_open()`, `tab_row_hidden(tab)`, `select_next_tab(back)`, `tab_list().row_counts()`, `tab_row_opacity`, `tab_row_buttons`, `tab_indicator`, `click_tab_indicator`, `tab_look`, `open_tab_menu(tab)`, `tab_menu_lines()`, `tab_search()` / `tab_search_buttons()`, `layout_probe()`, `address_bar()`, `bookmarks_bar()`, `home_button()`, `shows_downloads_button()`, `download_warning()`, `extension_action_button(id)`, `extension_popup_view()`, `close_extension_popup()`, `open_extension_context_menu(id)`, `open_extensions_menu()`, `profile_menu()`, `shows_profile_picture()`, `visible_dialog()`, `clipboard()`, named actions (`win.*`, `tab.*`, `tab-group.*`, `app.*`) activatable with targets.
- Address bar: `submit_text`, `focus_for_typing`, `type_text` (as typing: delete selection, insert at cursor), `press(key, mods)` (Enter activates), `observe() → {text, selection, highlighted, fills, open}`, `suggestions_popover()`, `bubble()`, `prompt()`, `click_security()`, `click_in_use()`, `click_zoom()`, `shown_zoom()`, `shown_in_use()`, `progress()`.
- Tab: `id()`, `session_id()`, `web_view()`, `committed_uri()`, `display_title()`, `is_blank()`, `is_asleep()`, `selection()`, `capturing()`, `security()`, `inspector_open()`, `load(url)`, `reload()`, `browsing()`.
- Tab search: `set_query`, `shown() → [(heading?, title, site)]`, `selected()`, `step(±1)`, `click(i)`, `press_enter()`, `says_no_results()`, `anchor()`, `popover()`, `is_open()`.
- Bookmarks bar: `shows_favicon(url)`, `button_for(url)`, `overflow() → (shown, hidden)`, `chevron()`, `item(i) → (Item, widget)`.
- Others: `session::snapshot(browser)`, `dialogs::bookmarks::export_to`, `save_page::save`, `page_menu::{add_extension_items, add_link_item, add_private_window_item, add_selection_item, add_page_items, name_add_to_dictionary}`, `bookmark_menu::open_context_menu`, `shortcut_settings::capture(..).{press, note, close}`, `extension_prompts::shown_lines`, `dialogs::welcome::{present, STEPS, Step::{name, next_label}}`, `dialogs::extensions::{UPDATE_BUTTON, re_enable_text}`, settings row-title constants (`dialogs/settings.rs:78-100`: `Tracking Protection`, `Always Use Secure Connections`, `Secure DNS`, `Check Spelling`, `Languages`, `Memory Savings`, `Show Profile Picker at Startup`, `Vsesvit Doesn't Save Passwords`; `dialogs/site_data.rs::SEE_ALL_ROW` = `See All Site Data`), `screenshot::{save_png, save_png_with_popovers}`, `crate::SCRIPTED`.
- Engine-level hooks the GTK test reaches straight through WebKit, which a CEF shell must provide another way: page JS evaluation with a result, title, committed URI, `is_loading`, `can_go_back`, `is_muted`, `zoom_level`, favicon, cookie jar reads, the private context's identity, the inspector's state, print to PDF, process-termination notification.

---

## 8. `test_support.rs` (GTK unit-test helpers, `#[cfg(test)]`)

The scaffolding for the GTK crate's widget tests (`#[gtk::test]`, run on one GTK thread with a display). 30 modules use it. Counts of `#[gtk::test]` per module: `address_bar` 8, `app` 1, `bookmark_editor` 1, `bookmark_menu` 2, `bookmarks_bar` 2, `browser` 12, `cookies` 1, `dialogs/bookmarks` 4, `dialogs/extensions` 4, `dialogs/history` 1, `dialogs/mod` 2, `dialogs/passphrase` 2, `dialogs/search_engines` 2, `dialogs/settings` 2, `dialogs/shortcut_settings` 1, `downloads` 4, `extensions` 2, `lib` 1, `permissions` 11, `profiles` 1, `save_page` 1, `session` 5, `sync` 1, `tab` 11, `view_source` 2, `window/ext_actions` 5, `window/groups` 4, `window/mod` 10, `window/tab_menu` 2 (`favicons` uses only `Server`).

- `browser() -> Browser`: one shared `Browser` per test process (thread-local `OnceCell`) on a scratch profile `scratch_dir("shared")`, with `Browser::new(&registered_app(), profile, None)`. The reason in the doc comment: "The extension runtime allows one per process" (**WEBKIT-ONLY**, a vsesvit-webext limit).
- `registered_app()`: `adw::Application` id `dev.mrquantumoff.vsesvit.Tests`, `NON_UNIQUE`, registered without a bus name.
- `scratch_dir(name)`: `$TMPDIR/vsesvit-gtk-tests/<pid>-<name>`, emptied. It first deletes sibling dirs whose pid no longer exists in `/proc`.
- `wait_until(what, done)`: iterates the default GLib main context until `done()`, panicking `timed out waiting for <what>` after **15 s** (`WAIT`).
- `settle(duration)`: runs the main loop for a fixed time.
- `TestWindow(BrowserWindow)`: derefs to the window and destroys it on drop, so a failed wait cannot leave a window in the next test's way.
- `Server::start(host, route)`: a plain HTTP server on a loopback host (`127.0.0.1` or another `127/8` address to make a different site). Calls `favicons::allow_local_hosts()`. One thread per connection. `url(path)` gives `http://host:port/path`. Replies (`Reply`):
  - `Page(title)`: 200 `<!doctype html><title>T</title><p>T`
  - `Body(content_type, bytes)`: 200
  - `Hang`: accepts and never answers (a provisional navigation; used by tests of loads that never commit)
  - `Drop`: closes without a response (a network error)
  - `StalledFile`: 200 octet-stream announcing `STALLED_FILE_SIZE` = 1,000,000 bytes and sending `STALLED_FILE_SENT` = 1,000
  - `NotFound`: 404

  Every response has `Cache-Control: no-store` and `Connection: close`.
- Uses: `Page` 36, `NotFound` 23, `Body` 19, `Hang` 7, `StalledFile` 5, `Drop` 3. `Hang`/`Drop`/`StalledFile` appear in tests named, for example, `a_tab_still_loading_its_first_page_is_saved`, `a_restored_tab_keeps_its_back_forward_state_until_it_commits`, `the_browser_shows_sources_that_pages_cannot_reach`, `an_error_page_leaves_the_history_title_of_the_page_that_failed`, `extensions_see_the_committed_url_while_the_next_page_loads`, `a_download_shows_live_progress_and_stays_on_the_list_once_cancelled`, `two_downloads_with_the_same_name_get_files_of_their_own`, `a_download_into_a_folder_that_cannot_be_made_says_why`, `a_request_during_a_provisional_load_is_from_the_page_on_screen`, `a_stopped_load_leaves_no_transition_behind`, `a_download_leaves_no_transition_behind`, `an_address_typed_over_a_load_still_counts_as_typed`.
- `permissions.rs` tests call `crate::SCRIPTED.set(false)` so popovers behave as for a user (autohide on outside click).
- Classification: the helpers are engine-neutral ideas (a shared browser, a scratch profile, a main-loop wait with timeout, auto-destroyed windows, a programmable loopback server with hang, drop and stall modes). The GLib main-context iteration and `adw::Application` are GTK-ONLY. *(inference)* `Server`/`Reply` duplicates much of `testkit::FixtureServer::route`. A new shell's tests can use the core one.

---

## 9. Engine-neutral versus WebKit-specific, per check

| check | as written | engine-touching parts |
|---|---|---|
| profile_open | neutral | none |
| install_prompt | neutral | unsupported-features line WEBKIT-ONLY (absent for the probe) |
| install_crx | neutral (core) | runtime load NEEDS-CEF |
| engine_loaded_extension | WebKit-specific | runtime list NEEDS-CEF; `pending_filters` WEBKIT-ONLY |
| favicon_preload | neutral | none (core fetch) |
| navigate | mixed | load, title, commit NEEDS-CEF |
| history_recorded | mixed | commit plus transition NEEDS-CEF |
| content_script, extension_port, dnr_blocked, dnr_site_allowed, dynamic_content_script | extension runtime (native in Chromium) | page eval NEEDS-CEF |
| extension_windows, extension_web_navigation, extension_cookies | extension runtime plus shell window and tab ids | id mapping, windows/tabs model NEEDS-CEF (high risk) |
| bookmark, star_bubble, bookmark_export | neutral | none |
| tabs | mixed | titles |
| tab_animation | neutral (GTK-ONLY measurement) | none |
| tab_layout | mixed | web view bounds |
| tab_menu | mixed | mute NEEDS-CEF; hand-built WebKit menu WEBKIT-ONLY |
| tab_search, tab_groups | neutral | titles |
| memory_saver | WebKit-specific | termination reason, URI release, restore_state WEBKIT-ONLY; discard plus history NEEDS-CEF (risk) |
| popup, extension_toolbar | mixed | action list, popup hosting NEEDS-CEF (risk) |
| context_menus | WebKit-specific plumbing | context menu handler NEEDS-CEF; action items no API |
| extension_commands | mixed | firing commands NEEDS-CEF (risk); keymap neutral |
| extension_notifications | WebKit runtime-specific | Chromium's notification bridge (risk) |
| extension_update | mixed | load/unload/replace NEEDS-CEF |
| omnibox | neutral | none |
| address_completion, search_suggestions | neutral | final navigation |
| selection_search | WebKit-specific plumbing | selection text and menu NEEDS-CEF |
| session | neutral | none |
| download, download_safety, download_speed | mixed | download handler NEEDS-CEF |
| private_window | WebKit-specific assertions in part | OTR context, cookies policy, extension-in-private NEEDS-CEF |
| new_tab_page | mixed | HTML at about:blank NEEDS-CEF |
| address_progress | mixed | progress NEEDS-CEF |
| settings | WebKit-specific first assertion | engine defaults NEEDS-CEF |
| spellcheck | WebKit-specific | Chromium spellcheck prefs and dictionaries (risk) |
| search_engines, bookmarks_bar_menus, ctrl_s_toggles_sidebar | neutral | none |
| shortcuts | neutral (accel tables) | in-page key handling NEEDS-CEF (untested on Linux) |
| save_page | mixed | MHTML NEEDS-CEF |
| page_commands | mixed | view-source fetch counts WEBKIT-ONLY; DevTools, PDF NEEDS-CEF |
| zoom_indicator, zoom_is_remembered_per_site | mixed | zoom NEEDS-CEF; Chromium's per-host zoom map |
| connection_info | mixed | certificate NEEDS-CEF |
| site_permissions | mixed | reload workaround WEBKIT-ONLY; permission handler plus content settings NEEDS-CEF |
| tracking_protection | mixed | blocker compile wait WEBKIT-ONLY; request blocking NEEDS-CEF; count could now appear |
| https_only | mixed | upgrade, error, interstitial NEEDS-CEF |
| cookies | WebKit-specific assertions in part | cookie manager, content settings, site data NEEDS-CEF |
| capture_in_use | WebKit-specific setup | media permission and indicator NEEDS-CEF; stop-one-device open question |
| sync_passphrase, profiles, welcome | neutral | (welcome's network install: load) |
| screenshot | mixed | page pixels in capture NEEDS-CEF/GPUI |
| cws_install | neutral core, engine load | load NEEDS-CEF |

### 9.1 Windows-only checks a Chromium shell could take on (reference; from self-test.md and `crates/vsesvit-winui/src/report.rs`)

- `bookmarks_bar_icons`: a page that changes its favicon four times, 300 ms apart, updates the bar in place (≥3 distinct icons, same entry objects). *(inference)* CEF `OnFaviconURLChange` (in bindings as `on_favicon_urlchange`) makes it feasible.
- `download_pause`: `/stalled.bin` Pause → core `Paused` keeping live counts, the button stops showing activity; Resume → `InProgress`; Cancel → `Cancelled`. *(inference)* feasible with `CefDownloadItemCallback::Pause` / `Resume`.
- `passwords_purged`: WebView2-specific (the local pref `passwords.purged` after clearing autosaved passwords). *(inference)* A CEF analogue would turn off Chromium's password manager prefs.
- `shortcuts_sync`: in the Windows list but not in self-test.md (not inventoried here).

---

## 10. Checklist

- [ ] `--self-test OUT_DIR [--network]` CLI with the exclusivity, repeat and `--network`-needs-`--self-test` rules; parse errors exit 2
- [ ] `self-test` cargo feature on by default pulling `vsesvit-core/testkit`; message when built without it
- [ ] Fresh profile at `OUT_DIR/profile` (deleted first); profile list `OUT_DIR/profiles.json` (deleted first) via `Home{ProfilesDir::at(OUT_DIR), "profile"}`
- [ ] Search suggestions turned off in the test profile before anything is typed
- [ ] `FixtureServer::start()` plus `favicons::allow_local_hosts()`; `probe.crx` written to OUT_DIR
- [ ] Scripted mode: no welcome at startup, no scheduled extension update checks, menus and popovers stay open for capture
- [ ] Checks run in sequence, each under its timeout (15 s default; 30 s for private_window and download_speed; 120 s network; welcome 60/180 s), timeout detail `timed out after N s; last observed: ...`
- [ ] Every check cleans up after itself even on failure
- [ ] Stdout lines `[self-test] <name>: ok|FAIL (<ms> ms) <detail>`, fixture origin line, final PASS/FAIL summary
- [ ] `report.json` via `testkit::report` (`platform: "linux"`, `ok`, `checks[{name, ok, ms, detail}]`), `complete("the self-test stopped before it")`, exit 0 only when `ok`
- [ ] Unit test that the expected list equals the checks the run makes, in order; unit test that an early stop fails
- [ ] In-app screenshots of a window including page content and open popovers, without stealing focus; all screenshot files of §2.2
- [ ] Page-script evaluation with a returned value, for every check that reads page state
- [ ] profile_open
- [ ] install_prompt: `Add “Vsesvit Probe”?` / `It can:` / two warnings; closing cancels; nothing committed
- [ ] install_crx: id `eonajgebgeenbhiiobbhmkafolkeghdb`, verification LocalCrx
- [ ] engine_loaded_extension: the engine reports the probe loaded under `PROBE_ID`
- [ ] favicon_preload: bar bookmark gets its icon with no tab opened (`favicon-preload.png`)
- [ ] navigate: address bar to `index.html`, title `Vsesvit fixture`, committed URL
- [ ] history_recorded: core history has the visit
- [ ] content_script: `data-vsesvit-probe = background-replied`
- [ ] extension_port: `data-vsesvit-probe-port = pong:probe`
- [ ] dnr_blocked: `/allowed.png` seen, `/vsesvit-blocked/pixel.png` never
- [ ] dnr_site_allowed: dynamic `allowAllRequests` rule lifts the static block, clearing restores it
- [ ] dynamic_content_script: registered script runs isolated on reload, gone after unregister
- [ ] extension_windows: windows.create, tabs.move to the end of the page's window (emptied window closes), windows.create with tabId plus windows.remove, exact event sequence, getAll equals the browser's windows
- [ ] extension_web_navigation: four events for frame 0 with transition `reload`, getAllFrames `[[0,-1,url]]`
- [ ] extension_cookies: set in store "0" listing the tab, visible to `document.cookie`, remove with two `explicit` onChanged events
- [ ] bookmark: star bookmarks into the bar, star active
- [ ] star_bubble: `Bookmark added` with the name selected, Done renames, second star click shows `Edit bookmark`
- [ ] bookmark_export: Bookmarks window menu Import/Export items; `bookmarks.html` round-trips the renamed bookmark
- [ ] tabs: open, switch back, close
- [ ] tab_animation: rows grow in and shrink out without delaying the tab model; reopen restores page2
- [ ] tab_layout: left default; right and top from geometry; restored to left
- [ ] tab_menu: exact sections and labels, clean Copy Link (menu, Ctrl+Shift+C, link's Copy Link Without Tracking), Duplicate, Pin (moves first, pin icon, session pinned, Unpin item), Mute indicator and unmute click, Close Others keeps pinned, Reopen, Close Tabs Below, New Tab Below, Move Tab to New Window keeps pin
- [ ] tab_search: Ctrl+Shift+A, ranking (title, then address, then recency), Recently closed, wrap-around selection, No results found, click switches window, Enter reopens in this window, toggle closes, opens from the tab bar's button when tabs are on top
- [ ] tab_groups: Add Tab to New Group plus editor, header collapse/expand, Ctrl+Tab skips hidden, close selects visible, child tab joins group, reopen restores group, session saves group, Ungroup
- [ ] memory_saver: only the idle unpinned clean background tab sleeps at +7 h; keeps title, half-opacity icon, session URL/title/history; wakes on select with Back; off means none; Settings switch plus Memory Savings combo (Balanced, insensitive when off)
- [ ] popup: toolbar action opens the popup page; title `visits=N≥1`
- [ ] extension_toolbar: button context menu (extension items, Unpin, Manage Extensions), puzzle menu pin toggle saving `toolbar.extensions`, popup from the puzzle row anchored to the puzzle, repin
- [ ] context_menus: `Vsesvit Probe page item` in the page menu before Inspect's separator, none on links, onClicked with pageUrl and tab; action menu starts with `Vsesvit Probe action item`, onClicked without pageUrl
- [ ] extension_commands: Alt+Shift+P / Alt+Shift+K bound, named command fires onCommand with tab and URL, `_execute_action` opens the popup, Settings row `Vsesvit Probe command`, reassign to Alt+Shift+J, Reset to Default
- [ ] extension_notifications: notification title, body, Open button and PNG icon; button fires onButtonClicked(0); Settings button opens the Extensions dialog on the probe with the Notifications switch; off fires `denied` and closes, on fires `granted`
- [ ] extension_update: fixture store install 1.0 (`1.0:1.0`), update to 2.0 keeps storage (`2.0:1.0`), 3.0 adding `tabs` withheld and off with `Enable “Vsesvit update probe”?` / `It can now:` / `• Read your browsing history`, switch insensitive, Update button insensitive while running, Re-enable runs it (`3.0:1.0`)
- [ ] omnibox: search versus URL classification
- [ ] address_completion: inline completion selected, Down fills row 1, Escape restores, Enter opens the root, list at least as wide as the bar
- [ ] search_suggestions: exactly one `/suggest` request, suggestions under the typed row, Down/Enter search, none for URLs or with the setting off
- [ ] selection_search: `Search <engine> for “<selection>”` right after Copy, opens a selected tab next to the page
- [ ] session: save and restore returns windows with tabs
- [ ] download: to the chosen folder, bytes match, Completed entry, downloads button shown, Downloads window (`downloads.png`)
- [ ] private_window: Ctrl+Shift+N; Open Link in Private Window after Open Link in New Window; `(Private)` title and style; separate ephemeral storage; third-party cookies blocked there by default; no cookie rule in its site info; no history or session; private zoom only; tab search per kind; private NTP text; extension only once allowed; private downloads list per kind; private closed tabs; everything gone when the window closes except the file
- [ ] download_safety: script download Unconfirmed in `.unconfirmed`; Keep moves it and completes; second goes to `dangerous (1).sh`; Discard deletes
- [ ] download_speed: speed and time left while running; `0 B/s - 1.0 KB of 1.0 MB` for a stalled download via the 1 s refresh; cancel
- [ ] new_tab_page: search box and top-site tiles while reading as `about:blank` and `New Tab`
- [ ] address_progress: progress line between 0 and 1 while loading, gone after
- [ ] settings: pop-ups blocked, smooth scrolling, GPU by default; home button shows and opens the homepage; General/Sync/Appearance/Search/Privacy/Shortcuts pages; passwords notice
- [ ] spellcheck: on by default in matching system languages, switch, per-language toggle and reset, Add to Dictionary wording, Settings rows
- [ ] search_engines: default not removable, duplicate-shortcut error, add, keyword search, remove
- [ ] bookmarks_bar_menus: overflow chevron menu with hidden items, folder menu with favicons and submenu, long titles ellipsized, context menu Edit… shows the URL
- [ ] ctrl_s_toggles_sidebar: Ctrl+S only toggles the tab sidebar
- [ ] shortcuts: reassignment applies at once, taking from another command, Settings shows it, capture-dialog conflict note, Reset All restores every default
- [ ] save_page: MHTML containing the page title
- [ ] page_commands: Ctrl+P, Ctrl+U, Ctrl+Shift+I and F12, Ctrl+Shift+J; view-source tab next to the page with source text; fresh view-source tab works; local page cannot reach a view-source page; Print… and View Page Source before Inspect; DevTools open and close toggle; print to PDF
- [ ] zoom_indicator: hidden at 100%, `110%` after zoom-in, bubble, hidden after reset
- [ ] zoom_is_remembered_per_site: 125% per host across pages, other host 100%, core remembers, reset forgets
- [ ] connection_info: http `Connection is not secure` without a certificate; with network, https `Connection is secure` with SHA-256, names and chain
- [ ] site_permissions: prompt `Know your location?`, Allow while visiting stores and reads `granted`, no re-prompt; site info Block stores and denies with code 1 without prompting; Settings Site Permissions lists and Remove resets with the empty-state text
- [ ] tracking_protection: tracker blocked; site info switch and status text; off stores the exception and reloads with the tracker loaded; Settings Standard and Strict
- [ ] https_only: Settings switch and Secure DNS row; upgrade fails to the `Connection is not secure` warning at the http address marked insecure; Continue loads over http and stores the exception
- [ ] cookies: third-party choice in Settings; site info Default, Block (empty cookies, deletion, no `served`), Clear on exit (stored, listed in Site Data, cleared on exit)
- [ ] capture_in_use: single camera-plus-microphone prompt with four answers; live tracks; address bar in-use button and tab camera indicator (sidebar and top); site info shows Allowed this time with Stop; Block camera stops only the camera; Stop ends the rest and the indicators
- [ ] sync_passphrase: offer once (Not Now remembered); Settings Sync state and buttons; set-passphrase dialog validation; Ready account and Change Passphrase…; enter-passphrase prompt that cannot be escaped, wrong-passphrase error, right one accepted; Sign Out signs out
- [ ] profiles: menu and titles with one or two profiles; private title and no profile button; superseded picture dropped; sign-in names the profile and shows the picture; hand rename and recolour win over later sign-ins; Manage Profiles; picker switch stored; remove deletes the directory
- [ ] welcome: every page with Next, Back/Next on Import; engine choice; import sources; recommended extensions Install/Installed; default-browser status; sync server set and reset; light and dark captures; Start Browsing sets `onboarding.done`
- [ ] screenshot: `window.png` with at least 2 colours
- [ ] cws_install (`--network`): uBlock Origin Lite from the Web Store, verification ChromeWebStore, loaded by the engine
- [ ] Fixtures kept: `tests/fixtures/site/*`, probe and update-probe extensions, test keys with README; the other fixture extensions as a compatibility spec (with fixes for Chromium ids, `externally_connectable`, MV2)
- [ ] GTK unit-test scaffolding equivalent: shared browser, scratch dirs, wait-with-timeout, auto-destroyed windows, loopback server with Page/Body/Hang/Drop/StalledFile/NotFound
