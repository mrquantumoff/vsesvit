# Behavior inventory: the browser window (`crates/vsesvit-gtk/src/window/`)

Scope: everything in `crates/vsesvit-gtk/src/window/` (`mod.rs` 1908 lines, `layout.rs`, `actions.rs`,
`menu.rs`, `tab_list.rs`, `tab_menu.rs`, `tab_search.rs`, `groups.rs`, `ext_actions.rs`; 5436 lines in
all), read at commit `27896e0` on branch `linux-cef-gpui`. To pin down what the window calls,
it also covers the parts of `browser.rs`, `tab.rs`, `session.rs`, `closed_tabs.rs`, `keymap.rs`,
`app.rs` (CSS and `app.*` actions), `lib.rs` (`popup`), `motion.rs`, `profiles.rs`
(avatar) and `extension_menus.rs` that the window depends on, plus the core modules it calls
(`tab_groups`, `tab_place`, `tab_search`, `private`, `prefs`, `extensions::toolbar`,
`extensions::manifest`, `shortcuts`, `profiles`, `new_tab`, `clean_url`, `permissions`, `session`).
It also covers the window parts of `docs/PLAN.md` ("Tabs", "Tab search", "Tab groups", "Private
windows", "Profiles > Titles/Profile button") and `docs/design/self-test.md`.

The document does not propose an architecture. It describes what the shell does today and what a
GPUI + CEF shell has to reproduce.

## Legend

- Statements are **verified** (read in the code or docs named) unless marked **[inference]**.
- Classification of engine-touching or toolkit-provided pieces:
  - **(KEEP)**: product behavior. Reproduce it.
  - **(WEBKIT-ONLY)**: a WebKitGTK workaround or limitation. It goes away with Chromium (note what
    replaces it, if anything).
  - **(NEEDS-CEF)**: needs a CEF equivalent. The likely CEF API is named. Names checked against the
    `cef-dll-sys 154.5.0+154.0.34` and `cef 154.5.0+154.0.34` bindings on disk are marked
    *(in bindings)*.
  - **(ADW-BUILTIN)**: behavior the old shell gets free from libadwaita/GTK widgets
    (`AdwTabView`, `AdwTabBar`, `AdwOverlaySplitView`, `GtkPopover`, ...). GPUI does not provide
    it, so the new shell has to build it explicitly. This is an extra tag, outside the three the
    task asks for.
- "core" means `vsesvit_core`. "runtime" means `vsesvit_webext::Runtime`, the old Linux extension
  runtime that is to be replaced; its calls are listed so the rewrite knows which extension events
  the window used to feed.

---

## 1. The window object and its lifecycle

Type: `BrowserWindow` (`window/mod.rs`), a subclass of `adw::ApplicationWindow` (GObject name
`VsesvitBrowserWindow`). Per-window state (`imp::BrowserWindow`):

- `browser`: the shared `Browser` controller. It is per process, and one process runs one profile.
- `browsing: core::private::Browsing` (`Normal` | `Private`), fixed for the window's whole life.
- `id: u32`: the id `chrome.windows` sees, from `Browser::allocate_window_id()` (a counter starting
  at 1).
- `layout: Option<Layout>`: the tab layout applied now (§4).
- `popups: Vec<Tab>`: `window.open` views WebKit has not declared ready (§20).
- `chrome_tab`: weak ref to the tab the header currently reflects. On a switch, its unsubmitted
  address text is saved into it.
- `closing`, `transferring`: guards that keep the window from closing itself in the middle of a
  close or a tab transfer.
- `bounds_report`: debounce timer for reporting resizes to extensions.
- `prompt: Option<ShownPrompt>`: the permission prompt on screen (§19).
- `fullscreen_notice_timeout`.
- `tab_menu: TabMenu` (§13), `tab_search: Option<TabSearch>` (§14).
- `groups: TabGroups<TabId>`, `groups_held: bool`, `chips`, `group_editor` (§15).

Creation, `BrowserWindow::with_browsing(browser, browsing)` (`new` = Normal), in order:
1. Sets `application`, the browser and the browsing kind, allocates the window id.
2. `set_default_size(1280, 820)`.
3. Private windows get the CSS class `private` (§17).
4. The title is "Vsesvit" at first (run through `show_title`, so it gets the profile and
   "(Private)" suffixes; §3).
5. `build_ui()` (§2), then `actions::install` (§8), `tab_menu::install` (§13), `groups::install`
   (§15), `connect_signals()`.
6. `apply_prefs()` (§4), `refresh_bookmarks_bar()`, `refresh_extension_actions()` (§16),
   `sync_profile()` (§7).
7. If `browser.downloads().started_this_session(browsing)`, the downloads button shows (§22).
8. `browser.updates()?.window_opened(&window)`: shows the update banner if one is pending, and
   schedules the first update check on the first window if updates are automatic.
9. `browser.sync().window_opened()`: on the first window of the run, schedules the first sync
   if signed in and the passphrase offer for later.
10. `browser.runtime().windows_changed()`: extensions hear `windows.onCreated` (§16, (NEEDS-CEF)).

Callers add tabs afterwards. `Browser::open_window(browsing, targets)` makes a window, opens the new
tab page when `targets` is empty, else opens one tab per target (`open_tabs`, the first selected),
then `present()`s it.

Closing (`close_request` → `before_close`):
- Sets `closing`.
- If this is the last normal window (`Browser::is_last_normal_window`: the window is normal and
  `windows_of(Normal).len() <= 1`) and it still has tabs, calls `browser.save_session_now()`, so the
  next start restores them.
- Every tab, and every pending popup, goes through `browser.tab_discarded(tab, window_closing=true)`,
  which calls `permissions::closed(tab)` and `runtime.tab_closed(id, true)`.
- Cancels the bounds-report timer.
- When the last private window is removed, `Browser` ends the private session (§17).
- On any `BrowserWindow` removal, `runtime.windows_changed()` runs.

`close_if_empty()`: the window closes itself when it has no pages, no transfer is under way (neither
`AdwTabView.is_transferring_page` nor its own `transferring`), and it is not already closing. It is
called on page detach, on a transfer state change and after `move_tab`.

Window-level signal wiring (`connect_signals`):

| Event | What the window does |
|---|---|
| tab view `selected-page` changed | `selection_changed()` (§10), then `settle_groups(None)` |
| `n-pinned-pages` changed | `settle_groups(None)` |
| `page-attached` (a tab added or dropped in) | If the tab's `browsing` differs from the window's: `send_back(page)` (§21). Otherwise: `sync_page(page)` (title, icon, loading, indicator), `browser.tab_attached(tab)` (calls `runtime.tab_attached`), `settle_groups(Some(tab))`, `browser.schedule_session_save()` |
| `page-reordered` | `browser.tab_moved(tab)` (calls `runtime.tab_moved` and schedules a save), `settle_groups(Some(tab))` |
| `close-page` | §10, "Closing a tab" |
| `page-detached` | `settle_groups(None)`, schedule a save, `close_if_empty()` |
| `is-transferring-page` changed | `close_if_empty()` |
| `indicator-activated` (speaker clicked) | toggles `web_view.is_muted` |
| `create-window` (a tab dragged out of a tab bar onto nothing) | a new window of the same browsing kind, presented. The page lands in its tab view |
| split view `collapsed` changed | `show_sidebar = (layout is Sidebar) && !collapsed` |
| address bar `edited(text, allow_inline)` | `browser.omnibox_changed(window, text, allow_inline)` |
| address bar `submitted(text)` | `browser.omnibox_activated(window, text)` |
| address bar `cancelled` | `focus_page()`: the selected web view takes the focus |
| address bar `bubble-changed` | `sync_permission_prompt()` |
| any click in the window (capture-phase `GestureClick`, all buttons, on release) | `clicked_while_prompting()` (§19) |
| `is-active` changed | When it becomes active: `browser.tab_used(selected)` (ticks tab search's MRU clock) and `browser.profile_used()` (re-reads `profiles.json`, marks this profile last used, quits if it was removed elsewhere). Always: `runtime.windows_changed()` (focus events) |
| surface `state` changed (minimized, maximized, fullscreen) | `runtime.windows_changed()` |
| surface `layout` (resize) | `report_bounds()`: debounced 250 ms (`BOUNDS_SETTLE_TIME`, "as in Chrome, a resize is reported once it is done"), then `runtime.windows_changed()` |
| profile button popover about to show | `browser.profile_used()`, `sync_profile()` |
| `fullscreened` changed | hides or shows all top bars (`ToolbarView.reveal_top_bars = !fullscreen`). Leaving fullscreen hides the notice. Then `sync_permission_prompt()` |

Classification:
- The lifecycle, the session save on the last normal window's close, and the MRU ticks are (KEEP).
- Extension window and tab events are (NEEDS-CEF). Chromium's extension system owns
  `chrome.windows` and `chrome.tabs`, and CEF exposes no public API for feeding it shell-drawn
  windows or tabs. The bindings have no extension types at all; see §27. This is the largest open
  issue for "total extension compatibility".
- `AdwTabView` page signals and `create-window` are (ADW-BUILTIN).

---

## 2. Window chrome layout (`build_ui`)

Widget tree, top to bottom:

```
AdwApplicationWindow
└─ AdwToolbarView  (css "browser-toolbar", top bar style Raised)
   ├─ top bar 1: AdwHeaderBar
   │   ├─ pack_start: header_start box (spacing 6):
   │   │     [Back] [Forward] [Reload|Stop] [Home (hidden by default)] [Sidebar toggle (left layout only)]
   │   ├─ pack_start (private windows only): Image "user-not-tracked-symbolic", tooltip "Private browsing"
   │   ├─ title widget: AdwClamp (hexpand) holding the AddressBar
   │   └─ pack_end: header_end box (spacing 6):
   │         [extension actions: pinned buttons + puzzle] [New Tab] [Downloads (hidden)] [Profile] [Main menu]
   │         [Sidebar toggle appended last in the right layout]
   ├─ top bar 2: AdwBanner (update banner; button runs app.update; hidden unless revealed)
   ├─ top bar 3: AdwTabBar (view = tab view, autohide=false; visible only in the top layout)
   │     start action widget: group chips box (spacing 2, hidden while there are no groups)
   │     end action widget: flat "Search Tabs" button (system-search-symbolic, win.search-tabs)
   ├─ top bar 4: BookmarksBar (revealed per the bookmarks_bar.visible pref)
   ├─ top bar 5: FindBar
   └─ content: AdwOverlaySplitView
         sidebar: TabList widget (§11)  min 180, max 300 px, width fraction 0.2
         content: AdwToastOverlay
                   └─ GtkOverlay
                        ├─ child: AdwTabView (one Tab widget per page: the web view + link preview + "sharing your screen" bar)
                        └─ overlay: fullscreen notice (§18)
```

Header buttons (all `icon_button(icon, action, tooltip)`):

| Button | Icon | Action | Tooltip | Notes |
|---|---|---|---|---|
| Back | `go-previous-symbolic` | `win.back` | "Back" | insensitive unless `can_go_back` |
| Forward | `go-next-symbolic` | `win.forward` | "Forward" | insensitive unless `can_go_forward` |
| Reload / Stop | `view-refresh-symbolic` / `process-stop-symbolic` | `win.reload` / `win.stop` | "Reload" / "Stop" | Becomes Stop while the selected tab loads (`sync_loading`) |
| Home | `go-home-symbolic` | `win.home` | "Home" | visible only with pref `toolbar.home_button` |
| Sidebar toggle | `sidebar-show-symbolic` (ToggleButton) | bound both ways to the split view's `show-sidebar` | "Show Tabs" | Sidebar layouts only. Sits after the navigation buttons on the left, last (after the menu) on the right |
| New Tab | `tab-new-symbolic` | `win.new-tab` | "New Tab" | |
| Downloads | `folder-download-symbolic` | `win.show-downloads` | "Downloads" | hidden until a download starts in this session for this browsing kind |
| Profile | MenuButton (flat) with the avatar as child | menu model (§7) | the profile's name | hidden in private windows and for a profile outside the list |
| Main menu | `open-menu-symbolic`, MenuButton `primary=true` | popover (§6) | "Main Menu" | `primary` means F10 opens it [inference: GTK MenuButton semantics] |

Address bar width (`set_compact_address_bar`), from pref `address_bar.compact` (default `true`):
compact means `AdwClamp.maximum_size = tightening_threshold = 720` (`COMPACT_ADDRESS_WIDTH`),
centered in the header. Off means `i32::MAX`, filling the space between the buttons. A unit test
checks that the compact bar is at most 720 px and within 40 px of the header's center.

Responsive behavior: an `AdwBreakpoint` on `max-width: 720sp` sets the split view's `collapsed=true`,
turning the sidebar into an overlay that the toggle opens. When `collapsed` changes, the sidebar
shows only if the layout is a sidebar and the split view is not collapsed. So narrowing hides it,
and widening shows it again. (ADW-BUILTIN; the behavior is KEEP.)

CSS that matters (from `app.rs` `CSS`):
- `.fullscreen-notice { padding: 12px 20px; border-radius: 12px; }`
- `.tab-sidebar { background-color: @sidebar_bg_color; }`; rows have zero padding.
  `.tab-row { padding: 4px 6px 4px 10px; min-height: 30px; }`.
  `.tab-row .tab-close { 22×22, opacity .6, 1 on hover }`.
  `.tab-sidebar row.tab-hidden { margin: 0 }`.
- `.tab-group-header { margin: 4px 6px 0; padding: 2px 6px 2px 10px; min-height: 24px; }`
- `.tab-group-chip { padding: 1px 8px; border-radius: 9px; font-size: .85em; bold }`;
  `.untitled` is a 12×12 dot (radius 6, font-size 1px).
- `.tab-group-button { padding: 2px 4px; }`, `.tab-group-swatch { 24×24 }`, `.tab-group-action { normal weight }`.
- Private window header: `window.private toolbarview.browser-toolbar > .top-bar` sets the headerbar
  background to `#2e2e32`, foreground `#ffffff`, backdrop `#222226` and its shade colors.
  Every top bar (header, banner, tab bar, bookmarks bar, find bar) turns dark.
- `.extension-badge { font-size .65em; bold; padding 0 3px; min 10×12; radius 6; background @accent_bg_color; color @accent_fg_color }`.
- Group colors are generated CSS (§15).

---

## 3. Window titles

`show_title(page_title)` sets the OS window title to
`window_title(profiles::with_profile(page_title, browser.profile_title()), browsing)`:
- `core::profiles::with_profile(page, Some(name))` gives `"{page} - {name}"`. `None` gives the page title.
- `Browser::profile_title()` is `Registry::title_name(&home.id)`, which returns the profile's name
  only when the profile list has 2 or more profiles. A profile outside the list (`--profile-dir`
  elsewhere) has no `home`, so no suffix.
- Private windows: `"{title} (Private)"`.
- Examples (self-test `profiles`): "Page" with one profile; "Page - Person 1" with two;
  "Page - Person 1 (Private)" for a private window; "New Tab (Private)" for a private window's new
  tab (unit test `moving_a_private_tab_to_a_new_window_keeps_it_private`). Windows differs:
  "Page - Work - Vsesvit".
- The page title is `Tab::display_title()`: the sleeping tab's remembered title, else the
  web view's non-blank title, else the committed URI shown for display (`display_uri`: punycode and
  percent-escapes decoded, using WebKit's `uri_for_display`), else "New Tab".
- The title updates when the selected tab's title changes (`TabChange::Title`), when the selection
  changes, and when the profile list changes (`profiles_changed`).
- A new window shows "Vsesvit" (with suffixes) until a tab is selected.

Classification: (KEEP). `display_uri` uses WebKit's `uri_for_display`, (WEBKIT-ONLY); the new shell
needs its own URL-for-display (core may have one; `core::address` was not checked here).

---

## 4. Tab layouts: left, right, top

Pref: `tabs.position` (`core::prefs::keys::TABS_POSITION`, synced, `TabsPosition::{Left, Right, Top}`,
default `Left`, serialized as `"left"|"right"|"top"`).

`layout::Layout::for_position`: Left gives `Sidebar(Start)`, Right gives `Sidebar(End)`, Top gives `TopBar`.

`apply_layout(position)` is applied live, without recreating widgets:
- Sidebar: hides the tab bar, sets the split view's `sidebar-position` (start or end), moves the
  sidebar toggle into `header_start` (after Home) for Start or appends it to the end of
  `header_end` (after the main menu) for End, makes it visible, and shows the sidebar unless the
  split view is collapsed.
- TopBar: hides the sidebar and the toggle, shows the `AdwTabBar`.
- Then `redraw_groups()`, because the group dots exist only in the top layout.

Who applies it:
- `apply_prefs()` at window creation, which also applies `bookmarks_bar.visible`,
  `toolbar.home_button`, `address_bar.compact` and `address_bar.full_urls`.
- `Browser::set_tabs_position(p)` (Settings): writes the pref, then `apply_layout` on every window.
- `Browser::sync_applied(changed)`: when any pref changed through sync, every window
  re-runs `apply_prefs()` and `refresh_extension_actions()`. A unit test checks that a synced
  change lays out an open window exactly as a new one would be laid out.
- Dev-only D-Bus action `app.debug-set-tabs-position(left|right|top)` (`app.rs`, debug builds).

Toggle: `win.toggle-tab-sidebar` (core `ToggleTabList`, default **Ctrl+S** and **F9**; Chrome's Ctrl+S
save moved to Ctrl+Shift+S) flips `show-sidebar` in sidebar layouts and does nothing on top. The
self-test `ctrl_s_toggles_sidebar` checks that Ctrl+S runs only `win.toggle-tab-sidebar`, hides the
sidebar, and shows it again.

Self-test support (`layout.rs`, `#[cfg(feature = "self-test")]`):
- `BrowserWindow::layout_probe() -> LayoutProbe { width, sidebar: Option<Rect>, tab_bar, web_view }`
  measures the widgets' bounds in window coordinates; a hidden or unmapped widget gives `None`.
- `layout::classify(&probe) -> Option<TabsPosition>`: Left when the sidebar is at x≈0, ends before
  the web view, and the web view reaches the right edge. Right is the mirror. Top when the tab bar is
  above the web view and the web view spans the width. A frame caught mid-transition, an overlay,
  or both bars at once classify as `None`. Slack is 0.5 px.
- Self-test `tab_layout`: a fresh profile reads Left; then Right, Top and Left again, each confirmed
  from geometry; the pref ends as Left.

Classification: (KEEP) for product behavior; (ADW-BUILTIN) for the split view and tab bar widgets. The
geometry-based self-test check should be kept: a GPUI shell can report element bounds the same way.

Windows differences (PLAN, context only): a collapsible pane shows only favicons when collapsed. With
vertical tabs, the title bar holds the toolbar. Linux has neither the favicon-only collapsed state
nor a title-bar merge.

---

## 5. Toolbar and header state following the selected tab

`selection_changed()` runs on every selected-page change:
1. The tab shown before keeps its unsubmitted address text: `previous.set_typed(address.take_edit())`.
2. With no tab selected, the window forgets `chrome_tab`, re-checks the permission prompt and returns.
3. `address.clear_progress()`. `typed = tab.take_typed()`. `address.restore(typed, committed_uri)`
   puts back the tab's own unsubmitted text if it had any, else shows its URL.
4. `find_bar.retarget(tab.web_view())`.
5. Title (§3), `sync_location`, `sync_history`, `sync_loading`, `sync_zoom`, `sync_star`, and
   `address.set_in_use(permissions::indicator(tab.capturing()))`.
6. `tab.mark_active(session::now_ms())` (the session's `last_active_ms`) and `tab.wake()` (Memory
   Saver: a sleeping tab reloads).
7. `browser.tab_activated(tab)`: an MRU tick, `runtime.tab_activated`, and a scheduled session save.
8. Focus: if the tab had unsubmitted text or `tab.is_blank()` (nothing requested yet, as on a new
   tab), the address bar takes the focus for typing. Otherwise the web view takes it.
9. `sync_permission_prompt()`.

Per-tab change dispatch, `tab_changed(tab, TabChange)`, called by `Tab::notify`:

| `TabChange` | Effect |
|---|---|
| `Title` | the page row/tab title. If selected: the window title. Then `browser.title_changed(tab)`: history title write (normal tabs, not error pages, non-empty), `runtime.tab_updated`, save |
| `Favicon` | the page icon. `browser.favicon_changed(tab)` stores it if bookmarked (normal tabs only) and refreshes bookmark views |
| `Loading` | the page's loading spinner. If selected: `sync_loading`, and when loading stopped, `sync_location`. A load that ends without committing (a download, a stop) restores the real URL in the bar |
| `Progress` (selected) | `sync_loading` (progress fraction in the address bar) |
| `Committed(commit)` | page title. If selected: `sync_location`, `sync_history`. `browser.navigation_committed(tab, uri, commit)`: site zoom applied, history visit (normal tabs, non-error, with the pending `Transition`, default `Link`), `runtime.tab_updated`, save. If selected: `sync_star`, `sync_permission_prompt` |
| `Audio` | `sync_indicator` |
| `Sleep` | page icon set to the faded icon (`Tab::icon()` returns the faded texture while asleep) |
| `Capture` | `sync_indicator`. If selected: the address bar's in-use button |
| `History` (selected) | `sync_history` |
| `Zoom` | `browser.zoom_changed(tab)` stores the per-site zoom (in memory for private tabs). If selected: `sync_zoom` |
| `Find(result)` (selected) | `find_bar.show_result(result)` |

Helpers:
- `sync_location`: `address.show_uri(committed_uri)`, `address.set_security(tab.security())`.
  `win.view-source` is enabled only if `tab.source_url()` is `Some`: http, https and file pages,
  via `view_source::source_url`.
- `sync_history`: enables `win.back` / `win.forward` from `can_go_back` / `can_go_forward`.
- `sync_loading`: swaps the Reload and Stop button; `address.set_progress(estimated_load_progress)`
  or `address.finish_progress()`.
- `sync_zoom`: the main menu's zoom label shows `zoom::percent(level)` ("100%"), and
  `address.set_zoom(level)` drives the address bar's zoom indicator.
- `sync_star`: `browser.is_bookmarked(committed_uri)` sets the `win.bookmark-page` state and the
  address bar's star.

Engine mapping:
- Title: (NEEDS-CEF) `CefDisplayHandler::OnTitleChange` *(in bindings)*.
- Favicon: (NEEDS-CEF) `CefDisplayHandler::OnFaviconURLChange` *(in bindings)* +
  `CefBrowserHost::DownloadImage` *(in bindings)*.
- Loading and back/forward: `CefLoadHandler::OnLoadingStateChange(isLoading, canGoBack,
  canGoForward)` *(in bindings)*.
- Progress: `CefDisplayHandler::OnLoadingProgressChange` *(in bindings)*.
- Committed URL: `CefDisplayHandler::OnAddressChange` (main frame) and/or `CefLoadHandler::OnLoadStart` /
  `OnLoadEnd` [inference on which one gives "committed"].
- Zoom: `CefBrowserHost::GetZoomLevel/SetZoomLevel` and `Zoom(cef_zoom_command_t)` *(in bindings)*.
  CEF zoom *levels* are logarithmic, not factors: 0.0 is 100% [inference: Chromium uses factor =
  1.2^level]. The preset table (`zoom.rs` `LEVELS` = 25, 33, 50, 67, 75, 80, 90, 100, 110, 125,
  150, 175, 200, 250, 300, 400, 500 %) and core's stored percent values need conversion.
- Find: `CefBrowserHost::Find` + `CefFindHandler` [inference].

---

## 6. Main menu (`menu.rs::main_menu`)

A `GtkPopoverMenu` from a `gio::Menu` with five untitled sections (underscores are mnemonics):

1. **New _Tab** (`win.new-tab`), **New _Window** (`app.new-window`), **New _Private Window** (`app.new-private-window`)
2. A custom row, "zoom": [**−** `zoom-out-symbolic`, `win.zoom-out`, tooltip "Zoom Out", flat circular]
   [**"100%"** label button, `win.zoom-reset`, tooltip "Reset Zoom", flat, numeric, hexpand]
   [**+** `zoom-in-symbolic`, `win.zoom-in`, tooltip "Zoom In", flat circular]. Margins 12, spacing 6.
   The label follows the selected tab (`sync_zoom`).
3. **_Print…** (`win.print`), **_Save Page As…** (`win.save-page`), **_Find…** (`win.find`),
   **_Fullscreen** (`win.fullscreen`), **More _Tools ▸** { **_Developer Tools** (`win.developer-tools`),
   **View Page _Source** (`win.view-source`) }
4. **_Bookmarks** (`win.show-bookmarks`), **_History** (`win.show-history`), **_Downloads**
   (`win.show-downloads`), **_Extensions** (`win.show-extensions`), **Show Bookmarks _Bar**
   (`win.show-bookmarks-bar`, a stateful boolean check item)
5. **_Settings** (`win.show-settings`), **_Keyboard Shortcuts** (`app.shortcuts`), **_Welcome**
   (`app.welcome`), **_About Vsesvit** (`app.about`)

Not in the menu: Quit (`app.quit`, Ctrl+Q), JavaScript console (shortcut only), Copy link, Reopen
closed tab, Search tabs (these have shortcuts or other entry points). PLAN says "New private window in
the main menu, after New window", which matches.

Classification: (KEEP). The menu's items point at actions; for the engine-touching ones see §8.

---

## 7. Profile button and menu

`sync_profile()`:
- The button is hidden when `browser.home()` is `None` (a profile directory outside
  `profiles/`) or the window is private ("as Chrome hides it in incognito windows").
- Otherwise: `registry = browser.profiles()` (the cached `core::profiles::Registry`), `entry =
  registry.get(&home.id)`. The name is `entry.name`, or the id string if not listed.
- Avatar: `crate::profiles::entry_avatar(&home.dir, entry, false)` gives an `adw::Avatar` (22 px) with
  the account picture (`ProfilesDir::picture(entry)`, a PNG in the profile dir) if one exists.
  Otherwise a 22 px circle label (`.profile-avatar.profile-<color>`, white bold initial from
  `core::profiles::avatar_initial(name)`). A missing entry gives `avatar(name, ProfileColor::Slate,
  None)`.
- Tooltip = the name. Menu model = `menu::profile_menu(&registry)`:
  - Section 1: one item per `registry.profiles()`, labelled with the name (`_` doubled to
    escape mnemonics). Each targets `win.open-profile(<profile id>)`. That action is stateful
    with the current profile's id as state, so the current profile shows checked (radio)
    [inference: GMenu radio rendering for a string-state action with a matching target].
  - Section 2: **_Add Profile…** (`win.add-profile`), **_Manage Profiles…** (`win.manage-profiles`).
- Opening the popover first calls `browser.profile_used()` (re-reads `profiles.json` through
  `ProfilesDir::opened(&id)`, which also marks the profile last used, and quits if the profile was
  removed by another process) and rebuilds the menu.

Actions:
- `win.open-profile(s)`: `Browser::open_profile(window, id)`. The current profile just
  `present()`s the window. Another profile is launched with `profile::launch(window,
  &home.dir.root(id))` (`vsesvit --profile-dir <dir>`, through the display's launch context so
  Wayland gets an activation token). Failure shows the toast "Could not open the profile: {e}".
- `win.add-profile`: `crate::profiles::add(window)`, a dialog asking name and colour, then
  `Browser::add_profile` → `ProfilesDir::add(name, color)` and opens it.
- `win.manage-profiles`: `crate::profiles::manage(window)`, the Manage Profiles dialog (separate inventory).
- `profiles_changed()` (from `Browser::set_profiles` on every window) re-syncs the button and the title.

Classification: (KEEP). Launching with an activation token is toolkit-specific: GPUI needs its own
xdg-activation token path [inference]. No engine involvement.

---

## 8. Window actions (`actions.rs`) and the actions other modules install

All actions are GActions on the window (`win.*`). Accelerators come from the profile's keymap
(`keymap::apply`, application-wide `set_accels_for_action`), so every shortcut below can be
reassigned in Settings except the ones marked built-in.

| Action | Param / state | Does | Default keys (core `shortcuts::Command`) | Engine |
|---|---|---|---|---|
| `open-profile` | `s`, state = current profile id | §7 | | |
| `add-profile` | | §7 | | |
| `manage-profiles` | | §7 | | |
| `new-tab` | | `new_tab()`: new tab page at the end, selected; address bar focused | Ctrl+T | |
| `close-tab` | | closes the selected page | Ctrl+W, Ctrl+F4 | |
| `reopen-closed-tab` | | `browser.reopen_closed_tab(window)` | Ctrl+Shift+T | §10 |
| `search-tabs` | | `toggle_tab_search()` (§14) | Ctrl+Shift+A | |
| `focus-location` | | `address.focus_for_typing()` | Ctrl+L, Alt+D, F6 | |
| `reload` | | `Transition::Reload` pending, then `tab.reload()` | Ctrl+R, F5 | (NEEDS-CEF) `CefBrowser::Reload` |
| `reload-bypass-cache` | | same with `reload_bypass_cache()` | Ctrl+Shift+R, Shift+F5 | `CefBrowser::ReloadIgnoreCache` |
| `stop` | | `tab.stop()`: clears the pending transition, stops loading | (Reload button while loading) | `CefBrowser::StopLoad` |
| `home` | | `go_home()` (§10) | none | |
| `back` / `forward` | enabled per history | `tab.go_back()` / `go_forward()` | Alt+Left / Alt+Right | `CefBrowser::GoBack/GoForward` |
| `copy-clean-link` | | `copy_link(selected, clean=true)` | Ctrl+Shift+C (Chrome uses it for the element inspector) | |
| `copy-link` | | `copy_link(selected, clean=false)` | Ctrl+Alt+Shift+C | |
| `bookmark-page` | state `bool` = starred | `browser.star_clicked(window)`: adds the page to the bookmarks bar ("Bookmark added" bubble) or opens the "Edit bookmark" bubble on the star | Ctrl+D | |
| `show-site-info` | | `show_site_info()` (§22) | | |
| `open-bookmark` | `s` url | `navigate_with(url, Transition::Bookmark)` | | |
| `open-in-new-tab` | `s` url | `open_tab(url, None, Foreground)` | | |
| `save-page` | | `save_page::present(window)` | Ctrl+Shift+S | (NEEDS-CEF), separate inventory |
| `find` / `find-next` / `find-previous` | | find bar open / step | Ctrl+F / Ctrl+G / Ctrl+Shift+G | (NEEDS-CEF) `CefBrowserHost::Find/StopFinding` |
| `print` | | `tab.print()`: WebKit's print dialog | Ctrl+P | (NEEDS-CEF) `CefBrowserHost::Print` *(in bindings)* |
| `view-source` | enabled iff the page has a source URL | `tab.view_source()`: opens `view-source:<url>` in a new foreground tab next to the page (opener = page, so it joins its group) | Ctrl+U | Chromium renders `view-source:` natively. The old shell's custom scheme is (WEBKIT-ONLY) |
| `developer-tools` | | `tab.toggle_inspector()`: opens, or closes if open | Ctrl+Shift+I, F12 | (NEEDS-CEF) `CefBrowserHost::ShowDevTools/CloseDevTools/HasDevTools` *(in bindings)* |
| `javascript-console` | | `tab.show_inspector()`. "WebKit has no way to open the inspector on a given panel" | Ctrl+Shift+J | (WEBKIT-ONLY) limitation. CEF: `ShowDevTools`, then select the console panel, e.g. via DevTools frontend APIs [inference] |
| `zoom-in` / `zoom-out` / `zoom-reset` | | steps through `zoom.rs` `LEVELS` / resets to 1.0 | Ctrl+Plus/Ctrl+Equal/Ctrl+KP_Add; Ctrl+Minus/Ctrl+KP_Subtract; Ctrl+0/Ctrl+KP_0 | (NEEDS-CEF) zoom level mapping, §5 |
| `fullscreen` | | toggles window fullscreen | F11 | |
| `toggle-tab-sidebar` | | §4 | Ctrl+S, F9 | |
| `show-bookmarks-bar` | state `bool` | `browser.set_bookmarks_bar_visible(v)`: writes the pref, applied to every window | Ctrl+Shift+B | |
| `show-bookmarks` / `show-history` / `show-downloads` | | open those windows | Ctrl+Shift+O / Ctrl+H / Ctrl+J | |
| `extension-pin` | `(sb)` id, pinned | `browser.pin_extension(id, pinned)` (§16) | | |
| `extension-move` | `(su)` id, index | `browser.move_extension(id, index)` (§16) | | |
| `extension-command` | `(ss)` id, command | `run_extension_command(id, name)` (§16) | per extension, from core `extensions::commands` | (NEEDS-CEF) |
| `show-extensions` | | Extensions dialog | | |
| `show-settings` | | Settings dialog | Ctrl+Comma | |
| `none` | disabled | target of "(Empty)" bookmark folder entries; never enabled | | |

Application actions used by the window chrome (`app.rs::install_actions`):
- `app.new-window` (Ctrl+N): `browser.open_window(Normal, [])`.
- `app.new-private-window` (Ctrl+Shift+N): `browser.open_window(Private, [])`.
- `app.about`, `app.welcome` (on the active `BrowserWindow`), `app.shortcuts` (Ctrl+?, the shortcuts
  help).
- `app.update`: the update banner's button.
- `app.quit` (Ctrl+Q): `save_session_now()` first, "the windows are gone by the time `shutdown`
  runs", then `app.quit()`.
- The notification action (extension notifications) is outside this inventory.

Other action groups on the window: `tab.*` (§13) and `tab-group.*` (§15).

---

## 9. Keyboard map summary

Rebindable shortcuts come from core `shortcuts.rs` (`Command`, defaults "follow Chrome", stored
overrides in synced pref `keyboard.shortcuts`) through `keymap::binding(cmd)`. Exceptions:
- `NextTab` / `PreviousTab` are `BuiltIn("<Control>Tab")` / `BuiltIn("<Control><Shift>Tab")`: listed
  in the shortcuts help but not rebindable. The window binds them itself (§15).
- `SelectTab1..8` and `SelectLastTab` (core defaults Ctrl+1..8, Ctrl+9) return `None`: **the GTK
  shell does not bind them** (unit test `binding(Command::SelectTab1) == None`). The WinUI shell
  binds them. With `AdwTabView`'s own shortcuts left on, Alt+1..9 and Alt+0 select tabs
  [inference from libadwaita's `AdwTabViewShortcuts` defaults (`ALT_DIGITS`, `ALT_ZERO`), not checked on disk].

Window-handled keys (not in the keymap):
- Ctrl+Tab, Ctrl+KP_Tab, Ctrl+Page_Down: next shown tab. Ctrl+Shift+Tab, Ctrl+Shift+ISO_Left_Tab,
  Ctrl+Shift+KP_Tab, Ctrl+Page_Up: previous shown tab. Capture phase on the window, which skips tabs
  hidden in collapsed groups and wraps around (§15). `AdwTabView`'s own versions are removed
  (`remove_shortcuts(CONTROL_TAB | CONTROL_SHIFT_TAB | CONTROL_PAGE_UP | CONTROL_PAGE_DOWN)`).
- Remaining `AdwTabView` defaults [inference: libadwaita docs]: Ctrl+Home / Ctrl+End select the
  first / last tab; Ctrl+Shift+Page_Up / Page_Down move the tab left / right; Ctrl+Shift+Home / End
  move it to the start / end; Alt+1..9 / Alt+0. (ADW-BUILTIN). Whether to keep them is a product
  decision; Chrome has Ctrl+Shift+PgUp/PgDn for moving tabs.
- Shift+F10 or the Menu key on a focused tab row opens its tab menu; on a group header, its editor.
- In the tab list: arrows, Home, End, Page Up and Page Down move the selection over shown rows only.
  Tab and Shift+Tab skip hidden rows. Enter and Space on a group header toggle it (§11).
- In tab search: Up/Down and KP_Up/KP_Down move the selection with wraparound, Enter chooses, Escape
  closes (§14).
- F10 opens the main menu (MenuButton `primary`) [inference].
- Extension commands: per-extension accelerators on `win.extension-command(('id','name'))`,
  including `"global": true` ones, which only work while a Vsesvit window has focus (PLAN).

---

## 10. Tab model operations (`mod.rs`)

A tab is a `crate::tab::Tab` widget (web view + overlays) placed in an `AdwTabView` page. Tab
order, pinned state and selection live in the `AdwTabView`. Groups live in core's `TabGroups`, keyed
by `TabId` (§15).

Identity: `Tab::id() -> vsesvit_webext::TabId` (from `Browser::allocate_tab_id`, a counter starting at 1)
is what extensions see. The session uses a separate `session_id() -> core::session::TabId` (UUID).
Each tab keeps `used: u64` (MRU tick, set on creation and on every activation) and
`last_active_ms` (wall clock, saved in the session).

Opening:
- `open_tab(uri, opener, focus)`: `Tab::new(browser, browsing)`, `tab.load(uri)` if given, then
  `insert_tab`. If there is an opener: `runtime.tab_opened_by(tab, opener)` and
  `adopt_into_group(tab, opener)` (core `TabGroups::adopt`). Then `AdwTabView.add_page(tab,
  parent_page)`, selected if `Focus::Foreground`.
  `add_page` with a parent places the new page after the opener and the opener's earlier children,
  as Chrome places tabs opened from a page, and records the parent for the selection-on-close pick
  [inference from libadwaita docs, not checked on disk]. Without a parent it appends.
- `open_tab_at(uri, index, focus)` (`chrome.tabs.create` with `index`): inserts at `min(index,
  n_pages)`.
- `open_tabs(uris)`: one tab per URI at the end; the first in the foreground, the rest in the
  background.
- `new_tab()`: a blank tab at the end, foreground, then `load_new_tab_page(tab)`:
  `core::new_tab::page(&mut profile, browsing)` returns HTML (most-visited tiles and a search box; in
  private windows `PRIVATE_PAGE`, "You're browsing privately"). It is loaded with
  `web_view.load_html(html, None)`, so the tab sits at `about:blank`, still reads as blank (empty
  address bar, title "New Tab", not saved in the session or history), and the address bar takes the
  focus.
  - (NEEDS-CEF) There is no direct `load_html`. Options are a data URL, a custom scheme handler
    (`CefSchemeHandlerFactory`), or `CefFrame::LoadRequest`. The "reads as `about:blank`"
    property has to be reproduced deliberately [inference].
- Links with middle click or Ctrl+click open a background tab; with Shift, a foreground tab
  (`tab.rs::new_tab_for_click`). Popups: §20.

Selecting: `select_tab(tab)`; `selected_tab()`; `tabs()` (in view order).

Navigation helpers:
- `navigate_with(uri, transition)`: the selected tab (or a new one) gets `set_pending_transition`,
  `load(uri)` and focus on the web view. Used by the omnibox (`Typed`), bookmarks and home
  (`Bookmark`).
- `go_home()`: `browser.homepage()` = `core::prefs::homepage_url(pref "homepage")`. `None` for
  `""`, `about:home` (the default), `about:blank` or text that isn't an address. `Some(url)` means
  `navigate_with(url, Transition::Bookmark)` ("Like Chrome, the visit counts as a bookmark's").
  `None` loads the new tab page into the **selected** tab (or a new one) and focuses the address bar.
  A unit test checks that the home button follows the setting in every window and opens the homepage
  in the selected tab.

Pinning: `is_pinned(tab)`, `set_pinned(tab, bool)` (`AdwTabView.set_page_pinned`). Pinned pages lead
the view; pinning moves a page to the end of the pinned block; unpinning moves it to the first
unpinned slot (core `Step::Unpin` docs: "puts it first among the unpinned tabs, as both shells' Unpin
does"). (ADW-BUILTIN; KEEP.)

Muting: `web_view.set_is_muted(bool)`. Speaker and mute indicator below. (NEEDS-CEF)
`CefBrowserHost::SetAudioMuted / IsAudioMuted` *(in bindings)*.

**Indicator** (`sync_indicator(page, tab)`), one icon slot per tab in both tab lists, in priority order:
1. Capturing (`permissions::indicator(tab.capturing())`): icon `screen-shared-symbolic` (screen),
   else `camera-web-symbolic` (camera), else `audio-input-microphone-symbolic`. The tooltip is
   core's `Capturing::description()`: "Using your camera and microphone", "Using your camera", "Using
   your microphone", "Sharing your screen", "Sharing your screen and using your {devices}". Not
   clickable.
2. Muted: `audio-volume-muted-symbolic`, tooltip "Unmute Tab", clickable (unmutes).
3. Playing audio: `audio-volume-high-symbolic`, tooltip "Mute Tab", clickable (mutes).
4. Top layout only: the group's dot (§15), tooltip = the group's name. Not clickable.

Clicking an activatable indicator emits `indicator-activated`, which toggles mute. The self-test
`tab_mute` part of `tab_menu` checks muted icon + "Unmute Tab", then a click unmutes and clears the
indicator. `capture_in_use` checks the camera icon in the sidebar and on the top strip.

- Capture state: (NEEDS-CEF) `CefDisplayHandler::OnMediaAccessChange(has_video_access,
  has_audio_access)` *(in bindings)*. It has no separate screen-share flag; screen capture likely
  counts as video [inference]. Telling camera from screen needs more (e.g. CefPermissionHandler
  request types `CEF_MEDIA_PERMISSION_DESKTOP_VIDEO_CAPTURE` [inference]).
- "Playing audio": (NEEDS-CEF, **risk**) no "is audible" getter or event exists in the 154
  bindings (searched: no `is_audible`, no audio-state callback in `cef_display_handler_t`).
  `CefAudioHandler::OnAudioStreamStarted/Stopped` *(in bindings)* exists, but its doc says it
  configures audio stream *capture* (`GetAudioParameters` "Return true to proceed with audio stream
  capture"), so using it may divert the audio [inference]. Open question.

**Sleeping tabs** (Memory Saver; policy lives in core `memory_saver` and `Browser::sleep_idle_tabs`,
separate inventory). The window shows a sleeping tab's icon faded (`favicons::faded`, half alpha) in
both lists (`TabChange::Sleep`), and `display_title` keeps the remembered title. Selecting the tab
wakes it (`tab.wake()` → reload). (WEBKIT-ONLY: sleep = `terminate_web_process`.) A CEF equivalent
is needed for the sleep itself (see the memory-saver inventory); the faded icon and kept title are
(KEEP).

**Closing a tab.** `close_tab(tab)` and `close_selected_tab()` call `AdwTabView.close_page`. The
`close-page` handler:
1. `browser.tab_closed(tab, position, pinned, group_id)`: `permissions::closed(tab)`,
   `runtime.tab_closed(id, false)`, schedule a save. Then, if the committed URI exists and is not
   `about:blank`, it pushes a `ClosedTab { uri, title: display_title, favicon, state:
   web_view.session_state() (back/forward), position, pinned, group, used: tick }` onto the closed
   stack **of the tab's browsing kind**: `closed_tabs` or `private_closed_tabs`,
   `ClosedTabs::new(CLOSED_TABS_KEPT = 25)`, oldest dropped, each with a never-reused `ClosedKey`.
2. If the page was selected: `select_after_closing(page)` (§15) picks the next tab before the
   view does.
3. If this is the window's last page and the window is the last normal window:
   `browser.save_session_now()` while the tab still exists, "so that, as in Chrome, the next start
   restores it" (unit test `closing_the_last_tab_of_the_last_window_keeps_it_in_the_session`).
4. `close_page_finish(page, true)`. The page leaves at once; its sidebar row animates out after (§11).
- Closing the last tab closes the window (`page-detached` → `close_if_empty`).

**Reopen closed tab** (`Browser::reopen_closed_tab(window)`): pops the stack of the window's kind
and calls `window.restore_closed(&closed)`: a new `Tab`; if `closed.pinned`, `insert_pinned` at
`clamp(position, 0, n_pinned)`, else `insert` at `clamp(position, n_pinned, n_pages)`; selected;
`rejoin_group(tab, group)` if it had a group (core `TabGroups::rejoin`);
`tab.restore(state, uri)`, which restores the WebKit back/forward state and goes to the current
item, or loads the URI.
- (NEEDS-CEF, **risk**) CEF has no API to *restore* a navigation history. Only
  `CefBrowserHost::GetNavigationEntries(visitor)` *(in bindings)* reads it, and no setter was found.
  So reopened, duplicated and restored tabs keep their back/forward history in the old shell
  (PLAN: "A reopened tab keeps its back/forward history, as Ctrl+Shift+T does"), and CEF probably
  cannot match that. WinUI has the same limitation and reopens the address only. Open question.
- `can_reopen_closed_tab(browsing)` enables the tab menu's Reopen Closed Tab.

**Moving a tab** (`move_tab(tab, target_window, index)`, used by `chrome.tabs.move` and
`windows.create({tabId})`):
- Same window: `reorder_page(page, min(index, n-1))`.
- Another window: `transfer_page(page, target_view, min(index, n_target))` with `transferring` set,
  then `close_if_empty()`. The same live web view moves, keeping its page, history and state.
  (WEBKIT-ONLY mechanism. In CEF this means reparenting a browser to another top-level window:
  windowed mode on X11 would need `XReparentWindow` of the browser's host window, or the browser
  is drawn off-screen into whichever GPUI window holds it [inference]. **Risk**: reparenting CEF
  browsers between top-level windows on Linux/Wayland.)

**Copy link** (`copy_link(tab, clean)`): `tab.link()` (committed URI, never `about:blank`), run through
`core::clean_url::clean(&link)` if `clean` (drops tracking parameters, unwraps redirect wrappers
up to 8 deep, http/https only, otherwise keeps the text unchanged). Puts it on the clipboard and
shows the toast **"Link copied"** (`dialogs::plain_toast`). Self-test `tab_menu`: Copy Link, Ctrl+Shift+C
and the page menu's "Copy Link Without Tracking" all give `/page2.html?a=1` from
`?utm_source=self-test&a=1`; `win.copy-link` gives the whole address.

---

## 11. Vertical tab list (`tab_list.rs`)

Structure (`TabList::new(view)`): a vertical box with class `tab-sidebar` holding:
1. **"Search Tabs"** button: `adw::ButtonContent` with `system-search-symbolic` and the label,
   `win.search-tabs`, flat, full width, margins 6. Tab search anchors here while the sidebar shows.
2. Separator.
3. A `GtkScrolledWindow` (no horizontal scroll, vexpand) with a `GtkListBox` (single selection, css
   `navigation-sidebar`, valign start).
4. Separator.
5. **"New Tab"** button (`tab-new-symbolic`, `win.new-tab`), same style.

The list mirrors `AdwTabView.pages()` (a `SelectionModel`) through `items-changed`: one row per page,
in view order. Row indices equal page indices, which is why group headers are list-box headers
and not rows.

**Row** (`TabRow`, a horizontal box, spacing 8, css `tab-row`), bound to the `AdwTabPage` properties:
| Part | Bound to | Notes |
|---|---|---|
| favicon (16 px) | `page.icon` (fallback `web-browser-symbolic`) | hidden while `loading` |
| spinner (16×16 `AdwSpinner`) | visible while `page.loading` | |
| title label | `page.title` | xalign 0, hexpand, ellipsize end. The row's tooltip is the title too |
| indicator (16 px, css `tab-indicator`) | `page.indicator-icon` (visible iff set), `indicator-tooltip` | primary click emits `indicator-activated` if `page.indicator_activatable` |
| close button | visible iff not pinned | `window-close-symbolic`, tooltip "Close Tab", flat circular `tab-close` |
| pin image | visible iff pinned | `view-pin-symbolic`, tooltip "Pinned Tab", dim |

Interactions:
- Selecting a row selects its page; selecting a page (anywhere) selects its row (`sync_selection`).
- Middle click (on release) closes the tab.
- Right click (pressed, claimed), or a touch long-press, opens the tab menu at the pointer.
  Shift+F10 or Menu opens it under the middle of the row. Opening it emits
  `AdwTabView::setup-menu(page)`, then shows a `PopoverMenu` built from the view's menu model,
  without an arrow, pointing at a 1×1 rectangle at the position.
- **Drag and drop reorder**: each row is a `DragSource` (MOVE) carrying the `AdwTabPage`, with a
  snapshot of the row as the drag icon (`WidgetPaintable`, hotspot 0,0). Each row is also a
  `DropTarget` for `AdwTabPage`. Dropping in the upper half puts the dragged page before the row,
  the lower half after it, using `reorder_target(dragged, own, below)` (→ `None` when nothing would
  move). It calls `AdwTabView.reorder_page`, which fires `page-reordered` and so settles the
  groups. A drop is refused if the dragged page is not in this view (`page_position < 0`), so **the
  sidebar reorders within its window only**. Moving between windows goes through the top tab bar,
  `chrome.tabs.move`, or Move to New Window. No drop-position indicator is drawn in the list
  (verified: no CSS or visuals for it in `tab_list.rs`).
- Unit tests for `reorder_target`: dragging down lands after the target row; up lands before or after;
  dropping next to the original slot is a no-op.

**Animations** (`motion.rs`: `TAB_ROW_MS = 180`, easing `EaseOutCubic`, via `AdwTimedAnimation`
and `GtkRevealer`, both skipping to the end when unmapped or when `gtk-enable-animations` is off):
- A new row grows in (revealer `SlideDown` over 180 ms plus an opacity fade 0→1), but only when the
  list is mapped. Rows added before the window shows, as on session restore, appear at once.
- A closed page's row stays as a "leaving" row: unselected, not selectable, activatable,
  targetable or focusable, with its header dropped. It shrinks (revealer hides) and fades to 0, then
  is removed and unbound. The tab count and closed-tab stack never wait on the animation.
- A page removed and re-added in one `items-changed` (a reorder) keeps its row, without animation.
- Unit test `tab_rows_follow_the_pages_and_only_animate_on_screen` and self-test `tab_animation`
  (row opacity < 1 right after opening when animations are on, 1.0 when settled; the row count drops
  at once on close with 1 leaving row; reopen works).

**Group presentation** (`show_groups(rows)`, driven by `BrowserWindow::redraw_groups` with core's
`TabGroups::rows(&tabs)` mapped to pages):
- Each group's header (`GroupHeader`) is the list-box header of the group's first row. Headers are
  kept across redraws by `GroupId`, so an editor open on one stays.
- Header = a flat button (css `tab-group-header`) holding the chip label (the group's title on its
  colour; an untitled group is a 12 px colour dot) and an arrow (`pan-end-symbolic` when collapsed,
  `pan-down-symbolic` when expanded; dimmed, end-aligned). Tooltip and accessible label = the
  group's `name()` (its title, or "{Colour} group"). Accessible state `Expanded(!collapsed)`.
- Header click (or Enter/Space, being a button) runs `tab-group.toggle(<id>)`. Right click, touch
  long-press, Shift+F10 or Menu runs `tab-group.edit(<id>)`.
- Every row in a group gets the class `tab-group-<colour>`, drawn as a **3 px inset bar on its left
  edge** (`box-shadow: inset 3px 0 <fill>`). Ungrouped rows lose the class.
- Rows of a collapsed group (`set_hidden(true, heads_group)`): the revealer collapses, class
  `tab-hidden` (zero margins), not selectable, activatable, focusable or targetable. They stay
  sensitive only for the group's first row, so keyboard focus can reach the header through it.
- Keyboard over hidden rows:
  - `ListRow::focus` override: a non-focusable row passes focus on in the direction of travel. On
    Tab/Down, to the next sensitive row's header, or else the row. On Shift+Tab/Up, to its own
    header, or else the previous sensitive row.
  - `move-cursor` override (`Rows::move_cursor`): arrows (`DisplayLines`), Home/End (`BufferEnds`)
    and Page Up/Down (`Pages`, by the adjustment's page increment, scrolling to match) only land on
    shown rows. A focused header counts as just above its row. Moving also selects the row unless
    the modifier (extend selection) is held. With nowhere to go the list rings the error bell.
  - Unit test `the_keyboard_never_lands_on_a_collapsed_groups_rows_in_the_tab_list` covers
    Down/Up/Home/End/PageDown/PageUp, Tab from a collapsed group's header past its rows, and
    Shift+Tab from the row after a collapsed group to its header.

Classification: all (ADW-BUILTIN) for widget mechanics and (KEEP) for behavior. The tab list itself
does not touch the engine.

---

## 12. Top tab bar (`AdwTabBar`, top layout)

What the old shell gets from `AdwTabBar` [inference: libadwaita behavior, not in repo code]:
horizontal tabs with title, favicon, loading spinner, indicator icon (clickable when activatable),
close button; pinned tabs compact at the start; drag to reorder; **drag between windows' tab bars**
(→ `page-attached`/`page-detached`); dragging a tab onto empty space emits `create-window` (handled:
new window of the same kind, §1); tab context menu from the view's menu model after `setup-menu`;
middle-click close. All (ADW-BUILTIN), to be rebuilt in GPUI.

Shell additions:
- `autohide(false)`: the bar shows even with one tab.
- End action widget: the flat **Search Tabs** button (`system-search-symbolic`, `win.search-tabs`,
  tooltip "Search Tabs"). Tab search anchors on it in the top layout (self-test `tab_search` checks
  this).
- Start action widget: the **group chips** (§15). Each is a flat button (`tab-group-button`) with the
  chip label. A click opens the **editor**; it does not collapse. Tooltip and accessible label =
  group name. Chips appear in group order, are reused by id, and the box is hidden when there are
  no groups.
- Grouped tabs show a **coloured dot** as their indicator when they have no capture or speaker
  indicator (`group_dot`, top layout only). The dot texture is 32×32 with radius 8 (half as wide as
  the 16 px indicator, drawn at 2× for HiDPI) in `GroupColor::rgb(dark)`, cached per colour.
  Tooltip = group name.
- `AdwTabBar` "can neither hide pages nor show headers", so collapsing a group **does not hide its
  tabs on the top bar**. Ctrl+Tab and the after-close pick still skip them, and selecting one
  expands its group (core settle). PLAN: "collapsing only hides tabs in the vertical list."
  [The GPUI shell can hide them; the Windows shell collapses tab items in place. Product decision.]

---

## 13. Tab context menu (`tab_menu.rs`)

The menu is one `gio::Menu` set as `AdwTabView.menu_model`. It is rebuilt per page in the view's
`setup-menu` handler (`setup_tab_menu(page)`), which also remembers the page the `tab.*` actions act
on. Both the top bar and the list rows open the same menu.

Facts (`TabFacts`) the menu depends on:
- `place: core::tab_place::TabPlace { index, count, pinned }` (page position, page count, pinned
  count).
- `vertical` (a sidebar layout), `muted`, `has_link` (`tab.link().is_some()`),
  `can_reopen` (`browser.can_reopen_closed_tab(browsing)`).
- `grouped` (the tab is in a group), `other_groups` (the window has a group the tab is not in; from
  `TabGroups::in_order` minus the tab's own).

Sections in Chrome's order (mnemonics as in code):

| # | Label (top / sidebar) | Action (`tab.*`) | Shown / enabled |
|---|---|---|---|
| 1 | "_New Tab to the Right" / "_New Tab Below" | `new-tab-next` | always |
| 1 | "_Add Tab to New Group" | `add-to-new-group` | shown when the window has no other group |
| 1 | "_Add Tab to Group ▸" submenu: "_New Group" (`tab.add-to-new-group`), then each other group by `name()` (`_` doubled), `tab.add-to-group(<uuid>)` | `add-to-group` (param `s`) | shown when there are other groups (pinned tabs too) |
| 1 | "Remove _From Group" | `remove-from-group` | only when grouped |
| 1 | "Move Tab to New _Window" | `move-to-new-window` | enabled iff `count > 1` (`TabPlace::can_move_out`) |
| 2 | "_Reload" | `reload` | always |
| 2 | "_Duplicate" | `duplicate` | always |
| 2 | "P_in Tab" / "Unp_in Tab" | `pin` / `unpin` | by pinned state |
| 2 | "_Mute Tab" / "Un_mute Tab" | `mute` / `unmute` | by muted state |
| 2 | "Copy _Link" | `copy-link` | only when the tab has a link |
| 3 | "_Close Tab" | `close` | always |
| 3 | "Close _Other Tabs" | `close-others` | enabled iff `closes_others()` is non-empty |
| 3 | "Close Tabs to the Ri_ght" / "Close Tabs _Below" | `close-after` | enabled iff `closes_after()` is non-empty |
| 3 | "R_eopen Closed Tab" | `reopen-closed` | enabled iff `can_reopen` |

What each does (`tab_action(page, action)`):
- **New tab next**: a new tab, adopted into the source tab's group, inserted at `max(index+1,
  pinned)`, selected, showing the new tab page.
- **Add to new group**: `group_tab(tab)`, i.e. core `new_group` (§15), then the editor opens.
- **Add to group (id)**: `add_tab_to_group(tab, id)`, i.e. core `join`.
- **Remove from group**: core `leave`.
- **Move to new window**: a new window of the same kind; `transfer_page(page, target, 0)`; select;
  `present()`. The tab keeps its state, pinned flag and browsing kind (self-test: still pinned; unit
  test: a private tab's new window is private, titled "New Tab (Private)").
- **Reload**: `Transition::Reload` and reload.
- **Duplicate**: a new tab adopted into the group at `index+1` (`insert_pinned` if the source is
  pinned), selected. `copy.restore(tab.web_view().session_state(), session_uri)` copies the
  back/forward history. With no session URI it shows the new tab page. (NEEDS-CEF, risk: no
  history restore in CEF, §10.)
- **Pin / Unpin**: `set_page_pinned`, schedule a session save. Grouped tabs: pinning removes them
  from the group via settle (core: "pinned tabs are in no group").
- **Mute / Unmute**: `set_is_muted`.
- **Copy link**: `copy_link(tab, clean=true)`. The tab menu always copies the clean link.
- **Close**: `close_page`.
- **Close others / Close after**: closes `place.closes_others()` / `place.closes_after()` (indices,
  last first; pinned tabs never close: `closes_others` = `(pinned..count).rev()` minus the tab;
  `closes_after` = `(max(index+1, pinned)..count).rev()`).
- **Reopen closed**: `browser.reopen_closed_tab(window)`.

Core contract: `vsesvit_core::tab_place::TabPlace::{is_pinned, closes_others, closes_after,
can_move_out}`. Unit tests in core and in `tab_menu.rs` pin the item order, the "Below" wording,
the toggles, Copy Link only for pages, the disabled states, and the group items. Self-test
`tab_menu` drives every item end to end (§26).

Windows-only items (PLAN; not in Linux): "Split view with ▸" / "Close split view".

Engine pieces: Reload is (NEEDS-CEF) `CefBrowser::Reload`; mute is (NEEDS-CEF) `SetAudioMuted`;
Duplicate's history copy is (NEEDS-CEF, risk); Move to New Window is (NEEDS-CEF, risk: browser
reparenting, §10).

---

## 14. Tab search (`tab_search.rs`)

Open/close: `win.search-tabs` (Ctrl+Shift+A, Chrome's key), the sidebar's Search Tabs button, and the
tab bar's Search Tabs button all call `toggle_tab_search()`. If open, it closes. Otherwise it builds
a `TabSearch` and opens it from the first *mapped* of: the sidebar's Search Tabs button (only while
the sidebar shows), the tab bar's Search Tabs button, the sidebar toggle; else (e.g. fullscreen with
the top bars hidden) at the top center of the page area (`Rectangle(width/2, 0, 1, 1)` on the toast
overlay). The popover's `closed` clears the window's reference.

UI:
- `GtkPopover` (position bottom), content width 360 (`WIDTH`).
- `GtkSearchEntry`, placeholder **"Search tabs"**. It takes the focus on open.
- `GtkListBox` (single selection, `navigation-sidebar`) in a scroller whose natural height grows to
  440 (`MAX_LIST_HEIGHT`) before scrolling.
- Placeholder label **"No results found"** (dimmed, margins 12) when there are no rows.
- Section headings are list-box headers: **"Open tabs"** over the first open row, **"Recently
  closed"** over the first closed row (css `heading`, xalign 0, margins 12/12/6/6).
- Row = `AdwActionRow`: title = core row title (1 line), subtitle = core row site (1 line),
  `use_markup(false)`, activatable. The prefix is a 16 px image: the tab's favicon texture
  (`browser.tab_icon(browsing, hit)`: an open tab's `web_view.favicon()`, a closed tab's stored
  favicon), else `web-browser-symbolic`.
- No close buttons on rows, no match highlighting, no times for closed tabs (PLAN).

Behavior:
- Every change of the entry text refreshes: `browser.search_tabs(window.browsing(), text)` builds
  `core::tab_search::Listed` for every open tab **of the same browsing kind in every window**
  (`key: TabId`, `title: display_title`, `url: session_uri or ""`, `used: tab.used()`) and for every
  closed tab in that kind's stack (`key: ClosedKey`, title, uri, `used`). It returns
  `core::tab_search::rows(query, open, closed)`. The first row is selected after each refresh.
- `core::tab_search::rows` rules: open rows before closed rows. Empty query: every tab, most recently
  used first. Otherwise only tabs whose lowercased title or URL contains the trimmed, lowercased
  query, ranked like omnibox bookmarks (`search::text_rank`: title prefix, then title, then address),
  most recently used first within a rank. Row `title` = the tab title, or its URL when the title is
  blank. Row `site` = the URL's host, or the whole URL when it has none (e.g. `file:`).
- MRU clock (`Browser::tick`): ticks when a tab is created (`Tab::wrap` → `tab_used`), selected
  (`tab_activated`), or its window becomes active (`is-active` → `tab_used(selected)`). A closed tab
  keeps the tick taken at close as its `used`.
- Up/Down (and keypad), captured on the entry, move the selection by ±1 with wraparound
  (`rem_euclid`) and scroll the row into view (`clamp_page`).
- Enter in the entry, or a click/activate on a row, runs `choose(row)`: closes the popover, then
  `browser.go_to_tab(window, hit)`:
  - `Hit::Open(id)`: find the tab in any window, `owner.select_tab(tab)`, `owner.present()`.
  - `Hit::Closed(key)`: `take(key)` from this window's kind's stack and `window.restore_closed(&closed)`
    in the **current** window (history kept in the old shell; §10 risk).
  - A tab gone since it was listed does nothing.
- Escape (`stop-search`) closes the popover. A second `win.search-tabs` also closes it.
- Private and normal windows see only their own kind's tabs and closed stack (self-test
  `private_window`: 0 rows from a normal window, 1 from the private one).

Self-test-only API: `set_query`, `shown()` (heading, title, site per row), `selected()`,
`says_no_results()`, `click(i)`, `press_enter()`, `is_open()`, `anchor()`; window
`tab_search()` and `tab_search_buttons()`.

Classification: (KEEP), with no engine involvement except favicons (§5).

---

## 15. Tab groups (`groups.rs` + core `tab_groups`)

### Core contract (`vsesvit_core::tab_groups`), the same for both shells

- `GroupId(Uuid)` (`new()`, `Display`/`FromStr` as the UUID string; used as the `s` action target).
- `GroupColor::{Grey, Blue, Red, Yellow, Green, Pink, Purple, Cyan, Orange}` (`ALL` in Chrome's
  order). `label()` gives "Grey", "Blue", .... `rgb(dark) -> [u8;3]` gives Chrome's light and dark
  shades (light: 5f6368, 1a73e8, d93025, f9ab00, 1e8e3e, d01884, 9334e6, 007b83, fa903e; dark:
  bdc1c6, 8ab4f8, f28b82, fdd663, 81c995, ff8bcb, c58af9, 78d9ec, fcad70). `next(used)` gives the
  least-used colour. An unknown colour deserializes as Grey.
- `TabGroup { id, title (empty until named), color, collapsed }`; `name()` gives the title, or
  "{Colour} group".
- `WindowTabs<K> { order: Vec<K> (every tab, hidden too), pinned: usize, active: Option<K> }`. The
  shell builds this fresh for every call (`window_tabs()`: tab ids in view order, the pinned count,
  the selected id).
- `Step<K>::{Unpin(K), Move(K, usize /*final index*/), Activate(K), OpenTab}`: what the shell
  applies after a group action.
- `Row<K>::{Header(TabGroup), Tab { tab, group, hidden }}`: the vertical list's lines.
- `TabGroups<K>` methods the shell calls: `restore(tabs_with_groups)`, `group_of(&k)`, `get(id)`,
  `in_order(&tabs)`, `members(&tabs, id)`, `new_group(&tabs, &k) -> (GroupId, Vec<Step>)`,
  `join(&tabs, &k, id)`, `leave(&tabs, &k)`, `set_collapsed(&tabs, id, bool)`,
  `next_shown(&tabs, back) -> Option<K>`, `after_closing(&tabs, &closing, opener) -> Option<K>`,
  `set_title(id, &str)`, `set_color(id, color)`, `ungroup(id)`, `adopt(k, &opener)`,
  `rejoin(&tabs, &k, id)`, `settle(&tabs, moved: Option<&K>)`, `rows(&tabs)`.
- `extension_group_id(Option<GroupId>) -> i32`: −1 or a positive fold of the UUID (old runtime's
  `chrome.tabs.Tab.groupId`). With Chromium's real extension system, `groupId` and
  `chrome.tabGroups` are Chromium's own, which only works if the shell's groups *are* Chromium's
  groups (NEEDS-CEF, risk; §27).
- Rules (core docs and PLAN): pinned tabs are in no group (grouping unpins); a group's tabs are
  contiguous; a group without tabs ends; a new group takes the least-used colour; Add to group moves
  the tab to the group's end and expands the group; Remove from group moves a middle tab to just
  after the group; collapsing the selected tab's group selects the nearest shown tab after it, else
  before it, else opens a new tab; selecting a hidden tab another way expands its group; a tab opened
  from a grouped tab joins the group; reopen puts a tab back into its group if the window still has
  it (stays if inside the group, else goes to the group's nearer end); settle: a tab dropped between
  two tabs of a group joins it, one dragged away leaves it, a tab moved to another window leaves its
  group, and a group split another way keeps its longest part. Settling twice is a no-op.

### Shell wiring

- One `TabGroups<TabId>` per window. `AdwTabView` keeps the order, core keeps membership.
- `settle_groups(moved)`: unless `groups_held`, calls `groups.settle(&window_tabs(), moved)`, then
  `redraw_groups()`. Called on selection change, pinned-count change, page attached (moved =
  tab), page reordered (moved = tab), page detached, and after `restore_groups`.
- `change_groups(f)`: runs the core mutation to get `steps`, sets `groups_held`, applies each step
  (`Unpin` → `set_page_pinned(false)`; `Move(id, to)` → `reorder_page(to)`; `Activate` →
  `set_selected_page`; `OpenTab` → `new_tab()`), clears the hold, settles, and schedules a
  session save.
- `redraw_groups()`: core `rows(&tabs)` and `in_order(&tabs)`; tab-list headers and row bars (§11);
  tab bar chips (§12); and `sync_indicator` for every page (the dots).
- `restore_groups(tabs: Vec<(TabId, Option<TabGroup>)>)`: `TabGroups::restore` then settle. Called by
  `session::restore` **after** the window's tab selection, because selecting a tab expands its
  group.
- `groups_for_menu(tab) -> (own id, other groups in order)` feeds the tab menu.
- `adopt_into_group(tab, opener)`: `groups.adopt(tab_id, &opener_id)`. Used by `insert_tab` with an
  opener (links opened in new tabs, `window.open` popups, view-source), New Tab Next, Duplicate and
  New Tab in Group.
- **Ctrl+Tab / Ctrl+Shift+Tab** (and Ctrl+PgDn / Ctrl+PgUp): `select_next_tab(back)` →
  `groups.next_shown(&tabs, back)`, which skips hidden tabs and wraps around.
- **Selection when the selected tab closes**: `select_after_closing(page)` computes the opener pick
  `by_parent` (a replica of `AdwTabView`'s own rule: the page before the closing one if it descends
  from the closing page's parent; else the parent if both are pinned), then
  `groups.after_closing(&tabs, &closing, opener)` (opener if shown; else the neighbour in its group,
  the one after first; else the nearest shown tab after, then before; else the neighbour, expanding
  its group), and selects it.

Actions (`tab-group.*`, each with the group id string as target):
| Action | Does |
|---|---|
| `toggle` | `set_collapsed(!collapsed)` through `change_groups` |
| `edit` | opens the group editor |
| `new-tab` | closes the editor; a new tab inserted right after the group's last member, adopted into the group, selected, showing the new tab page |
| `ungroup` | closes the editor; `groups.ungroup(id)`, no tab moves |
| `close` | closes the editor; closes each member page "the way each would close by itself" (each goes to the closed stack with its group) |

Other group mutations: `rename_group(id, title)` → `set_title`, redraw, save;
`recolor_group(id, color)` → `set_color`, redraw, save.

**Group editor** (`group_editor(group)`; one at a time, a new one replaces it):
- A popover (position bottom, css `tab-group-editor`), content width 300, spacing 12, margins 6.
- `GtkEntry` with the title, placeholder **"Name this group"**, accessible label "Group name". Each
  change renames live. Enter closes the popover.
- A row of 9 circular toggle-button swatches (24 px, class `tab-group-swatch tab-group-<colour>`),
  tooltip and accessible label = the colour label. The child is `object-select-symbolic`, visible
  only on the active swatch. They are radio-grouped, and activating one recolours live. The current
  colour is active at open.
- Separator, then flat left-aligned buttons **"New Tab in Group"**, **"Ungroup"**, **"Close Group"**.
- On map, focus goes to the name entry on idle ("as Chrome puts it in the name").
- Anchor: the group's header in the tab list if mapped, else its chip on the tab bar if mapped, else
  the top center of the page area.
- Opened automatically after "Add Tab to New Group".

**Colours in CSS** (`install_style(display)`, run at startup from `app.rs`): for each colour, generated
CSS gives `.tab-group-<c>.tab-group-chip` and `.tab-group-<c>.tab-group-swatch` a background of
`rgb(dark)`, with text white in light themes and `#202124` in dark, and `.tab-group-<c>.tab-row` the
3 px inset left bar. When `AdwStyleManager.dark` changes, the provider is reloaded and every
`BrowserWindow` redraws its groups (for the dot textures).

**Session**: each grouped tab's `TabSnapshot::group` carries a copy of its `TabGroup` (id, title,
colour, collapsed). `session::window_snapshot` writes `window.group_of(&tab)`. Restore rebuilds
the groups from the records (§15 shell wiring). Private windows have groups, which are never saved.

Self-test `tab_groups` and unit tests (`groups.rs`, `tab_menu.rs`, `session.rs`): see §26.

Windows differences (PLAN): header items in both the strip and the pane, colour along the tab's bottom
or leading edge, editor in a Flyout on the header's context menu.

Classification: (KEEP) entirely, except the Chromium-extension visibility of groups (NEEDS-CEF, risk).

---

## 16. Extension action buttons (`ext_actions.rs`) and extension hooks in the window

### Data flow
- `refresh_extension_actions()` runs on window creation, on `runtime.connect_actions_changed`
  (load, unload, `action.setBadgeText/setTitle/setIcon/setPopup`), after pin and move changes in any
  window, after extension list changes (`extensions.rs`), and after synced pref changes:
  1. `browser.extension_actions()`: `runtime.actions()` (`ActionInfo { extension, title, icon:
     Option<PathBuf>, has_popup, badge_text }`) sorted by install order
     (`profile.extensions().list()` position; unknown ones last, stable).
  2. Filter `runtime.runs_in(ext, window.browsing())`, so a private window shows only extensions
     allowed in private windows (synced pref `extensions.allowed_in_private`).
  3. `available` = those ids. `browser.extension_toolbar(&available)` =
     `core::extensions::toolbar::layout(available, prefs.get(&toolbar::TOOLBAR))` → `Layout { pinned,
     unpinned }`.
  4. `ExtensionActions::rebuild(actions, pinned, on_click)`.
- Pref `toolbar.extensions` (`toolbar::TOOLBAR`, synced, `Vec<Entry { id, pinned }>`): an
  extension with no entry is **pinned**, appended in install order; entries of extensions this
  device lacks are kept; repeated ids count once.
  - `toolbar::set_pinned(available, saved, id, pinned)`: an unpinned action keeps its place, so
    pinning it again restores it.
  - `toolbar::move_pinned(available, saved, id, to)`: reorders among the shown pinned actions only.
  - `Browser::pin_extension` / `move_extension` → `change_toolbar`: writes the pref, then every
    window runs `refresh_extension_actions()`.

### UI
- Container in `header_end`: `[pinned buttons box][puzzle button]`.
- **Puzzle button**: `application-x-addon-symbolic`, tooltip **"Extensions"**, visible only while at
  least one action is available.
- **Pinned button** (flat): an overlay of the action icon (16 px; texture from the icon file, else
  `application-x-addon-symbolic`) and a **badge** label (the `badge_text`, hidden when empty,
  bottom-right, not targetable, css `extension-badge` = accent colours). Tooltip = action title.
  Buttons are reused per extension and updated in place when `ActionInfo` changes, so an open popup
  survives badge, title and icon changes. Order follows `pinned`.
  - Gaps vs Chrome [verified absent in this code]: no `setBadgeBackgroundColor` or text colour (always
    accent colours); no per-tab action state (badge, title, icon or enabled state per tab; `ActionInfo`
    is global); no disabled/greyed action state; no action icon `ImageData`. With Chromium these come
    from the extension system itself, but a shell-drawn toolbar would need them exposed (NEEDS-CEF,
    risk).
- **Click** on a pinned button: `window.activate_extension_action(id)` →
  `runtime.activate_action(id, selected_tab_id, show)`, which grants `activeTab` on the tab. If the
  action has a popup, the runtime creates the popup web view and calls `show(view)`. Otherwise it
  fires `action.onClicked(tab)`.
- **Popup** (`show_popup(id, view)`): closes any other popup first (one at a time). Shows `view` in a
  `GtkPopover` (position bottom) anchored to the extension's pinned button, or to the **puzzle
  button** when it isn't pinned. Initial size 320×200 (`POPUP_INITIAL`). On each `load-changed ==
  Finished` it runs JS `JSON.stringify([document.documentElement.scrollWidth,
  document.documentElement.scrollHeight])` (2 s timeout) and sets the size clamped to
  **200..800 × 80..600** (`POPUP_MIN` / `POPUP_MAX`, "as Chrome does"). `window.close()` in the
  popup (the web view's `close` signal) closes it. Dismissal (click outside, Escape) drops the view.
  A rebuild closes the popup only if its extension is no longer loaded. Teardown is deferred to idle
  (GTK popover quirk).
  - (NEEDS-CEF) A popup is an extension page, `chrome-extension://<id>/<popup>`. In CEF it would be a
    CefBrowser in a GPUI popover-like surface, with auto-size from `CefBrowserHost::SetAutoResizeEnabled(min,
    max)` + `CefDisplayHandler::OnAutoResize` *(both in bindings)* instead of the JS measurement
    (WEBKIT-ONLY). CEF has no public API to *open an extension's action popup* or to read action
    state; with Chrome-style browsers, Chromium's own toolbar would do it (§27). **Risk.**
- **Drag to reorder pinned buttons**: `DragSource` (MOVE) carrying a boxed `Dragged(id)`, with the
  button snapshot as icon at the press point. Every pinned button is a `DropTarget`. Before or after
  is decided by x vs half width, mirrored for RTL. `drop_index(from, target, after)` (unit tested) →
  `win.extension-move(id, to)`.
- **Right-click on a pinned button** (`action_menu(button, id, (x, y))`): a `PopoverMenu` (no arrow,
  at the pointer) whose first sections are the extension's own `contextMenus` items for the
  `action` context (`runtime.action_menu(id)` → `extension_menus::model`: sections split at
  separators; checkbox and radio items as stateful actions; disabled items and submenus greyed;
  `_` escaped), then a section with **"_Unpin"** (`win.extension-pin((id,false))`) and **"_Manage
  Extensions"** (`win.show-extensions`). Choosing an extension item →
  `window.extension_menu_item_chosen(id, item)` → `runtime.menu_clicked(id, item, selected_tab,
  None)` (grants activeTab, fires `contextMenus.onClicked` with no page URL).
- **Puzzle (Extensions) menu** (`open_menu`): a popover with heading **"Extensions"**, then one row per
  available action in install order:
  - an open button (flat, hexpand) with the icon and the title (ellipsized, max 32 chars). It closes
    the menu and activates the action; the popup anchors to the pinned button if pinned, else the
    puzzle.
  - a pin toggle (`view-pin-symbolic`, tooltip **"Unpin from Toolbar"** / **"Pin to Toolbar"**,
    class `extension-pinned` vs `dim-label`) → `win.extension-pin((id, !pinned))`.
  - then a separator and **"Manage Extensions"** (`win.show-extensions`; closes the menu).
  - The menu refills live while open when actions change.
- **Extension keyboard commands**: `keymap::apply` binds each resolved extension command
  (`core::extensions::commands`, rules in PLAN "Extensions in detail") to
  `win.extension-command(('<id>','<name>'))`. `run_extension_command(id, name)`: if `name` is in
  `core::extensions::manifest::ACTION_COMMANDS` (`_execute_action`, `_execute_browser_action`,
  `_execute_page_action`), it does what a click on the action does (popup or onClicked); otherwise
  `runtime.command(id, name, selected_tab)` (fires `commands.onCommand`, grants activeTab).
  (NEEDS-CEF: with Chromium extensions, command dispatch is Chromium's. Whether CEF Chrome-style
  dispatches `commands` while a shell-drawn window has focus is unknown. Open question.)

Self-test: `popup`, `extension_toolbar`, `context_menus`, `extension_commands` (§26).

Windows differences (PLAN): draws the same buttons with WebView2 popups in flyouts; no action
context-menu items; only action shortcuts are bound; private windows show no extension buttons.

---

## 17. Private windows (window-level behavior)

Opened by `app.new-private-window` (Ctrl+Shift+N; the main menu's "New Private Window"), a link
context-menu item "Open Link in Private Window" (`page_menu::add_private_window_item`, right after
Open Link in New Window), or `chrome.windows.create({incognito:true})` from allowed contexts.
Command lines, extensions and sync open pages in a normal window (`Browser::normal_window()`).

Window-side differences:
- Class `private`: dark header and top bars (CSS in §2); a "Private browsing" icon
  (`user-not-tracked-symbolic`) after the navigation buttons; the title gets " (Private)" (§3).
- No profile button (§7).
- The new tab page is `core::new_tab::PRIVATE_PAGE` (heading "You're browsing privately"; no search
  box, no tiles).
- Extension buttons only for extensions allowed in private (§16).
- Tab search, Reopen Closed Tab and the tab menu's Reopen Closed Tab use the private closed stack
  (`private_closed_tabs`), and tab search lists only private tabs (§14).
- Downloads button shown if a download started in a private window this session (§22).
- Session: private tabs are skipped in `session::snapshot`. A save with no normal window open writes
  nothing (`save_session_now`: returns if `windows_of(Normal)` is empty).
- Tab kind is fixed. Move to New Window keeps the kind. A tab dropped from a window of the other kind
  is sent back to a window of its own kind (§21).
- Every tab of a private window lives in the private engine session: `Browser::network_session(Private)`
  makes one ephemeral `webkit::NetworkSession` for the first private tab and keeps it until the last
  private window is removed. Then `end_private_session()` cancels running private downloads,
  disconnects the session, clears `private_closed_tabs`, and calls
  `core::Profile::end_private_session()` (forgets private site choices, zoom and download rows).
  (NEEDS-CEF) One off-the-record `CefRequestContext` via `request_context_create_context` with an
  empty `cache_path` [inference: CEF in-memory context = incognito] *(function in bindings)*,
  shared by all private browsers and dropped at the end.

Self-test `private_window` covers these (§26).

---

## 18. Fullscreen

- `win.fullscreen` (F11) toggles window fullscreen. While fullscreen, every top bar is hidden
  (`reveal_top_bars=false`). The split view's sidebar is content, not a top bar, so it stays as it was
  [verified from structure].
- A page entering fullscreen (WebKit `enter-fullscreen`, `tab.rs`) calls
  `window.show_fullscreen_notice(site)`. `site` = the committed URI's origin `host_for_display()`,
  or "This page". The notice is an OSD label (`osd fullscreen-notice`), centered, top margin 24,
  crossfade, not targetable, with text **"{site} is now full screen\nPress Esc to exit full
  screen"**. It hides after 4 s (`FULLSCREEN_NOTICE_TIME`); a new notice restarts the timer.
  `leave-fullscreen` and leaving window fullscreen hide it. Rationale in code: "with the header
  hidden, a page could otherwise draw a fake one (Chrome warns the same)".
- Permission prompts are withdrawn while fullscreen (§19).
- Unit test `a_page_gone_full_screen_is_named_with_the_way_out`.
- (NEEDS-CEF) `CefDisplayHandler::OnFullscreenModeChange(fullscreen)` *(in bindings)*. With Alloy or
  windowless style the client drives the window transition itself; with Chrome style + Views it is
  automatic (per the binding's doc comment). Esc to exit page fullscreen:
  `CefBrowserHost::ExitFullscreen` [inference: CEF ≥ 120 has it; not checked].

---

## 19. Permission prompt orchestration (the window's part)

The permission model and prompt content belong to the permissions inventory. The window owns *which*
prompt shows *when*:
- `sync_permission_prompt()` shows the **selected tab's oldest waiting request**
  (`permissions::next_prompt(browser, tab)`), anchored in the address bar (`address.show_prompt`).
  Other tabs' prompts wait until their tab is selected. None shows while another address-bar bubble
  is open (`address.bubble().is_some()`) or the window is fullscreen. If the same request with the
  same asked permissions is already on screen, nothing changes. Otherwise the shown prompt is
  withdrawn (popdown, *not* answered) and the next one shows.
- It is called on selection change, commit of the selected tab, address-bar bubble changes,
  fullscreen changes, and from `permissions.rs` when requests arrive or end.
- Answer buttons call `answer_permission_prompt(request, answer)` → `permissions::answer(...)`, then
  `permissions::enforce(browser)`.
- A close the window did not make (Escape, or a click elsewhere that left the tab and address bar
  unchanged) counts as **Dismiss** ("Not now"), followed by `enforce` on idle.
- `clicked_while_prompting()`: after any click in the window, on idle, if the prompt is still shown,
  the selected tab is unchanged, and no other bubble opened, the prompt is popped down (and so
  dismissed). A tab switch or another bubble withdraws it instead, to come back later.
- Answers (from self-test `capture_in_use`): "Allow while visiting", "Allow this time", "Never allow",
  "Dismiss" (`core::permissions::Answer::label`); heading e.g. "Use your camera and microphone?";
  buttons ignore activation for 500 ms (`PROMPT_GUARD`).
- (NEEDS-CEF) Requests come from `CefPermissionHandler::OnRequestMediaAccessPermission` /
  `OnShowPermissionPrompt` [inference; names from CEF docs, not checked in bindings].

---

## 20. `window.open` / `target=_blank` popups

- `Tab::create_related()` (WebKit `create`) makes `Tab::new_related(browser, opener)`: the same kind
  and network session, a related view, and the opener's navigation gate. `window.adopt_popup(opener,
  popup)` keeps it in `popups` until WebKit emits `ready-to-show`. Then it is inserted as a
  **foreground tab next to the opener** (`insert_tab(popup, Some(opener), Foreground)`: the runtime
  learns the opener; the popup joins the opener's group). If the page closes it before it shows (the
  `close` signal), it is discarded (`tab_discarded`). Pending popups are discarded when the window
  closes. Unit test `a_popup_dropped_before_it_shows_leaves_the_extension_runtime`.
- `window.open` popups always become tabs. There is no separate popup window type (PLAN says
  extension `popup`/`panel` windows also open as normal browser windows).
- Classification: the two-phase create/ready-to-show is (WEBKIT-ONLY). The behavior (popup → tab next
  to the opener, same group, same session, opener relation kept for `window.opener`) is (KEEP).
  (NEEDS-CEF) `CefLifeSpanHandler::OnBeforePopup(target_url, target_disposition, user_gesture, ...)`
  *(in bindings)*: either let CEF create the popup browser with the shell's client and window info,
  then adopt it in `OnAfterCreated` *(in bindings)*, or cancel and open a tab (which loses
  `window.opener`). `DoClose` / `OnBeforeClose` *(in bindings)* handle `window.close()`.

---

## 21. Tabs across windows: drag, send-back, new windows

- Dragging a tab between windows (top tab bar) or onto nothing (→ `create-window`) uses libadwaita's
  page transfer (ADW-BUILTIN); the live web view moves (§10 risk).
- `send_back(page)`: a page attached to a window of the other browsing kind (its view keeps its
  network session) is moved, on idle once the drop is done, to the most recently focused window of
  its own kind that has tabs, or else to a new window of its kind (chosen *before* the source window
  may close, which would end the private session). It is selected there and the window is presented.
  If the tab vanished meanwhile, a window made for it closes again if empty. Unit test
  `a_tab_dropped_into_a_window_of_the_other_kind_goes_back_to_its_own`. The constraint carries over to
  CEF: a browser's `CefRequestContext` is fixed at creation [inference] (KEEP).
- `chrome.windows.create` / `update` / `remove`, `tabs.create/update/move/remove` act on windows
  through the runtime's `TabHost` implemented in `browser.rs` (`Host`): `open_tab_at`, `select_tab`,
  `move_tab`, `close_tab`, `BrowserWindow::with_browsing` + `set_default_size` + state (normal,
  minimized, maximized, fullscreen) + `present()` or `set_visible`. Window info reports `focused`,
  state, size (Wayland gives no position, so the origin). (NEEDS-CEF, risk; §27.)

---

## 22. Other window surfaces the window hosts (owned by other inventories)

- **Downloads button**: `show_downloads_button()` (called by `downloads.rs` when a download starts,
  and at window creation if one started this session for this kind). `warn_about_download(download)`
  shows Chrome's dangerous-file warning popover (`crate::downloads::warning`, Keep and Discard)
  under the downloads button.
- **Update banner**: `set_update_banner(Option<&Banner { title, button }>)` reveals the `AdwBanner`
  with the title and button label (action `app.update`). Driven by `updates::Updates` (lifecycle
  states Ready, Installing (foreground), Installed (not exit-now)).
- **Bookmarks bar**: `refresh_bookmarks_bar()` (`BookmarksBar::refresh(core)`),
  `set_bookmarks_bar_visible(bool)` (reveals it and sets `win.show-bookmarks-bar`'s state). Pref
  `bookmarks_bar.visible` (synced, default true).
- **Star and bookmark bubble**: `sync_star()`, `show_bookmark_bubble(node, added)` →
  `bookmark_editor::bubble` on `Anchor::Star`.
- **Site info**: `show_site_info()` → `site_info::popover(Connection::of(tab), sections)` with sections
  from `trackers::site_info_section`, `cookies::site_info_section`, `permissions::site_info_section`
  (each optional), anchored at `Anchor::Security`.
- **Address bar**: full-URL mode from pref `address_bar.full_urls` (`set_full_urls`); omnibox
  wiring (§1).
- **Find bar**: retargeted on selection change; `find`, `find-next`, `find-previous`.
- **Toasts**: `toast(adw::Toast)` on the window's `AdwToastOverlay` ("Link copied", "Could not open
  the profile: …", "Cannot bookmark the page: …", "Cannot resolve the address: …").

---

## 23. Split view and media player

- **Linux has no split view.** `AdwOverlaySplitView` in this code is the tab sidebar, not a
  page split. WinUI has a split view (tab menu "Split view with ▸", "Close split view", a resizable
  divider; `vsesvit-winui/src/window/tab_actions.rs`, `tab_menu.rs`, `chrome.rs` `SplitDivider`).
  Memory Saver counts "in a split view" as on screen (PLAN). Whether the new Linux shell adds a split
  view is a product decision.
- **Linux has no media player.** Pref `media.player` (`keys::SHOW_MEDIA_PLAYER`, "the media player at
  the foot of the vertical tab list") and picture-in-picture are used only by `vsesvit-winui`
  (`window/mod.rs` `media player`, `automation/pip_steps.rs`), not by `vsesvit-gtk`.

---

## 24. Persistent state the window reads or writes

| Key / store | Scope | Default | Read / written by the window |
|---|---|---|---|
| `tabs.position` (`keys::TABS_POSITION`) | synced | `left` | read: `apply_layout`; written: Settings, `Browser::set_tabs_position` |
| `bookmarks_bar.visible` (`keys::SHOW_BOOKMARKS_BAR`) | synced | `true` | read on create; written by `win.show-bookmarks-bar` |
| `toolbar.home_button` (`keys::SHOW_HOME_BUTTON`) | synced | `false` | Home button visibility |
| `address_bar.compact` (`keys::COMPACT_ADDRESS_BAR`) | synced | `true` | address bar max width 720 |
| `address_bar.full_urls` (`keys::SHOW_FULL_URLS`) | synced | `false` | address bar display mode |
| `homepage` (`keys::HOMEPAGE`) | synced | `about:home` | `go_home` via `prefs::homepage_url` |
| `toolbar.extensions` (`extensions::toolbar::TOOLBAR`) | synced | `[]` | pinned extension actions and their order |
| `extensions.allowed_in_private` (`extensions::private::ALLOWED_IN_PRIVATE`) | synced | `{}` | which actions a private window shows (through `runtime.runs_in`) |
| `keyboard.shortcuts` (`keys::SHORTCUTS`) | synced | `{}` | accelerators of all `win.*`/`app.*` actions and extension commands |
| `theme` (`keys::THEME`) | synced | `System` | dark or light drives group colours and dots |
| Session (`core.session().save(SessionSnapshot)`) | synced (Tabs type) | | per normal window: tabs (`TabSnapshot { id, url, title, pinned, last_active_ms, group, restore_state }`), `active_tab`, `bounds` (0,0,w,h from default size), `maximized`; windows ordered most recently focused first; private tabs never. Debounced 2 s (`SESSION_SAVE_DELAY`) after changes, immediately on last-normal-window close, last tab close and quit |
| `profiles/profiles.json` (`core::profiles` `ProfilesDir`/`Registry`) | per install | | read on window activation and profile-menu open; `title_name`, names, colours, pictures |
| Closed-tab stacks | memory only | | 25 per kind; private stack cleared when the private session ends |
| Per-site zoom | DB (normal) / memory (private) | | written on zoom change (§5) |
| History visits and titles, favicons | DB, normal tabs only | | via `navigation_committed`, `title_changed`, `favicon_changed` |

---

## 25. Core / sync / update API contract called from the window code

| API | Used for |
|---|---|
| `vsesvit_core::private::Browsing` | window and tab kind |
| `vsesvit_core::prefs::{TabsPosition, keys::*, homepage_url}` | layout, bars, home |
| `vsesvit_core::profiles::{with_profile, Registry::{get, profiles, title_name}, ProfileColor, ProfileId::parse, avatar_initial, ProfilesDir::{opened, picture, root, add, ...}}` | titles, profile button and menu |
| `vsesvit_core::tab_place::TabPlace` | tab menu enablement and close-others/after |
| `vsesvit_core::tab_search::{rows, Listed, Hit, Row}` | tab search |
| `vsesvit_core::tab_groups::{TabGroups, TabGroup, GroupId, GroupColor, WindowTabs, Step, Row, extension_group_id}` | groups (§15) |
| `vsesvit_core::new_tab::{page, PRIVATE_PAGE}` | new tab pages |
| `vsesvit_core::clean_url::clean` | Copy Link without tracking |
| `vsesvit_core::history::Transition::{Link, Typed, Bookmark, Reload}` | pending transitions for history |
| `vsesvit_core::permissions::{Answer, Permission, Capturing::description}` | prompts, indicators |
| `vsesvit_core::extensions::{ExtensionId, toolbar::{TOOLBAR, layout, set_pinned, move_pinned, Entry, Layout}, manifest::ACTION_COMMANDS, commands::*}` | extension toolbar and commands |
| `vsesvit_core::shortcuts::{Command, Keymap}` (through `keymap.rs`) | accelerators |
| `vsesvit_core::session::{SessionSnapshot, WindowSnapshot, TabSnapshot}` | session save and restore incl. groups |
| `vsesvit_core::bookmarks::BookmarkNode`, `Bookmarks::{is_bookmarked, find_by_url, add_url}` | star |
| `vsesvit_core::zoom::DEFAULT` | zoom reset |
| `vsesvit_core::downloads::Download` (+ `started_this_session` in the shell's Downloads) | downloads button and warning |
| `vsesvit_core::Profile::end_private_session` | when the last private window closes |
| `vsesvit_core::sync::Changed` (`Browser::sync_applied`) | relayout windows after synced prefs |
| `vsesvit_sync` | only indirectly: `Syncer::window_opened` (first-window scheduling) |
| `vsesvit_update` | only indirectly: `Updates::window_opened` and the banner (`updates/lifecycle.rs` `Banner`) |
| `vsesvit_webext::{Runtime, TabId, ActionInfo, menus::ItemId}` | **to be replaced**: `windows_changed`, `runs_in`, `activate_action`, `command`, `menu_clicked`, `action_menu`, `actions`, `connect_actions_changed`, `tab_opened_by`, `tab_attached`, `tab_moved`, `tab_activated`, `tab_updated`, `tab_closed`, `user_content_manager` |

---

## 26. Interactions with the self-test (`self_test.rs`, `docs/design/self-test.md`)

Self-test-only accessors on `BrowserWindow`/`TabList` (all `#[cfg(feature = "self-test")]` or
`cfg(test)`): `layout_probe`, `classify_layout`, `bookmarks_bar`, `shows_downloads_button`,
`download_warning`, `home_button`, `tab_indicator(tab) -> (icon name, tooltip)`, `tab_look(tab) ->
(title, icon)`, `click_tab_indicator`, `tab_row_buttons -> (close visible, pin visible)`,
`tab_row_opacity`, `tab_list().row_counts() -> (live, leaving, settled)`, `tab_search()`,
`tab_search_buttons() -> (sidebar button, tab bar button)`, `open_tab_menu(tab)`, `tab_menu_lines()
-> sections of (label, enabled)`, `group_editor_open`, `group_header(id)`, `tab_row_hidden(tab)`,
`select_next_tab(back)`, `extension_action_button(id)`, `extension_popup_view`,
`close_extension_popup`, `open_extension_context_menu(id)`, `open_extensions_menu`,
`shows_profile_picture`, `profile_menu`. `crate::popup` turns off autohide under the self-test
(`SCRIPTED`), because Wayland refuses popup grabs without real input.

Checks that exercise the window (a GPUI + CEF shell needs equivalents):
- `tabs`: open, switch, close.
- `tab_animation`: rows animate without delaying the tab count (Linux only).
- `tab_layout`: geometry-classified left, right, top, left.
- `tab_menu`: every item. Exact labels and enablement by section in the sidebar ("Below" wording);
  Copy Link clean vs whole; Duplicate placement and URL; Pin (row shows pin, session pinned, menu
  offers Unpin; screenshot `tab-pinned.png`); Mute (indicator, click unmutes); Close Other Tabs keeps
  pinned; Reopen; Close Tabs Below; New Tab Below placement and blank; Move to New Window (alone and
  still pinned). Screenshots `tab-menu.png`.
- `tab_search`: Ctrl+Shift+A binding, anchor on the sidebar button, ranking (title matches before
  address matches), headings at rows 0 and 3, Down/Up wrap, empty query MRU, "No results found",
  click switches to the other window and closes, closed tab listed under Recently closed and
  reopened by Enter here, toggle closes, anchor on the tab bar button in top layout
  (`tab-search.png`).
- `tab_groups`: menu offers Add Tab to New Group; editor opens (`tab-group-editor.png`); header
  click collapses (row hidden, header stays, first tab selected; `tab-group-collapsed.png`) and
  expands; Ctrl+Tab skips the collapsed tab; closing the selected last tab selects the first; a
  tab opened from a grouped tab joins; closed and reopened, it rejoins after the grouped tab; the
  session carries the group; Ungroup clears the header and the session group.
- `memory_saver`: faded icon and kept title in the tab (`tab-asleep.png`).
- `popup`, `extension_toolbar` (context menu screenshot, puzzle menu screenshot, unpin via the menu
  writes `toolbar.extensions`, unpinned popup anchors to the puzzle, re-pin), `context_menus`
  (the action menu starts with the probe's action item), `extension_commands` (`_execute_action`
  opens the popup; named command fires onCommand).
- `private_window`: Ctrl+Shift+N binding, `private` class, title "Vsesvit fixture 2 (Private)",
  tab search isolation, private new tab page, closed stack kept then dropped (`private-window.png`).
- `settings`: home button shown and working (`home-button.png`).
- `ctrl_s_toggles_sidebar`.
- `capture_in_use`: tab indicator camera icon in sidebar and top (`capture-in-use*.png`).
- `profiles`: menu names, titles with the profile name, private title, no profile button in private,
  account picture on the button.
- `download`: the downloads button shows.

---

## 27. CEF facts checked on disk that affect this inventory

From `~/.cargo/registry/src/index.crates.io-*/cef-dll-sys-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs`
and the `cef` crate's bindings:
- Present: `cef_browser_host_t::{set_audio_muted, is_audio_muted, show_dev_tools, close_dev_tools,
  has_dev_tools, execute_dev_tools_method, get_navigation_entries, print, get_zoom_level,
  set_zoom_level, can_zoom, zoom (cef_zoom_command_t), set_auto_resize_enabled, download_image,
  can_execute_chrome_command, execute_chrome_command, get_runtime_style}`;
  `cef_display_handler_t::{on_address_change, on_title_change, on_favicon_urlchange,
  on_fullscreen_mode_change, on_tooltip, on_status_message, on_console_message, on_auto_resize,
  on_loading_progress_change, on_cursor_change, on_media_access_change(has_video_access,
  has_audio_access), on_contents_bounds_change, get_root_window_screen_rect}`;
  `cef_load_handler_t::on_loading_state_change`; `cef_life_span_handler_t::{on_before_popup,
  on_before_popup_aborted, on_after_created, do_close, on_before_close}`; `cef_audio_handler_t`
  (stream capture); `cef_command_handler_t::{on_chrome_command, is_chrome_app_menu_item_visible,
  is_chrome_app_menu_item_enabled, is_chrome_page_action_icon_visible, is_chrome_toolbar_button_visible}`;
  `cef_runtime_style_t::{DEFAULT, CHROME, ALLOY}` (Alloy is forced when windowless rendering is on, per its doc);
  `cef_chrome_toolbar_type_t::{NONE, NORMAL, LOCATION}` (BrowserView delegate `get_chrome_toolbar_type`);
  `cef_chrome_toolbar_button_type_t` lists only CAST/DOWNLOAD/SEND_TAB/SIDE_PANEL/TAB_SEARCH (all
  `_DEPRECATED`), MEDIA, BATTERY_SAVER, AVATAR, with **no extensions button type**;
  `request_context_create_context`, `request_context_get_global_context`,
  `browser_host_create_browser(_sync)`.
- **Absent** (searched): any `_cef_extension_t` / `load_extension` / `get_extensions` (the old Alloy
  extension API is gone); any "is audible" or audio-state getter; any setter that restores navigation
  entries.
- Consequences for this inventory [inference]:
  1. Extension action buttons, popups, the puzzle menu, `chrome.tabs`/`windows`/`tabGroups`
     visibility of shell-drawn tabs, and command dispatch have no public CEF API when the shell
     draws its own toolbar (Chrome style with `CEF_CTT_NONE`, or Alloy). Chromium's own toolbar
     (`CEF_CTT_NORMAL`) in a Views-hosted Chrome-style browser would bring Chrome's extension UI
     but not GPUI chrome. This is the core conflict between "GPUI for all chrome" and "total
     extension compatibility", and has to be settled before implementing §16.
  2. Duplicate, Reopen closed tab, tab-search reopen and session restore lose back/forward history
     unless a way to restore it is found.
  3. The "playing audio" speaker indicator needs a strategy.
  4. Cross-window tab moves need browser reparenting or off-screen rendering.

---

## 28. Gaps and quirks noticed (keep or fix deliberately)

- Linux leaves Ctrl+1..8 / Ctrl+9 (core `SelectTab1..8`, `SelectLastTab`) unbound, though core's
  defaults and Windows have them. Alt+digits come from `AdwTabView` [inference].
- The sidebar drag-reorder works only within a window and draws no drop indicator. Cross-window drag
  works only on the top bar.
- On the top bar a collapsed group's tabs stay visible (libadwaita limitation, not a product rule).
- Extension badges ignore the badge colours; action state is not per tab; there is no action
  enabled/disabled state.
- The tab group chip on the top bar opens the editor and cannot collapse; the sidebar header collapses
  on click and edits on right-click.
- `open_tab_at` (extension `tabs.create` with `index`) clamps only to the page count, not to after
  the pinned tabs [verified in code; how `AdwTabView.insert` treats an index inside the pinned
  block was not checked].
- JavaScript Console cannot open the console panel (WebKit limit). With CEF this can be fixed.
- `display_uri` relies on WebKit's `uri_for_display`.

---

## 29. Checklist

- [ ] Window object per profile process with a fixed browsing kind (normal/private), extension-visible window id from 1, default size 1280×820
- [ ] Window creation sequence: layout prefs, bookmarks bar, extension actions, profile button, downloads button if a download started this session (per kind), update banner, first-window sync scheduling, extension windows.onCreated
- [ ] Close: last normal window saves the session if it has tabs; every tab and pending popup leaves the extension system; bounds timer cancelled
- [ ] Window closes itself when its last tab leaves (not mid-transfer, not mid-close)
- [ ] Last private window closing ends the private session (downloads cancelled, private closed stack cleared, `Profile::end_private_session`, off-the-record context dropped)
- [ ] Window activation: MRU tick for the selected tab, `profile_used()` (re-read profiles.json, quit if removed), extension focus events
- [ ] Resize reported to extensions after a 250 ms settle; minimize/maximize/fullscreen state changes reported
- [ ] Header layout: Back, Forward, Reload/Stop, Home (pref), sidebar toggle (left), private icon, address bar, extension actions + puzzle, New Tab, Downloads (hidden), Profile, Main menu, sidebar toggle last (right)
- [ ] Top bars order: header, update banner, top tab bar (top layout), bookmarks bar, find bar; all hidden in fullscreen
- [ ] Compact address bar (max 720 px, centered) vs full width, from `address_bar.compact`
- [ ] Full-URL mode from `address_bar.full_urls`
- [ ] Private window styling: dark top bars, "Private browsing" icon, " (Private)" title suffix, no profile button
- [ ] Window title = page title [+ " - <profile>" when ≥2 profiles] [+ " (Private)"]; "Vsesvit" before any tab; "New Tab" for blank tabs; display-decoded URL when untitled
- [ ] Tab layouts left/right/top from synced `tabs.position`, applied live to all windows, including on synced change
- [ ] Sidebar 180–300 px (20%); collapses to an overlay below 720sp; toggle button "Show Tabs"; Ctrl+S/F9 toggles (sidebar layouts only)
- [ ] Layout probe and geometry classification for the self-test `tab_layout`
- [ ] Reload button turns into Stop while loading; Back/Forward enabled from history
- [ ] Address bar follows the selected tab: URL, security, progress, zoom indicator, star, in-use indicator; unsubmitted text kept per tab across switches
- [ ] Focus on switch: address bar if typed text pending or tab blank, else page
- [ ] Selecting a tab wakes a sleeping tab and records `last_active_ms`
- [ ] Main menu with five sections and the zoom row (−, "100%" reset, +) whose label follows the tab
- [ ] Profile button: avatar (account picture or coloured initial), tooltip name, menu of profiles (current checked), Add Profile…, Manage Profiles…; launch other profiles with an activation token; error toast
- [ ] All `win.*` actions from §8 with keymap-driven, reassignable accelerators; `app.new-window`, `app.new-private-window`, `app.shortcuts`, `app.welcome`, `app.about`, `app.update`, `app.quit` (saves the session first)
- [ ] Built-in Ctrl+Tab/Ctrl+PgDn and Ctrl+Shift+Tab/Ctrl+PgUp skipping hidden tabs with wraparound
- [ ] Decide on Ctrl+1..9 (core defaults) and other tab-view key bindings (move tab, first/last)
- [ ] New tab page loaded as HTML that still reads as blank (about:blank, "New Tab", not saved); private windows get the private page
- [ ] Opening tabs: next to the opener (Chrome placement), joins the opener's group, opener reported; middle/Ctrl+click background, +Shift foreground
- [ ] `chrome.tabs.create` with index; `open_tabs` (first foreground)
- [ ] Home button: homepage URL as a Bookmark-transition visit, or the new tab page in the selected tab with the address bar focused
- [ ] Pin/unpin (pinned lead; unpin goes first among unpinned; saved in the session)
- [ ] Mute/unmute (tab menu and indicator click)
- [ ] Tab indicator priority: capture icon + description tooltip, muted speaker ("Unmute Tab"), playing speaker ("Mute Tab"), group dot (top only)
- [ ] Sleeping tab shows faded icon and keeps its title
- [ ] Closing a tab: closed-stack entry per kind (25 max, keyed, with back/forward state, position, pinned, group, MRU tick); pick the next selection via opener/groups; save the session when the last tab of the last normal window closes
- [ ] Reopen closed tab at its position (pinned block respected), selected, back into its group, with history if possible
- [ ] Move tab within a window / to another window (live state kept); `chrome.tabs.move`
- [ ] Copy link (clean or whole) with the "Link copied" toast
- [ ] Vertical tab list: Search Tabs button on top, New Tab button at the bottom, rows with favicon/spinner, title (tooltip), indicator, close (unpinned) or pin (pinned)
- [ ] Tab list: middle-click close; right click / long press / Shift+F10 / Menu open the tab menu
- [ ] Tab list drag-and-drop reorder (upper/lower half)
- [ ] Tab list row grow-in/shrink-out animations (180 ms, ease-out cubic, only when shown, respect reduced motion); closing never waits on animation
- [ ] Tab list group headers (chip + arrow, collapse on click/Enter/Space, edit on right click/long press/Shift+F10/Menu, accessible name and expanded state), 3 px group-colour bar on grouped rows, collapsed rows hidden
- [ ] Tab list keyboard: arrows/Home/End/PgUp/PgDn land on shown rows only; Tab/Shift+Tab skip hidden rows and reach headers; error bell at ends
- [ ] Top tab bar: always shown in top layout, tabs with title/icon/spinner/indicator/close, pinned compact, drag reorder, drag between windows, drag out to a new window, middle-click close, context menu
- [ ] Top tab bar: group chips at the start (click opens the editor), Search Tabs button at the end, group-colour dots on grouped tabs
- [ ] Tab context menu with Chrome's items/order/mnemonics, "to the Right" vs "Below", Pin/Unpin and Mute/Unmute toggles, Copy Link only with a link, Add to (new) group / submenu, Remove from group, enablement from `TabPlace` and the closed stack
- [ ] Tab menu actions: New tab next (in group), Move to new window (keeps state, pin, kind), Reload, Duplicate (next to it, pinned if pinned, in group, with history), Close, Close others/after (never pinned), Reopen closed
- [ ] Tab search popover (360 wide, list up to 440 tall): entry "Search tabs", headings "Open tabs"/"Recently closed", rows icon+title+site, "No results found"
- [ ] Tab search: core ranking (`tab_search::rows`), MRU clock, same-kind tabs across all windows, first row selected, Up/Down wrap, Enter/click choose (select+raise or reopen here), Escape/toggle close, anchor fallbacks (sidebar button, tab bar button, toggle, top of page)
- [ ] Tab groups via core `TabGroups` with `WindowTabs` built per call, `Step`s applied with settle held, `settle` after every tab change
- [ ] Group actions: toggle collapse, edit, new tab in group, ungroup, close group; Add to new group opens the editor
- [ ] Group editor: name entry (live, Enter closes, focused on open), 9 colour swatches (live), New Tab in Group, Ungroup, Close Group; anchored at header/chip/top
- [ ] Group colours from `GroupColor::rgb(dark)`, chip text white/#202124, restyled on theme change
- [ ] Groups saved per tab in `TabSnapshot::group` and restored after selection on session restore; private groups never saved
- [ ] Selecting a hidden tab expands its group; collapsing the selected tab's group moves selection per core
- [ ] Extension toolbar: pinned actions in `toolbar.extensions` order (new installs pinned), filtered by allowed-in-private for private windows
- [ ] Extension button: icon, badge text, tooltip title; in-place update keeps an open popup
- [ ] Extension button click: popup (anchored to button or puzzle, 320×200 initial, auto-sized 200–800 × 80–600, closes on `window.close()`, one at a time) or `action.onClicked`; grants activeTab
- [ ] Extension button drag reorder (RTL-aware) → `toolbar::move_pinned`
- [ ] Extension button right-click menu: extension's action context-menu items, Unpin, Manage Extensions
- [ ] Puzzle menu: heading "Extensions", per-action row (open + pin toggle with "Pin to Toolbar"/"Unpin from Toolbar"), Manage Extensions; live refill; hidden puzzle when no actions
- [ ] Extension commands: `_execute_action` family opens the action; others fire `commands.onCommand` with the selected tab
- [ ] Settle the CEF extension strategy (Chromium toolbar vs shell-drawn actions; tabs/windows/tabGroups visibility) before implementing the extension UI
- [ ] Private windows: entry points (Ctrl+Shift+N, menu, link context menu), private new tab page, private closed stack and tab search isolation, kind kept on moves, sent back on cross-kind drop, nothing saved
- [ ] Fullscreen (F11) hides top bars; page fullscreen notice "{site} is now full screen / Press Esc to exit full screen" for 4 s; prompts withdrawn while fullscreen
- [ ] Permission prompt scheduling: selected tab's oldest request only, none while another bubble is open or fullscreen, withdraw vs dismiss rules, click-elsewhere dismiss
- [ ] `window.open`/`_blank` popups become foreground tabs next to the opener, in its group and session, discarded if closed before showing
- [ ] Downloads button reveal and dangerous-download warning popover under it
- [ ] Update banner with title and button (`app.update`)
- [ ] Bookmarks bar visibility (pref + action state), star state, bookmark bubble, site-info popover anchoring
- [ ] Toast overlay for window messages
- [ ] Engine hooks: title, favicon, loading state/progress, committed URL, history availability, zoom (level ↔ percent mapping), find, print, devtools toggle/console, mute, capture state, audio-playing state, fullscreen, popups, off-the-record context
- [ ] Decide split view and media player for Linux (Windows-only today)
- [ ] Self-test equivalents for `tabs`, `tab_animation`, `tab_layout`, `tab_menu`, `tab_search`, `tab_groups`, `popup`, `extension_toolbar`, `context_menus`, `extension_commands`, `private_window`, `settings` (home), `ctrl_s_toggles_sidebar`, `capture_in_use`, `profiles`, `download` (button), `memory_saver` (faded icon)
