# Inventory: extension UI and integration in the WinUI shell

What the Windows shell (`crates/vsesvit-winui`, WinUI 3 + WebView2) does with extensions, written so that a GPUI + CEF Linux shell can rebuild every user-visible behavior without reading the old code. This is the closest existing reference for driving a Chromium extension system from Vsesvit: WebView2 is Chromium, and the shell around it had to draw every piece of extension UI itself.

Scope: only the extension parts of `crates/vsesvit-winui/src/`, plus the `vsesvit_core` APIs they call. No architecture is proposed.

Read at commit `27896e0` (branch `linux-cef-gpui`, worktree `/home/dy/git/vsesvit-cef`).

## Conventions

- **Verified** means I read the code or doc named. **Inference** is marked as such.
- `file:line` paths are relative to `crates/vsesvit-winui/src/` unless they start with `crates/`, `docs/` or `tests/`.
- Classification of engine-touching pieces:
  - **(KEEP)**: product behavior the new shell must reproduce.
  - **(WEBVIEW2-ONLY)**: a workaround for a WebView2 or Windows limitation. This is the WinUI counterpart of the task's (WEBKIT-ONLY) tag, and it likely goes away or changes shape with CEF. No piece of this shell is WebKit-specific, so (WEBKIT-ONLY) does not occur here.
  - **(NEEDS-CEF)**: needs a CEF equivalent. Likely CEF handlers and APIs are named where I know them. Anything about CEF that I did not check in the `cef-dll-sys` 154.5.0+154.0.34 bindings is marked as inference.

## 0. Module map (verified)

| File | Lines | Extension role |
|---|---|---|
| `extensions.rs` | 1064 | `ExtensionHost` state. Drives core's install, update and reconcile pipeline. Brings WebView2's extension list in line with core's (`sync_engine`). Writes the engine `key`. Schedules update checks. |
| `engine.rs` | 459 | The single `CoreWebView2Environment` with extensions enabled. Engine-side add, list, enable and remove. Long-path handling. The built-in extension ids. |
| `extension_toolbar.rs` | 339 | Pinned action buttons, the puzzle-piece Extensions menu, each button's context menu, drag to reorder. |
| `popup.rs` + `js/popup.js` | 347 + 110 | `ExtensionAction` (the toolbar model). The popup flyout hosting a WebView2 at `chrome-extension://<engine id>/<popup>`. The size protocol. The `chrome.tabs` shim. |
| `dialogs/extensions.rs` | 511 | The Extensions dialog: install box with progress, the inline install prompt, Update, the installed list (switch, Re-enable, Remove). |
| `store.rs` + `js/store.js` | 285 + 105 | Install, uninstall and list from Chrome Web Store and Edge Add-ons pages through a replaced `chrome.webstorePrivate` / `chrome.management`. |
| `shortcuts.rs` | 1385 | `Bindings`. Extension action commands bound as `Reserved`. The isolated-world page script that also carries store requests. |
| `dialogs/shortcut_settings.rs` | 686 | Settings > Keyboard shortcuts, "Extension shortcuts" section. |
| `dialogs/welcome.rs` | 685 | Welcome page "Add extensions worth having" (recommended installs). |
| `browser.rs` | 1831 | Startup wiring, `shortcuts_changed`, `sync_applied`, toolbar preference helpers, `engine_profile`. |
| `window/mod.rs`, `window/chrome.rs`, `window/wiring.rs`, `window/tab_layout.rs` | | Toolbar XAML, opening popups, the private-window refusals, accelerators, the "Extensions" main-menu item, drag-region exclusion. |
| `tab.rs` | 2061 | Injects the store script and the shortcut world. Routes `PageMessage::Store`. Context-menu additions (WebView2 adds extension items itself). |
| `omnibox.rs`, `cli.rs`, `config.rs`, `lib.rs`, `sync.rs`, `dialogs/mod.rs`, `pickers.rs` | | `chrome-extension:` URLs in the address box, `--load-extension`, the `chrome_version` sent to the Web Store, the synced-preference effects, `confirm_for_page`, the file and folder pickers. |
| `selftest.rs`, `selftest/*.rs`, `automation/*.rs`, `report.rs` | | Self-test checks and ui-smoke steps covering all of the above. |

## 1. Ownership model (verified)

- **Core owns the installed set.** `vsesvit_core::extensions` (`crates/vsesvit-core/src/extensions/mod.rs`) keeps two tables:
  - desired state, synced: the `extensions` table (`ExtensionRecord { id, store: Lww<StoreRef>, installed: Lww<bool>, enabled: Lww<bool> }`). Only store installs have a record.
  - actual state, local: `extension_installs`, with the version, the immutable dir, the source, the verification, the manifest JSON, `local_enabled`, `engine_id` and `granted`.
- **The engine mirrors core.** WebView2 persists its own extension registry in the profile's `engine/` user data folder. The shell never trusts that registry as the source of truth. After every change it runs `sync_engine` (`extensions.rs:489`), which adds, enables, disables and removes in WebView2 until WebView2 matches `Extensions::list()`.
- **The engine id is recorded in core.** `InstalledExtension::engine_id` is the id WebView2 assigned to *this* dir. It is recorded with `Extensions::set_engine_id`.
- **The toolbar shows loaded extensions only.** An extension gets a button only when `engine_id` is set and it is enabled (`ExtensionAction::from_installed`, `popup.rs:63`).
- **Threading.** `Profile` lives on the UI thread. Slow work runs on a worker thread through `exec::background` and is committed on the UI thread:
  - `InstallJob::run` (download, verify, unpack)
  - `UpdateCheck::run`

  All engine calls are async WinRT calls on the UI thread, wrapped in timeouts (`extensions.rs:30-34`):
  - `ENGINE_CALL_TIMEOUT` = 20 s
  - `ENGINE_ADD_TIMEOUT` = 90 s ("adding a large extension makes the engine index its rulesets")
  - `SYNC_WAIT` = 180 s
  - install progress is polled every 100 ms (`PROGRESS_INTERVAL`)

  Classification: the split is (KEEP). The timeouts are (WEBVIEW2-ONLY) in their exact values, but some guard against engine calls that never answer is (NEEDS-CEF).
- **`ExtensionHost`** (`extensions.rs:37`) holds:
  - `actions: Vec<ExtensionAction>`
  - sync generation counters (`sync_requested` / `sync_done` / `sync_running`)
  - `last_sync_error`
  - `engine_errors: HashMap<ExtensionId, String>`, shown in the dialog
  - weak listeners notified after each change to the installed set (the Extensions dialog subscribes, `dialogs/extensions.rs:155`)
  - `reconcile_running` / `reconcile_again`
  - `update_running`

## 2. Engine environment and loading (`engine.rs`)

### 2.1 Environment (verified)
- **Creation.** `Engine::create(user_data_dir, arguments)` (`engine.rs:96`) creates one `CoreWebView2Environment`:
  - options set `ICoreWebView2EnvironmentOptions6::SetAreBrowserExtensionsEnabled(true)`
  - optional `SetAdditionalBrowserArguments`
  - `CreateWithOptionsAsync("", <profile>/engine, options)`

  Every web view must use this environment (WebView2 refuses a second one with different options, ERROR_INVALID_STATE). The user data dir is `Profile::paths().engine_data` = `<profile>/engine` (`crates/vsesvit-core/src/lib.rs:165`). (WEBVIEW2-ONLY mechanics; NEEDS-CEF: the CEF counterpart is the `CefSettings.root_cache_path`/`cache_path` and the `CefRequestContext` whose profile holds extensions.)
- **Extensions live on the normal profile only.**
  - Private tabs use controller options `IsInPrivateModeEnabled` (`engine.rs:145`).
  - WebView2 has no per-extension InPrivate switch, and adding an extension to the InPrivate profile may reinstall it into the normal one, so the shell touches no extension there (`engine.rs:9-11`).
  - (WEBVIEW2-ONLY reason. The product rule "extensions off in private windows unless allowed" is (KEEP), see §13.)
- **Reaching the profile.** WebView2 gives a profile only through a web view.
  - `BrowserWindow::engine_profile()` waits up to 30 s for any tab's `CoreWebView2` and takes `ICoreWebView2_13::Profile()` (`window/mod.rs:2002`).
  - `Browser::engine_profile()` uses the first *normal* window (`browser.rs:1757`). While only private windows are open there is none, so every engine sync fails with "no web view to reach the engine through".
  - (WEBVIEW2-ONLY. CEF has a global or per-request-context profile without needing a browser. Inference.)
- **Chrome version for the store.** `lib.rs:209-220` reads the WebView2 runtime version (`engine::available_version`, `engine::chromium_version` takes the first word when it is four numeric parts). It passes it as `OpenOptions::chrome_version`, which core sends to the Chrome Web Store as `prodversion`. Linux keeps `DEFAULT_CHROME_VERSION` = `"150.0.0.0"`. (KEEP. NEEDS-CEF: the CEF build's Chromium version is `CHROME_VERSION_*` = 154.0.8037.98 in `cef-dll-sys` 154.5.0, verified in the bindings' constants.)

### 2.2 Engine extension calls (verified)
- `engine::add_extension(profile, dir)` → `ICoreWebView2Profile7::AddBrowserExtensionAsync(path)` returns an `EngineExtension { id, name, enabled, handle }`.
  - WebView2 persists it, keeps the folder in place, and drops the extension if the folder's files change afterwards.
  - Hence core's dirs are immutable: `extensions/<id>/<version>_<sha256 prefix 32 hex>/`.
- `engine::extensions(profile)` → `CoreWebView2Profile_Manual3::GetBrowserExtensionsAsync()`.
- `EngineExtension::set_enabled` → `EnableAsync`. `EngineExtension::remove` → `RemoveAsync`.
- (All NEEDS-CEF. CEF 154 has no extension-management API in its C API, see §18.)

