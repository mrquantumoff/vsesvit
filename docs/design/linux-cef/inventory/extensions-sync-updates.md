# Behavior inventory: extensions, extension menus, sync, self-updates (old GTK shell)

Scope: everything a GPUI + CEF Linux shell must reproduce from

- `crates/vsesvit-gtk/src/extensions.rs` (433 lines): the shell's driver of `vsesvit_core::extensions` and its hand-off to `vsesvit-webext`.
- `crates/vsesvit-gtk/src/extension_menus.rs` (80 lines): extensions' `chrome.contextMenus` items rendered into the page context menu and the toolbar action menu.
- `crates/vsesvit-gtk/src/sync.rs` (842 lines): the `Syncer` (sign-in, rounds, timers, passphrase prompting, account details).
- `crates/vsesvit-gtk/src/updates/{mod.rs,lifecycle.rs,headless.rs}` (304 + 644 + 20 lines): the self-updater lifecycle.

Those files cannot be understood alone, so this inventory also covers the parts of their callers that make them user-visible: `dialogs/extensions.rs` (the Extensions dialog), `dialogs/extension_prompts.rs` (install prompt, `permissions.request` prompt), `dialogs/passphrase.rs` (sync passphrase dialogs), the Sync page and the Updates group of `dialogs/settings.rs`, the welcome's Extensions and Sync pages (`dialogs/welcome.rs`), the extension parts of `browser.rs`, `window/mod.rs`, `window/actions.rs`, `window/ext_actions.rs`, `page_menu.rs`, `keymap.rs`, `app.rs`, `lib.rs`, and the self-test checks that drive them (`self_test.rs`). Other inventories cover those files in full; here they appear only where they call into the four target modules.

All paths below are relative to the repo root (`/home/dy/git/vsesvit-cef`, branch `linux-cef-gpui`).

## Legend

- **[V]** verified by reading the code or docs named next to it. **[I]** inference (not verified against code or docs).
- **(KEEP)** product behavior the new shell must reproduce.
- **(WEBKIT-ONLY)** exists only because of WebKitGTK or the home-grown runtime; disappears with Chromium.
- **(GTK-ONLY)** a GTK/libadwaita/GLib mechanism; the behavior is KEEP but the mechanism goes (for example `glib::timeout_add`, `gio::SimpleAction`, `adw::Toast`).
- **(NEEDS-CEF)** needs a CEF/Chromium equivalent; the likely CEF API is named when known.

Verified CEF facts used below (from `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cef-dll-sys-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs` and the `cef-154.5.0+154.0.34` crate) [V]:

- `CEF_VERSION = "154.0.34+g14c5a08+chromium-154.0.8037.98"`; `CHROME_VERSION_MAJOR/MINOR/BUILD/PATCH = 154/0/8037/98`.
- There is **no** `_cef_extension_t`, `cef_extension_handler_t` or `load_extension` in the 154 bindings: CEF's own extension-loading API is gone. Any extension support comes from Chrome's extension system inside Chrome-style browsers.
- `cef_runtime_style_t` = `CEF_RUNTIME_STYLE_DEFAULT | CHROME | ALLOY`. The `_cef_window_info_t.runtime_style` doc: "Alloy style will always be used if `windowless_rendering_enabled` is true." `shared_texture_enabled` doc: "Currently only supported on Windows (D3D11)."
- Present callbacks: `cef_browser_host_t.execute_dev_tools_method`, `add_dev_tools_message_observer`; `cef_context_menu_handler_t.on_before_context_menu`, `run_context_menu`; `cef_command_handler_t.on_chrome_command`, `is_chrome_toolbar_button_visible`, `is_chrome_app_menu_item_visible`, `is_chrome_page_action_icon_visible`; `cef_permission_handler_t.on_show_permission_prompt`; `cef_download_handler_t.on_before_download`, `can_download`; `cef_keyboard_handler_t.on_pre_key_event`; `cef_life_span_handler_t.on_before_popup`; `cef_request_handler_t.on_before_browse`, `on_open_urlfrom_tab`; `cef_request_context_t` (base `cef_preference_manager_t` with `set_preference`, `get_preference`, `add_preference_observer`), `set_content_setting`, `get_website_setting`; `cef_browser_view_delegate_t.get_chrome_toolbar_type` (`CEF_CTT_NONE/NORMAL/LOCATION`). The Rust crate exposes them as `cef::Impl*` traits (e.g. `ImplContextMenuHandler::run_context_menu`, `ImplCommandHandler::on_chrome_command`, `ImplKeyboardHandler::on_pre_key_event`, `ImplBrowserHost::execute_dev_tools_method`, `ImplPreferenceManager::set_preference`).
- Chrome command ids exported: `IDC_MANAGE_EXTENSIONS = 40022`, `IDC_EXTENSIONS_SUBMENU = 57334`, `IDC_CONTENT_CONTEXT_CUSTOM_FIRST = 47000`, `IDC_EXTENSIONS_CONTEXT_CUSTOM_FIRST = 49000`.
- Permission prompt types include `CEF_PERMISSION_TYPE_NOTIFICATIONS = 32768`, `CEF_PERMISSION_TYPE_LOCAL_NETWORK = 67108864`, `CEF_PERMISSION_TYPE_LOOPBACK_NETWORK = 134217728`.

---

# Part A. Extensions (`extensions.rs` and the surfaces it drives)

## A.1 What `extensions.rs` is [V]

Module doc: an install is core's three-step pipeline: `prepare_install` on the UI thread, `InstallJob::run` on a worker thread (`gio::spawn_blocking`) with its progress relayed back over a channel, and `commit` on the UI thread, after which the runtime loads the extension. A user install started in the Extensions dialog waits between run and commit for Chrome's install prompt. An update check runs the same way (`prepare_update_check`, `UpdateCheck::run` on a worker, `commit_updates`), then the runtime reloads each updated extension at its new version, or stops one that waits for the user to re-enable it. Every change to the enabled set re-applies the keymap, which binds extensions' commands.

It adds methods to `Browser` (an `Rc<Inner>` that owns the `Profile` as `core: Rc<RefCell<Profile>>` and the `vsesvit_webext::Runtime`), plus three free functions. Browser fields it uses (in `browser.rs`) [V]:

- `extension_errors: RefCell<HashMap<ExtensionId, String>>` "Why the runtime could not load an enabled extension, by extension." Accessors `extension_error(id)`, `set_extension_error(id, Option<String>)`.
- `updating_extensions: Cell<bool>` "Set while an extension update check runs."
- `extension_views: RefCell<Vec<Weak<dyn Fn()>>>` open Extensions dialogs, refreshed via `watch_extensions(&Rc<dyn Fn()>)` / `extensions_updated()` after an update check changed extensions.

## A.2 Failure types and their wording (user-visible) [V]

`InstallFailure` (Display text in quotes):

| Variant | Meaning | Display |
|---|---|---|
| `Prepare(vsesvit_core::Error)` | `prepare_install` refused (e.g. non-Unicode path) | the core error |
| `Run(InstallError)` | download/verify/unpack/manifest failed | the `InstallError` text (see A.20) |
| `Commit(vsesvit_core::Error)` | `commit` refused (e.g. `VerifiedIdTaken`, `IdCaseConflict`) | the core error |
| `Load(Box<InstalledExtension>, LoadError)` | committed to the profile, but the runtime cannot run it | "`{name} {version}` is installed but cannot run: `{e}`" |
| `WorkerPanicked` | worker thread panicked | "the install thread panicked" |
| `Cancelled` | user cancelled the install prompt | "the install was cancelled" |

`EnableFailure`: `Core(Error)` → the core error; `Load(LoadError)` → "it cannot run: `{e}`" (the switch is stored but the runtime cannot run it).

`UpdateFailure`: `Running` → "an update check is already running"; `Prepare(Error)` → the core error; `WorkerPanicked` → "the update thread panicked".

`LoadError` (webext) has one variant: `Io { path, source }` → "`{path}`: `{source}`" (a file the manifest names is missing or unreadable). (WEBKIT-ONLY as a type; the concept "committed but the engine cannot run it" is KEEP and NEEDS-CEF, see A.16.)

## A.3 Install flows

### A.3.1 Pipeline methods [V]

- `Browser::install(source, progress) -> Result<Option<InstalledExtension>, InstallFailure>`: `core.extensions().prepare_install(source)` then `run_install_job`. **No prompt.** `Ok(None)` = the extension was uninstalled on another device while it downloaded (reconcile installs only).
- `Browser::install_asking(window, source, progress)`: `prepare_install`, then `stage` (worker run), then if `staged.needs_approval()` shows `extension_prompts::install_dialog(staged.manifest())` with `choose_future(Some(window))`; any answer but `"add"` returns `Err(Cancelled)` (the staged files are dropped, nothing persisted). Then `commit_install`.
- `Browser::run_install_job(job, progress)`: `stage` then `commit_install` (no prompt).
- `stage(job, progress)`: unbounded `futures_channel::mpsc` channel; `gio::spawn_blocking(move || job.run(&mut |phase| tx.send(phase)))`; a local future pumps phases into `progress` on the UI thread; awaits the worker (panic → `WorkerPanicked`), then the pump (ends when the sender drops), maps `InstallError` → `Run`.
- `commit_install(staged)`: `core.extensions().commit(staged)` (→ `Commit` on error), **then `apply_keymap()`**, then if `Some(ext)`: `load_into_runtime(&ext)`; a load failure returns `Err(Load(ext, e))` although the extension is committed.
- `progress_to(f: Fn(String))` adapts `InstallPhase` to `phase.describe()` (a UI-thread-only, non-`Send` callback).

Core decisions behind these calls (vsesvit-core, [V] `crates/vsesvit-core/src/extensions/{mod.rs,install.rs}`):

- `InstallSource::parse(text)` accepts `https://chromewebstore.google.com/detail/<slug>/<id>` and `.../detail/<id>`, legacy `https://chrome.google.com/webstore/detail/...`, `https://microsoftedge.microsoft.com/addons/detail/...`, a bare 32-char `a..p` id (Chrome Web Store), `https://addons.mozilla.org/<locale>/firefox/addon/<slug>/` (locale/app optional), a bare gecko id (`name@domain`, `{uuid}`), `file://` URLs or paths ending in `.crx`/`.xpi`, or naming a directory with `manifest.json` (or the `manifest.json` itself). Query strings and fragments ignored. Errors (`SourceParseError`): "not a Chrome Web Store, Edge Add-ons or addons.mozilla.org extension URL, an extension id, or an extension file" and "the store URL does not contain a valid extension id".
- `InstallSource::from_path(path)` (file pickers, drag-and-drop): `.crx` → `CrxFile`, `.xpi` → `XpiFile`, `manifest.json` file or a dir containing it → `Unpacked { dir }`. Path made absolute with `..` resolved lexically (an unpacked id is a hash of its path).
- `Extensions::prepare_install(source)`: cheap; allocates a staging dir name, captures `chrome_version`, `ui_locale()`, `Stores`; refuses non-Unicode paths; `Intent::User`.
- `InstallJob::run(progress)`: blocking. CWS: GET update2 `?response=redirect&prodversion=<chrome_version>&acceptformat=crx2,crx3&x=id%3D<id>%26uc`, verify every CRX3 proof + id binding + CWS publisher key. Edge: Omaha update check, sha256, Edge publisher key. AMO: API JSON, XPI with sha256. Local CRX: any developer key. Local XPI: no signature. Unpacked: `Manifest::load(dir)` only, nothing copied. Then unpack (zip-slip, symlink, Windows-name, size, ratio checks; limits `MAX_ARCHIVE_BYTES = MAX_UNPACKED_BYTES = 256 MiB`, `MAX_ENTRIES = 20_000`, ratio 100), set manifest `key` (CRX: the verified key so the dir keeps the store id; XPI: removed), localize the manifest (`__MSG_*__` with `ui_locale()` from `LC_ALL`/`LC_MESSAGES`/`LANG`, default `en`).
- `InstallPhase::describe()` lines (identical in both shells): "Looking up the add-on…", "Downloading… 1.3 MB of 9.8 MB" / "Downloading… 500 KB", "Verifying the signature…", "Unpacking…", "Reading the manifest…". `fraction()` exists (0..1 while downloading with a known length) but the GTK shell shows no bar, only the line.
- `StagedInstall::needs_approval()` = `intent == User && source is not Unpacked`. So: store and package installs from the Extensions dialog ask; unpacked folders, reconcile installs and update installs never ask.
- `Extensions::commit(staged)`: idempotent; reconcile/update commits return `Ok(None)` if the synced record no longer wants the extension; refuses an id held more firmly (`InstallError::VerifiedIdTaken`) or differing only in case (`IdCaseConflict`); keeps an installed newer version unless the new copy holds the id more firmly; unpacked replaces at any version; renames the staging root to `<profile>/extensions/<id>/<version>_<hash32>`; a user store install sets the synced record `installed := true`, `store`, and `enabled := true` for a new/previously uninstalled record; upserts `extension_installs`; an update keeps `granted` so a version Chrome warns about anew becomes `withheld`.

### A.3.2 Extensions dialog installs (user) [V] `dialogs/extensions.rs`

1. The "Install" group's entry row (see A.6) on apply: trims the text; if not empty, `InstallSource::parse(&text)`, clears the row. A parse error toasts the `SourceParseError` text.
2. "Install from File…" opens a modal file chooser titled "Install Extension from File", filter "Extension packages (.crx, .xpi)" (suffixes `crx`, `xpi`), default filter selected; the chosen path goes through `InstallSource::from_path`. Cancel: nothing (debug log "no extension file chosen").
3. "Load Unpacked Extension…" opens a modal folder chooser titled "Load Unpacked Extension"; the folder goes through `InstallSource::from_path` (must contain `manifest.json`, else the parse error toast).
4. `State::install(source)`: `install_started()` (counter +1, progress row subtitle "Starting…", row shown); calls `browser.install_asking(&window, source, progress_to(|text| progress_row.set_subtitle(text)))`; then `install_finished()` (counter −1; row hidden only when no install is left, since two installs can overlap and share the one row); then toasts:
   - `Ok(Some(ext))` → "Installed `{name} {version}`"
   - `Ok(None)` → "The extension was removed elsewhere while it downloaded"
   - `Err(Load(ext, e))` → "Installed `{name} {version}`, but it cannot run: `{e}`"
   - `Err(Cancelled)` → nothing
   - other `Err(e)` → "Install failed: `{e}`"
   - and refreshes the list.
5. The install prompt is shown over the **browser window** (`choose_future(Some(window))`), i.e. on top of the Extensions dialog.

