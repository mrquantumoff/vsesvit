# Linux shell UI architecture (GPUI + CEF): `crates/vsesvit-gpui`

Date: 2026-10-16. Task: `ui-design`. Status: design. Everything marked provisional, [I] or [S:…] may change after the spikes report.

This document designs the GPUI side of the new Linux shell. The shell replaces `crates/vsesvit-gtk` (GTK4 + libadwaita + WebKitGTK) and `crates/vsesvit-webext`.

**Decided by the owner (taken as given here):**
- stock CEF 154 (`cef` = 154.5.0, CEF 154.0.34, Chromium 154.0.8037.98);
- the whole app on X11, so XWayland under a Wayland session;
- GPUI forced to its X11 backend, CEF started with `--ozone-platform=x11`;
- every tab is a **Chrome-style CEF browser embedded as a native X11 child window** of a GPUI window;
- GPUI draws all browser chrome; no off-screen rendering.

**Inputs read in full for this design:**
- inventories: `window.md`, `shell-core.md`, `tab-engine.md`, `omnibox-privacy.md`, `bookmarks-downloads.md`, `dialogs.md`, `extensions-sync-updates.md`, `self-test.md`, plus `core-surface.md` §0–1 for core's ground rules;
- research: `gpui.md`, `cef-windowing.md`, `cef-extensions.md`, `gpt-extension-architecture.md`, `cef-rs-api.md` (§1–2, §5–6), `prior-art.md` (§1, §8–9);
- the source of `gpui-pre` 0.3.9, `gpui-pre-linux` 0.3.9, `gpui-component` 0.7.3, `gpui-base` 0.7.3 and `gpui-kit-assets` 0.7.3 in `~/.cargo/registry`;
- `vsesvit-core`'s `shortcuts.rs` and `lib.rs`.

**Labels**
- **[V]**: verified. I read it in source on disk or ran the command; §7 records how.
- **[I]**: inferred. My reasoning, not run.
- **[S:n]**: depends on spike question *n* in §6. Each question belongs to `spike-core`, `ext-load` or `ext-api`.
- The inventories' tags are reused: **KEEP** (product behavior), **WEBKIT-ONLY** (goes away), **NEEDS-CEF** (needs an engine hook).

Contents:
0. Decisions in brief
1. Application structure
2. Feature map
3. Theme and visual design
4. Engine API boundary
5. Work breakdown
6. Spike dependencies
7. Verification log

---

## 0. Decisions in brief

1. **Three crates.**
   - `vsesvit-engine`: the engine-agnostic contract. It has types, an `Engine` trait, events, synchronous hooks and a `FakeEngine`. It depends on neither GPUI nor `cef`.
   - `vsesvit-cef`: the CEF glue. It implements the contract and is written by the CEF tasks.
   - `vsesvit-gpui`: the shell. It never names a `cef` type.
   - The binary `crates/vsesvit` wires them together, so `vsesvit-gpui` does not even depend on `vsesvit-cef` (§1.1).
2. **Boot order:**
   1. CEF sub-process dispatch;
   2. CLI;
   3. profile resolution, or the profile picker in a GPUI-only process;
   4. take core's profile lock, or forward the command line to the owner over our own per-profile Unix socket;
   5. CEF initialize (external message pump);
   6. GPUI on `WindowingModes::X11`.

   Steps 1–4 never start GPUI, and steps 3–4 never start CEF (§1.2).
3. **The main thread is GPUI's.** CEF's UI thread is the same thread. A GPUI foreground task pumps it and also iterates the default GLib context (needed for GtkUi IME and dialogs). The engine never calls into GPUI: it sends events through a channel and answers a few synchronous hooks from snapshots (§1.3, §4).
4. **State is held in:**
   - a `Shell` GPUI global: core `Profile` as `Rc<RefCell<…>>`, plus the engine;
   - one `Entity<BrowserState>` controller per process;
   - one `BrowserWindow` root view per browser window, each with an `Entity<TabListModel>`;
   - one `Entity<Tab>` per tab;
   - feature entities: `Omnibox`, `TabSidebar`, `Downloads`, `Syncer`, …

   The GTK shell's observer lists become GPUI events and subscriptions (§1.4).
5. **The page is an X11 child window.** GPUI cannot paint over it. So:
   - bars (find bar, update banner, screen-sharing bar) **push the page down** instead of covering it;
   - everything else that may cover the page goes through one per-window `OverlayHost`. Its default backend opens GPUI `WindowKind::PopUp` override-redirect windows at screen coordinates [V that X11 PopUp is override-redirect]. Its fallback backend punches an XShape hole in the CEF child [S:4] (§1.7).
6. **Secondary surfaces are separate GPUI windows:** Settings, Extensions, Bookmarks, History, Downloads, About, Keyboard Shortcuts, Manage Profiles, the Welcome tour and the picker.
   - Modal prompts raised from a *browser* window open as `WindowKind::Dialog` windows.
   - Inside GPUI-only windows, dialogs are ordinary gpui-component in-window dialogs (§1.8).
7. **Keyboard:**
   - While GPUI chrome has focus, a GPUI keymap built from core's `Keymap` handles shortcuts.
   - While the page has focus, the engine consults a routing table that the UI pushes (`OnPreKeyEvent`/`OnKeyEvent`) and posts `Shortcut` events.
   - Chrome's own accelerators are blocked in `OnChromeCommand` (§1.9).
   - **X11 focus needs our own handling:**
     - GPUI never calls `SetInputFocus` [V];
     - GPUI treats focus moving into a child window as the window going inactive [V];
     - X11 delivers keys to the child under the pointer whenever focus is on an ancestor [I].

     The fix is a focus proxy, through a small GPUI patch or our own X connection [S:2] (§1.10).
8. **Chromium now does natively** what the GTK shell built or lacked:
   - page context menu (we only add items);
   - JS dialogs, HTTP auth, file chooser, `<select>` and pickers;
   - print preview, `view-source:`, DevTools and the console;
   - spell check;
   - notification display;
   - possibly the link status bubble [S:8] (§2.9).
9. **Look:**
   - a gpui-component `Theme` with two Vsesvit JSON themes modelled on libadwaita;
   - follows the desktop color scheme through the portal [V that GPUI X11 reads `color-scheme` from the portal];
   - honours reduced motion;
   - Lucide icons (ISC licence) from `gpui-kit-assets`;
   - no Zed GPL crates (§3).
10. **Work:** 20 units in 5 waves. Wave 0 freezes the contracts (`vsesvit-engine`, module skeleton with the frozen interfaces of §5.2, theme, X11 glue). After that, units own disjoint files (§5).

---

## 1. Application structure

### 1.1 Crates and dependencies

```
crates/vsesvit (bin) ─┬─► vsesvit-cef  ──► cef =154.5.0, x11rb      (CEF glue; implements the contract)
                      └─► vsesvit-gpui ──► vsesvit-engine ◄─┘        (contract: types, trait, events, FakeEngine)
                                       ├─► vsesvit-core, vsesvit-sync, vsesvit-update
                                       ├─► gpui (= gpui-pre =0.3.9), gpui_platform (= gpui-pre-platform =0.3.9, feature "x11")
                                       ├─► gpui-component =0.7.3, gpui-kit-assets =0.7.3
                                       └─► x11rb 0.13 (shape, xinput), ashpd (portal file chooser with filters)
```

- **`vsesvit-engine` depends only on `vsesvit-core`** (for shared enums such as `Browsing`, `Permission`, `Capturing`, `Chord`, `Command`), `url` and `futures-channel`. A CI grep forbids `gpui` and `cef` in it.
- **`vsesvit-gpui` receives the engine as a `&dyn EngineFactory`** from `main`. Tests pass `FakeEngineFactory` and run in `gpui::TestAppContext` without a display or CEF.
- **The binary's Linux branch:**

```rust
// crates/vsesvit/src/main.rs
fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        // CEF sub-processes (zygote, renderer, GPU, utility) re-execute this binary. vsesvit-cef calls
        // api_hash() first, then execute_process(app), before any thread exists [V cef-rs-api §2.1, G1].
        if let Some(code) = vsesvit_cef::run_subprocess() {
            return code;
        }
        return vsesvit_gpui::run(&vsesvit_cef::CefEngineFactory);
    }
    #[cfg(windows)] { return vsesvit_winui::run(); }
    #[allow(unreachable_code)] { eprintln!("vsesvit supports Windows and Linux only"); ExitCode::FAILURE }
}
```

- **Helper executable:** whether `vsesvit-cef` uses a separate helper (`browser_subprocess_path`, as `cef-rs-api.md` §2.1 recommends) is the CEF glue's decision. Either way `run_subprocess()` returns `None` in the browser process.

### 1.2 Process boot order

`vsesvit_gpui::run(factory)` in `boot/mod.rs`. The steps run strictly in this order:

| # | Step | Module | Notes |
|---|---|---|---|
| 1 | Logging to stderr through the `log` crate (`env_logger`-style filter `VSESVIT_LOG`, default `warn`, `info` for `vsesvit*`). Private addresses only through `Browsing::loggable`. | `boot/logging.rs` | GLib logging goes. Whether to keep a log file under `<profile>/vsesvit.log` is open; Windows has one (shell-core §19). |
| 2 | Parse `args_os().skip(1)`, a verbatim port of `cli.rs` including its unit tests and `USAGE`. Errors print `vsesvit: {e}\n\n{USAGE}` and exit 2. | `boot/cli.rs` | Sub-process switches never reach this point (step 0 is in `main`). |
| 3 | `Help`, `Version`, `--check-for-updates`, `--update`: no profile, no display, no engine. | `boot/cli.rs`, `boot/headless.rs` | `download_dir()` = `$XDG_CACHE_HOME/vsesvit/updates` (`dirs`). |
| 4 | `--self-test OUT_DIR [--network]` uses the same steps 5–9 with: a fresh `OUT_DIR/profile`; `Home{ProfilesDir::at(OUT_DIR), "profile"}`; no instance socket; `SCRIPTED`. | `self_test/mod.rs` | |
| 5 | `Start::resolve(profile_dir, has_targets)`: sweep, then core's startup decision (shell-core §5). `Start::Picker(dir)` → `profiles::picker::run(dir)`, a GPUI-only app on `WindowingModes::X11` with no engine. It launches the chosen profile and exits. | `boot/profile_start.rs`, `profiles/picker.rs` | The picker never takes a profile's lock or a CEF cache. |
| 6 | `Profile::open(root, OpenOptions { chrome_version: factory.chromium_version(), .. })`, where `chromium_version()` = `"154.0.8037.98"`. | `boot/mod.rs` | |
| 6a | `Ok(profile)`: this process is the primary. If the profile list marks it removed: drop, sweep, print `vsesvit: the profile at {root} was removed`, FAILURE. | | |
| 6b | `Err(Locked)`: `instance::forward(root, argv, cwd, startup_id)` connects to the owner's socket and relays its stderr lines and exit status. If it cannot connect within 2 s: `vsesvit: the profile is in use by another Vsesvit process that could not be reached`, FAILURE. | `boot/instance.rs` | Replaces GApplication's D-Bus forwarding (shell-core §7). |
| 6c | Any other error: `vsesvit: cannot open the profile at {root}: {e}`, FAILURE. | | |
| 7 | `instance::Listener::bind(root)`: bind and listen only. The accept loop starts later, so that no extra thread exists during CEF initialization [I, cefclient: "the Chromium sandbox requires that there only be a single thread during initialization"]. | `boot/instance.rs` | |
| 8 | `factory.initialize(EngineConfig { … })` gives `EngineParts { engine, driver, events }`. | `boot/mod.rs` | |
| 8a | Config: `root_cache_path` = `<profile root>/engine`, with the request contexts at or below it. | | Must be decided with the CEF glue (cef-rs-api §5.1). |
| 8b | Config: startup switches from prefs: `scrolling.smooth`, `system.hardware_acceleration`. These apply at launch only, hence a restart note in Settings. | | |
| 8c | Config: `--ozone-platform=x11`, external message pump, CEF log file in the profile, `user_agent_product`. | | |
| 8d | `Err(EarlyExit(code))` → exit with `code`. | | |
| 9 | `gpui_platform::linux(WindowingModes::X11).with_assets(VsesvitAssets).run(move \|cx\| app::start(cx, StartArgs {…}))` [V API exists]. | `app/mod.rs` | |
| 10 | After `run` returns: `app::quit::finish(outcome)`:<br>1. `driver.shutdown()` (CEF shutdown; every browser was already closed);<br>2. drop the `Profile` and check that it was the last reference;<br>3. delete a removed profile's data;<br>4. spawn `restart_program --profile-dir root` if an update asked for it;<br>5. return the exit code. | `app/quit.rs` | Order from shell-core §9 and cef-rs-api G16. |

**Why forward before CEF.** CEF has its own process singleton on `root_cache_path` and calls `OnAlreadyRunningAppRelaunch(argv, cwd)` in the primary. But it cannot carry the primary's exit status back to the second process, which GApplication could (shell-core §7). It would also start CEF in a process that is about to exit.

Taking core's `LOCK` first means the second process never reaches CEF initialize. The CEF path stays as a fallback: `BrowserEvent::AppRelaunch` is handled exactly like a socket invocation, with no reply.

**Instance socket protocol** (`boot/instance.rs`):
- Path: `$XDG_RUNTIME_DIR/vsesvit/<first 16 hex of sha256(canonical root)>.sock`, in a 0700 directory. Fallback without a runtime dir: `<root>/instance.sock`.
- The peer is checked with `SO_PEERCRED` (same uid).
- The client sends one JSON line: `{"argv": [<base64 OsString>…], "cwd": "<path>", "startup_id": "<DESKTOP_STARTUP_ID or XDG_ACTIVATION_TOKEN, if any>"}`.
- The server answers with zero or more `{"stderr": "…"}` lines, then `{"status": 0|1}`.

**`app::start(cx, args)`** runs on the main thread inside GPUI's `run`:
1. `gpui_component::init(cx)`, then `theme::init(cx, core prefs)`.
2. Set the `Shell` global (`core`, `engine`, `location`, `home`, `scripted`).
3. `BrowserState::new`: it creates `Downloads`, `Extensions`, `Syncer`, `Updates`, `Privacy` and the session saver.
4. `engine_bridge::start(cx, driver, events)` starts the pump task and the event router (§4.3).
5. `engine.set_hooks(Rc<Hooks>)`, and `engine_bridge::keymap::install(cx, …)` (GPUI bindings plus engine `KeyRouting`).
6. `instance::Listener::spawn_accept(cx)`: a background thread accepts and an `async-channel` carries `Invocation`s to a foreground task.
7. `BrowserState::start`, in the order of shell-core §11.3:
   - theme;
   - keymap;
   - privacy: tracking-protection filter, cookie rules, `clear_at_start` (all pushed to the engine before any browser exists);
   - extensions: list, hand-off to the engine, reconcile, scheduled checks (not in scripted runs);
   - favicon preload;
   - the Memory Saver timer (60 s).