### 2.3 Windows path workarounds (verified; all WEBVIEW2-ONLY)
- **`MAX_PATH`.** `engine_path` (`engine.rs:349`) gives the engine `\\?\C:\...` when the dir path + 1 + max(longest relative entry, 64 chars of engine metadata such as `_metadata\generated_indexed_rulesets\_ruleset1`) passes 259 characters.
- **Deep folders.** `DEEP_FOLDER_CHARS` = 200 (`extensions.rs:641`). WebView2 fails for large extensions past ~215 characters even with the extended form. `engine_refusal` appends: "(its folder path is N characters long; WebView2 cannot load larger extensions from folders this deep, so use a profile folder with a shorter path)".
- **Built-ins.** `BUILTIN_EXTENSIONS` (`engine.rs:288`) holds `dgiklkfkllikcanfonkcabmbdfmgleag` (Microsoft Clipboard Extension) and `mhjfbmdgcfjbbpaeojofohoefgiehjai` (Microsoft Edge PDF Viewer). They are recognized by id, never by name, and are never removed. Inference: `mhjfbmdgcfjbbpaeojofohoefgiehjai` is also Chromium's PDF viewer component extension id, so a CEF shell probably needs the same "leave the engine's own component extensions alone" rule (NEEDS-CEF).

## 3. Engine sync algorithm (`extensions.rs:489`, `sync_engine`) (verified)

This is the core contract for keeping any Chromium extension registry in line with core.