### A.3.3 Welcome page installs [V] `dialogs/welcome.rs`

The welcome's "Extensions" page (title "Extensions", description "A few to start with, from the Chrome Web Store. The Extensions page adds more.") lists `vsesvit_core::onboarding::RECOMMENDED_EXTENSIONS`: uBlock Origin Lite (`ddkjiahejlhfcafbddmgiahcphecmpfh`, "Blocks ads and trackers with declarativeNetRequest."), Bitwarden (`nngceckbapebfimnlniiiahkandclblb`, "Open-source password manager that syncs across your devices."), Proton Pass (`ghmbeldphafepmbegfdlkpapadhbakde`, "End-to-end encrypted password manager with hide-my-email aliases."). Each row has an "Install" button, or reads "Installed" when `extensions().get(id)` finds it. Clicking calls `browser.install(recommended.install_source(), |_| {})` (**no install prompt**, verified), then shows "Installed", or the error: "Removed on another device while it downloaded" for `Ok(None)`, or the failure text. (KEEP)

### A.3.4 Reconcile installs (from other devices) [V]

`Browser::reconcile_extensions()`: `core.extensions().reconcile()` (error: log "cannot reconcile extensions: {e}", return). For each id in `work.removed` → `unload_from_runtime(id)`. Then `apply_keymap()`. For each `InstallJob` in `work.install`: spawn a local future running `run_install_job(job, |_| {})` (no prompt, no UI), logging "installed {id} from another device's list" or "reconcile install from {source:?} failed: {e}". Called from `Browser::start()` and from `Browser::sync_applied()` when `changed.extensions` is set.

Core `reconcile()`: desired (`extensions` table, synced) minus actual (`extension_installs`, local) → install jobs (`Intent::Reconcile`) for store records with `installed = true` and no local row (or a local row holding the id less firmly, e.g. an unpacked dir carrying a store key); local store installs whose record is no longer `installed` → deleted locally and returned in `removed` (their dirs are left for the GC at next open, since the engine may still hold files).

