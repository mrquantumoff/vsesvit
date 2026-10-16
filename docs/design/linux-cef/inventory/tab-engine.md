# Inventory: the tab and the engine (vsesvit-gtk → GPUI + CEF)

Scope: `crates/vsesvit-gtk/src/{tab.rs, engine.rs, location.rs, error_page.rs, zoom.rs, find_bar.rs, view_source.rs, save_page.rs, screenshot.rs, page_menu.rs, closed_tabs.rs}` on branch `linux-cef-gpui` (worktree `/home/dy/git/vsesvit-cef`). Where these files only make sense together with their callers, the callers are covered too: `window/mod.rs`, `window/actions.rs`, `window/menu.rs`, `browser.rs`, `address_bar.rs` (zoom indicator, security icon), `session.rs`, `favicons.rs::faded`, `downloads.rs::download_to`, `extension_menus.rs`, `keymap.rs`, `app.rs` (CSS, debug screenshot), `self_test.rs`.

The aim is that a new GPUI + CEF Linux shell can reproduce every user-visible behavior here without reading the old code. This is not an architecture proposal.

## Legend

- **(KEEP)**: product behavior the new shell must reproduce.
- **(WEBKIT-ONLY)**: a WebKitGTK workaround or limitation. It goes away with Chromium, or Chromium does it natively.
- **(NEEDS-CEF)**: needs a CEF equivalent. The likely CEF handler or API is named.
- **[V]**: verified by reading code (path:line given) or the CEF headers and bindings.
- **[I]**: inference: memory of WebKitGTK, Chromium or CEF behavior, not verified here.

CEF names were checked in two places [V]:
- the cef-rs sys bindings `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cef-dll-sys-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs`;
- the CEF source checkout already on disk at `/tmp/vsesvit-research/cef/cef-8037/include/*.h`, whose `CHROMIUM_BUILD_COMPATIBILITY.txt` says `refs/tags/154.0.8037.98`.

The DevTools protocol commands were checked against tip-of-tree `Page.pdl` at github.com/ChromeDevTools/devtools-protocol, not against a copy pinned to M154.

---

## 0. Cross-cutting model

### 0.1 What a tab is (tab.rs:114-178) [V]

A `Tab` is an `adw::Bin` holding a vertical box: an `adw::Banner` for screen sharing, then an overlay of the web view plus the link-preview label (tab.rs:213-253). Each tab carries:

| Field | Meaning | Persisted? |
|---|---|---|
| `id: vsesvit_webext::TabId` | Extension-runtime id (what `chrome.tabs` sees). Allocated by `Browser::allocate_tab_id`. | no |
| `session_id: vsesvit_core::session::TabId` (UUID) | Key of the tab's saved record and back/forward blob in the profile session. | yes, via `session.rs` |
| `browsing: vsesvit_core::private::Browsing` | Normal or Private, fixed for life. Decides the network session, which closed-tab stack it uses, and whether core writes happen. | no |
| `gate: vsesvit_webext::Gate` | Navigation policy (§5). | no |
| `last_active_ms` | Last selection time in wall-clock ms (`session::now_ms`). | yes (`TabSnapshot::last_active_ms`) |
| `used: u64` | MRU tick for tab search, from `Browser::tab_used` / `tick`. | no (closed tabs keep it) |
| `pending_transition: Option<vsesvit_core::history::Transition>` | How the next commit was reached (Typed, Bookmark, Reload); default Link. | no |
| `permissions: TabPermissions` | One-time grants plus waiting permission prompts (permissions inventory). | no |
| `load: LoadPhase` (Idle/Provisional/Committed) | WebKit load phase tracking. | no |
| `committed_uri` | URI of the document on screen, as opposed to one still loading. | used by session |
| `error_page_pending/shown: Option<ErrorPage>` (Certificate / HttpsOnly / Other) | Which built-in error page is being shown. | no |
| `https: vsesvit_core::https_only::Upgrades`, `https_judged`, `https_warning_due` | HTTPS-only per-tab state (§6). | no |
| `typed: Option<String>` | Address-bar text typed in this tab but not submitted, kept while another tab is shown. | no |
| `selection: String` | Last text selected in the page, for the context menu. | no |
| `rest: Rest` = `Awake(IdleClock)` or `Asleep{title, icon}` | Memory Saver state (§12). | no |
| `related: bool` | Has a `window.opener` relationship (opened one, or was opened). | no |
| `edited: bool` | A frame of the page was edited since commit (§12). | no |

### 0.2 The tab-to-window change contract (tab.rs:45-62, window/mod.rs:1396-1458) [V]

A tab reports to whatever window holds it now, found through the widget tree at the time of the change; tabs move between windows. `Tab::notify(change)` calls `BrowserWindow::tab_changed(tab, change)`. While the tab sleeps, `Title` and `Favicon` are suppressed (tab.rs:607-614).

| `TabChange` | Source (WebKit) | Window reaction |
|---|---|---|
| `Title` | `notify::title` | tab page title = `display_title()`; window title if selected; `Browser::title_changed` (history title, `runtime.tab_updated`, session save) |
| `Favicon` | `notify::favicon` | tab icon = `icon()`; `Browser::favicon_changed` (stores icon for bookmarked pages; normal tabs only) |
| `Loading` | `notify::is-loading` | tab spinner (`page.set_loading`); if selected: reload↔stop button and progress; when loading stops, the address bar shows the committed URI again (a load can end without committing, e.g. a download or a stop) |
| `Progress` | `notify::estimated-load-progress` | if selected: address-bar progress line |
| `Committed(Document/SameDocument/ErrorPage)` | `load-changed::Committed`, plus same-document detection (§4) | tab title; if selected: location, security, back/forward, star, permission prompt; `Browser::navigation_committed` |
| `History` | back/forward list `changed` | if selected: enable `win.back` / `win.forward` |
| `Zoom` | `notify::zoom-level` | `Browser::zoom_changed` (stores per-site zoom); if selected: menu zoom label, address-bar zoom pill |
| `Find(Matches(n)/NotFound)` | FindController signals | if selected: find bar status |
| `Capture` | camera/microphone/display capture state notify | tab indicator; address-bar in-use button if selected; sharing banner (tab-local) |
| `Audio` | `notify::is-playing-audio`, `notify::is-muted` | tab indicator (speaker) |
| `Sleep` | Memory Saver sleep/wake | tab icon (faded or normal) |

**(KEEP)** The new shell needs the same set of change notifications from CEF:
- Title: `CefDisplayHandler::OnTitleChange`.
- Favicon: `OnFaviconURLChange` plus `CefBrowserHost::DownloadImage`.
- Loading: `CefLoadHandler::OnLoadingStateChange(isLoading, canGoBack, canGoForward)`, which also covers History.
- Progress: `CefDisplayHandler::OnLoadingProgressChange`.
- Commit: `CefLoadHandler::OnLoadStart` (main frame), which per the CEF docs is called *after commit*, and `CefDisplayHandler::OnAddressChange` for same-document commits.
- Find: `CefFindHandler::OnFindResult`.
- Capture: `CefDisplayHandler::OnMediaAccessChange`.

Zoom and Audio have no CEF callback (see §11 and §18).

### 0.3 Identity and display helpers (tab.rs:414-503) [V] (KEEP)

- `committed_uri()`: the document on screen.
- `session_uri()`: the committed URI, else the requested URI if it is non-empty and not `about:blank`. A restored or background tab that has not committed yet still saves and lists its address.
- `display_title()`: the saved title if asleep; else the page title if not blank (after trim); else `display_uri(committed)` if not `about:blank`; else **"New Tab"**.
- `link()`: committed URI unless `about:blank`. `None` disables Copy Link.
- `is_blank()`: not asleep, and the view URI is empty or `about:blank`. The new-tab page is loaded with `load_html(html, None)`, so it sits at `about:blank` and the tab still reads as blank (window/mod.rs:1171-1183).
- `icon()`: the faded saved icon if asleep, else the view favicon.
- `security()` → `address_bar::Security`:
  - certificate or HTTPS-only error page → `Insecure`;
  - other error page → `NotApplicable`;
  - else `Security::of(committed)`: `https` → `Secure`, `http` → `Insecure`, any other scheme except `about:blank` → `Internal`, none or `about:blank` → `NotApplicable` (address_bar.rs:133-156).
  - The icons and tooltips are those of address_bar.rs:559-577: `channel-secure-symbolic` "Secure connection", `channel-insecure-symbolic` "This connection is not secure", `dialog-information-symbolic` "View site information".
- `display_uri(uri)` (tab.rs:1172): `about:blank` → empty; else `webkit::functions::uri_for_display` (decodes punycode and percent escapes). **(WEBKIT-ONLY)** Use core's engine-independent `vsesvit_core::address::readable_url` instead; the Windows shell already does (`vsesvit-winui/src/window/mod.rs`) [V].

---

## 1. Engine setup (engine.rs)

### 1.1 What it does [V]

- `Engine::new(profile)` (engine.rs:60-79):
  - calls `add_appimage_to_sandbox()` and `view_source::register()`;
  - creates one persistent `webkit::NetworkSession::new(data = ProfilePaths::engine_data, cache = ProfilePaths::engine_cache)`. A non-UTF-8 profile path panics on purpose, because WebKit would silently fall back to shared default directories;
  - creates one shared `webkit::Settings` with developer extras on, fullscreen on, and back/forward swipe gestures on;
  - calls `apply_prefs`.
