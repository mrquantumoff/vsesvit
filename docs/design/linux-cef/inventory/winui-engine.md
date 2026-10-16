# Inventory: how the WinUI shell drives a Chromium engine (WebView2), mapped onto CEF

This is the reference for the product behaviour that `crates/vsesvit-winui` implements on top of
a Chromium engine (WebView2, Chromium 154), so that the GPUI + CEF Linux shell can reproduce every
user-visible engine feature without reading the old code. For each feature it lists what the user
sees, the `vsesvit_core` contract the shell calls, the persistent state, the classification of each
engine-touching piece with its natural CEF equivalent, and how it meets other modules and the
self-test. It ends with a checklist.

Checkout read: `/home/dy/git/vsesvit-cef` (branch `linux-cef-gpui`). CEF API names were checked
against `cef-dll-sys 154.5.0+154.0.34` (`src/bindings/x86_64_unknown_linux_gnu.rs`, the C API that
`cef 154.5.0+154.0.34` wraps). The bundled Chromium is `154.0.8037.98` (`CEF_VERSION`,
`CHROME_VERSION_*` constants).

## 0. Conventions

Classification tags (one or more per engine-touching piece):

- **(KEEP)**: product behaviour the CEF shell must reproduce.
- **(WEBVIEW2-ONLY)**: a WebView2 workaround or limitation that disappears with CEF. It is the WinUI
  counterpart of the task's (WEBKIT-ONLY). Most of these exist because WebView2 hides an API that
  CEF exposes directly.
- **(WEBKIT-ONLY)**: noted only where the Windows code or PLAN.md contrasts with a WebKitGTK limit
  that goes away with CEF.
- **(NEEDS-CEF)**: needs a CEF equivalent. The likely handler or API is named.

Evidence markers:

- **[V]**: verified by reading the Vsesvit source, or the CEF bindings and their doc comments.
- **[I]**: inference. Chromium or CEF behaviour that is not confirmed in code on disk, or a mapping
  proposal. Each one needs a spike.

"Chrome style" and "Alloy style" are CEF's two runtime styles (`cef_runtime_style_t`:
`CEF_RUNTIME_STYLE_CHROME = 1`, `CEF_RUNTIME_STYLE_ALLOY = 2` [V]). Several default behaviours
differ between them (§17.1). Which style the new shell uses is outside this inventory.

Source files covered (all under `crates/vsesvit-winui/src/`): `engine.rs`, `tab.rs`, `browser.rs`,
`trackers.rs`, `cookies.rs`, `downloads.rs`, `permissions.rs`, `capturing.rs`, `zoom.rs`,
`connection.rs`, `session.rs`, `shortcuts.rs` (page script part), `media.rs`, `store.rs`,
`window/{mod,trackers,cookies,permissions,download_warning,progress,media}.rs`,
`dialogs/{settings,site_data,downloads,about}.rs`, `js/{capture,media,store,popup}.js`,
`selftest.rs`, `selftest/{cookie,memory_saver,download,page,private,shortcut}_checks.rs`,
`automation/mod.rs`, `report.rs`. Core modules: `vsesvit_core::{trackers, cookies, https_only,
downloads, permissions, memory_saver, view_source, zoom, session, private, favicons, history,
new_tab, certificate, clean_url, prefs::keys}`.

---

## 1. Engine bootstrap and per-tab plumbing (every feature below depends on it)

### 1.1 What the user sees

- If the engine cannot start, an interactive run shows a message box: "Vsesvit could not start the
  web engine.\n\n{error}\n\nVsesvit needs the Microsoft Edge WebView2 Runtime." The process then
  exits with code 1 (`browser::launch`) [V].
- Settings > General > System has "Use smooth scrolling" and "Use graphics acceleration when
  available", each captioned "Takes effect when you restart Vsesvit." (`dialogs/settings.rs`) [V].
- About shows "Microsoft Edge WebView2 {BrowserVersionString}" (`dialogs/about.rs`) [V]. The CEF
  shell would show the CEF/Chromium version instead (`CEF_VERSION` [V]).

### 1.2 Core contract

- `Profile::open(dir, OpenOptions { chrome_version, .. })`. `chrome_version` is the engine's
  Chromium version (`engine::available_version()` then `engine::chromium_version()`, the first
  word when it has four numeric parts). Core sends it to the Chrome Web Store (prodversion) for
  downloads and updates (`lib.rs::open_profile`) [V]. CEF equivalent: `CHROME_VERSION_MAJOR`,
  `CHROME_VERSION_MINOR`, `CHROME_VERSION_BUILD` and `CHROME_VERSION_PATCH`, which give
  `154.0.8037.98` [V].
- `profile.paths().engine_data` (`<profile>/engine/`) is the engine's user data folder [V]. Its doc
  says: "WebView2 user data folder, or WebKit `NetworkSession` data dir". `engine_cache`
  (`<cache_dir>/Vsesvit/<name>/`) is WebKit only.
- `keys::SMOOTH_SCROLLING` (`"scrolling.smooth"`, Synced, default true) and
  `keys::HARDWARE_ACCELERATION` (`"system.hardware_acceleration"`, Local, default true) are read
  only at engine creation (`engine::STARTUP_SWITCHES`, `browser_arguments`) [V].

### 1.3 Engine pieces and classification

| WinUI piece | Class | CEF equivalent |
|---|---|---|
| One `CoreWebView2Environment` per process, `CreateWithOptionsAsync(user_data = engine_data)`, `SetAreBrowserExtensionsEnabled(true)` | (KEEP) one engine per profile process; (NEEDS-CEF) | `CefSettings.root_cache_path` plus `CefRequestContextSettings.cache_path`. The doc says it "must be an absolute path that is either equal to or a child directory of CefSettings.root_cache_path" [V]. Extensions: Chrome's extension system (Chrome style) [I] |
| Off prefs become `--disable-smooth-scrolling` and `--disable-gpu` | (KEEP) | Same Chromium switches, through `CefApp::OnBeforeCommandLineProcessing` or the command line [I] |
| Scripted runs (`--self-test`, `--ui-smoke`) add `engine::SCRIPTED_ARGUMENTS` = `--use-fake-device-for-media-stream --auto-select-desktop-capture-source="Entire screen"`. They deliberately avoid `--use-fake-ui-for-media-stream`, so permission requests still reach the shell | (KEEP) | Same switches [I]. On Wayland, desktop capture goes through PipeWire and the portal, and `--auto-select-desktop-capture-source` may not apply [I] |
| Built-in WebView2 extensions left alone (`BUILTIN_EXTENSIONS`: `dgiklkfkllikcanfonkcabmbdfmgleag` Clipboard, `mhjfbmdgcfjbbpaeojofohoefgiehjai` Edge PDF viewer) | (WEBVIEW2-ONLY) | Chrome's component extensions (PDF viewer) [I] |
| `app_local_factory` loads `Microsoft.Web.WebView2.Core.dll` next to the exe | (WEBVIEW2-ONLY) | none |
| `engine_path` / `extended_length`: extended-length paths for extension folders past `MAX_PATH` | (WEBVIEW2-ONLY), Windows path limits | none |

### 1.4 Per-tab engine lifecycle (`Tab::start`, `Tab::ensure_core`) [V]

The order matters and is product behaviour:

1. Create the engine view in the profile of the tab's kind (`Engine::ensure_view`). Private tabs
   use controller options `SetIsInPrivateModeEnabled(true)` (§13).
2. Settings: `SetAreDevToolsEnabled(true)` and `SetIsWebMessageEnabled(false)` (no
   `chrome.webview` bridge). Autofill: `SetIsPasswordAutosaveEnabled(false)` and
   `SetIsGeneralAutofillEnabled(keys::AUTOFILL_FORMS)` (§17.7).
3. `Engine::set_up_profile` runs once per profile (normal, and once per private session). It sets
   `SetPreferredTrackingPreventionLevel(None)`, then for a normal tab runs
   `cookies::clear_at_start` *before any page loads*. Other views that start meanwhile wait on a
   waker list, and a failed setup is retried by the next view.
4. `wire`: subscribe to every engine event (table in §1.5).
5. `inject`: DevTools setup in the page's own session before the first document. That is
   `shortcuts::world_calls` (`Page.enable`, `Runtime.enable`,
   `Runtime.addBinding{name:"vsesvitShortcut", executionContextName:<secret world>}`,
   `Page.addScriptToEvaluateOnNewDocument{source:<page script>, worldName}`), then
   `Security.enable`, then three main-world `Page.addScriptToEvaluateOnNewDocument` calls
   (`store::MAIN_WORLD_SCRIPT`, `media::MAIN_WORLD_SCRIPT`, `capturing::MAIN_WORLD_SCRIPT`), then
   `Target.setAutoAttach` (`shortcuts::AUTO_ATTACH` = `{"autoAttach":true,
   "waitForDebuggerOnStart":true,"flatten":true,"filter":[{"type":"iframe"}]}`), then the cookie
   controls (§3), then the key sets.
6. Store the core handle, then `filter_trackers` (§2).
7. Load the initial content: `Initial::Url` uses `navigate`, `Initial::Blank` the new tab page
   (§17.11), and `Initial::Opener` fulfils a deferred `window.open` (§17.2).

Each cross-site frame (an OOPIF) gets its own DevTools session through `Target.attachedToTarget`.
The shell sets up the shortcut world and the cookie controls in that session, then sends
`Runtime.runIfWaitingForDebugger` (`Tab::attached`). `Target.detachedFromTarget` drops the
session's state [V].

CEF notes:

- **(NEEDS-CEF)** DevTools: `CefBrowserHost::ExecuteDevToolsMethod(message_id, method, params)`,
  `SendDevToolsMessage(raw JSON)` and `AddDevToolsMessageObserver(observer)`, with observer methods
  `OnDevToolsMethodResult(message_id, success, result)`, `OnDevToolsEvent(method, params)`,
  `OnDevToolsMessage`, `OnDevToolsAgentAttached` and `OnDevToolsAgentDetached` [V].
  `ExecuteDevToolsMethod` takes no session id. Flattened child sessions (OOPIFs) need
  `SendDevToolsMessage` with a top-level `"sessionId"` field [I, CDP flatten protocol].
- **(NEEDS-CEF)** Document-start ordering. A browser created with a URL starts navigating at once,
  so CDP scripts added afterwards can miss the first document [I]. WebView2 avoided this by setting
  everything up before the first `Navigate`, and for popups before `SetNewWindow`. A CEF equivalent
  is to create the browser on an empty page, set up, then call `CefFrame::LoadURL`, or inject in
  the renderer with `CefRenderProcessHandler::OnContextCreated` [I]. Popups created through
  `CefLifeSpanHandler::OnBeforePopup` navigate immediately, which is the same risk.
- `Tab::eval` (`ExecuteScriptAsync`, JSON result) has no direct CEF counterpart:
  `CefFrame::ExecuteJavaScript` returns nothing [V, `_cef_frame_t` has `execute_java_script` only].
  CDP `Runtime.evaluate{expression, returnByValue:true}` gives results [I].

### 1.5 WebView2 event to CEF callback map (the tab's `wire`) [V for both sides]

| WebView2 event or call (Tab) | What Vsesvit does with it | CEF equivalent |
|---|---|---|
| `NavigationStarting(Uri, IsRedirected, NavigationId, SetCancel)` | HTTPS-only gate (§4); tracker page state (§2); cookie controls for the target site (§3); `requested` URL; wake from sleep (§7); transition `starting`; `Load::Started`; security report set aside | `CefRequestHandler::OnBeforeBrowse(browser, frame, request, user_gesture, is_redirect)` with main frame only; return true to cancel. A cancel produces `OnLoadError(ERR_ABORTED = -3)` [V doc] |
| `ContentLoading` (new document) | HTTPS-only `committed`; `Load::Committed`; security `new_document`; trackers `new_document` (count reset); `committed(NewDocument)` | `CefLoadHandler::OnLoadStart(frame.is_main, transition_type)`, "called after a navigation has been committed" [V doc] |
| `SourceChanged(IsNewDocument=false)` | same-document commit (fragment, History API): history visit, grants | `CefDisplayHandler::OnAddressChange` for the main frame. `OnLoadStart` "will not be called for same page navigations" [V doc] |
| `HistoryChanged`, `CanGoBack/CanGoForward` | back and forward buttons | `CefLoadHandler::OnLoadingStateChange(isLoading, canGoBack, canGoForward)` [V] |
| `NavigationCompleted(IsSuccess, WebErrorStatus, NavigationId)` | ignore the navigation stopped for HTTPS-only; transition `completed`; `Load::Idle`; clear `requested` if nothing ever committed; HTTPS-only `finished(ok)` (OperationCanceled counts as ok); delete Block-site cookies (§3) | `OnLoadEnd(frame, httpStatusCode)` / `OnLoadError(frame, errorCode, errorText, failedUrl)` plus `OnLoadingStateChange(isLoading=false)` [V] |
| `DocumentTitleChanged` | title via `display_title`; history title | `CefDisplayHandler::OnTitleChange` [V] |
| `IsDocumentPlayingAudioChanged`, `IsMutedChanged`, `SetIsMuted` | tab speaker; media player; Memory Saver | `CefBrowserHost::SetAudioMuted` / `IsAudioMuted` [V]. **No CEF callback for "audible"** found [V absence in `_cef_display_handler_t`]. `CefAudioHandler` *captures* streams (its doc: "Return true (1) to proceed with audio stream capture") [V], which may divert output [I] |
| `FaviconChanged` + `GetFaviconAsync(Png)` | tab icon; bookmark icon store (§15) | `CefDisplayHandler::OnFaviconURLChange(icon_urls)` + `CefBrowserHost::DownloadImage(url, is_favicon, max_image_size, bypass_cache, callback)` → `CefImage::GetAsPNG` [V] |
| `ContainsFullScreenElementChanged` | window fullscreen (hide toolbar, bars) | `CefDisplayHandler::OnFullscreenModeChange(fullscreen)`. With Alloy style "the client is responsible for triggering the fullscreen transition" [V doc] |
| `NewWindowRequested(IsUserInitiated, Uri, GetDeferral, SetNewWindow)` | pop-up blocker; opener tab (§17.2) | `CefLifeSpanHandler::OnBeforePopup(…, target_url, target_disposition, user_gesture, popup_features, windowInfo, client, settings, extra_info, no_javascript_access)` + `OnAfterCreated`; `CefRequestHandler::OnOpenURLFromTab` for Ctrl/middle-click [V] |
| `WindowCloseRequested` | `window.close()` closes the tab (on the next turn) | `CefLifeSpanHandler::DoClose` / `OnBeforeClose` [V] |
| `DownloadStarting` (`ICoreWebView2_4`) | downloads (§5) | `CefDownloadHandler::CanDownload` / `OnBeforeDownload` / `OnDownloadUpdated` [V] |
| `IsDefaultDownloadDialogOpenChanged` → `CloseDefaultDownloadDialog` | hide Edge's flyout (even for Save As) | (WEBVIEW2-ONLY) |
| `PermissionRequested` | site permissions (§6) | `CefPermissionHandler::OnShowPermissionPrompt` / `OnRequestMediaAccessPermission` / `OnDismissPermissionPrompt` [V] |
| `ScreenCaptureStarting(OriginalSourceFrameInfo, SetCancel)` | blocked screen share → cancel (§6) | `OnRequestMediaAccessPermission` with `CEF_MEDIA_PERMISSION_DESKTOP_VIDEO_CAPTURE` (8) / `_DESKTOP_AUDIO_CAPTURE` (4) [V] |
| `ContextMenuRequested` (`ICoreWebView2_11`) | four custom items (§17.3) | `CefContextMenuHandler::OnBeforeContextMenu(params, model)` / `OnContextMenuCommand` / `RunContextMenu` [V] |
| `WebResourceRequested` + `AddWebResourceRequestedFilter` | tracker blocking (§2) | `CefRequestHandler::GetResourceRequestHandler` → `CefResourceRequestHandler::OnBeforeResourceLoad` (RV_CANCEL) or `GetResourceHandler` [V]. **Runs on the browser IO thread** [V doc] |
| `ProcessFailed` | logged only ("engine process failed, kind N") | `CefRequestHandler::OnRenderProcessTerminated(status, error_code, error_string)` / `OnRenderProcessUnresponsive` [V] |
| DevTools events `Runtime.executionContext*`, `Runtime.bindingCalled`, `Security.visibleSecurityStateChanged`, `Target.(de)attachedToTarget` | shortcut world, page messages, security popup, frame sessions | `CefDevToolsMessageObserver::OnDevToolsEvent` [V] |