8. The first `Invocation` (the process's own argv) goes through `BrowserState::handle_invocation`. That calls `open_startup_windows(targets)` and then the welcome if this run created the profile.

### 1.3 Threads and loops

| Thread | What runs there | Rule |
|---|---|---|
| Main (GPUI calloop) | All GPUI entities and views. Core `Profile`. CEF's UI thread: `EngineDriver::do_work()`, called from a foreground task; it runs `do_message_loop_work()` and non-blocking `g_main_context_iteration(NULL, 0)` until idle [V cef-rs-api §2.4]. Every `Engine` call. `ShellHooks` calls. | Core is borrowed for one call only, never across an engine or GPUI call. `ShellHooks` never touch `gpui::App` (they run inside `do_work`, outside any GPUI update). |
| GPUI background executor | Core's slow jobs: `InstallJob::run`, `UpdateCheck::run`, `FaviconFetch::run`, `SuggestRequest::run`, sync rounds, update download and install, `AccountPicture::fetch`, `import::Source::read`, export writes, `import::installed_browsers`. | Pattern: `cx.background_executor().spawn(job)` then `cx.spawn` to commit on the main thread. Replaces `gio::spawn_blocking`. |
| CEF IO thread | `RequestFilter` (tracker blocking, cookie access) on `Send + Sync` snapshots the UI publishes. | Never touches `Profile` (core-surface §0). |
| Instance accept thread | Blocking `accept`, then a channel. | Started after CEF initialize. |
| X11 watcher thread (`platform::x11`) | Our own `x11rb` connection: root `PropertyNotify` for `_NET_ACTIVE_WINDOW`, XI2 raw button presses for dismissing overlays. | Results reach GPUI through a channel. |

Timers use `cx.background_executor().timer(d)` awaited in foreground tasks:
- the 2 s session-save throttle;
- the 60 s Memory Saver sweep;
- sync ticks (2 s);
- the updater (30 s, then 24 h);
- extension checks (60 s, then 5 h);
- the 250 ms download progress gate;
- the 1 s Downloads re-wording;
- the 4 s fullscreen notice;
- the 500 ms prompt guard;
- the 200 ms progress-line hold.

### 1.4 Global state, entities and views

```
Shell  (gpui::Global)                                   app/shell.rs
├─ core:     Core = Rc<RefCell<vsesvit_core::Profile>>
├─ engine:   Rc<dyn vsesvit_engine::Engine>
├─ location: ProfileLocation { root, home: Option<Home> }
├─ scripted: bool                                       (self-test)
└─ state:    Entity<BrowserState>                       state/mod.rs
     ├─ windows: Vec<WindowHandle<BrowserWindow>>       most recently focused first
     ├─ library: LibraryWindows                         dialogs/library.rs (one Bookmarks, History, Settings, Extensions, Downloads per Browsing)
     ├─ tabs:    HashMap<TabId, Entity<Tab>>
     ├─ closed:  ClosedTabs<ClosedTab> × 2              normal, private (25 each)
     ├─ private: Option<PrivateSession>
     ├─ mru clock, next WindowId, SessionSaver (2 s throttle), profiles: Registry
     ├─ downloads:  Entity<Downloads>                   downloads/controller.rs
     ├─ extensions: Entity<Extensions>                  extensions/controller.rs
     ├─ privacy:    Entity<Privacy>                     privacy/mod.rs (permissions, trackers, cookies)
     ├─ sync:       Entity<Syncer>                      sync.rs
     └─ updates:    Option<Entity<Updates>>             updates/mod.rs

BrowserWindow  (root view of each browser window)       window/mod.rs
├─ id: WindowId, browsing: Browsing, x11: u32 (its XID)
├─ tabs:     Entity<TabListModel>                       tab/model.rs
├─ toolbar:  Entity<Toolbar>                            toolbar/mod.rs
│    ├─ omnibox:    Entity<Omnibox>                     omnibox/mod.rs
│    ├─ ext_bar:    Entity<ExtensionToolbar>            extensions/toolbar.rs
│    ├─ downloads:  Entity<DownloadsButton>             toolbar/downloads_button.rs
│    └─ profile:    Entity<ProfileButton>               toolbar/profile_button.rs
├─ sidebar:  Entity<TabSidebar>                         tabs_ui/sidebar.rs
├─ strip:    Entity<TabStrip>                           tabs_ui/strip.rs
├─ banner:   Entity<UpdateBanner>                       toolbar/update_banner.rs
├─ bookmarks_bar: Entity<BookmarksBar>                  bookmarks/bar.rs
├─ find_bar: Entity<FindBar>                            find_bar.rs
├─ page:     PageArea (element + per-window state)      window/page_area.rs
├─ overlays: Entity<OverlayHost>                        overlay/mod.rs
├─ prompts:  PromptScheduler                            omnibox/permission_prompt.rs
└─ subscriptions: BrowserState, TabListModel, each Tab it holds

Tab  (one per tab, owned by BrowserState, held by exactly one TabListModel)   tab/mod.rs
```

**Who does what.** The table maps old GTK pieces to new ones.

| Entity / view | Replaces (GTK) | Holds | Emits | Subscribes |
|---|---|---|---|---|
| `Shell` | `Browser(Rc<Inner>)` handles, the `Slot` | Core, engine, location | — | — |
| `BrowserState` | most of `browser.rs` | registries, stacks, sub-controllers | `BrowserStateEvent::{BookmarksChanged, HistoryChanged, PrefsChanged(Vec<String>), ProfilesChanged, KeymapChanged, ThemeChanged, ExtensionActionsChanged, DownloadsStarted(Browsing), UpdateBanner(Option<Banner>), PrivateSessionEnded}` | `Tab` (history, session, favicons), `Syncer` |
| `BrowserWindow` | `window/mod.rs`, `layout.rs`, `actions.rs` | layout, window lifecycle, focus arbitration, window actions | `BrowserWindowEvent::{LayoutChanged, Fullscreen(bool)}` | `BrowserState`, `TabListModel`, held `Tab`s, `OverlayHost` |
| `TabListModel` | `AdwTabView` order, pinned, selection, `TabGroups<TabId>` | `order`, `pinned`, `selected`, `groups`, `parents` (opener picks) | `TabListEvent::{Inserted, Removed, Moved, Selected, PinnedChanged, GroupsChanged}` | — |
| `Tab` | `tab.rs` (minus the WebKit view) | mirror of engine state; pending transition; typed text; rest (awake/asleep); permission queue | `TabChanged::{Title, Favicon, Loading, Progress, Committed(CommitKind), History, Zoom, Find, Capture, Audio, Sleep, Security, Fullscreen(bool), FocusChanged(bool), TakeFocus{next}, ClosedByPage}` | — (fed by the router) |
| `Omnibox` | `address_bar.rs`, `omnibox.rs` | field, edit model, suggestions, chips, bubble slot | `OmniboxEvent::{Submitted, Cancelled, BubbleChanged}` | selected `Tab` |
| `TabSidebar`, `TabStrip` | `tab_list.rs`, `AdwTabBar`, `groups.rs` UI | row view state, animations | actions | `TabListModel`, `Tab`s |
| `OverlayHost` | `crate::popup`, `GtkPopover` | open overlays, popup windows | `OverlayEvent::Dismissed{id, reason}` | platform watchers, window bounds and activation |
| `Downloads` | `downloads.rs` controller | live transfers, reservations | `DownloadsEvent::{List, Progress(id), Started(Browsing), Warn(id)}` | engine download events (router) |
| `Syncer`, `Updates`, `Extensions`, `Privacy` | `sync.rs`, `updates/`, `extensions.rs`, `permissions.rs`/`trackers.rs`/`cookies.rs` | as before | their own `*Event` | — |

**GPUI mechanics used:**
- `Entity<T>` with `EventEmitter<E>`, `cx.subscribe`/`cx.observe`, `WeakEntity` in async tasks;
- `WindowHandle<BrowserWindow>::update(cx, |w, window, cx| …)` for cross-window calls;
- `cx.global::<Shell>()` for process-wide handles;
- one `FocusHandle` per focusable view.

Every view holds `BrowserState` and other windows **weakly**, so that closing everything releases the profile lock. That is the GTK test `releasing_the_profile_frees_its_lock`, which stays.

### 1.5 Window anatomy

**Left layout** (the default, `tabs.position = left`). Right is the mirror image, with the sidebar toggle after the main menu.

```
┌───────────────────────────────────────────────────────────────────────────────────────────┐
│ TOOLBAR 46 px (also the CSD title bar; drag on empty space)                               │
│ [<][>][reload|stop][home?][toggle] [private-icon]  [ omnibox <= 720 px, centred ]  [ext...][puzzle][+][downloads?][avatar][menu] [-][□][x] │
├───────────────────────────────────────────────────────────────────────────────────────────┤
│ UPDATE BANNER 40 px (only while an update waits)                                          │
├───────────────────────────────────────────────────────────────────────────────────────────┤
│ BOOKMARKS BAR 28 px (pref)                                                                │
├───────────────────────────────────────────────────────────────────────────────────────────┤
│ FIND BAR 42 px (while open)                                                               │
├──────────────────┬────────────────────────────────────────────────────────────────────────┤
│ TAB SIDEBAR      │ SHARING BAR 40 px (while the selected tab shares the screen)           │
│ 180–300 px (20%) ├────────────────────────────────────────────────────────────────────────┤
│ [Search Tabs]    │                                                                        │
│ ─────────────    │      PAGE SLOT  =  the selected tab's CEF X11 child window             │
│ o Tab title   x  │      (GPUI paints only the page background under it)                   │
│ > Group chip     │                                                                        │
│ ─────────────    │                                                                        │
│ [+ New Tab]      │                                                                        │
└──────────────────┴────────────────────────────────────────────────────────────────────────┘
```

**Top layout** (`tabs.position = top`). The tab strip sits under the banner and above the bookmarks bar, in the GTK shell's order (window §2):

```
TOOLBAR 46 │ UPDATE BANNER │ TAB STRIP 38 px: [chips...][pinned][pinned][tab][tab]...   [search] │ BOOKMARKS BAR │ FIND BAR │ PAGE
```

**Fullscreen** (F11, or a page asking for it): every top bar and the sidebar collapse, and the page slot fills the window. The fullscreen notice is an overlay (§1.7).

**Narrow windows** (window width under 720 logical px): the sidebar hides and the toggle shows it again by **pushing** the page, not overlaying it. This differs from the old overlay drawer (decision D4 in §2.11).

The view tree in `BrowserWindow::render`:

```rust
div().size_full().flex().flex_col().key_context("BrowserWindow").track_focus(&self.focus)
    .on_action(cx.listener(Self::new_tab)) /* … every window action, §1.9 … */
    .child(self.toolbar.clone())                       // titlebar row; window controls when decorations are client-side
    .when_some(self.banner_visible(), |d, _| d.child(self.banner.clone()))
    .when(self.layout.is_top(), |d| d.child(self.strip.clone()))
    .when(self.bookmarks_bar_visible, |d| d.child(self.bookmarks_bar.clone()))
    .when(self.find_bar_open, |d| d.child(self.find_bar.clone()))
    .child(
        div().flex_1().flex().flex_row()                // flex_row_reverse for the right layout
            .when(self.layout.has_sidebar() && self.sidebar_shown, |d| d.child(self.sidebar.clone()))
            .child(
                div().flex_1().flex().flex_col()
                    .when(self.sharing_bar_visible(cx), |d| d.child(self.sharing_bar(cx)))
                    .child(PageArea::new(self.tabs.clone())),   // reports device-pixel bounds to the engine
            ),
    )
```

### 1.6 Living with a native child window

**Page slot geometry** (`window/page_area.rs`).
- `PageArea` is a custom `Element`. In `prepaint` it rounds `bounds × window.scale_factor()` to a `DeviceRect`, relative to the window's XID.
- It calls `engine.set_bounds(selected, rect)` only when the rectangle changed.
- On selection change: `set_visible(old, false)`, then `set_bounds(new, rect)`, then `set_visible(new, true)`. The engine maps and unmaps the child and calls `WasHidden` [S:1].
- GPUI and Chromium both read the X11 scale (Xft.dpi). The scale factor is passed in `TabSpec` [S:1].

**Native parent.**
- `platform::x11::native_window(window)` reads the XCB id from GPUI's `raw-window-handle` [V `impl HasWindowHandle for Window` in `gpui-pre` 0.3.9; `rwh::XcbWindowHandle::new` in `gpui-pre-linux` 0.3.9 `x11/window.rs`].
- It becomes `NativeParent::X11(xid)` for `TabSpec` and for `reparent`.

**Focus arbitration** (`engine_bridge/focus.rs`, `FocusArbiter`, one per window). There is one keyboard owner per window: `Owner::Page(TabId)` or `Owner::Chrome`.
- **Page takes focus.** A click into the page makes Chromium take X focus itself. The engine reports `TabEvent::FocusChanged{focused: true}`. The arbiter calls `window.blur()`, so that the omnibox shows its resting text, and closes autohide overlays.
- **Chrome takes focus.** A GPUI element gets focus: a mouse-down in chrome, a focus action (Ctrl+L, F6), or opening the find bar. The arbiter calls `engine.blur_page(tab)`, then `platform::x11::focus_window(focus_target)`. GPUI never sets X focus itself [V: no `set_input_focus` in `gpui-pre-linux`].
  - `focus_target` is the GPUI top-level, or the focus proxy of §1.10 [S:2].
- **After navigation, on Escape in the omnibox, or when switching to a non-blank tab:** `engine.focus_page(tab)`.
- **`TabEvent::TakeFocus{next}`** (Tab key past the page's last element): focus moves to the omnibox (next) or to the toolbar's last button (previous).

**Window activity** (`platform::x11::ActiveWindowWatcher`).
- GPUI's own `is_window_active()` turns false when focus enters the CEF child [V: `FocusOut` calls `set_active(false)` without looking at the detail].
- So activation comes from root's `_NET_ACTIVE_WINDOW` (`PropertyNotify`) instead. This drives:
  - the MRU tick of the selected tab;
  - `profile_used()`;
  - hiding and re-showing overlays;
  - inactive styling.
- Every browser window sets `inactive_frame_interval: None` [V field exists], so pages under GPUI's inactive state are not throttled. Chromium paints its own child anyway.

**Close ordering.** Destroying a parent X window under a live CEF child produces `BadWindow` and can crash (`cef-windowing.md` §1.2, CEF #3165). The rules:
- Closing a **window**:
  1. `on_window_should_close` returns `false`;
  2. the window hides (unmaps), saves the session if it is the last normal window, and discards its tabs (`engine.close_tab(tab, Force)` for each);
  3. it removes itself only after the router has delivered `TabEvent::Closed` for each of them.
- Closing a **tab**: the tab leaves `TabListModel` at once. Its row animates out, and its closed-stack entry is pushed. The engine browser closes asynchronously.
- **Quit:**
  1. save the session;
  2. `engine.close_all()`, and wait for `AllTabsClosed` (cap 5 s);
  3. final sync (3 s cap) and cookie `clear_at_exit` (3 s cap);
  4. `cx.quit()`.

  GPUI's `on_app_quit` gives only 200 ms [V gpui.md §4.1], so it is not used for this.

**Moving tabs between windows.**
- First choice: `engine.reparent(tab, NativeParent::X11(target), rect)` (an `XReparentWindow` of the CEF child) [S:10].
- Fallback if the spike fails: close and reopen the URL in the target window, losing the page state.
- A GPUI drag stays inside one window, so cross-window tab drag needs our own tracking: on drag end outside the window, find the window under the pointer through `platform::x11`, or open a new window.

**Screenshots.**
- GPUI renders only its own surface. `platform::x11::capture(xid)` reads the window with `GetImage` on the top-level, which includes inferiors, so the CEF child is captured too [S:11].
- Overlay windows are captured and composited at their positions, as GTK's `save_png_with_popovers` did.
- DevTools `Page.captureScreenshot` (`engine.capture_page`) is the fallback for page pixels.

**Fullscreen.**
- A page entering fullscreen sends `TabEvent::FullscreenRequested(true)`. The window then calls `window.toggle_fullscreen()`, hides the chrome and shows the notice.
- Escape is handled by Chromium; leaving gives `FullscreenRequested(false)`.
- F11 toggles window fullscreen without a notice.
- Whether Chromium also shows its own "Press Esc" bubble inside the child is unknown. If it does, Vsesvit's notice is dropped [S:7].

### 1.7 Overlay strategy

GPUI in-window overlays (`deferred(anchored(…))`, gpui-component `Popover` and `PopupMenu`) are clipped to the window [V gpui.md §2.9] and drawn *under* the CEF child. Three mechanisms remain:

1. **Layout.** The surface takes space and the page shrinks. Used for the find bar, update banner, screen-sharing bar, sidebar and tab strip.
2. **`OverlayHost`.** Every surface that may cover the page or extend past the window goes through it.
3. **Chromium-native UI.** Chromium draws its own X windows: its menu windows are parented to the root to avoid clipping [V `cef-windowing.md` §1.2], and its dialogs sit inside the child.

**`OverlayHost` backends** (`overlay/`):

| Backend | How | Pros | Cons | Use |
|---|---|---|---|---|
| `Popup` (default) | One GPUI `WindowKind::PopUp` window per open overlay, at `window.bounds().origin + anchor` in screen pixels. GPUI X11 creates PopUp windows as override-redirect children of the root, typed `_NET_WM_WINDOW_TYPE_NOTIFICATION` [V]. Transparent visual where available, so shadows and rounded corners work [I under XWayland's compositor]. | Above the CEF child; can extend past the window (bookmark menus up to 75% of monitor height, extension popups up to 800×600); the page stays live. | Gets no X focus by itself [V]. Must close or hide on window move, resize and deactivation. Outside clicks have to be detected ourselves. | everything listed below |
| `Cutout` (fallback, `VSESVIT_OVERLAY=cutout`) | In-window `deferred(anchored(…))` element, plus `engine.set_occlusions(tab, rects)`. The engine applies an XShape hole of those rectangles to the CEF child, so GPUI's drawing shows through. | Focus stays in the main window; gpui-component popovers work unchanged. | Clipped to the window; rectangular only (no shadows or rounded corners); Chromium may reset the shape [S:4]; a one-frame flash when opening. | if `Popup` fails S:3 |
| `Inline` | Plain in-window overlay, with no hole. | Free. | Only when the overlay rectangle does not intersect the page rectangle. | tooltips in the toolbar, menus fully inside the sidebar |

**Focus policy per overlay:**
- `OverlayFocus::Passive`: the overlay never takes keyboard focus. The owner keeps it and drives the overlay. Example: the omnibox moves the suggestion highlight on Up/Down.
- `OverlayFocus::Interactive`: on open, `platform::x11::focus_window(popup_xid)`. GPUI's X11 `FocusIn` handler then binds XIM to that window, so text entry and IME work in it [V: `FocusIn` sets `keyboard_focused_window` and enables IME for that window]. On close, focus goes back to whoever had it (chrome or page) [S:3].

**Dismissal** (`overlay/dismiss.rs`). An overlay closes on:
- Escape;
- a mouse-down in the browser window (GPUI capture handler);
- a page focus change (`TabEvent::FocusChanged{focused: true}`);
- an outside click seen by `platform::x11::PointerWatcher`: XI2 raw button press on the root, then `QueryPointer` against the overlay rectangles [S:3];
- `observe_window_bounds` (move or resize);
- window deactivation, when neither the window nor any of its overlays is `_NET_ACTIVE_WINDOW` or the X focus;
- its owner closing.

Persistent passive overlays (permission prompt, download warning, toasts) are **hidden** on deactivation and shown again on activation. They are not closed.

**Every surface and how it is shown:**

| Surface | Mechanism | Focus | Dismiss | Notes |
|---|---|---|---|---|
| Omnibox suggestions | Popup | Passive (omnibox drives it) | blur, Escape, submit, tab switch | width = the bar's width, max 440 px tall |
| Site info, zoom bubble, star (bookmark) bubble | Popup via `BubbleSlot` | Interactive (site info has dropdowns; the star bubble has fields) | Escape, outside click, move | one bubble at a time; the slot emits `BubbleChanged` (omnibox-privacy §1.11) |
| Permission prompt | Popup | Passive, 500 ms click guard | withdrawn on tab switch, another bubble or fullscreen; an outside click counts as Dismiss | anchored on the security icon |
| Main menu, profile menu, puzzle menu, extension button menu, tab context menu | Popup via `overlay::menu` (each submenu level is its own popup) | Interactive (keyboard navigation) | Escape, outside click, move | gpui-component `PopupMenu` submenus would be clipped inside a popup window, hence the per-level model |
| Bookmark folder menus, bookmark context menus | Popup (custom levels in `bookmarks/menu.rs`) | Interactive | as menus | rows with favicons, hover opens submenus, middle-click |
| Tab search | Popup | Interactive (search entry) | Escape, toggle, outside click | 360 px wide |
| Group editor | Popup | Interactive (name entry) | Escape, outside click | 300 px wide |
| Download warning | Popup | Passive, no autohide | close button, Keep, Discard | under the downloads button |
| Extension action popup | Popup whose content is a `PageArea` hosting a Chrome-style child (`TabKind::ExtensionPopup`) | page focus | outside click, Escape, `window.close()` | auto-size 200..800 × 80..600 from `PreferredSize` [S:13] |
| Toasts | Popup stack, bottom centre of the page area | Passive (buttons clickable) | timeout, action | plain text, never markup |
| Fullscreen notice | Popup at top centre | Passive | 4 s, leaving fullscreen | possibly replaced by Chromium's own bubble [S:7] |
| Link hover preview | Chromium status bubble (`BrowserSettings.chrome_status_bubble`) [S:8]; fallback a Popup at bottom-left fed by `TabEvent::StatusText` | — | — | |
| Tooltips | `HoverTooltip`: Inline when outside the page rectangle, else Popup | Passive | pointer leave | |
| Page context menu, `<select>`, colour and date pickers, JS dialogs, beforeunload, HTTP auth, print preview, file chooser, extension install and permission dialogs, DevTools | Chromium native | — | — | §2.9 |
| Find bar, update banner, screen-sharing bar, narrow sidebar | Layout | — | — | |

`OverlayHost` exposes `open_overlays()` for the self-test. It keeps the GTK rule that "a menu closes and leaves nothing behind when its anchor goes away" (shell-core §10).

### 1.8 Secondary windows and dialogs

| Surface | Presentation | Size | Instances |
|---|---|---|---|
| Settings | GPUI window, `WindowKind::Normal`, `WM_TRANSIENT_FOR` the opener | 1040×760 | one per process; raised on reopen; closes with its opener |
| Extensions | GPUI window | 720×760 | one; opens with a given row expanded |
| Bookmarks, History | GPUI windows (library) | 640×680 | one each; close with their opener (dialogs §1.1) |
| Downloads | GPUI window | 560×560 | one per `Browsing` |
| About, Keyboard Shortcuts | GPUI windows, `WindowKind::Floating` | 480×560, 640×720 | one each |
| Manage Profiles | GPUI window, `Floating` | 520×480 | one |
| Welcome | `WindowKind::Dialog` over the first normal window | 560×640 | one |
| Profile picker | GPUI window in its own process (no engine) | 760×520 | — |
| Alert-style prompts raised by a browser window:<br>• Add/Edit Profile;<br>• bookmark editor (Edit…/Rename…/Add Page…), Add Folder, Delete Folder?;<br>• install prompt, `permissions.request` prompt;<br>• passphrase offer and prompts;<br>• Save Page and Save File portal dialogs | `WindowKind::Dialog` (GPUI marks it `_NET_WM_STATE_MODAL` [V]) holding the same view an in-window dialog would hold | content size | — |
| Prompts inside GPUI-only windows (Settings, Extensions, library windows) | gpui-component `window.open_dialog(cx, …)` / `open_alert_dialog` in the window's `Root` [V `WindowExt` API] | — | — |

**GPUI picks a dialog's parent from its keyboard-focused window** [V `x11/client.rs` `open_window`]. That is none while the CEF child has focus. So `DialogHost` sets `WM_TRANSIENT_FOR` itself through `platform::x11::set_transient_for(dialog_xid, parent_xid)` after opening [I].

`dialogs/host.rs::present(parent: DialogParent, spec)` chooses between a Dialog window and an in-window `Root` dialog, so feature code writes each dialog view once.

### 1.9 Keyboard routing

**Actions.** Every command is a GPUI action, declared once in `app/actions.rs`:
- the full list from core's `shortcuts::Command`, plus `Stop`, `Home`, `ShowExtensions`, `ShowSiteInfo`, `About`, `Welcome`, `AddProfile`, `ManageProfiles`, `ApplyUpdate`, `MoveTabLeft` and `MoveTabRight`;
- parameterized actions with `#[derive(Action)] #[action(namespace = vsesvit, no_json)]`: `OpenProfile{id}`, `OpenBookmark{url}`, `OpenInNewTab{url}`, `ExtensionPin{id,pinned}`, `ExtensionMove{id,to}`, `TabOp{tab,op}`, `GroupOp{group,op}`.

**1. Chrome has focus.** `engine_bridge/keymap.rs::install` turns core's `Keymap` (`chords(cmd)`) into `KeyBinding::new(keystroke, action, Some("BrowserWindow"))`:
- **Modifiers:** `Mods{ctrl,alt,shift}` maps to `ctrl-` `alt-` `shift-`.
- **Keys:** `Key::A` maps to `a`, `Digit1` to `1`, `F5` to `f5`, `Tab` to `tab`, `Space` to `space`, `PageUp` to `pageup`, `Left` to `left`. `Plus` maps to `+`, `Minus` to `-`, `Equal` to `=`, `Comma` to `,`, `Period` to `.`, `Slash` to `/`, `Question` to `?`, `Semicolon` to `;`.
- **Keypad keys** (`Keypad0..9`, `KeypadPlus`, `KeypadMinus`): GPUI's `Keystroke` may not tell them from the main-row keys [S:6]. If it cannot, they bind to the same action, which is harmless.
- **Built-ins,** not rebindable:
  - `ctrl-tab`, `ctrl-pagedown` → `NextTab`; `ctrl-shift-tab`, `ctrl-pageup` → `PreviousTab`. Both skip tabs hidden in collapsed groups and wrap around (window §15).
  - `ctrl-shift-pageup` / `ctrl-shift-pagedown` → `MoveTabLeft` / `MoveTabRight` (Chrome's keys; this replaces libadwaita's defaults, decision D1).
- **Capture dialog:** `keymap::suspend(cx)` calls `cx.clear_key_bindings()` [V API] while it listens; `apply` restores the bindings.
- **Inner contexts:** `Omnibox`, `TabSearch`, `FindBar`, `TabList`. gpui-component's `Input` context keeps its editing keys (Ctrl+A, Ctrl+C/V/X/Z), which do not clash with browser defaults.

**2. The page has focus.** `keymap::routing_for(keymap)` builds a `KeyRouting` table that is pushed to the engine (§4.2):
- **`RoutePhase::BeforePage`** (Chrome's reserved keys): NewTab, NewWindow, NewPrivateWindow, CloseTab, ReopenClosedTab, NextTab, PreviousTab, SelectTab1..8, SelectLastTab, Quit.
  - The engine checks these in `OnPreKeyEvent` and swallows them.
- **`RoutePhase::AfterPage`**: every other browser chord.
  - The engine sets `is_keyboard_shortcut` in `OnPreKeyEvent`, so the page sees the key first. If the page does not consume it, `OnKeyEvent` posts the command.
- **Both phases end in `BrowserEvent::Shortcut{tab, key}`**, which the router turns into the same GPUI action on the tab's window: `window.dispatch_action(action, cx)`.
- **Chrome's own accelerators** (Ctrl+N opens a Chrome window, and so on; prior-art pitfall P16) are refused in `CefCommandHandler::OnChromeCommand`, except a short allow-list (copy, paste, select all, undo, redo) [S:6].

**3. Extension commands.**
- Core keeps resolving them (`Keymap::extension_shortcuts`) for the Settings page.
- Chromium runs them through its own per-Browser registry while the page has focus [V `cef-extensions.md` §4].
- With chrome focus there is no CEF API to fire them [S:13]. The fallback is to give the page focus and replay the chord with `engine.send_key(tab, chord)`. That is provisional and listed in the engine API as optional.

### 1.10 X11 platform glue and GPUI patches

`platform/x11.rs` owns one `x11rb::RustConnection` (any X client may act on any window). It aligns with `spike-core`'s `xwin.rs` in `/tmp/vsesvit-scratch/spike`.

**What it provides:**
- `native_window(&Window) -> u32`;
- `focus_window(xid)` (`SetInputFocus`, `RevertTo::Parent`);
- `set_transient_for(child, parent)`;
- `ActiveWindowWatcher` and `PointerWatcher` (watcher thread);
- `capture(xid) -> image::RgbaImage`;
- `top_level_at(screen_point)` for cross-window drags;
- startup notification (`activation.rs`): send `_NET_STARTUP_INFO_BEGIN` when launching another profile, and honour a forwarded `DESKTOP_STARTUP_ID` when raising a window.
  - GPUI's `activate()` sends `_NET_ACTIVE_WINDOW` with `CurrentTime` [V], which the window manager may refuse; the startup id gives it a timestamp [I].

**Pointer-focus delivery (the X11 rule).** When the keyboard focus is a window F and a key event's normal target (the window under the pointer) is F or one of F's inferiors, X reports it normally, that is, to the window under the pointer. Only otherwise does it go to F (X11 protocol, `SetInputFocus`).

- The CEF child is an inferior of GPUI's top-level, and Chromium's `X11Window` selects key events [I].
- So typing in the omnibox **while the mouse rests over the page** would reach Chromium. CEF PR #4120 describes exactly this: "Mouse crossing into Cef browser view will take the keyboard focus".
- GDK solved this with a per-top-level 1×1 InputOnly "focus window" that is not an ancestor of embedded children [I, from memory of GTK3's `gdkwindow-x11.c`].

**Patch plan.** Carried only if the spikes demand it, as a vendored copy of `gpui-pre-linux` 0.3.9 under `third_party/` plus `[patch.crates-io]`. It is a single line to drop later.

| Patch | Need | Size |
|---|---|---|
| **G1 focus proxy:** each X11 top-level creates an InputOnly child at (-1,-1) that selects KeyPress, KeyRelease and FocusChange. `SetInputFocus` targets the proxy when GPUI chrome wants the keyboard; key events from the proxy are dispatched as the top-level's; XIM is bound to the proxy. | if S:2 confirms that keys leak to the page under the pointer [I: likely] | about 80 lines in `x11/window.rs` and `x11/client.rs` |
| **G2 focus detail:** ignore `FocusOut` with detail `Inferior` or `Virtual`, so the window stays "active" while the page has focus. | nice to have; `ActiveWindowWatcher` already covers activation without it | about 5 lines |
| **G3 popup focus:** `focus: true` for `PopUp` calls `SetInputFocus`. | no; `platform::x11::focus_window` does it from outside | — |

Without G1, the fallback is our own proxy window, created on our connection as a child of the GPUI top-level. Its KeyPress events would be re-dispatched into GPUI with `Window::dispatch_event(PlatformInput::KeyDown(…))` [V public API], losing XIM composition. That is acceptable only for the omnibox's ASCII path, so G1 is preferred.

---

## 2. Feature map

Every item of each inventory checklist is mapped to a module of `crates/vsesvit-gpui` (paths relative to `src/`). Related checklist lines are merged into one row.

The default is KEEP (reproduce exactly). Notes say when Chromium changes or simplifies the item. "Engine" names the `vsesvit-engine` call or event (§4).

### 2.1 Window, tabs, groups, toolbar (window.md §29)

| Feature | Module | Notes |
|---|---|---|
| Window per profile process; fixed `Browsing`; window id from 1; 1280×820 | `window/mod.rs` (`BrowserWindow::new`), `state/windows.rs` (`next_window_id`, `open_window`) | `app_id` `dev.mrquantumoff.vsesvit` for *all* profiles (fixes the GTK `.Profile<hash>` mismatch); X11 icon from the packaging PNG |
| Creation sequence (layout prefs, bookmarks bar, extension actions, profile button, downloads button if started this session, update banner, first-window sync scheduling) | `window/mod.rs::new` | extension `windows.onCreated` is Chromium's (its popup Browsers); no shell work |
| Close: last normal window saves the session; tabs discarded; window removed after its browsers close | `window/close.rs`, `state/session.rs` | §1.6 close ordering |
| Window closes itself when its last tab leaves (not mid-transfer, not mid-close) | `window/close.rs::close_if_empty` | |
| Last private window ends the private session (downloads cancelled, private closed stack cleared, `Profile::end_private_session`, private context dropped) | `state/private_session.rs` | engine `end_private_context()` |
| Activation: MRU tick, `profile_used()`; extension focus events | `window/mod.rs` (from `ActiveWindowWatcher`), `state/windows.rs` | extension focus events are Chromium's own |
| Resize, minimize, maximize reported to extensions | — | dropped as shell work: Chromium sees its own popup Browsers (`cef-extensions.md` §2.3) |
| Header layout (Back, Forward, Reload/Stop, Home, sidebar toggle, private icon, address bar, extension actions and puzzle, New Tab, Downloads, Profile, Main menu, toggle last in the right layout) | `toolbar/mod.rs` | window controls at the far end when decorations are client-side (`window/frame.rs`) |
| Top bar order (header, banner, tab strip, bookmarks bar, find bar); all hidden in fullscreen | `window/layout.rs`, `window/fullscreen.rs` | |
| Compact address bar (≤ 720 px, centred within 40 px of the window centre) vs full width; full-URL mode | `toolbar/mod.rs` (centring), `omnibox/mod.rs` | |
| Private styling (dark top bars, "Private browsing" icon, " (Private)" title, no profile button) | `theme/mod.rs` (`private_chrome()`), `toolbar/mod.rs`, `window/mod.rs::title`, `toolbar/profile_button.rs` | |
| Window title ("Vsesvit", page title, " - {profile}" with 2+ profiles, " (Private)") | `window/mod.rs::title` | core `profiles::with_profile`; `display_title` uses core `address::readable_url` (WEBKIT-ONLY `uri_for_display` dropped) |
| Layouts left/right/top from synced `tabs.position`, live in every window, including after sync | `window/layout.rs` (`Layout::for_position`) | |
| Sidebar 180–300 px (20%); hidden below 720 px; toggle "Show Tabs"; Ctrl+S / F9 | `window/layout.rs`, `toolbar/mod.rs`, `window/actions.rs` | narrow mode pushes the page (D4) |
| Layout probe and geometry classification (self-test `tab_layout`) | `window/layout.rs::{layout_probe, classify}` | bounds recorded in prepaint; 0.5 px slack |
| Reload ↔ Stop; Back and Forward enabled from history | `toolbar/mod.rs` | `TabChanged::{Loading, History}` |
| Address bar follows the tab (URL, security, progress, zoom, star, in-use); unsubmitted text kept per tab | `omnibox/mod.rs` | |
| Focus on switch: address bar if typed text or blank tab, else page | `window/mod.rs::selection_changed`, `engine_bridge/focus.rs` | |
| Selecting wakes a sleeping tab; records `last_active_ms` | `tab/sleep.rs`, `window/mod.rs::selection_changed` | |
| Main menu, five sections, zoom row (−, "100%" reset, +) following the tab | `toolbar/main_menu.rs` | mnemonic underscores dropped (D13) |
| Profile button and menu; launch other profiles with a startup id; error toast | `toolbar/profile_button.rs`, `profiles/launch.rs` | |
| Every `win.*`/`app.*` action with keymap-driven accelerators | `app/actions.rs`, `engine_bridge/keymap.rs`, `window/actions.rs` | |
| Built-in Ctrl+Tab / Ctrl+PgDn and Ctrl+Shift+Tab / Ctrl+PgUp skipping hidden tabs with wraparound | `window/actions.rs`, `tab/model.rs::next_shown` | |
| Ctrl+1..9 and other tab-view keys | `engine_bridge/keymap.rs` | bind core's `SelectTab1..8`/`SelectLastTab`; Ctrl+Shift+PgUp/PgDn move the tab (D1) |
| New tab page loaded as HTML that reads as blank (`about:blank`, "New Tab", not saved); private page | `tab/content.rs` | engine `TabSpec{page: InitialPage::Html{reads_as_blank: true}}` [S:9] |
| Opening tabs next to the opener (Chrome placement), in its group; middle/Ctrl+click background, with Shift foreground | `tab/model.rs::insert(Insert::AfterOpener)`, `window/mod.rs::open_from_page` | `BrowserEvent::OpenUrlRequested{disposition}`; Shift+click alone is Chromium's NEW_WINDOW (D2) |
| `chrome.tabs.create` with index; `open_tabs` | `window/mod.rs::open_tabs` | extension tab creation is Chromium's (opens a Chrome-UI window); see `ForeignWindowAdopted` [S:13] |
| Home button: homepage as a Bookmark-transition visit, else the new tab page with the address bar focused | `window/actions.rs::home`, `toolbar/mod.rs` | |
| Pin and unpin (pinned lead; unpinning goes first among unpinned; saved) | `tab/model.rs::set_pinned` | |
| Mute and unmute (tab menu, indicator click) | `tabs_ui/row.rs`, `tabs_ui/tab_menu.rs` | engine `set_audio_muted` |
| Indicator priority (capture + description; muted "Unmute Tab"; playing "Mute Tab"; group dot on the top strip) | `tabs_ui/row.rs::indicator` | "playing" needs `TabEvent::AudioStateChanged` [S:14] |
| Sleeping tab: faded icon, kept title | `tabs_ui/row.rs` (`.opacity(0.5)`), `tab/sleep.rs` | |
| Closing a tab: closed-stack entry per kind (25, keyed, position, pinned, group, MRU tick); next selection via opener and groups; save when the last tab of the last normal window closes | `window/close.rs::close_tab`, `state/closed_tabs.rs`, `tab/model.rs::select_after_closing` | no back/forward state (CEF cannot restore it; D14) |
| Reopen closed tab at its position (pinned block respected), selected, back in its group | `window/mod.rs::restore_closed` | loads the URL only (D14) |
| Move a tab within a window or to another window (state kept) | `tab/model.rs::move_to`, `window/mod.rs::transfer_tab` | engine `reparent` [S:10] |
| Copy link (clean or whole) with the "Link copied" toast | `window/actions.rs`, `overlay/toast.rs` | core `clean_url::clean` |
| Vertical tab list: Search Tabs button, New Tab button, rows (favicon or spinner, title with tooltip, indicator, close or pin) | `tabs_ui/sidebar.rs`, `tabs_ui/row.rs` | |
| Tab list: middle-click close; right click, Shift+F10 and Menu open the tab menu | `tabs_ui/sidebar.rs` | |
| Tab list drag-and-drop reorder (upper or lower half) | `tabs_ui/sidebar.rs`, `tabs_ui/drag.rs` (`reorder_target`) | GPUI `on_drag`/`on_drop`; a drop indicator is added (GTK had none) |
| Row grow-in and shrink-out (180 ms, ease-out cubic, only when shown, reduced motion); closing never waits | `tabs_ui/sidebar.rs`, `theme/motion.rs` | `cx.reduce_motion()` [V] |
| Group headers (chip and arrow; click, Enter, Space collapse; right click, Shift+F10, Menu edit; accessible name and expanded state); 3 px colour bar; collapsed rows hidden | `tabs_ui/sidebar.rs` | AccessKit role and expanded state through GPUI accessibility [I] |
| Tab list keyboard over shown rows only; Tab/Shift+Tab reach headers; bell at the ends | `tabs_ui/sidebar.rs` | |
| Top strip: tabs (title, icon, spinner, indicator, close), pinned compact, drag reorder, drag between windows, drag out to a new window, middle-click, context menu | `tabs_ui/strip.rs`, `tabs_ui/drag.rs` | cross-window drag through `platform::x11::top_level_at` [S:10] |
| Top strip: group chips (click opens the editor), Search Tabs button, colour dots; collapsing *hides* the group's tabs on the strip | `tabs_ui/strip.rs` | GTK could not hide them (window §12); GPUI can |
| Tab context menu (Chrome's items and order; "to the Right" vs "Below"; toggles; Copy Link only with a link; group submenu; enablement from `TabPlace` and the closed stack) | `tabs_ui/tab_menu.rs` | `overlay::menu` |
| Tab menu actions (New tab next, Move to new window, Reload, Duplicate, Close, Close others/after, Reopen closed) | `window/actions.rs::tab_op`, `tab/model.rs` | Duplicate loads the URL only (D14) |
| Tab search popover (entry "Search tabs"; headings "Open tabs", "Recently closed"; icon, title, site; "No results found"); ranking, MRU, same kind across windows, wrap, choose, toggle; anchor fallbacks | `tabs_ui/tab_search.rs`, `state/tabs.rs::{search_tabs, go_to_tab}` | core `tab_search::rows` |
| Tab groups: core `TabGroups` with `WindowTabs` per call; `Step`s applied with settling held; settle after every change | `tab/model.rs` | |
| Group actions (toggle, edit, new tab in group, ungroup, close group); Add to new group opens the editor | `window/actions.rs::group_op`, `tab/model.rs` | |
| Group editor (name entry, live; nine swatches; New Tab in Group, Ungroup, Close Group; anchors) | `tabs_ui/group_editor.rs` | |
| Group colours from `GroupColor::rgb(dark)`; chip text white or #202124; restyled on theme change | `theme/colors.rs` | |
| Groups saved per tab and restored after selection; private groups never saved | `state/session.rs` | |
| Selecting a hidden tab expands its group; collapsing the selected tab's group moves the selection | `tab/model.rs` | |
| Extension toolbar, buttons, click, drag reorder, right-click menu, puzzle menu, commands | `extensions/toolbar.rs`, `extensions/popup.rs` | data source and popups [S:12, S:13]; §2.7 |
| Private windows (entry points, page, isolation, kind kept, sent back on cross-kind drop, nothing saved) | `state/private_session.rs`, `window/mod.rs::send_back`, `tab/content.rs` | a CEF browser's request context is fixed at creation |
| Fullscreen F11 hides the top bars; page notice for 4 s; prompts withheld | `window/fullscreen.rs` | [S:7] |
| Permission prompt scheduling | `omnibox/permission_prompt.rs` | §2.4 |
| `window.open` / `_blank` → foreground tab next to the opener, in its group and session; discarded if closed before showing | `engine_bridge/hooks.rs::popup_policy`, `window/mod.rs::adopt_popup` | `ShellHooks::popup_policy` returns `AllowAsTab`; `window.opener` works because CEF creates the browser |
| Downloads button and warning popover | `toolbar/downloads_button.rs` | |
| Update banner (title, button) | `toolbar/update_banner.rs` | |
| Bookmarks bar visibility, star state, bookmark bubble, site-info anchoring | `bookmarks/bar.rs`, `omnibox/mod.rs`, `bookmarks/editor.rs`, `omnibox/site_info.rs` | |
| Toasts for window messages | `overlay/toast.rs` | |
| Engine hooks (title, favicon, loading, progress, commit, history, zoom, find, print, DevTools, mute, capture, audio, fullscreen, popups, private context) | `tab/mod.rs::apply` | §4 |
| Split view and media player | — | none on Linux in v1 (D15) |

### 2.2 Shell core: startup, CLI, profiles, session, keymap (shell-core checklist)

| Feature | Module | Notes |
|---|---|---|
| `main` returns the shell's `ExitCode`; other OSes print the message | `crates/vsesvit/src/main.rs` | |
| CEF sub-processes handled before CLI parsing | `crates/vsesvit/src/main.rs` → `vsesvit_cef::run_subprocess` | |
| `self-test` default feature → `vsesvit-core/testkit`; `--no-default-features` | `crates/vsesvit-gpui/Cargo.toml` | |
| GTK, adw, webkit, webext dependencies and CI packages removed | workspace `Cargo.toml` (U19 switch-over) | the CI `apt` line is coordinated with packaging |
| `APP_ID` as WM_CLASS, default icon, app name "Vsesvit" | `window/mod.rs` (`WindowOptions{app_id, icon}`), `app/mod.rs` (`cx.set_app_identity`) [V API] | |
| CLI grammar, errors, USAGE verbatim, exit codes | `boot/cli.rs` | verbatim port with tests |
| `--check-for-updates`, `--update` | `boot/headless.rs` | no display, no engine |
| `--self-test` flow, scripted mode, profile list in OUT_DIR, fixture server, `probe.crx` | `self_test/mod.rs` | |
| Built without `self-test`: message and failure | `boot/mod.rs` | |
| Profile resolution (create and canonicalize `--profile-dir`; sweep; last used, Default, first, picker) | `boot/profile_start.rs` | port of `profile.rs` |
| Picker condition (more than one profile, `show_picker`, no URLs, none running) | `boot/profile_start.rs` (core decides) | |
| `--profile-dir` inside the list = listed profile; outside = no profile button | `boot/profile_start.rs` (`Home`) | |
| Instance identity per profile (any spelling of the path) | `boot/instance.rs` (socket name from the canonical root) | replaces GApplication ids |
| Bad profile dir; open errors; removed at startup | `boot/mod.rs` | exact messages |
| Second launch forwards argv and cwd and exits with the owner's status; unreachable owner message | `boot/instance.rs` | |
| Targets resolve against the invoker's cwd; `vsesvit: cannot open X` to the invoker | `state/invocation.rs` | port of `location::resolve_cli_target` (core `classify_url`) |
| No window before blockers apply and clear-on-start data is deleted | `state/mod.rs::start` → `privacy::apply_all` before `open_startup_windows` | trivial with an IO-thread filter; the ordering stays |
| Invocation with no normal window → startup windows; with windows and no URLs → raise; with URLs → new normal window | `state/invocation.rs` | |
| Invocation on a removed profile closes it without saving | `state/invocation.rs` | |
| Pages from outside never open in private windows | `state/windows.rs::normal_window` | |
| Startup windows (restore, homepage or NTP; targets without a blank tab) | `state/session.rs::open_startup_windows` | |
| Welcome on first run, once, over the first normal window | `state/mod.rs`, `dialogs/welcome.rs` | |
| App actions (new window, private window, about, welcome, shortcuts, update, quit) | `app/actions.rs`, `window/actions.rs` | the extension notification action is dropped (Chromium's notifications) |
| Quit (Ctrl+Q, no menu item) saves the session first | `app/quit.rs` | |
| Shutdown: final session save, clear-on-exit data, final sync; skipped for a removed profile | `app/quit.rs` | |
| Restart into an update releases the profile and CEF before spawning; AppImage relaunches `$APPIMAGE` | `app/quit.rs::finish` | |
| Removed current profile: launch the next, quit without saving, delete after release | `profiles/manage.rs`, `app/quit.rs` | |
| Debug-only tools (screenshot, apply-sync, set-tabs-position, close-dialog) | `boot/instance.rs` (debug builds accept `{"debug": …}` lines on the socket) | replaces `org.gtk.Actions` |
| Menus and popovers clean up when their anchor goes away | `overlay/mod.rs` | |
| Animation values (180 ms, ease-out cubic, reduced motion) | `theme/motion.rs` | |
| Logging, private addresses kept out, CEF log routed | `boot/logging.rs`; CEF log file in `EngineConfig` | |
| Picker window ("Who's Using Vsesvit?", cards, Add, "Show on startup", 760×520) | `profiles/picker.rs` | |
| Picker launches and closes; failure toast; never opens a profile | `profiles/picker.rs` | |
| Avatar (picture or coloured initial; 22 and 72 px; nine colours) | `theme/avatar.rs` | |
| Add and Edit Profile dialog | `profiles/dialogs.rs` | Dialog window from a browser window |
| Add Profile creates and opens in its own process | `profiles/launch.rs` | |
| Manage Profiles (rows, Open, Edit, Remove, confirmation text, live refresh) | `profiles/manage.rs` | |
| Removing another running profile notifies it | `profiles/launch.rs::notify` | |
| Profile list re-read on activation and menu open | `state/mod.rs::profile_used` | |
| Settings > General picker switch | `dialogs/settings/general.rs` | |
| Window titles | `window/mod.rs::title` | |
| Sync sign-in names and pictures the profile | `sync.rs::take_account_details` | |
| Window list in focus order drives the session and placement | `state/windows.rs` (MRU from `ActiveWindowWatcher`) | |
| Private session start and end | `state/private_session.rs` | engine `begin_private_context`/`end_private_context` |
| Two closed stacks; about:blank and window closes not kept | `state/closed_tabs.rs` | |
| Reopen closed tab rules | `window/mod.rs::restore_closed` | URL only (D14) |
| Tab search over same-kind open and closed tabs | `state/tabs.rs::search_tabs` | |
| MRU clock ticks | `state/windows.rs`, `state/tabs.rs` | |
| History visit on every main-frame commit (same-document included); none for error pages or private tabs | `state/history_hooks.rs::navigation_committed` | `TabEvent::Committed{kind, transition}` |
| History titles never empty, never an error page's | `state/history_hooks.rs::title_changed` | the WebKit reason disappears; the rule stays |
| Site zoom on each new document; per site; private in memory; level ↔ factor | `state/history_hooks.rs::show_at_site_zoom`, `tab/mod.rs` | engine `set_zoom(Zoom(factor))`; the engine converts to Chromium's log level; fighting Chromium's per-host map [S:14] |
| Star / Ctrl+D | `bookmarks/editor.rs::star_clicked` | |
| Favicons stored only for bookmarked pages; bars refresh | `favicons.rs` | engine gives PNG ≤ 32 px |
| Favicon preload batches of 32 | `favicons.rs::preload` | |
| Extension toolbar order and pins | `extensions/toolbar.rs` | |
| Omnibox core suggestions, async search suggestions, Enter resolve | `omnibox/suggestions.rs`, `omnibox/mod.rs` | |
| Live prefs (tabs position, bookmarks bar, home button, compact bar, full URLs) | `state/prefs.rs` → `BrowserStateEvent::PrefsChanged` | windows react |
| Engine prefs (pop-up blocking, smooth scrolling, hardware acceleration, spell check and languages) | `state/prefs.rs` → engine `apply_prefs`; startup switches | smooth scrolling and hardware acceleration need a restart (D7) |
| Tracking protection and third-party cookies apply at once | `privacy/trackers.rs`, `privacy/cookies.rs` | |
| Theme system/light/dark for chrome and web content | `theme/mod.rs`; engine `apply_prefs{color_scheme}` | `set_chrome_color_scheme` [I] |
| Updates (automatic switch, channel, banner, status, 30 s then daily) | `updates/mod.rs` | |
| Homepage pref semantics | `window/actions.rs::home` (core `homepage_url`) | |
| Sync refresh matrix | `state/sync_applied.rs` | |
| Session save throttle 2 s; never without a normal window or after shutdown | `state/session.rs::SessionSaver` | |
| Session saved at last-tab and last-window close | `window/close.rs`, `state/session.rs` | |
| Snapshot and restore rules | `state/session.rs` | `restore_state = None`; old WebKit blobs ignored |
| Old WebKit restore blobs ignored safely | `state/session.rs` | |
| Memory Saver sweep, unsaved-input check, edit tracking, re-check; asleep keeps title, faded icon, URL | `state/memory_saver.rs`, `tab/sleep.rs` | sleep = close the CEF browser and keep the `Tab`; wake = recreate it at the URL (back history lost, D14); unsaved input via engine `eval`; edits via a document-start script |
| `chrome.tabs`/`chrome.windows` semantics, extension cookie blocks, permission prompts over the requesting window | — | Chromium's own; Vsesvit-specific rules (committed URL only, Block-site cookies) only where hooks allow [S:13] |
| Extensions loaded, reconciled, update-checked; load errors shown | `extensions/controller.rs` | engine hand-off [S:12] |
| Keymap: commands bound with core's defaults; built-ins fixed | `engine_bridge/keymap.rs` | |
| Ctrl+S toggles the sidebar; Save Page Ctrl+Shift+S; Ctrl+Shift+C clean link | core defaults | |
| Shortcut edits through one path; "taken from" reporting | `state/prefs.rs::edit_keymap` | |
| Shortcuts suspended during capture; capture key rules | `dialogs/settings/capture.rs`, `engine_bridge/keymap.rs::suspend` | GPUI `KeyDownEvent` → core `Key` mapping replaces GDK keyvals |
| Extension command shortcuts resolved by core and dispatched | `engine_bridge/keymap.rs` (display); Chromium dispatch [S:13] | |
| Browser shortcuts work while the page has focus | `engine_bridge/keymap.rs::routing_for` + engine `KeyRouting` | [S:6] |
| In-app window capture including web content | `platform/capture.rs` | [S:11] |
| Appendix A visual values in the GPUI theme | `theme/tokens.rs` | §3.3 |

### 2.3 The tab and the engine (tab-engine §29)

| Feature | Module | Notes |
|---|---|---|
| Tab identities (`TabId`, session UUID, fixed `Browsing`, `last_active_ms`, MRU `used`) | `tab/mod.rs` | `TabId` from `vsesvit_engine::TabId::next()` |
| Change notifications (Title, Favicon, Loading, Progress, Committed{Document, SameDocument, ErrorPage}, History, Zoom, Find, Capture, Audio, Sleep); suppressed while asleep | `tab/mod.rs` (`TabChanged`) | |
| `display_title`, `session_uri`, `is_blank`, readable URLs | `tab/mod.rs` | core `address::readable_url` |
| Security state per tab (error-page overrides) | `tab/mod.rs::security` | `TabEvent::SecurityChanged(Option<SslInfo>)` |
| Persistent engine data in the profile; one in-memory private context | `boot/mod.rs` (`EngineConfig`), `state/private_session.rs` | |
| Certificate errors fail the load, no bypass | engine (cancels in `OnCertificateError`) + `tab/error_page.rs` | |
| Pop-up blocking live (no gesture → block) | `tab/navigation.rs::popup_decision` via `ShellHooks::popup_policy` | |
| Smooth scrolling and hardware acceleration prefs | `boot/mod.rs` startup switches | restart note (D7) |
| Spell check on/off and languages | `state/prefs.rs` → engine `apply_prefs{spellcheck}` | Chromium dictionaries (D10) |
| Engine prefs re-applied after Settings and sync changes | `state/prefs.rs`, `state/sync_applied.rs` | |
| Load, reload, bypass cache, stop, back, forward, home | `tab/mod.rs`, `window/actions.rs` | |
| Reload/Stop swap; back/forward enablement | `toolbar/mod.rs` | |
| Progress line driven by load progress; reset when the load ends | `omnibox/progress.rs` | `TabEvent::LoadProgress` |
| A load ending without a commit shows the committed URL again | `omnibox/mod.rs` | |
| Transition bookkeeping (Typed, Bookmark, Reload, default Link); stopped loads and downloads leave none; typed-over-load counts as typed | `tab/mod.rs` (`pending_transition`) | Chromium also reports a transition; core's mapping stays |
| History visit on every non-error commit (same-document included); titles non-empty, non-error | `state/history_hooks.rs` | |
| Same-document commits update URL, history, star, permissions without resetting zoom | `tab/mod.rs`, `state/history_hooks.rs` | `CommitKind::SameDocument` from `OnAddressChange` without `OnLoadStart` (heuristics WEBKIT-ONLY, dropped) |
| Web pages cannot reach non-web-accessible extension pages; browser loads can | Chromium | WEBKIT-ONLY gate dropped |
| Windows a page opened stay judged by it | Chromium | dropped |
| Pages cannot open or embed `view-source:`; browser loads can | Chromium natively [I]; `tab/navigation.rs` refuses renderer-initiated `view-source:` as a backstop | |
| Ctrl+click and middle-click → background tab next to the page; with Shift, foreground | `window/mod.rs::open_from_page` | `OpenDisposition` |
| New tabs from clicks reported as opened by the page and joining its group | `tab/model.rs::insert(AfterOpener)` | |
| Non-renderable responses become downloads | Chromium → `BrowserEvent::DownloadRequested` | the tab clears its pending transition on `DownloadRequested{tab}` |
| HTTPS-only: upgrade, main frame only before the request goes out, warning at the http URL, Continue stores the exception, warning marked Insecure | `tab/navigation.rs` (`Upgrades` per tab) via `ShellHooks::navigation_policy`; `tab/error_page.rs` | `is_main_frame` known (WEBKIT-ONLY guessing dropped) |
| Error pages (load failed, certificate, crashed) under the failed URL, Try Again, no history | `tab/error_page.rs` | engine `load_html(tab, html, Some(failed_url))` [S:9] |
| `window.open` → foreground tab next to the opener keeping `window.opener`; features ignored; discarded if closed before showing; `window.close()` closes the tab | `window/mod.rs::adopt_popup`; `TabChanged::ClosedByPage` | Chrome-style `window.close()` arrives as an unrequested `TabEvent::Closed` |
| Opener and opened tabs marked related | `tab/mod.rs` | |
| Link hover preview | Chromium status bubble [S:8]; fallback `overlay` + `TabEvent::StatusText` | |
| Page fullscreen, notice, Esc; F11; bars hidden; prompts withheld | `window/fullscreen.rs` | [S:7] |
| Audio indicator and mute toggle | `tabs_ui/row.rs` | [S:14] |
| Capture indicators; sharing bar "Sharing your screen with {host}" + "Stop sharing" | `tabs_ui/row.rs`, `window/mod.rs::sharing_bar` (Layout), `privacy/permissions.rs` | "Stop sharing" has no CEF API: reload the page, as Windows does (omnibox-privacy §4.9) |
| Notifications shown only when allowed | `privacy/permissions.rs` (content-settings mirror) | Chromium shows them |
| Memory Saver (sweep, unsaved input, edit tracking, faded icon, wake at the same page) | `state/memory_saver.rs`, `tab/sleep.rs` | back history lost on wake (D14) |
| Back/forward saved per tab and restored | — | CEF gap (D14); `restore_state` left empty |
| JS dialogs, beforeunload, file chooser, HTTP auth, `<select>`, colour and date pickers | Chromium native | §2.9 |
| Print (Ctrl+P, menu, context menu, `window.print()`); print to PDF for the self-test | `window/actions.rs::print` → engine `print`; `print_to_pdf` | Chrome print preview [S:15] |
| DevTools toggle, JavaScript console, Inspect Element | `window/actions.rs` → engine `show_devtools`/`close_devtools`/`show_console` | Chromium's DevTools window (D12); Inspect in Chromium's menu |
| Context menu (engine defaults plus our items: Open Link in Private Window, Copy Link Without Tracking, selection search, Print…, View Page Source; Add to Dictionary; extension items) | `engine_bridge/hooks.rs::context_menu_items`, `window/actions.rs::context_menu_command` | Chromium's native menu already has link, image and spelling items and extension items [V `cef-extensions.md` §4]; D11 |
| Zoom presets, menu row, chip, bubble, per-host memory, Ctrl+wheel detection | `toolbar/main_menu.rs`, `omnibox/zoom_bubble.rs`, `tab/mod.rs`, `state/history_hooks.rs` | `TabEvent::ZoomChanged` [S:14]; `BrowserSettings.chrome_zoom_bubble` off |
| Find bar (entry, Previous/Next, status, close; Ctrl+F select-all and search; live, case-insensitive, wraparound; status wording; Esc returns focus; follows tab switches) | `find_bar.rs` | engine `find`/`stop_finding`; `TabEvent::FindResult{matches, active}`; no 1000 cap (D5) |
| View page source (Ctrl+U, menu, context menu → `view-source:<page>` tab next to the page; http/https/file only) | `window/actions.rs::view_source` | Chromium renders it; scheme, loader and renderer WEBKIT-ONLY, dropped |
| Save Page As (dialog in the downloads folder; MHTML named after the title; other documents as served; format fixed when the dialog opens; toasts) | `save_page.rs` | portal save dialog (`platform/portal.rs`); engine `save_mhtml` (DevTools `Page.captureSnapshot`) or `start_download` + `Downloads::download_to`; MIME from `Committed{mime}` |
| In-app screenshot including page pixels | `platform/capture.rs` | [S:11] |
| Closed-tab stacks, keys, recording rules | `state/closed_tabs.rs` | verbatim port |
| CLI targets and homepage | `state/invocation.rs` | |
| Shortcuts reach the shell while the page has focus | `engine_bridge/keymap.rs` | [S:6] |

### 2.4 Address bar, omnibox, site info, permissions, cookies, tracking protection (omnibox-privacy §11)

| Feature | Module | Notes |
|---|---|---|
| Field: placeholder "Enter address", accessible label "Address", URL purpose, no spell check | `omnibox/field.rs` | gpui-component `Input` + `InputState` [V: `set_text_align`, `set_selected_range`, `select_all`, `InputEvent::{Change, PressEnter, Focus, Blur}`] |
| Centred at rest, left-aligned while focused or editing | `omnibox/field.rs` (`set_text_align`) | |
| Compact width; private icon before the bar | `toolbar/mod.rs` | |
| Resting URL simplified unless focused or full-URLs on; `about:blank` empty; `readable_url` | `omnibox/mod.rs` | core `address::{simplified_url, readable_url}` |
| Focus in → full URL, select all after the click; focus out → simplified; typed text survives | `omnibox/mod.rs` | |
| Error pages show the failed URL; the NTP shows empty | `omnibox/mod.rs` (committed URL) | |
| Security icon and tooltips; error-page overrides; hidden while editing; click → site info | `omnibox/mod.rs` | Lucide `lock` / `lock-open` / `info` |
| In-use button after the security icon (icon per capture kind) → site info | `omnibox/mod.rs` | |
| Zoom chip ("110%", "Zoom") before the star; bubble ("Zoom:", level, Out, In, "Reset") | `omnibox/mod.rs`, `omnibox/zoom_bubble.rs` | |
| Star ("Bookmark This Page" / "Edit Bookmark"); click or Ctrl+D | `omnibox/mod.rs` → `bookmarks/editor.rs` | |
| One bubble at a time; open and close re-sync the prompt | `overlay/bubble_slot.rs` | |
| Progress line (8% start, never back, 200 ms hold, cleared on switch, gradient) | `omnibox/progress.rs` | |
| Reload ↔ Stop | `toolbar/mod.rs` | |
| Edit model: `edited(text, inline_allowed)`; inline only after an insertion at the end | `omnibox/edit.rs` | verbatim port of `Edit`; insertion vs deletion detected by diffing values on `InputEvent::Change` |
| Inline completion selected after the typed text on row 0 | `omnibox/edit.rs`, `omnibox/field.rs` (`set_selected_range`) | |
| Down/Up/Tab/Shift+Tab move the highlight (clamped, no re-query); Up/Down never leave the bar | `omnibox/mod.rs` (key context `Omnibox`) | |
| Escape (row 0, then cancel and focus the page); Right/End accept inline; Enter; Ctrl+Enter `.com`; Shift+Delete forgets history | `omnibox/mod.rs` | core `search::ctrl_enter_url`, `history().delete_url` |
| Ctrl+L / Alt+D / F6 focus and select all | `window/actions.rs::focus_address` | |
| Suggestion list (bar-wide, max 440 px, no focus, no autohide, opens only while focused with rows) | `omnibox/suggestions.rs` | Passive Popup overlay |
| Row layout (icon, title end-ellipsized, dim subtitle middle-ellipsized; single click) | `omnibox/suggestions.rs` | |
| `set_suggestions` resets; `refill_suggestions` keeps highlight, text, caret | `omnibox/edit.rs` | |
| Choosing a row shows the current URL until commit; plain submit keeps the text | `omnibox/mod.rs` | |
| `Omnibox::resolve`, navigate Typed, focus the page; blank → page; error toast | `omnibox/mod.rs` | |
| Per-tab unsubmitted text; focus rules | `omnibox/mod.rs`, `tab/mod.rs::{set_typed, take_typed}` | |
| Core rows (icons, subtitles, transitions); search row; typed search title | `omnibox/suggestions.rs` | port of `omnibox.rs` |
| Open-tab rows (same kind, all windows, not the selected tab, ≤ 4, "Switch to tab · …") | `omnibox/suggestions.rs` | `BrowserState::tabs_matching` |
| Engine search suggestions (`suggest_request`, worker `run`, stale dropped, refill) | `omnibox/suggestions.rs` | background executor |
| Settings switches take effect on the next keystroke | core reads prefs | |
| Site info popover (380 px, scroll max 680, headline, icon, explanation, host) | `omnibox/site_info.rs` | Interactive Popup |
| Section order (tracking protection, cookies, permissions, certificate) | `omnibox/site_info.rs` | |
| Certificate section (valid or problem lines; issued to and by; validity; key; signature; serial; SHA-256; SAN expander; chain) | `omnibox/site_info.rs` | core `certificate::parse_der` on `SslInfo.der_chain`; `CertStatus` → problem sentences |
| Permission request mapping (location, notifications, clipboard, camera+mic as one prompt, screen share, MIDI, pointer lock auto-allowed, device labels, others denied) | `privacy/permissions.rs` | `BrowserEvent::PermissionRequested` with core `Permission`s; which CEF callback fires in Chrome style [S:16] |
| Decision (`decide` + same-document dismissal rule); queue ≤ 8 per tab | `tab/permissions.rs` | port |
| Prompt bubble (core heading, body, buttons; first suggested; 320 px; no focus; 500 ms guard) | `omnibox/permission_prompt.rs` | Passive Popup |
| Prompt only for the selected tab, one at a time, not with another bubble or in fullscreen; withdrawn and restored | `omnibox/permission_prompt.rs` (`PromptScheduler`) | |
| Escape or a click on the page = "Not now" | `omnibox/permission_prompt.rs` | page click seen as `FocusChanged` or `PointerWatcher` |
| Answers applied; Allow only if the same site is still on screen; enforce | `privacy/permissions.rs` | engine `answer_permission` |
| Commit and close rules | `tab/permissions.rs` | |
| `navigator.permissions.query` matches stored settings | `privacy/permissions.rs` | engine `set_site_setting` mirrors into content settings |
| Notifications allowed only when allowed; always blocked in private | `privacy/permissions.rs` | |
| Capture tracking; in-use button; tab indicator; sharing bar | `privacy/permissions.rs`, `omnibox/mod.rs`, `tabs_ui/row.rs`, `window/mod.rs` | `TabEvent::CaptureChanged` |
| Enforce after any settings change | `privacy/permissions.rs::enforce` | stopping a capture: reload (no CEF API) |
| Site-info Permissions section (rows, PiP rule, Stop, Reset) | `omnibox/site_info.rs` | PiP becomes possible with Chromium; until a decision, it stays hidden |
| Private overlay for permissions | `privacy/permissions.rs` (core `site_permissions_in(Private)`) | |
| Third-party cookie policy (normal and private contexts) | `privacy/cookies.rs` → engine `apply_prefs{cookie_controls}` | Chromium pref `profile.cookie_controls_mode` [I] |
| Site cookie Block (no cookies sent or stored, `document.cookie` hidden, existing deleted) | `privacy/cookies.rs` | `RequestFilter` cookie access + `set_document_start_scripts(core block_script)` + cookie deletion |
| Clear on exit / Block data deleted at start and exit (3 s bound) | `privacy/cookies.rs` | engine `delete_site_data` |
| Site-info cookies section (http(s) only, not private, choices, status, reload from URL) | `omnibox/site_info.rs`, `privacy/cookies.rs` | per-site Allow now offered (D17) |
| Settings third-party radios; See All Site Data | `dialogs/settings/privacy.rs`, `dialogs/settings/site_data.rs` | site list from cookies (`engine.cookie_sites`), as Windows does |
| Private tabs follow stored Block rules and tracking protection | `privacy/*` (filter applies to both contexts) | |
| Tracking protection levels, bundled list, exceptions; reaches every tab and view-source; reload after a change | `privacy/trackers.rs` | `RequestFilter` snapshot (`TrackerList` + level + allowed origins), IO thread |
| Site-info tracking protection switch | `omnibox/site_info.rs` | |
| Blocked tracker count in the subtitle | `privacy/trackers.rs` (counter in the filter) | now possible (Windows parity) |
| No restored or command-line page before rules apply | `state/mod.rs::start` | |
| Sync re-applies enforce, trackers, cookies, address-bar prefs | `state/sync_applied.rs` | |
| Self-test hooks for the address bar | `omnibox/mod.rs` (`#[cfg(feature = "self-test")]` probes: `observe`, `type_text`, `press`, `click_security`, …) | |

### 2.5 Bookmarks bar, menus, editor, drag, favicons, downloads (bookmarks-downloads §10)

| Feature | Module | Notes |
|---|---|---|
| Bar placement, visibility pref, slide reveal, all windows, sync | `bookmarks/bar.rs`, `window/layout.rs` | reveal animated with `theme::motion` |
| `win.show-bookmarks-bar` toggle, Ctrl+Shift+B, menu, context menus, Settings | `window/actions.rs`, `toolbar/main_menu.rs`, `bookmarks/menu.rs` | |
| Items (favicon or generic icon + 18-char title; tooltip URL; untitled → icon only, accessible name URL); folder buttons; separators; URL-less nodes hidden | `bookmarks/bar.rs` | |
| Click opens in the tab (Bookmark transition, page focus); middle-click foreground new tab | `bookmarks/bar.rs` | actions `OpenBookmark`, `OpenInNewTab` |
| Overflow `fitting` exactly; 1 px spacing; RTL; chevron "»" ("More Bookmarks"); never widens the window | `bookmarks/bar.rs` | port `fitting` + tests; measure item natural widths in prepaint |
| Empty label; "Other Bookmarks" button | `bookmarks/bar.rs` | |
| Refresh on change; icon-only changes update in place | `bookmarks/bar.rs` | keyed elements by node id |
| Context menus on items, bar space, Other Bookmarks | `bookmarks/menu.rs` | Popup |
| Folder menus (rows, submenus beside, 75% monitor height, 40-char labels, link and folder rows, hover and keyboard rules, `(Empty)`, depth 12) | `bookmarks/menu.rs` | each level its own Popup; monitor height from `cx.displays()` [V] |
| Link, separator, folder and bar context menus with exact items | `bookmarks/menu.rs` | |
| Actions close all bookmark menus first; delete-folder confirm; Add Page prefilled; Add Folder prompt; insertion at the end; Open All; Copy Link; Bookmark Manager | `bookmarks/menu.rs` | prompts as Dialog windows (`dialogs/prompts.rs`) |
| Star follows the committed URL; Ctrl+D adds or edits | `bookmarks/editor.rs` | |
| Bubble (heading, close, Name/URL/Folder, Remove, Done; 340 px; name selected; Enter = Done) | `bookmarks/editor.rs` | Interactive Popup through `BubbleSlot` |
| Editor dialog ("Edit Bookmark" / "Rename Folder" / "Add Page"; folder list with 16 px indent, depth 64, own subtree excluded, Mobile rule; `classify_url` validation; save semantics) | `bookmarks/editor.rs` | Dialog window from a browser window; in-window from the Bookmarks window |
| Error toasts with exact texts | each module, through `overlay/toast.rs` | |
| Drag on the bar (zones, before/after/into feedback, destination arithmetic, one `move_to`) | `bookmarks/drag.rs`, `bookmarks/bar.rs` | port `zone`, `destination` + tests |
| Drag in the Bookmarks window and between bar and window | `dialogs/bookmarks.rs` | cross-window GPUI drag not supported [V gpui.md §2.5]; the payload is the node id, so a drop in the other window is detected with `platform::x11::top_level_at` (provisional) |
| Favicons from core (page, else site); record from the engine; never private; store after the star; preload; faded when asleep | `favicons.rs` | `TabEvent::FaviconChanged(png)`; `gpui::Image::from_bytes(ImageFormat::Png, …)` |
| Downloads button (hidden until a listed download starts this run) | `toolbar/downloads_button.rs` | |
| Toasts (start "Downloading “…”" + Show; done "“…” downloaded" + Open; failed; refusals) | `downloads/controller.rs` | |
| Destination (folder pref or XDG Downloads; created; numbered free names; reservations) | `downloads/controller.rs` | engine `continue_download(id, path)` |
| Ask where to save (portal save dialog "Save File"; cancel → cancel) | `downloads/controller.rs`, `platform/portal.rs` | |
| Dangerous types held as `.unconfirmed`; warning popover (no focus, no autohide) with Keep / Discard / close | `downloads/warning.rs` | Passive Popup |
| Non-UTF-8 path refused with its toast | `downloads/controller.rs` | |
| Live counts, 250 ms progress gate, list notifications; `interrupt_stale` | `downloads/controller.rs` | `BrowserEvent::DownloadUpdated` |
| Change feed for the Downloads window; list limit 500 | `downloads/controller.rs` | GPUI events instead of `subscribe` |
| `download_to` (Save Page As non-HTML) | `downloads/controller.rs` | match in `DownloadRequested` by URL |
| Private downloads (in memory, private list only, cancelled at session end, files kept) | `downloads/controller.rs` | |
| Open file, show in folder | `downloads/controller.rs` | `cx.open_with_system`, `cx.reveal_path` [V] |
| Private paths never logged | `downloads/controller.rs` | |
| CEF mapping (`CanDownload` must return true; `OnBeforeDownload` + `Continue`; `OnDownloadUpdated`; callbacks) | `vsesvit-cef` (contract in §4) | the cef-rs `can_download` default refuses [V `bookmarks-downloads.md` §7.6] |
| Pause and resume | `downloads/controller.rs`, `dialogs/downloads.rs` | new (D6) |
| Clear the tab's pending transition when a navigation becomes a download | `tab/mod.rs` (on `DownloadRequested{tab}`) | |

### 2.6 Dialogs and library windows (dialogs §18)

| Feature | Module | Notes |
|---|---|---|
| Library windows single-instance; close with their opener; separate private Downloads | `dialogs/library.rs` | |
| Library layout (header buttons, clamped search ≤ 560 px, toast area, 640×680) | `dialogs/library.rs` (`LibraryFrame`) | gpui-component `TitleBar`, `Input` |
| Plain-text toasts and labels | everywhere (GPUI text is never markup) | the GTK source-scan test becomes moot |
| Text prompt, choice prompt, destructive confirm | `dialogs/prompts.rs` | |
| Visit times `%x %H:%M` | `util/time.rs` | locale-aware formatter (no `glib::DateTime`) |
| Views hold the browser weakly; closing releases the lock | all | test kept in `state/` |
| Settings frame (1040 px, six pages `general`…`shortcuts`, live, follows sync, never writes back) | `dialogs/settings/mod.rs` | gpui-component `setting::{Settings, SettingPage, SettingGroup, SettingItem}` [V module exists] |
| General (On Startup, Homepage, Profiles group, Download Folder, Ask, Check Spelling, Languages, Smooth Scrolling, Hardware Acceleration, Profile Folder, Memory Saver, Memory Savings, Updates group) | `dialogs/settings/general.rs` | restart notes (D7); languages = Chromium's list (D10) |
| Sync page (account row, buttons in fixed order, Customize Sync, Sync Server, Delete Data on Server) | `dialogs/settings/sync.rs` | `Syncer` API |
| Appearance (Theme, Tab Position, Show Bookmarks Bar, Show Home Button) | `dialogs/settings/appearance.rs` | |
| Search (engine, Manage Search Engines subpage, editor, Compact Address Bar, Always Show Full URLs, three suggestion switches) | `dialogs/settings/search.rs`, `dialogs/settings/search_engines.rs` | |
| Privacy (Tracking Protection, cookie radios, See All Site Data, Always Use Secure Connections, Secure DNS, Block Pop-ups, passwords notice, Site Permissions, Clear Browsing Data) | `dialogs/settings/privacy.rs`, `site_data.rs`, `site_permissions.rs` | Secure DNS wording for Chromium DoH (D9); Chromium's password manager kept off through prefs |
| Autofill row (`autofill.forms`) | `dialogs/settings/privacy.rs` | new, Windows parity (D8) |
| Shortcuts page (groups per section, Disabled, Reset to Default, Extension Shortcuts, Reset All) and capture dialog | `dialogs/settings/shortcuts.rs`, `dialogs/settings/capture.rs` | |
| Keyboard Shortcuts help (live accelerators) | `dialogs/shortcuts_help.rs` | gpui-component `Kbd` |
| Passphrase dialogs (four variants, rules, non-closable "Sign Out" variant, auto-close when moot) and the offer | `dialogs/passphrase.rs` | Dialog window over the most recent normal window |
| Welcome (seven steps, carousel without swipe, Back, dots, Next labels, every page's content) | `dialogs/welcome.rs` | gpui-component `carousel` [V exists] or a custom pager; default browser via `platform/default_browser.rs` (`xdg-settings`) |
| Extensions window (install by link, file, folder; progress; installed list; switches; Remove; Update; Re-enable; Not running; Notifications; Allow in private; ID; Location; open with a row expanded) | `dialogs/extensions.rs` | engine side [S:12]; "Not supported by the Linux runtime" rows dropped |
| `permissions.request` prompt | `dialogs/extension_prompts.rs` | Chromium shows its own prompt [S:13] |
| Bookmarks window (tree, lazy folders, favicons, expanded state; rows; open in new tab; search; New Folder; Edit; Move To; Delete; drag; Import; Export; refresh) | `dialogs/bookmarks.rs` | gpui-component `tree` / `virtual_list` [V exist] |
| History window (Tabs from Other Devices; visits or search; Forget; Clear… ranges; empty state; refresh on sync) | `dialogs/history.rs` | |
| Downloads window (list, rows, live bar and Cancel, Keep/Discard, Open, Show in Folder, Remove, 1 s re-wording, Open Download Folder, Clear List, empty state; Pause/Resume) | `dialogs/downloads.rs` | file-type icon from the extension (`file`, `file-archive`, `file-image`, `file-play`, `file-music`, `file-text`, `file-code`) |
| About (name, icon, version, comment; debug info with CEF, Chromium and GPUI versions) | `dialogs/about.rs` | engine `versions()` |

### 2.7 Extensions, sync, updates (extensions-sync-updates checklist)

**What does not change with Chromium.** Core keeps owning the install pipeline:
- verified CRX, prompts with core's wording;
- the synced records;
- update checks.

**What the engine adds.** It receives a desired set of unpacked directories through `Engine::sync_extensions(Vec<ExtensionLoad>)`. How it loads them is `ext-load`'s finding [S:12]: candidates are external-extension JSON, `--load-extension` at startup, or CDP `Extensions.loadUnpacked` on a browser-target session (`cef-extensions.md` §3).

**Action UI.** Stock CEF exposes no action state in Chrome style without Chrome's toolbar [V `cef-extensions.md` §4]. Until `ext-api` reports [S:13], the GPUI toolbar shows the manifest's `action.default_icon` and `default_title` and opens `default_popup` in a Chrome-style popup browser. Live badges, per-tab state and `activeTab` from a click are listed as gaps (D16).

| Feature | Module | Notes |
|---|---|---|
| Extensions view entry points (main menu, puzzle "Manage Extensions", button menu, notification Settings) | `window/actions.rs::show_extensions`, `dialogs/extensions.rs` | the notification Settings route has no hook (Chromium's notifications) [S:13] |
| Install from a pasted link or id, file picker (filter), unpacked folder | `dialogs/extensions.rs` | portal file chooser with filters (`platform/portal.rs`; GPUI's has no filters [V gpui.md §2.7]) |
| Pipeline (prepare on the main thread, run on a worker, commit on the main thread); shared progress row | `extensions/controller.rs` | background executor |
| Install prompt (`Add “X”?`, "It can:", bullets, details; Cancel default; "Add Extension"); outcome messages | `dialogs/extension_prompts.rs`, `extensions/controller.rs` | Dialog window over the browser window |
| Keymap re-applied after each change | `extensions/controller.rs` → `state/prefs.rs::apply_keymap` | |
| Engine runs the committed dir; "Not running" with the reason | `extensions/controller.rs` | engine `sync_extensions`, `BrowserEvent::ExtensionLoadFailed{id, error}` [S:12] |
| Welcome recommended extensions | `dialogs/welcome.rs` | |
| Reconcile at startup and after sync; remote enable changes applied (gap in GTK; decided: apply) | `extensions/controller.rs` | |
| Startup order (load, reconcile, schedule) | `state/mod.rs::start` | |
| Installed list rows; Enabled switch; withheld; Re-enable; Remove order; Allow in private; Notifications switch | `dialogs/extensions.rs`, `extensions/controller.rs` | private allowance and notification state inside Chromium [S:12] |
| Extension notifications with buttons and Settings | Chromium | dropped as shell work |
| `permissions.request` prompt | Chromium's prompt [S:13] | `dialogs/extension_prompts.rs` keeps core's version for the cases the shell asks itself |
| Update button and periodic checks (60 s, then 5 h); summary toast; keeps id and storage | `extensions/controller.rs` | |
| `prodversion` = Chromium version | `boot/mod.rs` (`OpenOptions.chrome_version`) | |
| Commands bound from `extension_shortcuts`; `_execute_action`; Settings rows | `engine_bridge/keymap.rs` (display), `extensions/commands.rs` | dispatch [S:13] |
| Toolbar actions, pins, puzzle, drag, popups 200×80..800×600, badges, private filter | `extensions/toolbar.rs`, `extensions/popup.rs` | badges [S:13] |
| `storage.sync` across devices | — | dropped, Windows parity (PLAN "Synced data at a glance") |
| Chrome-native install surfaces (Web Store "Add to Chrome", `chrome://extensions`) | `tab/navigation.rs` (`navigation_policy` may refuse `chrome://extensions`) | decision with `ext-load` [S:12] |
| Page context-menu extension items | Chromium's native menu | |
| Action button extension items | [S:13] | likely absent, as on Windows |
| Syncer (state model, scheduling 10 s, 2 s ticks, 60 s, 10 s; rounds; sign-in tab and loopback; cancel; sign-out; expiry; account details; passphrase offer and prompts; delete server data; Customize Sync; server field; final sync) | `sync.rs`, `dialogs/passphrase.rs`, `dialogs/settings/sync.rs` | verbatim port onto GPUI timers and background executor; the sign-in tab uses `BrowserState::normal_window`; Chromium's loopback-network permission prompt for `127.0.0.1` callbacks handled in `privacy/permissions.rs` (auto-allow for the pending sign-in only) |
| Welcome Sync page; History "Tabs from Other Devices" only when signed in | `dialogs/welcome.rs`, `dialogs/history.rs` | |
| Updates (installation detection, lifecycle table, timers, check, channel, download, install via pkexec, AppImage, banner, Settings group, failed-install toast, restart, prefs, cache dir) | `updates/mod.rs`, `updates/lifecycle.rs`, `toolbar/update_banner.rs`, `dialogs/settings/general.rs` | verbatim port of `lifecycle.rs` and its tests |
| Repeating timer armed before the first run | `util/timer.rs` (`Repeating`) | |
| Worker results back on the main thread; panics surface | `util/worker.rs` | |

### 2.8 Self-test (self-test §10)

`self_test/` keeps the report format, check names, order guard and artifacts. The checks are rewritten engine-neutrally and use the probes each unit exposes under `#[cfg(feature = "self-test")]`.

- **Page state:** read through `engine.eval` (DevTools `Runtime.evaluate`).
- **Cookie jars:** read through `engine.cookies_for(url)`.
- **Screenshots:** `platform::capture` plus overlay windows.
- **Key presses into chrome:** `Window::dispatch_keystroke` [V].
- **Key presses into the page:** `engine.send_key`.

| Check group | Module of the probes | WebKit-specific parts replaced by |
|---|---|---|
| `profile_open`, `session`, `omnibox`, `search_engines`, `bookmark*`, `tab_search`, `tab_groups`, `shortcuts`, `ctrl_s_toggles_sidebar`, `sync_passphrase`, `profiles`, `welcome` | owning UI modules | nothing (engine-neutral) |
| `install_prompt`, `install_crx`, `engine_loaded_extension`, `cws_install` | `extensions/controller.rs` | `runtime().loaded()` → `engine.loaded_extensions()` [S:12] |
| `content_script`, `extension_port`, `dnr_*`, `dynamic_content_script`, `extension_windows`, `extension_web_navigation`, `extension_cookies` | `self_test/checks/extensions.rs` | page eval; tab/window expectations rewritten to Chromium's model (each tab its own popup window) [S:13] |
| `navigate`, `history_recorded`, `tabs`, `tab_animation`, `tab_layout`, `tab_menu` | `window/`, `tabs_ui/` | layout probe uses the page slot rectangle; animation via GPUI animation state |
| `memory_saver` | `tab/sleep.rs` | sleep = browser closed; no `TerminatedByApi`; "Back available after wake" dropped (D14) |
| `popup`, `extension_toolbar`, `context_menus`, `extension_commands`, `extension_notifications`, `extension_update` | `extensions/*` | per S:13 |
| `selection_search`, `context_menus` page items | `engine_bridge/hooks.rs` | assert `context_menu_items` output and the ids inserted (no WebKit menu) |
| `private_window`, `cookies`, `site_permissions`, `capture_in_use`, `tracking_protection`, `https_only` | `privacy/*`, `omnibox/*` | `--use-fake-device-for-media-stream`; content settings; filter counters |
| `download*` | `downloads/*` | engine events |
| `new_tab_page`, `address_progress`, `zoom_*`, `connection_info` | `tab/`, `omnibox/` | |
| `settings`, `spellcheck` | `dialogs/settings/*` | Chromium prefs read back through the engine |
| `save_page`, `page_commands` | `save_page.rs`, `window/actions.rs` | MHTML via DevTools; print to PDF; view-source fetch counts dropped (Chromium loads it) |
| `screenshot` | `platform/capture.rs` | [S:11] |

### 2.9 What becomes simpler with Chromium

| Feature | GTK shell | New shell |
|---|---|---|
| View page source | own `view-source` scheme, hidden loader, reuse of another tab's resource, core `source_page` renderer, gate rules | open `view-source:<url>`; Chromium renders it (highlighted, line wrap) |
| Print | `webkit::PrintOperation` + GTK dialog | `engine.print(tab)` → Chrome print preview [S:15]; `print_to_pdf` for the self-test |
| DevTools | WebKit inspector, raw FFI open check, no console panel | `show_devtools`/`close_devtools`/`has_devtools`; console through `ExecuteChromeCommand(IDC_DEV_TOOLS_CONSOLE)` [I][S:8]; Inspect Element in Chromium's menu |
| Notifications | `GNotification` for extensions, WebKit seeding quirks | Chromium's Linux notification bridge; the shell only mirrors permission state |
| Spell check | Enchant/Hunspell scan, "No dictionaries are installed" | Chromium Hunspell `.bdic` downloaded on demand; suggestions and Add to Dictionary in Chromium's menu |
| Page context menu | WebKit menu + scripts for selection and frame | Chromium's native menu (link, image, media, spelling, extension items) plus our items from a hook |
| JS dialogs, beforeunload, HTTP auth, file chooser, `<select>`, pickers | WebKit built-ins | Chromium native (GtkUi needs the external pump on the main thread) |
| Same-document navigations | URI and back-forward-list heuristics | `OnAddressChange` without `OnLoadStart` |
| HTTPS-only | main-frame guessing, judge at load start | `OnBeforeBrowse` knows the main frame; cancel before the request |
| Tracking protection | compile JSON to WebKit content blockers, wait for compiles | per-request check on the IO thread; blocked counts now possible |
| Downloads | no pause; double-event quirks | pause and resume (Windows parity) |
| Find | 1000-match cap | exact count and active ordinal |
| Extension popup size | JS measurement | `SetAutoResizeEnabled` + `OnAutoResize` |
| Extensions | home-grown `vsesvit-webext` runtime (MV3 subset, DNR translation, menus, ports, storage) | Chromium's extension system (MV3 at Chrome 154 parity), within the limits of §2.7 |
| Link status bubble | own label overlay | Chromium status bubble [S:8] |
| Certificate details | `GTlsCertificate` walk | `CefSSLStatus` DER chain into the same core parser |
| IME in pages | WebKit | Chromium through GtkUi (native) |

### 2.10 WebKit-only workarounds that are dropped

1. `view_source::register`, the hidden-view loader and its HTTPS-only special cases (tab-engine §20).
2. `check_same_document_commit` (tab-engine §4).
3. The navigation gate (`vsesvit_webext::Gate`: browser-load tracking, opener judging, extension "speaks for" tracking) (tab-engine §5).
4. HTTPS-only main-frame heuristics `holds_no_http_frame` and judging at load start (tab-engine §6.3).
5. The error-page re-entrancy guard and the WebKit benign-error list; CEF reports `ERR_ABORTED` (tab-engine §7.3).
6. `Blocker`: content-blocker compile, generation counter, on-disk filter stores `<profile>/trackers`, `<profile>/cookies`, compile waits (omnibox-privacy §7). `when_applied` remains as an immediate call so that callers keep their shape.
7. The cookie content-blocker regex rules, and the single accept policy per network session that ruled out per-site Allow (omnibox-privacy §5.2).
8. Notification permission seeding into `WebContext`, and re-checking every notification (omnibox-privacy §4.8).
9. The JavaScript measurement of extension popup size (window §16).
10. Inspector `is_open` FFI (tab-engine §16).
11. `uri_for_display`, replaced by core `readable_url`.
12. The empty-title-before-commit reasoning (the rule itself stays).
13. The 1000-match find cap.
14. "WebKitGTK cannot pause a download" and the finished-after-failed double event.
15. Adding the AppImage to the WebKit sandbox (`add_appimage_to_sandbox`).
16. The whole `vsesvit-webext` runtime:
    - `TabHost`;
    - user content managers;
    - DNR translation;
    - `app.extension-notification`;
    - "Not supported by the Linux runtime" notices;
    - `<profile>/webext/`;
    - `private_session_started`;
    - `pending_filters`.
17. Live switching of smooth scrolling and hardware acceleration; they are process switches in Chromium.
18. Popover autohide-off for scripted runs because of Wayland grabs. Overlays are ours now; `SCRIPTED` keeps only its other effects.
19. GApplication-specific pieces:
    - D-Bus single instance;
    - per-profile application ids;
    - `org.gtk.Actions` debug actions;
    - `glib` logging;
    - `gio` file launchers;
    - `glib::user_special_dir`.
20. WebKit back/forward `WebViewSessionState` blobs. CEF cannot restore history, so nothing replaces them (D14).

### 2.11 Decisions for the owner (proposed defaults)

| # | Decision | Proposed default | Why |
|---|---|---|---|
| D1 | Tab keys | bind core's Ctrl+1..8 / Ctrl+9 (rebindable); Ctrl+Shift+PgUp/PgDn move the tab (built-in); drop libadwaita's Alt+digits, Ctrl+Home/End | Chrome and Windows parity; GPUI has no `AdwTabView` defaults |
| D2 | Shift+click on a link | follow Chromium's `NEW_WINDOW` disposition: open a new normal window | Chromium decides the disposition in the renderer; keeping the click in the tab would need cancel and reload (POST lost) |
| D3 | beforeunload on tab close | v1 force-closes without asking, as the GTK shell did | asking needs `TryClose` and a deferred removal |
| D4 | Sidebar below 720 px | toggle shows it by pushing the page | an overlay drawer over a native child needs a Popup window holding the whole tab list |
| D5 | Find status | keep "1 match", "{n} matches", "No matches"; no cap | wording KEEP; the cap was WebKit's |
| D6 | Download pause and resume | add them | CEF supports them; core states exist |
| D7 | Smooth scrolling, hardware acceleration | restart note "Takes effect when you restart Vsesvit." | Chromium process switches; Windows wording |
| D8 | Autofill row | show it | Chromium can autofill; Windows has the row |
| D9 | Secure DNS row | describe Chromium's DNS-over-HTTPS behaviour | the old wording described WebKit |
| D10 | Spell-check languages | Chromium's supported list; dictionaries downloaded by Chromium; Add to Dictionary writes Chromium's custom dictionary | Chromium ignores system Hunspell files |
| D11 | Page context menu | Chromium's native menu with our items added | extension items, spelling and native submenus come for free; its look differs from GPUI menus |
| D12 | DevTools | Chromium's separate DevTools window | docking needs a second child and layout work |
| D13 | Mnemonics | menus show labels without underscores | gpui-component menus have no mnemonics |
| D14 | Back/forward history | lost on session restore, reopen, duplicate and Memory Saver wake | CEF 154 cannot restore navigation entries (tab-engine §13); Windows has the same gap |
| D15 | Split view and media player | not on Linux in v1 | not in the GTK shell either |
| D16 | Extension action UI | manifest defaults only until `ext-api` reports; no badges or per-tab state | no action API in stock CEF (`cef-extensions.md` §4) |
| D17 | Cookie site Allow | offer Allow (per-site exceptions) | Chromium supports exceptions, as Windows does |
| D18 | Tooltips | Popup windows only where a tooltip can reach the page | in-window tooltips under the child would be cut off |

---

## 3. Theme and visual design

### 3.1 Theme engine

- **gpui-component `Theme`** [V `theme/mod.rs`] with two registered JSON themes, `assets/themes/vsesvit-light.json` and `vsesvit-dark.json`, which use gpui-component's colour keys [V `default-theme.json` schema].
- **Following the desktop** (`theme = system`, the default): call `Theme::change(window.appearance(), …)` at start, then from `cx.observe_window_appearance` [V API].
  - GPUI's Linux backend reads the portal's `org.freedesktop.appearance color-scheme` on X11 too [V `xdg_desktop_portal.rs`]. gpui-component's own comment advises using `window.appearance()` on Linux [V].
- **`theme = light | dark`**: `Theme::change(ThemeMode::Light | Dark)`; the appearance observer is ignored.
- **Web content:** `engine.apply_prefs(EnginePrefs { color_scheme })` → `CefRequestContext::SetChromeColorScheme` [I effect on `prefers-color-scheme`].
- **Private windows:** `theme::private_chrome()` overrides the top-bar colours (toolbar, banner, tab strip, bookmarks bar, find bar) with the GTK palette whatever the mode: background `#2e2e32`, foreground `#ffffff`, inactive background `#222226`. The page and sidebar keep the theme.
- **Group colours** use core `GroupColor::rgb(dark)`. Chip text is white in light mode and `#202124` in dark mode.
- **Profile colours** use core `ProfileColor::rgb()`.
- **Reduced motion:** `cx.reduce_motion()`. gpui-base reads the portal's `reduced-motion` and follows changes [V `reduce_motion.rs`]. It replaces `gtk-enable-animations`.
- **Fonts:** the system UI font through GPUI's font-kit and fontconfig path. Base text 14 px, captions 12 px, headings 15 px semibold, the picker title 26 px. URLs in the omnibox use the UI font.

### 3.2 Palettes

The values follow libadwaita, so that the browser still looks at home on GNOME and matches the error-page colours core already uses (light `#1e1e1e` on `#fafafb`, dark `#eeeeee` on `#222226`).

| Key (gpui-component) | Light | Dark |
|---|---|---|
| `background` (window) | `#fafafb` | `#222226` |
| `foreground` | `#2e3436` (90% black) | `#ffffff` |
| `title_bar.background` (toolbar row) | `#ffffff` | `#2e2e32` |
| `title_bar.border`, `border`, `window.border` | `#00000017` | `#ffffff17` |
| `sidebar.background` | `#ebebed` | `#2e2e32` |
| `sidebar.accent.background` (selected row) | `#0000001a` | `#ffffff1a` |
| `tab_bar.background` / `tab.active.background` | `#ebebed` / `#ffffff` | `#2e2e32` / `#3a3a3e` |
| `popover.background` | `#ffffff` | `#36363a` |
| `primary.background` / `accent.background` | `#3584e4` | `#3584e4` |
| `primary.foreground` / `accent.foreground` | `#ffffff` | `#ffffff` |
| `link.foreground` | `#1c71d8` | `#78aeed` |
| `muted.foreground` (dim labels) | `#2e343699` | `#ffffff99` |
| `input.border` / `ring` | `#0000001f` / `#3584e480` | `#ffffff1f` / `#78aeed80` |
| `danger.background` | `#e01b24` | `#c01c28` |
| `success.background` / `warning.background` | `#2ec27e` / `#e5a50a` | `#26a269` / `#cd9309` |
| `list.hover.background` | `#0000000f` | `#ffffff0f` |
| `overlay` (OSD: fullscreen notice, link preview fallback) | `#000000b3` | `#000000b3` |

### 3.3 Layout metrics (logical px; `theme/tokens.rs`)

| Token | Value | Source |
|---|---|---|
| `WINDOW_DEFAULT` | 1280×820 | window §1 |
| `TOOLBAR_H` | 46; buttons 34×34, 16 px icons, 6 px spacing; 6 px edge padding | libadwaita header bar |
| `OMNIBOX_H` / radius | 34 / 8 | entry |
| `OMNIBOX_COMPACT_MAX_W` | 720, centred (within 40 px of the window centre) | `COMPACT_ADDRESS_WIDTH` |
| Progress line | 2 high, inset 10 left and right, 1 from the bottom, radius 1, gradient transparent→foreground | Appendix A |
| Zoom chip | min-height 20, padding 0 6, 0.85em, radius 10 | Appendix A |
| In-use button / security icon | 24×24 each, 2 px gap (`ICON_GAP`) | omnibox-privacy §1.1 |
| `BANNER_H` | 40 | |
| `TAB_STRIP_H` | 38; tab height 34; min width 64; max 220; pinned 46 icon-only; close 22×22; 4 px between tabs | `AdwTabBar` look |
| Group chip (strip and sidebar) | padding 1 8, radius 9, 0.85em bold; untitled = 12×12 dot | Appendix A |
| Group dot on strip tabs | 8 px, in the indicator slot | window §12 |
| `BOOKMARKS_BAR_H` | min 28, padding 2 6; buttons min 24, padding 2 6; spacing 1; label ≤ 18 chars; chevron padding 2 8 bold | Appendix A, bookmarks §1 |
| Bookmark drop feedback | 2 px accent line before or after; "into" = accent 25% with radius 6 | bookmarks §0.4 |
| `FIND_BAR_H` | 42; entry about 28 chars (≈ 230 px) | tab-engine §19 |
| `SIDEBAR_W` | clamp(20% of window width, 180, 300); hidden below 720 | window §2 |
| Sidebar header and footer buttons | full width, margin 6, height 34 | window §11 |
| Tab row | min-height 30, padding 4 6 4 10, spacing 8, favicon 16, close 22×22 (opacity 0.6 → 1 on hover), pin 16 dim | Appendix A |
| Group header (sidebar) | margin 4 6 0, padding 2 6 2 10, min-height 24; 3 px colour bar inset on grouped rows | Appendix A |
| `SHARING_BAR_H` | 40 | |
| Omnibox suggestions | width = omnibox; max height 440; row margins 6 vertical and 8 horizontal; 12 spacing | omnibox-privacy §1.7 |
| Popover widths | tab search 360 (list max 440); group editor 300; site info 380 (max height 680); permission prompt 320; star bubble 340; download warning 300 | inventories |
| Menus | min width 200; row min-height 28; bookmark rows min 24, padding 4 8, labels ≤ 40 chars, max height 75% of monitor | bookmarks §2 |
| Extension popup | 200..800 × 80..600, initial 320×200 | window §16 |
| Extension badge | 0.65em bold, padding 0 3, min 10×12, radius 6, accent colours | Appendix A |
| Avatar | 22 (button, menus); 72 (picker, large) | shell-core §13.1 |
| Toast | max width 450; bottom margin 24; padding 8 12; radius 18 (pill) | adw look |
| Fullscreen notice | padding 12 20, radius 12, top margin 24, OSD colours, two lines | window §18 |
| Link preview (fallback) | padding 3 8, top-right radius 6, 0.9em, ≤ 90 chars middle-ellipsized | Appendix A |
| Secondary windows | Settings 1040×760; Extensions 720×760; Bookmarks and History 640×680; Downloads 560×560; Welcome 560×640; Picker 760×520; Manage Profiles 520×480 | dialogs, shell-core |
| Motion | tab rows 180 ms ease-out cubic (height + opacity); bar reveals 150 ms; none when `reduce_motion` | `motion.rs` |

**Window decorations.** The window requests `WindowDecorations::Client` [V option]. The toolbar row is then the title bar:
- empty space calls `window.start_window_move()` [V];
- double-click toggles maximize; right-click calls `show_window_menu`;
- window controls come from gpui-component's Linux `TitleBar` control buttons [V `title_bar.rs` exists];
- resize edges come from `WindowBorder` [V `window_border.rs`].

If the platform falls back to server-side decorations (`window.window_decorations()` returns `Server`), the controls are omitted.

### 3.4 Icons

- **Set:** Lucide 1.43.0 SVGs shipped by `gpui-kit-assets` 0.7.3, under the **ISC licence** [V `LICENSE-LUCIDE`]. ISC is MIT-compatible.
- **What we ship:** only the icons we use, copied into `assets/icons/` together with `LICENSE-LUCIDE`. `theme::icons::VsesvitAssets` implements `AssetSource` and falls back to `gpui_kit_assets::Assets` for the component icons [V: `AllAssets` would embed all 1,830 SVGs, about 1 MiB].
- **Use:** through `Icon::new(VsIcon::X)`, where `Icon::path("icons/….svg")` exists [V].
- **No Zed `ui`, `theme` or `icons` crates** (GPL-3.0) [V `gpui.md` §0].

Every name below exists in the asset directory [V, §7].

| GTK symbolic (old) | Lucide | | GTK symbolic (old) | Lucide |
|---|---|---|---|---|
| go-previous / go-next | `arrow-left` / `arrow-right` | | view-refresh / process-stop | `rotate-cw` / `x` |
| go-home | `house` | | sidebar-show (left / right) | `panel-left` / `panel-right` |
| user-not-tracked (private) | `hat-glasses` | | tab-new, list-add | `plus` |
| folder-download | `download` | | open-menu | `menu` |
| application-x-addon (puzzle) | `puzzle` | | system-search | `search` |
| channel-secure / channel-insecure | `lock` / `lock-open` | | dialog-information | `info` |
| non-starred / starred | `star` / `star-fill` | | zoom-in / zoom-out | `zoom-in` / `zoom-out` |
| window-close (tab close) | `x` | | view-pin | `pin` |
| audio-volume-high / -muted | `volume-2` / `volume-x` | | camera-web / microphone / screen-shared | `video` / `mic` / `screen-share` |
| web-browser (generic favicon) | `globe` | | folder / folder-open / folder-new | `folder` / `folder-open` / `folder-plus` |
| go-next (menu arrow), pan-end / pan-down | `chevron-right` / `chevron-down` | | go-up / go-down (find) | `chevron-up` / `chevron-down` |
| document-open-recent (history row) | `clock` | | user-bookmarks | `bookmark` |
| view-paged (switch to tab) | `app-window` | | object-select (swatch tick) | `check` |
| document-edit | `pencil` | | user-trash | `trash` |
| edit-undo (reset) | `undo-2` | | window-new (open profile) | `square-arrow-out-up-right` |
| security-high | `shield` | | preferences-system | `settings` |
| applications-graphics | `palette` | | preferences-desktop-keyboard-shortcuts | `keyboard` |
| dialog-warning / dialog-error | `triangle-alert` / `circle-x` | | dialog-password | `key-round` |
| package-x-generic | `package` | | view-more | `ellipsis-vertical` |
| go-jump (move to) | `folder-input` | | download cancel | `circle-stop` |
| file types | `file`, `file-archive`, `file-image`, `file-play`, `file-music`, `file-text`, `file-code` | | print, view source, copy, link | `printer`, `code`, `copy`, `link` |

---

## 4. Engine API boundary (`crates/vsesvit-engine`)

### 4.1 Principles

1. **Engine-agnostic.** No GPUI and no CEF types. The one platform fact that crosses is the X11 parent window id (`NativeParent::X11(u32)`), which the decided architecture requires.
2. **Main-thread object.** `Rc<dyn Engine>`. Every call is made from the GPUI main thread, which is CEF's UI thread under the external pump. No call blocks. Results of slow operations come back as events tagged with a `RequestId`.
3. **Events, never callbacks into GPUI.** The engine pushes `BrowserEvent`s into an unbounded channel. A GPUI foreground task drains it *after* each pump step, so there is no re-entrancy into `App` (the "RefCell already borrowed" pitfall in gpui.md §4.1).
4. **Three synchronous questions only** (`ShellHooks`): navigation policy, popup placement and context-menu items. CEF must answer them inside its callback. They run outside any GPUI update and read UI-maintained snapshots (`Rc<RefCell<HookSnapshot>>`) plus the core `Profile`; they never touch `gpui::App`.
5. **Core types are reused** where they already name the concept: `Browsing`, `Permission`, `Setting`, `Origin`, `Capturing`, `Chord`, `Command`, `Url`. The mapping from core's history `Transition` stays in the UI.
6. **The shell allocates `TabId`s** (`TabId::next()`, an atomic counter from 1). Tabs the engine creates for popups get theirs through `ShellHooks::popup_policy`.

### 4.2 Rust sketch

```rust
// ── ids.rs ──────────────────────────────────────────────────────────────────────────────
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(std::num::NonZeroU32);
impl TabId { pub fn next() -> TabId; pub fn get(self) -> u32; }
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)] pub struct RequestId(pub u64);
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)] pub struct EngineDownloadId(pub u32);

// ── types.rs ────────────────────────────────────────────────────────────────────────────
pub use vsesvit_core::{Url, private::Browsing, permissions::{Permission, Setting, Origin, Capturing},
                       shortcuts::{Chord, Command}};

#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum NativeParent { X11(u32) }
/// Device pixels, relative to the parent window's origin.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)] pub struct DeviceRect { pub x: i32, pub y: i32, pub width: u32, pub height: u32 }

pub enum InitialPage {
    Url(Url),
    /// Shown at `about:blank` (new tab page) or under `shown_url` (error pages). Mechanism: [S:9].
    Html { html: String, shown_url: Option<Url> },
    Blank,
}
pub enum TabKind { Page, ExtensionPopup { min: (u32, u32), max: (u32, u32) } }
pub struct TabSpec {
    pub tab: TabId, pub kind: TabKind, pub browsing: Browsing,
    pub parent: NativeParent, pub bounds: DeviceRect, pub scale_factor: f32, pub visible: bool,
    pub page: InitialPage, pub opener: Option<TabId>,
}
#[derive(Copy, Clone, Debug)] pub enum CloseMode { Force, RunUnloadHandlers }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OpenDisposition { CurrentTab, NewForegroundTab, NewBackgroundTab, NewWindow, NewPopup, OffTheRecord, SaveToDisk }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum CommitKind { Document, SameDocument, ErrorPage }
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EngineTransition { Link, Typed, AutoBookmark, Reload, FormSubmit, BackForward, Generated, Other }

pub struct NavigationRequest { pub tab: TabId, pub url: Url, pub is_main_frame: bool, pub is_redirect: bool,
                               pub user_gesture: bool, pub browser_initiated: bool, pub transition: EngineTransition }
pub enum NavigationDecision { Allow, Cancel, Redirect(Url) }

pub struct PopupRequest { pub opener: TabId, pub url: Url, pub disposition: OpenDisposition, pub user_gesture: bool }
pub enum PopupDecision { Block, AllowAsTab { tab: TabId, parent: NativeParent, bounds: DeviceRect } }

pub struct NetError { pub code: i32, pub name: String, pub description: String }
bitflags::bitflags! { pub struct CertStatus: u32 { const AUTHORITY_INVALID = 1; const NAME_INVALID = 2; const DATE_INVALID = 4;
    const REVOKED = 8; const WEAK = 16; const INVALID = 32; const OTHER = 1 << 31; } }
pub struct SslInfo { pub secure: bool, pub status: CertStatus, pub der_chain: Vec<Vec<u8>>, pub mixed_content: bool }
pub enum TerminationStatus { Crashed, Killed, OutOfMemory, Other(i32) }
pub struct FindResult { pub matches: u32, pub active: u32, pub final_update: bool }
/// 1.0 = 100 %. The engine converts to and from Chromium's logarithmic level (factor = 1.2^level).
#[derive(Copy, Clone, Debug, PartialEq)] pub struct Zoom(pub f64);

pub enum MediaKind { Image, Video, Audio, Other }
pub struct ContextMenuParams { pub page_url: Option<Url>, pub frame_url: Option<Url>, pub link_url: Option<Url>,
    pub src_url: Option<Url>, pub media: Option<MediaKind>, pub selection: Option<String>, pub editable: bool,
    pub plain_page: bool /* not link, image, media, editable or selection */ }
pub enum ShellMenuItem {
    OpenLinkInPrivateWindow, CopyCleanLink,
    SearchSelection { label: String, url: Url },
    Print, ViewSource { enabled: bool },
}

pub enum RoutePhase { BeforePage, AfterPage }
pub enum RoutedKey { Command(Command), NextTab, PreviousTab, MoveTabLeft, MoveTabRight }
pub struct KeyRouting { pub entries: Vec<(Chord, RoutePhase, RoutedKey)> }

pub enum ColorScheme { System, Light, Dark }
pub enum CookieControls { Allow, BlockInPrivate, Block }
pub struct EnginePrefs { pub block_popups: bool, pub color_scheme: ColorScheme, pub spellcheck: Option<Vec<String>>,
    pub accept_languages: String, pub cookie_controls: CookieControls, pub autofill: bool }

pub struct PermissionRequest { pub id: RequestId, pub tab: TabId, pub origin: Option<Origin>, pub permissions: Vec<Permission> }
pub enum PermissionAnswer { Allow, Deny, Dismiss }

pub struct DownloadRequest { pub id: EngineDownloadId, pub tab: Option<TabId>, pub browsing: Browsing, pub url: Url,
    pub original_url: Url, pub suggested_name: String, pub mime: Option<String>, pub total: Option<u64> }
pub enum EngineDownloadState { InProgress, Paused, Complete, Cancelled, Interrupted { resumable: bool }, Failed }
pub struct DownloadUpdate { pub id: EngineDownloadId, pub received: u64, pub total: Option<u64>,
    pub state: EngineDownloadState, pub full_path: Option<std::path::PathBuf> }

pub struct ExtensionLoad { pub id: String, pub dir: std::path::PathBuf, pub enabled: bool, pub allowed_in_private: bool }
pub struct EngineVersions { pub engine: String /* "CEF 154.0.34+g14c5a08" */, pub chromium: String /* "154.0.8037.98" */ }

/// Runs on the engine's IO thread. Snapshots only: never the core Profile.
pub trait RequestFilter: Send + Sync {
    fn block_request(&self, browsing: Browsing, page: Option<&Url>, request: &Url, resource: ResourceKind) -> bool;
    fn block_cookies(&self, browsing: Browsing, request: &Url) -> bool;
}
pub enum ResourceKind { MainFrame, SubFrame, Script, Image, Stylesheet, Xhr, Media, Font, Other }

pub enum RequestResult {
    Eval(Result<String, String>),            // JSON text of the value, or the exception
    PdfPrinted(Result<(), String>), MhtmlSaved(Result<(), String>),
    CookieSites(Vec<(String /* site */, u32 /* cookies */)>),
    Cookies(Vec<(String, String)>),          // name, value
    SiteDataDeleted(Result<(), String>), BrowsingDataCleared(Result<(), String>),
    Captured(Result<Vec<u8> /* PNG */, String>),
}

// ── command.rs ──────────────────────────────────────────────────────────────────────────
pub trait Engine {
    // set-up
    fn set_hooks(&self, hooks: std::rc::Rc<dyn ShellHooks>);
    fn set_key_routing(&self, routing: KeyRouting);
    fn apply_prefs(&self, prefs: &EnginePrefs);
    fn set_request_filter(&self, filter: std::sync::Arc<dyn RequestFilter>);
    fn set_document_start_scripts(&self, scripts: Vec<String>);
    fn set_site_setting(&self, browsing: Browsing, origin: &Origin, permission: Permission, setting: Option<Setting>);
    fn begin_private_context(&self);
    fn end_private_context(&self);
    fn versions(&self) -> EngineVersions;

    // tab lifecycle and presentation
    fn create_tab(&self, spec: TabSpec);
    fn close_tab(&self, tab: TabId, mode: CloseMode);
    fn close_all(&self);                                        // → BrowserEvent::AllTabsClosed
    fn set_bounds(&self, tab: TabId, bounds: DeviceRect);
    fn set_visible(&self, tab: TabId, visible: bool);
    fn set_occlusions(&self, tab: TabId, rects: &[DeviceRect]);  // Cutout overlay backend [S:4]
    fn reparent(&self, tab: TabId, parent: NativeParent, bounds: DeviceRect) -> Result<(), String>; // [S:10]
    fn focus_page(&self, tab: TabId);
    fn blur_page(&self, tab: TabId);

    // navigation
    fn load(&self, tab: TabId, url: &Url);
    fn load_html(&self, tab: TabId, html: String, shown_url: Option<&Url>);  // [S:9]
    fn reload(&self, tab: TabId, bypass_cache: bool);
    fn stop(&self, tab: TabId);
    fn go_back(&self, tab: TabId);
    fn go_forward(&self, tab: TabId);

    // page commands
    fn find(&self, tab: TabId, text: &str, forward: bool, find_next: bool);
    fn stop_finding(&self, tab: TabId, clear_selection: bool);
    fn set_zoom(&self, tab: TabId, zoom: Zoom);
    fn zoom(&self, tab: TabId) -> Zoom;
    fn set_audio_muted(&self, tab: TabId, muted: bool);
    fn print(&self, tab: TabId);                                 // [S:15]
    fn print_to_pdf(&self, tab: TabId, path: std::path::PathBuf) -> RequestId;
    fn show_devtools(&self, tab: TabId, inspect_at: Option<(i32, i32)>);
    fn show_console(&self, tab: TabId);
    fn close_devtools(&self, tab: TabId);
    fn has_devtools(&self, tab: TabId) -> bool;
    fn exit_fullscreen(&self, tab: TabId);
    fn save_mhtml(&self, tab: TabId, path: std::path::PathBuf) -> RequestId;
    fn start_download(&self, tab: TabId, url: &Url);
    fn eval(&self, tab: TabId, script: &str) -> RequestId;
    fn capture_page(&self, tab: TabId) -> RequestId;
    fn send_key(&self, tab: TabId, chord: Chord);                // self-test; extension-command fallback

    // answers to requests the engine raised
    fn answer_permission(&self, id: RequestId, answer: PermissionAnswer);
    fn continue_download(&self, id: EngineDownloadId, path: std::path::PathBuf);
    fn cancel_download(&self, id: EngineDownloadId);
    fn pause_download(&self, id: EngineDownloadId);
    fn resume_download(&self, id: EngineDownloadId);

    // storage
    fn cookie_sites(&self, browsing: Browsing) -> RequestId;
    fn cookies_for(&self, browsing: Browsing, url: &Url) -> RequestId;
    fn delete_site_data(&self, browsing: Browsing, site: &str) -> RequestId;
    fn delete_cookies_for_hosts(&self, browsing: Browsing, hosts: &[String]);
    fn clear_browsing_data(&self, browsing: Browsing) -> RequestId;

    // extensions (provisional, [S:12], [S:13])
    fn sync_extensions(&self, desired: Vec<ExtensionLoad>);
    fn loaded_extensions(&self) -> Vec<String>;
}

// ── hooks.rs ────────────────────────────────────────────────────────────────────────────
/// Called synchronously on the main thread from inside engine callbacks, never inside a GPUI update.
/// Implementations read snapshots and the core Profile; they must not touch gpui::App.
pub trait ShellHooks {
    fn navigation_policy(&self, request: &NavigationRequest) -> NavigationDecision;
    fn popup_policy(&self, request: &PopupRequest) -> PopupDecision;
    fn context_menu_items(&self, tab: TabId, params: &ContextMenuParams) -> Vec<ShellMenuItem>;
}

// ── event.rs ────────────────────────────────────────────────────────────────────────────
pub enum BrowserEvent {
    Tab(TabId, TabEvent),
    PopupOpened { opener: TabId, tab: TabId, url: Url, disposition: OpenDisposition },
    OpenUrlRequested { opener: TabId, url: Url, disposition: OpenDisposition, user_gesture: bool },
    ForeignWindowAdopted { url: Url },        // a Chrome-UI window Chromium opened itself (e.g. tabs.create), closed by the engine
    Shortcut { tab: Option<TabId>, key: RoutedKey },
    ContextMenuCommand { tab: TabId, item: ShellMenuItem, params: ContextMenuParams },
    PermissionRequested(PermissionRequest),
    PermissionWithdrawn { id: RequestId },
    DownloadRequested(DownloadRequest),
    DownloadUpdated(DownloadUpdate),
    RequestFinished { id: RequestId, result: RequestResult },
    ExtensionLoadFailed { id: String, error: String },
    AllTabsClosed,
    AppRelaunch { argv: Vec<std::ffi::OsString>, cwd: std::path::PathBuf },
    EngineError(String),
}
pub enum TabEvent {
    Created, Closed /* unrequested Closed = the page closed itself (window.close()) */,
    TitleChanged(String), AddressChanged(Url), FaviconChanged(Option<std::sync::Arc<[u8]>> /* PNG ≤ 32 px */),
    LoadingStateChanged { loading: bool, can_go_back: bool, can_go_forward: bool },
    LoadProgress(f32),
    NavigationStarted { url: Url, is_redirect: bool },
    Committed { url: Url, kind: CommitKind, transition: EngineTransition, mime: Option<String> },
    LoadFailed { url: Url, error: NetError },
    CertificateError { url: Url, status: CertStatus, der_chain: Vec<Vec<u8>> },
    SecurityChanged(Option<SslInfo>),
    RenderProcessGone(TerminationStatus),
    FindResult(FindResult),
    ZoomChanged(Zoom),                         // [S:14]
    AudioStateChanged { audible: bool },       // [S:14]
    CaptureChanged(Capturing),
    FullscreenRequested(bool),
    StatusText(Option<String>),                // only when Chromium's status bubble is off
    FocusChanged { focused: bool },
    TakeFocus { next: bool },
    PreferredSize { width: u32, height: u32 }, // extension popups
}

// ── factory.rs ──────────────────────────────────────────────────────────────────────────
pub struct EngineConfig { pub root_cache_path: std::path::PathBuf, pub smooth_scrolling: bool,
    pub hardware_acceleration: bool, pub log_file: Option<std::path::PathBuf>, pub user_agent_product: String,
    pub scripted: bool, pub extra_switches: Vec<String> }
pub trait EngineDriver {
    /// Wake requests (delay until the next do_work), sent from any thread.
    fn schedule(&mut self) -> futures_channel::mpsc::UnboundedReceiver<std::time::Duration>;
    /// One pump step: GLib default-context iteration, then do_message_loop_work (CEF).
    fn do_work(&self);
    fn shutdown(self: Box<Self>);
}
pub struct EngineParts { pub engine: std::rc::Rc<dyn Engine>, pub driver: Box<dyn EngineDriver>,
    pub events: futures_channel::mpsc::UnboundedReceiver<BrowserEvent> }
pub enum EngineInitError { EarlyExit(std::process::ExitCode), Failed(String) }
pub trait EngineFactory {
    fn chromium_version(&self) -> &'static str;
    fn initialize(&self, config: EngineConfig) -> Result<EngineParts, EngineInitError>;
}

// ── fake.rs (feature "fake") ────────────────────────────────────────────────────────────
pub struct FakeEngine { /* records every command; tests inject BrowserEvents; tracks tabs, bounds, visibility, focus */ }
pub struct FakeEngineFactory;
```

### 4.3 How events reach GPUI entities

```rust
// engine_bridge/mod.rs
pub fn start(cx: &mut App, mut driver: Box<dyn EngineDriver>, mut events: UnboundedReceiver<BrowserEvent>) {
    // Pump: as gpui.md §7.7. Wait for the shortest scheduled delay (cap 33 ms), call do_work() outside any update.
    let mut wakes = driver.schedule();
    cx.spawn(async move |cx| pump::run(cx, driver, wakes).await).detach();
    // Router: CEF handlers ran inside do_work() and only queued events; this task runs after do_work() returns.
    cx.spawn(async move |cx| {
        while let Some(event) = events.next().await {
            cx.update(|cx| router::route(event, cx));
        }
    }).detach();
}

// engine_bridge/router.rs
pub fn route(event: BrowserEvent, cx: &mut App) {
    let state = Shell::state(cx);
    match event {
        BrowserEvent::Tab(id, ev) => {
            if let Some(tab) = state.read(cx).tab(id) { tab.update(cx, |tab, cx| tab.apply(ev, cx)); }
        }
        BrowserEvent::PopupOpened { opener, tab, url, .. } => state.update(cx, |s, cx| s.adopt_popup(opener, tab, url, cx)),
        BrowserEvent::OpenUrlRequested { opener, url, disposition, .. } => state.update(cx, |s, cx| s.open_from_page(opener, url, disposition, cx)),
        BrowserEvent::Shortcut { tab, key } => state.update(cx, |s, cx| s.run_routed_key(tab, key, cx)),
        BrowserEvent::ContextMenuCommand { tab, item, params } => state.update(cx, |s, cx| s.context_menu_command(tab, item, params, cx)),
        BrowserEvent::PermissionRequested(r) => state.read(cx).privacy.clone().update(cx, |p, cx| p.request(r, cx)),
        BrowserEvent::PermissionWithdrawn { id } => state.read(cx).privacy.clone().update(cx, |p, cx| p.withdrawn(id, cx)),
        BrowserEvent::DownloadRequested(r) => state.read(cx).downloads.clone().update(cx, |d, cx| d.requested(r, cx)),
        BrowserEvent::DownloadUpdated(u) => state.read(cx).downloads.clone().update(cx, |d, cx| d.updated(u, cx)),
        BrowserEvent::RequestFinished { id, result } => requests::complete(id, result, cx),  // oneshot registry → Task<RequestResult>
        BrowserEvent::ExtensionLoadFailed { id, error } => state.read(cx).extensions.clone().update(cx, |e, cx| e.load_failed(id, error, cx)),
        BrowserEvent::ForeignWindowAdopted { url } => state.update(cx, |s, cx| s.open_from_outside(url, cx)),
        BrowserEvent::AllTabsClosed => crate::app::quit::all_closed(cx),
        BrowserEvent::AppRelaunch { argv, cwd } => state.update(cx, |s, cx| s.handle_invocation(Invocation::relaunch(argv, cwd), cx)),
        BrowserEvent::EngineError(e) => log::error!("engine: {e}"),
    }
}
```

**Requests.** `engine_bridge::requests::eval(cx, tab, script) -> Task<RequestResult>` wraps `engine.eval` plus a `oneshot` keyed by `RequestId`. The same pattern serves PDF, MHTML, cookies and capture.

**Fan-out.** `Tab::apply` updates the tab's state and emits `TabChanged`. Subscribers:
- `BrowserWindow`: row, title, focus, fullscreen;
- `Omnibox` and `Toolbar`: the selected tab only;
- `BrowserState`: history visits and titles, session save, favicon recording, site zoom.

**Hooks.** `engine_bridge/hooks.rs::Hooks` holds `Rc<RefCell<HookSnapshot>>` and a `Core` clone. The snapshot holds, per tab: its window's XID, the page rectangle, `Browsing`, the HTTPS-only `Upgrades` and its opener chain. BrowserWindows refresh it whenever layout or tabs change. `navigation_policy` borrows core (HTTPS-only `allowed`, `https_only::upgrade`) and uses `try_borrow_mut`, failing safe to `Allow`.

### 4.4 CEF mapping, for the `vsesvit-cef` implementer

| Contract | CEF 154 (names from the inventories, checked there against `cef-dll-sys` bindings) |
|---|---|
| `create_tab` | `WindowInfo::set_as_child(xid, rect)`, `runtime_style = CHROME`. `BrowserSettings`: `chrome_status_bubble` on [S:8], `chrome_zoom_bubble` off, background colour. The profile or private `RequestContext`. `browser_host_create_browser`. `ExtensionPopup` adds `set_auto_resize_enabled(min, max)`. |
| `set_bounds`, `set_visible`, `set_occlusions`, `reparent` | the engine's own x11rb connection on `host.window_handle()`: `ConfigureWindow`, `Map`/`UnmapWindow` + `was_hidden`, `shape::rectangles`, `ReparentWindow` [S:1, S:4, S:10] |
| `focus_page` / `blur_page` | `host.set_focus(1)` / `host.set_focus(0)` [S:2] |
| `load`, `reload`, `stop`, `go_back`, `go_forward` | `frame.load_url`, `browser.reload` / `reload_ignore_cache`, `stop_load`, `go_back`, `go_forward` |
| `load_html` | [S:9]: a scheme handler or a resource handler for a re-navigation, keeping the shown URL |
| `find`, `stop_finding` | `host.find`, `host.stop_finding`; `CefFindHandler::OnFindResult` |
| `set_zoom`, `zoom` | `set_zoom_level(ln f / ln 1.2)`, `get_zoom_level` |
| `set_audio_muted` | `host.set_audio_muted` |
| `print`, `print_to_pdf` | `host.print`, `host.print_to_pdf` + `PdfPrintCallback` |
| `show_devtools`, `show_console`, `close_devtools`, `has_devtools` | `show_dev_tools(default window info, inspect point)`, `execute_chrome_command(IDC_DEV_TOOLS_CONSOLE)`, `close_dev_tools`, `has_dev_tools` |
| `exit_fullscreen` | `host.exit_fullscreen` |
| `save_mhtml`, `eval`, `capture_page`, `delete_site_data` | `execute_dev_tools_method`: `Page.captureSnapshot{format:"mhtml"}`, `Runtime.evaluate`, `Page.captureScreenshot`, `Storage.clearDataForOrigin`; `DevToolsMessageObserver` |
| `start_download`, download answers | `host.start_download`; `CefDownloadHandler::{CanDownload → 1, OnBeforeDownload → keep the callback, OnDownloadUpdated}`; `BeforeDownloadCallback::cont`, `DownloadItemCallback::{cancel, pause, resume}` |
| `answer_permission` | `CefPermissionHandler::{OnShowPermissionPrompt, OnRequestMediaAccessPermission}` return 1 and keep the callback; `OnDismissPermissionPrompt` → `PermissionWithdrawn` [S:16] |
| `apply_prefs` | `RequestContext::set_preference`: `browser.enable_spellchecking`, `spellcheck.dictionaries`, `credentials_enable_service=false`, `profile.password_manager_enabled=false`, `profile.cookie_controls_mode`, `autofill.*`. Also `set_chrome_color_scheme`, and the default content setting for `POPUPS`. |
| `set_site_setting` | `RequestContext::set_content_setting` |
| `set_request_filter` | `CefRequestHandler::GetResourceRequestHandler` → `OnBeforeResourceLoad` (`RV_CANCEL`), `GetCookieAccessFilter` (IO thread) |
| `set_document_start_scripts` | DevTools `Page.addScriptToEvaluateOnNewDocument` per browser, or `RenderProcessHandler::OnContextCreated` |
| `begin_private_context` / `end_private_context` | `request_context_create_context(empty cache_path)` / drop it once its browsers have closed |
| `cookie_sites`, `cookies_for`, `delete_cookies_for_hosts`, `clear_browsing_data` | `CookieManager::visit_all_cookies` / `visit_url_cookies` / `delete_cookies`; `clear_http_cache` + DevTools |
| `ShellHooks::navigation_policy` | `CefRequestHandler::OnBeforeBrowse` (UI thread); `Redirect` = cancel + `load_url` |
| `ShellHooks::popup_policy` | `CefLifeSpanHandler::OnBeforePopup`: `Block` → return 1; `AllowAsTab` → fill `windowInfo.set_as_child(xid, rect)` and the client; `OnAfterCreated` → `PopupOpened` |
| `ShellHooks::context_menu_items` | `CefContextMenuHandler::OnBeforeContextMenu` (insert at the documented positions, ids from `MENU_ID_USER_FIRST`); `OnContextMenuCommand` → `ContextMenuCommand` |
| `KeyRouting` → `Shortcut` | `CefKeyboardHandler::OnPreKeyEvent` (BeforePage → swallow; AfterPage → `is_keyboard_shortcut = 1`), `OnKeyEvent`; `CefCommandHandler::OnChromeCommand` refuses Chrome accelerators [S:6] |
| `TitleChanged`, `AddressChanged`, `FaviconChanged`, `LoadProgress`, `FullscreenRequested`, `StatusText`, `PreferredSize`, `CaptureChanged` | `CefDisplayHandler::{OnTitleChange, OnAddressChange, OnFaviconURLChange → DownloadImage(max 32) → GetAsPNG, OnLoadingProgressChange, OnFullscreenModeChange, OnStatusMessage, OnAutoResize, OnMediaAccessChange (+ DESKTOP_* tracking)}` |
| `LoadingStateChanged`, `Committed`, `LoadFailed` | `CefLoadHandler::{OnLoadingStateChange, OnLoadStart (main frame, transition), OnLoadError (not ERR_ABORTED)}`; `SameDocument` = `OnAddressChange` with no `OnLoadStart` |
| `CertificateError`, `SecurityChanged`, `RenderProcessGone` | `CefRequestHandler::{OnCertificateError → 0, OnRenderProcessTerminated}`; `GetVisibleNavigationEntry().GetSSLStatus()` after commit |
| `FocusChanged`, `TakeFocus` | `CefFocusHandler::{OnGotFocus, OnSetFocus, OnTakeFocus}` |
| `Created`, `Closed`, `AllTabsClosed` | `CefLifeSpanHandler::{OnAfterCreated, OnBeforeClose}` (`DoClose` is Alloy-only [V `cef-rs-api` G16]) |
| `OpenUrlRequested` | `CefRequestHandler::OnOpenURLFromTab` |
| `ForeignWindowAdopted` | `CefBrowserProcessHandler::GetDefaultClient` + `OnAfterCreated` of browsers the shell did not create: read the URL, close the window (`cef-extensions.md` §7) |
| `AppRelaunch` | `CefBrowserProcessHandler::OnAlreadyRunningAppRelaunch` → return 1 |
| `ZoomChanged`, `AudioStateChanged` | no CEF callback [S:14] |

### 4.5 `FakeEngine`

`FakeEngine` lives in `vsesvit-engine` behind the `fake` feature. It:
- records every command (`commands() -> Vec<FakeCommand>`);
- keeps a model of tabs (bounds, visibility, focus, URL, zoom, muted);
- lets tests push any `BrowserEvent` (`inject`);
- answers `RequestId` operations with scripted results;
- runs `ShellHooks` when a test simulates `navigate_from_page` or `open_popup`.

With `gpui::TestAppContext` this lets every UI unit test its logic and views in CI with no display, no CEF and no X server.

---

## 5. Work breakdown

### 5.1 Module tree and owners

Every file has one owner unit. `U01` creates the whole tree in wave 0: `lib.rs` declares every module, and each file starts with `//! Owned by Uxx.` plus the frozen interface stubs of §5.2. Later units fill their own files and never edit another unit's file. A needed interface change goes through the owner.

```
crates/vsesvit-engine/                       U00
crates/vsesvit/{Cargo.toml, src/main.rs}     U01
Cargo.toml (workspace)                       U01 (adds), U19 (removes GTK crates)
crates/vsesvit-gpui/
  Cargo.toml, build.rs (rpath $ORIGIN)       U01
  assets/themes/*.json, assets/icons/*       U02
  examples/theme_gallery.rs                  U02
  examples/x11_probe.rs                      U03
  examples/overlay_demo.rs                   U04
  src/
    lib.rs                                   U01
    boot/{mod,cli,logging,profile_start,instance,headless}.rs          U01
    util/{mod,time,text,timer,worker}.rs                               U01
    app/actions.rs                                                     U01
    app/{mod,shell,quit}.rs                                            U05
    state/{mod,windows,tabs,closed_tabs,session,private_session,prefs,
           sync_applied,history_hooks,memory_saver,invocation}.rs      U05
    theme/{mod,tokens,colors,icons,avatar,motion}.rs                   U02
    platform/{mod,x11,watch,capture,activation,dirs,portal,default_browser}.rs   U03
    overlay/{mod,popup,cutout,dismiss,menu,tooltip,toast,bubble_slot}.rs         U04
    dialogs/{mod,host,prompts,library}.rs                              U04
    tab/{mod,model,navigation,content,error_page,sleep}.rs             U06
    engine_bridge/{mod,pump,router,requests,hooks,keymap,focus}.rs     U07
    window/{mod,layout,page_area,frame,actions,fullscreen,close}.rs    U08
    tabs_ui/{mod,row,sidebar,strip,tab_menu,tab_search,group_editor,drag}.rs     U09
    toolbar/{mod,main_menu,update_banner}.rs, find_bar.rs              U10
    omnibox/{mod,edit,field,suggestions,progress,zoom_bubble}.rs       U11
    omnibox/{site_info,permission_prompt}.rs, tab/permissions.rs,
      privacy/{mod,permissions,trackers,cookies,site_data}.rs          U12
    bookmarks/{mod,bar,menu,editor,drag}.rs, favicons.rs,
      dialogs/{bookmarks,history}.rs                                   U13
    downloads/{mod,controller,warning}.rs, toolbar/downloads_button.rs,
      dialogs/downloads.rs, save_page.rs                               U14
    profiles/{mod,picker,dialogs,manage,launch}.rs, toolbar/profile_button.rs    U15
    extensions/{mod,controller,toolbar,popup,commands}.rs,
      dialogs/{extensions,extension_prompts}.rs                        U16
    dialogs/settings/{mod,general,sync,appearance,search,privacy,shortcuts,capture,
      search_engines,site_data,site_permissions}.rs,
      dialogs/{about,shortcuts_help}.rs                                U17
    sync.rs, updates/{mod,lifecycle}.rs, dialogs/{welcome,passphrase}.rs         U18
    self_test/{mod,report,runner,probes,capture,checks/*.rs}.rs, test_support.rs U19
```

`tab/permissions.rs` is a file inside U06's directory but is owned by U12. That is allowed because it is a separate file.

### 5.2 Frozen interfaces (wave 0 stubs)

These signatures are written by U01 as stubs, with `todo!()` bodies and the real types. Every unit codes against them. Owners implement them in place.

```rust
// app/shell.rs (U05)
pub type Core = std::rc::Rc<std::cell::RefCell<vsesvit_core::Profile>>;
pub struct Shell { pub core: Core, pub engine: std::rc::Rc<dyn vsesvit_engine::Engine>,
                   pub location: ProfileLocation, pub scripted: bool, state: Entity<BrowserState> }
impl gpui::Global for Shell {}
impl Shell {
    pub fn get(cx: &App) -> &Shell;
    pub fn state(cx: &App) -> Entity<BrowserState>;
    pub fn engine(cx: &App) -> std::rc::Rc<dyn vsesvit_engine::Engine>;
    pub fn core(cx: &App) -> Core;
}

// state/mod.rs (U05)
pub enum BrowserStateEvent { BookmarksChanged, HistoryChanged, PrefsChanged(Vec<String>), ProfilesChanged,
    KeymapChanged, ThemeChanged, ExtensionActionsChanged, DownloadsStarted(Browsing),
    UpdateBanner(Option<crate::updates::Banner>), PrivateSessionEnded }
pub struct BrowserState {
    pub downloads: Entity<Downloads>, pub extensions: Entity<Extensions>, pub privacy: Entity<Privacy>,
    pub sync: Entity<Syncer>, pub updates: Option<Entity<Updates>>,
    /* private: registries, closed stacks, private session, MRU clock, session saver, profile registry */
}
impl EventEmitter<BrowserStateEvent> for BrowserState {}
impl BrowserState {
    pub fn windows(&self) -> &[WindowHandle<BrowserWindow>];                    // MRU first
    pub fn windows_of(&self, b: Browsing) -> Vec<WindowHandle<BrowserWindow>>;
    pub fn open_window(&mut self, b: Browsing, targets: Vec<Url>, cx: &mut Context<Self>) -> WindowHandle<BrowserWindow>;
    pub fn normal_window(&mut self, cx: &mut Context<Self>) -> WindowHandle<BrowserWindow>;
    pub fn window_activated(&mut self, w: WindowHandle<BrowserWindow>, cx: &mut Context<Self>);
    pub fn tab(&self, id: TabId) -> Option<Entity<Tab>>;
    pub fn window_of(&self, id: TabId, cx: &App) -> Option<WindowHandle<BrowserWindow>>;
    pub fn new_tab_entity(&mut self, b: Browsing, cx: &mut Context<Self>) -> Entity<Tab>;   // allocates TabId, registers
    pub fn tab_closed(&mut self, tab: &Entity<Tab>, place: ClosedPlace, cx: &mut Context<Self>); // pushes ClosedTab
    pub fn tab_discarded(&mut self, tab: TabId, cx: &mut Context<Self>);
    pub fn can_reopen_closed_tab(&self, b: Browsing) -> bool;
    pub fn take_closed(&mut self, b: Browsing, key: Option<ClosedKey>) -> Option<ClosedTab>;
    pub fn search_tabs(&self, b: Browsing, query: &str, cx: &App) -> Vec<tab_search::Row<TabId, ClosedKey>>;
    pub fn go_to_tab(&mut self, w: WindowHandle<BrowserWindow>, hit: tab_search::Hit<TabId, ClosedKey>, cx: &mut Context<Self>);
    pub fn tick(&mut self) -> u64;                                               // MRU clock
    pub fn schedule_session_save(&mut self, cx: &mut Context<Self>);
    pub fn save_session_now(&mut self, cx: &mut Context<Self>);
    pub fn bookmarks_changed(&mut self, cx: &mut Context<Self>);
    pub fn profile_used(&mut self, cx: &mut Context<Self>);
    pub fn profiles(&self) -> &vsesvit_core::profiles::Registry;
    pub fn profile_title(&self) -> Option<String>;
    pub fn pref<T: Clone>(&self, p: &vsesvit_core::prefs::Pref<T>, cx: &App) -> T;
    pub fn set_pref<T>(&mut self, p: &vsesvit_core::prefs::Pref<T>, v: &T, cx: &mut Context<Self>);  // writes + applies everywhere
    pub fn edit_keymap<R>(&mut self, edit: impl FnOnce(&mut Keymap) -> R, cx: &mut Context<Self>) -> R;
    pub fn apply_keymap(&mut self, cx: &mut Context<Self>);
    pub fn handle_invocation(&mut self, inv: Invocation, cx: &mut Context<Self>);
    pub fn adopt_popup(&mut self, opener: TabId, tab: TabId, url: Url, cx: &mut Context<Self>);
    pub fn open_from_page(&mut self, opener: TabId, url: Url, d: OpenDisposition, cx: &mut Context<Self>);
    pub fn open_from_outside(&mut self, url: Url, cx: &mut Context<Self>);
    pub fn run_routed_key(&mut self, tab: Option<TabId>, key: RoutedKey, cx: &mut Context<Self>);
    pub fn context_menu_command(&mut self, tab: TabId, item: ShellMenuItem, p: ContextMenuParams, cx: &mut Context<Self>);
}

// tab/mod.rs (U06)
pub enum TabChanged { Title, Favicon, Loading, Progress, Committed(CommitKind), History, Zoom, Find(FindResult),
    Capture, Audio, Sleep, Security, Fullscreen(bool), FocusChanged(bool), TakeFocus { next: bool }, ClosedByPage, EngineClosed }
pub enum Security { Secure, Insecure, Internal, NotApplicable }
pub enum TabIcon { Page(std::sync::Arc<gpui::Image>), Default, Faded(std::sync::Arc<gpui::Image>) }
impl EventEmitter<TabChanged> for Tab {}
impl Tab {
    pub fn id(&self) -> TabId; pub fn browsing(&self) -> Browsing; pub fn session_id(&self) -> vsesvit_core::session::TabId;
    pub fn display_title(&self) -> SharedString; pub fn committed_url(&self) -> Option<&Url>; pub fn session_url(&self) -> Option<Url>;
    pub fn link(&self) -> Option<&Url>; pub fn is_blank(&self) -> bool; pub fn icon(&self) -> TabIcon; pub fn security(&self) -> Security;
    pub fn ssl(&self) -> Option<&SslInfo>; pub fn loading(&self) -> bool; pub fn progress(&self) -> f32;
    pub fn can_go_back(&self) -> bool; pub fn can_go_forward(&self) -> bool; pub fn zoom(&self) -> Zoom;
    pub fn muted(&self) -> bool; pub fn audible(&self) -> bool; pub fn capturing(&self) -> Capturing;
    pub fn find_result(&self) -> Option<FindResult>; pub fn is_asleep(&self) -> bool; pub fn related(&self) -> bool;
    pub fn used(&self) -> u64; pub fn set_used(&mut self, tick: u64); pub fn last_active_ms(&self) -> i64; pub fn mark_active(&mut self, ms: i64);
    pub fn set_typed(&mut self, t: Option<String>); pub fn take_typed(&mut self) -> Option<String>;
    pub fn load(&mut self, url: Url, transition: Option<Transition>, cx: &mut Context<Self>);
    pub fn load_new_tab_page(&mut self, cx: &mut Context<Self>);
    pub fn reload(&mut self, bypass_cache: bool, cx: &mut Context<Self>); pub fn stop(&mut self, cx: &mut Context<Self>);
    pub fn go_back(&mut self, cx: &mut Context<Self>); pub fn go_forward(&mut self, cx: &mut Context<Self>);
    pub fn set_muted(&mut self, muted: bool, cx: &mut Context<Self>);
    pub fn sleep(&mut self, cx: &mut Context<Self>); pub fn wake(&mut self, cx: &mut Context<Self>);
    pub fn apply(&mut self, ev: TabEvent, cx: &mut Context<Self>);          // router entry point
}

// tab/model.rs (U06)
pub enum Insert { End, AfterOpener(TabId), Index(usize), Pinned(usize) }
pub enum TabListEvent { Inserted { tab: TabId, index: usize }, Removed { tab: TabId }, Moved { tab: TabId, to: usize },
    Selected { old: Option<TabId>, new: Option<TabId> }, PinnedChanged { tab: TabId }, GroupsChanged }
impl EventEmitter<TabListEvent> for TabListModel {}
impl TabListModel {
    pub fn tabs(&self) -> &[TabId]; pub fn pinned_count(&self) -> usize; pub fn selected(&self) -> Option<TabId>;
    pub fn index_of(&self, t: TabId) -> Option<usize>; pub fn is_pinned(&self, t: TabId) -> bool;
    pub fn place(&self, t: TabId) -> vsesvit_core::tab_place::TabPlace; pub fn window_tabs(&self) -> WindowTabs<TabId>;
    pub fn insert(&mut self, t: TabId, at: Insert, select: bool, cx: &mut Context<Self>);
    pub fn remove(&mut self, t: TabId, cx: &mut Context<Self>) -> Removed;   // index, pinned, group, picks next selection
    pub fn select(&mut self, t: TabId, cx: &mut Context<Self>);
    pub fn move_to(&mut self, t: TabId, index: usize, cx: &mut Context<Self>);
    pub fn set_pinned(&mut self, t: TabId, pinned: bool, cx: &mut Context<Self>);
    pub fn next_shown(&self, back: bool) -> Option<TabId>;
    pub fn group_of(&self, t: TabId) -> Option<&TabGroup>; pub fn rows(&self) -> Vec<tab_groups::Row<TabId>>;
    pub fn groups_in_order(&self) -> Vec<TabGroup>;
    pub fn change_groups<R>(&mut self, f: impl FnOnce(&mut TabGroups<TabId>, &WindowTabs<TabId>) -> (R, Vec<Step<TabId>>),
                            cx: &mut Context<Self>) -> R;                      // applies steps with settle held
    pub fn restore_groups(&mut self, tabs: Vec<(TabId, Option<TabGroup>)>, cx: &mut Context<Self>);
}

// overlay/mod.rs (U04)
pub enum OverlayKind { Menu, Bubble, Suggestions, Prompt, Warning, Toast, Tooltip, Notice, ExtensionPopup, TabSearch, GroupEditor }
pub enum OverlayAnchor { Rect(Bounds<Pixels>), Point(Point<Pixels>) }        // window coordinates
pub enum OverlayPlacement { Below { align: Align }, Beside { side: Side }, AtPoint, TopCentre, BottomCentre, BottomLeft }
pub enum OverlaySize { Fixed(Size<Pixels>), Width { width: Pixels, max_height: Pixels }, Content { max: Size<Pixels> } }
pub enum OverlayFocus { Passive, Interactive }
pub struct Dismiss { pub escape: bool, pub outside_click: bool, pub page_focus: bool, pub window_move: bool,
                     pub timeout: Option<std::time::Duration>, pub hide_when_inactive: bool }
pub struct OverlaySpec { pub kind: OverlayKind, pub anchor: OverlayAnchor, pub placement: OverlayPlacement,
                         pub size: OverlaySize, pub focus: OverlayFocus, pub dismiss: Dismiss, pub content: AnyView }
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct OverlayId(u64);
pub enum DismissReason { Escape, OutsideClick, PageFocus, WindowMoved, Timeout, Owner, Replaced }
pub enum OverlayEvent { Dismissed { id: OverlayId, reason: DismissReason } }
impl EventEmitter<OverlayEvent> for OverlayHost {}
impl OverlayHost {
    pub fn open(&mut self, spec: OverlaySpec, window: &mut Window, cx: &mut Context<Self>) -> OverlayId;
    pub fn open_menu(&mut self, menu: menu::Menu, anchor: OverlayAnchor, window: &mut Window, cx: &mut Context<Self>) -> OverlayId;
    pub fn close(&mut self, id: OverlayId, cx: &mut Context<Self>);
    pub fn close_kind(&mut self, kind: OverlayKind, cx: &mut Context<Self>);
    pub fn reposition(&mut self, id: OverlayId, anchor: OverlayAnchor, cx: &mut Context<Self>);
    pub fn resize(&mut self, id: OverlayId, size: Size<Pixels>, cx: &mut Context<Self>);
    pub fn is_open(&self, id: OverlayId) -> bool;
    pub fn native_window(&self, id: OverlayId) -> Option<vsesvit_engine::NativeParent>;   // extension popups host a child there
    #[cfg(feature = "self-test")] pub fn open_overlays(&self) -> Vec<OverlayInfo>;
}
// overlay/menu.rs
pub struct Menu { pub sections: Vec<Vec<MenuItem>> }
pub enum MenuAction { Action(Box<dyn gpui::Action>), Callback(std::rc::Rc<dyn Fn(&mut Window, &mut App)>) }
pub struct MenuItem { pub label: SharedString, pub icon: Option<VsIcon>, pub checked: Option<bool>, pub enabled: bool,
                      pub kind: MenuItemKind }
pub enum MenuItemKind { Run(MenuAction), Submenu(Menu), Custom(AnyView) }
// overlay/toast.rs
pub struct Toast { pub text: SharedString, pub action: Option<(SharedString, MenuAction)>, pub timeout: std::time::Duration }
pub fn toast(window: AnyWindowHandle, toast: Toast, cx: &mut App);
// overlay/bubble_slot.rs: one address-bar bubble at a time
impl BubbleSlot { pub fn show(&mut self, spec: OverlaySpec, window: &mut Window, cx: &mut Context<Self>) -> OverlayId;
                  pub fn current(&self) -> Option<OverlayId>; }   // emits BubbleChanged

// dialogs/host.rs, prompts.rs, library.rs (U04)
pub enum DialogParent { Browser(WindowHandle<BrowserWindow>), Plain(AnyWindowHandle) }
pub struct DialogSpec { pub title: SharedString, pub size: Size<Pixels>, pub content: AnyView, pub closable: bool }
pub fn present(parent: DialogParent, spec: DialogSpec, cx: &mut App) -> DialogHandle;
impl DialogHandle { pub fn close(&self, cx: &mut App); pub fn closed(&self) -> Task<()>; }
pub fn confirm(parent: DialogParent, heading: &str, body: &str, accept: &str, cx: &mut App) -> Task<bool>;
pub fn prompt_text(parent: DialogParent, heading: &str, initial: &str, accept: &str, cx: &mut App) -> Task<Option<String>>;
pub fn prompt_choice(parent: DialogParent, heading: &str, body: &str, options: Vec<String>, accept: &str, cx: &mut App) -> Task<Option<usize>>;
pub enum LibraryKind { Bookmarks, History, Downloads(Browsing), Settings, Extensions, About, Shortcuts, ManageProfiles }
pub fn present_library<V: Render>(opener: WindowHandle<BrowserWindow>, kind: LibraryKind, size: Size<Pixels>,
                                  build: impl FnOnce(&mut Window, &mut App) -> Entity<V>, cx: &mut App) -> AnyWindowHandle;

// platform (U03)
pub fn native_window(window: &Window) -> u32;
impl X11 { pub fn get() -> &'static X11; pub fn focus_window(&self, xid: u32); pub fn set_transient_for(&self, child: u32, parent: u32);
           pub fn top_level_at(&self, p: Point<Pixels>) -> Option<u32>; pub fn capture(&self, xid: u32) -> anyhow::Result<image::RgbaImage>; }
pub fn active_window_events(cx: &mut App) -> futures::channel::mpsc::UnboundedReceiver<Option<u32>>;
pub fn button_press_events(cx: &mut App) -> futures::channel::mpsc::UnboundedReceiver<Point<Pixels>>;   // screen px
pub mod activation { pub fn new_startup_id(cx: &App) -> String; pub fn activate(xid: u32, startup_id: Option<&str>); }
pub mod dirs { pub fn downloads() -> PathBuf; pub fn cache() -> PathBuf; pub fn host_name() -> String; }
pub mod portal { pub fn open_files(filters: &[Filter], title: &str) -> Task<Option<Vec<PathBuf>>>;
                 pub fn open_folder(title: &str, start: &Path) -> Task<Option<PathBuf>>;
                 pub fn save_file(title: &str, folder: &Path, name: &str, filters: &[Filter]) -> Task<Option<PathBuf>>; }
pub mod default_browser { pub fn state() -> DefaultBrowserState; pub fn make_default() -> anyhow::Result<()>; }

// engine_bridge (U07)
pub fn start(cx: &mut App, driver: Box<dyn EngineDriver>, events: UnboundedReceiver<BrowserEvent>);
pub mod keymap { pub fn install(cx: &mut App, keymap: &Keymap, ext: &ExtensionShortcuts);
                 pub fn suspend(cx: &mut App); pub fn keystroke(chord: Chord) -> String;
                 pub fn chord_for(event: &gpui::KeyDownEvent) -> Pressed; /* capture rules */ }
pub mod focus { impl FocusArbiter { pub fn chrome_wants_keyboard(&mut self, window: &mut Window, cx: &mut App);
                                    pub fn page_wants_keyboard(&mut self, tab: TabId, cx: &mut App); pub fn owner(&self) -> Owner; } }
pub mod requests { pub fn eval(cx: &mut App, tab: TabId, script: &str) -> Task<RequestResult>; /* and pdf, mhtml, cookies, capture */ }
pub mod hooks { pub fn snapshot(cx: &App) -> std::rc::Rc<std::cell::RefCell<HookSnapshot>>; }

// theme (U02)
pub fn init(cx: &mut App); pub fn apply(theme: vsesvit_core::prefs::Theme, cx: &mut App);
pub fn private_chrome(dark: bool) -> ChromePalette;
pub mod tokens { pub const TOOLBAR_H: Pixels; /* … every value of §3.3 … */ }
pub mod icons { pub enum VsIcon { Back, Forward, Reload, Stop, Home, SidebarLeft, SidebarRight, Private, NewTab, Download, Menu,
    Puzzle, Search, Secure, Insecure, Info, Star, StarFilled, ZoomIn, ZoomOut, Close, Pin, Volume, Muted, Camera, Microphone,
    Screen, Globe, Folder, FolderOpen, FolderNew, ChevronRight, ChevronDown, ChevronUp, History, Bookmark, SwitchTab, Check,
    Edit, Trash, Reset, OpenWindow, Shield, Settings, Palette, Keyboard, Warning, Error, Password, Package, More, MoveTo,
    CancelDownload, File(FileType), Print, Code, Copy, Link }
    pub struct VsesvitAssets; impl gpui::AssetSource for VsesvitAssets { /* … */ } }
pub mod avatar { pub struct Avatar; impl Avatar { pub fn new(name: &str, color: ProfileColor, picture: Option<PathBuf>, large: bool) -> Self; } }
pub mod motion { pub fn tab_row(cx: &App) -> Option<gpui::Animation>; /* None when reduce_motion */ }

// feature entities consumed by window, toolbar and omnibox (owners in parentheses)
impl Downloads /* U14 */ { pub fn started_this_session(&self, b: Browsing) -> bool; pub fn list(&self, b: Browsing) -> Vec<Download>;
    pub fn progress(&self, id: DownloadId) -> Option<Progress>; pub fn cancel(&mut self, id: DownloadId, cx: &mut Context<Self>);
    pub fn download_to(&mut self, tab: TabId, url: Url, path: PathBuf, cx: &mut Context<Self>); }
impl Syncer /* U18 */ { pub fn state(&self) -> &vsesvit_sync::status::State; pub fn is_signed_in(&self) -> bool;
    pub fn act(&mut self, a: vsesvit_sync::status::Action, cx: &mut Context<Self>); pub fn window_opened(&mut self, cx: &mut Context<Self>); }
impl Updates /* U18 */ { pub fn banner(&self) -> Option<Banner>; pub fn status(&self) -> Status; pub fn check(&mut self, cx: &mut Context<Self>);
    pub fn activate(&mut self, cx: &mut Context<Self>); pub fn window_opened(&mut self, cx: &mut Context<Self>); }
impl Extensions /* U16 */ { pub fn actions(&self, b: Browsing) -> Vec<ActionView>; pub fn toolbar_layout(&self, cx: &App) -> toolbar::Layout;
    pub fn activate(&mut self, id: &ExtensionId, window: WindowHandle<BrowserWindow>, cx: &mut Context<Self>); }
impl Privacy /* U12 */ { pub fn next_prompt(&self, tab: TabId) -> Option<NextPrompt>; pub fn answer(&mut self, tab: TabId, id: RequestId,
    a: Answer, cx: &mut Context<Self>); pub fn indicator(&self, tab: TabId) -> Option<(VsIcon, SharedString)>; }
// profiles/picker.rs (U15): pub fn run(dir: ProfilesDir) -> ExitCode;
// profiles/launch.rs (U15): pub fn open_profile(id: &ProfileId, cx: &mut App); pub fn notify(root: &Path);
```

### 5.3 Units

Each unit lists: files (§5.1); dependencies (units whose *implementation* it needs, beyond the frozen stubs); scope (the §2 rows it delivers); acceptance; spikes.

**U00 · engine-api** (wave 0)
- **Files:** `crates/vsesvit-engine/**`.
- **Depends:** none.
- **Scope:** §4.2 exactly, with docs on every item; `FakeEngine` (§4.5).
- **Acceptance:**
  - `cargo test -p vsesvit-engine --features fake`;
  - a CI grep rejecting `gpui` and `cef` in its `Cargo.toml`;
  - the CEF tasks review it as their contract.
- **Spikes:** S:9, S:10, S:13 and S:14 may add or remove members. Changes are additive and announced to all units.

**U01 · skeleton-boot** (wave 0)
- **Files:**
  - workspace `Cargo.toml` additions;
  - `crates/vsesvit/{Cargo.toml, src/main.rs}`;
  - `crates/vsesvit-gpui/{Cargo.toml, build.rs}`;
  - `src/lib.rs`, `src/boot/**`, `src/util/**`, `src/app/actions.rs`;
  - the stub tree of §5.1 with the §5.2 stubs.
- **Depends:** U00 (types only).
- **Scope:** §1.2 steps 1–10, CLI, headless commands, instance forwarding and listener, logging, `Repeating` timer, worker helper, `format_time`, the actions list.
- **Acceptance:**
  - the 8 `cli.rs` unit tests;
  - instance tests: forward to an in-process listener; status and stderr relayed; the unreachable message;
  - `vsesvit --version`;
  - `cargo check` of the whole tree with stubs;
  - the binary links with an rpath (`readelf -d` shows `$ORIGIN`).
- **Spikes:** S:5 (that `Profile::open` starts no thread before CEF initialize).

**U02 · theme** (wave 0)
- **Files:** `src/theme/**`, `assets/themes/*`, `assets/icons/*` (+ `LICENSE-LUCIDE`), `examples/theme_gallery.rs`.
- **Depends:** none.
- **Scope:** §3 entirely: init, follow the desktop, forced modes, private palette, tokens, icons, `VsesvitAssets`, avatar, group and profile colours, motion.
- **Acceptance:**
  - the gallery shows light and dark with every icon;
  - unit tests: core colour table, avatar initial, `reduce_motion` disables `tab_row`.

**U03 · platform-x11** (wave 0)
- **Files:** `src/platform/**`, `examples/x11_probe.rs`.
- **Depends:** none.
- **Scope:** §1.10 (focus, transient-for, watchers, capture, `top_level_at`, startup ids); dirs; portal file dialogs with filters (ashpd); default browser.
- **Acceptance:** `x11_probe` on a private X display:
  - opens a GPUI window with a foreign X child (an `x11rb`-created window standing in for CEF);
  - moves focus both ways;
  - logs `_NET_ACTIVE_WINDOW` changes and button presses;
  - captures a PNG that shows the child's colour.
- **Spikes:** S:2, S:3, S:11, S:18 (co-owned with `spike-core`; reuse its `xwin.rs` findings).

**U04 · overlay-dialogs** (wave 1)
- **Files:** `src/overlay/**`, `src/dialogs/{mod,host,prompts,library}.rs`, `examples/overlay_demo.rs`.
- **Depends:** U02, U03.
- **Scope:**
  - §1.7: both backends, dismissal, `overlay::menu` (submenu levels, keyboard navigation, accelerator hints from `window.keystroke_text_for`), tooltips, toasts, `BubbleSlot`;
  - §1.8: `DialogHost`, prompts (exact semantics: trimmed, empty means cancel, destructive with Cancel as default), library registry.
- **Acceptance:**
  - `overlay_demo` over an X child: a menu with a submenu, a passive list driven from an entry, an interactive popover with an entry, IME input in it, outside-click dismissal on the child;
  - TestAppContext tests for overlay bookkeeping and the library rules (`a_window_of_its_own_opens_once_and_closes_with_its_browser_window`).
- **Spikes:** S:3 (Popup) or S:4 (Cutout) decides the default backend.

**U05 · browser-state** (wave 1)
- **Files:** `src/app/{mod,shell,quit}.rs`, `src/state/**`.
- **Depends:** U00, U06 (Tab and TabListModel implementations, for integration tests only).
- **Scope:** the `Shell` global; `BrowserState` (§5.2); quit, shutdown and restart; and every §2.2 row owned by `state/`:
  - window and tab registries;
  - closed stacks (verbatim port with tests);
  - session snapshot, restore and throttle;
  - private session;
  - live prefs fan-out;
  - `sync_applied`;
  - history, title, favicon and zoom hooks;
  - Memory Saver sweep;
  - invocations;
  - startup windows.
- **Acceptance:** tests with `FakeEngine`:
  - no session write without a normal window;
  - 2 s throttle;
  - `closing_the_last_tab_of_the_last_window_keeps_it_in_the_session`;
  - `a_restored_session_keeps_its_active_window_first`;
  - pinned and grouped tabs restored;
  - history title rules;
  - private session ends with the last private window;
  - the three invocation outcomes;
  - `releasing_the_profile_frees_its_lock`.

**U06 · tab-core** (wave 1)
- **Files:** `src/tab/{mod,model,navigation,content,error_page,sleep}.rs`.
- **Depends:** U00.
- **Scope:** `Tab` and `TabListModel` (§5.2); §2.3 rows for:
  - transitions;
  - display rules and security;
  - HTTPS-only `Upgrades` and `navigation_policy`;
  - popup decision;
  - NTP and private page;
  - error, certificate, crash and HTTPS-only pages (port `error_page.rs` and its HTML);
  - sleep and wake;
  - the edit-tracking document-start script.
- **Acceptance:** ported tests:
  - `display_title`, `session_uri`, `is_blank`, `security`;
  - a stopped load or a download leaves no transition; typed-over-load counts;
  - HTTPS-only upgrade, warn and continue with `FakeEngine`;
  - error page HTML snapshots;
  - `TabListModel`: insert after opener, pinned rules, `select_after_closing`, `next_shown` skipping collapsed groups, steps applied with settle held.

**U07 · engine-bridge-keys** (wave 1)
- **Files:** `src/engine_bridge/**`.
- **Depends:** U00.
- **Scope:** §4.3 (pump, router, requests), `Hooks` (§4.3), keymap install, suspend and capture-key mapping (§1.9), `KeyRouting`, `FocusArbiter` (§1.6).
- **Acceptance:**
  - chord → keystroke for every core `Key`;
  - routing table for the default keymap (reserved vs page-first);
  - router delivery with `FakeEngine`;
  - `Hooks` never borrows `App` (a debug guard panics if it does);
  - capture-key rules (`a_key_press_becomes_the_chord_it_names`, `capture_keys_and_unbindable_presses_are_told_apart`) on GPUI `KeyDownEvent`s.
- **Spikes:** S:2, S:5, S:6.

**U08 · window-frame** (wave 2)
- **Files:** `src/window/**`.
- **Depends:** U02–U07.
- **Scope:** every §2.1 row owned by `window/`:
  - composition and lifecycle;
  - close ordering;
  - title;
  - layouts, sidebar sizing, probe and classify;
  - page area;
  - CSD frame;
  - all window and tab actions (`TabOp`, `GroupOp`, Copy Link, Home, view-source, print, DevTools, zoom, fullscreen);
  - `selection_changed`;
  - popups and open-URL adoption;
  - fullscreen and notice;
  - sharing bar;
  - activation;
  - cross-kind send-back;
  - tab transfer.
- **Acceptance:**
  - TestAppContext: `classify` for L, R and T; `close_if_empty`; the four title cases; focus on switch; Ctrl+S toggles the sidebar; `a_tab_dropped_into_a_window_of_the_other_kind_goes_back_to_its_own`;
  - manual run with the CEF engine under X11 [S:1].
- **Spikes:** S:1, S:7, S:10.

**U09 · tabs-ui** (wave 2)
- **Files:** `src/tabs_ui/**`.
- **Depends:** U02, U04, U06.
- **Scope:** sidebar, strip, row and indicator, tab menu, tab search, group editor, drag (§2.1 rows).
- **Acceptance:**
  - `reorder_target` tests;
  - keyboard over hidden rows (port of `the_keyboard_never_lands_on_a_collapsed_groups_rows_in_the_tab_list`);
  - animations happen only on screen;
  - exact tab-menu lines (top and sidebar wording);
  - tab search rows, headings and wrap with core data;
  - self-test probes (`row_counts`, `tab_menu_lines`, `tab_search()…`, `group_header`, `tab_row_buttons`, `tab_indicator`, `click_tab_indicator`, `tab_look`).

**U10 · toolbar** (wave 2)
- **Files:** `src/toolbar/{mod,main_menu,update_banner}.rs`, `src/find_bar.rs`.
- **Depends:** U02, U04, U06.
- **Scope:** toolbar row and slots, main menu with zoom row, update banner, find bar.
- **Acceptance:**
  - Reload ↔ Stop; Back and Forward enablement;
  - main menu lines;
  - find status wording ("1 match", "{n} matches", "No matches") and error style;
  - find retargets on tab switch;
  - Escape refocuses the page.

**U11 · omnibox** (wave 2)
- **Files:** `src/omnibox/{mod,edit,field,suggestions,progress,zoom_bubble}.rs`.
- **Depends:** U02, U04, U05, U06.
- **Scope:** §2.4 address-bar and omnibox rows.
- **Acceptance:**
  - the 8 `address_bar.rs` tests on the edit model (inline rules, refill keeps the highlight, Escape stages, Ctrl+Enter);
  - suggestion rows from core;
  - exactly one `/suggest` request (FixtureServer);
  - progress line timing;
  - self-test probes (`observe`, `type_text`, `press`, `submit_text`, `focus_for_typing`, `click_zoom`, `shown_zoom`, `progress`).

**U12 · site-privacy** (wave 3)
- **Files:** `src/omnibox/{site_info,permission_prompt}.rs`, `src/tab/permissions.rs`, `src/privacy/**`.
- **Depends:** U04, U05, U06, U11.
- **Scope:** §2.4 site-info, permission, cookie and tracker rows; Clear Browsing Data back end; site data back end.
- **Acceptance:**
  - port of the 11 `permissions.rs` tests with `FakeEngine`;
  - tracker filter decisions on core's `TrackerList`;
  - cookie `site_choices` with exceptions;
  - certificate section from a fixture DER chain;
  - prompt guard (an early click leaves the request waiting).
- **Spikes:** S:16.

**U13 · bookmarks** (wave 3)
- **Files:** `src/bookmarks/**`, `src/favicons.rs`, `src/dialogs/{bookmarks,history}.rs`.
- **Depends:** U04, U05.
- **Scope:** §2.5 bookmark and favicon rows, and §2.6 Bookmarks and History windows.
- **Acceptance:** tests for `fitting`, `zone`, `destination`, `folder_choices` depth, `urls_must_parse`, form save, label fallback, separator-only menu, deep-folder delete confirm, 32 px favicon, Bookmarks-window search and moves, History device rows.

**U14 · downloads** (wave 3)
- **Files:** `src/downloads/**`, `src/toolbar/downloads_button.rs`, `src/dialogs/downloads.rs`, `src/save_page.rs`.
- **Depends:** U03, U04, U05.
- **Scope:** §2.5 download rows, the Downloads window, Save Page As.
- **Acceptance:** with `FakeEngine` events:
  - `two_downloads_with_the_same_name_get_files_of_their_own`;
  - `a_script_waits_under_its_unconfirmed_name_until_kept_or_discarded`;
  - `a_download_into_a_folder_that_cannot_be_made_says_why`;
  - live progress then cancelled;
  - save-page name rules;
  - pause and resume states.
- **Spikes:** S:17.

**U15 · profiles** (wave 3)
- **Files:** `src/profiles/**`, `src/toolbar/profile_button.rs`.
- **Depends:** U02, U03, U04, U05.
- **Scope:** picker, profile button and menu, Add/Edit dialog, Manage Profiles, launch and notify, remove flows.
- **Acceptance:** `the_picker_lists_every_profile_and_can_add_one`; dialog validation (name ≤ 64, accept disabled while blank); menu lists with the current profile checked.
- **Spikes:** S:18.

**U16 · extensions** (wave 3)
- **Files:** `src/extensions/**`, `src/dialogs/{extensions,extension_prompts}.rs`.
- **Depends:** U04, U05.
- **Scope:** §2.7 extension rows.
- **Acceptance:**
  - install prompt lines for the probe;
  - progress row counter with overlapping installs;
  - enable and remove failure toasts;
  - `sync_extensions` contents after install, disable, withheld update and uninstall (`FakeEngine`);
  - toolbar pin and move prefs.
- **Spikes:** S:12, S:13. The toolbar and popup parts start only after `ext-api` reports.

**U17 · settings** (wave 3)
- **Files:** `src/dialogs/settings/**`, `src/dialogs/{about,shortcuts_help}.rs`.
- **Depends:** U04, U05, U07 (capture), U12 (site data API), U18 (`Syncer`; stub until then).
- **Scope:** §2.6 Settings rows, About, Keyboard Shortcuts help.
- **Acceptance:**
  - `a_download_folder_shows_as_named_not_as_markup`;
  - `picture_in_picture_synced_from_windows_is_not_listed`;
  - `the_list_shows_every_engine_and_follows_changes`;
  - `the_editor_marks_what_core_refuses_and_saves_through_it`;
  - shortcut capture notes;
  - never-write-back on refresh;
  - page names selectable.

**U18 · sync-updates-welcome** (wave 3)
- **Files:** `src/sync.rs`, `src/updates/**`, `src/dialogs/{welcome,passphrase}.rs`.
- **Depends:** U04, U05.
- **Scope:** §2.7 sync and update rows; Welcome; passphrase dialogs.
- **Acceptance:**
  - all `lifecycle.rs` tests;
  - sync `due`, `settle`, `refresh`, `prompt_due`, `stored_state`, deletion waiting, `Repeating` cancel-in-run;
  - `the_offer_waits_for_the_welcome_and_comes_once`;
  - `a_dialog_that_cannot_close_ends_unanswered_when_it_is_moot_or_its_window_closes`.

**U19 · self-test and switch-over** (wave 4)
- **Files:** `src/self_test/**`, `src/test_support.rs`; deleting `crates/vsesvit-gtk` and `crates/vsesvit-webext` and their workspace lines (CI package list coordinated with packaging).
- **Depends:** all.
- **Scope:** §2.8; the check-list order guard; the report; screenshots.
- **Acceptance:** `vsesvit --self-test OUT_DIR` passes on a private X display (kwin `--virtual --xwayland`, as `spike-core` uses, or Xvfb), and `report.json` lists every check of §2.8.

### 5.4 Waves and the critical path

```
wave 0:  U00 engine-api    U01 skeleton-boot    U02 theme    U03 platform-x11          (fully parallel)
            │                  │ (stubs for all)      │             │
wave 1:  U05 browser-state   U06 tab-core   U07 engine-bridge-keys   U04 overlay-dialogs (parallel; code against stubs)
            └──────┬───────────┴──────┬─────────────┘                  │
wave 2:      U08 window-frame    U09 tabs-ui    U10 toolbar    U11 omnibox              (parallel)
                   │   ← first runnable browser: window, tabs, omnibox navigation, find, menus →
wave 3:  U12 site-privacy  U13 bookmarks  U14 downloads  U15 profiles  U16 extensions  U17 settings  U18 sync-updates-welcome
wave 4:  U19 self-test + switch-over
```

- **Critical path:** U00/U01 → U06/U07 → U08 → U19.
- **First demo milestone:** end of wave 2, with U00–U11 and `vsesvit-cef`'s basic browser creation.
- **Units that wait on `ext-api`:** only U16's toolbar and popup work. Everything else can proceed with the provisional extension API.

---

## 6. Spike dependencies

| # | Question | Spike | Gates | If it fails |
|---|---|---|---|---|
| S:1 | Can a Chrome-style child be created under a GPUI X11 window, then mapped, unmapped and resized through its XID? Is the scale factor consistent? | spike-core | U08 page area | — (the decided architecture) |
| S:2 | Focus hand-over:<br>• does typing in GPUI reach Chromium while the pointer is over the page (pointer-focus rule)?<br>• does `SetInputFocus` on the GPUI top-level work after a page click?<br>• does `host.set_focus(1)` move X focus back? | spike-core | U03, U07; need for GPUI patch G1 | carry patch G1 (focus proxy) |
| S:3 | GPUI `WindowKind::PopUp` over the CEF child under XWayland and Mutter or KWin:<br>• stacking above the child;<br>• screen positioning, also at fractional scales;<br>• transparency and shadows;<br>• `SetInputFocus` to the popup and XIM input inside it;<br>• XI2 raw button events for outside clicks | spike-core | U04 default backend | Cutout backend (S:4) |
| S:4 | XShape cut-out on the CEF child: does Chromium reset the shape on resize? Flicker? | spike-core | U04 fallback | menus become Dialog-like windows |
| S:5 | External pump plus GLib iteration inside GPUI's loop; IME in the page via GtkUi; GTK file chooser; single thread before `CefInitialize` | spike-core | U01, U07 | `multi_threaded_message_loop` (loses page IME, `cef-windowing.md` §1.2) |
| S:6 | `OnPreKeyEvent`/`OnKeyEvent` routing in a child; blocking Chrome accelerators in `OnChromeCommand`; GPUI keypad key names | spike-core | U07 | GPUI-side intercept only while chrome has focus |
| S:7 | Page fullscreen in a child: `OnFullscreenModeChange`; does Chromium show its own exclusive-access bubble? | spike-core | U08 | own notice only |
| S:8 | Chrome status bubble, find, `IDC_DEV_TOOLS_CONSOLE`, native context menu with `OnBeforeContextMenu` items in a `TYPE_POPUP` child | spike-core | U07 hooks, U08 | GPUI-drawn context menu from `RunContextMenu`; own link-preview overlay |
| S:9 | Showing shell HTML at `about:blank` (NTP) and under a failed URL (error pages), keeping the address bar and Reload semantics | spike-core | U06 | NTP from a `vsesvit://newtab` scheme shown as blank by the omnibox |
| S:10 | `XReparentWindow` of a CEF child into another GPUI window (move tab, drag between windows) | spike-core | U08, U09 | close and reopen at the URL |
| S:11 | `GetImage` capture including the CEF child (Xvfb, kwin virtual) | spike-core | U03, U19 | DevTools `Page.captureScreenshot` composited into the GPUI capture |
| S:12 | Loading core-installed unpacked extensions into Chromium: enable/disable, incognito allowance, uninstall, load errors | ext-load | U16, U17 | load at startup only (`--load-extension`), restart on change |
| S:13 | Action state, popups with tab context, `activeTab`, commands with chrome focus, `tabs.create` windows, extension tab and window model | ext-api | U16 toolbar and popup, U19 extension checks | manifest defaults (D16) |
| S:14 | Detecting audio playing and zoom changes (Ctrl+wheel); reconciling Chromium's per-host zoom map with core's | spike-core | U06, U09 | no speaker indicator; poll the zoom after Ctrl+wheel |
| S:15 | Chrome print preview inside a child; `print_to_pdf` | spike-core | U08 | `CefPrintHandler` with portal printing |
| S:16 | Which permission callback fires in Chrome style; does `ACCEPT` persist in Chromium content settings? | spike-core | U12 | mirror and re-assert core state after each answer |
| S:17 | `OnBeforeDownload` handled in Chrome style suppresses Chromium's download bubble; the `can_download` default | spike-core | U14 | — |
| S:18 | `_NET_ACTIVE_WINDOW` tracking; startup-notification ids raising a window from a forwarded launch | spike-core | U03, U15 | GPUI `activate()` only (may only request attention) |

---

## 7. Verification log

Nothing was built for this design. These read-only checks were run on 2026-10-16 against the crate sources in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`, abbreviated `$R` below. Command output summaries are given where they are short.

| Claim | Command (abbreviated) | Result |
|---|---|---|
| gpui-component 0.7.3 widget set | `ls $R/gpui-component-0.7.3/src/` and `sed -n '80,160p' src/lib.rs` | modules include `menu/{popup_menu,context_menu,dropdown_menu,app_menu_bar}`, `popover`, `dialog/{dialog,alert_dialog}`, `sheet`, `notification`, `setting/{settings,page,group,item}`, `sidebar`, `tab/{tab,tab_bar}`, `tree`, `virtual_list`, `list`, `table`, `input`, `switch`, `checkbox`, `radio`, `select`, `combobox`, `slider`, `kbd`, `badge`, `avatar`, `spinner`, `progress`, `carousel`, `stepper`, `title_bar`, `window_border`, `resizable`, `scroll`, `tooltip`, `hover_card`, `theme` |
| `PopupMenu` API | `grep -n "pub fn" $R/gpui-component-0.7.3/src/menu/popup_menu.rs` | `PopupMenu::build(window, cx, f)`, `menu`, `menu_with_check`, `menu_element`, `submenu`, `separator`, `action_context`, `max_h`, `scrollable`; `PopupMenuItem::{new, submenu, element, separator, label, link}` |
| `Popover` API | `grep -n "pub fn" …/popover.rs` | `Popover::new(id).anchor(..).trigger(..).content(..).open(..).on_open_change(..).overlay_closable(..).track_focus(..)` |
| `Theme` API, appearance on Linux | `sed -n '354,400p' …/theme/mod.rs` | `Theme::change(mode, window, cx)`, `Theme::sync_system_appearance` (comment: "Better use window.appearance() for avoid error on Linux") |
| Theme JSON colour keys | `python3 -c "json.load(open('…/theme/default-theme.json'))…"` | keys include `background`, `title_bar.background`, `sidebar.background`, `tab_bar.background`, `popover.background`, `primary.background`, `accent.background`, `muted.foreground`, `input.border`, `ring`, `list.hover.background`, `overlay`, `window.border` |
| `InputState` capabilities | `grep -n "pub fn" $R/gpui-base-0.7.3/src/input/base/state.rs`; `sed -n '120,135p'` | `set_text_align`, `set_value`, `select_all`, `selected_range`, `set_selected_range`, `set_placeholder`, `set_submit_on_enter`; `InputEvent::{Change, PressEnter{secondary, shift}, Focus, Blur}` |
| In-window dialogs and notifications | `grep -n "fn " …/window_ext.rs` | `open_dialog`, `open_alert_dialog`, `close_dialog`, `push_notification`, `open_sheet` |
| Reduced motion | `sed -n 1,60p $R/gpui-base-0.7.3/src/reduce_motion.rs` | reads the portal's `org.freedesktop.appearance` `reduced-motion` into `App::reduce_motion` and follows changes |
| Icons: set, licence, size | `head $R/gpui-kit-assets-0.7.3/LICENSE-LUCIDE`; `ls assets/icons \| wc -l`; README | ISC licence; 1,830 SVGs (Lucide 1.43.0 + 12 GPUI Kit icons); `AllAssets` embeds all (+1.02 MiB) |
| Icon names used in §3.4 exist | a shell loop testing `[ -f assets/icons/<name>.svg ]` for 81 names | 78 present; `history`, `trash-2`, `file-video` absent → replaced by `clock`, `trash`, `file-play` (present) |
| X11 PopUp = override-redirect at root bounds | `sed -n '470,700p' $R/gpui-pre-linux-0.3.9/src/linux/x11/window.rs` | `.override_redirect((params.kind == WindowKind::PopUp) as u32)`; created on the root at `params.bounds`; `_NET_WM_WINDOW_TYPE_NOTIFICATION`; `AnchoredPopup` → `PopupNotSupportedError` |
| X11 Dialog windows | same file | `WM_TRANSIENT_FOR` to the parent, `_NET_WM_WINDOW_TYPE_DIALOG`, `_NET_WM_STATE_MODAL` |
| Dialog parent = keyboard-focused window | `sed -n '1685,1700p' …/x11/client.rs` | `parent_window = state.keyboard_focused_window…` |
| GPUI never sets X input focus | `grep -rn set_input_focus $R/gpui-pre-linux-0.3.9/src` | no matches |
| `FocusOut` deactivates without checking the detail; `FocusIn` binds XIM | `sed -n '1055,1090p' …/x11/client.rs` | `Event::FocusOut` → `window.set_active(false)`; `Event::FocusIn` → `keyboard_focused_window = Some(..)`, `xim_handler.window = event.event`, `enable_ime()` |
| GPUI X11 uses core key events on its top-level, no focus proxy | `grep -n "KEY_PRESS\|InputOnly\|focus_proxy" …/x11/{window,client}.rs` | `EventMask::KEY_PRESS` on its window; no InputOnly or proxy window |
| X11 windows hand out their XCB id | `grep -n "XcbWindowHandle" $R/gpui-pre-linux-0.3.9/src/linux/x11/window.rs` | `rwh::XcbWindowHandle::new(non_zero)` at lines 324 and 345 (no `unimplemented!()`) |
| `activate()` uses `_NET_ACTIVE_WINDOW` with `CurrentTime` | `sed -n '1525,1545p' …/x11/window.rs` | as stated |
| Appearance on X11 from the portal | `grep -rn "color_scheme" …/linux/xdg_desktop_portal.rs` | `settings.color_scheme()` + `receive_color_scheme_changed()` |
| `gpui_platform::linux(WindowingModes)` and features | `grep -n "pub fn" $R/gpui-pre-platform-0.3.9/src/*.rs`; `[features]` | `pub fn linux(allowed_modes: gpui::WindowingModes) -> gpui::Application`; features `x11`, `wayland`, `font-kit`, `test-support` |
| `WindowOptions` fields | `sed -n '2491,2560p' $R/gpui-pre-0.3.9/src/platform.rs` | `window_bounds`, `titlebar`, `focus`, `show`, `kind`, `is_movable`, `inactive_frame_interval`, `is_resizable`, `display_id`, `window_background`, `app_id`, `window_min_size`, `window_decorations`, `icon` (X11), `tabbing_identifier` |
| Window and app APIs used | `grep -n "pub fn …" $R/gpui-pre-0.3.9/src/{window.rs,app.rs,app/context.rs}` | `Window::{dispatch_keystroke, dispatch_event, start_window_move, set_window_title, set_app_id, toggle_fullscreen, is_fullscreen, appearance, bounds, scale_factor, on_next_frame, blur, on_window_should_close}`; `Context::{observe_window_bounds, observe_window_activation, observe_window_appearance, observe_keystrokes}`; `App::{reduce_motion, prompt_for_paths, prompt_for_new_path, reveal_path, open_with_system, show_system_notification, set_app_identity, bind_keys, clear_key_bindings, intercept_keystrokes, on_app_quit, displays}`; `impl HasWindowHandle for Window` |
| `dispatch_keystroke` falls back to the input handler | `sed -n '5548,5575p' …/window.rs` | dispatches `KeyDown`, then `input_handler.dispatch_input(key_char)` |
| `Icon::path` for custom SVGs | `grep -n "pub fn" $R/gpui-component-0.7.3/src/icon.rs` | `Icon::new(..)`, `Icon::path(..)` |
| Core shortcut model | `sed -n 15,110p crates/vsesvit-core/src/shortcuts.rs` | `Mods{ctrl,alt,shift}`, the `Key` list (letters, digits, F1–F24, Tab, Space, Insert, Delete, Home, End, PageUp, PageDown, arrows, Plus, Minus, Equal, Comma, Period, Slash, Question, Semicolon, keypad); `Chord::new` needs Ctrl, Alt or an F-key; `Command` list as in shell-core §16 |
| Core public modules | `grep -n "^pub mod" crates/vsesvit-core/src/lib.rs` | `address`, `bookmarks`, `certificate`, `clean_url`, `cookies`, …, `tab_groups`, `tab_place`, `tab_search`, `testkit` (feature), `trackers`, `vault`, `view_source`, `zoom` |
| `spike-core` uses its own x11rb connection for geometry, SHAPE, focus and screenshots | `head -40 /tmp/vsesvit-scratch/spike/src/xwin.rs` | module doc says exactly that; the design aligns with it |
