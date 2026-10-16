# Inventory: `crates/vsesvit-webext` (the WebKitGTK WebExtensions runtime)

Scope: everything under `crates/vsesvit-webext/` (`src/*.rs`, `src/js/api.js`, `src/js/frames.js`, `examples/harness.rs`, `examples/dnr_compile.rs`, `Cargo.toml`) and `docs/design/research-linux-extensions.md`, plus the places in `vsesvit-gtk` and `vsesvit-core` that touch this crate, so that the contract is visible from both sides. Checkout: `/home/dy/git/vsesvit-cef` (branch `linux-cef-gpui`, HEAD `27896e0`). Read on 2026-10-16.

Conventions:

- **Verified** means I read the code or doc named. **Inference** / **UNVERIFIED** is marked as such.
- Classification of each engine-touching piece:
  - **KEEP**: product behavior the new shell must reproduce (user-visible, or a Vsesvit policy beyond Chrome).
  - **WEBKIT-ONLY**: a WebKitGTK workaround or limitation that disappears with Chromium. Where Chromium's own extension system implements the same `chrome.*` behavior natively, I write **WEBKIT-ONLY (native in Chromium)**: the implementation goes away, but the behavior must still come out of Chromium once extensions are loaded into it.
  - **NEEDS-CEF**: a behavior that needs a CEF/Chromium equivalent wired by the shell; the likely CEF API is named when known.
- No architecture is proposed here.

---

## 1. Crate map

| File | Lines | Platform | What it is |
|---|---|---|---|
| `src/lib.rs` | 296 | all | Module wiring, the shell-facing API in its doc comment (the single most complete statement of the contract and the "Known limits"), `API_JS` constant |
| `src/runtime.rs` | 949 | Linux | `Runtime` (the handle the shell drives), `ActionInfo`, `LoadError`, per-tab state, tab/window event fan-out, persistence of DNR rules / dynamic scripts / menus / install markers |
| `src/bridge.rs` | 1385 | Linux | JS to Rust dispatch of every `chrome.*` method, Rust to JS event emission, messaging delivery, storage, tabs, windows, scripting, action, alarms, contextMenus, commands, notifications (GNotification), DNR API, webNavigation queries, permissions |
| `src/extension.rs` | 658 | Linux | Per-extension state (`Extension`), URL host for Gecko ids, page token, generated background page, `importScripts` pre-scan, CSP rules, `host_access`, `runs_in`, activeTab |
| `src/views.rs` | 271 | Linux | Background and popup `WebView`s, their navigation policy, `window.open` popup adoption, `connect_create` (ref-leak fix), `runtime.onInstalled/onStartup` firing |
| `src/scheme.rs` | 137 | Linux | `chrome-extension://` URI scheme handler (files, generated background page, WAR gate, CSP header, CORS) |
| `src/content.rs` | 102 | Linux | Manifest/dynamic content scripts and CSS as WebKit `UserScript`/`UserStyleSheet` |
| `src/filters.rs` | 183 | Linux | DNR compile pipeline: worker-thread translate, `UserContentFilterStore::save`, attach to every tab |
| `src/cookie_jar.rs` | 266 | Linux | `chrome.cookies` over WebKit `CookieManager`, `onChanged` by re-reading the store |
| `src/tabs.rs` | 232 | all (`TabHost` Linux) | `TabId`, `TabInfo`, `NewTab`, `tabs.query` matcher, the `TabHost` trait the shell implements |
| `src/windows.rs` | 409 | all | `WindowId`, `WindowInfo`, `WindowState`, `WindowScope`, `WindowQuery`, `NewWindow`, `WindowUpdate`, `changes()` event diff, Chrome error strings |
| `src/web_navigation.rs` | 771 | all (`NetError::of` Linux) | Frame tree model per tab, `Load`, `NetError`, `Report` from frames.js, event generation, `getFrame/getAllFrames` |
| `src/messaging.rs` | 365 | all | Port channel table (`Ports`), `accepts_extension` (externally_connectable.ids) |
| `src/protocol.rs` | 677 | all | Wire format (`Call`, `Method` table of 78 methods, `Dispatched`, `Replies`, `Sender`), JS source builders, guards |
| `src/menus.rs` | 965 | all | `chrome.contextMenus` model (`Menus`, `Entry`, `ItemId`, `Target`, `Media`) |
| `src/notifications.rs` | 513 | all | `chrome.notifications` model (`Notifications`, `Shown`, `Priority`, `Activation`, `ACTION`, `TURNED_OFF`) |
| `src/dnr.rs` | 1167 | all | DNR rule model + translator to WebKit content-blocker JSON (`Rule`, `Grants`, `HostScope`, `translate`, `url_filter_to_regex`, `check_webkit_regex`) |
| `src/dnr_rules.rs` | 342 | all | DNR API state: enabled rulesets, dynamic/session rules, Chrome limits, `Saved` |
| `src/dynamic_scripts.rs` | 513 | all | `scripting.registerContentScripts` family with Chrome's checks/errors |
| `src/cookies.rs` | 460 | all | Cookie model, `cookies.set` sanitization, filters, change diff |
| `src/patterns.rs` | 302 | all | Pattern text to WebKit allow lists, `url_matches`, `glob`, WAR checks, `navigation_url`, `requesting_document`, `may_enter` |
| `src/gate.rs` | 270 | all | Per-view navigation gate (`Gate`, `Policy`) |
| `src/i18n.rs` | 110 | all | `_locales` catalog loading |
| `src/lifecycle.rs` | 90 | all | `LoadReason`, `InstallEvent`, `install_event()` |
| `src/permissions.rs` | 47 | all | `Prompt`, `parse`, `to_json` for `chrome.permissions` |
| `src/support.rs` | 187 | all | `unsupported_features()`: what a manifest asks for that the runtime lacks |
| `src/mime.rs` | 50 | all | Content type by file extension |
| `src/js/api.js` | 660 | JS | The `chrome.*`/`browser.*` shim injected into every extension context |
| `src/js/frames.js` | 44 | JS | Frame reporter injected into every frame of every tab (webNavigation) |
| `examples/harness.rs` | 2915 | Linux | End-to-end harness under real WebKitGTK with a fake shell (`Host: TabHost`) |
| `examples/dnr_compile.rs` | 83 | Linux | Probe: which content-blocker keys the installed WebKit compiles |

Dependencies (`Cargo.toml`, verified): `vsesvit-core`, `serde`, `serde_json`, `thiserror`, `url`, `log`; Linux only: `getrandom 0.4`, `gtk` (gtk4 0.11), `webkit` (webkit6 0.6, `v2_52`), `javascriptcore` (javascriptcore6 0.6). Dev (Linux): `vsesvit-core` with `testkit`.

Public exports (`lib.rs`): modules `cookies, dnr, dnr_rules, dynamic_scripts, gate, i18n, lifecycle, menus, messaging, mime, notifications, patterns, permissions, protocol, support, tabs, web_navigation, windows`; re-exports `LoadReason`, `Unsupported`, `unsupported_features`, `NewTab`, `TabId`, `TabInfo`, `NewWindow`, `WindowId`, `WindowInfo`, `WindowState`, `WindowUpdate`, `Gate`, `Policy`; Linux: `ActionInfo`, `LoadError`, `Runtime`, `TabHost`, `connect_create`; constant `API_JS`.

### 1.1 Who depends on this crate (verified by grep)

- `Cargo.toml` workspace: `[workspace.dependencies] vsesvit-webext = { path = "crates/vsesvit-webext" }`; also `webkit` and `javascriptcore` workspace deps (used by gtk and webext). Workspace members are `crates/*` + `xtask`, so deleting the directory removes the member; the `workspace.dependencies` line must go too.
- `crates/vsesvit-gtk/Cargo.toml` and these GTK files:
  - `app.rs`: `notifications::{ACTION, Activation}` (the `app.extension-notification` action).
  - `browser.rs`: `Runtime::new`, the `TabHost` impl (`Host`), `Prompt`, event reporting, `actions()`, `storage_sync_changed`, `allowed_in_private_changed`, `private_session_started`, `windows_changed`.
  - `tab.rs`: `Gate`, `Runtime`, `TabId`, `web_navigation::{Load, NetError}`, `connect_create`, `user_content_manager`.
  - `extensions.rs`: `LoadError`, `Unsupported`, `unsupported_features`.
  - `dialogs/extension_prompts.rs`: `permissions::Prompt`.
  - `dialogs/extensions.rs`: `notification_permission_changed`, `runs_in`.
  - `extension_menus.rs`: `menus::{Entry, ItemId}`.
  - `page_menu.rs`: `menus::{Media, Target}`, `page_menu`, `runs_in`, `menu_clicked`, `may_navigate`.
  - `trackers.rs`: `dnr` (tracking protection blocker).
  - `view_source.rs`: doc reference to `Gate`.
  - `window/ext_actions.rs`: `ActionInfo`, `action_menu`.
  - `window/mod.rs`, `window/groups.rs`, `window/tab_search.rs`: `TabId`, `activate_action`, `command`, `menu_clicked`, `tab_opened_by`, `windows_changed`.
  - `self_test.rs`: `menus::Target`, `notifications::{self, Activation}`, `loaded()`, `pending_filters()`, `notification()`.
  - `permissions.rs`: `runtime.web_context().initialize_notification_permissions(..)` (web notifications, not extensions).
- No other crate depends on it: `vsesvit` (binary), `vsesvit-core`, `vsesvit-winui`, `vsesvit-sync`, `vsesvit-update`, `xtask`, `.github` workflows and `scripts/` have no reference (grep). Docs referencing it: `docs/PLAN.md` (lines 9, 29, 37, 56, 216-230, 242), `README.md` line 75, `docs/design/research-linux-extensions.md`.

---

## 2. The shell contract (Runtime API + TabHost)

This is the contract the GTK shell drives today. With Chromium's extension system, most of it is replaced by Chromium internals, but each item names a behavior the new shell must still get from somewhere.

### 2.1 `Runtime` methods (verified in `runtime.rs`)