---

## 2. Tracker blocking via request interception

### 2.1 What the user sees

- Settings > Privacy has the "Tracking protection" combo box (Off, Standard, Strict) with the
  level's description below it: `TrackingProtection::label()` / `description()`. The texts are
  "Sites can load trackers", "Blocks known advertising and analytics trackers on other sites" and
  "Also blocks social media trackers, such as Like and Share buttons. Some sites may not work" [V].
- The site-info popup (§12) has a `ToggleSwitch` with header `trackers::TITLE` ("Tracking
  protection") and a status line `trackers::site_status(on, Some(blocked))`. The status reads "Off
  for this site", "No trackers blocked on this page", "1 tracker blocked on this page" or "{n}
  trackers blocked on this page" [V]. The count follows the page live while the popup is open
  (`show_tracking_status` on every chrome refresh). Windows can count, but WebKitGTK reports no
  hits, so Linux showed "Blocking known trackers on this site" (`site_status(true, None)`). With
  CEF the count becomes possible on Linux (WEBKIT-ONLY limit gone).
- The switch is hidden on pages of no web site (only `http://` and `https://` origins), while the
  setting is Off everywhere, and in private windows (`window/trackers.rs::tracking_status`) [V].
- Toggling the switch stores the exception and **reloads the page** under the new setting, as
  Firefox does (`Tab::set_tracking_protection`) [V].
- Blocked requests get an empty `403 Forbidden` response [V].

### 2.2 Core contract [V]

- `TrackerList::bundled()` loads `crates/vsesvit-core/src/trackers.json` (MIT, by company).
  `TrackerList::domains()` lists every tracker domain. `TrackerList::blocks(level, page_host,
  request_host) -> Option<&str>` returns the matched list domain when blocked. A company's
  trackers are first party on its own sites.
- `TrackerList::with_tracker(domain, Category)` is used by the self-test (adds `localhost` as
  Analytics). `TrackerList::dnr_rules(level, allowed)` was Linux's declarativeNetRequest route and
  is not needed for request interception.
- `trackers::level_for(p, origin) -> TrackingProtection`: the user's level, or Off on an excepted
  site. `trackers::allowed(p, origin)` / `trackers::set_allowed(p, origin, bool)` store
  `Permission::Trackers = Allow` in site permissions (synced). `trackers::allowed_sites(p)`.
- `p.prefs().get(&keys::TRACKING_PROTECTION)` is `"privacy.tracking_protection"`, Synced, default
  Standard.

### 2.3 Behaviour details (`trackers.rs::Protection`) [V]

- Each tab keeps `page_host`, `level`, `blocked: BTreeSet<String>` (list domains blocked on the
  current page) and `filtered` (domains it registered filters for).
- `navigation_starting(url, level)` runs on every main-frame navigation start and redirect. It sets
  the page host and the level from `level_for(origin)`. Requests are judged by the *tab's main
  page*, not by the frame's own site. That is a known difference from WebKit, which judged by
  frame. A Google frame on another site can load Google trackers on Linux only.
- `new_document()` runs on each new document commit and clears the blocked set.
- `blocks(list, url)`: requests to the page's own host always load, the document included. When
  blocked, the domain is recorded and the tab notifies, so the popup count updates.
- Service and shared workers are deliberately **not** filtered
  (`CoreWebView2WebResourceRequestSourceKinds::Document` only). WebView2 raises their requests in
  every web view with a filter and no view can tell which page a worker serves.

### 2.4 Classification

- (KEEP) The per-tab, per-page decision through `TrackerList::blocks`, the blocked-domain count,
  the reset on new document, the per-site exception with reload, and the private window showing no
  switch.
- (WEBVIEW2-ONLY) `add_filters` with URI wildcards `*://{domain}?*` and `*://*.{domain}?*`
  (WebView2 raises events only for filtered URIs). Also `SetPreferredTrackingPreventionLevel(None)`.
  Edge tracking prevention does not exist in CEF.
- (NEEDS-CEF) Interception goes through `CefRequestHandler::GetResourceRequestHandler(browser,
  frame, request, is_navigation, is_download, request_initiator, disable_default_handling)` and
  then `CefResourceRequestHandler::OnBeforeResourceLoad`, which returns `RV_CANCEL`. A
  `GetResourceHandler` that serves a 403 would match the old response exactly [V API]. Both are
  called **on the IO thread** [V doc]. `Profile` is `!Send` and lives on the UI thread, so the
  decision inputs must be readable off the UI thread: the list (plain data), and per-browser page
  host and level [I]. Service-worker requests arrive with `browser`/`frame` NULL [V doc]. That
  matches the Windows choice of not judging them. `CefRequestContextHandler::GetResourceRequestHandler`
  also sees requests without a browser [V].

### 2.5 Interactions and self-test

- `Browser::trackers()` holds an `Rc<TrackerList>`. `Browser::set_trackers` (self-test) refilters
  open tabs.
- The site-info popup (§12) shares the switch with cookies and permissions.
- The popup switch calls `Tab::set_tracking_protection(on)`, then `trackers::set_allowed`, then
  `Tab::reload()`.
- Self-test `tracking_protection` [V]: add `localhost` as a tracker. `/trackers.html` at
  `127.0.0.1` must end titled `tracker blocked`, the fixture server must never see
  `/tracker/pixel.png`, and `tab.blocked_trackers() == ["localhost"]`. Then
  `set_tracking_protection(false)` must reload to `tracker loaded`, with the server seeing the
  pixel and `trackers::allowed(site)` true. Afterwards it restores the site and the bundled list.
  UI smoke `tracker_steps`: the popup switch on with the tracker counted, off stores the exception
  and loads the image, then the cookie choice in the same popup.

---

## 3. Cookie controls via the DevTools protocol

### 3.1 What the user sees

- Settings > Privacy has the "Third-party cookies" combo box with `ThirdPartyCookies::label()` and
  `description()` [V]:
  - "Allow third-party cookies": "Sites can use cookies from other sites they embed, which can
    track you across sites".
  - "Block third-party cookies in private windows" (**default**): "In private windows, sites can't
    use cookies from other sites they embed".
  - "Block third-party cookies": "Sites can't use cookies from other sites they embed. Some sites
    may not work".
- Under it is a "See all site data" button whose flyout is titled "Cookies and site data". It
  lists each site with stored cookies and its count, each with Delete, plus "Delete all". It is
  empty as "No site has stored cookies." (`dialogs/site_data.rs`). The list is read afresh from
  the engine on each opening.
- The site-info popup has a combo box headed `cookies::SITE_TITLE` ("Cookies and site data"). The
  choices are `cookies::site_choices(current, true)` = Default, Allow, Block, Clear on exit
  (Windows offers Allow because it can make a per-site third-party exception). The status line is
  `cookies::site_status(third_party_blocked, setting)` [V]:
  - "This site can't use cookies"
  - "This site's cookies and data are deleted when you close Vsesvit"
  - "This site and sites it embeds can use cookies"
  - "Third-party cookies are blocked on this site"
  - "This site can use cookies. Third-party cookies are blocked on it"
- A new choice applies in every tab, and the current page **reloads** under it
  (`window/cookies.rs::set_site_cookies`) [V].
- Private windows show no cookie rule in site info. Their tabs follow the stored rules and block
  third-party cookies as `ThirdPartyCookies::blocks(Private)` says [V].

### 3.2 Core contract [V]

- `cookies::third_party_blocked(p, browsing, top: Option<&Origin>) -> bool`: the pref's choice for
  `browsing`, lifted on a top site set to Allow.
- `cookies::setting(p, origin) -> Option<Setting>` / `cookies::set(p, origin, Option<Setting>)`
  store `Permission::Cookies` with Allow, Block or ClearOnExit (synced site permissions).
- `cookies::site_choices`, `cookies::choice_label`, `cookies::site_status`, `cookies::SITE_TITLE`.
- `cookies::site_rules(p) -> SiteRules`, with `blocked_hosts()` (sorted, deduped),
  `blocks_cookie(domain)`, `to_clear() -> &[Origin]` (Block plus ClearOnExit sites, in order),
  `clears_cookie(domain)` and `clears(name)` (the WebKit record name).
- `cookies::block_script(blocked_hosts) -> Option<String>`: a document-start script that makes
  `document.cookie` read `""` and ignore writes and deletes `cookieStore` on covered hosts (it goes
  by `self.origin`, so it also covers `about:blank` frames).
- `cookies::group_by_site(domains) -> Vec<SiteCookies { site, count }>` for the Settings list.
- `keys::THIRD_PARTY_COOKIES` is `"privacy.third_party_cookies"`, Synced, default BlockInPrivate.

### 3.3 Behaviour details (`cookies.rs`) [V]

- **Third-party blocking.** Every DevTools session of a tab (the page's own `""` and each attached
  frame session) gets `Network.enable{"maxTotalBufferSize":0,"maxResourceBufferSize":0}`
  (setCookieControls acts only with the Network domain on), then
  `Network.setCookieControls{"enableThirdPartyCookieRestriction": blocked}`. In the page session
  alone it blocks only HTTP cookies. Frame sessions also block `document.cookie` in third-party
  frames. `Sessions::navigation_starting` re-sends the control *before the engine's event
  returns* whenever the target site's answer differs, so a site set to Allow keeps its embedded
  sites' cookies.
- **Block sites.** WebView2 cannot stop a response from storing cookies (`Fetch` hides
  `Set-Cookie`), so three things happen. Every session gets `block_script` through
  `Page.addScriptToEvaluateOnNewDocument`, replaced atomically with generation counting and
  `removeScriptToEvaluateOnNewDocument`. Matching cookies are deleted when the rule is set, after
  **every** main-frame `NavigationCompleted` (`delete_blocked`), and at startup.
- **Clear on exit.** `clear_at_start` runs before any page loads, bounded to `START_CLEARING = 5
  s`. `ExitClearing::start` begins when the last tab or window is about to close, and
  `finish` waits up to `EXIT_CLEARING = 2 s` before the process exits. Clearing calls
  `Storage.clearDataForOrigin{origin, storageTypes:"all"}` per `to_clear()` origin, then deletes
  cookies with `clears_cookie(domain)` (subdomain cookies included).
- **Listing and deleting.** `Network.getAllCookies` is parsed into `Cookie { name, domain, path,
  partitionKey }`. Deletion uses `Network.deleteCookies{name, domain, path, partitionKey?}`;
  partitioned cookies must name their key.
- `cookies::changed(browser)` runs after any change (Settings, site info, sync with
  `PrefEffect::Cookies`, or `permissions::settings_changed`). It re-applies the choice and script
  in every tab and deletes the blocked sites' cookies.
- `any_core(browser)`: profile-wide calls go through any open normal tab's engine. There is none
  while only private windows are open.

### 3.4 Classification

- (KEEP) The three-way choice, per-site Allow/Block/Clear on exit, the private-window rules, reload
  on change, deletion at startup, after loads and at exit, and the site-data list with
  per-site/all delete.
- (WEBVIEW2-ONLY) Doing all of this through per-session CDP calls, the block script as the only JS
  defence, and post-load deletion to undo `Set-Cookie`. Each exists because WebView2 has no cookie
  policy API.
- (WEBKIT-ONLY) Linux's single accept policy per session ("no Allow exception") goes away. Chromium
  can make per-site exceptions, so `site_choices(current, true)` applies on Linux too.
- (NEEDS-CEF) Natural CEF equivalents, all [I] unless marked:
  - Per-site rules map to `CefRequestContext::SetContentSetting(requesting_url, top_level_url,
    CEF_CONTENT_SETTING_TYPE_COOKIES (0), value)`, where Allow = `CEF_CONTENT_SETTING_VALUE_ALLOW`,
    Block = `_BLOCK` and Clear on exit = `_SESSION_ONLY` (Chrome's own name for clear-on-exit)
    [V for the enum values]. A Chromium COOKIES block covers HTTP and JS cookie access, which could
    retire the block script and post-load deletion.
  - Per-request filtering: `CefResourceRequestHandler::GetCookieAccessFilter` →
    `CefCookieAccessFilter::CanSendCookie` / `CanSaveCookie` [V API]. It covers network cookies
    only, not `document.cookie`.
  - Third-party default: Chrome's profile preference (`profile.cookie_controls_mode`) through
    `CefRequestContext`'s preference manager (`SetPreference`) [I pref name]. The CDP route
    (`Network.setCookieControls`) also exists in CEF through DevTools.
  - Listing: `CefCookieManager::VisitAllCookies`. Note `cef_cookie_t` has **no partition key
    field** [V], so the CDP `Network.getAllCookies` / `Network.deleteCookies` route is needed for
    partitioned cookies. Deleting: `CefCookieManager::DeleteCookies(url, name, callback)` [V].
  - Clearing site storage: no CEF API. CDP `Storage.clearDataForOrigin` through any browser of the
    context. `CefRequestContext::ClearHttpCache` covers the cache [V].

### 3.5 Interactions and self-test

- Uses `permissions::settings_changed` (§6), which calls `cookies::changed`. Sync calls it through
  `Browser::sync_applied`. Lifecycle hooks: `Engine::set_up_profile` (startup clearing),
  `Browser::window_closing`, `Browser::tab_closing` and `Browser::window_closed` (exit clearing).
- Self-test `cookies` (60 s) [V]. `/cookies.html` at `127.0.0.1` sets `first=1` and frames
  `localhost`, which sets `third=1` (`SameSite=None; Secure`). The checks, in order:
  1. By default the frame reads `third=1`.
  2. With the choice set to Block and a reload, the frame does not read it and the page keeps
     `first=1`.
  3. With the site set to Allow, the frame reads `third=1` again.
  4. With the site set to Block, the page reads no cookie and the site's cookies (including
     `/set-cookie`'s `served`) are deleted after the load.
  5. With Clear on exit, `cookies::clear_sites` deletes what the page stored.
  It restores the default afterwards.
- `private_window` checks that the frame reads no `third=1` in a private window under the default,
  and that site info offers no cookie rule.

---

## 4. HTTPS-only in NavigationStarting

### 4.1 What the user sees

- Settings > Privacy has the "Always use secure connections" switch (`https_only::TITLE`) with the
  caption "Upgrade navigations to HTTPS and warn you before loading sites that don't support it"
  (`https_only::DESCRIPTION`). It is off by default [V].
- An http navigation to a public site (default port, not localhost, private or link-local, and no
  local TLD) loads its https URL instead. The user never sees the http page.
- When the https load fails (network, TLS or certificate error), or a redirect returns to http on
  an already upgraded host, the tab shows **core's warning page at the http address**. Its title is
  "Connection is not secure" (`WARNING_TITLE`) and its heading "This site doesn't support a secure
  connection". The body says the host (bold) may be watched, explains why the warning shows, and
  has a "Continue to site" button (`<a id="continue" href="{http url}">`) [V].
- The address bar shows the http URL while the warning is up (`refresh_url` substitutes
  `https.warning()`). The site icon reads "Not secure" because the page is http.
- "Continue to site" loads the http page and stores a permanent exception (synced). Unlike Chrome,
  it does not lapse after 15 days.
- **Reload** on the warning tries the https URL again, as Chrome does (`Tab::reload`, which
  navigates to the warning URL after `leave()`, so it is upgraded again) [V].
- The warning page is never a history visit and never gets a history title. The session saves the
  warning URL as the tab's URL [V].

### 4.2 Core contract [V]

- `https_only::upgrade(p, browsing, url, reach) -> Option<Url>`: `None` when the pref is off, the
  URL is not covered, or the site has an exception. Exceptions are read through
  `site_permissions_in(browsing)`, so a private session's are separate.
- `https_only::upgraded(url, Reach)`, with `Reach::Public` (normal) and `Reach::Everywhere` (the
  self-test reaches its plain-http `127.0.0.1` server).
- `https_only::allow(p, browsing, url)` stores `Permission::Http = Allow` (the private session for
  private windows). `https_only::allowed(p, browsing, origin)`.
- Per-tab state machine `https_only::Upgrades`:
  - `starting(url, Cause::{Link,Redirect,Other}, upgrade) -> Next::{Load, Upgrade(https),
    Warn(http), Allow(http)}`
  - `finished(ok) -> Option<Url>` (the http URL to warn about)
  - `committed()`
  - `leave()`: call before any shell-initiated navigation (typed, reload, back, forward)
  - `warning() -> Option<&Url>`
- `https_only::warning_page(url) -> String`: self-contained HTML with no script and every string
  escaped. It is meant to be loaded under the http URL.
- `keys::HTTPS_ONLY` is `"privacy.https_only"`, Synced, default false.

### 4.3 Behaviour details (`Tab::stopped_for_https` and friends) [V]

- On every main-frame `NavigationStarting`, the cause is `Redirect` if `IsRedirected`, else
  `Link`. The shell's own navigations call `leave()` first, so a non-redirect start that reaches
  the state machine is the page's. Windows never passes `Cause::Other`.
- `Browser::https_upgrade(browsing, url)` uses the browser-level `https_reach`.
- `Next::Upgrade(https)` and `Next::Warn(http)` cancel the navigation (`SetCancel(true)`), remember
  its `NavigationId` in `stopped` so its `NavigationCompleted` is ignored, and on the next turn
  either `core.Navigate(https)` or `show_https_warning(http)`. The warning loads with
  `NavigateToString(warning_page(url))`.
- `Next::Allow(url)` stores the exception and lets the navigation load.
- `NavigationCompleted`: `ok = IsSuccess || WebErrorStatus == OperationCanceled` (a user Stop is
  not a failed upgrade). `finished(ok)` may return a URL to warn about.
- `ContentLoading` calls `committed()`.
- `committed()` in the tab: while a warning shows, `Browser::session_changed()` runs instead of
  `navigation_committed` (no history visit), and `title_changed` skips the history title.
- `navigate`, `go_back` and `go_forward` call `https.leave()`.

### 4.4 Classification

- (KEEP) Everything core decides: the reach rules, exceptions in site permissions (synced), the
  warning page text and Continue behaviour, Reload retrying https, no history entry for the
  warning, and the shown address being the http URL.
- (WEBVIEW2-ONLY) `NavigationId` bookkeeping for the stopped navigation (CEF gives a cancel through
  the return value plus an `ERR_ABORTED` load error), and the "`Source` is empty for
  `NavigateToString`" handling.
- (WEBKIT-ONLY) Linux's restriction to upgrade "only what is surely the main frame's", because
  WebKit's `decide-policy` doesn't say which frame. CEF's `OnBeforeBrowse` gives the `frame`
  (`IsMain()`) and `is_redirect` [V], so the Windows semantics carry over.
- (NEEDS-CEF):
  - Gate: `CefRequestHandler::OnBeforeBrowse(browser, frame, request, user_gesture, is_redirect)`.
    Return true to cancel, then post `CefFrame::LoadURL(https)` [V API]. The cancel produces
    `CefLoadHandler::OnLoadError(errorCode = ERR_ABORTED (-3))` [V doc], which must be ignored
    like `stopped`.
  - Failure detection: `OnLoadError(main frame, errorCode != ERR_ABORTED)`. Certificate errors go
    through `CefRequestHandler::OnCertificateError` ("Return false (0) to cancel the request
    immediately") [V doc], then `OnLoadError` with an `ERR_CERT_*` code [I].
  - Showing the warning: **CEF has no NavigateToString / LoadString** (`_cef_frame_t` has
    `load_request` and `load_url` only) [V]. Candidates [I]: serve `warning_page` for the http URL
    from `CefResourceRequestHandler::GetResourceHandler` (the document then really is at the http
    URL, like Linux's `load_alternate_html`); CDP `Page.setDocumentContent`; or a `data:` URL with
    the shell overriding the displayed URL as Windows does. `request->GetTransitionType()`
    (`TT_LINK`, `TT_EXPLICIT`, `TT_RELOAD`, `TT_FORWARD_BACK_FLAG`, `TT_*_REDIRECT_FLAG`) [V] can
    refine the cause.
  - Chromium's own HTTPS-First mode (content setting types `HTTPS_ENFORCED` = 86 and `HTTP_ALLOWED`
    = 68 exist [V]) would duplicate core's exceptions. The core-driven behaviour is the product
    one.

### 4.5 Interactions and self-test

- Uses `vsesvit_core::private` (private exceptions), site permissions (§6 and Settings' site
  list, where the exception can be removed), history (§16) and session.
- Self-test `https_only` [V]. With the pref on and `Reach::Everywhere`, `/index.html` (plain http)
  is upgraded, the https attempt fails, and the tab must show `WARNING_TITLE` at the http URL. A
  script then clicks `#continue`, the page must load (`Vsesvit fixture`), and
  `https_only::allowed(Normal, site)` must be true. Afterwards it restores the exception, the pref
  and the reach.

---

## 5. Downloads (destination, dangerous files, Mark of the Web, pause/resume, private)

### 5.1 What the user sees

- **Toolbar Downloads button** (`window/chrome.rs`, `x:Name="Downloads"`): hidden until a download
  this window lists has started this session. Then it shows; a `ProgressRing` spins while one is
  running (`Indicator::{Hidden, Idle, Busy}`). It has no progress text [V].
- **Downloads view** (Ctrl+J, `Command::ShowDownloads`, its own window, `dialogs/downloads.rs`)
  [V]:
  - Rows newest first, up to `LISTED = 200`. Each shows the file name (`downloads::file_name`) and
    a status line from `downloads::status_line(d, live, exists)`, for example "1.3 MB/s - 3.2 MB of
    10 MB, 5 secs left", "0 B/s - 3.2 MB of 10 MB, Paused", "Interrupted · …", "This type of file
    can harm your device", "Deleted", "{size} · {host}", "Failed" or "Cancelled". A progress bar
    shows while the download is live.
  - Buttons per state (`Action::applies`):
    - InProgress: Pause, Cancel.
    - Paused or Interrupted: Resume, Cancel.
    - Unconfirmed: Keep, Discard.
    - Completed with the file present: Open, Show in folder, Remove from list.
    - Completed but deleted, or Failed: Remove from list.
    - Cancelled: Show in folder (if the file exists), Remove from list.
  - Header buttons "Open download folder" and "Clear list". Empty text: "Files you download appear
    here". Live updates come at most every 250 ms (`PROGRESS_INTERVAL`) plus a once-a-second tick.
- **Settings > General > Downloads**: "Download folder" (path, change, reset to default) and the
  "Ask where to save each file" switch [V].
- **Ask where to save**: a save dialog (`pickers::pick_save_file`, Windows `FileSavePicker`) opens
  in the download folder with the sanitized name. Cancelling cancels the download [V].
- **Dangerous file**: the file is written under `name.ext.unconfirmed`. When complete, the row
  reads "This type of file can harm your device" and **the window it came from** shows a flyout
  under the Downloads button (`window/download_warning.rs`). The flyout shows the file name
  (semibold, trimmed), the status line, and buttons **Keep** and **Discard** (the accent button).
  It opens `Transient` and takes no focus, so a key press cannot answer it [V].
- No Edge download flyout ever shows. `IsDefaultDownloadDialogOpenChanged` closes it, including
  for Save Page As, which raises no `DownloadStarting` [V].

### 5.2 Core contract [V]

- `p.downloads()` (`vsesvit_core::downloads::Downloads`):
  - `start(url, path, total, now_ms, browsing) -> Download` (private rows get negative ids and live
    in the private session)
  - `update(id, State, received, total)`
  - `list(limit, browsing)`
  - `remove(id)`
  - `keep(id) -> Option<PathBuf>` (moves the unconfirmed file to a free name with a hard link, else
    a rename)
  - `discard(id)`
  - `clear(browsing)`
  - `interrupt_stale() -> usize` (at startup: rows still live from the last run become Failed)
- `downloads::State::{InProgress, Paused, Interrupted, Unconfirmed, Completed, Failed,
  Cancelled}`, with `is_live()` and `is_final()`.
- `downloads::listed_in(started, window)`: private windows list all downloads, normal windows never
  private ones.
- `downloads::sanitize(suggested)`, `unique_destination(dir, name, exists)` (numbered `name (1).ext`
  that neither the name nor its unconfirmed path takes), `unconfirmed_path(path)`, and
  `is_dangerous(name, mime)`. The last uses the Windows and Linux tables, with `.jar`/`.jnlp` on
  both. Linux: `appimage, bash, csh, deb, desktop, flatpak, flatpakref, ksh, rpm, run, sh, snap,
  tcsh, zsh` plus the matching MIME types.
- `downloads::zone_identifier(url)`: `[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={http(s) url without
  credentials | about:internet}\r\n`.
- `downloads::Transfer` (a 10-second rate estimator with `update`, `restart` and `at(now) ->
  Progress`), `describe_size` and `status_line`.
- Prefs: `keys::DOWNLOADS_DIR` (`"downloads.directory"`, Local, `Option<PathBuf>`, None means the
  platform Downloads folder; reset with `prefs().reset`) and `keys::DOWNLOADS_ASK`
  (`"downloads.ask"`, Synced, default false).

### 5.3 Behaviour details (`downloads.rs`) [V]

- `Downloads::new` runs `interrupt_stale()` at startup. The default dir is the platform Downloads
  folder (`platform::downloads_folder()`), with temp as a fallback.
- `download_starting`: `SetHandled(true)`. The suggested name comes from `ResultFilePath`,
  sanitized. Without "ask", the path is `free_path(dir, name)`; `taken` means the file exists *or a
  running download has the path*. With "ask", the shell takes a deferral and shows the picker.
- `begin_download`: `dangerous = is_dangerous(file name, MimeType)`. A dangerous destination whose
  unconfirmed name another download holds is renumbered. `SetResultFilePath(written)`, where
  written is the unconfirmed path for dangerous files. Then `core.start(...)`, subscriptions to
  `BytesReceivedChanged` and `StateChanged`, and the `Live` entry with `window: Weak` (whose window
  will warn). A failure to place or record the download cancels it (`cancel_unplaced`).
- `list_state(engine state, interrupt reason, can_resume, dangerous)`:
  - Completed is Unconfirmed when dangerous, else Completed.
  - Interrupted with UserPaused is Paused; with UserCanceled it is Cancelled.
  - Any other interruption is Interrupted if resumable, else Failed.
  - WebView2 reports a pause as an interruption.
- Completion writes the **Mark of the Web**: an NTFS ADS `<file>:Zone.Identifier` created with
  `create_new` (skipped if the engine already wrote one). A kept file takes it along.
- Unconfirmed: `window.warn_about_download(&download)` on the originating window.
- `operate(id, f)` (pause, resume, cancel) applies `f` and then **re-reads the state itself**,
  whether or not the engine raises `StateChanged`.
- `end_private_downloads()` runs when the last private window closes. It cancels running private
  downloads and stops showing the private button flag.

### 5.4 Classification

- (KEEP) Destination choice, numbering, Ask, dangerous-file hold with Keep/Discard (flyout under
  the button plus view actions), stale-row interruption, private-session rows and button
  visibility, speed and time-left text, and pause/resume/cancel.
- (WEBKIT-ONLY) "WebKitGTK has no way to pause a download" (PLAN.md, and the self-test has no
  Linux `download_pause`). CEF has pause and resume.
- (WEBVIEW2-ONLY) Closing Edge's default download flyout; pause reported as an interruption; the
  NTFS `Zone.Identifier` mechanics.
- (NEEDS-CEF):
  - `CefDownloadHandler::CanDownload(browser, url, request_method)` [V].
    `OnBeforeDownload(browser, download_item, suggested_name, callback)` → `callback.Continue(path,
    show_dialog)`. Its doc: "Return false (0) to proceed with default handling (cancel with Alloy
    style, default download handling with Chrome style)" [V doc].
    `OnDownloadUpdated(browser, download_item, CefDownloadItemCallback)`, where the callback has
    `Cancel`, `Pause` and `Resume` [V].
  - `CefDownloadItem` has `IsInProgress`, `IsComplete`, `IsCanceled`, `IsInterrupted`,
    `GetInterruptReason`, `IsPaused`, `GetReceivedBytes`, `GetTotalBytes`, `GetCurrentSpeed`,
    `GetFullPath`, `GetUrl`, `GetOriginalUrl`, `GetSuggestedFileName`, `GetMimeType` and `GetId`
    [V].
  - Ask where to save: `show_dialog = true` uses CEF's own dialog [I: on Linux that depends on
    CEF's GTK or portal dialog support], or the shell shows its own picker (an xdg-desktop-portal
    FileChooser) and passes the chosen path.
  - Mark of the Web on Linux: no NTFS streams. Chromium on Linux marks downloads with xattrs
    `user.xdg.origin.url` and `user.xdg.referrer.url` [I], and it is unconfirmed whether CEF's
    download path applies them. Core has no Linux counterpart of `zone_identifier`, and the GTK
    shell wrote none.

### 5.5 Interactions and self-test

- Private session end (§13), `Browser::windows` (indicators), the Settings and Downloads views
  (subscribers), `platform::open_in_shell` / `show_in_folder` (Open, Show in folder).
- Self-test checks [V]:
  - `download`: `/download.bin` into `<out>/downloads`. The bytes must match, the entry must be
    Completed, `received` must match, the button must show, and the view must have one row
    (`downloads.png`).
  - `download_pause` (Windows only today): `/stalled.bin` (1 MB announced, 1 KB sent) is paused
    (Paused, live counts kept, indicator Idle), resumed (InProgress, Busy) and cancelled.
  - `download_speed`: `/slow.bin` reads speed, bytes and time left
    (`testkit::reads_speed_and_time_left`), then `/stalled.bin` reads `0 B/s - 1.0 KB of 1.0 MB`.
  - `download_safety`: `/dangerous.bat` (Linux: `/dangerous.sh`) is held as `.unconfirmed` and
    nothing is at the name. The warning flyout appears, Keep moves the file (Windows: the
    `Zone.Identifier` has `ZoneId=3`), a second download becomes `dangerous (1).<ext>`, and Discard
    deletes its file and entry.
  - `private_window`: the private download row is negative, not listed for normal windows, and the
    button shows in the private window only. After closing, no private rows remain and the file
    stays.

---

## 6. Permissions (prompts, site-info section, in-use indicators, screen share, engine mirror)

### 6.1 What the user sees

- **Prompt bubble** under the site-info button (`window/permissions.rs`): width 320, a heading
  (`Prompt.heading`, for example "Use your camera and microphone?"), a body ("“meet.example.com”
  wants to use your camera and microphone."), and stacked full-width buttons in core's order with
  the first as the accent: "Allow while visiting the site", "Allow this time", "Never allow", "Not
  now" [V]. Rules:
  - One prompt at a time, for the **selected** tab. A request in a background tab waits (deferred)
    until its tab is shown, as in Chrome.
  - Focus goes to the heading, never an answer. Answers are ignored for `INPUT_GUARD = 500 ms`
    after the prompt shows or changes.
  - Clicks on the tabs and the address bar pass through while it is open
    (`SetOverlayInputPassThroughElement`).
  - Esc, "Not now" or a click elsewhere (with a `CLOSE_GRACE = 250 ms`) **dismiss** it only if its
    tab is still selected. Losing the selection or opening the site-info popup *withdraws* it, and
    it comes back.
  - A cross-origin commit or a closed tab denies what waited.
- **Address bar capture button**: shown while the page captures. Its tooltip is
  `Capturing::description()` ("Using your camera and microphone", "Sharing your screen and using
  your microphone", …). A click opens the site-info popup [V].
- **Screen sharing bar** (`InfoBar`): "Sharing your screen with {host}" with a "Stop sharing"
  button [V].
- **Tab list in-use icon**: camera or microphone in red ("recording"), screen not red
  (`tab_header::capture_glyph`) [V].
- **Site-info popup > "Permissions" section**: one row per `site_rows(...)` entry. Each row has the
  permission glyph, its label (`Permission::label()`: Camera, Microphone, Location, Notifications,
  Screen sharing, Clipboard, MIDI devices, Picture-in-picture), "In use" plus a **Stop** button
  while live, and a combo box (width 168) of `SiteChoice` labels: "Allowed this time", "Ask",
  "Allow", "Block". When anything is stored there is a "Reset permissions" button. The section
  refills while open, keeping keyboard focus [V].
- **Settings > Site permissions**: every stored setting by site with an Allow/Block (or Clear on
  exit) choice and remove. The caption is "What you allowed or blocked for each site. Other sites
  ask before they use your camera, microphone, location and more."; empty text "Sites you allow or
  block show here." (`dialogs/site_permissions.rs`) [V].
- Screen share: when not blocked, the engine's own picker is the prompt, as in Chrome. When
  blocked, no picker shows [V].

### 6.2 Core contract [V]

- `p.site_permissions_in(browsing)` → `SitePermissions` with:
  - `decide(origin, &[Permission], &TabGrants) -> Decision::{Allow, Block, Ask(Vec)}`. Any block
    wins; never-asked permissions block unless allowed; private windows block Notifications
    without asking.
  - `answer(origin, asked, Answer, &mut TabGrants) -> Result<bool>`.
  - `get` and `set(origin, p, Option<Setting>)`. Setting Allow on ScreenShare errors with
    `Error::AlwaysAsks`.
  - `for_site`, `all`, `by_site` and `reset_site` (keeps Trackers and Cookies).
- `permissions::prompt(origin, &[Permission]) -> Prompt { heading, body, answers }`,
  `Answer::label()`, `site_rows(storable, stored, granted, capturing) -> Vec<SiteRow>`,
  `SiteChoice::{label, setting}`, `Capturing { camera, microphone, screen }` with
  `{any, uses, description}`, and `TabGrants` with `{allows, committed, granted, revoke}`.
- `Permission::remembers_allow()` is false for ScreenShare (only Block is remembered, and "Allow
  this time" covers one request). `Permission::asks()` is false for PictureInPicture, Trackers,
  Http and Cookies.
- `Origin::parse` / `Origin::of`. Opaque origins have none, so only grants apply.

### 6.3 Behaviour details (`permissions.rs`) [V]

- Engine kinds handled (`ENGINE_KINDS`): Camera, Microphone, Geolocation (Location),
  Notifications, ClipboardRead, MidiSystemExclusiveMessages (Midi). Other kinds keep the engine
  default (for example Autoplay).
- Every handled request gets `SetSavesInProfile(false)`, so the engine never remembers answers and
  core is the only record. Answers go through a deferral.
- Camera and microphone from one `getUserMedia` arrive as two requests in two turns. A request
  joins the last waiting entry of its tab if it is the same origin, even one already shown (which
  then asks for both).
- `next_prompt` skips entries the settings now decide (the user answered the same site in another
  tab).
- `settings_changed(browser)` runs after any stored change: a prompt answer, site info, Settings or
  sync. For every tab it does five things:
  1. Settles newly decided waiting requests.
  2. Refreshes `site_may_capture`.
  3. Stops captures now blocked (`stop_capture`).
  4. Restarts capture polling.
  5. Refreshes picture-in-picture (`window.show_media()`).
  It then calls `mirror(browser)` and `cookies::changed`.
- **Engine mirror** (`mirror`, `engine_changes`): reads `GetNonDefaultPermissionSettingsAsync` and
  sets `SetPermissionStateAsync(kind, origin, Allow/Deny/Default)`. The engine then holds exactly
  the stored settings (normal profile only), so pages see the right `Notification.permission` and
  `navigator.permissions.query`. Runs are serialized, and a change during a run triggers one more.
  ClearOnExit maps to Allow.
- **Capture tracking** (`capturing.rs`, `js/capture.js`): WebView2 reports no capture state. A
  main-world script in top documents wraps `getUserMedia`, `getDisplayMedia` and `clone`, tracks
  live tracks weakly, and exposes a frozen `window.__vsesvitCapture.{state(), stop(source)}`. The
  tab polls it every `CAPTURE_POLL = 500 ms` while `watched()` holds (capturing, or the site may
  capture, or within `CAPTURE_GRACE = 10 s` after a grant or screen-capture start).
  `stop_capture(p)` runs `stop("camera"|"microphone"|"screen")`, which ends tracks and dispatches
  `ended`.
- **Taking back**: a Block, a Reset, or removing an Allow (here, in Settings or by sync) for a
  capture the shell let happen **reloads** every affected tab of that site. Only tearing down the
  document is sure to release the device (`must_reload`, `reload_allowed_captures`,
  `reload_taken_back`, `sync_changed` with `moved_settings`). "Stop" only runs the script.
- `screen_capture_starting`: origin = `OriginalSourceFrameInfo.Source` or the tab URL. If blocked,
  `SetCancel(true)`; else watch.

### 6.4 Classification

- (KEEP) The core decision, prompt and answer flow; one prompt per window for the selected tab;
  deferral; the input guard; dismiss versus withdraw semantics; deny on navigation or close; the
  site-info rows with Stop and Reset; the in-use indicators; reload on take-back; Settings site
  list; the private-session answers; and blocking notifications in private windows.
- (WEBVIEW2-ONLY) Merging two `getUserMedia` requests across turns; `SetSavesInProfile(false)`;
  polling a script for capture state *where CEF can report it*.
- (NEEDS-CEF):
  - `CefPermissionHandler::OnShowPermissionPrompt(browser, prompt_id, requesting_origin,
    requested_permissions bitmask, CefPermissionPromptCallback)`. Return true and call
    `Continue(CEF_PERMISSION_RESULT_ACCEPT | _DENY | _DISMISS | _IGNORE)` now or later
    [V]. `OnDismissPermissionPrompt(prompt_id, result)` reports prompts dismissed by navigation or
    closure [V doc]. Type bits [V]: `CEF_PERMISSION_TYPE_CAMERA_STREAM` (4), `MIC_STREAM` (4096),
    `GEOLOCATION` (256), `NOTIFICATIONS` (32768), `CLIPBOARD` (16), `MIDI_SYSEX` (8192). Others
    (`MULTIPLE_DOWNLOADS`, `STORAGE_ACCESS`, `LOCAL_FONTS`, `WINDOW_MANAGEMENT`, …) need a policy:
    Windows kept the engine default for unmapped kinds.
  - `CefPermissionHandler::OnRequestMediaAccessPermission(browser, frame, requesting_origin,
    requested_permissions, CefMediaAccessCallback)`. For getUserMedia, "allowed_permissions must
    match required_permissions" [V doc], so one callback carries both camera and microphone. Its
    defaults: "With Chrome style, default handling will display the permission request UI. With
    Alloy style, default handling will deny the request" [V doc]. Screen share arrives as
    `CEF_MEDIA_PERMISSION_DESKTOP_VIDEO_CAPTURE` / `_DESKTOP_AUDIO_CAPTURE` [V].
  - Capture state: `CefDisplayHandler::OnMediaAccessChange(browser, has_video_access,
    has_audio_access)` [V API]. It does not separate camera from screen [I], so `capture.js`
    polling may still be needed for the three-way state and for Stop.
  - Engine mirror: `CefRequestContext::SetContentSetting(requesting_url, top_level_url, type,
    value)` / `GetContentSetting` [V]. Types: `GEOLOCATION` (4), `NOTIFICATIONS` (5),
    `MEDIASTREAM_MIC` (8), `MEDIASTREAM_CAMERA` (9), `MIDI_SYSEX` (13), `CLIPBOARD_READ_WRITE` (51),
    `DISPLAY_CAPTURE` (64). Values `DEFAULT`, `ALLOW`, `BLOCK` [V]. Whether Chromium persists an
    ACCEPT given through the prompt callback as its own content setting (WebView2's
    `SavesInProfile(true)` analogue) is unknown [I], and matters for "core is the only record".
  - The screen picker on Linux/Wayland comes from the xdg-desktop-portal ScreenCast picker through
    Chromium's PipeWire capturer [I], instead of WebView2's picker.

### 6.5 Interactions and self-test

- Interactions: Memory Saver (capturing keeps a tab awake; notifying sites keep tabs awake through
  `Sweep::new`), media player and picture-in-picture (`Permission::PictureInPicture`), cookies
  (`settings_changed` runs `cookies::changed`), sync (`Browser::sync_applied` runs
  `settings_changed` and `sync_changed`), and the site-info popup (§12).
- UI smoke `permission_steps` [V]:
  - A notification prompt and its answer, which the next request and a reload remember.
  - Site info's section blocking it.
  - Fake camera and microphone as **one** prompt, with the in-use button, the share bar, Stop and
    the tab icon.
  - A blocked screen share shows no picker.
  - Settings lists what is stored.
- Window test hooks: `permission_prompts_shown`, `permission_prompt`, `capture_button_shown`,
  `share_bar_shown`, `close_prompt_flyout`, `stop_sharing_button` and `tab_capture_shown`.

---

## 7. Memory Saver (suspend)

### 7.1 What the user sees

- Settings > General > Memory has the switch `memory_saver::TITLE` ("Memory Saver") with
  `DESCRIPTION` ("Frees up memory from inactive tabs, which become active again when you go back to
  them"). The combo box `MODE_TITLE` ("Memory savings") offers Moderate, Balanced or Maximum (6, 4
  or 2 hours) with descriptions ("Tabs become inactive after N hours in the background") [V].
- A sleeping tab shows its favicon at 0.5 opacity in both tab lists (`tab_header.rs`, `look.asleep`)
  [V].
- Selecting a sleeping tab, showing it in split view or picture-in-picture, or navigating it wakes
  it on the same page with the same title and Back available [V].
- A sleeping tab keeps its address, title and history. The session, tab search and open-tabs sync
  list it as before.

### 7.2 Core contract [V]

- `memory_saver::SWEEP_EVERY = 60 s`. `Sweep::new(p, now)` reads `keys::MEMORY_SAVER`
  (`"performance.memory_saver"`, Local, default true), `keys::MEMORY_SAVER_MODE`
  (`"performance.memory_saver.mode"`, Local, default Balanced) and the Notifications=Allow sites.
- `Sweep::sleeps(&TabActivity { url, shown, pinned, audible, capturing, related }, &mut IdleClock)
  -> bool`. Anything that keeps a tab awake restarts its clock. Only http/https pages sleep. The
  clocks run while the feature is off.
- `memory_saver::UNSAVED_INPUT_SCRIPT` (main-frame form fields changed from their defaults, or a
  file chosen) and `has_unsaved_input(result)`.

### 7.3 Behaviour details [V]

- `browser::sweep_tabs` loops every `SWEEP_EVERY` and calls `Browser::sleep_idle_tabs(now)`, which
  runs `tab.sleeps(&sweep)` for each tab and spawns `tab.sleep(now)` for those that should sleep.
- `Tab::sleeps`: a tab marked asleep that the engine woke by itself is woken. Blank tabs never
  sleep. `shown` means visible in the page grid or picture-in-picture. `audible` includes being
  the media player's tab. `related` means a `window.opener` link either way.
- `Tab::sleep(now)`:
  1. Run `UNSAVED_INPUT_SCRIPT`. Unsaved input restarts the clock and the tab stays awake.
  2. Re-judge with a fresh `Sweep` (the tab may have closed, come on screen, been pinned, or
     navigated meanwhile).
  3. `TrySuspendAsync()`. `Ok(true)` marks it asleep; if it came on screen meanwhile it is woken at
     once. `Ok(false)` means the engine kept it awake.
- `Tab::wake()` calls `Resume()`, restarts the clock and notifies. Callers: `place_views` when a
  view becomes visible, `NavigationStarting`, and `sleeps` when the engine already resumed it.

### 7.4 Classification

- (KEEP) Policy (core), sweep cadence, the unsaved-input guard with re-judgement, wake triggers,
  the faded icon, and title, URL and history surviving sleep.
- (WEBVIEW2-ONLY) `TrySuspendAsync`, `Resume`, `IsSuspended` ("WebView2's own sleeping tabs").
- (WEBKIT-ONLY) Linux ended the web process and reloaded on wake (so it also kept edited pages
  awake through a per-frame edit script).
- (NEEDS-CEF): CEF has **no suspend or discard API** [V absence in `_cef_browser_host_t`]. Options
  [I]:
  - CDP `Page.setWebLifecycleState{state:"frozen"|"active"}`: freezes timers and tasks and keeps
    state, like WebView2's suspend, but frees little memory.
  - Close the browser and recreate it on wake: frees memory, but CEF cannot restore navigation
    entries (there is `GetNavigationEntries`, a read-only visitor [V], and no setter), so history
    would be lost. That contradicts the self-test's "can go back" check.
  - `CefBrowserHost::WasHidden(true)` affects only windowless (OSR) rendering [I].
  - The tab still needs an "is the engine asleep" readback for the self-test (`Tab::suspended()`).

### 7.5 Interactions and self-test

- Interactions: permissions (capturing, notifying sites), media player (`media_tab`),
  picture-in-picture (`pip_tab`), the window opener map (`set_related` in `open_tab_from`), split
  view, and session (keeps listing).
- Self-test `memory_saver` [V]:
  - Three background tabs: one with Back history, one with typed input, one pinned.
  - `sleep_idle_tabs(now + 7 h)` must put only the first to sleep, with `(look.asleep,
    suspended()) == (true, true)`. Its title is kept and the saved session lists its URL and title.
    The others and the selected tab stay `(false, false)`.
  - Selecting it wakes it on `page2.html`, titled as before, with `can_go_back`.
  - Pinned while its page answers the sweep, it stays awake.
  - With the pref off, nothing sleeps.

---

## 8. View source

### 8.1 What the user sees [V]

- **Command** `ViewSource` (Ctrl+U by default, `InPage::Overridable` so a page can keep its own
  Ctrl+U) and main-menu item "More tools" > "View page source" (`MenuViewSource`). The menu item is enabled only
  for pages with a source (`menu_opening`). The command opens `view-source:<page>` **in a new
  foreground tab right after the page** (`open_tab_from(tab.id, Initial::Url(url), false)`), and
  the new tab joins the page's group.
- **Page context menu** "View page source": added when WebView2's own `viewPageSource` is missing,
  right before `inspectElement`, only for a `Page` target that is not a link and not editable, and
  only when `source_url` exists.
- The tab's address shows the `view-source:` URL while that page is current.

### 8.2 Core contract [V]

- `view_source::source_url(page) -> Option<String>`: http, https and file pages only, as in Chrome.
- `view_source::viewed_url(source) -> Option<&str>` gives back the inner page.
- `view_source::source_page(url, source)` renders WebKit's view-source and is not needed with a
  Chromium engine.

### 8.3 Classification

- (KEEP) The command, menu item enablement, new foreground tab next to the page, and the context
  menu item before Inspect.
- (WEBVIEW2-ONLY) `Tab::view_source` / `shown_url`: WebView2 reports the inner page's address for
  a `view-source:` page, so the tab remembers the address it loaded. In CEF, `OnAddressChange`
  probably reports the `view-source:` URL itself [I].
- (WEBKIT-ONLY) Linux's own `view-source` scheme handler, hidden fetch view and navigation gate.
  Chromium renders `view-source:` natively.
- (NEEDS-CEF) `CefFrame::LoadURL("view-source:…")` in the new tab's browser [I that it loads
  without restrictions from the browser side]. CEF's default context menu has
  `MENU_ID_VIEW_SOURCE` (132) [V]. Its default handling (`CefFrame::ViewSource`, a method that
  exists [V]) opens its own popup [I], so the shell should handle the command in
  `OnContextMenuCommand`. Chrome-style ids: `IDC_VIEW_SOURCE` (35002) and
  `IDC_CONTENT_CONTEXT_VIEWFRAMESOURCE` (50181) [V]. Renderer-initiated navigations to
  `view-source:` must stay blocked (Chromium does this itself [I]).

### 8.4 Self-test

`page_commands` (Windows) [V]:

- The bindings make Ctrl+P, Ctrl+Shift+I, F12 and Ctrl+Shift+J Native and Ctrl+U Overridable.
- Ctrl+U typed through CDP `Input.dispatchKeyEvent` opens `view-source:<fixture>` as a new
  foreground tab right after the page, with `<title>Vsesvit fixture</title>` in its text.
- A right-click through `Input.dispatchMouseEvent` raises `ContextMenuRequested` with `print`,
  `inspectElement`, and either `viewPageSource` or the custom item right before Inspect. The check
  chooses the custom item and verifies the same tab.

The Linux variant also printed to PDF and toggled the inspector (`docs/design/self-test.md`).

---

## 9. Find in page

### 9.1 What the user sees [V]

- `Command::Find` (Ctrl+F, `InPage::Native`): with the page focused, WebView2 handles Ctrl+F
  itself. From the window or menu, the shell calls `find_options("")` with
  `SetSuppressDefaultFindDialog(false)`, so **WebView2's built-in find bar** shows, with an empty
  term. Find next and previous, Esc and the match counter are WebView2's own UI. There is no
  shell-drawn find bar on Windows.
- `Tab::find(options) -> match count` and `Tab::stop_find()` exist. The UI smoke uses them.

### 9.2 Core contract

Find has no core state. Core's keymap (`vsesvit_core::shortcuts::Command`) defines `Find` (Ctrl+F),
`FindNext` (Ctrl+G) and `FindPrevious` (Ctrl+Shift+G) [V]. WinUI implements only `Find` and leaves
next and previous to WebView2's bar (`shortcuts::IMPLEMENTED`'s doc lists them as missing) [V]. The
CEF shell has to implement all three.

### 9.3 Classification

- (KEEP) Ctrl+F finds in the page (the default chord, reassignable), find next and previous, a
  match count, and close with Esc.
- (WEBVIEW2-ONLY) The built-in find bar (`ICoreWebView2_28::Find`, `CoreWebView2FindOptions`).
- (NEEDS-CEF) `CefBrowserHost::Find(searchText, forward, matchCase, findNext)` and
  `StopFinding(clearSelection)`. Results arrive through `CefFindHandler::OnFindResult(identifier,
  count, selectionRect, activeMatchOrdinal, finalUpdate)` [V]. Alloy style has no find bar [I], so
  the GPUI shell draws one. Its wording can follow the GTK shell's `find_bar.rs` ("{n} matches",
  "No matches", "More than 1000 matches") or Chrome's "x/y". Chrome-style commands `IDC_FIND`
  (37000) and `IDC_FIND_NEXT` (37001) exist [V] but only drive Chrome's own UI [I].

### 9.4 Self-test

UI smoke step `02-find` [V]: `find_options("e")` on the second tab must give `>= 1` match, with the
tab still active (screenshot), then `stop_find()`. The self-test has no find check.

---

## 10. Print, Save page as, Developer tools

### 10.1 What the user sees [V]

- **Print** (Ctrl+P, Native in the page; main menu "Print…" with its accelerator text from the
  bindings): `ShowPrintUI(CoreWebView2PrintDialogKind::Browser)`, which is WebView2's
  Chrome-like print preview.
- **Save page as** (`Command::SavePage`, Ctrl+Shift+S, Overridable; main menu "Save page as…"):
  WebView2's Save As dialog (`ShowSaveAsUIAsync`). A cancel is silent. Failures show the window's
  notice bar titled "Could not save the page" with one of: "The file already exists.", "The file
  name or folder is not valid.", "This page cannot be saved in that format.", "WebView2 gave the
  unknown result N.", or the error message.
- **Developer tools** (Ctrl+Shift+I and F12 Native; main menu "More tools" > "Developer tools")
  and **JavaScript console** (Ctrl+Shift+J): `OpenDevToolsWindow()`. WebView2 has no API that picks the console
  panel, so the console command opens DevTools like the other. In the page, WebView2 itself opens
  the console on Ctrl+Shift+J.
- WebView2's context menu carries Print and "Inspect" (`inspectElement`) natively.

### 10.2 Core contract [V]

The chords come from `vsesvit_core::shortcuts` (`Command::{Print, SavePage, DeveloperTools,
JavaScriptConsole}`), reassignable in Settings > Shortcuts.

### 10.3 Classification

- (KEEP) Print preview, Save page as (MHTML single-file in the test), DevTools in a window, the
  console shortcut, Inspect in the context menu, and the failure notice.
- (WEBVIEW2-ONLY) Leaving these keys to the engine while the page has focus (`InPage::Native`),
  and the "no console panel" limitation.
- (NEEDS-CEF):
  - Print: `CefBrowserHost::Print()` [V]. With Alloy on Linux the client must implement
    `CefPrintHandler` (`OnPrintStart`, `OnPrintSettings`, `OnPrintDialog`, `OnPrintJob`,
    `OnPrintReset`, `GetPdfPaperSize` [V]); with Chrome style Chrome's print preview shows [I].
    `PrintToPDF` [V] serves tests.
  - DevTools: `ShowDevTools(windowInfo, client, settings, inspect_element_at)`, `CloseDevTools`,
    `HasDevTools` [V], plus `CefLifeSpanHandler::OnBeforeDevToolsPopup` [V]. Console:
    `ExecuteChromeCommand(IDC_DEV_TOOLS_CONSOLE = 40005)` with Chrome style only ("Only used with
    Chrome style") [V doc]. Inspect at a point: `inspect_element_at`.
  - Save page: CEF has **no Save As API** [V absence]. Options: `ExecuteChromeCommand(IDC_SAVE_PAGE
    = 35004)` (Chrome style) [V id, I behaviour], or CDP `Page.captureSnapshot{format:"mhtml"}`
    written by the shell after its own file dialog [I].

### 10.4 Self-test [V]

- `save_page`: Ctrl+Shift+S into the page, with `SaveAsUIShowing` suppressing the dialog and
  choosing SingleFile at `<out>/saved-page.mhtml`. The file must contain `Vsesvit fixture`.
- `page_commands`: §8.4. The Windows check never opens a print dialog or DevTools; it only checks
  the bindings and `ICoreWebView2_16`.

---

## 11. Zoom

### 11.1 What the user sees [V]

- A **zoom chip** in the address pill shows the level ("125%") whenever it is not 100%, as in
  Chrome. Clicking it opens the **zoom bubble** (`Flyout`, Transient, takes no focus): "Zoom:", the
  level, "−" (tooltip "Zoom out (Ctrl+minus)"), "+" ("Zoom in (Ctrl+plus)") and "Reset". The
  buttons do not take focus (`AllowFocusOnInteraction="False"`). The bubble hides when the level
  returns to 100%.
- Ctrl+plus, Ctrl+minus, Ctrl+0 and Ctrl+wheel zoom the page (WebView2 handles them itself).
  Chrome's levels are 25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400 and
  500 percent.
- **Per-site memory across restarts**: a level set on one page applies to the site's other pages
  and after restart. Setting 100% forgets the site. A new document that starts at a different level
  than its site's is taken to the site's level.
- Private windows remember zoom only for the private session.

### 11.2 Core contract [V]

- Core's keymap defines `ZoomIn` (Ctrl+Plus, Ctrl+Equal, Ctrl+KeypadPlus), `ZoomOut` (Ctrl+Minus,
  Ctrl+KeypadMinus) and `ZoomReset` (Ctrl+0, Ctrl+Keypad0). WinUI does not bind them, because
  WebView2 handles the keys itself. Also `Fullscreen` (F11) and `ReloadBypassCache` (Ctrl+Shift+R,
  Shift+F5) are unbound in WinUI (`shortcuts.rs` doc: "Zoom, fullscreen, find next/previous,
  reload without the cache, quit, Settings and the shortcuts list are missing: WebView2 or nothing
  handles them"). The CEF shell can bind them through the keymap (`CefBrowser::ReloadIgnoreCache`
  [V]).
- `p.site_zoom(browsing)` → `SiteZoom` with `get(url) -> f64` (a factor, `zoom::DEFAULT = 1.0`) and
  `set(url, level)`. It is per **host** whatever the scheme or port, only for http and https,
  stored as LOCAL table `site_zoom(host, percent)` (never synced). Setting the default deletes the
  row. Private zoom stays in the private session.

### 11.3 Behaviour details (`zoom.rs`, `Tab::zoom_reported`, `BrowserWindow::take_to_site_zoom`) [V]

- WebView2's XAML control exposes neither the level nor a setter. The page script reports
  `devicePixelRatio` (`{"t":"zoom","dpr":…, "start":bool}`) at document start, on `resize`, and on
  `matchMedia("(resolution: …dppx)")` change. The level is `percent(ratio, window scale)` snapped
  to Chrome's levels within 2%.
- `Memory::reported`: `Start(level)` sets `wanted = site level if different`. `Change(level)` is
  remembered for the site after `SETTLE = 1 s`, unless the window scale changed meanwhile (a move
  to another monitor) or a later change came.
- Setting a level means sending real OS key presses (`SendInput`: Ctrl plus `VK_OEM_PLUS`,
  `VK_OEM_MINUS` or `0`, one step per Chrome level, Reset for 100%). The page is focused first, and
  50 ms later keys go only if the window is foreground, the page has focus and no modifier or mouse
  button is held (`may_press`). Scripted runs never send keys and only record `wanted`. Presses
  that could not be sent are put back for the next focus or selection.
- `take_to_site_zoom` runs on tab selection, page focus, and a new document's start report, and
  only interactively, in the foreground, not in picture-in-picture, and not while the address box
  is focused.

### 11.4 Classification

- (KEEP) The chip and bubble UI and wording, Chrome's preset levels, per-host memory in core
  (local, private-session aware), 100% forgetting the site, and new documents taking the site's
  level.
- (WEBVIEW2-ONLY) The whole devicePixelRatio inference, synthetic OS key presses, the
  `Memory`/`Unsettled`/`SETTLE` machinery, and `emulate_zoom` in the self-test.
- (NEEDS-CEF) `CefBrowserHost::GetZoomLevel()`, `SetZoomLevel(level)` ("Specify 0.0 to reset"),
  `Zoom(CEF_ZOOM_COMMAND_OUT|RESET|IN)`, `CanZoom` and `GetDefaultZoomLevel` [V]. Chromium zoom
  levels are logarithmic: factor = 1.2^level [I], so core's factor maps as `level =
  ln(factor)/ln(1.2)`. CEF has **no zoom-changed callback** [V absence in the display handler]. The
  shell owning Ctrl+plus/minus/0 and Ctrl+wheel (`CefKeyboardHandler::OnPreKeyEvent`) knows each
  change [I]. Chromium's per-host zoom map may also persist levels itself, which would duplicate
  core [I].

### 11.5 Self-test

`zoom_is_remembered_per_site` [V]: an emulated 125% (`Emulation.setDeviceMetricsOverride` with
deviceScaleFactor = scale × 1.25) must be remembered as 1.25 for the fixture host, and `page2.html`
keeps it. A `data:` page at 100% leaves the site remembered. `index.html` opened at 100% wants
125%. Back to 100% forgets it. `private_window` checks that 125% in private stays in the private
session and never reaches stored zoom. With CEF the check can call `SetZoomLevel` and read
`GetZoomLevel` instead of emulating.

---

## 12. Site-info popup and the page's security state

This is the hub where trackers, cookies and permissions surface. It is engine-driven by the CDP
security report.

### 12.1 What the user sees [V]

- **Site icon** at the start of the address pill (`site_look`): a lock "Connection is secure" for
  https with security state `secure`, a warning "Not secure" for http or https without a secure
  report, a search glyph "Search or enter web address" for blank or new tab, and a page glyph "This
  page is on your device or inside the browser" for local pages. blob: and filesystem: URLs are
  judged by their inner origin.
- **Popup** (opens from the site icon; width 380, max height 560, scrollable), only for pages with
  an address. Contents in order:
  1. Headline (glyph plus "Connection is secure" / "Connection is not secure" / "This page is on
     your device or inside the browser") and the explanation.
  2. The host.
  3. Tracking protection switch and status (§2).
  4. Cookies choice and status (§3).
  5. Permissions section (§6).
  6. TLS line "The connection is encrypted and authenticated using {protocol}, {key exchange [with
     group]} and {cipher [with mac]}."
  7. Certificate block: "Issued to", "Issued by", "Valid from" and "Valid until" (localized long
     dates), an expander "Subject alternative names (N)", "SHA-256 fingerprint" (monospace), the
     indented "Certificate chain", and a **"Show certificate"** button that opens the Windows
     certificate viewer (`CryptUIDlgViewContext` on a memory store of the chain).
- Opening the popup withdraws a shown permission prompt, which returns when the popup closes.

### 12.2 Core contract [V]

`vsesvit_core::certificate::parse_der(der) -> Certificate { subject, issuer, not_before_unix,
not_after_unix, dns_names, sha256_hex, … }` and `Name::display()`. Plus the trackers, cookies and
permissions APIs above.

### 12.3 Engine details [V]

`Security.enable` runs in each tab's session. `Security.visibleSecurityStateChanged` is parsed
(`connection::parse_report`) into `Report { state ("secure", "insecure", "insecure-broken",
"neutral" or "info"), tls { protocol, keyExchange[+Group], cipher[+mac] }, chain: Vec<DER> }` from
`certificateSecurityState.certificate` (base64). `Reports` keeps a report tied to its document: it
is set aside at navigation start, shown while that page is on screen, and dropped once another
document commits.

### 12.4 Classification

- (KEEP) Icon semantics, popup layout and wording, the TLS line, certificate details, the
  report-to-document lifetime, and the prompt yielding to the popup.
- (WEBVIEW2-ONLY) The Windows certificate viewer.
- (NEEDS-CEF):
  - The same CDP route through `ExecuteDevToolsMethod("Security.enable")` plus
    `AddDevToolsMessageObserver` [I that the Security domain behaves the same in CEF].
  - Alternatively `CefBrowserHost::GetVisibleNavigationEntry()` → `CefNavigationEntry::GetSSLStatus()`
    → `CefSSLStatus::{IsSecureConnection, GetCertStatus, GetSSLVersion, GetContentStatus,
    GetX509Certificate}` and `CefX509Certificate::{GetDEREncoded, GetDEREncodedIssuerChain, …}`
    [V]. This has no cipher or key exchange, so the CDP route is still needed for the TLS line.
  - "Show certificate" on Linux has no system viewer. The shell draws its own, or Chrome style's
    certificate viewer [I].

---

## 13. Private (InPrivate) mode

### 13.1 What the user sees [V]

- Ctrl+Shift+N (`Command::NewPrivateWindow`), main menu "New private window" (after New window),
  and a link's context menu "Open link in private window" (§17.3) open a private window. Ctrl+N
  from a private window opens a normal one.
- A private window is always dark (`theme_for` returns `Theme::Dark`), shows a "Private" pill in the
  toolbar (tooltip "Private windows keep no history, and forget their cookies and site data when the
  last one closes"), and its title ends "Vsesvit (Private)". With several profiles the title is "Page -
  Profile - Vsesvit (Private)" on Windows ("Page - Profile (Private)" on Linux).
- It has no profile button, no extension buttons, and extension popups and shortcuts do nothing
  there ("extensions do not run in private windows").
- The new tab page is core's private page (§17.11).
- Site info offers no tracking switch and no cookie rule.
- It has its own recently closed stack, tab search, and downloads visibility.
- When the last private window closes, the private session ends: running private downloads are
  cancelled, private closed tabs and core's private state are forgotten, and the engine's
  InPrivate data is gone.

### 13.2 Core contract [V]

- `vsesvit_core::private::Browsing::{Normal, Private}` is passed to every core write.
  `Browsing::loggable(address)` writes "(a private page)" in logs.
- `Profile::end_private_session()` clears private site choices, zoom and download rows.
- Writes skipped for private tabs, enforced in `Browser`:
  - no history visits or titles (`navigation_committed`, `title_changed`)
  - no favicons (`record_favicon`)
  - no session (`snapshot` lists normal windows only)
  - no search suggestions (`suggest_request(private)`)
- `site_permissions_in(Private)`, `site_zoom(Private)` and `downloads().start(.., Private)` go to
  the private session.
- `cookies::third_party_blocked(p, Private, top)` and `ThirdPartyCookies::blocks(Private)`.
- `new_tab::page(p, Private)` returns `PRIVATE_PAGE`.

### 13.3 Engine details [V]

- `Engine::ensure_view(view, Private)` → `CreateCoreWebView2ControllerOptions()` with
  `SetIsInPrivateModeEnabled(true)` from the **same environment** (WebView2 refuses a second
  environment on the same folder).
- The InPrivate profile is set up separately (`Engine.private: ProfileSetUp`) and set up again for
  each private session (`private_session_ended()` resets it).
- No startup cookie clearing runs for private tabs. No extensions are touched in the InPrivate
  profile, since adding one may install it in the normal profile.
- `Browser::end_private_session()` calls `end_private_downloads()`, clears the private closed
  tabs, calls `Profile::end_private_session` and `engine.private_session_ended()`.

### 13.4 Classification

- (KEEP) All user-visible rules above, nothing persisted, logs scrubbed, the session ending with
  the last private window, and command lines, extensions and sync opening pages in normal windows
  (`open_in_normal_window`).
- (WEBVIEW2-ONLY) "No extensions in private windows". WebView2 had no per-extension incognito
  switch. Linux had per-extension "Allow in private" (PLAN.md), and Chrome style CEF has Chrome's
  incognito extension switch [I].
- (NEEDS-CEF) A separate `CefRequestContext` created with an **empty `cache_path`**: "browsers will
  be created in 'incognito mode' where in-memory caches are used for storage and no
  profile-specific data is persisted to disk" [V doc]. Ending the session means closing every
  browser in that context and creating a new context for the next session [I]. Third-party cookie
  blocking in private windows needs the same mechanism as §3 applied to that context.

### 13.5 Self-test

`private_window` [V]:

- A private window on `/index.html?private-window` has `IsInPrivateModeEnabled`, and the detail
  lists the InPrivate profile's extensions.
- The probe's content script does not run, and the probe's action shortcut and popup do nothing.
- No history row and no session tab.
- 125% zoom stays private.
- Tab search lists it only from private windows.
- The default cookie choice blocks the `localhost` frame's cookie, and site info has no cookie
  rule.
- The private download row is shown only in private windows.
- After close: no private closed tabs, the normal stack is unchanged, zoom falls back, no private
  rows remain, and the file stays.
- **No log line names its addresses.**

The CEF variant should check its incognito request context instead of `IsInPrivateModeEnabled`.

---

## 14. Error pages and process failure

### 14.1 What the user sees [V]

- **Windows customizes no error page.** Failed loads show WebView2's built-in Chromium error pages
  and certificate interstitials. `NavigationCompleted` only logs "navigation failed, web error N"
  (`tab.rs`). An error page commits, so its URL is kept for the session (`navigation_ended`).
- The one Vsesvit page shown for a failure is the **HTTPS-only warning** (§4).
- **Renderer crash**: `ProcessFailed` is logged ("engine process failed, kind N"). No sad-tab UI is
  drawn.
- For contrast, the GTK shell had its own `error_page.rs` (TLS error, load failed, crashed, each
  with Retry). That is another inventory's subject.

### 14.2 Classification

- (KEEP) Failed loads show an explanatory page at the failing URL, keep the tab's URL, record no
  history visit, and HTTPS-only's warning takes precedence when an upgrade failed.
- (NEEDS-CEF) `CefLoadHandler::OnLoadError(frame, errorCode, errorText, failedUrl)` [V]. Alloy
  style shows a minimal page; Chrome style shows Chromium's net error pages and SSL interstitials
  [I]. Certificates: `CefRequestHandler::OnCertificateError(browser, cert_error, request_url,
  ssl_info, callback)` (`cef_sslinfo_t::{GetCertStatus, GetX509Certificate}`) [V]. Crash:
  `CefRequestHandler::OnRenderProcessTerminated(status, error_code, error_string)` ("the error that
  would be displayed in Chrome's 'Aw, Snap!' view") [V doc], plus `OnRenderProcessUnresponsive` /
  `OnRenderProcessResponsive` [V]. A custom error document must be loaded without `LoadString`
  (§4.4).

---

## 15. Favicons

### 15.1 What the user sees [V]

- Each tab shows the page's favicon in both tab lists (the strip and the pane), the media player,
  and tab search, including closed tabs, which keep the icon they had (`ClosedTab::favicon`).
- Bookmarks and the bookmarks bar show icons of bookmarked pages or sites. Visiting a bookmarked
  page stores its icon. Starring a page stores the current icon at once (`star_clicked`).
- Pages that change their icon repeatedly (badging) are handled. A generation counter drops stale
  loads.
- Private tabs show icons but never store them.

### 15.2 Core contract [V]

- `Browser::record_favicon(browsing, url, png)` → `p.favicons().record(&url, png) -> Result<bool>`.
  It stores the icon only when the page or its site is bookmarked; true means stored, after which
  the bars refresh.
- `p.favicons().get(&url)` feeds the bookmarks bar.
- Preload for bookmarks without icons: `p.favicons().missing(FAVICON_BATCH = 24)`, then
  `FaviconFetch::new(pages).run()` on a worker thread (core's own HTTP fetch, not the engine), then
  `p.favicons().commit_fetched(fetched)`. It runs only in interactive runs; scripted runs start it
  themselves.

### 15.3 Engine details [V]

`FaviconChanged`. If `FaviconUri` is empty there is no icon; else `GetFaviconAsync(Png)` returns
bytes, which become an `ImageSource` and the stored PNG.

### 15.4 Classification

- (KEEP) The icon per tab, store-on-visit for bookmarked sites only, never for private tabs, stale
  load protection, and the preload through core.
- (NEEDS-CEF) `CefDisplayHandler::OnFaviconURLChange(browser, icon_urls)`, then
  `CefBrowserHost::DownloadImage(image_url, is_favicon=true, max_image_size, bypass_cache,
  CefDownloadImageCallback::OnDownloadImageFinished)`, then `CefImage::GetAsPNG(scale,
  with_transparency, &w, &h)` [V]. With `is_favicon` true, "cookies are not sent and not accepted"
  [V doc].

### 15.5 Self-test

`bookmarks_bar_icons` (not read in detail here) uses `FAVICON_SWAPS`, a script that swaps the icon
four times 300 ms apart, and `record_favicon` [V for the constant and the call in
`automation/mod.rs`].

---

## 16. Session restore (and the history transitions it shares events with)

### 16.1 What the user sees [V]

- On start, `keys::STARTUP` (`"startup"`, Synced, default `RestoreSession`) decides what opens.
  Restore means every normal window with its tabs, the selected tab, bounds, maximized state,
  pinned tabs and groups. Homepage opens one tab on `keys::HOMEPAGE` (`homepage_url`, where
  `about:home` means the new tab page). NewTab opens one blank tab. Command-line URLs are added as
  tabs of the first window, with the first of them selected.
- A restored tab shows its saved title until its page reports one (`set_planned`) and keeps its
  session tab id. Back/forward history is **not** restored on Windows: WebView2 cannot.
- Private windows are never saved. A save with no normal window open writes nothing, so the last
  normal session stays to restore.
- Closing the last tab of the last window saves the session *with* that tab
  (`save_before_closing_tab`), so the next start reopens it.

### 16.2 Core contract [V]

- `p.session().save(&SessionSnapshot)` / `restore() -> Option<SessionSnapshot>`.
- `SessionSnapshot { device_name, windows: Vec<WindowSnapshot>, active_window }` and
  `WindowSnapshot { tabs, active_tab, bounds: Option<(i32,i32,u32,u32)>, maximized }`.
- `TabSnapshot { id: session::TabId(Uuid), url, title, pinned, last_active_ms, group:
  Option<TabGroup>, restore_state: Option<Vec<u8>> }`. `restore_state` is the engine back/forward
  blob. It is local-only and never on the wire; Windows writes `None`.
- `device_name` is `keys::DEVICE_NAME` (Local), else `COMPUTERNAME` on Windows. A Linux shell would
  use a host-name equivalent.
- Shell helpers (`session.rs`, pure functions): `startup_plan(startup, restored, homepage, urls)`,
  `restore_window` (an `about:blank` URL means a blank tab, and the active index is clamped),
  `tab_snapshot` (an unparseable or empty URL is saved as `about:blank`) and `window_snapshot` (a
  window without tabs is dropped).
- History: `p.history().record_visit(&url, Transition)` on every committed main-frame navigation of
  a normal tab, and `set_title`. `Transition::{Link, Typed, Bookmark, Reload, Redirect,
  FormSubmit}`.

### 16.3 Behaviour details [V]

- `Browser::session_changed()` debounces saves by `SESSION_SAVE_DELAY = 2 s`. It is triggered by
  tab open, close and select, commits, titles, the HTTPS warning and window close.
  `save_session_now` writes immediately. `session_final` stops saving once the last window is
  closing. `browser::shutdown` saves if needed, then runs `sync::final_sync`.
- `Tab::session_url()` is the committed URL, or the requested one before the first commit.
  `navigation_ended` clears `requested` when a first navigation never committed (a download or a
  Stop), so the session does not try to reload a download URL.
- `PendingTransition` attributes shell-initiated navigations (Typed or Bookmark through
  `navigate_as`, Reload) to the first navigation that starts after the request, across redirects.
  Navigations already under way end first (`superseded`). A typed fragment is a same-document
  commit. Page-initiated navigations count as Link. A Typed or Bookmark commit clears the window's
  opener map (`forget_openers`).

### 16.4 Classification

- (KEEP) The startup plan, debounce, final save with the last tab, private exclusion, planned
  title, and the transition attribution feeding history.
- (WEBVIEW2-ONLY) No back/forward restore.
- (WEBKIT-ONLY) Linux restored back/forward through the WebKit session-state blob in
  `restore_state` / `tab_restore_state`.
- (NEEDS-CEF) CEF has no API to restore navigation entries (`GetNavigationEntries` is read-only)
  [V absence], so a CEF tab likely restores like Windows: URL only, `restore_state = None` [I].
  Transitions: `CefRequest::GetTransitionType()` in `OnBeforeBrowse` and `transition_type` in
  `OnLoadStart` (`TT_LINK`, `TT_EXPLICIT`, `TT_AUTO_BOOKMARK`, `TT_FORM_SUBMIT`, `TT_RELOAD`, the
  redirect flags) [V] could replace part of `PendingTransition` [I], although the shell still knows
  best which loads it started.

### 16.5 Self-test

`session` [V]: `save_session_now()` then `restore()` must give at least one window with tabs.
`memory_saver` and `private_window` also read the restored snapshot.

---

## 17. Other engine-touching behaviour (outside the named list, needed for parity)

### 17.1 Runtime-style dependent defaults (from CEF doc comments) [V]

| Area | Chrome style default | Alloy style default |
|---|---|---|
| `OnShowPermissionPrompt` returns false | "display the permission prompt UI" | `CEF_PERMISSION_RESULT_IGNORE` |
| `OnRequestMediaAccessPermission` returns false | "display the permission request UI" | "deny the request" |
| `OnBeforeDownload` returns false | "default download handling" | "cancel" |
| `OnFullscreenModeChange` | "transition will be triggered automatically" | "client is responsible" |
| `ExecuteChromeCommand` | available | "Only used with Chrome style" |

### 17.2 New windows, pop-up blocking, opener tabs [V]

- Settings > Privacy has the "Block pop-ups" switch (`keys::BLOCK_POPUPS`,
  `"content.block_popups"`, Synced, default true), captioned "Sites can still open windows when you
  click a link or button."
- `new_window_requested`: when blocking is on and the request has no user gesture
  (`IsUserInitiated`), it is handled with no window ("blocked a popup … no user gesture"). Otherwise
  a deferral holds the request, and `window.open_tab_from(opener, Initial::Opener(request),
  background)` opens a tab whose engine view **becomes** the new window, so `window.opener` works.
  An unfulfilled request is cancelled on drop and the page's call returns `null`.
- A foreground tab opens right after its opener. Background tabs go after the tabs the page opened
  before (`tab_layout::from_page_index`). Ctrl+click or middle-click opens in the background; it is
  detected by the page script's `link` message within 3 s with the same URL. Both tabs are marked
  `related` (Memory Saver). New tabs join the opener's group.
- Classification: (KEEP) blocking rule, opener semantics, placement and background tabs.
  (WEBVIEW2-ONLY) background-link detection by script. (NEEDS-CEF)
  `CefLifeSpanHandler::OnBeforePopup(…, target_disposition, user_gesture, …,
  no_javascript_access)` gives `CEF_WOD_NEW_FOREGROUND_TAB` (3), `NEW_BACKGROUND_TAB` (4),
  `NEW_POPUP` (5), `NEW_WINDOW` (6) and `OFF_THE_RECORD` (8) [V]. Middle-click and Ctrl+click on
  normal links go through `CefRequestHandler::OnOpenURLFromTab(…, target_disposition,
  user_gesture)` [V]. The default pop-up content setting can be set with
  `SetContentSetting(empty, empty, CEF_CONTENT_SETTING_TYPE_POPUPS (3), …)` ("If both URLs are
  NULL … the default value will be set") [V doc].

### 17.3 Page context menu items [V]

Four items are inserted into the engine's menu:

1. **Selection search** (Chrome's "Search {engine} for '…'"), right after `copy`. Label and URL
   come from `Browser::selection_action(text)` → `p.omnibox().for_selection(text) ->
   SelectionAction { label, url }`. It opens a new tab next to the page.
2. **"Copy link without tracking"** (`CLEAN_LINK_ITEM`), right after `copyLinkLocation`. It copies
   `vsesvit_core::clean_url::clean(link)`.
3. **"Open link in private window"** (`PRIVATE_LINK_ITEM`), right after `openLinkInNewWindow`. It
   calls `browser.open_link_in(Browsing::Private, url)`.
4. **"View page source"** before `inspectElement` (§8).

Items run their action after the menu has closed. Extension `contextMenus` items appear in the
engine's menu by themselves. CEF mapping: `OnBeforeContextMenu(params, model)` with ids in
`MENU_ID_USER_FIRST` (26500) to `MENU_ID_USER_LAST` (28500), and `OnContextMenuCommand` [V].
`CefContextMenuParams` gives `GetSelectionText`, `GetLinkUrl`, `GetUnfilteredLinkUrl`,
`IsEditable`, `GetTypeFlags`, `GetPageUrl`, `GetFrameUrl`, `GetMisspelledWord` and
`GetDictionarySuggestions` [V]. Chrome-style ids that anchor the insert positions [V]:
`IDC_CONTENT_CONTEXT_COPY` (50150), `IDC_CONTENT_CONTEXT_COPYLINKLOCATION` (50104),
`IDC_CONTENT_CONTEXT_OPENLINKNEWWINDOW` (50101), `IDC_CONTENT_CONTEXT_OPENLINKOFFTHERECORD` (50102,
Chrome's own "incognito" item, which must map to Vsesvit's private window [I]),
`IDC_CONTENT_CONTEXT_INSPECTELEMENT` (50162), `IDC_CONTENT_CONTEXT_SEARCHWEBFOR` (50191, Chrome's
own search item, which uses Chrome's default engine and not core's [I]),
`IDC_EXTENSIONS_CONTEXT_CUSTOM_FIRST` (49000, extension items). Alloy's default menu holds only
`MENU_ID_*` basics (Back, Forward, Reload, Copy, Find, Print, View Source, spelling) [V ids, I
content]. Extension items in the menu need Chrome style [I]. Self-tests `tab_menu` (link menu
items) and `context_menus` (extension item fires `contextMenus.onClicked` with `pageUrl`) [V].

### 17.4 Keyboard shortcuts while the page has focus (`shortcuts.rs`) [V]

- WebView2 never forwards keys to XAML while the page is focused. So an isolated-world script
  (secret world name `vsesvit-<128-bit random>`, a binding `vsesvitShortcut` that exists only in
  that world, injected in every frame, cross-site frames included through `AUTO_ATTACH`) takes keys
  in three classes. `Reserved` keys are taken in the capture phase before the page sees them.
  `Overridable` keys are taken in the bubble phase only if the page did not `preventDefault()`.
  `Native` keys are left to WebView2 (reload, back, forward, find, print, DevTools). Key sets update
  live (`Bindings::keys_script` through `Runtime.evaluate` in each tracked context plus a replaced
  new-document script).
- (KEEP) the three classes, the key table `IMPLEMENTED` and extension action shortcuts as Reserved.
  (WEBVIEW2-ONLY) the whole script and binding mechanism. (NEEDS-CEF)
  `CefKeyboardHandler::OnPreKeyEvent(event, os_event, is_keyboard_shortcut)` runs "before a
  keyboard event is sent to the renderer", which fits Reserved. `OnKeyEvent` runs after the
  renderer did not handle the key, which fits Overridable [V doc]. The same page script also
  carried zoom reports (§11), background-link reports (§17.2) and store requests (§17.10).

### 17.5 Fullscreen [V]

Page fullscreen (`ContainsFullScreenElementChanged`) in the selected tab hides the toolbar, the
bookmarks bar and the update bar, switches the `AppWindow` presenter to FullScreen, and saves
windowed bounds for the session. Selecting another tab leaves fullscreen. (KEEP), (NEEDS-CEF)
`OnFullscreenModeChange`, plus `CefBrowserHost::ExitFullscreen` / `IsFullscreen` [V].

### 17.6 Audio, mute and load progress [V]

- The tab speaker shows "Mute tab" (playing) or "Unmute tab" (muted) and toggles
  `SetIsMuted`. The audible state feeds the media player and Memory Saver. (NEEDS-CEF)
  `SetAudioMuted` / `IsAudioMuted` [V]. The audible signal is an open question (§1.5).
- Load progress bar along the address pill: WebView2 reports no fraction, so the bar eases toward
  a ceiling per stage (START 0.08, Started 0.3, Committed 0.85, completion runs to the end and holds
  0.15 s) (`window/progress.rs`). (WEBVIEW2-ONLY) easing. (NEEDS-CEF)
  `CefDisplayHandler::OnLoadingProgressChange(progress 0.0..1.0)` [V]. UI smoke
  `progress_steps`.

### 17.7 Passwords and autofill [V]

- Password saving is always off (`SetIsPasswordAutosaveEnabled(false)`). Settings > Privacy says
  "Vsesvit doesn't save passwords. Use a password manager such as Bitwarden or Proton Pass through
  its browser extension." Form autofill for addresses follows `keys::AUTOFILL_FORMS`
  (`"autofill.forms"`, Synced, default true), labelled "Save and fill form entries such as
  addresses", and is applied live to open tabs.
- One-time purge of WebView2-saved passwords: `ClearBrowsingDataAsync(PasswordAutosave)`, then the
  Local pref `passwords.purged` (`browser::PASSWORDS_PURGED`) is set. It retries each start.
  Self-test `passwords_purged`.
- (KEEP) no password saving, the address-autofill switch. (WEBVIEW2-ONLY) the purge and its pref.
  (NEEDS-CEF) Chrome style has a password manager and autofill. Turn them off and on with
  `CefRequestContext` preferences (`SetPreference`, pref names such as
  `credentials_enable_service`, `profile.password_manager_enabled`, `autofill.profile_enabled`)
  [I names].

### 17.8 Clear browsing data and site data [V]

Settings > Privacy > "Browsing data" ("History, cookies, site data and cached files.") has a "Clear
browsing data…" flyout. It asks "Clear browsing data?" with "Deletes your history on every device
you sync with, and this device's cookies, site data and cached files." and a **Clear** button.
Clicking it calls `p.history().delete_range(0, now)`, then
`ClearBrowsingDataAsync(AllSite | DiskCache | BrowsingHistory)`. The status line reads "Browsing
data cleared." or "Could not clear all browsing data: …". (KEEP), (NEEDS-CEF) no single CEF call:
`CefCookieManager::DeleteCookies("", "")` (all), `CefRequestContext::ClearHttpCache`, and CDP
`Storage.clearDataForOrigin` or `Storage.clearDataForStorageKey` [I]. The site data list is §3.

### 17.9 Media player and picture-in-picture (engine part only) [V]

A main-world `js/media.js` keeps `navigator.mediaSession` handlers and exposes `__vsesvitMedia`
(state, play/pause, previous/next, and a picture-in-picture presentation that shows the video over
the whole small page). The shell polls `media::STATE_SCRIPT` and calls `act_script` / `pip_script`
through `ExecuteScript`. Picture-in-picture **reparents the tab's own web view** into the pane's
box. It is gated by `Permission::PictureInPicture` (Allow per site through the address-bar button)
and the prefs `keys::SHOW_MEDIA_PLAYER` (`"media.player"`) and `keys::PICTURE_IN_PICTURE`
(`"media.picture_in_picture"`). (KEEP) behaviour. (NEEDS-CEF) script injection and evaluation
through CDP (§1.4); moving a CEF browser view between containers. UI smoke `pip_steps`.

### 17.10 Extension store pages [V]

`js/store.js` replaces `chrome.webstorePrivate` on Chrome Web Store and Edge Add-ons HTTPS origins
and sends DOM events. The isolated world forwards them with the real origin, and the shell installs
through core's pipeline with Chrome's install prompt. This exists because WebView2 exposes
`webstorePrivate` but cannot complete installs. Detailed in the extension inventory. Whether Chrome
style CEF completes Web Store installs natively is an open question [I].

### 17.11 New tab page [V]

`new_tab::page(p, browsing)` gives HTML (a search box for the default engine plus most-visited
tiles, or `PRIVATE_PAGE` in private windows), loaded with `NavigateToString` so the tab reads as
`about:blank`. That means an empty address bar, the title "New tab", and no session or history
entry. On failure the tab navigates to `about:blank`. Self-test `new_tab_page`: url
`about:blank`, title "New tab", tiles include the fixture origin (`new-tab.png`). (KEEP),
(NEEDS-CEF) no LoadString (§4.4). Candidates [I]: CDP `Page.setDocumentContent` on `about:blank`,
or a custom scheme (`CefSchemeHandlerFactory` through `RegisterSchemeHandlerFactory`, which exists
on `CefRequestContext` [V]) that the shell displays as blank.

### 17.12 Title rules [V]

`display_title`: the document title; if empty or `about:blank`, the URL; if the URL is also empty
or `about:blank`, "New tab". Window title: `window_title(page, profile, browsing)` joins the page
title, the profile name (with several profiles) and "Vsesvit" or "Vsesvit (Private)" with " - ".

---

## 18. Self-test and UI smoke: engine-related needs

### 18.1 Contract [V]

- `vsesvit --self-test OUT_DIR [--network]` writes `report.json` (`vsesvit_core::testkit::report`)
  with exactly `report::CHECKS` in order (47 checks, plus `cws_install` with `--network`). A unit
  test enforces that the run's `check(report, "...")` calls match.
- It never activates a window or sends OS input. Every check is bounded (`DEFAULT_TIMEOUT 15 s`,
  `ENGINE_LOAD_TIMEOUT 60 s`, `NETWORK_TIMEOUT 240 s`), and a timeout reports the last observed
  value (`Probe`). The process exits 0 only if all checks pass.
- `prepare(out_dir)` deletes the earlier run's `profile/`, `downloads/` and artifacts:
  `report.json`, `window.png`, `downloads.png`, `settings-shortcuts.png`, `shortcut-capture.png`,
  `omnibox-inline.png`, `new-tab.png`, `saved-page.mhtml`, `bookmarks.html`, `probe.crx`,
  `vsesvit.log`, `profiles.json`, `profile-menu.png` and `profiles-manage.png`.
- The fixture server is `vsesvit_core::testkit::FixtureServer` (plain http on `127.0.0.1`, with
  `server.hits()`). Fixtures used by engine checks: `/index.html`, `/page2.html`,
  `/trackers.html` (+ `/tracker/pixel.png` on `localhost`), `/cookies.html` (+ a `localhost`
  frame), `/set-cookie`, `/download.bin`, `/stalled.bin`, `/slow.bin` and `/dangerous.bat` or
  `/dangerous.sh`.

### 18.2 Engine primitives the checks use, and their CEF equivalents

| Primitive (Windows) | Used by | CEF equivalent |
|---|---|---|
| `Tab::eval` → `ExecuteScriptAsync` (JSON result) | nearly every check | CDP `Runtime.evaluate{returnByValue}` [I] |
| `Tab::devtools(method, params)` → `CallDevToolsProtocolMethodAsync` | input, emulation | `ExecuteDevToolsMethod` + observer [V] |
| `Input.dispatchKeyEvent` (`rawKeyDown` / `keyUp`, `windowsVirtualKeyCode`, modifiers Alt=1 Ctrl=2 Shift=8) | shortcuts, page_commands, save_page | same CDP [I]; or `CefBrowserHost::SendKeyEvent` [V API] |
| `Input.dispatchMouseEvent` (right click for context menus, Ctrl+click) | page_commands, context_menus, tab_menu, extension tests | same CDP [I]; or `SendMouseClickEvent` [V API] |
| `Emulation.setDeviceMetricsOverride` / `clearDeviceMetricsOverride` | zoom, private zoom | unnecessary with `SetZoomLevel` / `GetZoomLevel` [V API] |
| `ContextMenuRequested` with `SetSelectedCommandId` + `SetHandled(true)` (no native menu shown) | page_commands, context_menus | `CefContextMenuHandler::RunContextMenu` returning true and calling `CefRunContextMenuCallback::Cont(command_id, flags)` [V API] |
| `SaveAsUIShowing` with `SetSuppressDefaultDialog`, `SetSaveAsFilePath`, `SetKind(SingleFile)`, `SetAllowReplace` | save_page | depends on §10 |
| `CapturePreviewAsync` / Windows.Graphics.Capture of the HWND (`capture::window_png`) | screenshot, new_tab_page, downloads, profile shots | CDP `Page.captureScreenshot` for content [I]; whole-window capture under Wayland is an open question |
| `Tab::suspended()` (`IsSuspended`) | memory_saver | depends on §7 |
| `ICoreWebView2_13::Profile().IsInPrivateModeEnabled` | private_window | `CefRequestContext` of the browser (`GetRequestContext`, `IsGlobal`, `GetCachePath`) [V API] |
| `engine::extensions(profile)` (`GetBrowserExtensionsAsync`) | private_window, extension checks | extension inventory |
| `SCRIPTED_ARGUMENTS` (fake media devices, auto-select screen) | permission_steps (UI smoke) | same switches [I] |
| `Browser::set_trackers`, `set_https_reach(Reach::Everywhere)`, `set_download_dir` | tracking_protection, https_only, download | core and shell hooks, engine-agnostic |
| Log file scan (`browser.config().log_file()`) | private_window ("no log line names its addresses") | same |

### 18.3 Engine-related checks (name: what the engine must support)

| Check | Engine support needed |
|---|---|
| `memory_saver` | sleep, wake, a suspended readback |
| `zoom_is_remembered_per_site` | zoom level read and set |
| `private_window` | incognito context, cookie blocking, downloads, no extensions |
| `new_tab_page` | blank-address HTML |
| `tracking_protection` | request cancel plus count |
| `https_only` | navigation gate, failure detection, warning at the http URL |
| `cookies` | third-party blocking, per-site Allow/Block/Clear on exit, list and delete cookies, clear site storage |
| `passwords_purged` | (WEBVIEW2-ONLY) |
| `save_page` | MHTML save |
| `page_commands` | key bindings, view-source tab, context menu introspection |
| `context_menus` | extension menu items |
| `download`, `download_pause`, `download_speed`, `download_safety` | download handler, pause/resume/cancel, speed, unconfirmed path, mark |
| `screenshot` | window capture |
| `session` | — |

Checks `content_script`, `extension_port`, `dnr_blocked`, `dnr_site_allowed`, `popup`,
`extension_commands`, `extension_notifications`, `extension_update`, `engine_loaded_extension`,
`install_crx` and `cws_install` belong to the extension inventory.

UI smoke (`--ui-smoke OUT_DIR`, writes `smoke.json`) steps tied to engine features:
`tracker_steps`, `permission_steps`, `connection_steps` (the security popup on HTTPS, HTTP and local
pages; the certificate viewer only with `VSESVIT_SMOKE_CERT_VIEWER=1`), `pip_steps`,
`progress_steps` and `02-find` [V].

---

## 19. Checklist (one line per feature)

- [ ] Engine starts per profile under `<profile>/engine/`; failure shows "Vsesvit could not start the web engine…" and exits 1
- [ ] Engine Chromium version feeds `OpenOptions.chrome_version` (Web Store prodversion) and About
- [ ] "Use smooth scrolling" off adds `--disable-smooth-scrolling` at next start
- [ ] "Use graphics acceleration when available" off adds `--disable-gpu` at next start
- [ ] Scripted runs add fake media devices and auto-selected screen capture, but keep real permission prompts
- [ ] Per-tab setup completes before the first document (document-start scripts, cookie controls, security domain)
- [ ] First normal view clears Clear-on-exit and Block sites before any page loads (bounded 5 s)
- [ ] Main-frame navigation events map to Started, Committed, Idle with back/forward state
- [ ] Same-document navigations commit as SameDocument (history visit, grants kept)
- [ ] Tracker blocking: each request judged by the tab's main page host and level via `TrackerList::blocks`; blocked requests fail
- [ ] Tracker count per page, reset on new document; site-info status line follows live
- [ ] Tracking switch in site info: hidden off-web, when Off, and in private windows; toggling stores `Permission::Trackers` and reloads
- [ ] Settings tracking protection combo box (Off, Standard, Strict) with descriptions
- [ ] Third-party cookies setting (Allow, Block in private, Block) applied per page, lifted by a site's Allow
- [ ] Site-info cookie rule (Default, Allow, Block, Clear on exit) with status text; reloads page; hidden in private
- [ ] Block sites: no cookies readable or writable in documents; their cookies deleted on set, after loads, at start
- [ ] Clear on exit: data of Block and Clear-on-exit sites cleared at exit (best effort) and at next start
- [ ] "See all site data": per-site cookie counts, Delete per site and Delete all (cookies plus storage of http and https origins)
- [ ] HTTPS-only setting; http navigations to public hosts upgrade to https
- [ ] HTTPS-only warning page at the http address on failed upgrade or redirect loop; title "Connection is not secure"
- [ ] "Continue to site" stores a permanent (or private-session) exception and loads http
- [ ] Reload on the warning retries https; warning never a history visit; shown URL is the http URL
- [ ] Downloads go to the chosen folder or the platform Downloads folder, numbered to avoid collisions with files and running downloads
- [ ] "Ask where to save each file" shows a save dialog; cancel cancels the download
- [ ] Download recorded in core with state transitions; stale live rows become Failed at startup
- [ ] Toolbar Downloads button: hidden until a listed download this session, busy indicator while running
- [ ] Downloads view rows, status lines (speed and time left), per-state buttons, Open download folder, Clear list
- [ ] Pause, resume (including resumable interruptions) and cancel through the engine
- [ ] Dangerous files written as `.unconfirmed`, then a Keep/Discard flyout under the Downloads button of the originating window (no focus steal)
- [ ] Keep moves to a free name; Discard deletes file and row
- [ ] Mark of the Web on finished files (Windows `Zone.Identifier`; decide a Linux equivalent, such as `user.xdg.origin.url` xattrs)
- [ ] No engine-drawn download UI ever shows
- [ ] Private downloads listed in private windows only; cancelled when the private session ends; files kept
- [ ] Permission requests for camera, microphone, location, notifications, clipboard read and MIDI decided by core (stored, private, grants)
- [ ] Engine never persists answers by itself; core is the only record; engine per-origin state mirrors stored settings
- [ ] One prompt at a time for the selected tab; background requests wait; heading focus; 500 ms input guard
- [ ] Prompt dismiss versus withdraw semantics (tab switch or site-info opening withdraws; Esc, "Not now" or a click elsewhere dismisses)
- [ ] Camera plus microphone from one getUserMedia gets one prompt
- [ ] Waiting requests denied on cross-origin commit or tab close
- [ ] Notifications blocked without asking in private windows
- [ ] Screen share: blocked sites get no picker; otherwise the engine or system picker is the prompt; only Block remembered
- [ ] In-use indicators: address-bar capture button, tab icon (red for camera or microphone), "Sharing your screen with {host}" bar with Stop sharing
- [ ] Site-info Permissions section rows with choices, "In use" plus Stop, Reset permissions; refills live keeping focus
- [ ] Block, Reset or removing Allow reloads tabs whose capture the shell had allowed (here, Settings, sync)
- [ ] Settings > Site permissions list by site with remove
- [ ] Memory Saver setting and mode (6, 4, 2 h); sweep every 60 s via `memory_saver::Sweep`
- [ ] Tabs kept awake: shown, split, picture-in-picture, pinned, audible or media tab, capturing, notifying site, opener-related, non-web pages
- [ ] Unsaved form input keeps a tab awake; re-judge after the script answers
- [ ] Sleeping tab icon at 0.5 opacity; wakes on selection, show or navigation with page, title and Back intact
- [ ] View page source command (Ctrl+U, page may override) and menu item (enabled for http, https, file) opens `view-source:` in a new foreground tab next to the page
- [ ] "View page source" page context menu item before Inspect (page target, not link or editable)
- [ ] Find in page: Ctrl+F, find next and previous, match count, Esc closes (GPUI find bar needed)
- [ ] Print opens the print preview or dialog (Ctrl+P and menu)
- [ ] Save page as (Ctrl+Shift+S and menu), single-file MHTML; failure notice "Could not save the page"
- [ ] Developer tools (Ctrl+Shift+I, F12, menu) and JavaScript console (Ctrl+Shift+J); Inspect in context menu
- [ ] Zoom chip (shown when not 100%) and zoom bubble (−, +, Reset; no focus taken)
- [ ] Ctrl+plus, Ctrl+minus, Ctrl+0 and Ctrl+wheel zoom in Chrome's preset levels
- [ ] Zoom remembered per host in core (local), 100% forgets, new documents take the site level; private zoom private
- [ ] Site icon (lock, warning, search, page) from the security state of the current document
- [ ] Site-info popup: headline, host, trackers, cookies, permissions, TLS line, certificate details, chain, Show certificate
- [ ] Private windows: Ctrl+Shift+N, menu item, link context item; dark theme, "Private" pill, "(Private)" titles
- [ ] Private windows use an incognito engine context; nothing persisted (history, favicons, session, suggestions); logs scrubbed
- [ ] Private windows: no profile button, no extension buttons, popups or shortcuts (unless per-extension incognito is adopted)
- [ ] Private session ends with the last private window (downloads cancelled, closed tabs and core private state dropped, engine data gone)
- [ ] Load failures show an error page at the failing URL; URL kept for the session; HTTPS-only warning takes precedence
- [ ] Certificate errors fail the load (feeding HTTPS-only's warning when upgrading)
- [ ] Renderer crash handled (Windows only logs; decide on a crash page)
- [ ] Favicon per tab (strip, pane, media player, tab search, closed tabs); stale icon loads dropped
- [ ] Favicon stored only for bookmarked pages or sites, never for private tabs; stored on star
- [ ] Bookmark icon preload through core's `FaviconFetch` (interactive runs)
- [ ] Startup plan: restore session, homepage, or new tab; command-line URLs added to the first window
- [ ] Restored tabs show saved title until loaded; keep session tab ids, pinned state, groups, bounds, maximized
- [ ] Session saves debounced 2 s; last tab of last window saved before closing; private windows never saved
- [ ] History visits with Typed, Bookmark, Reload or Link attribution; titles recorded; none for private tabs or the HTTPS warning
- [ ] Pop-up blocking (pref) for windows without a user gesture; allowed pop-ups become opener-linked tabs
- [ ] Ctrl+click and middle-click open background tabs placed after earlier page-opened tabs
- [ ] `window.close()` closes the tab
- [ ] Context menu: selection search (core engine), "Copy link without tracking", "Open link in private window", extension items
- [ ] Reserved, Overridable and Native shortcut classes while the page has focus; live key-set updates
- [ ] Page fullscreen hides chrome; leaving on tab switch; windowed bounds kept for session
- [ ] Tab mute or unmute and audible indicator; media player follows the last audible tab
- [ ] Load progress bar along the address pill
- [ ] Password saving off; address autofill switch applied live
- [ ] Clear browsing data (history everywhere plus this device's cookies, site data, cache) with status text
- [ ] Media player controls and picture-in-picture box via the page media script, gated by site permission and prefs
- [ ] Web Store and Edge Add-ons page installs through core (see the extension inventory)
- [ ] New tab page HTML reading as `about:blank` ("New tab", empty address, not in history or session); private variant
- [ ] Self-test: report all engine checks in `report::CHECKS` order with CEF equivalents of the listed primitives
- [ ] UI smoke: tracker, permission, connection, picture-in-picture, progress and find steps