Observed gap [V by reading]: `reconcile_extensions` does not reload extensions whose synced `enabled` register changed on another device (core's `Reconcile` reports only installs and removals). The keymap is re-applied (so command bindings follow), but the runtime keeps running or not running the extension until restart. The Windows shell's reconcile brings WebView2's list fully in line with core's (per `crates/vsesvit-winui/src/extensions.rs` module doc). The new shell should decide deliberately.

## A.4 Install prompt (Chrome's "Add “X”?") [V] `dialogs/extension_prompts.rs`

- An `adw::AlertDialog` with heading `permissions::install_heading(name)` = `Add “{name}”?`.
- Extra child: a vertical list (6 px spacing) of wrapped, left-aligned labels: the lead `INSTALL_LEAD` = "It can:" only when there are warnings; then each `PermissionMessage` as "• {text}" (margin 6) and each of its `details` as "◦ {detail}" (margin 24). Warnings come from `permissions::install_warnings(manifest)` (Chrome's message rules, coalesced: `<all_urls>` + `tabs` reads only "Read and change all your data on all websites"; content-script sites count).
- (WEBKIT-ONLY) A last caption label in warning style: "Not supported by the Linux runtime: {notice}. Features that depend on them will not work." from `unsupported_notice(manifest)`.
- Responses: "_Cancel" (id `cancel`, default response, close response: Escape and Enter cancel) and "_Add Extension" (id `add`, suggested appearance). Only `add` installs.
- Expected text for the probe (self-test `install_prompt`): `Add “Vsesvit Probe”?`, `It can:`, `• Read and change all your data on all websites`, `• Display notifications`. Unit test: Reader with `tabs`, `webRequest`, `https://a.example/*` → "• Read and change your data on a.example", "• Read your browsing history" (+ the unsupported line).
- `shown_lines(dialog)` (heading then each label text) exists for checks.
- (KEEP) The prompt and its wording. (NEEDS-CEF) only in that Chrome's own install UI must not also appear: if CEF's Chrome extension system installs packages itself (e.g. from the Chrome Web Store page's "Add to Chrome", see A.21), Chrome would show its own prompt and bypass core; core's pipeline must stay the one that installs.

## A.5 `permissions.request` prompt [V] `browser.rs` `Host::ask_permissions`, `dialogs/extension_prompts.rs::request_dialog`

- The runtime calls `TabHost::ask_permissions(window_id, prompt, answer)` when an extension's `permissions.request` needs the user. If the window is gone, `answer(false)`. Otherwise an `adw::AlertDialog` over that window: heading `permissions::request_heading(name)` = `“{name}” has requested additional permissions.`; lead `REQUEST_LEAD` = "It could:"; bullets as in A.4 (e.g. "• Read and change your data on a number of websites" with "◦ a.example" … indented). Responses "_Deny" (default and close response) and "_Allow" (suggested); `answer(response == "allow")`.
- Which window: the one the request came from, or the last focused window the extension may know, never a private one it does not run in (webext decides).
- Core keeps grants in the local pref `extensions.granted_permissions` (not synced) via `Extensions::grant_permissions` / `remove_permissions` / `active_permissions`.
- (KEEP wording and the "never over a private window it does not run in" rule; NEEDS-CEF: Chrome runs `permissions.request` itself and shows Chrome's own prompt (ExtensionInstallPrompt "permissions" type) [I]; CEF exposes no hook to replace that dialog with the shell's [I]. Whether Chrome's prompt can appear at all over a client-parented Chrome-style browser, and whether Chrome's grants can be mirrored into core's `extensions.granted_permissions`, is an open question.)

## A.6 The Extensions dialog [V] `dialogs/extensions.rs`

Opened by: main menu Library section "_Extensions" (`win.show-extensions`), the puzzle-piece menu's "Manage Extensions", a pinned action's context menu "_Manage Extensions", and (expanded on one extension) a notification's Settings button (`present_extension(window, id)`). No keyboard shortcut. It is an `adw::PreferencesDialog` titled "Extensions" presented over the window, one page:

**Group "Install"**, description "Paste a store link or id, choose a .crx or .xpi file, or load a folder for development":

1. Entry row titled "Chrome Web Store, Edge Add-ons or Firefox Add-ons link or ID", with an apply button, input purpose URL.
2. Button row "Install from _File…" (mnemonic F, icon `package-x-generic-symbolic`).
3. Button row "Load _Unpacked Extension…" (mnemonic U, icon `folder-symbolic`).
4. Progress row "Installing…" with a spinner prefix, hidden until an install runs; its subtitle is the current `InstallPhase::describe()` line ("Starting…" first).

**Group "Installed"**, header suffix: a flat "_Update" button (widget name `update-extensions`, tooltip "Check every extension for a newer version now"). Pressing it makes it insensitive, runs `browser.update_extensions()`, makes it sensitive again and toasts `report.summary()` or "Cannot update the extensions: {e}". The rows follow via `watch_extensions`.

Rows, in `extensions().list()` order (install order; an update keeps the position):

- No extensions: one dim row "No extensions installed".
- Each extension: an expander row
  - title = `manifest.name` (not markup), subtitle = "`{version}` · `{verification.label()}`". Labels (core, both shells): "Chrome Web Store, publisher verified", "Edge Add-ons, publisher verified", "Firefox Add-ons, hash checked", "Local CRX, signature verified", "Local XPI, not verified", "Unpacked folder, not verified".
  - prefix: the largest icon the manifest declares (`icon_path`: last entry of `manifest.icons`, resolved against `ext.dir`, must be a file), at 32 px; fallback icon `application-x-addon-symbolic`.
  - expanded when it is the extension the dialog was opened for, or when it has a warning row (unsupported, re-enable, not running).
  - suffix 1: switch, tooltip "Enabled", `active = ext.enabled`, **insensitive while `ext.withheld` is not empty**. See A.7.
  - suffix 2: flat "Remove" button (icon `user-trash-symbolic`, tooltip "Remove"). See A.9. **No confirmation.**
  - child rows, in this order:
    1. (WEBKIT-ONLY) warning row "Not supported by the Linux runtime", subtitle "This extension requests: {notice}. Features that depend on them will not work." (icon `dialog-warning-symbolic`).
    2. (KEEP) re-enable row when `withheld` is not empty, see A.8.
    3. (KEEP concept) error row "Not running", subtitle = the stored load error, error style, icon `dialog-error-symbolic`; only when `extension_error(id)` is set **and** `ext.enabled`.
    4. (KEEP) switch row "Notifications", subtitle "Let this extension show notifications", only when `manifest.permissions` contains `notifications`. See A.11.
    5. (KEEP) switch row "Allow in private windows" (`ALLOW_IN_PRIVATE_ROW`), subtitle "Vsesvit cannot stop it from recording what you do there", active = `extensions().allowed_in_private(id)`. See A.10.
    6. (KEEP) property row "ID", subtitle `ext.id`, selectable.
    7. (KEEP) property row "Location", subtitle `ext.dir` (managed `<profile>/extensions/<id>/<version>_<hash32>` or the developer's folder), selectable, wraps.

The dialog's state holds the window weakly (it must not keep the profile open). `refresh()` rebuilds every row from `installed_extensions()` and `extension_error`. Toasts are the dialog's own (`dialog.add_toast(plain_toast(text))`, plain text, never markup).

Not present in the GTK shell (verified absent): a "Reload" button for unpacked extensions (core has `Extensions::reload_unpacked(id)`, unused on Linux), an "Options"/"Details" link, drag-and-drop install onto the dialog, a developer-mode switch, an errors page.

## A.7 Enable / disable [V]

- The switch's `state-set` handler: browser gone → put the switch back after the handler returns, stop. Else remember whether a load error was shown, call `browser.set_extension_enabled(&id, active)`:
  - `Ok` → proceed.
  - `Err(EnableFailure::Load(e))` → toast "Enabled, but it cannot run: {e}" (the switch stays on: the choice is stored).
  - other `Err(e)` → toast "Cannot change the extension: {e}", refresh after idle (shows what the profile holds, or no row if the extension is gone, e.g. removed by sync meanwhile), stop the switch change.
  - If the "error shown" state changed, refresh after idle (so the "Not running" row appears/disappears) without replacing the switch under its own handler.
- `Browser::set_extension_enabled(id, enabled)`: `core.extensions().set_enabled(id, enabled)` (store installs: the synced `enabled` register; local installs: `extension_installs.local_enabled`), then `extension_changed(id)`.
- `extension_changed(id)`: `apply_keymap()`; `core.extensions().get(id)`: `Some(ext)` → `load_into_runtime(&ext)` (map error to `EnableFailure::Load`); `None` → `unload_from_runtime(id)`.
- `load_into_runtime(ext)`: if `!ext.enabled` (this includes withheld), unload and return Ok. Else `runtime.load(ext)` (always `LoadReason::Startup`; the shell never uses `load_with(.., LoadReason::Enable)` although webext documents it for re-enabling); on error log "extension {id}: {e}" and store the error text; on success clear it.
- `unload_from_runtime(id)`: `runtime.unload(id)` and clear the stored error.
- Unit test: a manifest naming a missing content script installs (committed) but `install` returns `Err`; `set_enabled(false)` succeeds, `set_enabled(true)` errors, the runtime does not list it, `extension_error` contains `missing.js`; disabling clears the error.
- (KEEP) behavior. (NEEDS-CEF) the engine side: enabling/disabling an extension inside Chromium. No CEF API exists [V: no extension API in bindings]. Candidates [I]: Chrome's extension prefs (`extensions.settings.<id>.state` / `disable_reasons`) through `CefRequestContext::SetPreference` if writable; DevTools protocol through `ExecuteDevToolsMethod` (the `Extensions` domain has `loadUnpacked` and, in recent Chromium, `uninstall`, but no enable/disable as far as known); or unloading/reloading the unpacked dir.

## A.8 Re-enable an update that asks for more (Chrome's "Enable “X”?") [V]

- A store update whose new warnings exceed what the user approved is committed as `withheld` (`InstalledExtension.withheld: Vec<PermissionMessage>`, local to this device); `enabled` is false while withheld; the runtime does not run it (`load_into_runtime` unloads).
- Row: title `permissions::re_enable_heading(name)` = `Enable “{name}”?`; subtitle `re_enable_text(&ext.withheld)` = "It can now:" (`RE_ENABLE_LEAD`) then "• {warning}" lines with "    ◦ {detail}" sub-lines, joined by newlines; warning style, icon `dialog-warning-symbolic`; suffix button "_Re-enable".
- "_Re-enable" → `browser.approve_extension_permissions(&id)` → `core.extensions().approve_permissions(id)` (grants what the update added, clears `granted`, clears `engine_id` when it was withheld) → `extension_changed(id)` (loads it if the user's choice is enabled). Toasts: "Re-enabled, but it cannot run: {e}" or "Cannot re-enable the extension: {e}". Then refresh: the switch becomes sensitive and on, the re-enable row disappears.
- Expected self-test text: `Enable “Vsesvit update probe”?` / "It can now:\n• Read your browsing history" for an update adding `tabs`.
- `UpdateReport::summary()` adds, per withheld update: "The newest version of the extension “{name}” requires more permissions, so it has been disabled. It can now: {warnings joined by "; "}".
- (KEEP) behavior. (NEEDS-CEF) The new version's code must not run in Chromium before approval: Chromium should keep the version before (off) or not load the new dir until `approve_permissions`. Windows does exactly that with WebView2 (`engine_id` logic in core). With Chromium the equivalent is whatever loads/unloads/disables extensions (see A.7, A.16).

## A.9 Uninstall [V]

- "Remove" → `browser.uninstall_extension(&id)`; error toast "Cannot remove the extension: {e}"; refresh.
- `uninstall_extension(id)`: `unload_from_runtime(id)` **first** (so the runtime never runs files the uninstall removes), then `core.extensions().uninstall(id)`, then `apply_keymap()`. If core refused (e.g. DB locked), reload the extension as the profile still has it (a load failure is kept for the dialog) and return the error.
- Core `uninstall`: store installs → synced `installed := false` (other devices remove it); local installs that took a store id leave the record alone (reconcile brings the store copy back); deletes the local row and `storage.local`, keeps `storage.sync`; removes the managed dir best-effort (GC at next open otherwise); never touches an unpacked dir; forgets granted permissions.
- Unit test: with an exclusive SQLite lock held, uninstall errors and the extension keeps running; after the lock is released, uninstall succeeds and it stops.
- (KEEP). (NEEDS-CEF) removing it from Chromium (and Chromium's own per-extension storage). Candidate [I]: DevTools `Extensions.uninstall`; or never letting Chromium own install state (load unpacked dirs at startup only).

## A.10 Allow in private windows [V]

- Switch row → `browser.set_extension_allowed_in_private(&id, active)`; error toast "Cannot change the extension: {e}".
- `set_extension_allowed_in_private`: `core.extensions().set_allowed_in_private(id, allowed)` (synced pref `extensions.allowed_in_private`, a `BTreeSet<ExtensionId>`, off until the user turns it on; ids of extensions this device lacks are kept); then `runtime.allowed_in_private_changed()` (each loaded extension joins/leaves open private tabs for loads that start afterwards); then `refresh_extension_actions()` on every private window (toolbar buttons appear/disappear there).
- Sync: `Browser::sync_applied` calls `runtime.allowed_in_private_changed()` when `changed.prefs` contains `extensions.allowed_in_private`.
- Effects (webext): in private tabs, a not-allowed extension gets no content scripts or rules, `chrome.tabs`/`chrome.windows` hide private tabs/windows, its toolbar action and context-menu items are not offered (`runtime.runs_in(id, Browsing::Private)`), its commands do nothing there.
- Self-test `private_window`: the probe's content script does not run in a private tab until `set_extension_allowed_in_private(probe, true)` and a reload, then reports `background-replied`.
- (KEEP). (NEEDS-CEF) Chrome's "Allow in Incognito" equivalent for the private session's request context [I: Chrome stores it as `extensions.settings.<id>.incognito`]; whether Chrome extensions run at all in a CEF off-the-record `CefRequestContext` is an open question.

## A.11 Notifications switch and notification activation [V]

- Switch row "Notifications" (only for extensions with the `notifications` permission), active = `core.extensions().notifications_allowed(id)` (on until turned off). On change: `core.extensions().set_notifications_allowed(id, active)` (synced pref `extensions.notifications_off`, a `BTreeSet<ExtensionId>` of extensions turned off; ids kept as in the toolbar pref) then `runtime.notification_permission_changed(id)` (webext closes that extension's notifications, refuses more with "Notifications are turned off for this extension.", fires `notifications.onPermissionLevelChanged` with `denied`/`granted`). Error → toast "Cannot change the extension's notifications: {e}" and refresh after idle.
- Notifications themselves (webext, WEBKIT-ONLY mechanism): `GNotification`s of the default `GApplication` whose clicks invoke `app.extension-notification` with parameter `(sss)` = (extension id, notification id, activation name: `default`, `settings`, or a button index). `app.rs` registers that action: `Activation::Settings` → `dialogs::extensions::present_extension(active window, ext)` (the Extensions dialog with that row expanded, as Chrome's notification Settings button leads to the extension's settings); other activations → `runtime.notification_activated(ext, id, activation)`.
- Self-test `extension_notifications`: the probe's notification shows title "Vsesvit Probe notification", body "Sent by the probe", one button "Open", a PNG icon; button 0 fires `notifications.onButtonClicked(id, 0)`; Settings opens the dialog on the probe's expanded row with Notifications on (`extension-notifications.png`); off → `denied`, notification closed, core off; on → `granted`.
- (KEEP) the per-extension switch, its sync, the Settings button leading to the extension's row. (NEEDS-CEF) Chromium shows extension notifications through its own platform bridge [I]; the shell needs a way to (a) enforce `extensions.notifications_off` inside Chromium (Chrome's per-extension notifier state [I]) and (b) route a notification's settings click to the Extensions view. No CEF notification hook is known to exist [I].

## A.12 Extension update checks [V]

`Browser::update_extensions() -> Result<UpdateReport, UpdateFailure>`:

1. If `updating_extensions` is set → `Err(Running)` (a concurrent call: "an update check is already running"; what the running one updates shows when it ends).
2. `core.extensions().prepare_update_check()` (records "now" as the last check in the LOCAL meta row `extensions_update_checked_ms` even if nothing is checked; captures an `Intent::Update` job for every extension installed here from a store that the synced record still wants, enabled or not) → `Err(Prepare)` on error.
3. Empty → `Ok(UpdateReport::default())` ("Your extensions are up to date").
4. Set the flag, `gio::spawn_blocking(move || check.run())` (Chrome Web Store gupdate XML in batches of 20, Edge Omaha 3.1 JSON in one request, AMO API per add-on; "newer" decided by `cmp_versions`, never by the store's status; one failure never stops others; each newer version fetched and verified exactly as an install), clear the flag (panic → `WorkerPanicked`).
5. `core.extensions().commit_updates(updates)` → `UpdateReport { updated, failed }`.
6. If anything updated: `apply_keymap()`, `load_into_runtime(ext)` for each updated (a withheld one is unloaded; a load failure is kept for the dialog), `extensions_updated()` (open dialogs refresh).
7. `UpdateReport::summary()` texts: "Your extensions are up to date", "N extension(s) could not be updated", "Updated 1 extension" / "Updated N extensions", "Updated N extensions; M could not be updated", plus withheld sentences (A.8), joined with ". ".

Periodic: `Browser::schedule_extension_updates()` (called from `start()` unless the self-test runs, `SCRIPTED`): a local future that loops for as long as the browser lives (weak ref): `due = core.extensions().next_update_check()` (`FIRST_CHECK_DELAY` = 60 s on a device that never checked, else until `UPDATE_INTERVAL` = 5 h after the last check, clamped to [60 s, 5 h]; on error log and use 5 h); sleep `due`; `update_extensions()`; log "extension update check: {summary}" or "extension update check failed: {e}". Results show only in the log (and in the dialog if open).

Updated extensions keep their id and storage; webext fires `runtime.onInstalled` with `update`. Self-test `extension_update`: 1.0 → page reads `1.0:1.0`; publish 2.0 → report `[(id, "2.0", enabled=true)]`, page `2.0:1.0` (storage kept); publish 3.0 adding `tabs` → through the dialog's Update button (insensitive while running) → version 3.0, withheld `["Read your browsing history"]`, not enabled, not running, row shows the re-enable text, switch insensitive (`extension-update.png`); Re-enable → running, page `3.0:1.0`. Unit test: two concurrent `update_extensions()` → second errors "an update check is already running".

(KEEP) all scheduling and wording; the network work is core's. (NEEDS-CEF) "reload at the new version" inside Chromium, keeping `chrome.storage` (Chromium keys storage by extension id, which stays stable because CRX installs carry `key` [I]); the version before must keep running (off) for a withheld update.

Chrome's own updater: core sends `prodversion` = `Profile::chrome_version()` (default `DEFAULT_CHROME_VERSION = "150.0.0.0"`; Windows passes WebView2's real version via `OpenOptions.chrome_version`). (NEEDS-CEF) The CEF shell can pass CEF's Chromium version (154.0.8037.98, from `CHROME_VERSION_*` [V]). Chromium's built-in extension auto-updater must not fight core's (it would update Chromium-owned installs behind core's back) [I].

## A.13 Startup sequence for extensions [V] `browser.rs`

`Browser::new`: the webext `Runtime` is created before any tab view (every tab view is built with the runtime's content manager) (WEBKIT-ONLY ordering); `runtime.connect_actions_changed` → every window `refresh_extension_actions()`; window removal → `runtime.windows_changed()`.

`Browser::start()`: `apply_keymap()` (binds extension commands too) → trackers/cookies → `core.extensions().list()` and `load_into_runtime` for each enabled one (failures logged and kept for the dialog; list error logged "cannot list extensions: {e}") → `reconcile_extensions()` → `schedule_extension_updates()` unless scripted → favicons → Memory Saver.

`BrowserWindow::new`: `refresh_extension_actions()` and `runtime.windows_changed()`.

## A.14 Extensions' keyboard shortcuts (`commands`) [V]

- `Browser::apply_keymap()`: `keymap = core.prefs().keymap()`; `keymap::apply(app, &keymap, &extension_shortcuts(&keymap))`. Called after every install commit, enable/disable, re-enable, uninstall, reconcile, update with changes, sync of `keyboard.shortcuts`, and Settings > Shortcuts edits (`edit_keymap`).
- `Browser::extension_shortcuts(keymap)` = `keymap.extension_shortcuts(commands::extension_commands(&installed_extensions()))`. Core rules: only **enabled** extensions' commands; browser chords always win; then stored user overrides (`keyboard.shortcuts` entries keyed `extension:<id>:<command>`) in install order; then manifest suggested keys in install order (first installed keeps a contested chord); at most one chord per command. `ExtensionCommand::title()` = description, else "Activate the extension" for an action command, else the name.
- `keymap::apply` binds each `(command, Some(chord))` to the detailed window action `win.extension-command(('<id>', '<name>'))`, after clearing every previously bound `win.extension-command(...)` accelerator. `keymap::suspend` clears them while the capture dialog is open.
- `win.extension-command` (parameter `(ss)`) → `BrowserWindow::run_extension_command(id, name)`: if `name` ∈ `manifest::ACTION_COMMANDS` = `["_execute_action", "_execute_browser_action", "_execute_page_action"]` → `activate_extension_action(id)` (popup or `action.onClicked`, like clicking the toolbar button); else `runtime.command(id, name, selected tab)` (fires `commands.onCommand` with the tab, grants `activeTab`).
- Settings > Shortcuts lists "Extension Shortcuts" (group shown only when an enabled extension has commands), one row per command (title = command title, subtitle = extension name), with capture dialog: an extension chord needs Ctrl or Alt ("An extension's shortcut needs Ctrl or Alt"); a browser-held chord is refused ("Used by {command}, which an extension cannot take"); taking another extension's chord notes "Also used by {ext}: {title}. Saving moves it here."; "Reset to Default". Core APIs: `Keymap::offer_extension`, `assign_extension`, `reset_extension`, `ExtensionShortcuts::{chord,is_default,command_for,note_for_browser}`; Reset All resets them too.
- Self-test `extension_commands`: probe's `_execute_action` = Alt+Shift+P, `probe-command` = Alt+Shift+K; activating `probe-command` fires `onCommand` with name, tab id and page URL; `_execute_action` opens the popup (`/popup.html`); Settings row shows Alt+Shift+K; assigning Alt+Shift+J moves the accelerator (Alt+Shift+K runs nothing); Reset to Default restores Alt+Shift+K and hides the reset button.
- (KEEP) core-resolved bindings, the Settings UI, and that commands work while a Vsesvit window has focus (GTK accelerators also work while the web view has focus [I: GTK app accelerators are processed before the focused widget]). (NEEDS-CEF) (a) key events that reach the CEF view must be checked against the bindings first: `CefKeyboardHandler::OnPreKeyEvent`; (b) firing `commands.onCommand` and `_execute_action` inside Chromium: no CEF API [I]; Chrome's own extension keybinding registry is tied to Chrome's browser window UI [I]. Chrome's own `extensions.commands` pref would also have to agree with core's resolution [I].

## A.15 Toolbar actions (interaction with `ext_actions.rs`, covered by another inventory) [V]

- `Browser::extension_actions()` = `runtime.actions()` sorted by install order; each `ActionInfo { extension, title, icon: Option<PathBuf>, has_popup, badge_text }`.
- `BrowserWindow::refresh_extension_actions()` filters to `runtime.runs_in(id, window.browsing())`, computes pinned order from the synced pref `toolbar.extensions` (`toolbar::layout`), rebuilds the toolbar: pinned buttons (icon 16 px + badge label), the puzzle-piece "Extensions" menu (heading "Extensions", each action with icon and title to open it and a pin toggle "Pin to Toolbar"/"Unpin from Toolbar", then "Manage Extensions"), drag-to-reorder (`win.extension-move (su)`), right-click action menu (Part B). Popups: the runtime's popup `WebView` in a popover under the button (or the puzzle piece when unpinned), sized to the document within 200×80..800×600 (initial 320×200); closed when its extension unloads or the page calls `window.close()`.
- `win.extension-pin (sb)` → `browser.pin_extension(id, pinned)`; `win.extension-move (su)` → `browser.move_extension(id, to)`; both write `toolbar.extensions` and refresh every window.
- (KEEP) all of this. (NEEDS-CEF) action state (title, icon, badge, popup URL, `onClicked`) lives in Chromium's extension system; CEF exposes no API to read action state or open an action popup [I]. Chrome-style browsers with `CEF_CTT_NORMAL` show Chrome's own toolbar (with extension icons) [I], which the GPUI chrome replaces; `ImplCommandHandler::is_chrome_toolbar_button_visible` / `get_chrome_toolbar_type` decide Chrome's toolbar visibility [V: callbacks exist].

## A.16 The hand-off contract to `vsesvit-webext` (what CEF replaces) [V]

Everything the GTK shell calls on `vsesvit_webext::Runtime` (`crates/vsesvit-webext/src/runtime.rs`), with what it decides:

| Call | When | Product meaning | CEF |
|---|---|---|---|
| `Runtime::new(profile, network_session, host)` | `Browser::new`, before any tab | one runtime per process | WEBKIT-ONLY |
| `load(&InstalledExtension) -> Result<(), LoadError>` | startup, after install/enable/re-enable/update, after a refused uninstall | run content scripts, background, action, DNR, menus; fire `onInstalled`/`onStartup`; `Err` = cannot run (missing file) | NEEDS-CEF: make Chromium run the dir at `ext.dir` (immutable managed dir or the developer's folder); report load errors |
| `unload(&ExtensionId)` | disable, withheld, uninstall (before core), reconcile removal | stop everything, close popups and ports | NEEDS-CEF |
| `loaded()` | self-test, unit tests | which extensions run | NEEDS-CEF (for checks) |
| `runs_in(id, Browsing)` | toolbar, page menu | whether it runs in normal/private tabs | NEEDS-CEF |
| `allowed_in_private_changed()` | private switch, sync | apply "Allow in private windows" | NEEDS-CEF |
| `notification_permission_changed(id)` | notifications switch | close/refuse notifications | NEEDS-CEF |
| `notification_activated(id, nid, activation)` | `app.extension-notification` | click/button | NEEDS-CEF |
| `notification(id, nid)` | self-test | what a notification shows | NEEDS-CEF (for checks) |
| `actions()`, `connect_actions_changed(f)` | toolbar | action list and changes | NEEDS-CEF |
| `activate_action(id, tab, show_popup)` | toolbar click, `_execute_action` | popup or `action.onClicked` | NEEDS-CEF |
| `page_menu(&Target) -> Vec<(ExtensionId, Entry)>` | page context menu | items for what was clicked | NEEDS-CEF (Chrome-style context menus may already include them, see Part B) |
| `action_menu(id) -> Vec<Entry>` | action right-click | items with `contexts: ["action"]` | NEEDS-CEF |
| `menu_clicked(id, item, tab, Option<&Target>)` | item chosen | `contextMenus.onClicked` | NEEDS-CEF |
| `command(id, name, tab)` | extension shortcut | `commands.onCommand` | NEEDS-CEF |
| `storage_sync_changed(ext, &[StorageChange])` | `sync_applied` with `changed.ext_storage` | remote `storage.sync` changes reach `storage.onChanged` | NEEDS-CEF (or drop: on Windows, `storage.sync` stays inside WebView2 and does not sync, PLAN "Synced data at a glance") |
| `private_session_started(&session)` | first private tab | cookies store "1" for allowed extensions | WEBKIT-ONLY |
| `tab_attached/moved/updated/activated/closed`, `tab_load`, `tab_opened_by`, `windows_changed` | tab/window events (browser.rs, tab.rs) | `chrome.tabs`/`windows`/`webNavigation` over Vsesvit's real tabs | NEEDS-CEF: Chrome's `chrome.tabs`/`chrome.windows` operate on Chrome's own browser/tab-strip objects [I]; how each CEF browser maps onto them decides whether extensions see Vsesvit's tabs as tabs of windows |
| `TabHost` (implemented in `browser.rs`): `windows`, `tabs`, `create_tab`, `update_tab`, `move_tab`, `remove_tab`, `web_view`, `create_window`, `update_window`, `remove_window`, `cookies_blocked`, `private_session`, `ask_permissions` | runtime asks the shell | extensions open/move/close Vsesvit tabs and windows; cookies blocked for sites the user blocked; permission prompts | NEEDS-CEF [I: Chrome-style CEF routes extension-created tabs/popups through `CefLifeSpanHandler::OnBeforePopup` / `CefRequestHandler::OnOpenURLFromTab`] |
| `pending_filters()`, `on_filters_ready` | self-test before DNR checks | DNR compile done | WEBKIT-ONLY |
| `vsesvit_webext::unsupported_features(manifest)` via `unsupported_notice` | dialog row, install prompt | what the Linux runtime lacks (`SUPPORTED_PERMISSIONS`: activeTab, alarms, contextMenus, cookies, declarativeNetRequest(+WithHostAccess), menus, notifications, scripting, storage, tabs, unlimitedStorage, webNavigation; `UNSUPPORTED_MANIFEST_KEYS`: chrome_settings_overrides, chrome_url_overrides, devtools_page, nacl_modules, omnibox, sandbox, side_panel, sidebar_action, user_scripts) | WEBKIT-ONLY as written; open question whether a reduced notice is needed for Chrome surfaces the GPUI chrome does not build (side panel, omnibox keyword, new-tab override, devtools page) |

Runtime state on disk: `<profile>/webext/` (compiled filters, dynamic rules, registered content scripts, install markers) (WEBKIT-ONLY; obsolete with Chromium, which keeps its own state under the CEF cache path).

## A.17 Persistent state (extensions) [V]

| Where | Key / path | Scope | Written by |
|---|---|---|---|
| SQLite `vsesvit.db` table `extensions` | `ExtensionRecord { id, store: Lww<StoreRef>, installed: Lww<bool>, enabled: Lww<bool> }` | synced (store installs only) | commit (user store install), set_enabled (store), uninstall (store), sync |
| table `extension_installs` | id, version, dir (managed `"<id>/<version>_<hash32>"` relative, or absolute unpacked path), source (JSON `InstallSource`), source_kind, verification, manifest (JSON), local_enabled, engine_id, installed_ms, granted (JSON `PermissionSet`, migration v11) | local | commit, set_enabled (local), approve_permissions, uninstall, reconcile |
| meta row | `extensions_update_checked_ms` (unix ms) | local | prepare_update_check |
| dirs | `<profile>/extensions/<id>/<version>_<hash32>/` immutable; `<profile>/staging/` wiped at open | local | commit (rename), uninstall, GC at open |
| pref | `extensions.allowed_in_private` (`BTreeSet<ExtensionId>`) | synced | private switch |
| pref | `extensions.notifications_off` (`BTreeSet<ExtensionId>`) | synced | notifications switch |
| pref | `extensions.granted_permissions` (`BTreeMap<ExtensionId, PermissionSet>`) | local | permissions.request/remove, approve_permissions |
| pref | `toolbar.extensions` (`Vec<toolbar::Entry { id, pinned }>`) | synced | pin/move |
| pref | `keyboard.shortcuts` (`extension:<id>:<command>` entries among the browser's overrides) | synced | Settings > Shortcuts |
| ext storage | `storage.local` (local, deleted on uninstall), `storage.sync` (synced kind `ExtStorageSync`, kept on uninstall) | | webext today |
| webext | `<profile>/webext/` | local | WEBKIT-ONLY |
| memory | `extension_errors`, `updating_extensions`, `extension_views` | process | shell |

## A.18 Classification summary (extensions)

| Piece | Class |
|---|---|
| Three-step install pipeline, worker thread, progress line, Cancel semantics | KEEP (mechanism `gio::spawn_blocking`/`glib::spawn_future_local` is GTK-ONLY) |
| Install prompt with core's words | KEEP |
| "Not supported by the Linux runtime" in prompt and dialog | WEBKIT-ONLY |
| Loading/unloading/reloading an extension dir in the engine, load errors | NEEDS-CEF (no CEF extension API in 154 [V]; candidates [I]: `--load-extension=<dir,...>` switch set in `CefApp::OnBeforeCommandLineProcessing` (startup only), DevTools `Extensions.loadUnpacked`/`Extensions.uninstall` via `ExecuteDevToolsMethod` (needs a browser-target session and, upstream, `--enable-unsafe-extension-debugging`), Chrome extension prefs) |
| Enable/disable, re-enable, withheld updates | KEEP + NEEDS-CEF (engine side) |
| Uninstall order (stop first, re-run if refused) | KEEP + NEEDS-CEF |
| Allow in private windows | KEEP + NEEDS-CEF (incognito equivalent) |
| Notifications switch, Settings button → dialog row | KEEP + NEEDS-CEF |
| `permissions.request` prompt | KEEP wording + NEEDS-CEF |
| Update checks (core), schedule (60 s / 5 h), Update button, summary | KEEP |
| Reconcile at start and after sync | KEEP (+ apply remote enabled changes, gap A.3.4) |
| Commands binding through core | KEEP; dispatch NEEDS-CEF (`OnPreKeyEvent` for keys in the page; firing `onCommand` unknown) |
| Toolbar actions/popups/badges | KEEP + NEEDS-CEF |
| `storage.sync` bridge | NEEDS-CEF or dropped (Windows parity) |
| `chrome_version` for CWS | NEEDS-CEF (use CEF's `CHROME_VERSION_*`) |
| DNR compile wait, `<profile>/webext/`, `private_session_started` | WEBKIT-ONLY |

## A.19 Interactions [V]

- `browser.rs`: owns fields and calls `start()`, `sync_applied()` (reconcile, private allowance, storage.sync), `apply_keymap()`, `extension_actions()`, `Host` (TabHost incl. `ask_permissions`).
- `dialogs/extensions.rs`: all user actions (A.6–A.12).
- `dialogs/extension_prompts.rs`: `install_dialog` (uses `unsupported_notice`), `request_dialog`.
- `dialogs/welcome.rs`: `install` (A.3.3).
- `keymap.rs` / `dialogs/shortcut_settings.rs` / `window/actions.rs`: commands (A.14).
- `window/mod.rs` / `window/ext_actions.rs` / `page_menu.rs`: actions, popups, menus (A.15, Part B).
- `app.rs`: `app.extension-notification` (A.11).
- `sync.rs`: `sync_applied` → `reconcile_extensions` (Part C.5).
- Self-test (`self_test.rs`, `docs/design/self-test.md`): `install_prompt` (install_asking, prompt lines, close = cancel, nothing committed, `install-prompt.png`), `install_crx` (install of `<out>/probe.crx`, `Verification::LocalCrx`, id = `testkit::PROBE_ID`), `engine_loaded_extension` (`runtime.loaded()` + `pending_filters() == 0`), `content_script`, `extension_port`, `dnr_blocked`, `dnr_site_allowed`, `dynamic_content_script`, `extension_windows`, `extension_web_navigation`, `extension_cookies`, `popup`, `extension_toolbar`, `context_menus`, `extension_commands`, `extension_notifications`, `extension_update` (uses `UPDATE_BUTTON` and `re_enable_text`), `private_window` (allowed in private), `welcome` (Install buttons), and with `--network` `cws_install` (real CWS install of uBlock Origin Lite `ddkjiahejlhfcafbddmgiahcphecmpfh`, progress via `InstallPhase::describe`, `Verification::ChromeWebStore`, loaded). The self-test sets `SCRIPTED`, which disables the periodic extension update loop.

## A.20 Install errors the user can see (core `InstallError` Display) [V]

"network: {msg}", "server returned HTTP {code}", "the store has no version of this extension for this browser", "unexpected store response: {msg}", "download larger than the {n} byte limit", "archive expands to more than {n} bytes", CRX errors (`CrxError`), "downloaded file does not match the store's sha256", "archive: {msg}", "archive entry with unsafe path {path:?}", manifest errors (`ManifestError`), "extension id {actual} does not match the expected {expected}", "only unpacked extensions can be reloaded", "extension {id} comes from a store or a signed package, and an unverified copy cannot take its id", "extension id {id} differs only in letter case from the installed {installed}", "{path} is not a valid Unicode path", I/O errors. Shown after "Install failed: ".

## A.21 Things that change user-visibly with Chromium (not in the GTK shell) [I]

- The Chrome Web Store page itself will offer "Add to Chrome" to a Chromium user agent. The GTK shell had no store-page install. The Windows shell intercepts `chrome.webstorePrivate` on the stores' pages (`crates/vsesvit-winui/src/store.rs`, `js/store.js`) and installs through core with the install prompt [V]. With CEF Chrome style, Chromium's own `webstorePrivate` may complete installs natively, bypassing core (no synced record, no core prompt, no core verification) unless intercepted. Open question for research.
- `chrome://extensions` may be reachable in Chrome-style browsers and let users load/remove extensions outside core. Open question whether to block (e.g. `CefRequestHandler::OnBeforeBrowse` refusing `chrome://extensions`) or reflect.

---

# Part B. Extension menu items (`extension_menus.rs`)

## B.1 What it renders [V]

Input: `vsesvit_webext::menus::Entry` trees (pure data from the runtime):

- `Entry::Item { id: ItemId, title, enabled, checked: Option<bool> }` (`checked` is `Some` for checkbox or radio items),
- `Entry::Submenu { title, enabled, children }` (an item with children, or the extension's own submenu grouping several items under its name),
- `Entry::Separator`.

`ItemId` = `Int(i64) | Str(String)`. `Choose = Rc<dyn Fn(&ItemId)>`, what choosing an item does.

Rules (`command(entry, name, choose)`):

- `label(title)`: `_` → `__` so GTK does not read mnemonics (GTK-ONLY detail; the GPUI menu must show the title literally).
- An item becomes an action: stateful with a boolean state when `checked` is `Some` (shown as a check; GTK and WebKit show radio items with a check mark too, WEBKIT/GTK-ONLY limitation), plain otherwise; `set_enabled(enabled)`; activation calls `choose(&id)`.
- A **disabled submenu** is shown as a greyed item that never opens.
- An enabled submenu opens its children; a separator separates.
- Chrome's limits (`ACTION_MENU_TOP_LEVEL_LIMIT = 6`, title 75 chars, selection 50 chars for `%s`) are applied by webext before rendering [V `menus.rs` constants].

## B.2 Page context menu [V] `page_menu.rs::add_extension_items`

- `runtime.page_menu(&target)` with `Target { page_url, frame_url, link_url, src_url, media: Image|Video|Audio, selection, editable }`, filtered to extensions that `runs_in(tab.browsing())` (in a private tab only extensions allowed there).
- Nothing found → nothing added. Otherwise: a separator, then each `(extension, entry)` as `page_item(entry, choose, &mut n)` (actions named `extension-item-{n}`, numbered because WebKit keeps one menu's actions by name). `choose` = `runtime.menu_clicked(&extension, item, Some(tab_id), Some(&target))`.
- Placement: above "Inspect Element" and the separator above it, or appended at the end when there is no Inspect Element.
- Self-test `context_menus`: a menu `[Reload, -, InspectElement]` becomes `[Reload, -, "Vsesvit Probe page item", -, InspectElement]` for a page click; a link click gets none of the probe's items (its item is `contexts: ["page"]`); choosing it fires `contextMenus.onClicked` with `pageUrl` = the page and `tab` = the tab id.
- (KEEP) contents, order and filtering. (NEEDS-CEF) In Chrome style, Chromium builds the page context menu itself, including extensions' items with command ids from `IDC_EXTENSIONS_CONTEXT_CUSTOM_FIRST` (49000) [V: constant present; I: that CEF's menu model carries them]. A GPUI-rendered menu would come from `ImplContextMenuHandler::run_context_menu(browser, frame, params, model, callback)` walking the `CefMenuModel` and answering `CefRunContextMenuCallback::Continue(command_id, event_flags)` (or `Cancel`); the shell's own items (Search for selection, Open link in private window, Copy link without tracking, Print, View Page Source, Inspect) are added in `on_before_context_menu` [I].

## B.3 Toolbar action menu [V] `window/ext_actions.rs::action_menu`

- Right-click on a pinned action button opens a popover menu at the click point, without arrow:
  1. The extension's own action items: `runtime.action_menu(id)` rendered by `extension_menus::model(entries, group, "extension-menu", choose)` as sections split at separators (actions `item-{k}` in a `gio::SimpleActionGroup` inserted as `extension-menu`); `choose` = `window.extension_menu_item_chosen(id, item)` = `runtime.menu_clicked(id, item, selected tab, None)` (no target: `pageUrl` null).
  2. A section: "_Unpin" (`win.extension-pin ('<id>', false)`) and "_Manage Extensions" (`win.show-extensions`).
- Self-test `context_menus`: the probe button's menu starts with "Vsesvit Probe action item", which fires `onClicked` with the selected tab and no page URL; `extension_toolbar` captures `extensions-context-menu.png`.
- (KEEP). (NEEDS-CEF) reading an extension's `contexts: ["action"]` items and dispatching `onClicked` has no known CEF API [I] (Windows has the same gap with WebView2: its toolbar button menu shows none of the extension's items, PLAN [V]).

---

# Part C. Sync (`sync.rs`) and its surfaces

## C.1 What `Syncer` is [V]

Module doc: sync with a Vsesvit sync server (`vsesvit-sync`). The browser owns one `Syncer`: it signs the profile in and out, and syncs 10 s after the first window opens, then every minute, soon after a local change, when asked, and once more as the browser quits. What touches the profile runs on the UI thread; network steps run on worker threads. Settings watches the `State`, and what it asks of the sync passphrase is prompted over the browser window. Signing in names and pictures the profile after the account, as Chrome does.

Constants [V]:

| Name | Value | Meaning |
|---|---|---|
| `FIRST_SYNC_DELAY_SECS` | 10 | first sync after the first window, when signed in at startup |
| `TICK_SECS` | 2 | how often the scheduler looks |
| `SYNC_INTERVAL` | 60 s | a sync starts this long after the last start even with nothing changed here |
| `SYNC_SOON` | 10 s | a local change syncs no sooner than this after the last start |
| `FINAL_SYNC_WAIT` | 3 s | quitting waits this long for the final round |
| `RUNNING_WAIT` | 120 s | deleting server data or a passphrase step waits this long for a running sync |
| `vsesvit_sync::MAX_ROUNDS` | 50 | rounds per sync; the next sync takes the rest |

Shell-side fields of `Inner` (all memory only) [V]: `browser: Weak<browser::Inner>`; `http: vsesvit_sync::Http` (ureq agent, UA `vsesvit/<version>`, connect 20 s, response 60 s, body 120 s); `state: RefCell<State>`; `running: Cell<bool>` (a sync, a passphrase step, or deletion running; kept apart from `State::SignedIn.syncing` because a round can outlive its sign-in); `last_start: Option<Instant>`; `start_seq: Option<Seq>` (`Profile::change_seq` at the last start; `None` asks for a sync at the next tick); `synced_seq: Option<Seq>` (at the start of the last completed sync); `attempt: u64` (bumped by sign-in and cancel); `details: u64` (bumped when the profile takes account details and by sign-out); `canceller: Option<Arc<AtomicBool>>`; `watchers: Vec<Box<dyn Fn(&State) -> bool>>`; `window_seen: bool`; `timer: Repeating`; `prompting: bool`; `welcomes: u32`.

## C.2 State model (`vsesvit_sync::status`) [V]

```
State::SignedOut { error: Option<String> }        // why the last sign-in failed
State::SigningIn                                   // server's sign-in page is open in a tab
State::SignedIn { name, server, last_synced: Option<u64 secs>, syncing, error, needs_sign_in, encryption }
Encryption = Checking | Off | Enter | Changed | Ready
```

`State::status(now_secs) -> Status { title, subtitle, actions: Vec<Action>, busy, server_editable }` (both shells' words):

| State | title | subtitle | actions (first is suggested) | server_editable |
|---|---|---|---|---|
| SignedOut | "Not signed in" | error, else "Sign in to sync bookmarks, history, open tabs, extensions and settings across your devices." | SignIn | yes |
| SigningIn | "Signing in…" | "Finish signing in on the page that opened." | Cancel | no |
| SignedIn | "Signed in as {name}" or "Signed in to {host}" | needs_sign_in: "Sign-in failed: {error}" or "Sign in again to keep syncing."; else error (not while syncing): "Sync failed: {error}"; else Checking: "Checking for a sync passphrase…", Enter: "Enter your sync passphrase to sync on this device.", Changed: "Your sync passphrase was changed on another device. Enter the new one to keep syncing."; else syncing: "Syncing…"; else "Last synced {ago}" / "Not synced yet", plus for Off: ". Not encrypted: whoever runs the sync server can read your synced data." | needs_sign_in: SignIn, SignOut. Checking: SignOut, DeleteServerData. Off: SyncNow, SetPassphrase, SignOut, DeleteServerData. Enter/Changed: EnterPassphrase, SignOut, DeleteServerData. Ready: SyncNow, ChangePassphrase, SignOut, DeleteServerData | no |

`busy = syncing` (Sync Now shown but insensitive). `ago(secs)`: "just now" (<60 s), "N minute(s) ago", "N hour(s) ago", "N day(s) ago". Action labels: "Sign In", "Cancel", "Encrypt with a Passphrase…", "Enter Passphrase…", "Sync Now", "Change Passphrase…", "Sign Out", "Delete Data on Server…".

Shell helpers [V]: `stored_state(profile, error, after_sign_in)` = `Account::load(profile.sync())`: `Some(account)` → `SignedIn` built from the account (`name`, `server`, `last_synced`, `encryption`), `syncing=false`, `error`, `needs_sign_in = after_sign_in`; `None` → `SignedOut { error }`; load error (e.g. locked keyring) → log and `SignedOut { error: Some(e) }`. `encryption(state)` = `Some(encryption)` only when `SignedIn` and `!needs_sign_in`. `should_sync(state, running)` = `!running && SignedIn { needs_sign_in: false, syncing: false }`. `due(...)` = `should_sync && (first of run || since_start >= 60 s || (changed && since_start >= 10 s))`. `refresh(state, now_encryption)` updates `encryption` and reports change. `settle(state, result, synced_at)` (below).

Watchers: `Syncer::watch(show)` calls `show(&state)` now and after every change until it returns false (widgets hold themselves weakly and return false when gone). `notify()` runs watchers then `prompt_later()`.

## C.3 Construction and the first window [V]

- `Syncer::new(weak browser, profile)` in `Browser::new`: state = `stored_state(profile, None, false)` (reads the account at startup; may unseal secrets, which on Linux may wait on a Secret Service unlock prompt on first use of the vault key).
- `window_opened()` (called by every `BrowserWindow::new`; acts on the first only): if signed in → `schedule(10)` (first tick 10 s, then every 2 s); then `prompt_later()`.

## C.4 Scheduling and a sync [V]

- `Repeating::start(first, every, owner, run)`: a one-shot GLib timeout after `first` seconds that arms a repeating timeout every `every` seconds **before** the first run (so a `cancel` inside `run` sticks), then runs; `cancel()` removes; `is_armed()`. Shared with the updater. (KEEP semantics; GTK-ONLY mechanism.)
- `tick()` every 2 s: `changed = profile.change_seq() != start_seq`; `since_start = last_start.elapsed()`; if `due(...)` → `sync_now()`.
- `sync_now()`: if `!should_sync` return; `start_seq = change_seq()`, `last_start = now`, `running = true`, state `syncing = true` (notify); spawn local future: `rounds(&mut synced_at)`; on `Ok` set `synced_seq = start_seq`, on `Err` log "sync: {e}"; `running = false`; `settle(state, result, synced_at)` (notify).
- `rounds()`: for n in 1..=50: browser gone → `Err(SignedOut)`; `gather(profile)` on the UI thread (`Account::load` + `Round::gather(store, account, &prefs[sync.types])`, `None` when signed out → `Err(SignedOut)`); `round.run(&http)` on a worker; `exchanged.finish(&mut profile.sync())` on the UI thread (applies the downloaded page, moves cursors, saves the account; `Err(SignedOut)` when the profile signed out/in or took other keys meanwhile); unless `SignedOut`, `show_encryption(finished.account.encryption())`; `Ok(synced)` → `synced_at = account.last_synced()`, `browser.sync_applied(&synced.report.changed)`, `synced.outcome(n)` → stop with its result (a refused upload waits for a later sync and is what this one comes to; 50 rounds → stop, next sync continues); `Err(SignedOut)` → next round (maybe a new account); other `Err` → return it. Worker panics re-raise on the UI thread (`on_worker` → `resume_unwind`).
- `settle`: only for `SignedIn`: `syncing = false`; `last_synced = synced_at` if any; `Ok` → `error = None`; `SignedOut` → nothing; `e.needs_sign_in()` (`SignInExpired`, "the sync session ended; sign in again") → `needs_sign_in = true`, `error = None`; other → `error = Some(e.to_string())`. A profile that signed out meanwhile keeps the sign-out's state.
- `types_changed()` (Settings > Customize Sync): `start_seq = None`, `sync_now()` (or at a tick once the running sync ends).
- Sync errors the user can see (vsesvit_sync::Error): "{0} is not a valid server address", "could not reach {host}", "the server answered {status}: {message}", "the server speaks sync protocol {n}, and this version of Vsesvit speaks {P}", "unexpected answer from {0}", "could not open a local port to finish signing in: {e}", "the sign-in was refused: {reason}", "the sync session ended; sign in again", "the sign-in was cancelled", "signed out while syncing", "the sign-in took too long", "the passphrase is wrong", "that passphrase opens a key this device never had; sign out and sign in again to use it", "the sync server's key record is damaged, or from a newer version of Vsesvit", "the sync passphrase changed meanwhile; try again", profile errors.

(KEEP all; no engine involvement.)

## C.5 `Browser::sync_applied(&Changed)` fan-out [V] `browser.rs`

Called after every round that applied remote records. `Changed { bookmarks, history, sessions, extensions, ext_storage: Vec<(ExtensionId, Vec<StorageChange>)>, prefs: Vec<String>, search_engines, site_permissions }`:

- `bookmarks` → `bookmarks_changed()` (bars, menus, Bookmarks window).
- `history || sessions` → refresh History windows ("Tabs from other devices").
- any pref or `search_engines` → open Settings controls re-read their values.
- `extensions` → `reconcile_extensions()` (A.3.4).
- prefs contain `extensions.allowed_in_private` → `runtime.allowed_in_private_changed()`.
- each `ext_storage` entry → `runtime.storage_sync_changed(ext, changes)` (NEEDS-CEF or dropped).
- `site_permissions` → `permissions::enforce`; with `tracking.protection` pref → trackers re-applied; with `cookies.third_party` pref → cookie rules re-applied.
- `keyboard.shortcuts` → `apply_keymap()`.
- any pref → theme, engine prefs (`engine.apply_prefs`), every window `apply_prefs()` and `refresh_extension_actions()` (e.g. `toolbar.extensions`).

(KEEP; each target is in another module's inventory. Engine-touching ones are NEEDS-CEF there.)

## C.6 Sign-in flow (tab + loopback port) [V]

1. Trigger: Settings > Sync "Sign In", or the welcome's Sync page "Sign In" (which then advances the welcome with its default widget, "Next"), or "Sign In" again for an expired sign-in (`needs_sign_in`). `act(Action::SignIn)` → `sign_in()`.
2. `sign_in()`: read local pref `sync.server` (default `https://vsesvit-service.mrquantumoff.dev`); `attempt += 1`; state `SigningIn` (Settings shows "Signing in…", only "Cancel"; server field insensitive).
3. `authorize(attempt, server)`: on a worker, `SignIn::start(&http, &server)`: normalize the server (HTTPS, or HTTP only on `localhost`/`127.0.0.1`/`[::1]`; no query/fragment/userinfo), GET server info and check the protocol version, bind `127.0.0.1:0`, make `redirect_uri = http://127.0.0.1:<port>/callback`, a PKCE verifier and state (256 random bits base64url each), the authorize URL `{server}{AUTHORIZE_PATH}?response_type=code&redirect_uri=..&state=..&code_challenge=<S256>&code_challenge_method=S256`.
4. If the attempt is still current: `browser.normal_window()` (most recently focused normal window, else a new normal window with a blank tab; never a private window) → `open_tab(Some(authorize_url), None, Focus::Foreground)` (a new foreground tab, a normal page load) → `window.present()`. Store `pending.canceller()`.
5. On a worker, `pending.finish(&http)`: polls the listener (100 ms sleeps, cancel checked each loop) up to 15 minutes ("the sign-in took too long"); each connection: reads one request line (≤ 8 KiB, ≤ 5 s); path ≠ `/callback` → 404 "Not found." and keep waiting; `state` mismatch → 400 "This sign-in link is not the one Vsesvit is waiting for." and keep waiting; `error` param → 200 "Sign-in was not completed. You can close this tab." and `Err(Refused(error_description or error))`; `code` → 200 "Vsesvit is signing you in to sync. You can close this tab." Responses are a small HTML page titled "Vsesvit sync" (system-ui font, 32 em wide). Then POST `{server}{TOKEN_PATH}` form (`grant_type=authorization_code`, `code`, `redirect_uri`, `code_verifier`); 400 → `Refused(description)`; parse `TokenResponse`; `AccountDetails::from_claims(claims)`; returns `SignedIn { account: Account::new(server, name, access_token, limits), details }`.
6. Back on the UI thread: if `attempt` changed meanwhile (cancelled or restarted) → drop the result. Clear canceller. `account.save_signed_in(&mut profile.sync())` (replaces any stored account; clears protocol-1 tokens).
   - Success → state `SignedIn` from the account (syncing = whether a round still runs), `sync_now()`, `schedule(TICK_SECS)` (first tick in 2 s), `take_account_details(details)`.
   - Failure → log "sync sign-in: {e}"; error text unless `Cancelled`; state = `stored_state(profile, error, after_sign_in = true)`: with an account still stored (signing in again after expiry) it stays `SignedIn` with `needs_sign_in` and "Sign-in failed: {error}"; else `SignedOut { error }`.
7. The sign-in tab is **not** closed automatically; the loopback page tells the user they can close it.

Classification: the flow is KEEP. Engine-touching points (NEEDS-CEF, verify):
- The tab must load `http://127.0.0.1:<port>/callback` from an HTTPS provider page redirect. Core's HTTPS-only already exempts loopback (PLAN "HTTPS-only follows Chrome's HTTPS-First mode": localhost and private addresses load as asked) [V]. Chromium's own HTTPS-Upgrades/HTTPS-First must not upgrade it either [I: Chromium exempts localhost].
- Chromium 154 has Local/Loopback Network Access permissions (`CEF_PERMISSION_TYPE_LOOPBACK_NETWORK` [V]). A public page redirecting the top-level tab to loopback may trigger a permission prompt or block [I, needs verification]; if so, `ImplPermissionHandler::on_show_permission_prompt` must handle it (allow the sign-in's own callback).
- Identity providers that refuse embedded browsers (e.g. Google's "This browser or app may not be secure") may treat a CEF-based browser as embedded [I]; the sync server's provider is operator-chosen.

## C.7 Cancel, sign-out, expired sign-in [V]

- `cancel()` (Settings "Cancel" while signing in): `attempt += 1`, set the canceller (the worker returns `Cancelled` within ~100 ms), state = `stored_state(profile, None, true)` (no error shown for a cancel).
- `sign_out()` (Settings "Sign Out", or the passphrase dialog's "Sign Out"): `Account::load` then `forget(browser, account)`: `Account::forget(profile.sync())` (clears the account JSON and the sealed session/keyring/old tokens; works with the keyring locked), `details += 1` (drops a picture still on its way), `cancel_timer()`, state `SignedOut { error: None }`, and if an account was loaded, `account.revoke(&http)` on a plain `std::thread` (DELETE `{server}{SESSION_PATH}` with the bearer token, best effort). A round still running is dropped by `Exchanged::finish`, which sees the account gone. Errors logged "sync sign-out: {e}". Signing out keeps the profile's name/picture (Chrome does too) [V PLAN].
- Expired session: a round's `SignInExpired` (server 401) → `needs_sign_in`; syncing stops (`should_sync` false), no passphrase prompts (encryption() is None), Settings offers Sign In and Sign Out.

## C.8 Account details → profile name and picture [V]

`take_account_details(details: AccountDetails { name: Option<String>, picture: Option<Url> })`:

1. `details += 1` (latest); requires a `Home` (profile in the profile list; a `--profile-dir` outside it does nothing).
2. `home.dir.take_account_details(&home.id, details.name, None)` (core: if the entry is set by hand, change nothing; otherwise settle `set_by_hand = false`, take the name) → `browser.set_profiles(registry)` (windows' profile buttons, titles, Manage Profiles refresh). Error: log "the profile list: {e}".
3. If the entry is set by hand, or there is no picture URL, stop. Else on a worker `AccountPicture::fetch(&url)` (HTTPS, public addresses only, ≤ 1 MiB, decode PNG/JPEG/GIF/WebP/BMP, centre square scaled to 144 px PNG; `None` on any failure); when it arrives, if `details` changed (a newer sign-in or sign-out), drop it; else `take_account_details(&home.id, None, Some(&picture))` (writes `Account Picture <sha256 prefix>.png` in the profile dir and the list entry) → `set_profiles`.

Name choice (core `AccountDetails::from_claims`): given name, else full name, else preferred username, else the email's local part; whitespace collapsed, control chars dropped, capped. Self-test `profiles`: a sign-in whose picture is still on its way followed by one without a picture shows no picture 1.5 s later; signing in as Alex with `/allowed.png` names the profile "Alex" and shows the picture (`profiles-account.png`); a rename by hand while a slow picture is on its way wins (the picture neither lands nor brings back the old name); a later sign-in as Sam changes neither name nor picture. (KEEP)

## C.9 Sync passphrase [V] `sync.rs` prompting + `dialogs/passphrase.rs` + `vsesvit_sync::status`

### C.9.1 Prompts sync shows of its own accord

- `prompt_later()` defers `prompt(false)` to idle, so a welcome a new window opens comes first. Called after every state change (`notify`), on the first window, after a welcome closes, and after each prompt ends.
- `hold_prompts(welcome)` (the welcome dialog calls it): `welcomes += 1`; when that dialog closes, `welcomes -= 1` and `prompt_later()`.
- `prompt(asked)`: `offered = pref sync.passphrase_offered`; `welcome = !asked && welcomes > 0`; `prompt_due = status::prompt(state, offered)` filtered by `!prompting && !welcome`. `status::prompt`: `SignedIn { needs_sign_in: false, encryption: Off }` and not offered → `Prompt::Offer`; `SignedIn { needs_sign_in: false, encryption: Enter | Changed }` → `Prompt::Enter`; else none. The window: the most recently focused **normal** window (never private); none open → no prompt (retried at the next notify). Sets `prompting`, runs `passphrase::prompt(window, syncer, prompt)` to its end (errors logged "sync passphrase: {e}"), clears `prompting`, then `prompt(false)` again (the next one).
- `act(Action::EnterPassphrase)` (Settings' "Enter Passphrase…") = `prompt(true)`: shows the due prompt even while a welcome is open (still one at a time). Note: it is shown over the active normal window, not over the Settings dialog [V].

`passphrase::prompt(window, syncer, prompt)`:

- `Prompt::Enter` → `take(window, syncer, no busy indicator)`.
- `Prompt::Offer` → alert with `OFFER` = title "Encrypt Your Synced Data?", body "Your bookmarks, history, open tabs, extensions and settings sync unencrypted, so whoever runs the sync server can read them. Set a sync passphrase and Vsesvit encrypts them before they leave your devices. You can also do this later in Settings > Sync.", responses "Not Now" (`decline`, close response) and "Set Passphrase" (`accept`, suggested, default). It closes itself unanswered when the state no longer offers (e.g. signed out). Any answer (including Escape) sets local pref `sync.passphrase_offered = true` (offered once per profile/device); `accept` → `take(...)`.

### C.9.2 Settings' passphrase buttons

- "Encrypt with a Passphrase…" (`SetPassphrase`) and "Change Passphrase…" (`ChangePassphrase`) → `ask_for_passphrase(row, spinner, syncer)` → `passphrase::take(&row, syncer, set_busy)` where busy makes the account row insensitive and shows its spinner; the dialog is parented to the Settings row (so it appears over Settings). Failure → Settings toast "Could not use the sync passphrase: {e}".
- "Enter Passphrase…" → `act(EnterPassphrase)` (C.9.1).

### C.9.3 `take(parent, syncer, busy)` and the dialog

- `encryption = syncer.encryption()` and `words = status::passphrase_dialog(encryption)`; none (Checking, needs sign-in, signed out) → `Ok(())`.
- Loop while the account still asks the same `encryption`: `ask(...)`:
  - `Typed(passphrase)` → `busy(true)`, `syncer.passphrase(passphrase).await`, `busy(false)`; `Ok` → done; `Err(e)` → ask again with `failed = e` shown under the fields.
  - `Dismissed` with dismiss label "Sign Out" → `syncer.act(SignOut)`, done.
  - `Dismissed` / `Gone` → done.
- Dialog words (`passphrase_dialog`):

| Encryption | title | body | field(s) | accept | dismiss |
|---|---|---|---|---|---|
| Off (set) | "Encrypt Sync with a Passphrase" | "Vsesvit encrypts your bookmarks, history, open tabs, extensions and settings with this passphrase before they leave your devices, so the sync server can't read them. Your other devices stop syncing until you enter it on each of them. If you forget it, it can't be recovered. What the server already holds unencrypted stays there until you delete your data on it, which is also the only way to turn encryption off." | "Passphrase", "Confirm passphrase" | "Set Passphrase" | "Cancel" |
| Enter | "Enter Your Sync Passphrase" | "Your synced data is encrypted with a sync passphrase. Enter it to sync on this device, or sign out to stop syncing here. If you've forgotten it, delete your data on the sync server in Settings > Sync." | "Passphrase" | "Start Syncing" | "Sign Out" |
| Changed | "Enter Your New Sync Passphrase" | "Your sync passphrase was changed on another device. Enter the new one to keep syncing, or sign out to stop syncing here. Never enter an old passphrase here." | "New passphrase" | "Start Syncing" | "Sign Out" |
| Ready (change) | "Change Your Sync Passphrase" | "Vsesvit encrypts your data again with the new passphrase. Your other devices stop syncing until you enter it on each of them. The sync server may keep copies it made earlier, which the old passphrase still opens." | "New passphrase", "Confirm new passphrase" | "Change Passphrase" | "Cancel" |

- `ask(...)` layout: alert dialog (title, body), wide layout preferred; extra child: a boxed list of password entry rows (each "activates default", so Enter accepts) and below it a wrapped error label (red), shown only when there is a problem. The previous failure (e.g. "the passphrase is wrong") is shown capitalized ("The passphrase is wrong") until something is typed. Focus starts in the first password field. Responses: dismiss and accept (accept suggested and default, **disabled until valid**).
- Validation on every keystroke (`check_passphrase(text, confirm)`): confirm differs → "The passphrases don't match"; shorter than 8 chars (`MIN_PASSPHRASE_CHARS`) → "Use at least 8 characters". While the confirmation is still empty, only the first field's length problem is shown. Accept enabled only when `check` passes.
- When dismiss is "Sign Out" (Enter/Changed): the dialog **cannot be closed** by Escape or the close button (`can_close = false`); only "Start Syncing" or "Sign Out" end it (responses force-close). Otherwise Escape = dismiss.
- `choose_while(dialog, parent, syncer, asks)`: if the state no longer asks, return `None` at once; watch the syncer and force-close (answer `None` = `Gone`) when the account no longer asks the same encryption (signed out elsewhere, entered on another path) or when the parent window unrealizes (closing window). Self-test unit test: both endings return `None`.

### C.9.4 The passphrase step (`Syncer::passphrase`) [V]

`hold_syncs()` (wait up to 120 s, polling every 100 ms, for `running` to clear, then set it; timeout → "a sync is still running; try again"); `take_passphrase`: load the account (`Err(SignedOut)` if none), `PassphraseJob::new(account, passphrase)` (`None` → `KeysChanged`), `job.run()` on a worker (Argon2id key derivation, no network), `keys.finish(&mut profile.sync())` → account → `encryption()`; clear `running`; on success `show_encryption(encryption)` and `sync_now()`. Set when the account has none (Off), enter (Enter/Changed), change when the device syncs (Ready).

Self-test `sync_passphrase`: a stored account on `http://127.0.0.1:9` with no key record → Settings > Sync shows Sync Now and "Encrypt with a Passphrase…" and a subtitle containing "Not encrypted"; the Offer comes by itself (`sync-passphrase-offer.png`), "Not Now" sets the pref and it does not come back after `Syncer::reload()`; the set dialog has fields "Passphrase", "Confirm passphrase", accept off when empty, mismatched or short (with a visible error), on when valid (`sync-passphrase.png`); accepting stores a `Ready` account and Settings shows "Change Passphrase…"; with `account.keyring` cleared, the Enter prompt comes by itself with "Sign Out" and "Start Syncing", does not close on Escape, a wrong passphrase re-asks with "The passphrase is wrong", the right one closes it leaving only Settings; again, "Sign Out" signs out and closes it.

## C.10 Delete data on the server [V]

- Settings > Sync last group: a destructive button row "Delete Data on Server…", visible only when `status.actions` contains `DeleteServerData` (signed in, not needing sign-in).
- Click → confirm alert `DELETE_CONFIRMATION`: heading "Delete your data on the sync server?", body "Your bookmarks, history, open tabs, extensions and settings are deleted from the server, along with your sync passphrase, and all your devices sign out. They stay on your devices, which upload them again, unencrypted until a passphrase is set, when they next sign in.", buttons "_Cancel" (default, close) and "Delete" (destructive).
- Accept → row insensitive → `syncer.delete_server_data()`: `hold_syncs()`; load account; on a worker `account.delete_server_data(&http)` (DELETE `{server}{ACCOUNT_PATH}`); then `forget` (sign out, revoke); clear `running`. Failure (or a sync still running after 120 s) leaves the profile signed in; Settings toast "Could not delete the data on the server: {e}". Row sensitive again.
- Unit test: deletion waits while `running` is set, then (no browser) errors, and clears `running`.

## C.11 Customize Sync and the server field [V] `dialogs/settings.rs`

- Group "Customize Sync", description "What this device syncs": switch "Sync Everything", then one switch per `DataType::ALL` with labels "Bookmarks", "History", "Open tabs", "Extensions", "Settings" (`DataType::kinds`: History = pages + deletions; Open tabs = sessions; Extensions = extensions + `storage.sync`; Settings = prefs + search engines + site permissions).
  - Shown: with every type chosen and not "customizing", Sync Everything is on and the type switches are on and insensitive. Turning Sync Everything off while all are chosen enters "customizing" (type switches become sensitive, still on). Turning a type switch writes `sync.types` (`DataType::toggled`, in `ALL` order) and calls `types_changed()`. Turning Sync Everything on again chooses all.
  - Pref `sync.types` (local, default all).
- Server group (no title): entry row "Sync Server" with apply button and URL input purpose, text = pref `sync.server`; sensitive only when `status.server_editable` (signed out). Typing clears the error. Apply: `vsesvit_sync::server_input(text)`: empty → reset the pref and show `DEFAULT_SYNC_SERVER`; valid → store the normalized URL and show it normalized; invalid → the group's description shows "{text} is not a valid server address" above the field and the row gets the error style. The welcome's Sync page shows the same row (`settings::sync_server_row`).

## C.12 Final sync on quit [V]

`Browser::shutdown()` (from `GApplication::shutdown`): save the session, mark shut down, (unless the profile was removed) clear exit-cleared site data, then `sync.final_sync()`: cancel the timer; if `change_seq != synced_seq` (something changed since the start of the last completed sync) and `should_sync`: gather one round on the UI thread, run it on a plain thread, wait at most 3 s (`recv_timeout`); on time, `finish` it and log "final sync: done" or "final sync: {e}" (including a refused upload); else log "final sync: no answer in 3s; quitting without it" (abandoning a round is safe). (KEEP; NEEDS-CEF only for ordering relative to CEF shutdown: the final sync touches only the profile and the network, so it can run before `CefShutdown` [I].)

## C.13 Settings > Sync page layout [V]

Page name `sync`, title "Sync", icon `view-refresh-symbolic` (Adwaita lacks a sync icon; the welcome uses the same). Groups, top to bottom:

1. (no title) The account row: title/subtitle from `state.status(now_secs())` (re-rendered only on state changes; the minute syncs keep "Last synced …" fresh), a hidden spinner suffix, then buttons in the fixed order Sign In, Cancel, Enter Passphrase…, Sync Now, Encrypt with a Passphrase…, Change Passphrase…, Sign Out; each visible iff in `status.actions`; Sync Now insensitive while `busy`; the first action of `status.actions` gets the suggested style. Buttons call `syncer.act(action)` except Set/Change (C.9.2). Delete has its own group. Unit test: every state lists its buttons in that fixed order.
2. "Customize Sync" (C.11).
3. (no title) "Sync Server" (C.11).
4. (no title) "Delete Data on Server…" (C.10).

## C.14 Welcome's Sync page and History [V]

- Welcome page "Sync your browser", description "Have your bookmarks, history, open tabs, extensions and settings on all your devices.": the server row; a pill "Sign In" button (visible only while the status offers Sign In) which signs in and moves to the next welcome page without waiting; a status label with `status.title` when not offering Sign In. Prompts are held while the welcome is open (C.9.1).
- History window: "Tabs from other devices" are listed only when `sync().is_signed_in()`; each device's age uses `vsesvit_sync::status::ago` ("Synced {ago}").

## C.15 Persistent state (sync) [V]

| Where | Key | Scope |
|---|---|---|
| pref | `sync.server` (default `https://vsesvit-service.mrquantumoff.dev`) | local |
| pref | `sync.types` (`Vec<DataType>`, default all) | local |
| pref | `sync.passphrase_offered` (bool, default false) | local |
| core sync store, engine state | `account` (Account JSON: sign_in, server, name, limits, cursors, last_synced, own_keys, server_keys, pending, converting, upload_keys, epoch, …) | local |
| core sealed secrets (`sync_secrets`, sealed by the profile vault key) | `account.session`, `account.keyring`, legacy `account.tokens` (cleared) | local |
| vault key | Secret Service item "Vsesvit Safe Storage" (Linux), or unprotected where there is none | local |
| `profiles/profiles.json` | the profile's name, `set_by_hand`, picture file name | per install |
| profile dir | `Account Picture <sha256 prefix>.png` | local |
| change seq | `Profile::change_seq()` (memory, from DB seqs) | |
| memory | everything in C.1's field list | process |

## C.16 Classification (sync)

Everything in Part C is KEEP and engine-free except: the sign-in tab load (NEEDS-CEF verification: loopback redirect, HTTPS upgrades, loopback-network permission, embedded-browser detection), the `sync_applied` targets that touch the engine (in other inventories), `runtime.storage_sync_changed` and `runtime.allowed_in_private_changed` (NEEDS-CEF, A.16), and the GTK mechanisms (`glib` timers, `gio::spawn_blocking`, `adw::AlertDialog`/`choose_future`, `connect_unrealize`, `idle_add_local_once`; GTK-ONLY). Dialog modality rules to keep: the Enter/Changed prompt is non-dismissable except by its two buttons; a prompt closes itself when moot or when its window closes; prompts never appear over private windows; prompts wait for the welcome.

## C.17 Interactions [V]

- `browser.rs`: owns the `Syncer` (`Browser::sync()`), `sync_applied`, `normal_window()`, `set_profiles`, `home()`, `shutdown()` → `final_sync`.
- `window/mod.rs`: `BrowserWindow::new` → `sync().window_opened()`.
- `dialogs/settings.rs`: `watch`, `act`, `types_changed`, `delete_server_data`, `passphrase` (via `passphrase::take`), `sync_server_row`.
- `dialogs/passphrase.rs`: `encryption()`, `state()`, `watch`, `passphrase`, `act(SignOut)`; `sync::encryption`.
- `dialogs/welcome.rs`: `hold_prompts`, `act(SignIn)`, `watch`, `sync_server_row`.
- `dialogs/history.rs`: `is_signed_in()`.
- Self-test: `sync_passphrase`, `profiles` (`take_account_details`), `settings` (renders the `sync` page), unit tests in `sync.rs` (scheduling `due`, `settle`, `refresh`, `prompt_due`, `stored_state`, deletion waiting, `Repeating` cancel-in-run) and `dialogs/passphrase.rs` (offer waits for the welcome and comes once; `choose_while` endings). `Syncer::reload()` exists only with the `self-test` feature (re-reads the stored account into the state).
- `scripts/sync-e2e.sh` runs `crates/vsesvit-sync/tests/e2e.rs` against the real server (engine-level, shell-independent).

---

# Part D. Self-updates (`updates/`)

## D.1 What `Updates` is [V]

Module doc: self-updates (`docs/design/packaging.md`). The application owns one `Lifecycle`: a timer, Settings and the banner's button feed it events; checks, downloads and installs run on a worker thread whose results come back to the UI thread as more events.

`Updates::new(app, automatic, channel) -> Option<Updates>` (called in `Browser::new` with local prefs `updates.automatic` and `updates.channel` read once):

- `Installation::detect()`: reads the one-line marker `package-format` next to the **canonicalized** `current_exe()` (on Linux `<prefix>/bin/vsesvit` is a symlink to `<prefix>/lib/vsesvit/vsesvit`, where the marker is) and `$APPIMAGE`. Values: `deb`, `rpm`, `pacman`, `appimage` (needs non-empty `$APPIMAGE`, else Unpackaged), `flatpak`, `nsis`; none/unknown → `Unpackaged`.
- If `installation.self_updates()` (has a format: deb, rpm, pacman, AppImage on Linux): `Config::builtin()` (compiled-in `packaging/updater.json`, overridden at build time by `VSESVIT_UPDATER_PUBKEY` / `VSESVIT_UPDATER_ENDPOINTS`) and `Updater::new(config, current_version, installation)` (fails `Disabled(NoPublicKey | InvalidPublicKey | NoEndpoints | InvalidConfig)`); else `Err(Disabled(NotSelfUpdating))`.
- `program`: AppImage → `$APPIMAGE`; else `current_exe()` resolved **now** (once a package manager replaces the executable, `current_exe()` names the deleted file).
- Any error → log at info level and return `None`: **no Updates object, no banner, no Updates group in Settings** (unpackaged builds, Flatpak, or a build without an updater key).
- Otherwise builds `Lifecycle::new(current_version, background_install = (format == AppImage))`, `dir = glib::user_cache_dir()/vsesvit/updates` (i.e. `$XDG_CACHE_HOME/vsesvit/updates`, default `~/.cache/vsesvit/updates`), logs "automatic updates are off" if so, and registers the application action `app.update` (→ `Event::Activate`).

`current_version()` = `CARGO_PKG_VERSION` as semver.

## D.2 Lifecycle state machine [V] `updates/lifecycle.rs`

States: `Idle`, `Checking`, `UpToDate`, `Downloading { version }`, `Rechecking` (user picked another channel while a check/download for the old one ran: its result is dropped and the new channel's check starts when it ends; one worker at a time), `Ready { version, downloaded }` (waiting for "Update", also after a failed install), `Installing { version }`, `Installed { version, next: Installed }`, `Failed(String)`, `Disabled(DisabledReason)`.

Events: `Check` (timer or user), `ChannelChanged`, `Activate` (banner/Settings button), `UpToDate`, `Found(Version)`, `Downloaded(D)`, `Installed(Installed)`, `InstallFailed { error, downloaded }`, `Failed(Error)`.

Effects: `Check`, `Install(D)`, `Restart` (quit, then start the new version), `Exit` (quit; the installer starts the new version), `Notify(String)` (an install the user asked for failed).

Transitions (`step`), first match wins:

| From | Event | To | Effect |
|---|---|---|---|
| any | `Failed(Disabled(reason))` | `Disabled(reason)` (terminal) | — |
| Idle, UpToDate, Failed | Check | Checking | Check |
| Idle, UpToDate, Failed, Ready | ChannelChanged | Checking (a Ready download for the old channel is dropped) | Check |
| Checking, Downloading, Rechecking | ChannelChanged | Rechecking | — |
| Rechecking | UpToDate, Downloaded, Failed | Checking | Check |
| Checking | UpToDate | UpToDate | — |
| Checking | Found(v) | Downloading{v} | — |
| Downloading{v} | Downloaded(d), background (AppImage) | Installing{v} | Install(d) |
| Downloading{v} | Downloaded(d) | Ready{v,d} | — |
| Ready{v,d} | Activate | Installing{v} | Install(d) |
| Installing{v} | Installed(next) | Installed{v,next} | Exit if `next == ExitNow` |
| Installing{v} | InstallFailed{e,d}, not background | Ready{v,d} | Notify(e) |
| Installing | InstallFailed (background) | Idle (log "updating failed"; the next check finds the verified file on disk) | — |
| Checking, Downloading | Failed(e) | Failed(e.to_string()) (log) | — |
| Installing | Failed(e) (worker failed to start) | Idle (log) | — |
| Installed{Relaunch or NextLaunch} | Activate | same | Restart |
| anything else | any | unchanged | — |

So: one check at a time; no checks while an update waits, installs, or after install; the Ready retry reinstalls the same file without checking or downloading; an install already started is past a channel change.

`can_check()` = Idle, UpToDate or Failed.

## D.3 Status (Settings) and banner (every window) [V]

`status()` → `Status { title, button }` with `StatusButton::Check { enabled: can_check() }` or `StatusButton::Update(label)`:

| State | title | button |
|---|---|---|
| Idle | "Vsesvit {current}" | Check (enabled) |
| Checking, Rechecking | "Checking for updates…" | Check (disabled) |
| UpToDate | "Vsesvit is up to date" | Check (enabled) |
| Downloading{v} | "Downloading Vsesvit {v}…" | Check (disabled) |
| Ready{v} | "Vsesvit {v} is ready to install" | Update("Update") |
| Installing{v} | "Installing Vsesvit {v}…" | Check (disabled) |
| Installed{v, NextLaunch} | "Vsesvit {v} will open next time" | Update("Restart now") |
| Installed{Relaunch} | "Restart to finish updating" | Update("Restart") |
| Installed{v, ExitNow} | "Vsesvit {v} is installed" | Check (disabled) |
| Failed(msg) | "Could not check for updates: {msg}" (e.g. "update server returned HTTP 503", "network: reset") | Check (enabled) |
| Disabled(reason) | "Updates are off: {reason}" | Check (disabled) |

`banner()` → `Some(Banner { title: status.title, button })` only while the user has something to do or wait for: `Ready`; `Installing` unless background (AppImage installs quietly); `Installed` unless `ExitNow`. Button = the status's Update label, else none.

UI [V]:

- **Banner**: an `adw::Banner` added as a top bar of every browser window, directly under the header bar and above the tab bar, bound to `app.update`; `set_update_banner(Some(b))` sets title and button label and reveals it; `None` hides it. Each `dispatch` updates every window; `window_opened(window)` sets it for a new window.
- **Settings > General > "Updates"** group (only when `browser.updates()` is `Some`):
  1. Status row: title = `status.title`, subtitle "Version {CARGO_PKG_VERSION}"; suffix "Check for Updates" (→ `updates.check()`, sensitive per `Check { enabled }`) and a second button bound to `app.update` labelled with the Update label; exactly one of the two is visible (the Update one when `StatusButton::Update`).
  2. Choice row "Update Channel", subtitle "Vsesvit moves to a steadier channel once that channel has a version newer than this one.", options Stable, Beta, Weekly, Nightly (`UpdateChannel`), → `Browser::set_updates_channel` (writes `updates.channel`, then `updates.set_channel(channel)` → `Event::ChannelChanged`: checks at once even with automatic updates off).
  3. Switch row "Automatic Updates", subtitle "Download new versions of Vsesvit in the background", pref `updates.automatic` → `Browser::set_updates_automatic` (writes the pref, then `updates.set_automatic`).
- **Toast** for `Effect::Notify(message)`: first ASCII letter upper-cased, shown in the active browser window or else the first one, e.g. "Installing the update failed: administrator authorization was not given".

## D.4 Timer, automatic, channel [V]

- `FIRST_CHECK_DELAY_SECS = 30`, `CHECK_INTERVAL_SECS = 86400`.
- `window_opened(window)`: sets the banner; on the **first** window, if automatic, `schedule()` = `timer.start(30, 86400, …, dispatch(Check))`.
- `set_automatic(false)` cancels the timer; `set_automatic(true)` re-arms it if a window was seen and the timer is not armed.
- `check()` (Settings button) dispatches `Check` even with automatic off.
- Entering `Disabled` logs `Error::Disabled(reason)` and cancels the timer.
- Status watchers (`watch_status`) are called after every step until they return false.

## D.5 Worker work [V]

`spawn(work)`: a named thread `vsesvit-update`; each event it sends goes over an unbounded channel and is dispatched on the UI thread (while the `Updates` lives). A failure to start the thread dispatches `Failed(Io)`.

- `Effect::Check` → `check_and_download(updater, channel, dir, send)`:
  1. `create_dir_all(dir)`;
  2. `updater.check(channel.name())` (`stable|beta|weekly|nightly`): each endpoint in order with `{{channel}}`, `{{current_version}}`, `{{target}}` = `linux`, `{{arch}}`, `{{bundle_type}}` = variant; `204`/`404` = none; other statuses/network errors → next endpoint; dynamic or static manifest; offered only if newer and its channel is at least as steady as asked; `Available::Update(Update)` or `NotInstallable(Release)`;
  3. `remove_stale_downloads(dir, available version)` (deletes other versions' artifacts and partials, keeps this version's partials to resume);
  4. none → `UpToDate`;
  5. `available.into_update()` (`NotInstallable` → `Disabled(NotSelfUpdating)` → terminal);
  6. `Found(version)`;
  7. `update.download(dir, |_, _| {})` (no progress shown; resumes partial files; verifies the minisign signature, the signed `version:` in the trusted comment, and magic bytes; cap 512 MiB);
  8. `Downloaded(downloaded)`. Any error → `Failed(e)`.
- `Effect::Install(downloaded)` → `downloaded.install(&installation, &[])` → `Installed(next)` or `InstallFailed { error, downloaded }` (the verified download comes back for the retry). Linux:
  - deb: `pkexec apt-get install -y --no-install-recommends <absolute path>`; rpm: `pkexec rpm -U <path>`; pacman: `pkexec pacman -U --noconfirm <path>`. Exit 0 → delete the file, `Installed::Relaunch`. pkexec 126/127 → "administrator authorization was not given". Other → "{program} failed ({status})". pkexec missing → "could not run pkexec: {e}". The polkit agent's password dialog is the system's, not the browser's.
  - AppImage: write the verified bytes to `.<name>.vsesvit-update` next to `$APPIMAGE`, copy its permissions, fsync, rename over the image, fsync the dir, delete the download → `Installed::NextLaunch`. Errors "replacing {image}: {e}".
  - NSIS only on Windows (`ExitNow`).
- `Effect::Restart` → set `restart = true`, `app.quit()`. `Effect::Exit` → `app.quit()` (Windows NSIS path; not reached on Linux).

## D.6 Restart mechanics [V] `app.rs`

After `app.run_with_args_os` returns: if the profile was removed, release it and sweep; else if `browser.restart_program()` is `Some(program)` (`restart` set): `release_profile(app, slot)` (drop the `Browser`; force-close every visible dialog; destroy every window; iterate the GLib main loop for up to 2 s for teardown; warn "the profile is still open; the restarted browser may find it locked" if the profile `Rc` is still alive), then `Command::new(program).args(location.relaunch_args()).spawn()` where `relaunch_args` = `["--profile-dir", <profile root>]`; log "restarted into {program}" or "could not start {program}: {e}". The session was saved at shutdown, so the restart restores it.

(KEEP. NEEDS-CEF [I]: the new process must find both locks free: core's `LOCK` file in the profile dir and Chromium's `SingletonLock` in CEF's cache/root cache path; so the spawn must come after `CefShutdown` completes. CEF sub-processes (`--type=renderer|gpu-process|utility|zygote`) re-execute the binary; they must not detect an installation, read prefs or run the updater, and `restart_program` must point at the browser executable, not a helper.)

## D.7 Headless CLI [V] `updates/headless.rs`, `lib.rs`, `cli.rs`

- `vsesvit --check-for-updates`: `cli::print(cli::check(&Installation::detect(), &current_version(), UpdateChannel::of_build().name()))` → one JSON line `{"installation": "<variant>|unpackaged", "current": "...", "available": null | {"version", "notes", "pub_date"}}`, exit 0, or `{"error": "..."}` and exit 1.
- `vsesvit --update`: check, download into the same cache dir (progress on stderr), install; adds `"installed": null | {"version", "next": "exit_now"|"relaunch"|"next_launch"}`.
- Neither opens a profile (so neither reads `updates.automatic` or `updates.channel`); both follow the channel the build was released on (`UpdateChannel::of_build()`: first prerelease field of `CARGO_PKG_VERSION`, else stable). Each must run alone (`cli.rs` rejects combinations with URLs, `--profile-dir`, `--self-test`, each other, or `--update=now`). Usage lines: "--check-for-updates   print the newest available version as JSON, without opening a window", "--update              download and install the newest version, then print the outcome as JSON".
- Used by scripts and the packaging end-to-end test (module doc). (KEEP; NEEDS-CEF only in that these must work without initializing CEF or a display [I].)

## D.8 Persistent state (updates) [V]

| Where | What |
|---|---|
| pref `updates.automatic` (local, default true) | timer on/off |
| pref `updates.channel` (local, default `UpdateChannel::of_build()`) | channel asked for |
| `$XDG_CACHE_HOME/vsesvit/updates/` | downloads and partial downloads (`glib::user_cache_dir()`, GTK-ONLY call; the path is KEEP) |
| `<real exe dir>/package-format` | installation marker (written by packaging) |
| `$APPIMAGE` | the image to replace |
| compiled-in `packaging/updater.json` | pubkey (base64 minisign) and endpoint `https://api.usequadrant.dev/api/any/vsesvit/updates/{{channel}}/{{target}}/{{arch}}/{{current_version}}?variant={{bundle_type}}` |
| memory | lifecycle state, `automatic`, `channel`, watchers, `window_seen`, `timer`, `restart` |

## D.9 Classification (updates)

All lifecycle, wording, timers, CLI: KEEP. GTK-ONLY mechanisms: `adw::Banner`, `gio::ActionEntry` `app.update`, `adw::Toast`, `glib::user_cache_dir`, `glib::spawn_future_local`, `app.quit()`. NEEDS-CEF: restart ordering with `CefShutdown` and the CEF cache singleton lock; sub-process guard; the installation marker location relative to the CEF bundle layout (`libcef.so`, resources, locales, `chrome-sandbox`) and the AppImage replacement of a much larger image (the 512 MiB download cap applies [V]) [I].

## D.10 Interactions and tests [V]

- `browser.rs`: creates `Updates`; `updates()`, `restart_program()`, `set_updates_automatic`, `set_updates_channel`.
- `window/mod.rs`: banner widget, `updates.window_opened(window)`.
- `dialogs/settings.rs`: Updates group (`watch_status`, `check`, `app.update`).
- `app.rs`: restart after the main loop.
- `lib.rs`/`cli.rs`: headless commands.
- The self-test has **no** updater check [V]. Unit tests in `lifecycle.rs` cover: up-to-date kept until the next check; a package waits for the user then asks for a restart; a failed install keeps the download for the retry; AppImage installs in the background; a failed background install is retried by the next check; check/download failures shown until the next check; Settings offers the banner's button; Settings shows a background install the banner keeps quiet about; channel change checks at once when idle; channel change drops the Ready download; an old-channel check never lands; any end of the old check starts the new one; an install is past a channel change; Disabled is terminal; ExitNow quits without relaunch; the button does nothing without an update.

---

# Part E. Observed quirks and gaps in the old shell (verified by reading)

1. Remote `enabled` changes for installed store extensions are not applied to the runtime until restart (A.3.4).
2. Welcome installs and `Browser::install` skip the install prompt; only the Extensions dialog uses `install_asking` (A.3.3).
3. The shell always loads with `LoadReason::Startup`, never `LoadReason::Enable` (A.7).
4. `Updates` is `None` (no Settings group at all) for unpackaged builds and Flatpak, although `vsesvit-update` can "check but not apply" for them (D.1).
5. "Enter Passphrase…" in Settings shows the prompt over the browser window rather than over Settings, while Set/Change show over Settings (C.9).
6. No "Reload" for unpacked extensions on Linux although core has `reload_unpacked` (A.6).
7. Remove has no confirmation (A.6/A.9).
8. The sign-in tab stays open after sign-in (C.6).
9. The sync account row's "Last synced …" is not re-rendered on a clock, only on state changes (C.13).

---

# Checklist

Extensions: install and lifecycle
- [ ] Extensions view reachable from the main menu ("Extensions"), the puzzle-piece menu ("Manage Extensions"), an action button's context menu ("Manage Extensions"), and a notification's Settings button (opens with that extension expanded)
- [ ] Install from a pasted store link or id (CWS, legacy CWS, Edge Add-ons, AMO URL, bare CWS id, bare gecko id, file:// or path) via `InstallSource::parse`, with parse errors shown as text
- [ ] Install from a .crx/.xpi file picker ("Install Extension from File", filter "Extension packages (.crx, .xpi)")
- [ ] Load an unpacked folder picker ("Load Unpacked Extension"), no prompt
- [ ] Install pipeline: `prepare_install` (UI) → `InstallJob::run` (worker) → `commit` (UI), progress line from `InstallPhase::describe`, overlapping installs share one progress indicator
- [ ] Chrome install prompt `Add “X”?` / "It can:" / bullets / "◦" details, Cancel default, "Add Extension"; shown only when `StagedInstall::needs_approval()`; cancel commits nothing
- [ ] Install outcome messages: "Installed X V", "The extension was removed elsewhere while it downloaded", "Installed X V, but it cannot run: …", "Install failed: …"
- [ ] Apply the keymap after every commit, enable change, re-enable, uninstall, reconcile, update
- [ ] Engine runs the committed dir (managed immutable dir or developer folder); a committed extension the engine cannot run is reported, kept installed, and shown as "Not running" with the reason
- [ ] Welcome "Extensions" page: uBlock Origin Lite, Bitwarden, Proton Pass with Install/Installed, installs without a prompt
- [ ] Reconcile at startup and after sync (`Changed.extensions`): run install jobs without prompts, unload removed ones
- [ ] Apply remote enable/disable changes of installed store extensions (gap in GTK shell; decide)
- [ ] Startup: load every enabled extension, then reconcile, then schedule update checks (not in the self-test)
- [ ] Installed list: icon (largest declared, fallback addon icon, 32 px), name, "version · verification label", Enabled switch, Remove, ID and Location rows, "No extensions installed"
- [ ] Enable/disable switch through `set_enabled`, with "Enabled, but it cannot run: …" and "Cannot change the extension: …" (and list refresh)
- [ ] Withheld update: switch insensitive and off, "Enable “X”?" row with "It can now:" warnings and "Re-enable" → `approve_permissions`; the new version's code never runs before approval
- [ ] Remove: stop in the engine first, then `uninstall`, re-run if the profile refuses; "Cannot remove the extension: …"
- [ ] "Allow in private windows" switch (synced `extensions.allowed_in_private`), applied at once to private tabs and private windows' toolbars, and after sync
- [ ] "Notifications" switch for extensions with the notifications permission (synced `extensions.notifications_off`), closing/refusing notifications and firing onPermissionLevelChanged
- [ ] Extension notifications shown as desktop notifications with buttons and a Settings button that opens the Extensions view on that extension
- [ ] `permissions.request` prompt `“X” has requested additional permissions.` / "It could:" / Deny default / Allow, over the right window, never over a private window it does not run in; grants kept in core
- [ ] "Update" button: insensitive while running, `update_extensions`, toast `UpdateReport::summary()` or "Cannot update the extensions: …"; concurrent check refused
- [ ] Periodic store update checks: 60 s after start or 5 h after the last check (`next_update_check`), then every 5 h, results logged
- [ ] Updated extensions run at the new version keeping their id and storage; open Extensions views refresh
- [ ] Send the engine's Chromium version as `prodversion` (`OpenOptions.chrome_version`)
- [ ] Extensions' commands bound from `Keymap::extension_shortcuts` (enabled only, browser wins, overrides, suggested keys in install order), working while the page has focus
- [ ] `_execute_action`/`_execute_browser_action`/`_execute_page_action` open the popup or click the action; other commands fire `commands.onCommand` with the selected tab
- [ ] Settings > Shortcuts "Extension Shortcuts" rows with capture rules (needs Ctrl or Alt, browser chords refused, moving notes) and Reset to Default
- [ ] Toolbar actions in install order, pinned per synced `toolbar.extensions`, puzzle-piece menu with pin toggles, drag to reorder, popups sized 200×80..800×600, badges, filtered by `runs_in` for private windows
- [ ] `storage.sync` from other devices reaches extensions (`Changed.ext_storage`), or a recorded decision to drop it
- [ ] Decide how Chrome-native install surfaces (Chrome Web Store "Add to Chrome", chrome://extensions) relate to core's pipeline
- [ ] Self-test equivalents: install_prompt, install_crx, engine_loaded_extension, popup, extension_toolbar, context_menus, extension_commands, extension_notifications, extension_update, private_window (extension part), welcome (Install), cws_install (--network)

Extension menu items
- [ ] Page context menu: separator then extensions' items for what was clicked (page, frame, link, media, selection, editable), only extensions that run in the tab's kind, above Inspect Element's separator or at the end
- [ ] Item rendering: literal titles, checkbox/radio state, disabled items greyed, disabled submenus greyed and unopenable, submenus, separators
- [ ] Choosing a page item fires `contextMenus.onClicked` with page URL and tab id
- [ ] Action button right-click menu: the extension's action items (sections at separators), then "Unpin" and "Manage Extensions"; choosing fires onClicked with the selected tab and no page URL

Sync
- [ ] One syncer per browser; state model `SignedOut/SigningIn/SignedIn` with status words from `vsesvit_sync::status`
- [ ] Stored account read at startup (locked keyring → signed out with the reason)
- [ ] First window: first sync after 10 s when signed in; ticks every 2 s; sync when first, a minute after the last start, or 10 s after the last start when something changed (`change_seq`)
- [ ] One sync at a time; rounds up to 50; each round gather (UI) → run (worker) → finish (UI); apply `sync_applied` per round; settle errors/needs-sign-in/last synced
- [ ] `sync_applied` fan-out: bookmarks, history/sessions, Settings, extensions reconcile, private allowance, storage.sync, site permissions, trackers, cookies, keymap, theme/engine/window prefs, extension toolbar
- [ ] Sign In: worker `SignIn::start`, open the authorize URL in a foreground tab of a normal window and present it, wait up to 15 min for the loopback redirect (`http://127.0.0.1:<port>/callback`), save the account, sync, schedule
- [ ] Loopback redirect loads in the engine (no HTTPS upgrade, no loopback-network block/prompt for the sign-in)
- [ ] Cancel while signing in (canceller, no error shown); failed sign-in shows "Sign-in failed: …" or the signed-out error
- [ ] Sign Out: forget account and keys, stop the timer, revoke the session in the background, drop pending pictures
- [ ] Expired session: "Sign in again to keep syncing." with Sign In and Sign Out; no syncs or prompts meanwhile
- [ ] Account details on sign-in: profile name (unless set by hand) and picture fetched on a worker, superseded pictures dropped
- [ ] Passphrase offer once per device ("Encrypt Your Synced Data?", Not Now / Set Passphrase) setting `sync.passphrase_offered`
- [ ] Enter/Changed passphrase prompt of its own accord, over the most recent normal window, not closable except by "Start Syncing" or "Sign Out", re-asking with the error on failure, closing itself when moot or its window closes
- [ ] Prompts one at a time and held while the welcome is open
- [ ] Settings: "Encrypt with a Passphrase…", "Change Passphrase…" (over Settings, row busy meanwhile), "Enter Passphrase…"; validation "The passphrases don't match" / "Use at least 8 characters"; accept disabled until valid
- [ ] Passphrase step waits up to 120 s for a running sync, runs Argon2id on a worker, then syncs
- [ ] Delete Data on Server… with the confirmation text, waits for a running sync, signs out on success, toast on failure
- [ ] Customize Sync: Sync Everything plus Bookmarks, History, Open tabs, Extensions, Settings, writing `sync.types` and syncing
- [ ] Sync Server field: editable only signed out, empty = default, normalized, invalid shown above the field
- [ ] Settings > Sync account row with buttons in fixed order, suggested first action, Sync Now insensitive while syncing
- [ ] Welcome Sync page: server field, Sign In that advances the welcome, status title
- [ ] History "Tabs from other devices" only when signed in
- [ ] Final sync on quit: one round if something changed, at most 3 s
- [ ] Self-test equivalents: sync_passphrase, profiles (account details), settings (sync page)

Updates
- [ ] Detect the installation from `package-format` next to the real executable and `$APPIMAGE`; no update UI at all for unpackaged/Flatpak/no-key builds
- [ ] Lifecycle state machine exactly as the transition table (one worker at a time, Ready retry, channel change rules, Disabled terminal)
- [ ] Automatic checks 30 s after the first window, then every 24 h; switch "Automatic Updates" starts/stops them
- [ ] "Check for Updates" works with automatic updates off; channel change checks at once
- [ ] Check → remove stale downloads → download (resumable, verified) on a worker; results back on the UI thread
- [ ] deb/rpm/pacman wait for "Update" and install via pkexec; AppImage replaces itself in the background
- [ ] Banner under the header bar in every window for Ready / Installing (non-AppImage) / Installed (Relaunch, NextLaunch) with "Update", "Restart", "Restart now"
- [ ] Settings > General > Updates: status row (title + "Version X"), Check or the banner's button, "Update Channel" (Stable/Beta/Weekly/Nightly) with its subtitle, "Automatic Updates"
- [ ] Failed user-started install: toast with capitalized message, download kept for the retry
- [ ] Restart: quit, release the profile (and the engine's locks), relaunch the resolved program with `--profile-dir <root>`
- [ ] `--check-for-updates` and `--update` JSON CLI without a profile, display or engine, following the build's channel
- [ ] Prefs `updates.automatic`, `updates.channel`; cache dir `$XDG_CACHE_HOME/vsesvit/updates`

Shared
- [ ] A repeating timer whose interval is armed before the first run (cancel inside the run sticks)
- [ ] Worker-thread jobs whose results return to the UI thread, with worker panics surfacing on the UI thread for sync
- [ ] Plain-text (never markup) toasts for names, errors and server messages