| Call | When the shell calls it | What it does | Class |
|---|---|---|---|
| `Runtime::new(profile: Rc<RefCell<Profile>>, session: &NetworkSession, host: Rc<dyn TabHost>)` | once per process, before any tab view | creates `<profile>/webext/{dnr,filters,installed,menus,scripts}`, registers the `chrome-extension` scheme on `WebContext::default()` (secure + CORS-enabled), opens `UserContentFilterStore` at `webext/filters`, starts watching the normal session's cookies, reads `vsesvit_core::extensions::ui_locale()` | WEBKIT-ONLY (native in Chromium) |
| `private_session_started(&session)` | when the first private window's ephemeral `NetworkSession` is created | starts watching store "1" cookies for `cookies.onChanged` | NEEDS-CEF (private profile = off-the-record request context) |
| `web_context()` | tab views must use this context | returns `WebContext::default()` | WEBKIT-ONLY |
| `load(&InstalledExtension) -> Result<(), LoadError>` / `load_with(.., LoadReason)` | at startup for each enabled extension, after an install/update commit, on re-enable | unload if loaded; read `active_permissions`, build `Extension` (reads every content-script file: `LoadError::Io` if one is missing), read `allowed_in_private`, decide `InstallEvent`, restore dynamic scripts, attach to every tab it runs in, restore DNR rules and compile (async), restore menus, start background, refresh cookie watches, fire actions-changed | WEBKIT-ONLY (native); load failure shown in UI is KEEP |
| `unload(&ExtensionId)` | disable, uninstall (before files go), before reload | detach from tabs, clear alarms, withdraw notifications, close ports, blank and drop background, drop views, release held work, fire actions-changed | WEBKIT-ONLY (native) |
| `loaded() -> Vec<ExtensionId>` | self-test, dialogs | ids in load map | NEEDS-CEF (list of running extensions) |
| `runs_in(&id, Browsing) -> bool` | toolbar, page menu filter by window kind | Normal always; Private only if allowed | KEEP (behavior) / NEEDS-CEF |
| `allowed_in_private_changed()` | after `Extensions::set_allowed_in_private` or when sync changes `extensions.allowed_in_private` | each extension joins/leaves open private tabs (for later loads), revokes activeTab in private tabs, re-reads private cookie store | KEEP / NEEDS-CEF |
| `user_content_manager(tab, Browsing) -> UserContentManager` | building every tab view (and views opened by `create`) | per-tab UCM with frame script + every running extension attached; same object on later calls | WEBKIT-ONLY |
| `may_navigate(source, target) -> bool` | `Gate` (via `Policy`), page menu "Open Link in Private Window" | a web page may reach an extension page only if web-accessible to it; extension reaches its own | WEBKIT-ONLY (native WAR enforcement in Chromium) |
| `web_reachable(url) -> bool` | `Gate::committed` | whether some outside page may load that extension URL | WEBKIT-ONLY |
| `tab_updated(tab)` | after each committed navigation and title change | `tabs.onUpdated(tabId, changeInfo{url?,title?,status:"complete"}, tab)`; url/title only if the extension may see the tab; origin change revokes activeTab | WEBKIT-ONLY (native, if tabs are Chrome tabs) |
| `tab_activated(tab)` | on tab switch | `tabs.onActivated({tabId, windowId})` | same |
| `tab_attached(tab)` | a window took the tab (new, or moved from another window); report before selecting it | first time: `tabs.onCreated(tab)` then held webNavigation events; from another window: `tabs.onDetached(tabId,{oldWindowId,oldPosition})` then `tabs.onAttached(tabId,{newWindowId,newPosition})` | same |
| `tab_moved(tab)` | reordered within window | `tabs.onMoved(tabId,{windowId,fromIndex,toIndex})` | same |
| `tab_load(tab, Load)` | every top-frame load event: `Started(uri)`, `Redirected(uri)`, `Committed(uri, Transition)`, `Finished`, `Failed(uri, NetError)`; not for the shell's own error pages | feeds webNavigation | same |
| `tab_opened_by(new, opener)` | before placing a tab a page opened | its first top-frame navigation fires `webNavigation.onCreatedNavigationTarget` first | same |
| `tab_closed(tab, window_closing)` | tab closed | closes its ports, detaches, revokes activeTab, `tabs.onRemoved(tabId,{windowId,isWindowClosing})` if it was ever placed | same |
| `windows_changed()` | a window opened, closed, took/lost focus, resized | diff vs last snapshot per extension: `windows.onCreated`, `onRemoved`, `onBoundsChanged`, `onFocusChanged` (private windows invisible to non-allowed extensions; focus to one reads `-1`) | same |
| `actions() -> Vec<ActionInfo>` | building the toolbar | `ActionInfo { extension, title, icon: Option<PathBuf>, has_popup, badge_text }` for each loaded extension with an action, in id order (GTK re-sorts by install order) | NEEDS-CEF (extension action state) |
| `connect_actions_changed(f)` | once | called after load/unload and every `action.set*` | NEEDS-CEF |
| `activate_action(&id, Option<TabId>, show: FnOnce(WebView))` | toolbar click, puzzle-menu click, `_execute_action` shortcut | grants activeTab on the tab; popup: hands the (already loading) popup `WebView` to `show`, possibly later; no popup: `action.onClicked(tab)` | NEEDS-CEF |
| `page_menu(&Target) -> Vec<(ExtensionId, Entry)>` | building the page context menu | one entry per extension, sorted by lowercase extension name | NEEDS-CEF (`CefContextMenuHandler`) |
| `action_menu(&id) -> Vec<Entry>` | right-click on a pinned toolbar button | the extension's `action`/`browser_action`/`page_action`/`all` items, max 6 top level | NEEDS-CEF |
| `menu_clicked(&id, &ItemId, Option<TabId>, Option<&Target>)` | user chose an extension menu item | flips checkbox/selects radio, saves menus (lazy bg), grants activeTab, fires `contextMenus.onClicked(info, tab)` once the background has loaded | NEEDS-CEF |
| `command(&id, name, Option<TabId>)` | user pressed a named command's chord (not `_execute_*`) | grants activeTab, `commands.onCommand(name, tab)` once background loaded | NEEDS-CEF |
| `notification_activated(&id, notification, Activation)` | `app.extension-notification` action from a GNotification click | `notifications.onClicked(id)` / `onButtonClicked(id, index)`; Settings handled by the shell | NEEDS-CEF |
| `notification_permission_changed(&id)` | after `Extensions::set_notifications_allowed` | off: withdraws all, `onClosed(id,false)` each; always `onPermissionLevelChanged("granted"/"denied")` | KEEP / NEEDS-CEF |
| `notification(&id, notification) -> Option<Shown>` | self-test | how it shows | test-only |
| `storage_sync_changed(&ext, &[StorageChange])` | after a sync round whose `ApplyReport.changed.ext_storage` lists the extension | `storage.onChanged(changes, "sync")` in pages and content scripts | KEEP (storage.sync syncing) / NEEDS-CEF |
| `on_filters_ready(f)`, `pending_filters()` | self-test waits before `dnr_blocked` navigation | count of DNR compiles in flight | WEBKIT-ONLY |

`Inner::permissions_changed`, `reload` (`runtime.reload()`), `dynamic_scripts_changed`, `save_rules`, `save_menus` are internal.

Discrepancy (verified): the GTK shell never calls `load_with(.., LoadReason::Enable)`; `vsesvit-gtk/src/extensions.rs::load_into_runtime` always calls `runtime.load(ext)`, so a user re-enable in GTK fires `runtime.onStartup` (only `runtime.reload()` and the harness use `Enable`). Chrome fires nothing on re-enable.

### 2.2 `TabHost` trait (the shell implements it; verified in `tabs.rs` and `vsesvit-gtk/src/browser.rs` `impl TabHost for Host`)

All called on the UI thread, never while the runtime holds borrows, so the shell may call back into `Runtime`.

| Method | GTK implementation | Contract |
|---|---|---|
| `windows() -> Vec<WindowInfo>` | every `BrowserWindow`, most recently focused first | `WindowInfo { id, focused, browsing, state, width, height }` |
| `tabs() -> Vec<TabInfo>` | every window's tabs in order | `TabInfo { id, window_id, index, url: committed URL only (never one still loading), title: display title, active, browsing, group_id: tab_groups::extension_group_id(..) }` |
| `create_tab(&NewTab) -> Option<TabId>` | `window.open_tab_at(url, index, Foreground/Background)`; `window: None` = `browser.normal_window()` | `NewTab { url (absolute, already checked), active, window, index }` |
| `update_tab(tab, url, active)` | `tab.load(url)`, `window.select_tab` | |
| `move_tab(tab, window, index)` | `from.move_tab(&tab, &to, index)` | must then report `tab_attached`/`tab_moved` and close an emptied window |
| `remove_tab(tab)` | `window.close_tab` | |
| `web_view(tab) -> Option<WebView>` | the tab's view | WEBKIT-ONLY |
| `create_window(&NewWindow)` | `BrowserWindow::with_browsing`, move `tab` in, open `urls`, else a new tab; size; state; `present()` if focused else `set_visible(true)` | `popup`/`panel` types open as normal windows |
| `update_window(window, &WindowUpdate)` | state (normal/minimized/maximized/fullscreen), size, `present()` on `focused:true` | unfocus and `drawAttention` do nothing |
| `remove_window(window)` | `window.close()` | |
| `cookies_blocked(domain) -> bool` | `vsesvit_core::cookies::site_rules(&mut profile).blocks_cookie(domain)` | extensions may set no cookie for a site the user set to Block |
| `private_session() -> Option<NetworkSession>` | the private engine's session while a private window exists | |
| `ask_permissions(window, Prompt, answer: Box<dyn FnOnce(bool)>)` | `extension_prompts::request_dialog(&prompt)` as an `adw::AlertDialog` over that window; Allow = true; missing window = false | at most once; never answering = no |

---

## 3. Persistent state