1. `engine.list()` gives the loaded extensions. `list()` is core's `Extensions::list()`, in install order.
2. **For each wanted extension**, skip it when WebView2 already lists its recorded `engine_id`, or when `withheld` is non-empty (an update awaiting Re-enable; the engine starts everything it adds, so a withheld version is never added).
3. **Synthetic key.** If `engine_id` is `None`, the install is managed (not `InstallSource::Unpacked`), and the manifest has no `key`, write a synthetic key:
   - `key = base64(STANDARD, "vsesvit-extension:" + core id)` (`engine_key`, `extensions.rs:592`).
   - It is inserted as the first member of `manifest.json`, textually, so comments survive (`add_manifest_key`, `object_start` skips a BOM, whitespace, `//` and `/* */`).
   - The write goes to `manifest.json.vsesvit-new` and is then renamed.
   - It is idempotent when the key is already present.
   - Why: Chromium derives an unpacked extension's id from `key`, else from the folder path, and every version of a managed install gets a new folder. A key made from core's id keeps the engine id, and with it `chrome.storage`, across versions. This matters for XPI installs, whose `key` core strips; CRX installs carry the verified developer key (core's `install::set_key`).
   - A developer's unpacked folder is never written to.
4. **Add.** `engine.add(dir)`. On success, `record_engine_id(id, added.id)`. If core says disabled but the engine started it, call `set_enabled(false)` right away; on failure, record the error "could not be switched off: …". On refusal, record `engine_refusal(e, dir)` and log "engine refused …".
5. **Second pass.** List the engine again:
   - For each loaded extension whose id core recorded with a different enabled state: `set_enabled(w.enabled)`. On failure, the error is "could not be switched on/off: …".
   - A loaded extension that core does not know (no recorded engine id) and that is not a built-in is removed ("removing X from the engine"). This also unloads extensions another device uninstalled, since reconcile already deleted their rows.
6. The function returns the per-extension errors, which become `ExtensionHost::engine_errors` and are shown in the dialog.

**Add comes before remove.** Adding a folder whose `key` gives an id the engine knows replaces that extension in place, keeping its storage. That is how updates take effect: the new dir has `engine_id = None` after core's `commit`, so it is added and replaces the old version in place (`extensions.rs:483-488`). Unit tests cover this at `extensions.rs:907-1063`: built-ins left alone, unrecorded removed, disabled switched off after add, failed switch reported, keyless versions keep the engine id, withheld not loaded until approved.

**Serialization** (`sync_extensions`, `extensions.rs:316`):
- Each call bumps a generation counter and starts `sync_loop` if none is running.
- It waits (max 180 s) until a pass that started after the call has finished, then returns that pass's error.
- After every pass: `refresh_extension_actions()` (rebuilds the toolbar model), `shortcuts_changed()` (rebinds extension action chords), `notify()` (the dialog re-renders).

Classification: the whole idempotent mirror is (KEEP as a requirement: core decides, the engine follows), and its engine calls are (NEEDS-CEF). Key injection is (KEEP) for any Chromium: the id comes from `key`. The synthetic key for XPI is (KEEP) if CEF loads XPIs at all. Inference: Chromium hashes the base64-decoded `key` bytes without checking that they are a valid SPKI, which is why WebView2 accepts the synthetic key; verify the same for CEF.

## 4. Install paths

All of them go through core: `Extensions::prepare_install(source)` → `InstallJob::run(progress)` on a worker thread → `StagedInstall` → (prompt if `needs_approval()`) → `Extensions::commit(staged)` → `sync_extensions()`.

### 4.1 Browser methods (verified, `extensions.rs`)

| Method | What it does |
|---|---|
| `stage_extension(source, progress)` (`:198`) | `prepare_install` plus the job on a worker thread. Relays `InstallPhase::describe()` / `fraction()` as `Progress { text, fraction }` every 100 ms. Nothing is committed. |
| `commit_extension(staged)` (`:164`) | `commit`, `notify`, `sync_extensions`, then re-reads the extension ("the extension was removed meanwhile" if it is gone). |
| `install_extension(source, progress)` (`:153`) | Stage, then commit **without asking**. Used by `--load-extension`, the welcome's picks and scripted runs. |
| `install_package` (`:179`) | Stage and commit in core only. No engine sync. Self-test only. |
| `run_install_job` (`:210`) | Reconcile jobs: job then `commit`. |

`commit` returns `Ok(None)` with "the extension is no longer wanted" when a reconcile or update job is no longer desired.

### 4.2 Startup (verified)
- `browser.rs:321` spawns `start_extensions()` after the first windows open (`extensions.rs:84`):
  1. Each `--load-extension DIR` (`cli.rs:17`, repeatable; made absolute in `config.rs:114`) installs through `InstallSource::from_path` + `install_extension`. The result is logged: `--load-extension: <name> <version> from <dir>`.
  2. Then `reconcile_extensions()`.
- In interactive mode only, `browser.rs:323-325` spawns `schedule_updates` (§7).

### 4.3 Reconcile with other devices (verified, `extensions.rs:105-149`)
- One reconcile at a time. A request during a run sets `reconcile_again`, and the loop runs once more.
- `reconcile_once`:
  1. `Extensions::reconcile()` returns `Reconcile { install: Vec<InstallJob>, removed: Vec<ExtensionId> }`.
  2. Removed ones are only logged ("uninstalled on another device"); their rows are already gone.
  3. Each install job runs and commits (no prompt, as in Chrome).
  4. `sync_extensions()` then unloads the removed ones and loads the new ones.
- Triggers: startup, and `Browser::sync_applied` when `changed.extensions` (`browser.rs:1590`).
- (KEEP.)

### 4.4 The Extensions dialog install box: §6.

### 4.5 Store pages: §5.

### 4.6 Welcome recommended extensions (verified, `dialogs/welcome.rs:152-660`)
- **Page.** Title "Add extensions worth having". Text: "Each comes from the Chrome Web Store, checked against its publisher's signature. Add any now, or later from Extensions."
- **Rows.** One card per `vsesvit_core::onboarding::RECOMMENDED_EXTENSIONS`:
  - uBlock Origin Lite `ddkjiahejlhfcafbddmgiahcphecmpfh`
  - Bitwarden `nngceckbapebfimnlniiiahkandclblb`
  - Proton Pass `ghmbeldphafepmbegfdlkpapadhbakde`

  Each card shows the name, the blurb, a hidden progress bar and status, and an Install button (MinWidth 96, automation name "Install <name>").
- **Button states:**
  - "Install"
  - "Installing…" (disabled, progress shown, status "Starting" then the phase text)
  - "Installed" (disabled)
  - "Try again" after a failure, with status "Not installed: <e>"

  An engine refusal shows "WebView2 did not load it: <e>". Installed state comes from `Extensions::get(&recommended.id())`.
- **No install prompt.** `install_extension` is used: the user's explicit pick.
- (KEEP. The "WebView2" wording becomes engine-neutral.)

## 5. Installing from store pages (`store.rs`, `js/store.js`, `shortcuts.rs` page script)

### 5.1 The problem WebView2 had (verified, `store.rs:1-14`, `docs/design/research-winui.md:65`)
- **Chrome Web Store.** WebView2 exposes `chrome.webstorePrivate` and `chrome.management` on `chromewebstore.google.com` only, and `beginInstallWithManifest3` never answers: there is no install prompt, so the store waits forever.
- **Edge Add-ons** uses the same `webstorePrivate` calls (`isMicrosoftStore: true`) and enables its Get button once the API exists.

(WEBVIEW2-ONLY problem. The product behavior, "Add to Chrome" / "Get" on the store page installs through Vsesvit's pipeline after Chrome's prompt, is (KEEP).)

### 5.2 Page side: `js/store.js`, main world of every document (verified)
- **Where it runs.** Injected with `Page.addScriptToEvaluateOnNewDocument` (no world name = main world) in every tab's DevTools session (`tab.rs:460-463`). It does nothing unless `location.hostname` is `chromewebstore.google.com` (store `"chrome"`) or `microsoftedge.microsoft.com` (store `"edge"`), and only in the top frame.
- **Requests.**
  - `ask(op, args)` dispatches `document` `CustomEvent("vsesvit-store", { detail: JSON.stringify({...args, seq, op}) })`.
  - Replies arrive as `window` `CustomEvent("vsesvit-store-reply", { detail: <JSON string> })` and are matched by `seq`.
  - The callback-or-promise convention follows Chrome: with a callback, `chrome.runtime.lastError` is defined around the call; without one, a promise.
- **`chrome.webstorePrivate` replacements:**
  - `beginInstallWithManifest3(details, cb)` → `ask("install", {store, id: details.id, name: details.localizedName})`. On ok it resolves `""` and, when `management` is emulated, fires `management.onInstalled` listeners. Otherwise it answers with the error and the result `"user_cancelled"` (if cancelled) or `"unknown_error"`.
  - `completeInstall` and `completeInstallWithCV` succeed immediately (the install already happened).
  - `install(...)` fails with "Installs start from the Add button".
  - Edge only:
    - `getExtensionStatus(id, manifest, cb)` → `ask("list")` → `"enabled"` / `"disabled"` / `"installable"`
    - `getFullChromeVersion` → `{version_number: <Chrome/x.y.z.w from the UA>}`
    - `isInIncognitoMode` → `false`
- **`chrome.management`.** On the Chrome Web Store the native `chrome.management` is kept (the store reads install state from it; a stand-in left the button at "Add to Chrome"). Elsewhere (Edge) it is emulated: `getAll`, `get` ("No extension with id X"), `onInstalled` / `onUninstalled` / `onEnabled` / `onDisabled`. Items are described as `{id, name, version, enabled, type: "extension", installType: "normal", mayDisable: true, isApp: false}`.
- **`management.uninstall(id)`** → `ask("uninstall", {store, id})`; fires `onUninstalled` when emulated.

### 5.3 Bridge to the shell (verified)
- **Forwarding.** The shortcut page script (`shortcuts.rs:646`, `page_script()`) runs in a secret isolated world (§11.3). In the top frame only, it listens for `document` `"vsesvit-store"` events with a string detail and reports `{t: "store", origin: location.origin, detail}` through the DevTools binding `vsesvitShortcut`. The page cannot forge the origin, because the isolated world reads `location.origin` itself.
- **Parsing.** `shortcuts::parse_page_message` → `store::parse_request(origin, detail)` (`store.rs:76`):
  - The store is decided by the **sender's origin**, never by the page's claim. Only exactly `https://chromewebstore.google.com` or `https://microsoftedge.microsoft.com`; http, other ports and look-alike hosts are refused.
  - `op` ∈ {`list`, `install`, `uninstall`}. Install and uninstall need an `id` that `ExtensionId::is_chrome_style()` accepts (32 chars `a-p`). `seq` must be a u64.
  - Messages from frame sessions (`in_frame`) are refused except keys.
- **Routing.** `tab.rs:1740` routes `PageMessage::Store(request)` to `store::answer(window, tab_id, request)`.

### 5.4 Carrying out the request (verified, `store.rs:105-183`)
- **The tab must still show the store.**
  - The tab's committed origin (`Tab::origin()`) must be that store's origin when the request arrives; otherwise "ignoring a store request from …".
  - It must also still be that store when the answer is handed back; otherwise "dropping the answer … the tab left it" and nothing is sent. Asking the user can take a while.
- **`list`** → `{extensions: [describe(ext) for Extensions::list()]}`, each `{id, name, version, enabled}`.
- **`install {id}`:**
  1. Already installed (`Extensions::get`) → `{ok: true, installed}` with no prompt.
  2. Otherwise `stage_extension(Store::source(id))` (`InstallSource::ChromeWebStore` or `EdgeAddons`). It is downloaded and verified first, so the prompt describes the package itself, not what the page claims.
  3. Then `dialogs::confirm_for_page(window, install_heading(name), prompt_text(manifest), "Add extension")`: a `ContentDialog` over the window with primary button "Add extension", close button "Cancel", and **DefaultButton = Close** so that Enter, which a page can time, does not accept (`dialogs/mod.rs:300`).
  4. Cancel → `{ok: false, cancelled: true, error: "User cancelled install"}`.
  5. Add → `commit_extension`. An engine error becomes `Err("WebView2 did not load it: …")`. Success → `{ok: true, installed}`.
- **`uninstall {id}`.** Not installed → error "This extension is not installed". Otherwise `confirm_for_page("Remove “<name>”?", "", "Remove")`. Cancel → error "User cancelled uninstall". Yes → `uninstall_extension(id)` → `{ok: true}`.
- **Reply.** `{..., seq}` is delivered by `tab.eval` (main world) of `dispatchEvent(new CustomEvent("vsesvit-store-reply", { detail: "<json>" }))`, with the JSON double-encoded as a JS string literal (`reply_script`, `store.rs:196`).
- **Private windows.** `store.rs` has no private-window check: the store script and bridge run in every tab, including private ones, and an install from a private tab would install into the normal profile. Verified from the code. What WebView2's InPrivate store page actually offers is not verified.

Classification: the product behavior (store button → Chrome prompt → core pipeline → page sees installed state) is (KEEP). The JS shim is (WEBVIEW2-ONLY). The CEF equivalent is (NEEDS-CEF). Options to research:
- (a) let the Chrome runtime's own `webstorePrivate` and install prompt run, then mirror into core;
- (b) inject a shim the same way, with `CefFrame::ExecuteJavaScript` / a renderer-side `CefV8Handler` or a DevTools `Page.addScriptToEvaluateOnNewDocument` via `CefBrowserHost::ExecuteDevToolsMethod`, plus a `CefProcessMessage` or DevTools binding back.

Which `chrome.*` the Chrome runtime exposes on these pages inside CEF is not verified.

## 6. The Extensions dialog (`dialogs/extensions.rs`)

### 6.1 Presentation (verified)
- **Container.** A `ContentDialog` over the browser window, titled "Extensions", with a Close button (`DefaultButton="Close"`). It is not a separate window (`Dialog::window_size` is `None` for Extensions, `dialogs/mod.rs:76-83`). Scripted runs show it as an overlay preview without taking focus.
- **Install prompt placement.** Only one `ContentDialog` can show at a time, so the install prompt is drawn *inside* this dialog: "a dialog cannot open over this one".
- **Entry points:**
  - main menu "Extensions" (`MenuExtensions`, glyph EA86, `window/chrome.rs:286`, wired `window/wiring.rs:173`); enabled in private windows too
  - "Manage extensions" in the puzzle menu
  - "Manage extensions" in a pinned button's context menu (`toolbar::Command::Manage` → `show_dialog(Dialog::Extensions)`, `window/mod.rs:1831`)
- **Layout** (`MARKUP`, `dialogs/extensions.rs:24`), a 640 px StackPanel with spacing 12:
  1. "Add an extension" (BodyStrong).
  2. A grid row:
     - `TextBox InstallSource` with placeholder "Chrome Web Store, Edge Add-ons or addons.mozilla.org link, extension ID, or file path"
     - accent button "Install"
     - button "File..." (tooltip "Install a .crx or .xpi file")
     - button "Folder..." (tooltip "Load an unpacked extension folder")
  3. `ProgressBar InstallProgress` (collapsed).
  4. `InstallPrompt` card (collapsed): heading, text, buttons "Cancel" and accent "Add extension", right-aligned.
  5. `TextBlock InstallStatus` (secondary color, wraps).
  6. Header row "Installed" with a right-aligned button "Update" (tooltip "Update every extension from its store now").
  7. `ListView ExtensionsList` (MaxHeight 340, no selection).
  8. "No extensions installed" while the list is empty.

### 6.2 Install box behavior (verified)
- **Install.** `InstallSource::parse(text)`. A parse error shows its message in the status, for example `SourceParseError::Unrecognized`: "not a Chrome Web Store, Edge Add-ons or addons.mozilla.org extension URL, an extension id, or an extension file".
  - Accepted inputs (core `install.rs:142-172`): CWS detail URLs (new and legacy `chrome.google.com/webstore`), Edge Add-ons detail URLs, a bare 32-char a–p id (meaning CWS), AMO addon URLs, bare gecko ids (`name@domain`, `{uuid}`; a bare slug is not accepted), `file://` URLs, paths ending `.crx` or `.xpi`, a directory with `manifest.json`, or `manifest.json` itself.
- **File... and Folder...** `pickers::pick_file(owner, [".crx", ".xpi"])` / `pickers::pick_folder(owner)`, owned by the browser window. The chosen path is written into the box, then `InstallSource::from_path` → install. A picker failure shows "Could not open the picker: <e>"; a bad path shows "<path>: <e>".
- **One install at a time** (`busy`). While installing, the three install buttons are disabled and the progress bar shows, indeterminate until a fraction is known. The status shows "Starting", then `InstallPhase::describe()`:
  - "Looking up the add-on…"
  - "Downloading… X of Y" / "Downloading… X"
  - "Verifying the signature…"
  - "Unpacking…"
  - "Reading the manifest…"

  With a `fraction`, the bar becomes determinate (0..1).
- **After staging.** `staged.needs_approval()` is true for `Intent::User` and a non-Unpacked source (`install.rs:821`):
  - True: show the prompt (§6.3).
  - False: an unpacked folder commits immediately, without asking, as in Chrome.
- **Results:**
  - "Installed <name> <version>."
  - "Installed <name> <version>, but WebView2 did not load it: <engine error>"
  - "Not installed: <e>"

  Then the list re-renders.
- **No drag-and-drop install** anywhere in the shell: no `.crx` drop on the window or dialog (verified by grep). There is no download interception for `.crx` either.

### 6.3 Install prompt (verified)
- **Heading:** `permissions::install_heading(name)` = `Add “<name>”?` (curly quotes).
- **Text:** `prompt_text(manifest)` (`:462`):
  - empty when `permissions::install_warnings(manifest)` is empty, and the text block is then hidden
  - otherwise `INSTALL_LEAD` ("It can:") followed by lines `• <warning>`; details (sites behind "a number of websites") are indented as `    ◦ <site>`
- **Example** (unit test `:497`): `It can:\n• Read and change your data on a number of websites\n    ◦ a.example\n …\n• Read and change your bookmarks`.
- **Probe expectation:** `It can:\n• Read and change all your data on all websites\n• Display notifications`.
- **Warning source.** Core words the warnings with Chrome's message rules and coalescing. Content scripts' sites count (`permissions::prompted`).
- **Buttons.** Add → commit + engine sync. Cancel → the staged install is dropped, the status is cleared and nothing is committed.
- The same heading and text are reused by the store path's `confirm_for_page`.
- (KEEP.)

### 6.4 Installed list rows (verified, `row`, `:331`)
- **Order.** `Extensions::list()` order (install order). Every installed extension is shown, enabled or not, including withheld.
- **Columns:**
  - a 32 px icon: `best_icon(manifest.icons)`, else the action's `default_icon`; best = the size closest to 32; fallback FontIcon glyph EA86
  - name (BodyStrong, trimmed)
  - a details line `"<version> · <verification label> · <id>"`, with labels from `Verification::label()`:
    - "Chrome Web Store, publisher verified"
    - "Edge Add-ons, publisher verified"
    - "Firefox Add-ons, hash checked"
    - "Local CRX, signature verified"
    - "Local XPI, not verified"
    - "Unpacked folder, not verified"
  - an engine error line in the critical color: "WebView2 did not load it: <e>"
  - a Re-enable notice in the caution color: `re_enable_text` = `Enable “<name>”?` + `\n` + "It can now:" + `• <withheld warning>` lines (`permissions::re_enable_heading`, `RE_ENABLE_LEAD`)
  - a "Re-enable" button, visible only with the notice
  - a `ToggleSwitch Enabled` ("On" / "Off", automation name "Enabled")
  - a "Remove" button
- **Switch.**
  - It shows `ext.enabled` (the user's choice AND nothing withheld).
  - It is **disabled while withheld** ("An update's new permissions keep it off whatever the switch says, until re-enabled").
  - Toggling calls `set_extension_enabled(id, on)`, which is `Extensions::set_enabled` (the synced register for store installs, `local_enabled` otherwise) followed by an engine sync.
  - On failure: "Could not change <id>: <e>", and the list re-renders, so the switch snaps back to core's state.
- **Re-enable.** `approve_extension_permissions(id)` → `Extensions::approve_permissions` (grants what the update added, clears `granted`, clears `engine_id` so the new dir is loaded) → sync. Statuses: "Re-enabled <name>." / "Could not re-enable <name>: <e>".
- **Remove.** Immediate; the dialog row has **no confirmation**, unlike Chrome. `uninstall_extension(id)` (`extensions.rs:292`):
  1. Unload from the engine first (find by recorded `engine_id`, `RemoveAsync`) so core can delete files Windows would otherwise keep locked.
  2. `Extensions::uninstall(id)`. If core refuses, sync again so the extension keeps running, and return the error.
  3. Otherwise notify and sync.

  Statuses: "Removed <name>." / "Could not remove <name>: <e>". Core's uninstall sets synced `installed := false` for store installs, deletes the local row and `storage.local` data, keeps `storage.sync`, forgets granted permissions, and deletes the managed dir best effort.
- **Live updates.** Subscribes to `ExtensionHost` changes and re-renders on every install, removal or engine sync, including those from sync or other windows.
- (KEEP for all UI. "WebView2" wording → engine-neutral. Unload-before-delete is (WEBVIEW2-ONLY) as a Windows lock issue, but unloading before deleting is still the correct order for CEF; inference.)

### 6.5 Update button (verified)
- Disabled while running. Status "Checking for updates…", then `UpdateReport::summary()` or "Could not check for updates: <e>"; the list re-renders.
- Summary wording (core `update.rs:72`):
  - "Your extensions are up to date"
  - "Updated N extension(s)"
  - "N extension(s) could not be updated"
  - "Updated N extensions; M could not be updated"
  - plus per withheld update: "The newest version of the extension “X” requires more permissions, so it has been disabled. It can now: w1; w2". Parts are joined by ". ".

## 7. Updates (verified, `extensions.rs:245-269`, `:402-425`)
- **Schedule** (`schedule_updates`, interactive runs only):
  - loop `Extensions::next_update_check()`: 60 s after startup when never checked, else when 5 h have passed since the last check, clamped to [60 s, 5 h]
  - sleep, then `update_extensions()`, log the summary
  - nothing is shown; the dialog and toolbar follow through the engine sync
  - the last check is stored in the local meta row `extensions_update_checked_ms`
- **`update_extensions()`:**
  1. One at a time ("an update check is already running").
  2. `prepare_update_check()` records the check time and captures a job per store install the synced record still wants, enabled or not.
  3. `check.run()` on a worker thread: CWS gupdate XML in batches of 20, Edge Omaha 3.1, AMO API; newer = `cmp_versions` > installed; fetched and verified as an install.
  4. `commit_updates()`. If anything updated, `sync_extensions()`.
- **Engine behavior on update.** Core's `commit` gives the new dir `engine_id = None`, so the sync adds it and WebView2 replaces the old version in place (same key → same id → storage kept). The self-test checks storage survives: `2.0:1.0` (§19).
- **Withheld updates.** When the new version's prompted permissions add warnings beyond what was approved plus granted since (`permissions::update_warnings` semantics, core `commit`), the update is installed but `withheld` is non-empty, so `enabled` = false. Core keeps the *old* `engine_id`; the old version stays loaded and is switched off by the second pass. The new folder is not added, because WebView2 starts what it adds. Re-enable clears `engine_id`, and the next sync adds the new dir, which replaces the old one and runs.
- Local and unpacked installs are never checked.
- There is no "reload unpacked" button on Windows (core `reload_unpacked` is unused).
- (KEEP. The "keep old loaded but off" trick is (WEBVIEW2-ONLY) in mechanism; the product rule "new code does not run before Re-enable" is (KEEP) and NEEDS-CEF.)

## 8. Toolbar action buttons (`extension_toolbar.rs`, `popup.rs`, `browser.rs:1711-1760`)

### 8.1 Model (verified)
- **`ExtensionAction`** (`popup.rs:49`):
  - `id`: core id; the toolbar preference keys by it
  - `extension_id`: the engine id, which is the `chrome-extension://` host
  - `title`: `action.default_title` if non-empty, else `manifest.name`
  - `popup`: `action.default_popup`
  - `icon`: the action `default_icon` closest to 32 px, else `manifest.icons` closest to 32, resolved against the dir

  Built only for enabled extensions with a recorded engine id. `manifest.action` covers MV3 `action` and MV2 `browser_action` (the unit test parses `browser_action`).
- **Static manifest values only.** No badge text or color, no `setIcon`, `setTitle`, `setPopup` or `enable`/`disable`, no `openPopup`. WebView2 exposes no action state (verified by grep: no badge or dynamic-action code). (WEBVIEW2-ONLY limitation; dynamic action state is (NEEDS-CEF) for "behaving as in Chrome".)
- **Refresh.** `refresh_extension_actions()` runs after each engine sync. It rebuilds the list from core and, when it changed, calls `show_extension_actions()` for every window.
- **Layout.** `Browser::extension_toolbar()` = `toolbar::layout(available_ids, saved)` with `saved = prefs.get(&toolbar::TOOLBAR)`, the synced preference `toolbar.extensions` holding `Vec<Entry { id, pinned }>`.
  - Ids with no entry are pinned after the others, so a new install shows up in the toolbar.
  - Entries of extensions this device lacks are kept (they sync as one value).
- **Writes.** `set_extension_pinned(id, pinned)` → `toolbar::set_pinned`. `move_extension(id, to)` → `toolbar::move_pinned`. Each writes the preference with `write_pref`, then calls `show_extension_actions()`.
- **Sync.** A synced change to `toolbar.extensions` (`PrefEffect::ExtensionToolbar`, `sync.rs:787`) calls `show_extension_actions()`.

### 8.2 What the user sees (verified, `window/chrome.rs:218-233`)
- **Position.** Toolbar order: Back, Forward, Reload/Home, address pill (zoom chip, PiP, Copy link, Star inside), **ExtensionActions**, Downloads, Private pill, Profile, More (main menu). The extension area is a horizontal StackPanel (spacing 2) holding:
  - `ListView PinnedExtensions`: horizontal `ItemsStackPanel`, `SelectionMode=None`, `IsItemClickEnabled`, `CanDragItems`, `CanReorderItems`, `AllowDrop`, scrolling disabled, automation name "Pinned extensions"
  - `Button ExtensionsMenu`, the puzzle piece: glyph EA86, size 16, tooltip and automation name "Extensions", collapsed until any extension has an action (`set`, `extension_toolbar.rs:103`; shown even when all are pinned)
- **Each pinned button:**
  - `ListViewItem` 36×32, no padding, content centered
  - tooltip and automation name = title (the action shortcut is *not* in the tooltip)
  - a 16 px image (`file:///` URL of the icon, escaped) or glyph EA86
  - the whole list is rebuilt on every `set`
- **Click** → `Command::Open { id, anchor: that button }` → popup (§9).
- **Right-click** → `MenuFlyout` built on `Opening` for the target button. "Unpin" (glyph E77A) → `Pin(id, false)`. "Manage extensions" (glyph E713) → opens the dialog. **No extension-defined `contextMenus` items with `contexts: ["action"]`**: WebView2 offers no way to read them (PLAN.md "Windows" paragraph).
- **Drag to reorder** within the pinned list (XAML's own drag). On `DragItemsCompleted`:
  1. Compare the before and after orders with `bookmarks_bar::moved`, which finds the one element that moved.
  2. `Command::Move(id, new index among pinned)`.
  3. Core `move_pinned` keeps unpinned and absent entries in their slots.
- **Puzzle menu** (`show_menu`, `:246`): `Flyout` placed `BottomEdgeAlignedRight`, a 300 px StackPanel:
  1. heading "Extensions"
  2. one row per action, in install order, pinned or not:
     - an "Open" button (left aligned, transparent; icon + title trimmed at MaxWidth 200; automation name = title). Click hides the flyout and opens the popup **anchored on the puzzle button**.
     - a pin toggle 36×32 with glyph E842 (pinned) or E718 (not pinned) and tooltip "Unpin from the toolbar" / "Pin to the toolbar". Click flips the glyph in place (the flyout stays open) and writes `Pin(id, bool)`.
  3. a divider
  4. "Manage extensions" (gear E713): hides the flyout and opens the dialog
- **Popup anchor.** `anchor_of(engine_id)` is the pinned button when pinned, else the puzzle button. Used by keyboard-opened popups too.
- **Title bar.** With tabs on top, the toolbar is part of the title bar, and the extension area is excluded from the window drag region like the other controls (`window/tab_layout.rs:161`).
- **Private windows** show no extension buttons and no puzzle button: `set_extension_actions` passes `&[]` (`window/mod.rs:1791`).
- (KEEP for all UI.)

## 9. Action popups (`popup.rs`, `js/popup.js`, `window/mod.rs:1799-1877`)

### 9.1 Opening (verified)
- **Entry points:**
  - pinned button click
  - puzzle row click
  - action keyboard shortcut (§11)
  - scripted `open_extension_popup(engine_id, Activation::Keep)`

  All go through `BrowserWindow::show_popup` (`window/mod.rs:1841`).
- **Private windows refuse:** error "extensions do not run in private windows". Their extensions are the normal profile's, and the popup would learn the window's tabs.
- **Opener tabs.** `OpenerTab { url: tab.session_url(), active }` for every tab of this window in order. Passed to `popup::open(anchor, environment, action, activation, opener)`.
- **No popup.** If the action has no `default_popup`, `open` logs "extension X has no popup" and shows nothing. **`action.onClicked` is never fired**: WebView2 cannot (verified by code). PLAN says Linux clicks the action instead. (KEEP the Chrome behavior: clicking an action without a popup dispatches `action.onClicked`; NEEDS-CEF.)
- **Flyout** (`FLYOUT_XAML`, `popup.rs:111`):
  - `Placement="BottomEdgeAlignedRight"`, `ShouldConstrainToRootBounds="True"`
  - presenter padding 0, MaxWidth 820, MaxHeight 640
  - content: a `Grid` holding a new `WebView2` element, initially 360×240
- **Activation:**
  - `Activation::Focus` → `FlyoutShowMode::Standard`: takes focus like any flyout. Used for clicks, and for shortcuts when the window is foreground (`window/mod.rs:1254`).
  - `Activation::Keep` → `FlyoutShowMode::Transient`: shows without taking focus. Used for scripted runs and shortcuts while the window is not foreground.
- **Web view setup** (`load`, `:197`), run asynchronously after showing:
  1. `EnsureCoreWebView2WithEnvironmentAsync(shared environment)`: the normal profile, so the page has the full `chrome.*` of an extension page.
  2. `WindowCloseRequested` → hide the flyout, so `window.close()` in the popup closes it.
  3. `WebMessageReceived` → `parse_size` → set the element width and height.
  4. `AddScriptToExecuteOnDocumentCreatedAsync(popup.js(opener))` (main world, every document).
  5. `Navigate("chrome-extension://<engine id>/<popup path without leading '/'>")`.

  On any error: log "popup <url>: <e>" and hide.
- **Close.** Flyout `Closed` → `WebView2::Close()`: the popup document is destroyed on dismiss, as in Chrome. Light dismiss: clicking elsewhere closes it (XAML Flyout default; Esc closing it is also an XAML default, inference).
- **Bookkeeping.** `BrowserWindow.popup` keeps the last popup. `extension_popup()` returns it while open, with `title()` = `DocumentTitle` and `url()` = `Source`.
- **No handlers on the popup's web view** for `NewWindowRequested`, `ContextMenuRequested`, downloads or permissions (verified by absence). WebView2's defaults apply there: links with `target=_blank`, `window.open` and the default context menu with Inspect. What `chrome.tabs.create` from a popup does in WebView2 is not verified. (NEEDS-CEF: links from popups should open as tabs in the opener window, as in Chrome; inference about Chrome's behavior.)

### 9.2 Size protocol (verified; `popup.js:12-71`, `popup.rs:239-251`)
- The top document only (`window === window.top` and `chrome.webview.postMessage` exists).
- `measure()`:
  1. Temporarily set `documentElement.style.width = "max-content"` and measure the width.
  2. Restore, then measure the height.
  3. `ceil`, then clamp to [25, 800] × [25, 600] (Chrome's popup limits).
- It reports `{"popupSize":[w,h]}` on change, coalesced to one per animation frame, through a `ResizeObserver` on `<html>` and `<body>`, a `MutationObserver` on the subtree, attribute and text changes (records produced by measuring are discarded), and `resize`, `DOMContentLoaded` and `load`.
- The shell clamps again (non-finite values rejected) and resizes the WebView2 element.
- (KEEP: popups size to content within 25..800 × 25..600. Mechanism (WEBVIEW2-ONLY). NEEDS-CEF: inference, the Chrome runtime's own `ExtensionPopup` view does this natively, but a GPUI-hosted popup browser would need `CefDisplayHandler::OnAutoResize` with `CefBrowserHost::SetAutoResizeEnabled(true, min, max)`, which exists in CEF's C API to my knowledge, or this same JS protocol.)

### 9.3 `chrome.tabs` shim in popups (verified; `popup.js:73-109`)
- **Why.** In WebView2 every web view, the popup included, is its own extension window with one tab. In a popup, `tabs.query({active: true, currentWindow: true})` returns the popup page and `tabs.getCurrent()` returns a tab; Chrome returns none. uBlock Origin Lite then says "not a website", and Proton Pass sizes itself to 100% of a view that has no size yet (`docs/design/research-winui.md:64`).
- **`tabs.query(info, cb)`.** When `info` asks about the current window (`currentWindow: true`, `lastFocusedWindow: true` or `windowId: -2`) and opener tabs exist:
  1. Query natively without the placement keys (`active`, `highlighted`, `index`, `windowId`, `currentWindow`, `lastFocusedWindow`).
  2. Sort the results by `lastAccessed`, newest first.
  3. Match each opener tab by `url` or `pendingUrl`, in opener order.
  4. Rewrite `active` and `highlighted` from the opener and set `index` = the opener position.
  5. Filter by the requested `active` / `highlighted` / `index`.

  Other queries pass through unchanged.
- **`tabs.getCurrent`** resolves `undefined`.
- **Ordering.** All tab answers wait for the first fitted size, or 1 s, because Chrome's popup has its size before scripts get tab answers.
- **What it does not fix.** Tab ids in the answers are WebView2's own, and they do not map to Vsesvit tabs (WebView2Feedback #3853, #3854). So `tabs.sendMessage`, `scripting.executeScript` and similar work only if WebView2's id for the matched tab is real; inference: they are, for the WebView2 web view showing that URL.
- (WEBVIEW2-ONLY shim. The requirement "in a popup, the current window is the browser window it opened from, and its active tab is the selected tab" is (KEEP) and NEEDS-CEF. CEF documents `CefBrowser::GetIdentifier()` as "also used as the tabId for extension APIs" (verified in the bindings), which suggests real tab ids, but window membership in a GPUI shell is unverified; see §18.)

## 10. Context menus (verified)
- **Page context menu.** WebView2 itself puts extensions' `contextMenus` items in the page's context menu. The item `Name()` is `"extension"`, and the label is the extension's title. The shell's `ContextMenuRequested` handler (`tab.rs:1336`) only adds its own items (selection search after `copy`, "Copy link without tracking" after `copyLinkLocation`, "Open link in private window" after `openLinkInNewWindow`, "View page source" before `inspectElement`) and never removes the extension items. Choosing an extension item fires the extension's `contextMenus.onClicked` with `pageUrl` (self-test `context_menus`).
- **Grouping.** How WebView2 groups several items under the extension name is not handled by the shell (WebView2's own behavior).
- **Toolbar button menu.** It has no extension items (WebView2 offers no way to read them). It shows only Unpin and Manage extensions.
- **PLAN target (Linux).** Items in the page menu by what was clicked and in which frame, grouped under the extension's name when there are several, and `action`-context items in the toolbar button's menu.
- (KEEP: extension items in the page menu, and action items in the button menu as in Chrome. NEEDS-CEF: in the Chrome runtime, extension items are part of Chrome's `RenderViewContextMenu` (command ids from `IDC_EXTENSIONS_CONTEXT_CUSTOM_FIRST` = 49000, verified constant). A GPUI-drawn menu would receive them through `CefContextMenuHandler::OnBeforeContextMenu` / `RunContextMenu` and dispatch through `OnContextMenuCommand`; whether CEF passes extension items through in each runtime style is not verified. Action-context items have no CEF API that I know of; inference.)

## 11. Extension keyboard shortcuts (`shortcuts.rs`, `window/wiring.rs`, `dialogs/shortcut_settings.rs`)

### 11.1 Resolution (verified)
- **Inputs.** `Browser::shortcuts_changed()` (`browser.rs:1555`) builds `Bindings::new(keymap, extension_commands(&Extensions::list()), &extension_actions())`.
  - `keymap = prefs().keymap()`, the synced `keyboard.shortcuts` (`keys::SHORTCUTS`).
  - `extension_commands` takes the enabled extensions' manifest `commands`, in install order, each extension's in name order.
- **Triggers:** every engine sync, `edit_keymap`, and a synced change to `keyboard.shortcuts` (`PrefEffect::Keymap`).
- **Core rules** (`Keymap::extension_shortcuts`, `crates/vsesvit-core/src/extensions/commands.rs:121`):
  1. The browser's own chords are never an extension's.
  2. Then stored user overrides (`extension:<id>:<command>`) claim their chords in order.
  3. Then the suggested keys claim theirs in install order, so the first installed wins.
  4. One chord per command.
  5. Manifest parsing allows Ctrl or Alt but not both and at most four suggested keys per extension (PLAN).
- **What the shell binds.** Only **action commands** (`ManifestCommand::activates_action()`: `_execute_action`, `_execute_browser_action`, `_execute_page_action`), and only when the extension has a toolbar action, meaning it is loaded and enabled. Each becomes `Command::ExtensionAction(index)` with in-page kind **`Reserved`** (taken before the page, as Chrome handles extension shortcuts), mapped to the engine id (`extension_action(index)`).
- **Named commands are not bound.** WebView2 has no way to fire `commands.onCommand`, so their chords stay the page's (`shortcuts.rs:22-24`). (WEBVIEW2-ONLY gap. PLAN's Linux behavior, a named command fires `commands.onCommand` with the selected tab and grants `activeTab`, `"global": true` included, is the Chrome behavior to (KEEP); NEEDS-CEF.)

### 11.2 Running an action command (verified, `window/mod.rs:1249-1262`)
- `Command::ExtensionAction(i)` looks up the engine id, picks Activation (Focus if the window is foreground, else Keep), and calls `open_extension_popup(engine_id, activation)`, which anchors on the pinned button or the puzzle button.
- An action without a popup does nothing.
- (KEEP.)

### 11.3 How keys are caught (verified)
- **XAML focus.** One `KeyboardAccelerator` per binding on the window root (`set_accelerators`, `window/wiring.rs:237`). **Private windows skip `ExtensionAction` accelerators.** Accelerators are suspended while Settings captures a shortcut.
- **Page focus.** Keys go to WebView2's own window, and XAML never sees them, so each tab injects a page script into an **isolated world** named `vsesvit-<random secret>`. The secret matters because WebView2 exposes the binding to every world of that name, and extensions' content scripts run in worlds named after the extension.
  - Setup uses DevTools calls `Page.enable`, `Runtime.enable`, `Runtime.addBinding {name: "vsesvitShortcut", executionContextName: world}`, `Page.addScriptToEvaluateOnNewDocument {source, worldName}`.
  - Cross-site frames get the same world through `Target.setAutoAttach {autoAttach, waitForDebuggerOnStart, flatten, filter: iframe}`, then `Runtime.runIfWaitingForDebugger`.
  - A capture-phase `keydown` listener with `reserved` chords (`keyCode:mods`) calls `preventDefault` + `stopImmediatePropagation` and reports `{t: "key", vk, m}`.
  - The key sets are pushed per document by `keys_script()` (`globalThis.vsesvitKeys = {reserved, overridable, swallowed}`). It is replaced on change through `Page.removeScriptToEvaluateOnNewDocument` + `Runtime.evaluate` in each live context of the world (`tab.rs:575`).
- **Private tabs swallow the chord.** The key sets are global, not per window, so in a private tab an extension action chord is still taken from the page and reported. `show_popup` then refuses, so the chord does nothing in private windows. Verified from code.
- (KEEP: action shortcuts work while the page has focus, before the page sees them. Mechanism (WEBVIEW2-ONLY). NEEDS-CEF: `CefKeyboardHandler::OnPreKeyEvent` sees keys before the renderer, with `is_keyboard_shortcut`, so no injected world is needed for keys. The store bridge, which rides on the same world, needs its own CEF channel; see §5.)

### 11.4 Settings > Keyboard shortcuts, "Extension shortcuts" (verified, `dialogs/shortcut_settings.rs`)
- **Placement.** After the browser's sections, a group titled "Extension shortcuts". It is shown only when some enabled extension has an **action** command; named commands are not listed on Windows (`fill`, `:376-399`).
- **Row.** A card button with:
  - title `"<extension name>: <command title>"`; the title is the description, else "Activate the extension" for action commands, else the name
  - keycaps for its chord, or "Disabled" when none
  - automation name `"<title>: <chord text>"`
  - a reset button (glyph E7A7, tooltip "Reset to the default", automation name "Reset <title> to the default"), visible only when overridden (`ExtensionShortcuts::is_default`)
  - x:Names `ExtensionShortcut_<id with non-alphanumerics as _>_<command>` and `ResetExtensionShortcut_…`
- **Capture flyout** (click a row):
  - title "Press the new shortcut for <title>", the keycaps of the pressed chord or "…", a note line, the hint "Esc cancels. Backspace removes the shortcut.", buttons Save (disabled until a chord) and Cancel
  - window accelerators are suspended while it is open
  - the flyout shows non-transient only if the host window is foreground
- **Capture rules for extension commands:**
  - Esc cancels. Backspace removes (assign `None`). Enter or Save saves. A modifier alone waits.
  - A chord without Ctrl or Alt → "Extension shortcuts need Ctrl or Alt." and no chord. Function keys alone are not allowed for extensions; browser commands allow them.
  - A non-key → `NOT_A_KEY_NOTE` "This key cannot be part of a shortcut".
  - `Keymap::offer_extension`: a browser chord → "Used by <browser command>, which an extension cannot take" and no chord. Another extension's chord → "Also used by <ext name>: <title>. Saving moves it here."
  - `assign_extension` takes the chord from the other extension command, which keeps its other stored chords as an override. `reset_extension` restores the suggested key if free.
- **Browser command capture** notes an extension holder: `ExtensionShortcuts::note_for_browser` → "Also used by Probe: Activate the extension. Saving moves it here." Saving takes it, because browser chords win.
- **Reset all** (flyout "Reset all shortcuts?" / "Every command gets its default shortcuts back." / "Reset") calls `Keymap::reset_all`, which also drops extension overrides.
- After every edit: `Browser::edit_keymap` stores the keymap through `prefs().set_keymap`, calls `shortcuts_changed()` for all windows, and the page refills while keeping its scroll offset.
- (KEEP; the new shell should also list named commands once it can run them, as the Linux PLAN says.)

## 12. Sync interplay (verified)
- **`Browser::sync_applied(changed, …)`** (`browser.rs:1576`):
  - `changed.extensions` → `reconcile_extensions()`
  - preference effects (`sync.rs:761-800`): `toolbar.extensions` → `show_extension_actions()`; `keyboard.shortcuts` → `shortcuts_changed()`
- **`storage.sync` is not bridged on Windows.** It belongs to WebView2 (`browser.rs:1573-1575`, PLAN "Open questions"). Core's `ExtStorageSync` kind (`crates/vsesvit-core/src/sync.rs:65`) and `changed.ext_storage` are unused by WinUI. (NEEDS-CEF: either bridge Chromium's `chrome.storage.sync` into core's records or accept that it stays local; inference, CEF has no Chrome Sync, so `storage.sync` behaves as local storage there.)
- **Sync data type.** "Extensions" covers `Kind::Extensions` and `Kind::ExtStorageSync` (core `sync.rs:107-121`).

## 13. Private windows (verified)
- **No extension UI and no extension code paths:**
  - no toolbar actions or puzzle button (`set_extension_actions`)
  - `show_popup` refuses
  - no extension accelerators; page-script chords are swallowed and do nothing (§11.3)
  - extensions are not loaded in the InPrivate profile, so no content scripts run there (self-test `private_window` checks `dataset.vsesvitProbe` stays unset)
- **Still available there:** the main-menu "Extensions" item, which acts on the normal profile's extensions, and the store bridge.
- **Unused core API.** `Extensions::allowed_in_private` / `set_allowed_in_private` (`extensions.allowed_in_private`, synced, Chrome's "Allow in Incognito") is never called by WinUI; GTK uses it.
- (Product rule (KEEP): off in private windows by default, with Chrome's per-extension "Allow in private" switch. WinUI's "never" is (WEBVIEW2-ONLY). NEEDS-CEF: inference, the Chrome runtime has incognito profiles and per-extension incognito enablement (`ExtensionPrefs` "incognito" flag) internally, with no CEF API, so it would have to be reached through preferences or the extensions page.)

## 14. Other touchpoints (verified)
- **Omnibox.** `chrome-extension:` (with `blob:` and `mailto:`) typed in the address box is navigated as-is (`omnibox::ENGINE_SCHEMES`, used at `browser.rs:1327`). (KEEP.)
- **Memory saver.** Core's `memory_saver::Sweep` keeps non-http(s) pages awake, `chrome-extension://` included: "an extension's page … costs little and may not come back as it was" (`crates/vsesvit-core/src/memory_saver.rs:153-156`). (KEEP.)
- **Settings > Privacy text:** "Vsesvit doesn't save passwords. Use a password manager such as Bitwarden or Proton Pass through its browser extension." (`dialogs/settings.rs:247`). (KEEP.)
- **Manage profiles text.** It mentions that extensions are per profile (`profiles.rs:196`, `:218`).
- **Profile layout** (core): `extensions/<id>/<version>_<hash32>/` (immutable managed dirs), `staging/` (in-flight installs, wiped at open), `engine/` (the engine's user data, where WebView2 keeps its extension registry).

## 15. Persistent state

| Name | Scope | Written by | Read by |
|---|---|---|---|
| `extensions` table (`ExtensionRecord`: store, installed, enabled LWW) | synced | `commit` (user installs), `set_enabled`, `uninstall` | `reconcile`, `list` (enabled for store installs), `prepare_update_check` |
| `extension_installs` table (version, dir, source, verification, manifest JSON, `local_enabled`, `engine_id`, `granted`, `installed_ms`) | local | `commit`, `set_enabled` (local installs), `set_engine_id`, `approve_permissions`, `uninstall`, `reconcile` | `list`, `get` |
| `toolbar.extensions` (`Vec<{id, pinned}>`, `vsesvit_core::extensions::toolbar::TOOLBAR`) | synced pref | toolbar pin, unpin and drag | toolbar layout in every window |
| `keyboard.shortcuts` (`keys::SHORTCUTS`; extension overrides under `extension:<id>:<command>`) | synced pref | Settings > Keyboard shortcuts | `Bindings` |
| `extensions.granted_permissions` (`GRANTED_PERMISSIONS`) | local pref | core (`approve_permissions` grants what an update added) | core (`commit` of updates: what was approved) |
| `extensions_update_checked_ms` | local meta row | `prepare_update_check` | `next_update_check` |
| `onboarding.done` | local pref | welcome | welcome |
| `extensions.allowed_in_private`, `extensions.notifications_off` | synced prefs | not by WinUI | not by WinUI (GTK only) |
| `ext_storage_local` rows | local | not by WinUI (WebView2 keeps `chrome.storage` itself) | deleted by core `uninstall` |
| `<profile>/engine/` | files | WebView2 (its extension registry, `chrome.storage`, service workers) | WebView2 |
| `<profile>/extensions/<id>/<ver>_<hash>/manifest.json` | files | core (`set_key`); WinUI adds a synthetic `key` once (§3) | the engine |

## 16. Core contract the WinUI shell calls

Module `vsesvit_core::extensions` unless noted. All calls are made on the UI thread through `Browser::core(|p| …)`, except the two `run` calls.

| API | Decides / does | WinUI caller |
|---|---|---|
| `InstallSource::parse(&str)` / `from_path(&Path)` / `web_store(store, id)` | which source an input is; the error wording | dialog install box, pickers, `--load-extension`, store bridge (`Store::source`), welcome (`Recommended::install_source`) |
| `Extensions::prepare_install(source) -> InstallJob` | allocates staging; refuses non-Unicode paths | `stage_extension` |
| `InstallJob::run(&mut progress) -> StagedInstall` (worker thread) | download, verify (CRX3 proofs, publisher key, AMO sha256), unpack safely, set `key`, localize manifest, check the expected id | `run_job` |
| `InstallPhase::describe()` / `fraction()` | progress wording and fraction | dialog, welcome |
| `StagedInstall::needs_approval()` / `manifest()` / `id()` | whether Chrome would prompt (user intent, not unpacked) | dialog, store bridge |
| `permissions::install_heading`, `install_warnings`, `INSTALL_LEAD`, `re_enable_heading`, `RE_ENABLE_LEAD`, `PermissionMessage { text, details }` | Chrome's prompt wording and coalescing | dialog (`prompt_text`, `re_enable_text`), store bridge |
| `Extensions::commit(staged) -> Option<InstalledExtension>` | publishes; desired-state update for user store installs; id-hold checks; downgrade rules; `withheld` for updates; clears `engine_id` when the dir changes (except withheld) | `commit_package`, `run_install_job` |
| `Extensions::list()` / `get(id)` | installed set in install order; `enabled` = user choice ∧ ¬withheld | everywhere |
| `Extensions::set_enabled(id, bool)` | synced register (store) or local column | dialog switch |
| `Extensions::uninstall(id)` | `installed := false` (store), delete row and `storage.local`, keep `storage.sync`, forget grants, delete dir best effort | dialog Remove, store uninstall |
| `Extensions::reconcile() -> Reconcile { install, removed }` | what other devices asked for | `reconcile_once` |
| `Extensions::set_engine_id(id, engine_id)` | remembers the engine's id for the current dir | `sync_once` |
| `Extensions::approve_permissions(id)` | Chrome's Re-enable: grant what was added, clear `granted`, clear `engine_id` if withheld | dialog Re-enable |
| `Extensions::next_update_check()`, `prepare_update_check()`, `UpdateCheck::run()` (worker thread), `UpdateCheck::is_empty()`, `commit_updates(updates) -> UpdateReport`, `UpdateReport::summary()`, `UPDATE_INTERVAL` | update schedule, store protocols, newer-version rule, report wording | `schedule_updates`, `update_extensions`, dialog |
| `InstalledExtension { id, version, dir, manifest, enabled, withheld, source, verification, engine_id }`, `Verification::label()` | row content | dialog, toolbar, store |
| `manifest::Manifest { name, action: Option<Action { default_popup, default_title, default_icon }>, icons, key, commands, … }`, `RelPath::resolve(dir)` | toolbar title, popup and icon | `ExtensionAction::from_installed`, dialog icon |
| `toolbar::{TOOLBAR, Entry, Layout, layout, set_pinned, move_pinned}` | pinned order and new-install default (pinned) | `Browser::extension_toolbar` / `change_toolbar` |
| `commands::{extension_commands, ExtensionCommand, ExtensionShortcuts}` + `shortcuts::Keymap::{extension_shortcuts, offer_extension, assign_extension, reset_extension, reset_all}`, `manifest::{ManifestCommand::activates_action, ACTION_COMMANDS}` | extension chord resolution and capture rules | `Bindings::new`, Settings |
| `vsesvit_core::onboarding::RECOMMENDED_EXTENSIONS`, `Recommended::{id, install_source}` | welcome picks | welcome |
| `vsesvit_core::OpenOptions::chrome_version` | CWS `prodversion` | `lib.rs:open_profile` |
| `vsesvit_core::sync::Changed::extensions`, preference keys | when to reconcile or repaint | `sync_applied` |

`vsesvit_sync` and `vsesvit_update` are not called by any extension code (verified by grep). `vsesvit_sync::status` only words "extensions" in its sync texts.

### 16.1 Core APIs WinUI does not call (GTK does; needed for full Chrome parity)

| API | Purpose |
|---|---|
| `Extensions::allowed_in_private` / `set_allowed_in_private` (`extensions::private::ALLOWED_IN_PRIVATE`) | per-extension "Allow in private windows" |
| `Extensions::notifications_allowed` / `set_notifications_allowed` (`extensions::notifications::NOTIFICATIONS_OFF`) | per-extension notification switch |
| `permissions::request_heading`, `REQUEST_LEAD`, `permissions::request`, `Extensions::active_permissions` / `grant_permissions` / `remove_permissions` | `chrome.permissions.request` prompt and grant bookkeeping |
| `Extensions::reload_unpacked` | developer reload |

## 17. What WebView2 could not do, and what the shell did instead

| # | WebView2 limitation (source) | WinUI workaround or consequence |
|---|---|---|
| 1 | No browser UI entry points for extensions (research-winui.md:66) | Shell-drawn action buttons, puzzle menu, popups in flyouts |
| 2 | Every web view is its own extension window with one tab; `chrome.tabs` does not know shell tabs (WebView2Feedback #3853, #3854) | `popup.js` rewrites current-window `tabs.query`, `getCurrent()` → undefined; tab ids remain WebView2's |
| 3 | No action state API (badge, dynamic icon, title, popup, enable) | Toolbar shows manifest defaults only; no badge |
| 4 | Cannot fire `action.onClicked` | Clicking an action without a popup does nothing |
| 5 | Cannot read `contextMenus` items for the action | Button menu has only Unpin and Manage extensions |
| 6 | Does not run `commands` and cannot fire `commands.onCommand` | Only action commands bound, by the shell; named commands unlisted and unbound |
| 7 | No install prompt; `beginInstallWithManifest3` never answers on CWS; Edge Add-ons has no `webstorePrivate` | `store.js` shim + isolated-world bridge + core install + `ContentDialog` prompt |
| 8 | No per-extension InPrivate switch; adding to the InPrivate profile may reinstall into the normal one | No extensions in private windows at all |
| 9 | Unpacked folders only; drops an extension if its files change; `_`-prefixed names fail with E_ACCESSDENIED; `MAX_PATH`; deep folders fail | Immutable content-addressed dirs; pipeline rejects `_` names (except `_locales`, `_platform_specific`) and drops `_metadata`; `\\?\` paths; deep-folder hint |
| 10 | Id from `key` or folder path | Core writes the verified CRX key; shell writes a synthetic key for keyless managed installs |
| 11 | Starts every extension it adds | Disabled ones switched off right after add; withheld updates not added until Re-enable |
| 12 | Installs its own built-ins | Left alone by id |
| 13 | Profile reachable only through a web view | No engine sync while only private windows are open |
| 14 | Calls may never answer (closed view) | Timeouts 20 s / 90 s |
| 15 | `chrome.storage.sync` owned by WebView2, no hook | Not synced on Windows |
| 16 | `chrome.notifications` support unknown | Self-test records, does not judge; no shell notification UI, no per-extension switch |
| 17 | `permissions.request` prompt: the shell shows none | Unknown whether WebView2 shows one (not verified) |
| 18 | Keys in the page never reach XAML | DevTools isolated-world script with a binding |

## 18. CEF outlook: better, worse, unknown

Verified in `cef-dll-sys` 154.5.0+154.0.34 (`src/bindings/x86_64_unknown_linux_gnu.rs`) and `cef` 154.5.0:

- **No extension management API.** There is no `_cef_extension_t`, `CefExtensionHandler` or `load_extension`, and `_cef_request_context_t` has none of `LoadExtension` / `GetExtensions` / `HasExtension` (its members: cache path, cookie manager, scheme handler factories, website and content settings, Chrome color scheme, media router, resolve host, clear caches, setting observers). The `cef` crate has no extension wrapper either. **There is no direct counterpart to WebView2's `AddBrowserExtensionAsync` / `GetBrowserExtensionsAsync` / `EnableAsync` / `RemoveAsync`**, so `sync_engine` has no direct CEF port. Extensions would have to come from Chrome's own system inside the Chrome runtime. Candidate mechanisms, all inference to verify:
  - the Chromium `--load-extension` switch, which Google Chrome 137+ disables in branded builds; whether CEF keeps it is unverified
  - Chrome's `chrome://extensions` / Web Store install inside CEF
  - preferences through `CefPreferenceManager::SetPreference` (verified to exist: `has_preference`, `get_preference`, `get_all_preferences`, `can_set_preference`, `set_preference`, `add_preference_observer`)
- **`CefBrowser::GetIdentifier()`** is documented as "also used as the tabId for extension APIs". That is better than WebView2 #3853/#3854, at least for tab ids.
- **Runtime styles** (`cef_runtime_style_t`): Chrome style provides "the full Chrome UI and browser functionality"; Alloy style provides less but adds client callbacks and off-screen rendering. "Windowless rendering will always use Alloy style". "A Chrome style Window can host at most one Chrome style BrowserView". The Chrome toolbar (`cef_chrome_toolbar_type_t` NORMAL / LOCATION via `CefBrowserViewDelegate::GetChromeToolbarType`) and `CefCommandHandler::OnChromeCommand` are Chrome style only. Command ids exist for `IDC_MANAGE_EXTENSIONS` = 40022, `IDC_EXTENSIONS_SUBMENU` = 57334, `IDC_EXTENSIONS_SUBMENU_MANAGE_EXTENSIONS` = 40268, `IDC_EXTENSIONS_SUBMENU_VISIT_CHROME_WEB_STORE` = 40269, `IDC_EXTENSIONS_CONTEXT_CUSTOM_FIRST` = 49000.
- **CEF `cef_window_info_t.shared_texture_enabled`** is documented as "Currently only supported on Windows (D3D11)".

Inference (verify in the CEF research task):

- **Likely better:**
  - a real Chrome extension runtime: MV3 service workers, DNR, `webRequest`, `commands` / `commands.onCommand`, `action.onClicked`, badge and dynamic action state, `permissions.request` prompts, notifications through Chrome's Linux notification bridge, incognito enablement, `contextMenus` with action contexts
  - an extension popup host and install prompt, *when Chrome UI is present*
- **Likely worse or harder in a GPUI shell:**
  - If the shell draws all chrome in GPUI and hosts pages off-screen (Alloy style forced), Chrome's own extension UI (toolbar, popups, prompts, menus) is absent, as in WebView2. Whether Alloy-style browsers in the Chrome runtime are registered as tabs in a Chrome `Browser` / `TabStripModel` (which `chrome.tabs` and `chrome.windows` enumerate) is unknown. If not, the WebView2 problems of §9.3 may recur.
  - With Chrome-style views, each Chrome-style view is likely its own Chrome window with one tab, a similar windows/tabs mismatch.
  - Reading action state (icon, badge, popup URL) to draw it in GPUI has no CEF API.
  - Opening a popup as a separate browser at `chrome-extension://<id>/<popup>` reproduces the WinUI approach, including the need for a tabs shim and a size protocol (`CefBrowserHost::SetAutoResizeEnabled` + `CefDisplayHandler::OnAutoResize` would replace the JS size messages).
- **Unknown:**
  - whether the Chrome Web Store's `webstorePrivate` install flow completes in CEF and which UI it shows
  - how to list, enable, disable or remove extensions programmatically from the host
  - whether Chrome's extension prefs can be driven to match core's desired state

## 19. Self-test and ui-smoke coverage (verified)

Fixture: `tests/fixtures/extensions/probe/`. MV3 "Vsesvit Probe" 1.0.0 with:
- permissions storage, declarativeNetRequest, contextMenus, notifications, scripting, webNavigation, cookies; `<all_urls>`
- a service worker, a content script at document_end
- `action { default_title: "Vsesvit Probe", default_popup: "popup.html" }`; the popup sets `document.title = "visits=N"`
- DNR ruleset `block`
- commands `_execute_action` Alt+Shift+P and `probe-command` Alt+Shift+K ("Vsesvit Probe command")
- `contextMenus` `probe-page` ("Vsesvit Probe page item", page) and `probe-action` (action)

`testkit::probe_crx()` / `testkit::PROBE_ID`. Other fixtures there (commands, cookies, dnr, menus, navigation, notifications, permissions, ports, ports-friend, scripts, update-probe, windows) serve Linux checks.

Self-test checks in `report.json` (`report.rs:5`; meanings in `docs/design/self-test.md`):

| Check | Windows expectation |
|---|---|
| `install_crx` | install `probe.crx` via `install_package`; id == `PROBE_ID`, verification `LocalCrx` |
| `engine_loaded_extension` | after `sync_extensions`, the recorded engine id == `PROBE_ID` (proves the injected key) and WebView2 lists it enabled; no engine error |
| `content_script`, `extension_port` | `dataset.vsesvitProbe == "background-replied"`, `vsesvitProbePort == "pong:probe"` |
| `dnr_blocked`, `dnr_site_allowed` | static block; dynamic `allowAllRequests` rule (as uBlock Origin Lite does per site) lifts it, clearing restores it |
| `popup` | `open_extension_popup(PROBE_ID, Keep)` → title `visits=N`, N ≥ 1 |
| `context_menus` | right-click via DevTools `Input.dispatchMouseEvent` → `ContextMenuRequested` has an item named `extension` labelled "Vsesvit Probe page item"; choosing it fires `onClicked` with the page URL |
| `extension_commands` | Alt+Shift+P is `Reserved` and opens the probe's popup from the page; Alt+Shift+K is unbound; Settings lists the action row; whether WebView2 fired `onCommand` within 5 s is recorded, not judged |
| `extension_notifications` | records `typeof chrome.notifications` and the create result, not judged |
| `extension_update` | `FixtureStore` stands in for CWS (`set_stores`); 1.0 → `1.0:1.0`; 2.0 updates in place and enabled, page `2.0:1.0` (storage kept); 3.0 adds `tabs` → withheld "Read your browsing history", engine lists it disabled; Re-enable → enabled, page `3.0:1.0`; uninstall removes it from core and engine |
| `private_window` | InPrivate profile; the probe's content script does not run; the action shortcut and `open_extension_popup` open no popup there |
| `shortcuts` / `shortcuts_sync` | the keymap including extension override semantics |
| `cws_install` (`--network`) | real CWS install of `ddkjiahejlhfcafbddmgiahcphecmpfh` (uBlock Origin Lite), verification `ChromeWebStore`, engine id == id |

ui-smoke steps (`automation/`):
- `00-install-probe`
- `15-extensions-dialog`: switch off → engine disabled and no toolbar action; Update button labelled "Update"
- `15b-extensions-toggle-remove-install`: on again → action back; Remove → gone from core, engine and toolbar; reinstall through the install box
- `15c-extensions-install-prompt`: heading `Add “Vsesvit Probe”?`, the text above
- `18-extension-popup`
- `18b`…`18h` toolbar (`automation/toolbar_steps.rs`):
  - a second CRX → both pinned in install order
  - puzzle menu pins both checked
  - drag reorders and is saved in `toolbar.extensions`
  - the button menu is exactly ["Unpin", "Manage extensions"]
  - Unpin saves `pinned: false`
  - the unpinned popup opens under the menu button
  - Pin1 in the menu pins it back in its old place
- `22-xpi-update-keeps-its-engine-id`: XPI 1.0.0 → 1.0.1 keeps the engine id and storage (visits 1 → 2), one engine entry
- `25e-welcome-extensions`: one button per recommended extension

## 20. Checklist

- [ ] Core is the source of truth; the engine's extension set is reconciled to `Extensions::list()` after every change, idempotently, one pass at a time, with per-extension errors kept for the UI
- [ ] Record the engine's id per installed dir (`set_engine_id`); clear on new dir; `engine_id` gates the toolbar button
- [ ] Keep extension ids and storage stable across versions: CRX `key` from core; synthetic `key` (`base64("vsesvit-extension:"+id)`) for keyless managed installs; never write into a developer's unpacked folder
- [ ] Load updates over the old version in place so `chrome.storage` survives (self-test `2.0:1.0`)
- [ ] A withheld update never runs before Re-enable; the old version stays off; Re-enable grants and loads the new one
- [ ] Disabled extensions are not running (switch off right after load if the engine starts them)
- [ ] Unload from the engine before core deletes files; on core refusal, load it back
- [ ] Remove engine extensions core does not know, except the engine's own component and built-in extensions (by id)
- [ ] Guard engine calls against never answering (timeouts)
- [ ] Startup: `--load-extension DIR` (repeatable) installs through the pipeline without prompting, then reconcile
- [ ] Reconcile at startup and whenever sync reports `changed.extensions`; installs for other devices never prompt
- [ ] Periodic store update checks (60 s after start when due, every 5 h; interactive only), one at a time
- [ ] Send the engine's real Chromium version as `OpenOptions::chrome_version`
- [ ] Extensions dialog opens from the main menu "Extensions", the puzzle menu and the button menu ("Manage extensions")
- [ ] Install box: parse store links, ids, gecko ids, file paths and `file://`; Install, File... (.crx/.xpi), Folder...; one install at a time; progress text and fraction; result and error wording
- [ ] Install prompt for user store and package installs (`needs_approval`): `Add “X”?`, "It can:", bullet warnings with indented site details; Cancel drops, Add commits; unpacked installs commit without asking
- [ ] Installed list in install order: icon, name, "version · verification label · id", engine error line, Re-enable notice and button, On/Off switch (disabled while withheld), Remove
- [ ] Switch failure snaps back to core's state with "Could not change …"
- [ ] Update button: "Checking for updates…", then `UpdateReport::summary()` (including the withheld sentence)
- [ ] "No extensions installed" empty state; the dialog follows changes made elsewhere live
- [ ] Store pages (CWS, Edge Add-ons): the Add/Get button installs through core after Chrome's prompt (Cancel default, so Enter does not accept); Remove asks `Remove “X”?`; install state answers (`getAll` / `get` / `getExtensionStatus`); origin checked at request and answer time; https store origins only; Chrome-style ids only
- [ ] Welcome page "Add extensions worth having": uBlock Origin Lite, Bitwarden, Proton Pass rows with Install / Installing… / Installed / Try again, progress and status; no prompt
- [ ] Toolbar: pinned action buttons (36×32, 16 px icon or puzzle glyph, tooltip = action title or name) after the address pill and before Downloads; puzzle button shown while any action exists
- [ ] New installs are pinned by default; pins and order in synced `toolbar.extensions`; entries for absent extensions preserved; synced changes repaint every window
- [ ] Drag to reorder pinned buttons (`move_pinned`)
- [ ] Pinned button context menu: Unpin, Manage extensions (plus the extension's `action` context menu items, as in Chrome; WinUI could not)
- [ ] Puzzle menu: heading "Extensions", every action (icon, title, opens popup under the puzzle), pin toggle with "Pin to the toolbar" / "Unpin from the toolbar", divider, "Manage extensions"
- [ ] Toolbar extension area is excluded from the title-bar drag region when tabs are on top
- [ ] Action popup opens under its pinned button, or under the puzzle button when unpinned; destroyed on close; `window.close()` closes it; light dismiss
- [ ] Popup sizes to content within 25..800 × 25..600 before its scripts get tab answers
- [ ] In a popup, "current window" is the opener browser window and its active tab is the selected tab; `tabs.getCurrent()` is undefined
- [ ] Popup takes focus on click; shortcut-opened popups do not steal focus when the window is in the background
- [ ] Action without popup dispatches `action.onClicked` (Chrome; WinUI did nothing)
- [ ] Dynamic action state: badge text and color, `setIcon`, `setTitle`, `setPopup`, enable/disable (Chrome; WinUI could not)
- [ ] Links and new windows from popups open as tabs in the opener window (Chrome; WinUI left WebView2 defaults)
- [ ] Page context menu shows extensions' `contextMenus` items and dispatches `onClicked` with page and frame info; the browser's own additions stay
- [ ] Extension action shortcuts (`_execute_action`, `_execute_browser_action`, `_execute_page_action`) open the popup, taken before the page, in pages and cross-site frames
- [ ] Named extension commands fire `commands.onCommand` (with the tab, `activeTab`, `global`) (Chrome; WinUI could not)
- [ ] Extension chord resolution: browser chords win, then user overrides, then suggested keys in install order; one chord per command
- [ ] Settings > Keyboard shortcuts "Extension shortcuts": rows "<ext>: <title>", keycaps or "Disabled", capture flyout notes ("Extension shortcuts need Ctrl or Alt.", "Used by …, which an extension cannot take", "Also used by …: …. Saving moves it here."), Esc / Backspace / Enter, per-row reset, Reset all includes extensions, accelerators suspended while capturing
- [ ] Browser command capture notes an extension holder and takes its chord
- [ ] Private windows: no extension buttons, popups or shortcuts; no content scripts; per-extension "Allow in private windows" (`allowed_in_private`) as in Chrome (WinUI: never)
- [ ] Store installs from a private window go to the normal profile (WinUI behavior, unguarded); decide deliberately
- [ ] `chrome-extension://` URLs typed in the address box navigate directly
- [ ] Extension pages never sleep under Memory Saver (core rule)
- [ ] `storage.sync`: decide on bridging into core's `ExtStorageSync` records (WinUI: not synced)
- [ ] Per-extension notification switch and `permissions.request` prompt (core APIs exist; GTK uses them; WinUI did not)
- [ ] Self-test parity: install_crx, engine_loaded_extension, content_script, extension_port, dnr_blocked, dnr_site_allowed, popup, context_menus, extension_commands, extension_notifications, extension_update, private_window, cws_install (network), plus the toolbar, dialog and prompt ui-smoke steps