- `Engine::ephemeral_session()`: a `NetworkSession::new_ephemeral()` for the private session. It is set up the same way and keeps the default web context, so extension pages, view-source and spell check still work (engine.rs:116-120).
- `set_up(session)`: `TLSErrorsPolicy::Fail`, so certificate errors fail the load and the shell shows its own page (§7). Favicons are enabled in the website data manager.
- `web_view(session, ucm)`: every tab's view gets the shared settings plus the extension runtime's per-tab `UserContentManager`, which carries content scripts, the tracking-protection blocker and the cookie-rules blocker.
- `related_web_view(opener, ucm)`: for `window.open` and `target=_blank`. WebKit requires a related view, which shares the opener's session and web process.
- `apply_prefs(profile)` (engine.rs:83-102) applies these core prefs live to every view:
  - `keys::BLOCK_POPUPS` (`content.block_popups`, synced, default true) → `javascript_can_open_windows_automatically = !block`. Blocking only stops windows a page opens without a user gesture; `window.open` from a click still opens.
  - `keys::SMOOTH_SCROLLING` (`scrolling.smooth`, synced, default true).
  - `keys::HARDWARE_ACCELERATION` (`system.hardware_acceleration`, LOCAL, default true) → policy Always or Never.
  - `keys::SPELLCHECK` (`spellcheck.enabled`, synced, default true) and `keys::SPELLCHECK_LANGUAGES` (`spellcheck.languages`, synced, default None = the system's) → `vsesvit_core::spellcheck::Dictionaries::new(spellcheck::installed_in(dirs), &spellcheck::system_locales()).checked(chosen)`. Checking is off when no language resolves, because WebKit given no languages checks the system's. Dictionary dirs: `~/.config/enchant/hunspell`, `/usr/share/hunspell`, `/usr/share/myspell`, `/usr/share/myspell/dicts`.
- Callers of `apply_prefs`:
  - `Browser::set_engine_switch` (Settings switches);
  - `Browser::set_spellcheck_language`;
  - `Browser::sync_applied` whenever any synced pref changed (browser.rs:1196-1210, 1326-1333).
- `add_appimage_to_sandbox()` adds `$APPDIR` (only when the executable is inside it) and `~/.cache/vsesvit/appimage` to the WebKit sandbox once per process (engine.rs:22-45).

### 1.2 Classification

| Piece | Class | CEF note |
|---|---|---|
| Profile-persistent engine data/cache dirs | (KEEP) / (NEEDS-CEF) | `CefSettings.root_cache_path` and `CefRequestContextSettings.cache_path` under `ProfilePaths::engine_data`. CEF keeps cache inside the profile dir, so `engine_cache` may become unused [I]. |
| Private session in memory | (KEEP) / (NEEDS-CEF) | `CefRequestContext::CreateContext` with an empty `cache_path` = "incognito mode", in-memory storage ([V] `cef_types.h` comment, lines 524-528). |
| TLS errors fail, no bypass | (KEEP) | `CefRequestHandler::OnCertificateError` returns false to cancel; then `OnLoadError` with an `ERR_CERT_*` code [V signature]. |
| Favicons enabled | (NEEDS-CEF) | `OnFaviconURLChange` gives URLs only; fetch with `CefBrowserHost::DownloadImage`. |
| Developer extras | (KEEP) | DevTools exist by default in CEF. |
| Fullscreen enabled | (KEEP) | Chromium default; the shell must implement the window side (§10). |
| Back/forward touchpad swipe | (NEEDS-CEF) / open | Chromium's overscroll history navigation on Linux [I]. |
| Popup blocking pref, live | (KEEP) / (NEEDS-CEF) | Decide in `CefLifeSpanHandler::OnBeforePopup(..., user_gesture, ...)`: return true to block when `BLOCK_POPUPS` and `!user_gesture`. Alloy style has no built-in popup blocker [I]. |
| Smooth scrolling pref, live | (NEEDS-CEF), risk | In Chromium this is a command-line switch (`--disable-smooth-scrolling`), process-wide, not live [I]. |
| Hardware acceleration pref, live | (NEEDS-CEF), risk | `--disable-gpu` / `--disable-gpu-compositing` are command-line flags: process-wide, need a restart [I]. |
| Spell check on/off and languages | (NEEDS-CEF), risk | `CefRequestContext::SetPreference("browser.enable_spellchecking", …)` and `("spellcheck.dictionaries", […])` [I]. Chromium uses `.bdic` dictionaries it downloads, not the system Hunspell `.dic/.aff` files that core's `spellcheck::installed_in` lists [I]. |
| AppImage sandbox paths | (WEBKIT-ONLY) | CEF has its own sandbox (`chrome-sandbox` SUID or user namespaces); packaging inventory. |
| `view_source::register()` | (WEBKIT-ONLY) | Chromium renders `view-source:` natively (§20). |
| Related view for popups | (WEBKIT-ONLY mechanism) | CEF popups from `OnBeforePopup` share the opener's request context and process when scriptable. |

---

## 2. Tab construction (tab.rs:180-253)

### 2.1 Behavior [V]

`Tab::new(browser, browsing)`:
1. allocates the id;
2. `runtime.user_content_manager(id, browsing)`;
3. `trackers().attach(ucm)` and `cookies().attach(ucm)`;
4. `network_session(browsing)`, which creates the private engine on first use (browser.rs:337-356);
5. `engine.web_view`.

`Tab::new_related(browser, opener)` does the same as the opener's kind, but uses a related view. Its gate is `Gate::opened_by(opener.gate)`, and both tabs get `related = true`.

`wrap` builds:
- **Link preview** (KEEP): a `gtk::Label` overlaid at the bottom-left of the page. Middle ellipsis, at most 90 chars, not clickable (`can_target(false)`), hidden by default, CSS `.link-preview`: `padding: 3px 8px; border-top-right-radius: 6px; background @view_bg_color; color @view_fg_color; box-shadow 0 0 0 1px alpha(currentColor,0.15); font-size 0.9em` (app.rs:31-39). Behavior in §9.
- **Screen sharing banner** (KEEP): `adw::Banner` above the page. Button "Stop sharing" sets the display capture state to None. Title `permissions::sharing_title(committed)` = "Sharing your screen with {host}" ("this page" fallback). Revealed while the screen is captured (tab.rs:316-324; permissions.rs:398-401).
- `connect_web_view()` (§0.2 and below).
- `page_menu::attach` (§17), `track_edits` (§12), `browser.tab_used` (MRU tick).

### 2.2 CEF notes

- **(NEEDS-CEF)** Hosting a CEF browser inside a GPUI tab.
  - In windowed mode the browser is a native X11 child window. GPUI cannot draw *over* it, so overlays drawn on top of the page (link preview, fullscreen notice, sharing banner if overlaid, find bar if overlaid, permission bubbles) need separate popup surfaces, or the page must be off-screen rendered (OSR: `CefRenderHandler::OnPaint` / `OnAcceleratedPaint`) [I].
  - In the GTK shell the banner sits *above* the page in the layout (not over it). The link preview and fullscreen notice are true overlays.
- **(WEBKIT-ONLY)** The per-tab `UserContentManager` from `vsesvit_webext`. It disappears if Chromium's own extension system runs extensions. Tracking protection and cookie rules then need Chromium-side mechanisms (other inventories).

---

## 3. Navigation commands (tab.rs:326-374; window/actions.rs; window/mod.rs:1240-1250)

### 3.1 Behavior [V] (KEEP)

| Command | UI | What happens |
|---|---|---|
| Load (address bar, bookmark, `tabs.update`, CLI, home) | — | `Tab::load(uri)`: `gate.browser_load(uri)` then `load_uri`. `BrowserWindow::navigate_with(uri, transition)` sets `pending_transition` (Typed for omnibox, Bookmark for bookmarks and home), loads in the selected tab (opens one if none), then focuses the web view. |
| Reload | `win.reload`, Ctrl+R / F5; header button `view-refresh-symbolic` "Reload"; tab menu Reload | sets `Transition::Reload`; `browser_load(committed or requested uri)`; `reload()` |
| Reload bypassing cache | `win.reload-bypass-cache`, Ctrl+Shift+R / Shift+F5 | as above with `reload_bypass_cache()` |
| Stop | `win.stop`: the reload button turns into `process-stop-symbolic` "Stop" while loading (window/mod.rs:1534-1552). No keyboard shortcut. | clears `pending_transition` (a stopped load commits nothing, so its transition must not go to a later visit), `gate.stop()`, `stop_loading()` |
| Back / Forward | `win.back` / `win.forward`, Alt+Left / Alt+Right; header buttons `go-previous-symbolic` "Back", `go-next-symbolic` "Forward" | `browser_load(back/forward item uri)` then go. Actions enabled from `can_go_back` / `can_go_forward` on History change and tab selection. |
| Home | `win.home`, button `go-home-symbolic` "Home" (shown only with `keys::SHOW_HOME_BUTTON`) | homepage (`location::homepage_url(keys::HOMEPAGE)`) via `navigate_with(…, Bookmark)`; or, when the homepage is `about:home`, `about:blank` or not an address, the new-tab page in the selected tab, with focus in the address bar |
| Ctrl/middle click on a link | — | §5.3 |

The address-bar progress line shows `estimated_load_progress` while loading and finishes at the end (window/mod.rs:1547-1551). Self-test `address_progress` checks a value strictly between 0 and 1 while an image stalls, and 0 once the load ends.

### 3.2 Transitions → history (browser.rs:838-856) [V] (KEEP)

`Browser::navigation_committed`:
- takes `pending_transition` (default `Transition::Link`);
- records `history().record_visit(url, transition)` for normal tabs and non-error commits, including same-document commits;
- shows the site zoom for non-same-document commits (§18);
- calls `runtime.tab_updated`;
- schedules a session save.

Linux never sets `Redirect` or `FormSubmit` [V grep]. Tests that pin this behavior (tab.rs:1276-1342):
- a stopped load leaves no transition;
- a download leaves no transition (the response-policy download clears it);
- an address typed over a stalled load still counts as typed (`typed_count == 1`).

`Browser::title_changed` (browser.rs:891-906) writes `history().set_title(committed, title)`, but only for normal tabs that are not on an error page and only for a non-empty title. WebKit clears the title as the next document commits, before it reports the commit, so an empty title would land on the page being left.

### 3.3 CEF notes

- **(NEEDS-CEF)** `CefFrame::LoadURL`, `CefBrowser::Reload`, `ReloadIgnoreCache`, `StopLoad`, `GoBack`, `GoForward`, `CanGoBack`/`CanGoForward` (also passed to `OnLoadingStateChange`).
- Chromium gives a navigation's transition natively: `CefRequest::GetTransitionType` in `OnBeforeBrowse`, and `transition_type` in `OnLoadStart`. The shell still has to map it to core's `Transition`; the pending-transition bookkeeping may become simpler [I].
- **(KEEP)** A stopped or aborted load and a download must not leave a transition for the next visit.

---

## 4. Load lifecycle and commit tracking (tab.rs:826-907)

### 4.1 Behavior [V]

`load_changed`:
- **Started / Redirected**:
  - wakes a sleeping tab (`Rest::default()` plus a `Sleep` notification);
  - phase becomes Provisional;
  - unless an error page is pending, tells the runtime `Load::Started(uri)` / `Load::Redirected(uri)` (webNavigation);
  - calls `https_started(redirect)` (§6).
- **Committed**:
  - phase Committed; `edited = false`; `https.committed()`; `gate.committed(runtime, uri)`;
  - moves `error_page_pending` into `error_page_shown`;
  - unless on an error page, tells the runtime `Load::Committed(uri, transition)`;
  - sets `committed_uri`; `permissions::committed(tab)` (expires one-time grants, denies prompts of the site left);
  - notifies `Committed(Document)` or `Committed(ErrorPage)`.
- **Finished**: phase Idle; unless on an error page, `Load::Finished`; then `check_same_document_commit()`.

`check_same_document_commit` (tab.rs:886-907) is **(WEBKIT-ONLY)**. WebKit emits no load events for fragment and History API navigations, so a URI change while no load is in flight, to the back/forward list's current entry, is treated as a same-document commit. One made during a load commits when the load finishes. The test `a_history_navigation_made_while_the_page_loads_commits_when_it_finishes` pins this.

**(KEEP)** What the workaround produces:
- `committed_uri` follows fragment and `pushState` navigations;
- they record history visits;
- they do not reset site zoom;
- the address bar, star and permissions follow them.

CEF: `CefDisplayHandler::OnAddressChange(frame main)` fires for same-document navigations, and `OnLoadStart` is documented as not called for them [V header text].

### 4.2 Classification

| Piece | Class |
|---|---|
| Committed-vs-requested URI distinction (address bar shows the committed page; extensions see the committed URL while the next page loads; test browser.rs:1854-1873) | (KEEP) |
| Same-document detection by URI/back-forward-list heuristics | (WEBKIT-ONLY) → `OnAddressChange` plus the absence of `OnLoadStart` [I] |
| `vsesvit_webext` `tab_load` events (webNavigation) | (WEBKIT-ONLY) if Chromium's extension system is used. Otherwise (NEEDS-CEF) from `OnBeforeBrowse` / `OnLoadStart` / `OnLoadEnd` / `OnLoadError`. |
| Waking on load start | (KEEP) in spirit (§12) |

---

## 5. The navigation gate and decide-policy (tab.rs:943-1008, 1119-1168; vsesvit-webext/src/gate.rs)

### 5.1 decide-policy [V]

`decide_policy(decision, kind)`:

- **NavigationAction / NewWindowAction**:
  1. Read target URI. `new_window = kind == NewWindowAction`. `browser_load = gate.started_by_browser(target)`.
  2. If `!may_navigate(action, target, new_window)`: record as refused (test hook), `decision.ignore()`.
  3. `new_tab_for_click(navigation_type, mouse_button, modifiers)` (tab.rs:1153-1168), for a **LinkClicked** action with the **middle button**, or with the **primary button plus Ctrl**:
     - Shift held → `Focus::Foreground`, else `Focus::Background`;
     - `window.open_tab(uri, opener = self, focus)`, then `ignore()`.
     - A plain click, or Shift+click without Ctrl, stays in the tab (unit tests tab.rs:1189-1227).
  4. Otherwise, for a same-window navigation, HTTPS-only may stop it (`https_policy`, §6). Everything else gets WebKit's default.
- **Response**: when the main frame's main resource has a MIME type WebKit cannot show: clear `pending_transition` (the page stays, so the transition goes to no visit), then `decision.download()`. Otherwise default. WebKit's default also downloads `Content-Disposition: attachment` [I].
- **`create` (window.open / target=_blank)**, via `vsesvit_webext::connect_create` (views.rs:218-232): if the target is known and `!may_navigate(…, new_window = true)`, refuse (return no view). Otherwise `create_related()` (§8).

### 5.2 The gate (vsesvit-webext/src/gate.rs:1-137) [V]

Purpose: as in Chrome, a web page reaches an extension's `chrome-extension://` pages only where `web_accessible_resources` lets it; the extension itself and the browser reach them all. **Only the browser opens `view-source:`**, which no page reaches, not even a local `file://` page.

WebKit does not say who started a navigation, so the gate tracks three things:
- `browser_load(target)`: the browser's own load (address bar, reload, history, restored session, `tabs.update`), which is allowed once. A server redirect is never the browser's.
- `opened_by(opener)`: a window a page opened is also judged by that page, for as long as both live, by reference or by name.
- Which page an extension page "speaks for".

`decide()`:
1. Allows a non-redirect, non-new-window browser load.
2. Refuses `view-source:`.
3. Otherwise requires `Policy::may_navigate(source, target)` (= `Runtime::may_navigate`: `patterns::may_enter(source, target)` or the extension's `web_accessible(path, source)`), and the same for the opener.

`committed()` updates whose page the document speaks for. `stop()` forgets a pending browser load.

Integration tests (tab.rs:1344-1568) pin all of this:
- window.open and `target=_blank rel=noreferrer` to a non-web-accessible options page are refused; the web-accessible page and a site still open;
- a `no-referrer` self-navigation is refused, and so is a reload of it;
- a 302 with `Referrer-Policy: no-referrer` is refused, even when typed;
- typing the options URL works; from the options page, going to the bouncing site does not bounce back;
- restoring history to the options page works;
- a held window sent to the options page by reference, by name, or via `opener.location` is refused;
- browser reload, back, forward and restore reach it.

### 5.3 Classification

| Piece | Class | CEF note |
|---|---|---|
| Web pages cannot reach non-web-accessible extension pages | (KEEP) behavior; mechanism (WEBKIT-ONLY) if Chromium's extension system serves `chrome-extension://` | Chromium enforces `web_accessible_resources` natively for extension URLs [I]. |
| Pages (web or `file://`) cannot open or embed `view-source:`; browser loads can | (KEEP) | Chromium blocks renderer-initiated `view-source:` navigations and subframes natively [I, verify]. Otherwise `CefRequestHandler::OnBeforeBrowse` with `user_gesture` / `CefRequest::GetTransitionType`. |
| Gate heuristics (browser_load, opener tracking) | (WEBKIT-ONLY) | `OnBeforeBrowse` has `frame`, `is_redirect`, `user_gesture`, the request; `GetResourceRequestHandler` has `request_initiator` [V signatures]. |
| Ctrl+click / middle-click open a background tab next to the opener; Ctrl+Shift / middle+Shift a foreground tab | (KEEP) | `CefRequestHandler::OnOpenURLFromTab(target_disposition)` with `CEF_WOD_NEW_BACKGROUND_TAB` / `NEW_FOREGROUND_TAB` [V]. **Divergence:** Chromium maps Shift+click to `NEW_WINDOW` and Alt+click to save-to-disk [I]; Linux today keeps Shift+click in the tab. Product decision. |
| New tab from a click: `runtime.tab_opened_by(tab, opener)`, joins the opener's tab group, inserted next to the opener (`add_page(tab, parent)`) | (KEEP) | window/mod.rs:1287-1298 |
| Unsupported MIME → download, without leaving a transition | (KEEP) | Chromium turns non-renderable responses into downloads → `CefDownloadHandler::CanDownload` / `OnBeforeDownload` [V names]; the navigation ends with `OnLoadError(ERR_ABORTED)` [I]. |

---

## 6. HTTPS-only (tab.rs:1010-1098; vsesvit_core::https_only)

### 6.1 Product behavior [V] (KEEP)

Chrome's HTTPS-First mode is the synced pref `keys::HTTPS_ONLY` (`privacy.https_only`, default false). The Settings row is `https_only::TITLE` = "Always use secure connections"; its description is `https_only::DESCRIPTION`.

- **Upgrade:** a main-frame http navigation loads its https URL instead. Only public sites on the default port are upgraded (`Reach::Public`): no localhost, private, link-local or CGNAT (100.64/10) addresses, no single-label names, no local TLDs `local, localhost, internal, lan, home, corp, test, invalid, example`, no `home.arpa`.
- **Warn:** when the upgraded load fails (network, TLS or certificate error), or redirects back to http on a host already upgraded, the tab shows `https_only::warning_page(http_url)` *at the http address*:
  - `<title>` = `WARNING_TITLE` "Connection is not secure";
  - heading "This site doesn't support a secure connection";
  - body "Attackers might be able to see or change what you send to or receive from **{host}**." and "You're seeing this warning because Always use secure connections is on, and the site doesn't offer a secure connection.";
  - button link `<a id="continue" href="{http url}">Continue to site</a>`;
  - no script; escaped; light/dark CSS.
- **Continue:** "Continue to site" is a link navigation to the same http URL. `Upgrades::starting` answers `Next::Allow(url)`, and the shell calls `https_only::allow(profile, browsing, url)`, which stores `Permission::Http = Setting::Allow` for the origin. That setting is synced, listed in Settings' site list, and in private it is kept in memory for the private session.
- The address bar marks the warning page **Insecure** (§0.3).

### 6.2 Core contract [V]

- `Browser::https_upgrade(browsing, url)` = `https_only::upgrade(&mut profile, browsing, url, reach)`. `reach` is `Reach::Public`, or `Reach::Everywhere` set by the self-test (`Browser::set_https_reach`). It returns `Some(https)` only when the pref is on, `upgraded(url, reach)` applies, and `!allowed(profile, browsing, origin)`.
- Per tab, `Upgrades::starting(url, cause, upgrade) -> Next` gives `Load`, `Upgrade(https)`, `Warn(http)` or `Allow(url)`. `Cause` is `Link` (the page's own link click, which the warning's Continue is), `Redirect` or `Other`.
- `Upgrades::finished(ok)` gives the http URL to warn about after a failed upgrade. `Upgrades::committed()` runs on each commit. `Upgrades::leave()` runs on browser navigations: **Windows calls it; Linux never does** [V grep] (minor divergence).
- `https_only::allowed(profile, browsing, origin)` is used by the self-test.

### 6.3 Linux mechanism [V]

1. `https_policy` runs in decide-policy for same-window NavigationActions only when the navigation is surely the main frame's: either the browser started it, or `holds_no_http_frame()` (the document on screen is an error page, nothing, `about:blank`, or `https:`, where an http frame would be blocked mixed content).
   - Cause: `Redirect` if `is_redirect`; else `Link` if not a browser load and `LinkClicked`; else `Other`.
   - It records `https_judged = target`, then calls `https_next(target, cause, stop = decision.ignore)`.
2. Otherwise the load is judged when it starts (`https_started` on Started or Redirected, unless it was judged already): cause `Redirect` or `Other`, and stop = `stop_loading()`. So one http request may already be out, but its page never shows.
3. `https_next`:
   - `Upgrade(https)` → stop, then `self.load(https)` (a browser load);
   - `Warn(http)` → `https_warning_due = http` and stop; WebKit then reports a benign `Cancelled` failure, and `load_failed` shows the warning (§7);
   - `Allow` → store the exception and continue;
   - `Load` → continue.
4. A failed upgraded load (`load-failed` provisional, or `load-failed-with-tls-errors`) → `https_failed()` → `Upgrades::finished(false)` → the warning page instead of the generic error page.

### 6.4 Classification

| Piece | Class | CEF note |
|---|---|---|
| Upgrade/warn/continue policy and wording | (KEEP) | core decides; the shell feeds events |
| Main-frame-only judging heuristics (`holds_no_http_frame`, judge at load start) | (WEBKIT-ONLY) | `CefRequestHandler::OnBeforeBrowse(browser, frame, request, user_gesture, is_redirect)`: check `frame->IsMain()`, return true to cancel before any request goes out [V signature]. Cancel produces `OnLoadError(ERR_ABORTED)` [V header text]. |
| Cause::Link detection | (NEEDS-CEF) | `CefRequest::GetTransitionType()` == `TT_LINK` (minus qualifiers) plus `user_gesture` [I]. Redirects: `is_redirect` in `OnBeforeBrowse` [V] or `CefResourceRequestHandler::OnResourceRedirect` [I]. |
| Warning page shown at the http URL | (KEEP) / (NEEDS-CEF) | CEF has no `load_alternate_html`; see §7.3. |
| Failure of the upgraded load triggers the warning | (KEEP) / (NEEDS-CEF) | `CefLoadHandler::OnLoadError` (any net error, including `ERR_CERT_*` after `OnCertificateError` returns false). |
| Chromium's built-in HTTPS-First (`HttpsUpgradesInterceptor`, Chrome layer) | alternative [I] | Its exceptions would not be core's synced ones; the product rule is that core decides. |

Self-test `https_only` (self_test.rs:3221-3285):
- turns the pref on through Settings > Privacy (`HTTPS_ONLY_ROW`), off before; finds the Secure DNS row text;
- sets `Reach::Everywhere`; `/index.html` over https fails, and the tab must show the title "Connection is not secure" at the http URL, with `tab.security() == Insecure`; writes `https-only-warning.png`;
- clicks `#continue` by script → "Vsesvit fixture" over http, and `https_only::allowed` is true.

`view_source` keeps to HTTPS-only too (§20).

---

## 7. Error pages (error_page.rs; tab.rs:671-708, 909-941)

### 7.1 What the user sees [V] (KEEP)

Error pages are self-contained HTML **shown under the URL that failed**. The address bar keeps the failed address, and reload retries it. They carry no script, and every interpolated string is escaped with `vsesvit_core::html::escape`. They share a layout (error_page.rs:362-390):
- `<meta name="color-scheme" content="light dark">`, system-ui font;
- light `#1e1e1e` on `#fafafb`, dark `#eeeeee` on `#222226`;
- body max 36rem, margin 12vh auto;
- `h1` 1.6rem / 800;
- `.detail` at 0.7 opacity, 0.9rem;
- a blue `#3584e4` button `<a class="button" href="{failed uri}">Try Again</a>`.

| Kind | `<title>` | `<h1>` | Body |
|---|---|---|---|
| Certificate (`tls_error`) | "Security Warning" | "This connection is not secure" | "Vsesvit could not confirm that you are connected to **{host}**. Someone could be pretending to be this site to steal what you send it." then a bullet per problem, then "The page was not loaded." |
| Load failed (`load_failed`) | "Problem Loading Page" | "Unable to load this page" | "Vsesvit could not load **{host}**." plus `<p class="detail">{engine error message}</p>` |
| Crashed (`crashed`) | "Page Crashed" | "This page crashed" | "The page at **{host}** stopped working and was closed." |
| HTTPS-only warning | §6.1 | | |

Certificate problem bullets (error_page.rs:327-353), in this order; when none apply, "The certificate could not be validated.":

| Flag | Text |
|---|---|
| UNKNOWN_CA | "The certificate was not issued by an authority this browser trusts." |
| BAD_IDENTITY | "The certificate belongs to a different site." |
| NOT_ACTIVATED | "The certificate is not valid yet." |
| EXPIRED | "The certificate has expired." |
| REVOKED | "The certificate has been revoked." |
| INSECURE | "The certificate uses an insecure algorithm." |
| GENERIC_ERROR | "The certificate is not valid." |

`{host}` is the URL's host, or the whole URI when it has none. There is **no "proceed anyway"** for certificate errors.

### 7.2 When each shows [V]

- `load-failed`:
  - If an error page was pending, this is the error page itself failing (e.g. a port WebKit refuses), so nothing replaces it and no event goes to the runtime.
  - Otherwise the runtime gets `Load::Failed(uri, NetError::of(error))`.
  - Benign errors are `NetworkError::Cancelled`, `PolicyError::FrameLoadInterruptedByPolicyChange` (ignored navigation or a download) and `MediaError::Load`. A benign error with an HTTPS-only warning due shows the warning.
  - Benign errors, failed error pages, and failures after commit (event ≠ Started) get WebKit's default (nothing replaced) [V], [I for what WebKit's default does].
  - Otherwise: an HTTPS-only warning if an upgrade failed, else the "load failed" page.
- `load-failed-with-tls-errors`: the runtime gets `Load::Failed(uri, CertificateInvalid)`; then the HTTPS-only warning if it was an upgrade, else the certificate page.
- `web-process-terminated` with a reason other than `TerminatedByApi` (Memory Saver uses that one): log a warning, then the "crashed" page under the view's URI.
- While the error page shows (`error_page_shown`):
  - no webNavigation events for it;
  - no history visit or title;
  - `Commit::ErrorPage`;
  - site zoom is still applied for the failed URL's site;
  - security per §0.3.
- Self-test `view_source` checks an error page for a dropped connection under `view-source:` (`tab.shows_error_page()`).

### 7.3 CEF notes

- **(NEEDS-CEF)**
  - Failure: `CefLoadHandler::OnLoadError(browser, frame, errorCode, errorText, failedUrl)` with `frame->IsMain()`.
  - Certificate errors: `CefRequestHandler::OnCertificateError(browser, cert_error, request_url, ssl_info, callback)`. Return false to cancel, or keep the `CefSSLInfo` (`GetCertStatus`, `GetX509Certificate`) for the bullet list.
  - Crash: `CefRequestHandler::OnRenderProcessTerminated(browser, status, error_code, error_string)`. The `TS_*` statuses include crash, kill and OOM [V signature]. Several tabs may share a renderer process in Chromium, so one crash can hit several tabs [I].
- **Mapping** Chromium errors to the bullets [I]:
  - `ERR_CERT_AUTHORITY_INVALID` → UNKNOWN_CA
  - `ERR_CERT_COMMON_NAME_INVALID` → BAD_IDENTITY
  - `ERR_CERT_DATE_INVALID` → EXPIRED or NOT_ACTIVATED (tell them apart by the certificate's validity dates)
  - `ERR_CERT_REVOKED` → REVOKED
  - `ERR_CERT_WEAK_SIGNATURE_ALGORITHM` / `ERR_CERT_WEAK_KEY` → INSECURE
  - others → GENERIC
- **Mechanism to show HTML under the failed URL:** CEF has no `load_alternate_html`. Options [I]:
  1. Serve the page for the failed URL through `CefResourceRequestHandler::GetResourceHandler` on a re-navigation;
  2. use `data:` or a custom-scheme URL while the shell keeps showing the failed URL;
  3. Chromium's own error page (`chrome-error://chromewebdata/`).
  The behavior to keep: the address bar shows the failed URL, reload and Try Again retry it, no history entry for the error itself, and the HTTPS-only warning's "Continue" link works.
- **(WEBKIT-ONLY)** The `error_page_pending` re-entrancy guard and the benign-error list. In CEF, `ERR_ABORTED` covers cancellations and downloads [I].

---

## 8. Pop-ups, window.open, target=_blank, window.close (tab.rs:717-739, 1119-1126; window/mod.rs:1252-1285)

### 8.1 Behavior [V] (KEEP)

- `create` → (gate check, §5) → `create_related()`: `Tab::new_related(browser, opener)` (same Browsing, same session, related view, `Gate::opened_by`, both tabs `related`), then `window.adopt_popup(opener, popup)`.
- The window keeps the popup aside until WebKit's `ready-to-show`. It is then inserted **as a tab next to its opener, selected** (`insert_tab(popup, opener, Foreground)`), with `runtime.tab_opened_by` and the opener's group. Window features (size, position, `popup=yes`) are ignored: always a tab.
- A popup closed before it shows (`close` before `ready-to-show`) is discarded from the extension runtime (`Browser::tab_discarded(popup, false)`). A popup still pending when its window closes is discarded too (test window/mod.rs:1692-1714).
- **Any** tab's `close` signal (page `window.close()`) → `window.close_tab(tab)`, an ordinary close (it goes on the closed-tab stack §23, and selection moves on).
- When closed, the opened tab and the opener both release their views (test tab.rs:1660-1688).
- Pop-up blocking: `BLOCK_POPUPS` (default on) stops windows a page opens without a user gesture (§1).
- `related` keeps both tabs awake under Memory Saver (§12).

### 8.2 CEF notes

- **(NEEDS-CEF)** `CefLifeSpanHandler::OnBeforePopup(browser, frame, popup_id, target_url, target_frame_name, target_disposition, user_gesture, popupFeatures, windowInfo, client, settings, extra_info, no_javascript_access)`: block by returning true, or host the new browser in a new tab [V signature].
  - Keep `no_javascript_access = false` so `window.opener` works.
  - Also needed: `OnBeforePopupAborted` (the analog of "closed before shown"), `OnAfterCreated`, `DoClose` / `OnBeforeClose` for `window.close()` [V names].
- `CefBrowserSettings.javascript_close_windows` controls closing windows not opened by script. Scripts may always close windows they opened or that have no back/forward history [V cef_types.h:627-633].
- The deferred-insertion (`ready-to-show`) bookkeeping is (WEBKIT-ONLY).

---

## 9. Link hover preview (tab.rs:794-805) [V]

- **(KEEP)** Hovering a link shows its address, `display_uri(link)`, in the bottom-left label. It hides when the pointer leaves the link. Middle ellipsis, max 90 chars, styling in §2.1.
- **(NEEDS-CEF)** `CefDisplayHandler::OnStatusMessage(browser, value)` gives the hovered link URL, or empty [V name] [I that it carries the link URL]. Chrome style has its own status bubble (`chrome_status_bubble` setting [V cef_types.h:717]). Format the text with `vsesvit_core::address::readable_url`.

---

## 10. Full screen (tab.rs:767-793; window/mod.rs:609-667; window/actions.rs:137-145)

### 10.1 Behavior [V] (KEEP)

- **Page-initiated** (`element.requestFullscreen()`):
  - WebKit `enter-fullscreen` makes the window show a notice; WebKit then fullscreens the window.
  - Notice text, two lines: **"{site} is now full screen\nPress Esc to exit full screen"**. `{site}` = `Origin::parse(committed).host_for_display()` (host plus a non-default port), else "This page".
  - The notice is a `gtk::Revealer` with crossfade at the top center of the page area, margin-top 24, not clickable, label CSS `osd fullscreen-notice` (`padding: 12px 20px; border-radius: 12px`). It hides after **4 s** (`FULLSCREEN_NOTICE_TIME`); a new notice restarts the timer.
  - `leave-fullscreen` hides the notice. Esc exits (WebKit handles it).
  - Test `a_page_gone_full_screen_is_named_with_the_way_out` (window/mod.rs:1658-1690) checks that the notice contains the host and "Esc", and that it goes when full screen ends.
- **User full screen:** `win.fullscreen` (F11; main menu "_Fullscreen") toggles window full screen. No notice.
- **Either way**, when the window becomes full screen, all toolbar top bars hide (`ToolbarView::set_reveal_top_bars(false)`): header bar, update banner, tab bar, bookmarks bar, find bar. The tab sidebar is content, not a top bar, so it stays as it was. Leaving full screen hides any notice, and permission prompts are held back while full screen (`sync_permission_prompt`).

### 10.2 CEF notes

- **(NEEDS-CEF)** `CefDisplayHandler::OnFullscreenModeChange(browser, fullscreen)`. Per the CEF docs, for non-Views Alloy-style browsers the client is responsible for the fullscreen transition [V header text]. Exit with `CefBrowserHost::ExitFullscreen(will_cause_resize)`; query with `IsFullscreen()` [V].
- Who handles Esc in Alloy style (Chromium's renderer, or the client via `CefKeyboardHandler::OnPreKeyEvent`) needs checking [I].
- The fullscreen page must cover the whole window (sidebar included). Whether it does today with WebKit is unverified [I].

---

## 11. Media: audio, mute, capture, notifications, permissions

### 11.1 Audio and mute [V] (KEEP)

- Tab indicator priority (window/mod.rs:1505-1519):
  1. capture icon (not clickable);
  2. else muted: `audio-volume-muted-symbolic`, tooltip "Unmute Tab", clickable;
  3. else playing: `audio-volume-high-symbolic`, "Mute Tab", clickable;
  4. else, on the top tab bar, the tab's group colour dot.
- Clicking the speaker toggles mute (`TabView::indicator-activated` → `set_is_muted(!muted)`). The tab menu has Mute Tab / Unmute Tab (tab_menu.rs).
- Self-test `tab_menu` checks the muted icon and tooltip, and that clicking unmutes.
- Audible tabs stay awake (§12).
- **(NEEDS-CEF)** Mute: `CefBrowserHost::SetAudioMuted` / `IsAudioMuted` [V].
- **Risk:** "is playing audio" has no CEF notification. The only audio hook, `CefAudioHandler` (`OnAudioStreamStarted/Stopped`), is a *capture* interface: `GetAudioParameters` returning true means "proceed with audio stream capture" [V header text], and while it captures, sound is delivered to the client instead of the speakers [I]. Options: page-side detection of media and WebAudio activity, or a CEF patch [I].

### 11.2 Capture (camera, microphone, screen) [V] (KEEP; details in the permissions inventory)

- `permissions::capturing(view)` reads WebKit capture states (Active or Muted count as capturing). `TabChange::Capture` →
  - tab indicator: `screen-shared-symbolic`, else `camera-web-symbolic`, else `audio-input-microphone-symbolic`, with tooltip `Capturing::description()`, e.g. "Using your camera and microphone", "Sharing your screen";
  - address-bar in-use button;
  - the sharing banner (§2.1) for the screen.
- Capturing tabs stay awake.
- **(NEEDS-CEF)** `CefDisplayHandler::OnMediaAccessChange(browser, has_video_access, has_audio_access)` [V]. It does not separate camera from screen [I]. Stopping a single capture ("Stop sharing", "Stop" in site info) has no obvious CEF API [I]; this is a risk.

### 11.3 Notifications and permission prompts (tab.rs:740-766) [V]

- `permission-request` → `permissions::handle`; `query-permission-state` → `permissions::query`. Both belong to the permissions inventory.
- `show-notification`: shown only if `permissions::notification_allowed(tab)` (site setting Allow); otherwise closed.
- **(NEEDS-CEF)** `CefPermissionHandler::OnRequestMediaAccessPermission`, `OnShowPermissionPrompt`, `OnDismissPermissionPrompt` [V names]. How web notifications display under Alloy style needs checking [I].

---

## 12. Memory Saver: sleeping and waking (tab.rs:98-112, 536-614; browser.rs:257-287, 722-756, 1604-1616; vsesvit_core::memory_saver)

### 12.1 Product behavior [V] (KEEP)

- Prefs: `keys::MEMORY_SAVER` (`performance.memory_saver`, LOCAL, default **on**) and `keys::MEMORY_SAVER_MODE` (`performance.memory_saver.mode`, LOCAL, default Balanced).
- Delays: Moderate 6 h, Balanced 4 h, Maximum 2 h (`MemorySaverMode::delay`). Labels and descriptions come from core; for example Balanced is "Tabs become inactive after 4 hours in the background".
- **Sweep** every `memory_saver::SWEEP_EVERY` (60 s), a GLib timeout started in `Browser::start`.
  - `Browser::sleep_idle_tabs(now)`: `Sweep::new(&mut profile, now)` reads both prefs and the origins allowed to notify.
  - For every tab with a committed URI it builds `TabActivity { url, shown (selected in its window), pinned, audible (is_playing_audio), capturing (any), related }`.
  - `Tab::sleeps(sweep, activity)` → `Sweep::sleeps(activity, &mut IdleClock)`. Anything that keeps a tab awake restarts its clock:
    - shown, pinned, audible, capturing, related;
    - a site allowed to notify;
    - a non-web page (not http/https: a new tab, an extension page, `file:`);
    - an unparsable URL.
  - The clocks keep running while Memory Saver is off.
- **Unsaved input check** (`sleep_unless_unsaved`): run `memory_saver::UNSAVED_INPUT_SCRIPT` in the page (main frame; it compares input, textarea, checkbox, radio, select and file fields with their defaults).
  - If `tab.has_edits()` or `memory_saver::has_unsaved_input(result)` → `keep_awake(now)`.
  - Otherwise, once the answer arrives, judge again: same committed URI, and `sweeps()` again with a fresh `Sweep` at the original `now`. Only then `tab.sleep()` and a session save.
  - Test `a_tab_pinned_while_its_page_answers_memory_saver_stays_awake`.
- **Asleep, as the user sees it:**
  - the tab keeps its title (saved, since WebKit forgets it);
  - its icon shows at **half opacity** (`favicons::faded`: premultiplied BGRA channels halved);
  - its address and history stay for the session, tab search and sync;
  - title and icon updates are suppressed.
- **Wake:** selecting the tab (`selection_changed` → `tab.wake()` → `reload()`) loads the page again at the same place in its history. The load starting ends the sleep and restores the icon.

### 12.2 Linux mechanism [V]

- `Tab::sleep()`: saves `display_title()` and the faded favicon, sets `Rest::Asleep`, then `web_view.terminate_web_process()`.
  - The `web-process-terminated` handler ignores `TerminatedByApi`.
  - After sleep, `web_view.uri()` is empty, but `session_uri()` and `session_state_bytes()` are unchanged and `is_blank()` is false (test tab.rs:1570-1596).
- **Edit tracking** (`TRACK_EDITS`, tab.rs:36-43, 549-575): a user script in the isolated world `vsesvit-edits`, all frames, at document start, adds `input` and `change` listeners (capture, once) that post to the `vsesvitEdited` message handler, which sets `edited = true`. It resets on each main-frame commit.
  - A file set on an `<input type=file>` by script fires no event, so it is not an edit; the unsaved-input script still catches it.
  - Test `memory_saver_keeps_a_page_edited_in_any_frame_awake` (contenteditable `input`, an iframe `change`, a file set by script; a reload resets edits).

### 12.3 Classification

| Piece | Class | CEF note |
|---|---|---|
| Sweep policy, delays, keep-awake rules, unsaved-input check, re-judge after the async answer | (KEEP) | core decides; run the script with `CefFrame::ExecuteJavaScript` plus a reply channel (process message), or DevTools `Runtime.evaluate` via `CefBrowserHost::ExecuteDevToolsMethod` [I] |
| Asleep tab keeps title, faded icon, address, history; wakes on selection to the same history position | (KEEP) | |
| Sleeping by terminating the web process | (WEBKIT-ONLY) | **Risk:** CEF exposes no tab discard. Options [I]: DevTools `Page.setWebLifecycleState {state: "frozen"}` (experimental [V Page.pdl]; freezes the page but keeps its memory); `CefBrowserHost::WasHidden(true)` (throttling only); or close the browser and recreate it on wake, which loses back/forward history (§13). |
| Edit tracking in all frames | (WEBKIT-ONLY) if sleeping keeps the page (as Windows' `TrySuspendAsync` does); (KEEP) if sleeping reloads | core docs: "Where sleeping reloads the page (Linux), a page edited in any frame stays awake too." |
| `audible` input | (NEEDS-CEF) | see §11.1 risk |

Self-test `memory_saver` (self_test.rs:1262-1378):
- three background tabs: A on `/icon.html` with history, B with typed input, C pinned;
- a sweep at +7 h puts only A to sleep, and the check expects `TerminatedByApi` (a Linux-specific assertion that must change for CEF);
- A's view releases the page; the tab title stays "Icon"; the icon alpha is halved (`tab-asleep.png`); the session snapshot keeps the URL, title and restore state;
- selecting A wakes it on icon.html with Back available and its own icon;
- with Memory Saver off nothing sleeps;
- Settings > General shows the switch and "Memory savings" (Balanced), insensitive while the switch is off.

---

## 13. Back/forward history persistence (tab.rs:497-530, 1129-1140; session.rs; browser.rs ClosedTab)

### 13.1 Behavior [V] (KEEP)

- `session_state_bytes()` = WebKit `session_state().serialize()`. It is saved locally in core's `tab_restore_state` table, through `TabSnapshot::restore_state`, which is never synced and never part of record equality (vsesvit-core/src/session.rs:7-12, 71-74, 134-160).
- `restore_saved(bytes, uri)` decodes with a NULL-safe FFI wrapper (`decode_session_state`). Unreadable bytes (another WebKit version, a damaged row) → log "this WebKit cannot read the saved history of {uri}; loading the page alone", then load the URI (test tab.rs:1229-1240).
- `restore(state, uri)`: `restore_session_state`, then `go_to_back_forward_list_item(current)` with a gate browser load. Without state it loads the URI.
- Used for:
  - session restore at startup (`session::restore`);
  - **Reopen Closed Tab** and tab search's closed rows (the in-memory `WebViewSessionState` in `ClosedTab`);
  - Memory Saver wake (implicitly: WebKit keeps the list across the process restart).
  - PLAN says: "A reopened tab keeps its back/forward history, as Ctrl+Shift+T does."
- Tabs moved between windows keep everything: the WebKit view is reparented by `AdwTabView::transfer_page`.

### 13.2 CEF notes

- **(NEEDS-CEF), major risk:** CEF has **no API to serialize or restore a browser's navigation history**.
  - `CefBrowserHost::GetNavigationEntries(visitor, current_only)` reads entries (URL, title, transition, …), but nothing restores them [V: `cef_browser.h` lists `GetNavigationEntries`; the only `restore` member in the bindings is `_cef_window_t::restore`, Views window restore].
  - So session restore, reopen-closed-tab, and any Memory Saver mode that recreates the browser lose back/forward history unless a CEF patch or another workaround is found [I].
  - `restore_state` bytes in existing profiles are WebKit format and unreadable by CEF; the core fallback (load the URL alone) already covers this.
- Moving a tab to another window: in windowed mode the CEF child X11 window would be reparented [I] (other inventory).

---

## 14. Engine defaults the tab inherits (no shell code) [V absence by grep] [I behavior]

The GTK shell does **not** handle any of the following WebKit signals, so WebKitGTK's built-in UI applies [V: no `script_dialog`, `run_file_chooser`, `authenticate`, `print` or `try_close` handlers in vsesvit-gtk/src]:

| User-visible behavior | WebKitGTK default [I] | Class | CEF handler |
|---|---|---|---|
| `alert` / `confirm` / `prompt` | dialog drawn inside the web view | (KEEP) / (NEEDS-CEF) | `CefJSDialogHandler::OnJSDialog`, `OnResetDialogState`, `OnDialogClosed` [V]; CEF's default under Alloy style is unverified [I] |
| `beforeunload` "leave page?" | on navigating away; closing a tab destroys the view without asking (no `try_close`) [I] | (KEEP), decide on close | `CefJSDialogHandler::OnBeforeUnloadDialog` [V]; `CefBrowserHost::TryCloseBrowser` / `CloseBrowser(force)` on tab close [V names] |
| `<input type=file>` | GTK file chooser / portal | (NEEDS-CEF) | `CefDialogHandler::OnFileDialog`, or CEF's default Linux dialog [I] |
| HTTP auth | built-in auth dialog | (NEEDS-CEF) | `CefRequestHandler::GetAuthCredentials` [I name] |
| `window.print()` | GTK print dialog | (NEEDS-CEF) | `CefPrintHandler` (§15) |
| `<select>`, colour and date pickers | engine popups | (NEEDS-CEF) in OSR mode | `CefRenderHandler::OnPopupShow` / `OnPopupSize` / `OnPaint(PET_POPUP)` [I]; native in windowed mode |
| Web process unresponsive | none handled | open | `CefRequestHandler::OnRenderProcessUnresponsive` / `OnRenderProcessResponsive` [V] |

---

## 15. Print (tab.rs:376-379; page_menu.rs:211-233; window/menu.rs)

### 15.1 Behavior [V] (KEEP)

- Entry points:
  - `win.print`: Ctrl+P (`Command::Print`, reassignable);
  - main menu "_Print…";
  - page context menu "_Print…" (on a plain page hit, before Inspect Element);
  - `window.print()` from the page (WebKit default) [I].
- Action: `webkit::PrintOperation::new(view).run_dialog(window)`, the GTK print dialog, modal over the window.
- Self-test `page_commands` checks the accelerators and prints without a dialog: a `PrintOperation` with `gtk::PrintSettings` for printer "Print to File", format pdf, output URI `<out>/print.pdf`. The file must start with `%PDF`.

### 15.2 CEF notes

- **(NEEDS-CEF)** `CefBrowserHost::Print()` [V].
- On Linux a client `CefPrintHandler` is required: "Implement this interface to handle printing on Linux" [V cef_print_handler.h:78]. Methods: `OnPrintStart`, `OnPrintSettings`, `OnPrintDialog(browser, has_selection, callback)` (show our dialog, then `callback.Continue(settings)` or `Cancel()`), `OnPrintJob(browser, document_name, pdf_file_path, callback)` (send the PDF to the printer), `OnPrintReset`, `GetPdfPaperSize` [V].
- **Risk:** without GTK, the print dialog and the printer back end (CUPS) must be provided by the shell [I]. Chrome style instead uses Chromium's print preview [I].
- Self-test replacement: `CefBrowserHost::PrintToPDF(path, settings, callback)` → `CefPdfPrintCallback::OnPdfPrintFinished(path, ok)` [V], or DevTools `Page.printToPDF` [I].

---

## 16. Developer tools (tab.rs:393-412, 1142-1150; window/actions.rs:131-133)

### 16.1 Behavior [V] (KEEP)

- `win.developer-tools` (Ctrl+Shift+I and F12; main menu More Tools > "_Developer Tools") **toggles** the inspector: it closes it when open, otherwise shows it. WebKit decides whether it opens attached or in its own window.
- `win.javascript-console` (Ctrl+Shift+J) just shows the inspector, because WebKit cannot pick a panel.
- WebKit's own "Inspect Element" in the context menu inspects the element (developer extras on).
- `is_open` uses a raw FFI pointer check, because the binding's `WebInspector::web_view` would sink a floating view and close the inspector. **(WEBKIT-ONLY)**
- Self-test (cfg `self-test`) uses `Tab::inspector_open()`: the first activation opens the inspector (attached or in a window, via `attach` / `open-window` signals), and the second closes it.

### 16.2 CEF notes

- **(NEEDS-CEF)** `CefBrowserHost::ShowDevTools(windowInfo, client, settings, inspect_element_at)`, `CloseDevTools()`, `HasDevTools()` [V].
- Inspect Element: `ShowDevTools` with `inspect_element_at` = the context-menu point [V].
- The console panel: CEF has no panel selector [I]. Options: Chrome style's DevTools toggles, or script in the DevTools front end [I]. Windows also cannot pick the console (PLAN, "Page commands").
- `CefLifeSpanHandler::OnBeforeDevToolsPopup` lets the shell host the DevTools window [V name].

---

## 17. Context menu (page_menu.rs; extension_menus.rs)

### 17.1 What the user sees [V] (KEEP)

The shell keeps WebKit's default menu and modifies it. The defaults vary by context: link items, image items, media items, Copy/Cut/Paste/Select All, spelling guesses, Back/Forward/Stop/Reload, Inspect Element [I list, from WebKitGTK's `WebKitContextMenuAction`].

Note: WebKit's **"Open Link in New Window"** goes through `create`, which this shell always turns into a *tab* next to the opener (§8) [I that WebKit routes it through `create`].

Additions and changes:

| Hit | Change | Label | Position | Action |
|---|---|---|---|---|
| Misspelled word | rename WebKit's Learn Spelling | "_Add to Dictionary" | in place | WebKit adds the word to the system's personal dictionary (Enchant) |
| Link (the page may navigate there: `runtime.may_navigate(committed, link)`) | add | "Open Link in _Private Window" | right after Open Link in New Window, else last | `browser.open_window(Browsing::Private, [url])` |
| Link | add | "Copy Link _Without Tracking" | right after Copy Link Address, else last | clipboard ← `vsesvit_core::clean_url::clean(link)` (no toast) |
| Selection (non-blank) | add | `SelectionAction.label` from `profile.omnibox().for_selection(text)`: `Search {Engine} for “{text}”`, or `Go to {address}` when the selection is one http(s) address | right after Copy, else last | open `SelectionAction.url` in a **new foreground tab next to the page** |
| Plain page (not link, image, media, editable, scrollbar or selection) | add | "_Print…", "View Page _Source" (disabled when `source_url()` is None) | right before Inspect Element, else last | `tab.print()`, `tab.view_source()` |
| Any hit but a scrollbar | add extension items (`chrome.contextMenus`), only of extensions that `runtime.runs_in(ext, browsing)` | separator, then each extension's item or submenu; titles with `_` escaped as `__` | above the separator over Inspect Element (if there is one), else before Inspect Element, else last | `runtime.menu_clicked(ext, item, Some(tab id), Some(target))`, which fires `contextMenus.onClicked` and grants `activeTab` |

Extension entries (`vsesvit_webext::menus::Entry`) render as follows (extension_menus.rs):
- `Item{title, enabled, checked}`: checkable when `checked` is Some;
- `Submenu{enabled:false}`: a disabled item;
- `Submenu{enabled:true, children}`: a nested menu;
- `Separator`.

The click target passed to extensions (`menus::Target`) carries:
- `page_url` (committed);
- `frame_url` (only for a click in a subframe);
- `link_url`;
- `src_url` (image, video or audio source);
- `media` (Image, Video or Audio, the last two told apart by the clicked element's tag);
- `selection` (only for a selection hit);
- `editable`.

### 17.2 Linux mechanism [V]

A user script in isolated world `vsesvit-shell`, all frames, at document start:
- posts `String(getSelection())` on every `selectionchange` (`vsesvitSelection`, stored in `Tab::selection`);
- on `contextmenu` (capture) posts `[location.href, window === top, closest video|audio tag]` (`vsesvitContextMenu`).

A report left over from a menu the page cancelled is discarded (`take()`).

### 17.3 Classification and CEF notes

- Selection and frame tracking scripts: **(WEBKIT-ONLY)**. `CefContextMenuParams` gives `GetSelectionText`, `GetFrameUrl`, `GetPageUrl`, `GetLinkUrl`, `GetUnfilteredLinkUrl`, `GetSourceUrl`, `GetMediaType` (image, video, audio, …), `GetTypeFlags`, `IsEditable`, `GetMisspelledWord`, `GetDictionarySuggestions`, `IsSpellCheckEnabled`, `GetEditStateFlags`, `GetMediaStateFlags` [V].
- Menu modification: **(NEEDS-CEF)**
  - `CefContextMenuHandler::OnBeforeContextMenu(browser, frame, params, model)` to edit the `CefMenuModel`;
  - `RunContextMenu(...)` to draw the menu in GPUI and run the callback with the chosen command id;
  - `OnContextMenuCommand`, `OnContextMenuDismissed` [V].
  - Custom ids must lie in `MENU_ID_USER_FIRST..MENU_ID_USER_LAST` (26500..28500) [V].
  - Default ids include `MENU_ID_BACK`, `FORWARD`, `RELOAD`, `RELOAD_NOCACHE`, `STOPLOAD`, `UNDO`, `REDO`, `CUT`, `COPY`, `PASTE`, `PASTE_MATCH_STYLE`, `DELETE`, `SELECT_ALL`, `FIND`, `PRINT`, `VIEW_SOURCE`, `SPELLCHECK_SUGGESTION_0..4`, `NO_SPELLING_SUGGESTIONS`, `ADD_TO_DICTIONARY` [V cef_types.h:2018-2057].
- **Gap:** CEF's default (Alloy) menu has **no link, image or media items** (Open link in new tab or window, Copy link address, Save image, Copy image, …) [V: no such MENU_ID]. Reproducing today's menu means adding them. Chrome style shows Chromium's full menu, which includes extensions' items [I].
- Add to Dictionary in Chromium writes Chromium's custom dictionary in the profile, not the system dictionary [I]. That diverges from PLAN ("Vsesvit keeps no dictionary of its own").
- Extension items: with Chromium's extension system, where `contextMenus` items come from needs research. If CEF offers no way to list an extension's items, Alloy style cannot show them [I] (risk).

Self-test:
- `context_menus`: on a page hit with [Reload, separator, Inspect Element], expects exactly `["Reload","-","Vsesvit Probe page item","-","InspectElement"]`; a link gets no probe items; `onClicked` gets `pageUrl` and the tab id.
- `selection_search`: the label `Search Fixture Search for “Vsesvit fixture page”` at index 1 after Copy; it opens `<origin>/search?q=Vsesvit+fixture+page` in a new selected tab next to the page.
- `tab_menu`: Copy Link Without Tracking at index 1 after Copy Link Address copies the clean URL.
- `private_window`: Open Link in Private Window right after Open Link in New Window opens a private window titled "Vsesvit fixture 2 (Private)".
- `page_commands`: `["Reload","_Print…","View Page _Source","InspectElement"]`.
- `spellcheck`: Learn Spelling reads "_Add to Dictionary".

---

## 18. Zoom (zoom.rs; window/actions.rs:205-211; browser.rs:858-889; address_bar.rs:615-652; window/menu.rs:218-248)

### 18.1 What the user sees [V] (KEEP)

- Commands: `win.zoom-in` (Ctrl+Plus, Ctrl+Equal, Ctrl+KeypadPlus), `win.zoom-out` (Ctrl+Minus, Ctrl+KeypadMinus), `win.zoom-reset` (Ctrl+0, Ctrl+Keypad0). All reassignable.
- Steps follow Chrome's presets `0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0`, two decimals so that a level read back from whole percents is still a preset. An off-preset level snaps to the neighbouring preset; the steps stop at the ends (epsilon 0.001). Reset goes to `vsesvit_core::zoom::DEFAULT` = 1.0.
- `percent(level)` = `"{:.0}%"`.
- **Main menu zoom row:** a custom row with a flat circular `zoom-out-symbolic` "Zoom Out" button, a flat numeric expanding button showing "NNN%" (action `win.zoom-reset`, tooltip "Reset Zoom"), and `zoom-in-symbolic` "Zoom In".
- **Address-bar zoom pill:** a flat `.address-zoom` button with the percent, before the star, **visible only when the level is not 100%**, tooltip "Zoom". Clicking it opens the **zoom bubble** (popover anchored at the pill): "Zoom:", the level (numeric heading, 5 chars), linked Zoom Out / Zoom In buttons, and a "Reset" button (tooltip "Reset to 100%"). The bubble's level updates live.
- **Per-site memory:** zoom is remembered **per host** (any scheme or port, http/https only) across pages and restarts.
  - A new document (not same-document; error pages included) is shown at its site's level, or 100% for a site with none.
  - Setting 100% forgets the site.
  - Private windows keep their zoom in memory for the private session only.
- Self-tests:
  - `zoom_indicator`: hidden at 100%; "110%" after zoom-in; bubble opens (`zoom.png`); hidden after reset.
  - `zoom_is_remembered_per_site`: two zoom-ins give 125% on index.html; page2.html keeps 125%; `localhost` (another host) shows 100%; index.html shows 125% again; core stores 1.25; reset shows 100% and core forgets it.
  - `private_window` checks private zoom isolation (also browser.rs:1709-1737).

### 18.2 Core contract [V]

- `profile.site_zoom(browsing) -> vsesvit_core::zoom::SiteZoom`:
  - `.get(&url) -> f64` (DEFAULT for no row, non-http(s) pages, or no host; in private the session-kept value wins);
  - `.set(&url, level)`: stores whole percent in the LOCAL table `site_zoom(host TEXT PRIMARY KEY, percent INTEGER)`; the default deletes the row; private writes go to memory.
- Shell calls:
  - `Browser::navigation_committed` → `show_at_site_zoom(tab, url)` (sets the level only if it differs, outside the profile borrow);
  - `TabChange::Zoom` → `Browser::zoom_changed(tab)` → `site_zoom.set(committed url, view level)`.

### 18.3 CEF notes

- **(NEEDS-CEF)** `CefBrowserHost::GetZoomLevel`, `SetZoomLevel`, `CanZoom(command)`, `Zoom(command)` with `CEF_ZOOM_COMMAND_IN/OUT/RESET`, and `GetDefaultZoomLevel` [V].
  - CEF zoom *levels* are Chromium's logarithmic levels (factor = 1.2^level; 0.0 = 100%) [I], so convert to and from core's factor.
  - `Zoom(IN/OUT)` calls `zoom::PageZoom::Zoom`, i.e. Chromium's preset factors [V `libcef/browser/browser_host_base.cc:376-398`], which are the same list as `LEVELS` [I].
- **No zoom-changed callback** in CEF [V: no zoom callback in `include/*.h`]. Ctrl+wheel zoom is handled inside Chromium (`AlloyBrowserHostImpl::ContentsZoomChange` → `PageZoom::Zoom`) [V alloy_browser_host_impl.cc:947-950], so the shell is not told. Risk: the per-site store and the indicator need polling or another trigger [I].
- CEF uses `zoom::ZoomController` [V browser_host_base.cc:409-453]. In default mode Chromium's `HostZoomMap` already applies zoom **per host, to every tab of that host in the same request context, and remembers it** [I]. This overlaps core's store: it may make `show_at_site_zoom` mostly redundant, and it changes behavior (zooming one tab re-zooms other open tabs of the same host). Windows notes WebView2 "remembers the level per site while it runs" (vsesvit-winui/src/zoom.rs:1-5) [V].
- The keyboard shortcuts are the shell's (core keymap). Chromium's own Ctrl+/- handling is not active in Alloy style; Ctrl+wheel is [I].
- `chrome_zoom_bubble` (`cef_types.h:723`) is Chrome style's own zoom bubble [V].

---

## 19. Find in page (find_bar.rs; window/actions.rs:116-128; window/mod.rs:358, 397, 1455, 1476)

### 19.1 What the user sees [V] (KEEP)

- **One find bar per window**, a `gtk::SearchBar` that is the last toolbar top bar (under the bookmarks bar, above the page), with a close button. Contents, left to right:
  - search entry: placeholder **"Find in page"**, 28 chars wide;
  - linked buttons `go-up-symbolic` **"Previous Match"** and `go-down-symbolic` **"Next Match"**;
  - status label (dim, numeric).
- **Open:** `win.find` (Ctrl+F; main menu "_Find…") targets the selected tab's view, reveals the bar, focuses the entry, **selects all its text** (the previous query is kept), and searches at once.
- **Search:** each text change (GTK search-changed, debounced) runs a case-insensitive, wrap-around search, up to **1000** matches, and counts matches.
  - Empty text: clear highlights, empty status, no error style.
- **Next / Previous** (only when the bar is open and the text is not empty):
  - Next: Enter in the entry, the Next button, the entry's next-match key (Ctrl+G), `win.find-next` (Ctrl+G);
  - Previous: Previous button, previous-match key (Ctrl+Shift+G), `win.find-previous` (Ctrl+Shift+G).
- **Status wording:** "1 match", "{n} matches" (including "0 matches"), "More than 1000 matches", "No matches".
  - Not found: the entry gets the CSS `error` class (red) and the status reads "No matches".
  - The status shows only for the selected tab's results (`TabChange::Find` when selected).
- **Close:** the close button, or Esc in the entry (`SearchBar.connect_entry`). It ends the search (clears highlights), clears the status and error style, and **returns keyboard focus to the page**.
- **Tab switch while open:** finish the search in the old tab, retarget, and search again in the new tab with the same text.

### 19.2 Classification and CEF notes

- The find UI and its behavior: (KEEP).
- **(NEEDS-CEF)**
  - Search: `CefBrowserHost::Find(searchText, forward, matchCase=false, findNext)`. A text change restarts the search; empty text stops it [V header text].
  - Close: `StopFinding(clearSelection=true)` [V].
  - Results: `CefFindHandler::OnFindResult(browser, identifier, count, selectionRect, activeMatchOrdinal, finalUpdate)` [V].
- **(WEBKIT-ONLY)** The 1000-match cap and "More than 1000 matches": WebKit reports `u32::MAX` past `MAX_MATCHES`. Chromium has no cap [I] and also reports the active ordinal, so a Chrome-like "3/12" is possible (product choice).
- Chromium wraps around by default [I].

---

## 20. View page source (view_source.rs; vsesvit_core::view_source; tab.rs:381-391)

### 20.1 What the user sees [V] (KEEP)

- `win.view-source` (Ctrl+U; main menu More Tools > "View Page _Source"; page context menu "View Page _Source") opens `view-source:<page>` in a **new selected tab right after the page**, as opener (it joins the page's group).
  - Only for http, https and file pages: `vsesvit_core::view_source::source_url(committed)`.
  - The action, and the menu item, are disabled otherwise (`sync_location` sets enablement on every location sync).
- Typing `view-source:<url>` in the address bar, or passing it on the command line (`classify_url` accepts it, location.rs test), also works. `viewed_url` is case-insensitive about the prefix and rejects nested `view-source:` and non-http/https/file pages.
- The source page (`vsesvit_core::view_source::source_page(url, text)`):
  - title **`view-source:<url>`**;
  - a monospace table of numbered lines (CRLF and CR normalized);
  - line numbers right-aligned in GrayText with a separator border, **not selectable**, so a copy is the source alone;
  - `color-scheme: light dark`; CSP `default-src 'none'; style-src 'unsafe-inline'`; everything escaped.
- **Where the bytes come from:**
  1. from another tab (not the asking one) **of the same Browsing kind** that has finished loading exactly that page: its main resource, with no new fetch;
  2. else a **hidden view** loads it:
     - scripts and images off; tracking protection and cookie rules on; the tab kind's network session;
     - **over https when HTTPS-only would upgrade it**, failing (never falling back to http) with "{url} has no secure connection";
     - a redirect back to an http URL that would be upgraded is refused the same way;
     - every response is shown, never downloaded, so a binary file's source shows as text.
- Failure (no such page, network error, dropped connection) → the generic error page under the `view-source:` URL.
- **Pages cannot open or embed it:** the scheme is registered as *local*, which keeps web pages out, and the gate refuses `view-source:` to every load the browser did not start, which keeps `file://` pages out.

Tests and self-test:
- view_source.rs:386-481:
  - a web lure and a local lure that embed and navigate to view-source fetch nothing;
  - a typed view-source fetches the page once;
  - a file's source is shown, not downloaded;
  - a dropped connection gives an error page;
  - with HTTPS-only (reach Everywhere) there is no http fetch, and with a site exception there is one.
- `page_commands`:
  - from the open tab, 0 fetches, and the text contains `<title>Vsesvit fixture</title>`;
  - a fresh tab, 1 fetch through the hidden view;
  - the local `file://` lure stays and the server is never asked.

### 20.2 Classification and CEF notes

- **(WEBKIT-ONLY)** Serving the scheme yourself, the hidden-view loader, reuse of another tab's main resource, and core's `source_page` renderer. PLAN: "WebKitGTK has no view-source, so the shell serves the `view-source` scheme itself." Chromium renders `view-source:` natively: highlighted, with line numbers and a line-wrap toggle [I].
- **(KEEP)** The `view-source:` tab next to the page, the http/https/file restriction and action enablement, pages unable to reach it, and the HTTPS-only interplay. The last needs checking: how Chromium's view-source navigation looks in `OnBeforeBrowse` (inner URL or `view-source:` URL) decides whether HTTPS-only can judge it [I].
- Note from Windows (Chromium too): "WebView2 renders `view-source:` pages natively but reports the address of the page inside, so the tab keeps the `view-source:` address the shell loaded" (PLAN, Page commands) [V]. Check what `OnAddressChange` reports in CEF [I].
- `CefFrame::ViewSource()` is **not** the product behavior: it "save[s] this frame's HTML source to a temporary file and open[s] it in the default text viewing application" [V cef_frame.h:116-122]. `CefFrame::GetSource(visitor)` exists [V] if a custom page were wanted.

---

## 21. Save Page As (save_page.rs; window/actions.rs:113-115)

### 21.1 What the user sees [V] (KEEP)

- `win.save-page` (Ctrl+Shift+S; main menu "_Save Page As…") opens a `gtk::FileDialog`:
  - title **"Save Page As"**, modal;
  - initial folder = the download directory (`keys::DOWNLOADS_DIR`, `downloads.directory`, LOCAL; else the platform Downloads folder);
  - initial name as below.
- The format is decided by the main resource's MIME type **when the dialog opens**:
  - `text/html`, `application/xhtml+xml`, or no response yet (a page still loading, the new-tab page) → **MHTML**. Name: `"{title}.mhtml"`, where the title has `/` and `\` replaced by `-`, is cut at 200 bytes on a char boundary, is trimmed, and is "page" when empty; then `vsesvit_core::downloads::sanitize`. Examples: "A/B testing" → "A-B testing.mhtml"; a blank title → "page.mhtml".
  - Anything else (image, PDF, `text/plain`, …) → **the file as served**. Name: the server's suggested filename, else the URL's last path segment unescaped, then sanitized.
- The URI is also captured when the dialog opens, so a page that moves on behind the dialog cannot change what lands under the offered name (test save_page.rs:138-161).
- Result:
  - MHTML: WebKit `save_to_file(Mhtml)`, then toast **`Saved “{file name}”`**;
  - File: `Downloads::download_to(view, uri, path)` starts a regular download into the chosen path (overwrite allowed, listed in Downloads), with no toast;
  - error: toast **`Cannot save the page: {error}`**.
- Self-test `save_page`: `save_page::save` (cfg `self-test`) writes `<out>/saved-page.mhtml` containing "Vsesvit fixture".

### 21.2 CEF notes

- **(NEEDS-CEF)**
  - MHTML: DevTools `Page.captureSnapshot {format: "mhtml"}` (experimental [V Page.pdl]) via `CefBrowserHost::ExecuteDevToolsMethod`, then write the string to the path [I that it is the way to go].
  - File: `CefBrowserHost::StartDownload(url)` [V] with `CefDownloadHandler::OnBeforeDownload` → `callback.Continue(chosen_path, show_dialog=false)` [V names].
  - MIME of the main resource: no direct CEF getter for the current document [I]. Options: record it from `CefResourceRequestHandler` responses or `OnLoadEnd`; read `document.contentType` via JavaScript or DevTools [I].
- The file dialog: GPUI or portal (no GTK). The `Saved “…”` toast: the shell's toast UI.

---

## 22. In-app screenshot (screenshot.rs; app.rs:380-410; self_test.rs)

### 22.1 Behavior [V] (KEEP for testing and tooling)

- `capture(widget)` replays the widget's last rendering through its window's own GSK renderer at the display scale. Nothing outside the app is read, and the window needs neither focus nor to be on top.
- Error kinds:
  - `NotRealized`: "the widget is not shown in a window";
  - `NothingDrawn`: "the widget has no current rendering";
  - `Save(e)`: "could not write the PNG: {e}".
- `capture_next_frame` captures right after the next paint. `capture_settled` retries up to 10 more frames while nothing is drawn. `save_png(widget, path)` writes the PNG.
- `save_png_with_popovers(window, popovers, path)` composites popovers (separate surfaces) at their compositor positions; the image grows to take in popovers that reach past the window.
- Users:
  - **Self-test:** `window.png` for the `screenshot` check (at least 2 distinct colours) and many evidence PNGs (`tab-asleep.png`, `zoom.png`, `https-only-warning.png`, `address-progress.png`, `tab-menu.png`, `connection-*.png`, `permission-prompt.png`, `capture-in-use*.png`, …).
  - **Debug builds only:** `app.debug-screenshot(path)` over D-Bus `org.gtk.Actions` captures the active window, with a 5 s timeout.

### 22.2 CEF notes

- **(NEEDS-CEF), risk.** The captures must include the web content.
  - In CEF windowed mode the page is a separate native child window that GPUI's renderer does not draw [I].
  - Options: OSR mode, where the page is a texture GPUI composites; or compose GPUI's frame with the page from DevTools `Page.captureScreenshot` [I].
- Popover compositing has a GPUI equivalent: GPUI popovers are in-window elements or separate windows [I].
- The D-Bus debug action is tied to GApplication; it needs a GPUI-era replacement for scripts [I].

---

## 23. Closed tabs (closed_tabs.rs; browser.rs:60-160, 665-836; window/mod.rs:1224-1238)

### 23.1 Behavior [V] (KEEP)

- `ClosedTabs<T>`: a bounded stack, capacity **25** (`CLOSED_TABS_KEPT`).
  - `push` forgets the oldest when full and returns a `ClosedKey(u64)` that is **never reused**, not even after `clear`.
  - `pop` returns the most recent. `take(key)` / `get(key)` find a tab by key; a key keeps naming its tab whatever else is closed or reopened. `iter` goes oldest first. `clear`.
- **Two stacks:** normal, and private. The private stack is cleared when the private session ends (browser.rs:358-366).
- **What is recorded** (`Browser::tab_closed`, on the tab view's `close-page`):
  - only tabs whose `committed_uri` is set and is not `about:blank`. Blank tabs and the new-tab page are not recorded;
  - fields `ClosedTab { uri, title (display_title), favicon (texture at close), state (WebKit back/forward), position, pinned, group, used (MRU tick) }`.
  - It also runs `permissions::closed(tab)` (waiting prompts denied), `runtime.tab_closed(id, false)` and a session save.
- **Not recorded:** tabs that go with their window (`before_close` → `tab_discarded`, so closing a window does not fill the stack), and popups dropped before showing.
- **Reopen** comes from:
  - `win.reopen-closed-tab` (Ctrl+Shift+T);
  - the tab menu "R_eopen Closed Tab", enabled when `can_reopen_closed_tab(browsing)`;
  - tab search's "Recently closed" rows (`go_to_tab(Hit::Closed(key))` → `take(key)`).
- `restore_closed` always reopens **into the current window**:
  - a new tab of the window's kind at the saved position, clamped: inside the pinned range and pinned if it was pinned, else after the pinned tabs;
  - selected;
  - back in its group if the window still has that group (core `tab_groups` rules);
  - then `tab.restore(state, uri)`, **keeping back/forward history**.
- Tab search lists closed tabs by core's `tab_search::rows(query, open, closed)`. Each row has the title, URL, `used` tick and the favicon kept at close (`Browser::tab_icon`).
- Self-test `tab_menu`: Close Other Tabs, then Reopen Closed Tab brings the duplicate back third on index.html, and the menu item is enabled when a tab was closed before. `private_window`: a tab closed there is kept in the private stack, and gone once the window closes.

### 23.2 CEF notes

- The stack and its rules are engine-independent: (KEEP). `closed_tabs.rs` could move unchanged.
- The `state` field: **(NEEDS-CEF), risk.** There is no CEF history restore (§13). A CEF-era `ClosedTab` can keep the navigation entries (`GetNavigationEntries`) for display, but cannot restore them [I].
- Favicon: keep the last downloaded icon (from `DownloadImage`).

---

## 24. Command-line targets and homepage (location.rs) [V] (KEEP; engine-independent)

- `resolve_cli_target(arg, cwd, is_file)`:
  1. if `cwd.join(arg)` is absolute and an existing file → a `file://` URL;
  2. else `vsesvit_core::search::classify_url(arg)` reads it like the address bar minus search: a URL with a scheme the engine can show stays as is; a host gets `https://`; `localhost:port` and IP:port get `http://`; `view-source:` passes;
  3. anything else → None, and app.rs prints **`vsesvit: cannot open {arg}`** to the invoking terminal (app.rs:271-278).
- Pinned examples (location.rs tests):
  - `page.html` (existing) → `file:///home/u/page.html`;
  - `missing.html` → `https://missing.html/`;
  - `[::1]:8080` → `http://[::1]:8080/`;
  - `./missing.html` → None;
  - `rust lifetimes` → None;
  - `mailto:` → None;
  - `javascript:` → None.
- `homepage_url` is a re-export of `vsesvit_core::prefs::homepage_url(pref)`: `""`, `about:home` and `about:blank` → None (meaning the new-tab page); otherwise `classify_url`.
- The resolved URLs go to `open_startup_windows`, `open_window` or `present`, after the content blockers are applied (`when_blockers_applied`). That is the app/CLI inventory.

---

## 25. Keyboard shortcuts touching this inventory (vsesvit_core::shortcuts; keymap.rs) [V]

All come from core's keymap, so they are user-reassignable. The GTK action names are listed for traceability.

| Command | Default chords | GTK action |
|---|---|---|
| ReopenClosedTab | Ctrl+Shift+T | `win.reopen-closed-tab` |
| Back / Forward | Alt+Left / Alt+Right | `win.back` / `win.forward` |
| Reload | Ctrl+R, F5 | `win.reload` |
| ReloadBypassCache | Ctrl+Shift+R, Shift+F5 | `win.reload-bypass-cache` |
| SavePage | Ctrl+Shift+S | `win.save-page` |
| Print | Ctrl+P | `win.print` |
| Find / FindNext / FindPrevious | Ctrl+F / Ctrl+G / Ctrl+Shift+G | `win.find` / `win.find-next` / `win.find-previous` |
| CopyCleanLink | Ctrl+Shift+C | `win.copy-clean-link` (toast "Link copied") |
| CopyLink | Ctrl+Alt+Shift+C | `win.copy-link` (toast "Link copied") |
| ViewSource | Ctrl+U | `win.view-source` |
| ZoomIn / ZoomOut / ZoomReset | Ctrl+Plus, Ctrl+Equal, Ctrl+KeypadPlus / Ctrl+Minus, Ctrl+KeypadMinus / Ctrl+0, Ctrl+Keypad0 | `win.zoom-*` |
| Fullscreen | F11 | `win.fullscreen` |
| DeveloperTools | Ctrl+Shift+I, F12 | `win.developer-tools` |
| JavaScriptConsole | Ctrl+Shift+J | `win.javascript-console` |

Not in the keymap:
- Stop (button only);
- Esc to leave page full screen (engine);
- Esc to close the find bar and Enter for next match (find entry);
- Ctrl+G / Ctrl+Shift+G inside the find entry (GTK SearchEntry built-ins, matching the keymap defaults).

(NEEDS-CEF) With a CEF page focused, key events reach the shell through `CefKeyboardHandler::OnPreKeyEvent` / `OnKeyEvent` [I names]. They must be matched against the keymap before or after the page, as GTK accelerators are today.

---

## 26. Persistent state read or written by these features [V]

| State | Where | Scope | Feature |
|---|---|---|---|
| `privacy.https_only` | prefs (`keys::HTTPS_ONLY`) | synced | §6 |
| Site permission `Http = Allow` per origin | `site_permissions` (private: in memory) | synced | §6 Continue |
| `performance.memory_saver`, `performance.memory_saver.mode` | prefs | LOCAL | §12 |
| Notification-allowed origins | `site_permissions` | synced | §12 keep-awake |
| `content.block_popups`, `scrolling.smooth`, `system.hardware_acceleration`, `spellcheck.enabled`, `spellcheck.languages` | prefs | synced except hardware acceleration | §1 |
| `site_zoom(host, percent)` table | profile DB (private: in memory) | LOCAL, never synced | §18 |
| History visits and titles | `history()` | synced | §3.2, §4 |
| Favicons for bookmarked pages | `favicons::record` | synced via bookmarks | §0.2 |
| Session `TabSnapshot` (id, url = `session_uri`, title = `display_title`, pinned, last_active_ms, group) | `session().save` | synced (Tabs) | §13 |
| `tab_restore_state` (engine back/forward blob) | profile DB | LOCAL, never on the wire | §13 |
| `downloads.directory` | prefs | LOCAL | §21 |
| `homepage` | prefs | synced | §3.1, §24 |
| Keymap (`shortcuts`) | prefs | synced | §25 |
| Engine data dir `<profile>/engine/`, cache dir `<cache>/Vsesvit/<name>/` | files | LOCAL | §1 |
| Closed-tab stacks | memory only | — | §23 |

Nothing is written for private tabs: no history, favicons, session or site-zoom rows.

---

## 27. Interactions with other modules

- **Address bar** (address_bar.rs): `show_uri(committed)`, `set_security`, `set_progress` / `finish_progress` / `clear_progress`, `set_zoom`, `set_in_use`, `set_starred`; per-tab typed text (`Tab::set_typed` / `take_typed`, restored on selection). On selection, focus goes to the address bar when the tab is blank or has typed text, otherwise to the web view (window/mod.rs:1460-1493).
- **Permissions** (permissions.rs): `handle`, `query`, `notification_allowed`, `committed`, `closed`, `capturing`, `indicator`, `sharing_title`, `enforce`; prompts hidden in full screen.
- **Tab groups / tab menu / tab search**: opener-based group adoption, closed-tab reopening into groups, `can_reopen_closed_tab`, MRU `used`.
- **Downloads**: response-policy downloads, `download_to` for Save Page As, `directory()`.
- **Favicons**: `favicons::faded` for sleeping tabs, `Browser::favicon_changed`.
- **Extension runtime** (`vsesvit_webext::Runtime`): `user_content_manager`, `tab_load`, `tab_updated`, `tab_opened_by`, `tab_closed`, `tab_attached`, `tab_moved`, `tab_activated`, `may_navigate`, `page_menu`, `runs_in`, `menu_clicked`. All are (WEBKIT-ONLY) if Chromium's extension system replaces vsesvit-webext. Otherwise each needs a CEF event source.
- **Trackers / cookies**: attached to every tab's content manager and to the view-source hidden view.
- **Session** (session.rs): `snapshot` uses `session_uri`, `display_title`, `session_state_bytes`, `last_active_ms`, `session_id`; `restore` uses `restore_saved`.
- **Self-test** (self_test.rs) touches these features in checks `navigate`, `tabs`, `tab_menu`, `tab_search`, `memory_saver`, `context_menus`, `selection_search`, `private_window`, `address_progress`, `spellcheck`, `save_page`, `page_commands`, `zoom_indicator`, `zoom_is_remembered_per_site`, `connection_info` (http heading "Connection is not secure"), `https_only`, `capture_in_use`, `screenshot`. Self-test-only hooks: `Tab::inspector_open`, `save_page::save`, `Browser::set_https_reach`, `Engine::settings`, `window.tab_look` / `tab_indicator` / `click_tab_indicator`. Several assertions are WebKit-specific (e.g. `TerminatedByApi`, `webkit::ContextMenu` item lists, `webkit::PrintOperation`) and need CEF-era equivalents. The check list itself is shared with Windows (`docs/design/self-test.md`).

---

## 28. Open questions (for the rewrite, not answered here)

1. Alloy style or Chrome style CEF browsers? This decides whether Chromium supplies the context menu (with extension items), print preview, popup blocking, status bubble, zoom bubble and the JS dialog UI, or whether the shell must.
2. Is there any way to restore navigation history in CEF 154 without patching? (Session restore, Ctrl+Shift+T and tab search reopen, Memory Saver wake.)
3. Which sleeping mechanism does Memory Saver use under CEF (freeze, hide, or recreate), and does edit tracking stay?
4. How should "is playing audio" be detected without `CefAudioHandler` swallowing the sound?
5. How should zoom changes from Ctrl+wheel, and Chromium's own per-host `HostZoomMap`, be reconciled with core's `site_zoom` (and its LOCAL persistence, private isolation, and "set 100% forgets the site")?
6. With Chromium, should Shift+click open a new window (Chromium default) or stay in the tab (Linux today)?
7. How are Vsesvit error pages and the HTTPS-only warning shown "under the failed URL"?
8. How does a Chromium `view-source:` navigation appear to `OnBeforeBrowse` and `OnAddressChange` (inner URL or prefixed)? This decides HTTPS-only and address bar behavior.
9. Live switching of smooth scrolling and GPU acceleration: accept restart-required semantics?
10. Spell check dictionaries: Chromium `.bdic` (downloaded) or system Hunspell? And where Add to Dictionary writes.
11. Find status: keep "N matches" wording or adopt Chrome's "i/N"? Drop the 1000 cap?
12. Screenshots that include page pixels with windowed CEF.

---

## 29. Checklist

- [ ] Tab identities: extension TabId, session UUID, Browsing fixed for life, `last_active_ms`, MRU `used` tick
- [ ] Tab change notifications to the holding window (Title, Favicon, Loading, Progress, Committed{Document, SameDocument, ErrorPage}, History, Zoom, Find, Capture, Audio, Sleep); Title and Favicon suppressed while asleep
- [ ] `display_title` rules (asleep title → page title → readable URL → "New Tab")
- [ ] `session_uri` (committed, else requested non-blank) for session and tab search
- [ ] `is_blank` (new-tab page at `about:blank` counts as blank)
- [ ] Readable URLs via `vsesvit_core::address::readable_url` (replacing WebKit `uri_for_display`)
- [ ] Security state per tab (Secure, Insecure, Internal, NotApplicable; error pages Insecure or NotApplicable)
- [ ] Persistent engine data in `ProfilePaths::engine_data` (and cache)
- [ ] One in-memory private engine context per private session
- [ ] Certificate errors fail the load, with no bypass
- [ ] Popup blocking pref applied live (block popups without a user gesture)
- [ ] Smooth scrolling pref applied
- [ ] Hardware acceleration pref applied
- [ ] Spell check on/off and languages from core `spellcheck::Dictionaries`
- [ ] Engine prefs re-applied after Settings changes and after sync changes any pref
- [ ] Load, reload, reload bypassing cache, stop, back, forward, home
- [ ] Reload/Stop button swap with tooltips; back/forward enablement
- [ ] Address-bar progress line driven by load progress; reset when the load ends
- [ ] After a load ends without commit (download, stop), the address bar shows the committed URL again
- [ ] Transition bookkeeping (Typed, Bookmark, Reload, default Link); stopped loads and downloads leave no transition; typed-over-load counts as typed
- [ ] History visit on every non-error commit (same-document included) for normal tabs; history title only for non-empty, non-error titles
- [ ] Same-document (fragment, pushState) commits update committed URI, history, star, permissions, without resetting zoom
- [ ] Web pages cannot open, redirect to, or open windows at non-web-accessible extension pages; browser loads, reload, back/forward and restore can
- [ ] Windows a page opened stay judged by that page (by reference, by name, via `opener`)
- [ ] Pages (web or `file://`) can neither open nor embed `view-source:`; browser loads can
- [ ] Ctrl+click and middle-click open a background tab next to the page; with Shift, a foreground tab; plain click stays (decide Shift+click)
- [ ] New tabs from clicks are reported as opened by the page and join its tab group
- [ ] Non-renderable main-frame responses become downloads
- [ ] HTTPS-only: upgrade public http main-frame navigations (core `https_only::upgrade`, `Upgrades::starting`)
- [ ] HTTPS-only: judge only main-frame navigations, before the request goes out where possible
- [ ] HTTPS-only: warning page at the http URL when the upgrade fails or loops back to http, titled "Connection is not secure"
- [ ] HTTPS-only: "Continue to site" (link click) stores the `Permission::Http` Allow exception (private: in memory) and loads over http
- [ ] HTTPS-only: warning marked Insecure in the address bar
- [ ] Error page "Unable to load this page" (with engine detail) under the failed URL, with Try Again
- [ ] Certificate error page "This connection is not secure" with per-problem bullets
- [ ] Crashed page "This page crashed" when the renderer dies (not for Memory Saver)
- [ ] Error pages: no history, no extension navigation events, no loop when the error page itself fails, light/dark styling, escaped content
- [ ] `window.open` / `target=_blank` open a tab next to the opener, selected, keeping `window.opener`; window features ignored
- [ ] Popups closed before showing are discarded from extension bookkeeping
- [ ] Page `window.close()` closes its tab (as an ordinary close)
- [ ] Opener and opened tabs are marked related (Memory Saver)
- [ ] Link hover preview at the bottom-left (readable URL, middle ellipsis, 90 chars, not clickable)
- [ ] Page full screen: window goes full screen; notice "{site} is now full screen / Press Esc to exit full screen" for 4 s; Esc exits
- [ ] F11 window full screen; top bars hidden in any full screen; prompts held back; notice hidden on leaving
- [ ] Audio indicator: muted speaker "Unmute Tab", playing speaker "Mute Tab", click toggles mute; capture icon takes priority; group dot fallback on the top bar
- [ ] Capture indicators (screen, camera, microphone) with descriptions; screen-sharing banner "Sharing your screen with {host}" with "Stop sharing"
- [ ] Notifications shown only when the site's setting allows; permission requests and queries routed to the permissions module
- [ ] Memory Saver sweep every 60 s with core `Sweep` / `TabActivity` / `IdleClock` (prefs, delays, keep-awake rules)
- [ ] Memory Saver unsaved-input script and re-judge after the async answer
- [ ] Memory Saver edit tracking in all frames (if sleeping reloads)
- [ ] Sleeping tab keeps title, address, history; icon at half opacity; session save after sleep
- [ ] Selecting a sleeping tab wakes it on the same page and history position
- [ ] Back/forward history saved per tab locally (`tab_restore_state`) and restored on session restore, with URL fallback when unreadable
- [ ] JS dialogs (alert, confirm, prompt) shown
- [ ] beforeunload confirmation on navigating away (decide on tab close)
- [ ] File chooser for `<input type=file>`; HTTP auth dialog; `<select>`/colour/date popups
- [ ] Print: Ctrl+P, main menu, page context menu, and `window.print()` open a print dialog; print to PDF for the self-test
- [ ] Developer tools toggle (Ctrl+Shift+I, F12, menu); JavaScript console (Ctrl+Shift+J); Inspect Element in the context menu
- [ ] Context menu: engine defaults for link, image, media, edit, spelling, navigation (add missing link and image items under Alloy style)
- [ ] Context menu: "Open Link in Private Window" after "Open Link in New Window" (only where the page may navigate)
- [ ] Context menu: "Copy Link Without Tracking" after "Copy Link Address" (core `clean_url::clean`)
- [ ] Context menu: selection item after Copy (core `omnibox().for_selection`; "Search {engine} for “…”" or "Go to …") opening a foreground tab next to the page
- [ ] Context menu: "Print…" and "View Page Source" (disabled without source) before Inspect Element on plain page hits
- [ ] Context menu: "Add to Dictionary" for misspellings
- [ ] Context menu: extensions' `contextMenus` items (separator + items/submenus/checkables) above Inspect Element's separator, only extensions allowed in the tab's kind, with the full click target (page, frame, link, src, media, selection, editable); `onClicked` with tab id; activeTab grant
- [ ] Zoom in/out/reset on Chrome's presets (core `zoom::DEFAULT`), snapping off-preset levels
- [ ] Zoom percent in the main menu row (Zoom Out, NNN% reset, Zoom In)
- [ ] Address-bar zoom pill only when not 100%, opening the zoom bubble (Zoom:, level, Zoom Out/In, Reset)
- [ ] Per-host zoom remembered in core `site_zoom` (LOCAL), applied on each new document, forgotten at 100%, private kept in memory; detect engine-side zoom changes (Ctrl+wheel)
- [ ] Find bar: one per window under the top bars; "Find in page" entry, Previous/Next buttons, status, close button
- [ ] Find: open with Ctrl+F (select all text, search at once); live case-insensitive wrap-around search; Enter/Ctrl+G next, Ctrl+Shift+G previous
- [ ] Find: status "1 match" / "{n} matches" / "No matches" (decide cap wording); error style when not found
- [ ] Find: Esc or close clears highlights and refocuses the page; follows tab switches with the same query
- [ ] View page source: Ctrl+U, menu and context menu open `view-source:<page>` in a selected tab next to the page; only http/https/file; action disabled otherwise
- [ ] View page source: typed and CLI `view-source:` addresses work; title `view-source:<url>`; line numbers not copied; keeps to HTTPS-only (no http fallback); a binary file's source shown, not downloaded
- [ ] Save Page As: Ctrl+Shift+S and menu; dialog titled "Save Page As" in the downloads folder
- [ ] Save Page As: HTML as MHTML named after the title (rules above); other documents saved as served (server or URL name); format and URI fixed when the dialog opens
- [ ] Save Page As: toast `Saved “name”` or `Cannot save the page: …`
- [ ] In-app screenshot of a window (with popovers) that includes page pixels, for the self-test and a debug hook
- [ ] Closed-tab stacks (normal and private, 25 each, stable never-reused keys); only committed non-blank tabs recorded; window closes don't record; private stack cleared at session end
- [ ] Reopen closed tab (Ctrl+Shift+T, tab menu enabled-state, tab search Recently closed) into the current window at its position, pinned state and group, with its history if the engine can restore it
- [ ] CLI target resolution (existing file → `file://`, else address-bar reading, else "vsesvit: cannot open …"); homepage via core `homepage_url`
- [ ] All shortcuts above come from core's keymap and reach the shell even while the page has focus
- [ ] Self-test checks listed in §27 ported to CEF-era assertions