| Location | Written by | Content | Lifetime rule (Chrome's) | Class |
|---|---|---|---|---|
| `<profile>/webext/installed/<host>` | `Inner::install_event` | `"<version>\n<stamp>"`, stamp = extension dir mtime in ns | decides `onInstalled(install/update)` vs `onStartup`; a reinstall (new dir) of the same version is an install again | WEBKIT-ONLY (native) |
| `<profile>/webext/dnr/<host>.json` | `save_rules` | `dnr_rules::Saved { enabled?: [ruleset ids] (only once changed), dynamic: [rule JSON] }` | dynamic rules survive restarts and updates, dropped on fresh install; enabled rulesets reset on a new version; session rules never saved | WEBKIT-ONLY (native) |
| `<profile>/webext/scripts/<host>.json` | `dynamic_scripts_changed` | `[RegisteredContentScript JSON]` with `persistAcrossSessions: true` | survive restart; dropped on install and update | WEBKIT-ONLY (native) |
| `<profile>/webext/menus/<host>.json` | `save_menus` | serde of `menus::Menus` | only for lazy backgrounds (service worker / `persistent:false`); dropped on install/update | WEBKIT-ONLY (native) |
| `<profile>/webext/filters/` | `UserContentFilterStore` | compiled WebKit content blockers, identifier = extension URL host | rebuilt on change | WEBKIT-ONLY |
| core DB `ext_storage_local` | `ExtStorage::{set,remove,clear}` via `storage.local` | per (ext,key) JSON, not synced; deleted on uninstall (`extensions::delete_local`) | | KEEP data semantics; NEEDS-CEF if Chromium owns storage |
| core DB `ext_storage_sync` | `storage.sync` | LWW register per (ext,key), sync Kind `ExtStorageSync = 6`, wire id `{ext}:{hex(sha256(key))[..32]}`; kept on uninstall | synced between Vsesvit installs | KEEP / NEEDS-CEF |
| pref `extensions.granted_permissions` (Local) | `grant_permissions`/`remove_permissions` | `BTreeMap<ExtensionId, PermissionSet>` of optional grants; forgotten on uninstall | not synced, as in Chrome | KEEP behavior / NEEDS-CEF |
| pref `extensions.allowed_in_private` (Synced) | `set_allowed_in_private` (Extensions dialog) | set of ids | Chrome's "Allow in Incognito", synced | KEEP / NEEDS-CEF |
| pref `extensions.notifications_off` (Synced) | `set_notifications_allowed` (Extensions dialog) | set of ids | per-extension notification switch, synced | KEEP / NEEDS-CEF |
| pref `toolbar.extensions` (Synced) | shell (`extensions::toolbar`) | pinned/order of action buttons | | KEEP (shell) |
| pref `keyboard.shortcuts` (Synced) | core `extensions::commands` | user's extension command chords | | KEEP (shell) |
| In memory only | | `storage.session`, alarms, session DNR rules, notifications list, activeTab grants, ports | lost on unload/restart | WEBKIT-ONLY (native) |

Env var: `VSESVIT_WEBEXT_CONSOLE` (set = extension views' console messages to stdout). Extension views always have `enable_developer_extras(true)` (Inspect works in background/popups).

---

## 4. `vsesvit_core` APIs the runtime calls (the core contract)

All verified by reading the call sites.

| Core item | Called from | Decides |
|---|---|---|
| `Profile::paths().root` | `Runtime::new` | where `webext/` lives |
| `extensions::ui_locale()` (`install.rs`: first non-empty of `LC_ALL`, `LC_MESSAGES`, `LANG`; `C`/`POSIX` = `en`; `-` to `_`) | `Runtime::new` | `chrome.i18n` locale, `@@ui_locale` |
| `extensions::InstalledExtension { id, dir, version, manifest, enabled, .. }` | `load` | what to run |
| `Profile::extensions().get(id)` | `runtime::reload` | reload from current install |
| `Extensions::active_permissions(id, &manifest) -> PermissionSet` | `load`, `permissions_changed` | required permissions + granted optional ones the manifest still lists |
| `Extensions::allowed_in_private(id)` | `load`, `allowed_in_private_changed` | private-window participation |
| `Extensions::notifications_allowed(id)` | notifications calls, `notification_permission_changed` | whether `chrome.notifications` works |
| `Extensions::grant_permissions(id, &set)` | `permissions.request` grant | stores grants |
| `Extensions::remove_permissions(id, &manifest, &set) -> Result<PermissionSet, RemoveError>` | `permissions.remove` | refuses required / unlisted, returns what was held |
| `extensions::permissions::request(&manifest, &active, &requested) -> Result<Request, PermissionsError>` | `permissions.request` | `Request::Held` / `Grant(set)` (no warning) / `Ask(set, warnings)`; errors `NotInManifest` ("Only permissions specified in the manifest may be requested."), `FileAccess(p)` ("Extension must have file access enabled to request 'p'."), `InvalidOrigin` |
| `extensions::permissions::{required, optional, PermissionSet::{from_request, union, contains, within, beyond, covers}, PermissionMessage}` | `extension.rs`, `permissions.rs`, `bridge.rs` | namespaces in shim; JSON shape |
| `extensions::permissions::{request_heading, REQUEST_LEAD}` | via `Prompt` (GTK dialog) | prompt wording |
| `Profile::extension_shortcuts() -> ExtensionShortcuts` (`.iter()` of `(ExtensionCommand{extension, command: ManifestCommand{name, description, ..}}, Option<Chord>)`) | `commands.getAll` | the shortcut each command has now ("" for none) |
| `Profile::ext_storage()` -> `ExtStorage::{get, set, remove, clear, bytes_in_use}(ext, Area::{Local,Sync}, ..) -> Vec<StorageChange>` | `storage.*` | quotas (`SYNC_QUOTA_BYTES` 102400, `SYNC_QUOTA_BYTES_PER_ITEM` 8192, `SYNC_MAX_ITEMS` 512, `LOCAL_QUOTA_BYTES` 10485760; errors "QUOTA_BYTES quota exceeded" etc.), change lists for `onChanged`. Local quota is enforced even with `unlimitedStorage` (inference from `check_quota(area, ..)`) |
| `ext_storage::{Area, StorageChange}` | `storage_sync_changed` | remote change payload from `sync::ApplyReport.changed.ext_storage` |
| `extensions::manifest::{Manifest (raw, name, manifest_version, background, action, content_scripts, permissions, optional_permissions, web_accessible_resources, dnr_rulesets, options_page, default_locale, icons, commands), Background::{ServiceWorker{script,module}, Scripts{scripts,persistent}, Page{page,persistent}}, ContentScript, RunAt, World, WebAccessible, DnrRuleset{id,enabled,path}, MatchPattern::{parse, matches, as_str, within}, RelPath::{parse, resolve, as_str}, ManifestVersion, ManifestCommand::activates_action, locale_chain, parse_tolerant_json, read_text}` | throughout | manifest model (core normalizes MV2 `browser_action`/`page_action` into `action`, `options_ui.page` into `options_page`, MV2 bare WAR list into `<all_urls>` matches) |
| `extensions::ExtensionId::{parse, as_str, is_chrome_style}` | throughout | Gecko ids get a hashed URL host |
| `private::Browsing::{Normal, Private}` | throughout | window/tab kind |
| `history::Transition::{Link, Typed, Bookmark, Reload, Redirect, FormSubmit}` | `web_navigation::Load::Committed` | Chrome transition type/qualifiers |
| `tab_groups::extension_group_id(Option<GroupId>) -> i32` | shell fills `TabInfo.group_id` | `groupId`: -1 or a positive fold of the group UUID |
| `html::escape` | generated background page | title/src escaping |
| `cookies::site_rules(..).blocks_cookie(domain)` | shell's `TabHost::cookies_blocked` | Block rule wins over `cookies.set` |
| `trackers::TrackerList::dnr_rules(level, allowed)` | GTK `trackers.rs` through `dnr::translate` | tracking protection as DNR JSON |
| `testkit::{probe_crx, PROBE_ID, FixtureServer, FixtureResponse, zip_files}` | harness | fixtures |
| `extensions::Extensions::{prepare_install, commit}`, `InstallSource::from_path`, `InstallJob::run` | harness `install()` | real install pipeline |

No `vsesvit_sync` or `vsesvit_update` API is called by this crate (verified). The sync link is indirect: the shell forwards `ApplyReport.changed.ext_storage` to `storage_sync_changed` and `changed.prefs` containing `extensions.allowed_in_private` to `allowed_in_private_changed` (`vsesvit-gtk/src/browser.rs::sync_applied`).

---

## 5. Feature inventory

Each feature: (1) user-visible, (2) core APIs, (3) state, (4) classification, (5) interactions / self-test.

### 5.1 `chrome-extension://` scheme (`scheme.rs`, `mime.rs`)

1. Users see extension pages (popups, options, tabs opened by extensions) at `chrome-extension://<host>/<path>`. Web pages can only load `web_accessible_resources` (403 otherwise).
2. Core: `RelPath::parse/resolve` (path cannot escape the dir), `WebAccessible`/`MatchPattern::matches`.
3. None.
4. Details (all WEBKIT-ONLY, native in Chromium):
   - `split_uri`: host lowercased, path percent-decoded, query/fragment stripped; empty host = 400; unknown host = 404.
   - Requester judged by `patterns::requesting_document(base, view_url, Referer, own_view)`: a non-empty `Referer` names the requesting frame; else the view's top URL or "own view" (background/popup).
   - Not same-origin and not `web_accessible(path, page_url)` = 403.
   - `_generated_background_page.html` served from `Extension::generated_background_page()`.
   - Every response: `Cache-Control: no-store`; `Content-Security-Policy: <extension CSP>` (so extension pages keep their CSP in tabs and frames); `Access-Control-Allow-Origin: *` for cross-origin (WAR) loads.
   - Content types by extension (`mime::for_path`): html, js/mjs (`text/javascript`), css, json, txt/md, xml, svg, png, jpg/jpeg, gif, webp, ico, bmp, avif, woff, woff2, ttf, otf, wasm, map (`application/json`), webm, mp4, mp3, ogg/oga, wav, pdf; else `application/octet-stream`.
   - CSP (`extension.rs::content_security_policy`): MV3 `content_security_policy.extension_pages` only if "secure" by Chrome's rule (`script-src` or `default-src` present; `script-src`, `object-src`, `worker-src` may name only `'self'`, `'none'`, `'wasm-unsafe-eval'`, `http(s)://localhost` / `127.0.0.1` with optional port); MV2 string as given; default `script-src 'self'; object-src 'self';`. Insecure MV3 policy is logged and replaced.
5. Harness checks the twin options page's CSP holds in a tab; the widget popup's iframe loads.

### 5.2 Extension identity inside the runtime (`extension.rs`)

- URL host (`url_host`): Chrome-style ids (32 a-p) are the host; Gecko ids (`{uuid}`, `name@domain`) get a stable 32-hex host from two FNV-1a 64-bit hashes (seeds `0xcbf29ce484222325` and `a ^ 0x9e3779b97f4a7c15`). `chrome.runtime.id` stays the real id; `@@extension_id` and `getURL` use the host. WEBKIT-ONLY, but note: Chromium cannot give a Gecko-id extension that id at all (it derives ids from `key` or path); see risks.
- Isolated world name = the extension id. Content-script handler `vsesvit_<host>` (in that world); page handler `vsesvit_<host>_page` (default world).
- `page_token`: 128 random bits (getrandom) per load; every page call must carry it because the default-world handler is reachable by any document in the view. WEBKIT-ONLY.
- `cors_granted`: once any view got a CORS allowlist, its views refuse cross-origin subframes for the rest of the web process. WEBKIT-ONLY.

### 5.3 Background context (`views.rs::start_background`, `extension.rs::background_page`, `api.js`)

1. Invisible. Errors/console only through Inspect (developer extras on) or `VSESVIT_WEBEXT_CONSOLE`.
2. Core: `Background` enum, `html::escape`.
3. Install marker (5.24).
4. Behavior:
   - `background.page`: loaded at its URL. `background.scripts` and MV3 `background.service_worker`: a generated page `_generated_background_page.html` (`<!doctype html><html><head><meta charset="utf-8"><title>NAME</title><script src=..>...</head><body></body></html>`). `type: "module"` worker = one `<script type="module">`. Classic worker: the page first loads every script the worker imports by string-literal `importScripts(...)` (recursive, resolved against the worker URL, only inside the extension, cycle-safe, `//`-commented lines skipped, template literals with `${}` ignored), then the worker; the shim's `importScripts` only checks the script is already in `document.scripts` and throws `Vsesvit: importScripts(..) cannot load ..` otherwise. WEBKIT-ONLY (Chromium runs real service workers).
   - Background view: own UCM with the page shim + page handler, `web_extension_mode` MV2/MV3, `default_content_security_policy`, CORS allowlist = host permissions, network session = profile's, context = default.
   - Navigation policy of extension views: inside = own base URL, `about:`, `blob:`, `data:`; user-driven navigations (link click, form) leaving it are cancelled and http(s) or own URLs open as a tab (`Inner::open_tab`: selected tab at the end of the last focused normal window); main-frame responses outside are cancelled and opened as a tab; cross-origin subframes refused once `cors_granted`. Background `NewWindowAction`s refused except the popup it was asked to open. KEEP: "links in extension popups/backgrounds open in a normal tab". The rest WEBKIT-ONLY.
   - Background never suspends (service worker emulated as a persistent page); `runtime.onSuspend` is never fired. WEBKIT-ONLY.
   - Work arriving before the background's `load` finished (`sendMessage`/`connect` to the extension, `contextMenus.onClicked`, `commands.onCommand`, notification events, popup `window.open`) is held (`background_waiting`) and run after; `onInstalled`/`onStartup` fire on first `LoadEvent::Finished`. Native in Chromium (event-driven SW wake).
   - Web process termination or view destroy closes the view's ports (idle callback).
5. Self-test `content_script`, `popup`; harness lifecycle section.

### 5.4 Content scripts from the manifest (`content.rs`, `patterns.rs::webkit_patterns`)

1. Extensions modify pages. Not visible otherwise.
2. Core: `ContentScript { matches, exclude_matches, js, css, run_at, all_frames, match_about_blank, world }`.
3. None.
4. Behavior (WEBKIT-ONLY, native in Chromium):
   - One `UserScript` per entry: bootstrap (the shim with `kind:"content"`) + each file joined with `"\n;\n"`, in the extension's world; MAIN-world entries go to the page world without the shim (no `chrome.*`), as in Chrome.
   - `run_at`: `document_start` = Start; `document_end` and `document_idle` = End (no idle in WebKit).
   - `all_frames` = AllFrames else TopFrame. `match_about_blank` ignored (inference: not read by `content.rs`).
   - Patterns: `<all_urls>` = `http://*/*` + `https://*/*`; `*://` expanded to http and https; `file:` patterns dropped (no file access grant, KEEP as policy: no extension reaches `file:` pages); an entry left with no allow pattern is skipped.
   - CSS: one `UserStyleSheet` per entry at **user** level in the extension's world (WebKitGTK ignores author-level user sheets on standards-mode pages), so page rules beat it unless `!important`. WEBKIT-ONLY.
   - Applies to loads that start after attaching (as Chrome).
   - Private tabs get them only if allowed in private (KEEP).
5. Self-test `content_script`, `private_window` (probe's content script absent in private until allowed).

### 5.5 Extension pages in tabs

- Every tab UCM gets each running extension's page shim restricted to `chrome-extension://<host>/*`, all frames, document start, default world, plus the page handler, so the options page, `tabs.create(getURL(..))` pages and links to extension pages get the full page API (`Origin::TabPage`). Events and dispatches into a tab's default world are wrapped in `page_guard(host)` (`location.protocol === "chrome-extension:" && location.host === host`) so a web page that took over the tab sees nothing. WEBKIT-ONLY (native).
- `about:blank` frames inside extension pages get no API (limit).

### 5.6 The JS shim (`src/js/api.js`) and wire protocol (`protocol.rs`)

- Bootstrap = `(function (config) {...})(CONFIG);` declaring nothing global; guard `if (globalThis.__vsesvit) return;` so it can run many times per world. `config = { id, host, handler, token, kind: "content"|"page", manifest: raw, i18n: {locale, messages}, permissions: [namespaces of required ∪ optional], optionsPage, dnr: constants }`.
- Defines non-configurable `globalThis.__vsesvit = { dispatchMessage, dispatchConnect, emit, id, kind }` and sets `globalThis.chrome = globalThis.browser = api` (same object, callback and promise both work; no `browser`-specific promise-only semantics difference).
- Callback/promise duality: a trailing function argument is the callback (popped in exactly one place); `chrome.runtime.lastError` is set only during the callback; promise rejects with `Error(message)`.
- Request: `{ m: method, a: args, u: location.href, top: window===top, t: token }` via `webkit.messageHandlers[handler].postMessage` (Promise). Reply `{v: value}` / `{}` (undefined) / rejection message. Unknown method: `"<name> is not supported by Vsesvit"`. Missing handler: `"Vsesvit: the extension bridge is unavailable in this context"`. Wrong token: same text. Unloaded: `"Vsesvit: the extension is no longer loaded"`.
- Rust to JS: `globalThis.__vsesvit && globalThis.__vsesvit.emit("name", ...args);` via `evaluate_javascript`; dispatches via `call_async_javascript_function` returning `Dispatched {none?, v?, e?}`.
- Events: `ExtensionEvent` with `addListener/removeListener/hasListener/hasListeners`; listener exceptions are logged (`console.error`) not propagated. `UrlFilteredEvent` for webNavigation supports `{url: [UrlFilter]}` keys `hostContains, hostEquals, hostPrefix, hostSuffix, pathContains/Equals/Prefix/Suffix, queryContains/Equals/Prefix/Suffix, urlContains/Equals/Prefix/Suffix, urlMatches (RegExp), originAndPathMatches, schemes (ignored), ports (ignored)`; bad filter throws `TypeError("Error in invocation of <event>.addListener: filters.url must be a list of events.UrlFilter")`.
- Content scripts may call only: `runtime.sendMessage`, `runtime.connect`, `port.*`, `storage.get/set/remove/clear/getBytesInUse` (`Method::allowed_in_content_script`); anything else errors `"<m> is not available in content scripts"`.
- All WEBKIT-ONLY (native in Chromium).

### 5.7 `runtime` messaging and ports (`bridge.rs`, `messaging.rs`)

1. Not directly visible; it is how extension parts talk.
2. Core: `Manifest.raw["externally_connectable"]`.
3. In memory.
4. Behavior (WEBKIT-ONLY, native in Chromium):
   - `runtime.sendMessage(message, options?, extensionId?)` (argument forms: `(msg)`, `(msg, options)`, `(id, msg)`, `(id, msg, options)`; options recognized only if keys are `includeTlsChannelId`): delivered to every page of the target extension (views, then tabs showing its pages) except the sender's own context; first response or listener error wins; `undefined` once all finished; `"Could not establish connection. Receiving end does not exist."` when none listened. Listeners answer by `sendResponse`, `return true` (async) or returning a Promise.
   - `MessageSender`: `{id, url, origin, tab? (as receiver may see it), frameId: 0 only for top-frame callers}`.
   - `tabs.sendMessage(tabId, msg, options)`: to the extension's content scripts in the tab's **top frame** and its page if the tab shows one. `options.frameId` ignored.
   - Ports: `runtime.connect([id], {name})`, `tabs.connect(tabId, {name, frameId})` (non-zero frameId reaches nothing). Opener's port opens at once (posts queued), each target context with an `onConnect` listener gets its own port; opener's posts reach all receivers, receivers' posts reach the opener; opener's `onDisconnect` once every receiver left, with `NO_RECEIVER` error if none accepted. A receiver disconnecting inside its listener counts as having accepted. Ports close when the document unloads (`pagehide` in shim), tab closes, popup/view closes, web process dies, or extension unloads. Posting on a closed port throws `"Attempting to use a disconnected port object"`. Receiving uses a long-poll `port.receive` returning `[{m: msg} | {d: error|null}]`.
   - External: another extension's pages can `sendMessage`/`connect` (`onMessageExternal`/`onConnectExternal`) unless the target's `externally_connectable` lacks `ids` naming the caller (`"*"` = all; `externally_connectable` without `ids` = no extension). Web pages cannot message extensions (`externally_connectable.matches` unsupported; reported by `support.rs`).
5. Self-test `content_script`, `extension_port`; harness section 8.

### 5.8 `storage` (`bridge.rs::storage`, `session_storage`, core `ext_storage`)

1. Extension settings persist and, for `storage.sync`, follow the user between Vsesvit installs on Linux.
2. Core: `ExtStorage::{get,set,remove,clear,bytes_in_use}`, `StorageChange`.
3. `ext_storage_local`, `ext_storage_sync`; `storage.session` in memory per load.
4. Behavior:
   - `local`, `sync` (content scripts and pages), `session` (pages only, `setAccessLevel` no-op). Methods `get(keys|null|string|array|object-with-defaults)`, `set(items)`, `remove(key|keys)`, `clear()`, `getBytesInUse(keys?)`; area `onChanged`, global `storage.onChanged(changes, areaName)`. Quota constants exposed (`local.QUOTA_BYTES` 10485760; `sync.QUOTA_BYTES` 102400, `QUOTA_BYTES_PER_ITEM` 8192, `MAX_ITEMS` 512, `MAX_WRITE_OPERATIONS_PER_HOUR` 1800, `MAX_WRITE_OPERATIONS_PER_MINUTE` 120 (write rates not enforced)); `session.QUOTA_BYTES` 10485760 (not enforced). Missing: `storage.managed`, `getKeys`.
   - `onChanged` fires in every page and, except for `session`, in content scripts in tabs (top frame only).
   - **KEEP + NEEDS-CEF**: `storage.sync` syncing through Vsesvit sync (Kind 6) and remote changes firing `storage.onChanged`. With Chromium owning `chrome.storage`, this is lost unless bridged. Lead (verified in Chromium source at `/tmp/vsesvit-research/chromium/extensions_handler.cc`, downloaded by another research step): CDP `Extensions.getStorageItems/setStorageItems/removeStorageItems/clearStorageItems` exist, gated by `CanAccessStorage`; whether reachable through CEF `CefBrowserHost::ExecuteDevToolsMethod` is UNVERIFIED.
   - `storage.local` deleted on uninstall, `storage.sync` kept (core `uninstall`), as Chrome.
5. Self-test `content_script` (visits counter), `extension_update` (storage survives update); harness section 12 (`storage.session` dropped on restart).

### 5.9 `i18n` (`i18n.rs`, shim)

- Catalog: `_locales/<locale>/messages.json` for `locale_chain(ui_locale, default_locale)` from least to most specific, tolerant JSON (BOM, comments, trailing commas), keys lowercased, entries without `message` dropped, `default_locale` cannot escape the dir. Embedded in the shim.
- `getMessage(name, subs)`: predefined `@@extension_id` (URL host), `@@ui_locale`, `@@bidi_dir`, `@@bidi_reversed_dir`, `@@bidi_start_edge`, `@@bidi_end_edge` (RTL for `ar fa he iw ps sd ug ur yi dv ckb`); one-pass substitution of `$NAME$` placeholders (only the message's own names) and `$1..$9`, `$$` = `$`; unknown = `""`.
- `getUILanguage()` (locale with `-`), `getAcceptLanguages()` (just the UI locale), `detectLanguage()` stub `{isReliable:false, languages:[]}`.
- WEBKIT-ONLY (native in Chromium). Core's `locale_chain`/`MessageCatalog` stay for manifest localization (core-owned).

### 5.10 `tabs` (`bridge.rs::tabs`, `tabs.rs`)

1. Extensions open, switch, move and close the user's tabs; the user sees those changes in the tab strip/sidebar.
2. Core: `Browsing`, `extension_group_id` (via shell).
3. None.
4. Behavior (WEBKIT-ONLY implementation; the requirement that `chrome.tabs` sees **Vsesvit's own tabs and windows** is KEEP + NEEDS-CEF, the WebView2 shell lacks it (WebView2Feedback #3853/#3854)):
   - `Tab` JSON: `id, windowId, index, active, highlighted (=active), selected (=active), pinned: false (always, even for pinned tabs), incognito, groupId, status: "complete"`; `url` and `title` only with `tabs` permission or host access to the tab URL (or activeTab). No `favIconUrl`, `audible`, `mutedInfo`, `discarded`, `openerTabId`, `width/height`, `lastAccessed`.
   - `query(queryInfo)`: keys `active, index, windowId (incl. WINDOW_ID_CURRENT -2), currentWindow, lastFocusedWindow, windowType ("normal" only), title (glob), url (pattern or list)`; other keys ignored (so `{pinned:true}` matches every tab); tabs whose content the caller can't see never match `url`/`title`.
   - `get(tabId)` ("No tab with id: N."), `getCurrent()` (only from an extension page in a tab, else undefined), `create({url="about:blank", active|selected=true, windowId, index})` (relative URLs resolve against the calling extension page, else extension root; allowed schemes http, https, data, `about:blank`, own extension pages; `javascript:` refused "JavaScript URLs are not allowed in API based tab navigation. Use the scripting API instead."; `file:` and others "Cannot navigate to \"X\"."), `update([tabId], {url, active})` (no tabId = active tab of current window; other props ignored), `move(tabIds, {index (-1 end), windowId})` (consecutive placement, cross normal/private refused "Tabs can only be moved between windows in the same profile."), `remove(tabId|ids)`, `reload([tabId])` (`bypassCache` ignored), `sendMessage`, `connect`, `TAB_ID_NONE: -1`.
   - Events: `onCreated, onUpdated (only on commit/title change; status always "complete", never "loading"), onActivated, onMoved, onDetached, onAttached, onRemoved`. Order rule: report `tab_attached` before selecting so `onActivated` follows `onCreated`.
   - Missing: `tabs.executeScript`, `tabs.insertCSS`, `tabs.removeCSS` (MV2 APIs absent from the shim), `duplicate`, `highlight`, `discard`, `group`, `ungroup`, `captureVisibleTab`, `detectLanguage`, `getZoom/setZoom`, `goBack/goForward`, `onReplaced`, `onHighlighted`, `onZoomChange`; no `chrome.tabGroups`.
   - Private tabs invisible to non-allowed extensions (KEEP).
5. Harness sections 7 and 14; self-test `extension_windows`.

### 5.11 `windows` (`windows.rs`, `bridge.rs::windows`)

1. Extensions open/resize/focus/close real browser windows.
2. Core: `Browsing`.
3. None.
4. Behavior (WEBKIT-ONLY implementation; KEEP that it maps onto Vsesvit's windows; NEEDS-CEF):
   - `Window` JSON: `id, focused, incognito, type: "normal", state, alwaysOnTop: false, left: 0, top: 0 (Wayland gives no position), width, height, tabs? (populate)`.
   - `WindowScope`: a page in a tab is in that tab's window; any other page's "current" is the last focused window it may know.
   - `get(id, {populate, windowTypes})`, `getCurrent`, `getLastFocused`, `getAll` (sorted by id), `create({url|urls, tabId, type normal|popup|panel (all normal), state, focused (minimized defaults unfocused), incognito, width, height, left/top (ignored)})`, `update(id, {focused, state, width, height})` (unfocus/drawAttention ignored), `remove(id)`. Constants `WINDOW_ID_NONE -1`, `WINDOW_ID_CURRENT -2`, `WindowType`, `WindowState`, `CreateType`.
   - Errors (Chrome's): `"Invalid value for state"` (minimized+focused, maximized/fullscreen+unfocused, any of these with bounds), `"No current window"`, `"No last-focused window"`, `"No window with id: N."`, `"Invalid enumeration value \"X\" for type|state"`, `"Cannot open URL \"X\" in an incognito window."`.
   - `create({incognito:true})` works for any extension; if it does not run in private, its own pages are stripped (error if only those) and it gets `undefined` back (KEEP, Chrome behavior).
   - Events (`changes()`): created, removed, bounds changed (size only), focus changed (may pass through `-1`); private windows hidden for non-allowed extensions, focus to one = `-1`.
5. Self-test `extension_windows`; harness 14.

### 5.12 `scripting` (`bridge.rs`)

1. Extensions inject scripts/CSS on demand (e.g. after the user clicks the action).
2. Core: `RelPath`.
3. None (dynamic scripts: 5.13).
4. Behavior (WEBKIT-ONLY, native in Chromium):
   - Requires `scripting` permission ("<method> requires the \"scripting\" permission") and host access to the tab's committed URL **and** to a pending URL of another origin (`"Cannot access contents of url \"X\". Extension manifest must request permission to access this host."`); the injected source starts with a document guard that throws `"Cannot access contents of the page. Extension manifest must request permission to access the respective host."` if a navigation committed in between.
   - `executeScript({target:{tabId}, files|func (or legacy function) + args, world: ISOLATED|MAIN})`: top frame only (`frameIds`, `allFrames` ignored), result `[{frameId: 0, result}]`; ISOLATED code gets the content-script shim even without a manifest content script; files accept `/x.js` and own URLs.
   - `insertCSS/removeCSS({target, css|files})`: exactly one of css/files ("Exactly one of 'css' and 'files' must be specified."), missing file "Could not load file: 'X'."; inserted as `<style>` elements keyed by `css:<text>`/`file:<path>`; remove takes out the last inserted with the same key; `origin` ignored.
   - Enums `ExecutionWorld`, `StyleOrigin`.
5. Harness 6, 7, 13.

### 5.13 Dynamic content scripts (`dynamic_scripts.rs`)

- `registerContentScripts([..])`, `getRegisteredContentScripts({ids})`, `updateContentScripts([..])`, `unregisterContentScripts({ids})`, all-or-nothing, with Chrome's exact errors (empty id, `_` prefix, duplicate, missing `matches`, no js/css, empty matches, invalid pattern with Chrome's parser reason, missing file `"Could not load javascript 'x' for script."`, bad `runAt`/`world` enums, unknown property). Empty `ids` list = all. Defaults: `runAt: document_idle`, `world: ISOLATED`, `allFrames: false`, `persistAcrossSessions: true`. `matchOriginAsFallback` kept and reported, ignored.
- Injected only where the extension holds host permissions (`matches` narrowed by `MatchPattern::within`), from the next load; persisted scripts survive restarts, dropped on install/update.
- WEBKIT-ONLY (native in Chromium). Self-test `dynamic_content_script` (uBlock Origin Lite's cosmetic filtering path).

### 5.14 `action` / `browserAction` and the toolbar UI (`bridge.rs::action`, `views.rs::open_popup`, GTK `window/ext_actions.rs`)

1. User-visible (GTK shell, verified in `ext_actions.rs`):
   - A button per **pinned** action, in the synced order (`toolbar.extensions`), flat style, 16 px icon (largest manifest icon; fallback `application-x-addon-symbolic`), badge text overlaid bottom-right (CSS class `extension-badge`, hidden when empty), tooltip = title.
   - A puzzle-piece button (`application-x-addon-symbolic`, tooltip "Extensions"), shown only while any action exists, opening the Extensions menu: heading "Extensions", one row per action in install order (icon + ellipsized name, max 32 chars, clicking activates the action) with a pin toggle (`view-pin-symbolic`, tooltip "Pin to Toolbar" / "Unpin from Toolbar"), separator, "Manage Extensions" (`win.show-extensions`).
   - Click on a button or row: popup page in a `gtk::Popover` under the button (or under the puzzle piece when unpinned); popup sized to its document after load (`scrollWidth/scrollHeight`), clamped to min 200x80, max 800x600, initial 320x200; closing the popover drops the view; `window.close()` in the page closes it; a badge/title/icon update keeps an open popup; unloading the extension closes it. No popup: `action.onClicked(tab)`.
   - Drag a pinned button onto another to reorder (`win.extension-move`); right-click: extension's own action items (`action_menu`), then "_Unpin" and "_Manage Extensions".
   - Private windows show only actions of extensions allowed there.
2. Core: `extensions::toolbar::{TOOLBAR, layout, set_pinned, move_pinned}` (shell), `Manifest.action { default_popup, default_title, default_icon }`.
3. `toolbar.extensions` (synced); action state in memory.
4. Runtime behavior:
   - State seeded from manifest: title = `default_title` or name; icon = largest `default_icon` else largest `icons`; popup = `default_popup`; badge "".
   - `setBadgeText({text})` (global only; `tabId` ignored), `getBadgeText`, `setTitle({title})` (null resets to the **extension name**), `getTitle`, `setIcon({path: string | {size: path}})` (largest size; `imageData` ignored), `setPopup({popup})` ("" = none; accepts `/x` and own URL), `getPopup` (full URL or ""), `setBadgeBackgroundColor`, `setBadgeTextColor`, `enable`, `disable` = no-ops, `getBadgeBackgroundColor` = `[0,0,0,0]`, `isEnabled` = true, `onClicked`. Error without action: "This extension declares no action". Missing: `openPopup`, `getUserSettings`, `onUserSettingsChanged`, per-tab state.
   - Activation grants activeTab on the selected tab.
   - Popups: for page backgrounds (`background.page`/`scripts`) the popup is opened with `window.open` from the background page so `runtime.getBackgroundPage()` works (WEBKIT-ONLY: Chromium has native `getBackgroundPage`/`getViews`); for service workers a plain view. Popup views share the background's web process (`related_view`).
   - Classification: rendering buttons/badges/popups in shell chrome is **KEEP**; getting the action state, the popup page, and firing `onClicked` from GPUI is **NEEDS-CEF**. Verified: CEF 154 bindings have no extension API (`_cef_request_context_t` has no `load_extension`/`get_extension`; no `cef_extension_t`); the only toolbar hooks are Chrome-style (`CefCommandHandler`, `cef_chrome_toolbar_button_type_t` with no extensions entry). CDP `Extensions.triggerAction(id, targetId)` exists in Chromium (verified in `extensions_handler.cc`) but requires `allow_loading_extensions_` (browser-type DevTools target) and a Chrome tab with an `ExtensionsContainer`.
5. Self-test `popup` (title `visits=N`), `extension_commands` (`_execute_action` opens popup); harness 5-7.

### 5.15 `alarms` (`bridge.rs`)

- `create([name=""], {when | delayInMinutes | periodInMinutes})`, `get(name)`, `getAll()`, `clear(name)` (bool), `clearAll()` (bool), `onAlarm(Alarm{name, scheduledTime, periodInMinutes?})`. Minimum 0.5 min (30 s) for delay and period, clamped with a warning; max 500 alarms ("This extension already has the maximum of 500 alarms."); delays over `u32::MAX` ms waited in legs; periodic re-armed each time. In memory only; cleared on unload and `runtime.reload()`.
- WEBKIT-ONLY (native in Chromium). Harness 18 (reload drops alarms).

### 5.16 `contextMenus` / Firefox `menus` (`menus.rs`, `bridge.rs`, GTK `page_menu.rs`, `extension_menus.rs`)

1. User-visible (GTK): in the page context menu, after a separator and before Inspect Element (and its separator), each extension contributes nothing, its single matching item (a lone parent shows as its own submenu), or a submenu titled with the extension's name holding its items; extensions sorted by name; checkbox and radio items show as check items (radio shows a check mark, a WebKit limit); disabled submenus greyed; `_` in titles escaped. Private tabs: only extensions running there. The pinned action button's right-click menu lists the extension's action items (max 6 top level) in sections split at separators.
2. Core: `MatchPattern`.
3. `webext/menus/<host>.json` for lazy backgrounds.
4. Behavior (model):
   - Create defaults: `type normal`, `contexts ["page"]`, visible, enabled; ids string or integer (shim generates integers: from 1 in the background page, random base ≥1e6 elsewhere); lazy backgrounds must pass an id and may not pass `onclick` (Chrome's errors). Errors: duplicate id, missing title (non-separator), checked on non-checkable, parent not found / not normal / cycle, invalid patterns (`Invalid url pattern 'X'`), invalid contexts (Chrome's list), `launcher` refused, type errors.
   - Contexts: `all, page, frame, selection, link, editable, image, video, audio, browser_action, page_action, action`, plus Firefox's `tab, tools_menu, bookmark, password` accepted but matching nothing. `page` only when no link/selection/editable/media; `frame` only in subframes; `documentUrlPatterns` tested against the frame URL when there is one; `targetUrlPatterns` against link or media source.
   - Titles: `%s` = selection cut to 50 chars, whole title to 75, with `…`.
   - Radio runs: consecutive radio siblings; each keeps its last checked or checks the first; unchecking a radio does nothing.
   - Separators cleaned (no leading/trailing/doubled).
   - Click: checkbox flips, radio selected, `OnClickData {menuItemId, parentMenuItemId?, editable, wasChecked?, checked?, pageUrl, frameUrl | frameId: 0, linkUrl?, srcUrl?, mediaType?, selectionText?}`; action menu clicks carry no page data. `onclick` functions run only in the page that created the item.
   - `update` with `parentId` makes the item its parent's last child; `remove` takes descendants; `removeAll`.
   - The shell supplies `Target { page_url, frame_url, link_url, src_url, media, selection, editable }`; GTK gets selection and clicked frame via its own script world `vsesvit-shell` (handlers `vsesvitSelection`, `vsesvitContextMenu`) because WebKit doesn't say.
   - Classification: menu model WEBKIT-ONLY (native in Chromium); showing extension items inside the **shell-drawn** context menu (GPUI) and the action button menu is KEEP + NEEDS-CEF (`CefContextMenuHandler::OnBeforeContextMenu` model; Chrome assigns extension items command ids from `IDC_EXTENSIONS_CONTEXT_CUSTOM_FIRST = 49000` (constant verified in the bindings); whether CEF populates them for the shell's browsers is UNVERIFIED).
5. Self-test `context_menus` (Linux): probe's "Vsesvit Probe page item" in a page menu before Inspect Element, none on a link, click reports page URL and tab id; action menu starts with "Vsesvit Probe action item". Harness 9.

### 5.17 `commands` (`bridge.rs::commands`, `runtime.rs::command`, GTK keymap)

1. User-visible: extension shortcuts work while a Vsesvit window has focus (including `"global": true` ones). Settings > Shortcuts lists them under "Extension Shortcuts" (shell + core).
2. Core: `Profile::extension_shortcuts()` (resolution of suggested keys vs browser shortcuts vs user overrides, in core `extensions::commands`), `ManifestCommand::activates_action`, `ACTION_COMMANDS = ["_execute_action", "_execute_browser_action", "_execute_page_action"]`.
3. `keyboard.shortcuts` (synced, core).
4. Behavior: GTK binds `win.extension-command((ext, name))` accelerators from `ExtensionShortcuts`; action commands call `activate_action`; named commands call `Runtime::command` (grants activeTab, `commands.onCommand(name, tab)`). `commands.getAll()` lists every command incl. action ones with `{name, description, shortcut}` ("" for none). `chrome.commands` exists only if the manifest declares `commands`. Classification: KEEP (Vsesvit resolves and binds shortcuts itself) + NEEDS-CEF (firing `onCommand` / opening the action from the shell; no CEF API found).
5. Self-test `extension_commands` (Linux: Alt+Shift+K fires onCommand with tab id and URL; Alt+Shift+P opens the popup; reassigning in Settings). Harness 10.

### 5.18 `notifications` (`notifications.rs`, `bridge.rs::show_notification`, GTK `app.rs`)

1. User-visible: desktop notifications via `GNotification` of the default `GApplication` (reaches the portal in Flatpak), laid out as Chrome on Linux: title (progress: `"NN% - title"`), body = context message, blank line, message, then list items as `"title - message"` lines; icon = PNG scaled to fit 128x128 (images other than the icon are loaded but not shown); up to 2 extension buttons, then a "Settings" button that opens the Extensions dialog on that extension (`dialogs::extensions::present_extension`), whose "Notifications" switch ("Let this extension show notifications") turns the extension's notifications off. Priority 0/1/2+ = Normal/High/Urgent. Notification id `"<extId>-<id>"`. Clicks invoke `app.extension-notification` with `(sss)` = (extension id, notification id, activation `"default"` | `"<button index>"` | `"settings"`).
2. Core: `Extensions::notifications_allowed`, `set_notifications_allowed` (shell), pref `extensions.notifications_off`.
3. Pref; live list in memory.
4. API behavior: needs `notifications` permission; `create([id], options)` (missing id = `crypto.randomUUID()`; replaces same id), `update(id, options)` (merge; bool), `clear(id)` (bool, fires `onClosed(id, false)`), `getAll()` (`{id: true}`), `getPermissionLevel()` ("granted"/"denied"). While turned off every call but `getPermissionLevel` fails "Notifications are turned off for this extension." Chrome's validation errors (missing properties, low priority deprecated, image/list/progress mismatch, progress range, id > 500 bytes, "Unable to download all specified images."). Events `onClicked`, `onButtonClicked`, `onClosed`, `onPermissionLevelChanged`, `onShowSettings` (never fired). Dismissal is not reported, so notifications stay listed until cleared/replaced. `requireInteraction`, `silent`, `eventTime`, `isClickable` ignored. Classification: model WEBKIT-ONLY (native in Chromium: Chromium has its own Linux notification bridge); KEEP: the Settings button opening Vsesvit's Extensions dialog and the synced per-extension switch; NEEDS-CEF (no CEF hook for extension notifications found; UNVERIFIED what CEF 154 does with them).
5. Self-test `extension_notifications` (Linux). Harness 11.

### 5.19 declarativeNetRequest (`dnr.rs`, `dnr_rules.rs`, `filters.rs`, `bridge.rs`)

1. Ad blockers (uBlock Origin Lite) block requests; per-site "no filtering" works; no blocked count is shown (WebKit reports nothing).
2. Core: `Manifest.dnr_rulesets`, `active_permissions`.
3. `webext/dnr/<host>.json`, `webext/filters/`.
4. Behavior:
   - API (needs `declarativeNetRequest` or `declarativeNetRequestWithHostAccess`): `updateDynamicRules`, `getDynamicRules({ruleIds})`, `updateSessionRules`, `getSessionRules`, `updateEnabledRulesets({disableRulesetIds, enableRulesetIds})` (disable then enable; "Invalid ruleset id: X."; max 50 enabled), `getEnabledRulesets` (manifest order), `getAvailableStaticRuleCount` (330000 minus enabled static rules), `isRegexSupported({regex})` (WebKit subset; `{isSupported:false, reason:"syntaxError"}`), `setExtensionActionOptions` no-op; constants `MAX_NUMBER_OF_DYNAMIC_RULES 30000`, `..._UNSAFE_DYNAMIC_RULES 5000`, `..._SESSION_RULES 5000`, `..._UNSAFE_SESSION_RULES 5000`, `..._REGEX_RULES 1000`, `..._STATIC_RULESETS 100`, `..._ENABLED_STATIC_RULESETS 50`, `GUARANTEED_MINIMUM_STATIC_RULES 30000`, `DYNAMIC_RULESET_ID "_dynamic"`, `SESSION_RULESET_ID "_session"`; enums `ResourceType`, `RequestMethod`, `RuleActionType`, `DomainType`, `HeaderOperation`, `UnsupportedRegexReason`. Errors as Chrome ("Rule with id N does not have a unique ID.", "...valid ID.", "...valid priority.", tabIds only in session rules, count exceeded). Missing: `getMatchedRules`, `onRuleMatchedDebug`, `testMatchOutcome`, `getDisabledRuleIds`/`updateStaticRules`.
   - Update promises resolve only once tabs have the new blocker (`filters::when_compiled`).
   - Compile: one WebKit content blocker per extension (enabled static rulesets read from disk on a worker thread + dynamic + session rules), translated by `dnr::translate`, saved under identifier = URL host, attached to every tab UCM the extension runs in (beside the shell's own blockers); queued changes coalesce.
   - Translator (WEBKIT-ONLY): sorted by priority desc then action rank (allow < allowAllRequests < block < upgradeScheme < redirect < modifyHeaders) so `ignore-following-rules` reproduces DNR precedence inside one filter; `urlFilter` grammar to regex (`||`, `|`, `*`, `^`, trailing `^` = two regexes), regex subset check, resource type mapping, `domainType` to `load-type`, initiator domains to `if/unless-frame-url` (one condition per trigger), request methods fanned out, `requestDomains` folded into url-filter, `allowAllRequests` to `if/unless-top-url` / `if/unless-frame-url`, redirect (url, extensionPath, regexSubstitution, transform), modifyHeaders; skipped and logged: `tabIds`, `responseHeaders`, `excludedRequestDomains` (except allowAllRequests), domains+excludedDomains together, non-ASCII, unsupported regex.
   - Grants (KEEP as Chrome's rule, native in Chromium): redirect and modifyHeaders act only on hosts the extension holds; with only `...WithHostAccess`, every rule needs host access.
   - Private tabs get the blocker only if the extension runs there.
   - Classification: entire feature WEBKIT-ONLY (native in Chromium, which also provides matched-rule counts for badges).
5. Self-test `dnr_blocked`, `dnr_site_allowed`, `engine_loaded_extension` (waits `pending_filters()==0`). Harness 12. `examples/dnr_compile.rs` (21 cases) is WebKit evidence only.

### 5.20 `webNavigation` (`web_navigation.rs`, `frames.js`)

- Events: `onBeforeNavigate`, `onCommitted` (with `transitionType`/`transitionQualifiers`: Typed = `typed` + `from_address_bar`; Bookmark = `auto_bookmark`; Reload = `reload`; Redirect = `link` + `client_redirect`; FormSubmit = `form_submit`; Link = `link`; server redirect adds `server_redirect`; subframes `auto_subframe` first then `manual_subframe`), `onDOMContentLoaded`, `onCompleted`, `onErrorOccurred` (`error`: `net::ERR_ABORTED`, `NAME_NOT_RESOLVED`, `CONNECTION_REFUSED`, `FILE_NOT_FOUND`, `UNKNOWN_URL_SCHEME`, `CERT_INVALID`, `FAILED`), `onCreatedNavigationTarget` (`sourceFrameId: 0`), `onReferenceFragmentUpdated`, `onHistoryStateUpdated` (`forward_back` for traversals), `onTabReplaced` (never). Details: `tabId, frameId, parentFrameId, processId: -1, url, frameType (outermost_frame|sub_frame), documentLifecycle "active", documentId (32 hex), parentDocumentId, timeStamp`. URL filters supported. `getFrame({tabId, frameId} | {documentId})`, `getAllFrames({tabId})` (null for unknown; Chrome's errors). Needs `webNavigation` permission.
- Implementation (WEBKIT-ONLY): top frame from the shell's load events; subframes from `frames.js` running in world `vsesvit:frames` (handler `vsesvitFrames`) reporting `start/ready/load/gone/shown/same` with a per-document token and the frame path; frames identified by path; events held until the tab is placed.
- Native in Chromium. Self-test `extension_web_navigation`; harness 15.

### 5.21 `cookies` (`cookies.rs`, `cookie_jar.rs`)

- `get({url,name,storeId,partitionKey})` (longest path first), `getAll(filter)` (each cookie judged by its own URL for host access), `set(details)` (Chrome's `CreateSanitizedCookie` rules: name/value chars, 4096 limit, domain must contain the URL host, IP host-only, path default, `__Secure-`/`__Host-` rules, SameSite=None needs secure, expiry capped at 400 days), `remove`, `getAllCookieStores` (stores with open tabs; "1" only for allowed extensions while a private window exists), `onChanged({removed, cause, cookie})` from re-reading the store (overwrite = removed `overwrite` + added `explicit`; gone = `explicit` or `expired`; never `evicted`). Errors: "Invalid cookie store id: \"X\".", "Invalid url: \"X\".", "No host permissions for cookies at url: \"X\".", "Failed to parse or set cookie named \"X\"." Partition keys naming a site reach nothing; SameSite unspecified stored as lax.
- **KEEP**: an extension cannot set a cookie for a site the user set to Block (`TabHost::cookies_blocked`); private store "1" only for extensions allowed in private. NEEDS-CEF for those two Vsesvit rules if Chromium owns the cookie API (UNVERIFIED whether Chromium's `cookies.set` honors content settings).
- Rest WEBKIT-ONLY (native). Self-test `extension_cookies`; harness 16.

### 5.22 Optional permissions (`permissions.rs`, `bridge.rs`, core `extensions::permissions`, GTK `dialogs/extension_prompts.rs`)

1. User-visible: during a user gesture, `permissions.request` for permissions with warnings shows Chrome's prompt over the window the request came from (a tab's window, else the last focused window the extension may know, never a private one it doesn't run in): heading `“<name>” has requested additional permissions.`, lead `It could:`, bullets `• <warning>` with sites as `◦ <site>` indented, buttons `_Deny` (default, close) and `_Allow` (suggested).
2. Core: `request`, `grant_permissions`, `remove_permissions`, `active_permissions`, `warnings`, `request_heading`, `REQUEST_LEAD`.
3. `extensions.granted_permissions` (local).
4. Behavior: `getAll()`, `contains({permissions, origins})` (false for any `file:` origin), `request` (no user activation: "This function must be called during a user gesture"; held = true; no-warning = granted without asking; otherwise prompt; no window = false), `remove` (refuses required: "You cannot remove required permissions."; resolves true), `onAdded`, `onRemoved`. Grants widen at once: CORS allowlist of the extension's views, dynamic scripts, DNR grants (recompile), cookie access and events, tab visibility. Namespaces for every optional permission exist in the shim from the start. A removed host stays fetchable until the view's web process ends (WebKit limit). Classification: prompt wording/placement KEEP; API WEBKIT-ONLY (native in Chromium, whose prompt UI is Chrome's own); NEEDS-CEF if the shell must draw the prompt.
5. Harness 17; GTK unit test `an_extensions_permission_request_asks_over_the_window_it_names`.

### 5.23 Private windows / "Allow in private windows" (`Extension::runs_in`)

1. Extensions dialog row switch "Allow in private windows", subtitle "Vsesvit cannot stop it from recording what you do there"; synced.
2. Core: `Extensions::{allowed_in_private, set_allowed_in_private}`, `ALLOWED_IN_PRIVATE`.
3. `extensions.allowed_in_private`.
4. Not allowed: no content scripts, page shim, DNR blocker, tab/window events, cookies store "1" in private tabs; toolbar and page-menu items hidden in private windows. Allowed: `incognito: true` on tabs/windows. Changes apply to later loads in open private tabs. `extension.inIncognitoContext` is always false. KEEP + NEEDS-CEF (Chromium's per-extension incognito pref; CDP `Extensions.loadUnpacked` has an `enableInIncognito` parameter, verified in `extensions_handler.cc`).
5. Self-test `private_window`; GTK test `the_private_windows_switch_lets_the_extension_into_private_tabs`.

### 5.24 activeTab

- Invoking the action, an action shortcut, a named command, or a context-menu item on a tab grants host access to that tab (only with `activeTab`) until it leaves the origin (`tab_updated` origin change) or closes; never for `file:`. WEBKIT-ONLY (native). KEEP: the shell must route these invocations through Chromium so the grant happens (NEEDS-CEF).

### 5.25 Lifecycle (`lifecycle.rs`, `runtime.rs`)

- `runtime.onInstalled({reason:"install"})` on first load of an install (marker absent or stamp changed), `({reason:"update", previousVersion})` on a version change, `runtime.onStartup` on later startups, nothing on re-enable (`LoadReason::Enable`). `runtime.reload()`: reloads the extension as a re-enable (no event), restarts background, alarms, content scripts; tabs showing its pages reload. Update installs (core `update.rs`, every 5 h) are loaded by the shell and fire `update`. WEBKIT-ONLY (native). Harness 18; self-test `extension_update`.

### 5.26 Navigation gate (`gate.rs`)

- Per tab view: web pages reach an extension page only if web-accessible; the browser (address bar, reload, history, session restore, `tabs.update`) reaches every page; a server redirect is never the browser's; a window a web page opened stays limited by its opener; an extension page reached through WAR speaks for the web page; only the browser opens `view-source:`. Shell calls `browser_load(target)` before its own loads, `stop()` on stop, `decide(policy, target, is_redirect, new_window)` from `decide-policy`/`create`, `committed(policy, uri)` on commit, `Gate::opened_by(&opener)` for popups.
- WEBKIT-ONLY (Chromium enforces WAR navigation and blocks renderer-initiated `view-source:`; inference for the latter). KEEP as policy: web pages cannot drive extension pages; only the browser opens view-source.

### 5.27 Unsupported-feature report (`support.rs`)

1. Extensions dialog row "Not supported by the Linux runtime" with subtitle "This extension requests: <list>. Features that depend on them will not work." and a warning icon; install prompt label "Not supported by the Linux runtime: <list>. Features that depend on them will not work." (CSS `warning caption`).
2. Core: `Manifest.permissions`, `optional_permissions`, `raw`.
3. None.
4. Supported permissions list: `activeTab, alarms, contextMenus, cookies, declarativeNetRequest, declarativeNetRequestWithHostAccess, menus, notifications, scripting, storage, tabs, unlimitedStorage, webNavigation` (+ any host pattern). Unsupported manifest keys reported: `chrome_settings_overrides, chrome_url_overrides, devtools_page, nacl_modules, omnibox, sandbox, side_panel, sidebar_action, user_scripts`, and `externally_connectable.matches`. Output sorted, deduped, displayed as `name`, `name (optional)`, `name (manifest key)`. KEEP the concept (tell users what won't work); the list must be redone for Chromium's system.

### 5.28 Tracking protection's use of `dnr.rs` (GTK `trackers.rs`)

- Linux tracking protection = core `TrackerList::dnr_rules(level, allowed_sites)` (block rules per tracker entity with `requestDomains` and `excludedInitiatorDomains` = the company's own sites; `allowAllRequests` priority 2 per allowed site) translated by `dnr::translate(.., "", Grants { hosts: All, host_access_only: false })` into a WebKit content blocker under identifier `vsesvit-tracking-protection` in `<profile>/trackers/`, attached to every tab. No blocked count on Linux. With CEF this becomes per-request blocking like Windows (`TrackerList::blocks` in `CefResourceRequestHandler::OnBeforeResourceLoad` returning `RV_CANCEL`, or a 403 response), so `dnr.rs` is not needed for it (KEEP behavior, NEEDS-CEF, translator WEBKIT-ONLY).

### 5.29 `connect_create` (`views.rs`)

- Wraps `WebView::connect_create` and unrefs the extra reference the Rust binding hands WebKit. WEBKIT-ONLY.

---

## 6. Implemented `chrome.*` surface (checklist of the runtime)

Content scripts get `runtime` (`id, getURL, getManifest, getPlatformInfo {os:"linux", arch:"x86-64", nacl_arch:"x86-64"}, sendMessage, connect, onMessage, onConnect, onInstalled, onStartup, onSuspend, lastError`), `storage` (`local, sync, onChanged`), `i18n`. Extension pages additionally get everything below. `browser` is the same object. Namespaces gated by permission (present only if required or optional permissions list them): `contextMenus`/`menus`, `notifications`, `declarativeNetRequest`, `webNavigation`, `cookies`; `commands` only if manifest declares `commands`. Always present on pages regardless of permission: `tabs, scripting, action, browserAction, alarms, windows, permissions, extension, storage.session` (the Rust side then refuses calls without the permission where Chrome requires one: `scripting`, `contextMenus`, `notifications`, DNR, `webNavigation`, `cookies`; `alarms`, `tabs`, `windows` need none here).

- runtime (pages): `openOptionsPage` ("This extension has no options page"), `onMessageExternal`, `onConnectExternal`, `getBackgroundPage` (background page and its popups only; service worker: "You do not have a background page."), `reload`.
- extension: `getURL`, `inIncognitoContext: false`, `getViews() => []`, `getBackgroundPage`.
- Not implemented anywhere (calls throw TypeError or "not supported by Vsesvit"): `runtime.setUninstallURL`, `getContexts`, `requestUpdateCheck`, `onUpdateAvailable`, `restart`, `sendNativeMessage`, `connectNative`, `getPackageDirectoryEntry`; `tabs.executeScript/insertCSS/removeCSS` (MV2), `tabs.duplicate/highlight/discard/group/ungroup/captureVisibleTab/getZoom/setZoom/goBack/goForward`, `tabGroups`, `bookmarks`, `history`, `downloads`, `webRequest`, `webRequestBlocking`, `proxy`, `privacy`, `contentSettings`, `management`, `identity`, `idle`, `offscreen`, `sidePanel`, `sessions`, `topSites`, `search`, `tts`, `ttsEngine`, `fontSettings`, `debugger`, `declarativeContent`, `pageAction` (mapped to `action` in manifest only), `omnibox`, `devtools.*`, `userScripts`, `storage.managed`, `storage.getKeys`, `nativeMessaging`, `system.*`, `power`, `gcm`, `instanceID`, `browsingData`, `readingList`, `favicon`, `dom`, `i18n.detectLanguage` (stub).

---

## 7. Known limits (from `lib.rs` docs, verified) and their fate with Chromium

All WEBKIT-ONLY (gone with Chromium's native system) unless noted:

- Events/messages reach a tab's top frame only (`tabs.sendMessage`, `tabs.connect`, `storage.onChanged` in subframes); webNavigation frame ids name frames to nothing else.
- Subframe `onBeforeNavigate` arrives with its document; failed subframe loads report nothing; `onCreatedNavigationTarget` names the opener's top frame.
- `cookies.onChanged` by re-reading: undone changes unreported, no `evicted`, expired set = `explicit`.
- No `externally_connectable.matches` (web pages cannot message extensions).
- `getBackgroundPage` unavailable from extension pages in tabs.
- Context-menu clicks in subframes have no `frameId`; radio items show a check mark.
- Image notifications show no image; `requireInteraction`/`silent` ignored; dismissals unreported.
- Every window is `normal` at (0,0); `popup`/`panel` open as normal windows; no unfocus or drawAttention (Wayland). KEEP-ish: Wayland still gives no position under XWayland-less GPUI; inference.
- No DNR matched-rule reporting (no badge counts, no `getMatchedRules`); no `webRequest`.
- Content-script CSS user-level.
- Registered scripts' `matchOriginAsFallback` ignored.
- A removed host stays fetchable until the web process ends; extension views refuse remote subframes after any host grant.
- One runtime per process.
- `about:blank` frames inside extension pages get no API.
- In background/popup views an http(s) iframe loads only for extensions without host permissions.
- Requests without `Referer` judged by the view's top document.

---

## 8. Platform-neutral pieces and what must survive the deletion

| Piece | Used outside the crate | Must it survive? |
|---|---|---|
| `dnr.rs` (`parse_rules`, `translate`, `Grants`, `HostScope`, `Skipped`, `describe_skipped`) | `vsesvit-gtk/src/trackers.rs` (tracking protection blocker) | No, once tracking protection is enforced per request under CEF (Windows already does this with `TrackerList::blocks`). Its test `the_tracker_list_translates_whole` is the only cross-crate test of `TrackerList::dnr_rules`; core's own `tests/core_trackers.rs` still tests `dnr_rules` JSON. If core's `dnr_rules` is no longer consumed it becomes dead code (decision for later). |
| `patterns.rs` | `may_navigate` via GTK page menu; `url_matches`/`glob` nowhere else | No (Chromium native); core has its own `MatchPattern`. |
| `i18n.rs` | none | No; core keeps `locale_chain`, `parse_tolerant_json`, `MessageCatalog` for manifests. |
| `menus.rs` (`Entry`, `ItemId`, `Target`, `Media`) | GTK menus | Only as a shape idea for the shell's menu rendering; the model itself is Chromium's. |
| `notifications.rs` (`ACTION = "extension-notification"`, `Activation`) | GTK `app.rs` action | No; but the behaviors in 5.18 (Settings button, synced switch) are KEEP. |
| `support.rs` | GTK dialog and install prompt | Concept KEEP, list must be rewritten for the Chromium runtime. |
| `gate.rs` | GTK tab/view-source | No (Chromium enforces); view-source-only-by-browser policy KEEP. |
| `tabs.rs`/`windows.rs` types (`TabId`, `WindowId`, `TabInfo`, ...) | GTK uses `TabId` as its tab id type in several modules | GTK goes away; the new shell needs its own ids. |
| `cookies.rs`, `dnr_rules.rs`, `dynamic_scripts.rs`, `web_navigation.rs`, `messaging.rs`, `protocol.rs`, `lifecycle.rs`, `mime.rs`, `permissions.rs` | none beyond GTK types | No. |
| `tests/fixtures/extensions/*` (probe, update-probe, commands, cookies, dnr, menus, navigation, notifications, permissions, ports, ports-friend, scripts, windows) and `tests/fixtures/site/*` | core `testkit` (probe CRX), both self-tests | Yes: the probe and update-probe back the cross-platform self-test; the others are harness-only fixtures that remain useful to prove Chromium behavior. |

## 9. `vsesvit_core` items that exist only (or mainly) to serve this runtime

Verified by grep across crates (consumers in parentheses):

- `ext_storage` module entirely: `ExtStorage::{get,set,remove,clear,bytes_in_use}`, `Area`, `StorageChange`, `StorageError`, `SYNC_QUOTA_BYTES`, `SYNC_QUOTA_BYTES_PER_ITEM`, `SYNC_MAX_ITEMS`, `LOCAL_QUOTA_BYTES`, `SyncItemRecord`, `StorageTable`, tables `ext_storage_local`/`ext_storage_sync`, `Profile::ext_storage()`, sync `Kind::ExtStorageSync = 6` (grouped under `DataType::Extensions`), `ApplyReport.changed.ext_storage`, `delete_local` clearing `ext_storage_local` (webext + GTK forwarding; Windows unused, as PLAN.md says).
- `extensions::permissions::{GRANTED_PERMISSIONS ("extensions.granted_permissions"), Extensions::active_permissions, Extensions::remove_permissions, RemoveError, request, Request, PermissionsError (NotInManifest, Required, FileAccess, InvalidOrigin), REQUEST_LEAD, request_heading, PermissionSet::from_request}` (webext, GTK prompt). `grant_permissions` is also used by core's `approve_permissions` (re-enable after an update), and `install_warnings`/`update_warnings`/`warnings`/`INSTALL_LEAD`/`RE_ENABLE_LEAD` are shared by both shells.
- `extensions::private::{ALLOWED_IN_PRIVATE ("extensions.allowed_in_private"), allowed_in_private, set_allowed_in_private}` (webext, GTK only; Windows touches no extension in private windows).
- `extensions::notifications::{NOTIFICATIONS_OFF ("extensions.notifications_off"), notifications_allowed, set_notifications_allowed}` (webext, GTK only).
- `tab_groups::extension_group_id` (GTK `TabHost`, webext doc).
- `trackers::TrackerList::dnr_rules` (GTK trackers through webext `dnr`; core tests).
- Manifest model parts consumed only here: `Manifest::dnr_rulesets` + `DnrRuleset` (doc: "The Linux runtime translates these"), `WebAccessible`, `Background` variants, `ContentScript.{run_at, all_frames, match_about_blank, world}`, `RunAt`, `World` (WinUI has an unrelated `World` symbol), `MatchPattern::within`, `locale_chain` and `parse_tolerant_json` as public (also used inside core).
- `extensions::ui_locale` as used for `chrome.i18n` (core also uses it for manifest localization).
- `history::Transition` is shared (history), but its mapping to webNavigation lives here.
- `testkit::{FixtureServer, FixtureResponse, zip_files, probe_crx, PROBE_ID}` are shared with both self-tests.

Not runtime-only (shared with WinUI): `extensions::commands` (`extension_shortcuts`, `ExtensionShortcuts`, `activates_action`, `ACTION_COMMANDS`), `extensions::toolbar`, `cookies::site_rules().blocks_cookie`, the install pipeline (CRX3 verify, `key` injection, `_metadata` removal, per-version immutable dirs).

---

## 10. Interactions with the self-test (GTK) and the harness

GTK self-test checks that exercise this runtime (`docs/design/self-test.md`, `vsesvit-gtk/src/self_test.rs`): `install_prompt` (alert `Add “Vsesvit Probe”?`, `It can:`, `• Read and change all your data on all websites`, `• Display notifications`), `install_crx`, `engine_loaded_extension` (`loaded()` contains `PROBE_ID`, waits `pending_filters()==0`), `content_script` (`data-vsesvit-probe="background-replied"`), `extension_port` (`pong:probe`), `dnr_blocked`, `dnr_site_allowed`, `dynamic_content_script`, `extension_windows`, `extension_web_navigation`, `extension_cookies`, `popup`, `context_menus`, `extension_commands`, `extension_notifications` (uses `notification()` and the `app.extension-notification` action), `extension_update`, `private_window` (probe content script only after allowed), `tracking_protection` (via `dnr.rs`), `cws_install` (`--network`, uBlock Origin Lite `ddkjiahejlhfcafbddmgiahcphecmpfh`). The probe (`tests/fixtures/extensions/probe/`) is MV3, service worker, `<all_urls>`, permissions `storage, declarativeNetRequest, contextMenus, notifications, scripting, webNavigation, cookies`, one ruleset `block`, content script at `document_end`, popup, commands `_execute_action` (Alt+Shift+P) and `probe-command` (Alt+Shift+K). The checks are driven by setting `data-vsesvit-*` attributes on the fixture page, which the probe's content script relays to the background.

`examples/harness.rs` (run `bash scripts/wsl.sh run -p vsesvit-webext --example harness [-- --show]`): 18 sections covering everything in section 5, with a fake shell (`Host`) that shows the minimal correct `TabHost` and event-reporting order (create view with runtime UCM, `tab_attached` after placing, `browser_load` before every browser load, `committed` on commit, `tab_load` for each load event with `Transition::Link`, `Load::Failed` from `load-failed`, `tab_opened_by` and `Gate::opened_by` for `create`, `tab_closed`/`close_if_empty`/`windows_changed`). Extensions used: probe, twin (XPI, Gecko id `twin@vsesvit.test`), widget, ports, friend, menus, classic (MV2 without background), commands, notifications, dnr, scripts, windows, navigation, cookies, permissions. It is the best executable specification of Chrome-compatible behavior to re-run against the Chromium runtime.

---

## 11. Verified CEF 154 facts relevant to this inventory

- `cef-dll-sys 154.5.0+154.0.34` bindings (`src/bindings/x86_64_unknown_linux_gnu.rs`): `_cef_request_context_t` has no `load_extension`, `get_extension`, `has_extension` or extension handler; no `cef_extension_t` type. Only command ids (`IDC_MANAGE_EXTENSIONS = 40022`, `IDC_EXTENSIONS_SUBMENU = 57334`, `IDC_EXTENSIONS_CONTEXT_CUSTOM_FIRST = 49000`) and resource ids remain.
- `cef_runtime_style_t` doc: windowless (off-screen) rendering always uses Alloy style; Chrome style provides "the full Chrome UI and browser functionality".
- `cef_chrome_toolbar_button_type_t` has no extensions entry (MEDIA, BATTERY_SAVER, AVATAR, deprecated others).
- The high-level `cef` crate exposes `execute_dev_tools_method`, `add_dev_tools_message_observer`, `register_scheme_handler_factory`, `on_before_browse`, `get_resource_request_handler`, `on_before_resource_load`, `on_before_context_menu`, `on_context_menu_command`, `on_process_message_received`, `send_process_message`, `on_context_created`.
- Chromium source in `/tmp/vsesvit-research/chromium/` (fetched by another research step, read only here): CDP `Extensions` domain has `loadUnpacked(path, enableInIncognito?)`, `uninstall(id)` (unpacked only), `getStorageItems/setStorageItems/removeStorageItems/clearStorageItems`, `triggerAction(id, targetId)`; `loadUnpacked`/`triggerAction` require `allow_loading_extensions_`, which `chrome_devtools_session.cc` sets only for browser-type agent hosts. `extension_service.cc` handles `--load-extension` and `--disable-extensions-except` (with a note that `--load-extension` is disallowed in Google Chrome builds).

---

## 12. Checklist

- [ ] Extensions installed from CWS/AMO/CRX/XPI/unpacked actually run in the new engine (load at startup, after install, after update, on enable; stop on disable/uninstall)
- [ ] Load failure ("installed but cannot run: …") shown under the extension and in install errors
- [ ] `chrome-extension://` pages served with WAR enforcement for web pages
- [ ] Extension page CSP kept (MV3 insecure policy replaced by default)
- [ ] Links/navigations out of popups and background open as a tab in the last focused normal window
- [ ] Background contexts run (page, scripts, MV3 service worker incl. classic `importScripts` and `type: module`)
- [ ] `runtime.onInstalled` install/update, `onStartup`, nothing on re-enable; reinstall of the same version is an install
- [ ] `runtime.reload()` restarts the extension and reloads its tabs
- [ ] Manifest content scripts: matches/exclude_matches, run_at, all_frames, CSS, MAIN world, no `file:` access
- [ ] Extension pages in tabs (options page, `tabs.create(getURL())`) get the full API
- [ ] `chrome`/`browser` objects with promise and callback styles and `runtime.lastError`
- [ ] `runtime.sendMessage`/`onMessage` with first-response-wins and Chrome's no-receiver error
- [ ] `runtime.connect`/`tabs.connect` ports with Chrome's lifetime rules
- [ ] Messages/connections from other extensions honoring `externally_connectable.ids`
- [ ] `storage.local`, `storage.sync`, `storage.session`, `onChanged`, quotas
- [ ] `storage.sync` syncs through Vsesvit sync and remote changes fire `storage.onChanged`
- [ ] `storage.local` wiped on uninstall, `storage.sync` kept
- [ ] `chrome.i18n` with `_locales`, predefined `@@` messages, placeholders
- [ ] `chrome.tabs` sees Vsesvit's real tabs (ids, windows, index, active, groupId, incognito) with Chrome's URL/title visibility rules
- [ ] `tabs.create/update/move/remove/reload/query/get/getCurrent` act on Vsesvit's tabs
- [ ] Tab events created/updated/activated/moved/detached/attached/removed in Chrome's order
- [ ] `chrome.windows` sees and drives Vsesvit's windows, with Chrome's errors and events
- [ ] `windows.create({incognito:true})` rules for extensions not allowed in private
- [ ] `scripting.executeScript/insertCSS/removeCSS` with host-access checks
- [ ] `scripting.registerContentScripts` family with persistence rules
- [ ] Toolbar: pinned action buttons in synced order, icon, badge, tooltip
- [ ] Toolbar: puzzle-piece Extensions menu with every action, pin toggles, Manage Extensions
- [ ] Toolbar: drag to reorder pinned buttons; right-click menu with extension action items, Unpin, Manage Extensions
- [ ] Action popup in a popover under its button (or the puzzle piece), sized to content within 200x80..800x600, closes on dismiss and `window.close()`, survives badge updates, closes on unload
- [ ] `action.onClicked` when no popup; `action.set*/get*` reflected in the toolbar
- [ ] Private windows show only actions of extensions allowed there
- [ ] `alarms` with Chrome's minimum and limit
- [ ] Extension items in the page context menu (grouped under the extension name when several, by context and patterns, `%s` selection, sorted by name, before Inspect Element)
- [ ] Extension items in the action button's menu (max 6 top level)
- [ ] `contextMenus.onClicked` with Chrome's OnClickData and the tab; checkbox/radio state
- [ ] Context-menu items of lazy backgrounds survive restart, dropped on install/update
- [ ] Extension keyboard shortcuts bound from core's resolution (browser keys win, user overrides, suggested keys in install order), while a Vsesvit window is focused
- [ ] `_execute_action` shortcuts open the popup or click the action
- [ ] Named commands fire `commands.onCommand(name, tab)`; `commands.getAll` reports current shortcuts
- [ ] Settings > Shortcuts lists extension shortcuts (assign/reset)
- [ ] `chrome.notifications` shown as desktop notifications (Chrome's Linux layout, max 2 buttons, icon)
- [ ] Notification Settings button opens the Extensions dialog on that extension
- [ ] Per-extension Notifications switch (synced `extensions.notifications_off`) closes notifications and refuses new ones; `onPermissionLevelChanged`
- [ ] declarativeNetRequest static/dynamic/session rules, enabled rulesets, persistence rules, Chrome limits and errors
- [ ] uBlock Origin Lite per-site "no filtering" (`allowAllRequests` dynamic rule) works
- [ ] DNR redirect/modifyHeaders limited to host permissions
- [ ] `chrome.webNavigation` events for every frame, URL filters, `getFrame/getAllFrames`
- [ ] `chrome.cookies` over the profile's store with Chrome's errors, host permissions and `onChanged`
- [ ] Extensions cannot set cookies for sites the user set to Block
- [ ] Private cookie store "1" visible only to extensions allowed in private, while a private window is open
- [ ] Optional permissions: `permissions.request` prompt (“X” has requested additional permissions. / It could: / Deny / Allow) over the right window, during a user gesture
- [ ] `permissions.getAll/contains/remove`, `onAdded/onRemoved`; grants widen access at once; grants per profile, not synced, forgotten on uninstall
- [ ] activeTab granted by action click, action shortcut, named command, context-menu click; revoked on origin change/close
- [ ] "Allow in private windows" switch (synced `extensions.allowed_in_private`), applied to open private tabs
- [ ] Extensions dialog notice of what the runtime does not support (list rebuilt for Chromium)
- [ ] Install prompt includes the unsupported-features line (if still relevant)
- [ ] Tracking protection still blocks the tracker list (no longer through `dnr.rs`)
- [ ] Web pages cannot navigate to non-web-accessible extension pages; only the browser opens `view-source:`
- [ ] Gecko-id (AMO/XPI) extensions: decide what id, `browser.*` and Firefox-only APIs they get
- [ ] MV2 extensions: decide whether they can still run
- [ ] Self-test extension checks re-pass: install_prompt, install_crx, engine_loaded_extension, content_script, extension_port, dnr_blocked, dnr_site_allowed, dynamic_content_script, extension_windows, extension_web_navigation, extension_cookies, popup, context_menus, extension_commands, extension_notifications, extension_update, private_window (extension part), tracking_protection, cws_install
- [ ] Remove `vsesvit-webext` from `[workspace.dependencies]`, drop `webkit`/`javascriptcore` workspace deps when GTK goes, update PLAN.md/README references
