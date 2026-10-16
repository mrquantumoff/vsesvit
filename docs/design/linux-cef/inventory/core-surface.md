# Inventory: the vsesvit-core surface the shells use

Scope: everything in `crates/vsesvit-core/src/` that the GTK shell (`crates/vsesvit-gtk`) and the
WinUI shell (`crates/vsesvit-winui`) call, with special depth on `crates/vsesvit-core/src/extensions/`,
plus the `vsesvit_sync` / `vsesvit_update` calls a shell makes. Written so a new GPUI + CEF Linux
shell can reproduce every user-visible feature without reading the old code. No architecture is
proposed here.

Checkout read: `/home/dy/git/vsesvit-cef` (branch `linux-cef-gpui`, HEAD `27896e0`). CEF facts were
checked against `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cef-dll-sys-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs`
(`CEF_VERSION = "154.0.34+g14c5a08+chromium-154.0.8037.98"`).

## Legend

- **(KEEP)**: product behavior; reproduce it. Core already implements the policy; the shell must call it.
- **(WEBKIT-ONLY)**: exists only because of a WebKitGTK limitation or the home-grown runtime; it disappears with Chromium.
- **(NEEDS-CEF: ...)**: the shell must drive this from a CEF hook. The likely CEF API is named; names were checked in the
  bindings unless marked "(inference)".
- Statements are verified by reading the code unless marked **(inference)**.
- "UI thread" means the thread that owns the `Profile`. Core's `Profile` is `!Send` (`PhantomData<*const ()>`, `lib.rs`).

---

## 0. Ground rules core imposes on any shell

1. **One `Profile` per process, owned by the UI thread** (`crates/vsesvit-core/src/lib.rs`, `struct Profile`). Every
   accessor takes `&mut self` (reads too). GTK keeps it as `Rc<RefCell<Profile>>` (`crates/vsesvit-gtk/src/profile.rs`,
   `type Core`); WinUI in a `thread_local!`. Borrow for one call only; core never calls back into the shell.
2. **Slow work is a `Send` value with no database handle**, run on a worker, committed back on the UI thread:
   `extensions::InstallJob::run`, `extensions::UpdateCheck::run`, `favicons::FaviconFetch::run`,
   `suggest::SuggestRequest::run`, `profiles::AccountPicture::fetch`, and every `vsesvit_sync` / `vsesvit_update` network step.
3. **Core has no observer/event system.** Local mutations return what they changed; remote changes come back as
   `sync::ApplyReport::changed: sync::Changed`. The shell refreshes its views itself.
4. **Every write is one SQLite transaction with at most one HLC stamp and one change seq** (`Profile::write`, `db::Tx`).
   A write that changes nothing mints nothing (no sync traffic). `Profile::change_seq()` moves forward on every
   uploadable change; the shell compares it to decide when to sync soon.
5. **Private windows never write**: the shell routes each kind of write through one function that skips private tabs, or
   uses the `Browsing::Private` variants of core APIs, which keep state in `Profile.private` (memory) until
   `Profile::end_private_session()`.

CEF consequence (verified from bindings doc comments): `CefRequestHandler::OnBeforeBrowse`,
`OnCertificateError`, the download and permission handlers run on the **CEF UI thread**, but
`CefRequestHandler::GetResourceRequestHandler` and every `CefResourceRequestHandler` method
(`OnBeforeResourceLoad`, `GetCookieAccessFilter`, `OnResourceRedirect`, `OnResourceResponse`,
`OnResourceLoadComplete`, ...) run on the **browser-process IO thread**. Those cannot touch `Profile`; the shell must hand
them plain `Send` snapshots (for example a `TrackerList` clone plus each tab's `TrackingProtection` level and page host).

---

## 1. Process start, profile choice and profile lifetime

### 1.1 What the user sees

- `vsesvit [URL...] [--profile-dir DIR]`, `vsesvit --self-test OUT_DIR [--network]`, `--check-for-updates`, `--update`,
  `-h/--help`, `--version` (`crates/vsesvit-gtk/src/cli.rs`, `USAGE`). WinUI also accepts `--load-extension <dir>`
  (installs unpacked dirs through core at start, `crates/vsesvit-winui/src/extensions.rs`, `start_extensions`).
- A launch that names no profile opens the profile used last; with more than one profile, the picker switched on and no
  profile running, it shows the picker window "Who's Using Vsesvit?" with one card per profile (avatar + name), an "Add"
  card and a "Show on startup" switch (`crates/vsesvit-gtk/src/profiles.rs`, `run_picker`). A launch with URLs skips the
  picker. Choosing a profile starts it and closes the picker.
- A second launch on a profile that already runs hands its command line (URLs) to the running process and exits; an empty
  command line just brings its window forward.
- A profile removed from the list while it runs closes all its windows without saving.

### 1.2 Core calls

| Step | Call | Decides |
|---|---|---|
| list dir | `profiles::ProfilesDir::standard()` (`<data_local_dir>/vsesvit/profiles` on Linux) or `ProfilesDir::at(path)` (self-test) | where `profiles.json` lives |
| housekeeping | `ProfilesDir::sweep() -> Registry` | deletes data of removed profiles no process holds |
| choose | `ProfilesDir::startup(has_urls: bool) -> profiles::Startup` (`Open(ProfileId)` / `Picker`) | last used vs picker (`Registry::startup(has_urls, any_running)`) |
| locate | `ProfilesDir::locate(root: &Path) -> Option<ProfileId>`, `ProfilesDir::root(&ProfileId) -> PathBuf`, `profiles::Home { dir, id }` | whether the profile is in the list (profile menu exists only then) |
| open | `Profile::open(root: &Path, OpenOptions) -> Result<Profile, OpenError>` | locks `<root>/LOCK` (100 ms patience, `profiles::LOCK_PATIENCE`), migrates, mints device id, loads bookmarks, wipes `staging/`, GCs extension dirs, prunes favicons |
| errors | `OpenError::Locked` (another process: forward command line), `OpenError::TooNew { found, supported }`, `Db`, `Io` | GTK prints `vsesvit: cannot open the profile at ...` and exits 1 for anything but `Locked` |
| listed | `ProfilesDir::opened(&ProfileId) -> io::Result<Registry>` | lists the profile if new ("Person N"), marks last used; also called whenever a window comes forward |
| removed? | `Registry::is_removed(&ProfileId)` | close at once |
| first run | `Profile::is_new()`, `onboarding::should_show(&mut Profile)` | welcome dialog on the launch that created the profile |

`OpenOptions { time: TimeSource::System, new_device_id: None, chrome_version: String, key_store: vault::KeyStore }`.
`chrome_version` is sent to the Chrome Web Store as `prodversion`; default `extensions::DEFAULT_CHROME_VERSION = "150.0.0.0"`.
WinUI passes the WebView2 runtime version (`crates/vsesvit-winui/src/lib.rs`, `engine::available_version()`).
**(NEEDS-CEF)** pass Chromium's version: `cef_dll_sys::CHROME_VERSION_{MAJOR,MINOR,BUILD,PATCH}` = `154.0.8037.98`.
`key_store`: `KeyStore::System` (Secret Service "Vsesvit Safe Storage" on Linux, else Basic) or `KeyStore::Basic` (tests).

Single instance per profile: GTK derives a GApplication id from the profile path
(`app_id_for_profile`: plain `dev.mrquantumoff.vsesvit` for the standard Default profile, else
`dev.mrquantumoff.vsesvit.Profile<sha256[..16]>`) so GApplication forwards the command line (**WEBKIT-ONLY**
mechanism, GTK-only; the new shell needs its own forwarding, e.g. D-Bus or a socket). Starting another profile:
`profile::launch` runs `vsesvit --profile-dir <root>` through `gio::AppInfo` with the display's launch context so it
carries a Wayland activation token; an AppImage launches the image (`vsesvit_update::Installation::detect()`).
`profile::notify(root)` re-runs the program without a token to make another profile's process re-read the list.

**(NEEDS-CEF, risk)** Chromium has its own process singleton on its user-data dir (`root_cache_path`, `SingletonLock`).
It must not fight core's `<root>/LOCK` hand-over (inference).

### 1.3 Profile directory (verified: `lib.rs`, `ProfilePaths`)

```
~/.local/share/vsesvit/profiles/                 ProfilesDir (directories::ProjectDirs::from("", "", "Vsesvit"))
  profiles.json, profiles.lock                   the profile list (every profile's process reads/writes it under the lock)
  <ProfileId>/                                   ProfilePaths.root ("Default", "Profile 1", ...)
    LOCK                                         OS file lock held for the process lifetime
    vsesvit.db (+ -wal, -shm)                    everything (WAL, synchronous=NORMAL, foreign_keys, busy_timeout=0)
    extensions/<id>/<version>_<hash32>/          managed unpacked extensions, immutable (section 22.4)
    staging/<uuid>/root/                         install scratch, wiped at every open
    engine/                                      ProfilePaths.engine_data: WebKit NetworkSession data dir today
    Account Picture <sha256 prefix>.png          sync account picture (profiles/account.rs, 144 px PNG)
~/.cache/vsesvit/<ProfileId>/                    ProfilePaths.engine_cache (only for profiles in the default location; else <root>/cache)
```

**(NEEDS-CEF)** `engine_data` becomes CEF's `root_cache_path`/`CefRequestContextSettings.cache_path`. Chromium has no separate
cache-dir setting except the `--disk-cache-dir` switch (inference). Existing profiles hold WebKit data in `engine/`;
the rewrite must decide to wipe or move it (risk). WinUI documents `engine/` as the WebView2 user data folder.

### 1.4 Start sequence the GTK shell runs (`crates/vsesvit-gtk/src/browser.rs`, `Browser::new` + `Browser::start`)

1. `home.dir.opened(&home.id)` (profile list), wrap profile, create engine session from `paths.engine_data/engine_cache`.
2. `prefs().get(&keys::UPDATES_AUTOMATIC)`, `prefs().get(&keys::UPDATES_CHANNEL)` (self-updater).
3. `onboarding::should_show` (skipped in scripted runs).
4. Extension runtime + sync (`Syncer::new` reads `vsesvit_sync::Account::load`).
5. `permissions::seed_notifications` (WEBKIT-ONLY: hands stored notification Allow/Block to `WebContext`).
6. `start()`: apply theme (`keys::THEME`), keymap (`prefs().keymap()` + `Profile::extension_shortcuts()`), tracking
   protection blocker, cookie rules, `cookies` clear-at-start; `extensions().list()` and load every enabled one;
   `extensions().reconcile()`; schedule extension update checks (`extensions().next_update_check()`); preload favicons
   (`favicons().missing(32)`); Memory Saver sweep timer every `memory_saver::SWEEP_EVERY` (60 s).
7. `downloads().interrupt_stale()` (in the downloads controller's constructor).
8. `open_startup_windows(targets)`: by `keys::STARTUP` restore session / homepage / new tab, then URLs, then welcome.

Shutdown (`Browser::shutdown`): `save_session_now()`, then (unless the profile was removed) cookies clear-at-exit, then
the final sync (waits up to 3 s). Nothing writes the session afterwards.

### 1.5 Checks and self-test

`profile_open` (core opens a new profile), `profiles` (menu, titles, sync details, Manage Profiles, picker switch).

---

## 2. Preferences (`crates/vsesvit-core/src/prefs.rs`)

API: `Profile::prefs() -> Prefs`; `Prefs::get<T>(&Pref<T>) -> T` (default when missing/undecodable; a Local pref reads only
a value this device wrote), `Prefs::set<T>(&Pref<T>, &T) -> Result<(), Error>` (no stamp when unchanged; setting the
default value explicitly stores it so the choice syncs), `Prefs::reset<T>(&Pref<T>)` (stores `None` = default, syncs),
`Prefs::keymap() -> Keymap`, `Prefs::set_keymap(&Keymap)`. `Pref<T> { key, scope: Scope::{Synced, Local}, default: fn() -> T }`.
Unknown keys from newer builds are stored and re-synced untouched; sync may never write a Local key
(`prefs::remote_may_write`, `keys::LOCAL_KEYS`).

| Constant | Key | Type / default | Scope | GTK UI (Settings page > row wording) | Live effect | CEF mapping |
|---|---|---|---|---|---|---|
| `keys::HOMEPAGE` | `homepage` | String, `"about:home"` | Synced | General > Homepage (entry; empty = new tab; `homepage_input(text)` validates, "Not a web address" toast) | Home button, startup | KEEP |
| `keys::DEFAULT_SEARCH_ENGINE` | `search.default` | `SearchEngineId`, `builtin:ddg` | Synced | Search > default engine | omnibox | KEEP |
| `keys::THEME` | `theme` | `Theme::{System,Light,Dark}`, System | Synced | Appearance > Theme ("Follow the system", "Light", "Dark") | `adw::StyleManager` color scheme | KEEP; NEEDS-CEF: also tell pages (`CefRequestContext::SetChromeColorScheme` exists, inference that it drives `prefers-color-scheme`) |
| `keys::STARTUP` | `startup` | `Startup::{RestoreSession,Homepage,NewTab}`, RestoreSession | Synced | General > On Startup ("Restore the previous session", "Open the homepage", "Open a new tab") | next start | KEEP |
| `keys::TABS_POSITION` | `tabs.position` | `TabsPosition::{Left,Right,Top}`, Left | Synced | Appearance > Tab Position ("Left sidebar", "Right sidebar", "Top bar"; "Vertical tabs in a sidebar, or a bar above the page") | re-lays every window | KEEP |
| `keys::SHOW_BOOKMARKS_BAR` | `bookmarks_bar.visible` | bool, true | Synced | Appearance > Show Bookmarks Bar; Ctrl+Shift+B | every window | KEEP |
| `keys::COMPACT_ADDRESS_BAR` | `address_bar.compact` | bool, true | Synced | Search > Address Bar > Compact Address Bar | narrow centered address bar | KEEP |
| `keys::SHOW_FULL_URLS` | `address_bar.full_urls` | bool, false | Synced | Search > Always Show Full URLs ("Otherwise https:// and www. show only while editing the address") | address display (`address::simplified_url`) | KEEP |
| `keys::SHOW_HOME_BUTTON` | `toolbar.home_button` | bool, false | Synced | Appearance > Show Home Button ("Next to Reload, opens the homepage") | toolbar | KEEP |
| `keys::SHOW_MEDIA_PLAYER` | `media.player` | bool, true | Synced | (WinUI: media player at foot of vertical tabs) | WinUI only today | KEEP (product, Windows-only today) |
| `keys::PICTURE_IN_PICTURE` | `media.picture_in_picture` | bool, true | Synced | WinUI | WinUI only | KEEP |
| `keys::SUGGEST_HISTORY` | `address_bar.suggest.history` | bool, true | Synced | Search > Suggestions > Browsing History | `Omnibox::suggest` | KEEP |
| `keys::SUGGEST_BOOKMARKS` | `address_bar.suggest.bookmarks` | bool, true | Synced | Search > Suggestions > Bookmarks | `Omnibox::suggest` | KEEP |
| `keys::SEARCH_SUGGESTIONS` | `address_bar.suggest.search` | bool, true | Synced | Search > Search Suggestions ("Sends what you type in the address bar to your search engine") | `Omnibox::suggest_request` | KEEP |
| `keys::BLOCK_POPUPS` | `content.block_popups` | bool, true | Synced | Privacy > Pop-ups > Block Pop-ups ("Sites can still open windows when you click") | WebKit `javascript_can_open_windows_automatically` | NEEDS-CEF: `CefLifeSpanHandler::OnBeforePopup(user_gesture)` or content setting `CEF_CONTENT_SETTING_TYPE_POPUPS` |
| `keys::AUTOFILL_FORMS` | `autofill.forms` | bool, true | Synced | no GTK row ("WebKitGTK fills no forms"); WinUI Privacy switch | WebView2 form autofill | NEEDS-CEF: Chromium pref `autofill.profile_enabled` via `CefRequestContext::SetPreference` (inference) |
| `keys::TRACKING_PROTECTION` | `privacy.tracking_protection` | `trackers::TrackingProtection::{Off,Standard,Strict}`, Standard | Synced | Privacy > Trackers > Tracking Protection (labels + descriptions from core) | blocker rebuilt | section 9 |
| `keys::HTTPS_ONLY` | `privacy.https_only` | bool, false | Synced | Privacy > Security > Always Use Secure Connections (`https_only::TITLE`, `DESCRIPTION`) | section 11 | section 11 |
| `keys::THIRD_PARTY_COOKIES` | `privacy.third_party_cookies` | `cookies::ThirdPartyCookies::{Allow,BlockInPrivate,Block}`, BlockInPrivate | Synced | Privacy > Cookies (radio rows with `label()`/`description()`) | section 10 | section 10 |
| `keys::SPELLCHECK` | `spellcheck.enabled` | bool, true | Synced | General > Spell Check > Check Spelling ("Underline misspelled words as you type on web pages") | section 14 | section 14 |
| `keys::SPELLCHECK_LANGUAGES` | `spellcheck.languages` | `Option<Vec<String>>`, None (= system's) | Synced | General > Spell Check > Languages (expander of switches) | section 14 | section 14 |
| `keys::SMOOTH_SCROLLING` | `scrolling.smooth` | bool, true | Synced | General > System > Smooth Scrolling | WebKit setting; WinUI `--disable-smooth-scrolling` at engine start | NEEDS-CEF: Chromium switch `--disable-smooth-scrolling` (startup only, as WinUI) |
| `keys::HARDWARE_ACCELERATION` | `system.hardware_acceleration` | bool, true | **Local** | General > System > Hardware Acceleration ("Use the graphics card to draw pages when available") | WebKit policy; WinUI `--disable-gpu` | NEEDS-CEF: `--disable-gpu` (startup only) |
| `keys::MEMORY_SAVER` | `performance.memory_saver` | bool, true | **Local** | General > Memory > `memory_saver::TITLE` ("Memory Saver"), `DESCRIPTION` | section 13 | section 13 |
| `keys::MEMORY_SAVER_MODE` | `performance.memory_saver.mode` | `MemorySaverMode::{Moderate,Balanced,Maximum}`, Balanced | **Local** | General > Memory > Memory Savings (labels/descriptions from core) | section 13 | KEEP |
| `keys::DEVICE_NAME` | `device.name` | String, "" (shell falls back to host name) | **Local** | none in GTK | session record `device_name` | KEEP |
| `keys::UPDATES_AUTOMATIC` | `updates.automatic` | bool, true | **Local** | General > Updates > Automatic Updates ("Download new versions of Vsesvit in the background") | self-updater on/off | KEEP |
| `keys::UPDATES_CHANNEL` | `updates.channel` | `UpdateChannel::{Stable,Beta,Weekly,Nightly}`, `UpdateChannel::of_build()` | **Local** | General > Updates > Update Channel ("Vsesvit moves to a steadier channel once that channel has a version newer than this one.") | immediate check | KEEP |
| `keys::DOWNLOADS_DIR` | `downloads.directory` | `Option<PathBuf>`, None (= XDG Downloads) | **Local** | General > Downloads > Download Folder ("Use Default", "Change…") | new downloads | KEEP |
| `keys::DOWNLOADS_ASK` | `downloads.ask` | bool, false | Synced | General > Downloads > Ask Where to Save Each File | save dialog per download | KEEP; NEEDS-CEF: `CefBeforeDownloadCallback::Continue(path, show_dialog)` |
| `keys::ONBOARDING_DONE` | `onboarding.done` | bool, false | **Local** | (welcome) | `onboarding::finish` | KEEP |
| `keys::SHORTCUTS` | `keyboard.shortcuts` | `shortcuts::Overrides` (map id -> chord strings), empty | Synced | Shortcuts page | keymap | section 16 |
| `keys::SYNC_SERVER` | `sync.server` | String, `prefs::DEFAULT_SYNC_SERVER` = `https://vsesvit-service.mrquantumoff.dev` | **Local** | Sync > Sync Server (entry; empty resets; `vsesvit_sync::server_input`) | sign-in target | KEEP |
| `keys::SYNC_TYPES` (`prefs::SYNC_TYPES_KEY` = `sync.types`) | `sync.types` | `Vec<sync::DataType>`, all | **Local** | Sync > Customize Sync ("Sync Everything" + one switch per `DataType::label()`) | `Round::gather` | KEEP |
| `keys::SYNC_PASSPHRASE_OFFERED` | `sync.passphrase_offered` | bool, false | **Local** | (prompt) | `vsesvit_sync::status::prompt` | KEEP |
| `extensions::toolbar::TOOLBAR` | `toolbar.extensions` | `Vec<toolbar::Entry{id, pinned}>`, empty | Synced | toolbar pins/order | section 22.12 | KEEP |
| `extensions::notifications::NOTIFICATIONS_OFF` | `extensions.notifications_off` | `BTreeSet<ExtensionId>`, empty | Synced | Extensions dialog > row > Notifications | section 22.13 | NEEDS-CEF (risk) |
| `extensions::private::ALLOWED_IN_PRIVATE` | `extensions.allowed_in_private` | `BTreeSet<ExtensionId>`, empty | Synced | Extensions dialog > row > "Allow in private windows" | section 22.14 | NEEDS-CEF (risk) |
| `extensions::permissions::GRANTED_PERMISSIONS` | `extensions.granted_permissions` | `BTreeMap<ExtensionId, PermissionSet>` | **Local** | none (granted by `permissions.request` and Re-enable) | section 22.7 | NEEDS-CEF (risk) |

WinUI-only shell prefs (not in core): `tabs.pane_collapsed`, `tabs.pane_width`, `window.backdrop`, `passwords.purged`
(`crates/vsesvit-winui/src/browser.rs`). The GTK shell declares no prefs of its own.

Helpers: `prefs::homepage_input(text) -> Option<HomepageValue::{Default, Url(String)}>`;
`prefs::homepage_url(pref) -> Option<Url>` (`None` = new tab for `""`, `about:home`, `about:blank`, non-addresses; the home
page never searches). `UpdateChannel::{ALL, name(), of_version(&str), of_build()}`.

GTK pref plumbing: `Browser::pref`, `set_pref`, `reset_pref`, `switch`, plus setters that also apply
(`set_tabs_position`, `set_bookmarks_bar_visible`, `set_home_button_visible`, `set_compact_address_bar`, `set_full_urls`,
`set_engine_switch`, `set_spellcheck_language`, `set_tracking_protection`, `set_third_party_cookies`, `set_theme`,
`set_updates_automatic`, `set_updates_channel`, `edit_keymap`). After sync: `Browser::sync_applied(&Changed)` re-applies
theme, engine prefs, window prefs, toolbar, keymap (if `keyboard.shortcuts` changed), trackers and cookie rules (if their
keys or site permissions changed), and runtime private-allowance (if `extensions.allowed_in_private` changed).

Settings rows that are shell-only wording, not prefs: "Secure DNS" ("Your system's network settings control DNS, and
whether it is secure"; **(NEEDS-CEF)** Chromium has its own DoH settings: `dns_over_https.mode`/`templates` prefs, inference),
"Vsesvit Doesn't Save Passwords" ("Use a password manager such as Bitwarden or Proton Pass through its browser extension";
**(NEEDS-CEF)** turn Chromium's password manager off, pref `credentials_enable_service` = false, inference), "Profile Folder"
(shows `paths().root`), "Clear Browsing Data…" (section 5), "Site Permissions" subpage (section 8).

---

## 3. Session, tabs, windows

### 3.1 Session save and restore (`crates/vsesvit-core/src/session.rs`)

User-visible: on start with "Restore the previous session", every saved window comes back with its tabs, pinned state,
groups, selected tab, size and maximized state; the active window last and on top. A tab still loading its first page is
saved. Private windows are never saved. Quitting, or closing the last normal window while a private one stays, never
overwrites the last real session.

API:
- `Profile::session() -> Session`; `Session::save(&SessionSnapshot) -> Result<()>` (new stamp + seq only if the snapshot
  changed; replaces `tab_restore_state` for listed tabs); `Session::restore() -> Result<Option<SessionSnapshot>>`
  (restore blobs re-attached); `Session::other_devices() -> Result<Vec<DeviceTabs>>` (other devices' sessions updated in
  the last 14 days, newest first).
- `SessionSnapshot { device_name: String, windows: Vec<WindowSnapshot>, active_window: usize }`
- `WindowSnapshot { tabs: Vec<TabSnapshot>, active_tab: usize, bounds: Option<(i32, i32, u32, u32)>, maximized: bool }`
- `TabSnapshot { id: TabId(Uuid), url: Url, title: String, pinned: bool, last_active_ms: i64, group: Option<TabGroup>, restore_state: Option<Vec<u8>> }`
  (`restore_state` is `#[serde(skip)]`, stored locally in `tab_restore_state`, excluded from equality).
- `DeviceTabs { device: DeviceId, device_name, updated_ms, windows }`.

GTK (`crates/vsesvit-gtk/src/session.rs`): `snapshot(browser)` lists windows most-recently-focused first (active = index 0),
only normal tabs with a `session_uri`, `bounds = (0, 0, width, height)`, `device_name = keys::DEVICE_NAME` or host name;
`restore(browser, snapshot)` recreates windows in reverse so the active one is presented last, then
`window.restore_groups(groups)` after selecting. Save is debounced 2 s (`SESSION_SAVE_DELAY`) after any tab change
(open/close/move/activate/commit/title) and run immediately on shutdown and when the last normal window closes.

Classification:
- Session record, ordering, debounce, private exclusion: **(KEEP)**.
- `restore_state` = `webkit_web_view_get_session_state` bytes: **(WEBKIT-ONLY)**. **(NEEDS-CEF, gap)**: CEF exposes
  `CefBrowserHost::GetNavigationEntries` (read only) and no way to restore a back/forward list, so restored tabs reopen
  their URL only, as WinUI does (`restore_state: None`, `crates/vsesvit-winui/src/session.rs`). Same for reopen-closed-tab
  and tab moves between windows (GTK keeps history; WinUI does not).
- `bounds`: GTK writes origin (0,0) because Wayland does not report window positions.

### 3.2 Tab menu placement (`crates/vsesvit-core/src/tab_place.rs`) (KEEP)

`TabPlace { index, count, pinned }` with `is_pinned()`, `closes_others() -> Vec<usize>` (last first, never pinned),
`closes_after() -> Vec<usize>` (to the right, never pinned), `can_move_out()` (Move Tab to New Window only when other tabs
stay). Used by the tab context menu (`crates/vsesvit-gtk/src/window/tab_menu.rs`, `place_of`).

Tab menu items (both shells, Chrome's): New Tab to the Right / Below (sidebar), Add Tab to New Group / Add Tab to Group ▸
(New Group + the window's groups by `TabGroup::name()`), Remove from Group, Move Tab to New Window, Reload, Duplicate, Pin /
Unpin, Mute / Unmute, Copy Link (without tracking: `clean_url::clean`), Close Tab, Close Other Tabs, Close Tabs to the
Right / Below, Reopen Closed Tab. WinUI adds Split view. Reload from the menu records `Transition::Reload`.

### 3.3 Tab search (`crates/vsesvit-core/src/tab_search.rs`) (KEEP)

User-visible (PLAN.md "Tab search"): Ctrl+Shift+A or the Search Tabs button (top of the sidebar, or end of the top tab bar)
opens a popover: search entry over "Open tabs" (every window of the window's kind) and "Recently closed". Rows: icon, title
(or address), site. First row selected; Up/Down wrap; Enter/click selects an open tab and raises its window or reopens a
closed tab in the current window; Escape closes. No close buttons, no highlighting, no times.

API: `tab_search::rows<O, C>(query: &str, open: Vec<Listed<O>>, closed: Vec<Listed<C>>) -> Vec<Row<O, C>>`;
`Listed { key, title, url, used: u64 }`; `Row { hit: Hit::{Open(O), Closed(C)}, title, site }`. Empty query: all tabs,
most recently used first; otherwise title-prefix > title > address, MRU within a rank; open before closed.
`used` is a shell counter ticked when a tab opens, is selected, its window activates, or it closes (GTK `Browser::tick`).
The closed-tab stack (`crates/vsesvit-gtk/src/closed_tabs.rs`, 25 entries, separate stacks for normal and private) is
shell state.

### 3.4 Tab groups (`crates/vsesvit-core/src/tab_groups.rs`) (KEEP)

User-visible: Chrome's groups per window: header chip (title or "<Colour> group"), nine colours, collapse/expand, editor
popover (name, colours, New tab in group, Ungroup, Close group). Vertical list: header before the group's first row;
collapsed rows hidden and skipped by focus/arrow keys. Top bar: chips in the bar's start widget and a coloured dot per
grouped tab. Ctrl+Tab / Ctrl+Shift+Tab / Ctrl+PgDn / Ctrl+PgUp skip hidden tabs. Closing the selected tab picks the next as
core says. Reopen puts a tab back in its group. A tab opened from a grouped tab joins the group.

API: `TabGroups<K>` per window with `restore(tabs)`, `group_of`, `get`, `is_empty`, `in_order(&WindowTabs)`,
`members`, `new_group -> (GroupId, Vec<Step>)`, `join`, `leave`, `set_collapsed`, `next_shown(&WindowTabs, back)`,
`after_closing(&WindowTabs, closing, opener)`, `set_title`, `set_color`, `ungroup`, `adopt(tab, opener)`,
`rejoin(&WindowTabs, tab, group) -> Vec<Step>`, `settle(&WindowTabs, moved: Option<&K>)`, `rows(&WindowTabs) -> Vec<Row<K>>`.
`WindowTabs { order: Vec<K>, pinned: usize, active: Option<K> }`; `Step::{Unpin(K), Move(K, usize), Activate(K), OpenTab}`;
`Row::{Header(TabGroup), Tab { tab, group, hidden }}`; `TabGroup { id: GroupId, title, color: GroupColor, collapsed }`;
`GroupColor::{Grey, Blue, Red, Yellow, Green, Pink, Purple, Cyan, Orange}` with `label()`, `rgb(dark)`, `next(used)`.
The shell must call `settle` after every add/close/move/pin/select of a tab. Session carries `TabSnapshot.group`.
`tab_groups::extension_group_id(Option<GroupId>) -> i32` (-1 or a folded positive id) was for the Linux runtime's
`chrome.tabs` (**WEBKIT-ONLY** consumer; Chromium's own `chrome.tabs`/`chrome.tabGroups` would not know about shell
groups, see section 22.17).

### 3.5 Private windows (`crates/vsesvit-core/src/private.rs`) (KEEP; engine part NEEDS-CEF)

User-visible: Ctrl+Shift+N, main menu "New Private Window" (after New Window), link menu "Open Link in Private Window".
Dark header bar, private-browsing icon before the address bar, "(Private)" after the title. New tab page is
`new_tab::PRIVATE_PAGE`. No profile button. Separate recently-closed stack; tab search, Ctrl+Shift+T and switch-to-tab rows
see only tabs of the window's kind. Notifications blocked without asking. No tracking-protection switch and no cookie rule
in site info. When the last private window closes: its running downloads are cancelled, its closed tabs forgotten,
`Profile::end_private_session()` called, engine session dropped.

API: `private::Browsing::{Normal, Private}` (passed to many APIs), `Browsing::loggable(&dyn Display)` (hides private
addresses in logs), `Profile::end_private_session()`.
Core keeps for the private session: site choices (`site_permissions_in(Private)`), HTTPS-only exceptions, zoom
(`site_zoom(Private)`), downloads rows (negative `DownloadId`s, listed only in private windows).
Engine: GTK uses `webkit::NetworkSession::new_ephemeral()` (**WEBKIT-ONLY**). **(NEEDS-CEF)** an off-the-record
`CefRequestContext` (`CefRequestContext::CreateContext` with empty `cache_path`, inference about OTR semantics) per private
session, with its own download handler.

---

## 4. Navigation and the address bar

### 4.1 Omnibox (`crates/vsesvit-core/src/search.rs`, `suggest.rs`) (KEEP)

User-visible (`crates/vsesvit-gtk/src/address_bar.rs`): while editing, the first suggestion is highlighted and is what Enter
opens; inline completion selected after the caret (`git` -> `github.com`); Up/Down/Tab/Escape move the highlight and show
each row's `fill` text without re-querying; Escape gives up editing; Ctrl+Enter makes a bare word `https://www.<word>.com/`.
Rows: Search (icon search, subtitle "Search"), typed URL, suggested searches, bookmarks, history (with a "forget" action that
calls `history().delete_url`), then up to 4 "Switch to tab · <url>" rows (shell-only, same kind of window, not the current
tab). At rest the entry shows the simplified URL centered; security icon at start, in-use capture button, zoom level, star
at end.

API:
- `Profile::omnibox() -> Omnibox`.
- `Omnibox::resolve(text) -> Result<Option<NavTarget>>` on Enter; `None` -> GTK focuses the page.
- `Omnibox::suggest(text, limit, allow_inline) -> Result<Suggestions>` every keystroke (GTK limit 8);
  `Suggestions { items: Vec<Suggestion>, inline: Option<String> }`;
  `Suggestion { source: SuggestionSource::{Search, Typed, Bookmark, History, SuggestedSearch}, title, target: NavTarget, fill }`.
- `Omnibox::suggest_request(text, &Queries, private: bool) -> Result<Option<SuggestRequest>>` (sends nothing when
  suggestions off, private, no `suggest_url`, or text is not a plain default-engine search);
  `SuggestRequest::run(self) -> Option<SearchSuggestions>` (worker; waits `suggest::DEBOUNCE` = 100 ms, 5 s limit);
  `SearchSuggestions::is_current()`; `Suggestions::add_search_suggestions(&SearchSuggestions)`; `Queries::default()`,
  `Queries::cancel()` (when the user leaves the box).
- `search::classify(text, engines, default)`, `search::classify_url(text) -> Option<NavTarget>` (also used for CLI targets
  and homepage), `NavTarget::{Url(Url), Search { engine, url }}`, `NavTarget::url()`.
- `search::ctrl_enter_url(text) -> Option<Url>`.
- Navigable schemes: `http https file about data view-source vsesvit`.
Transition recorded: omnibox Search/Typed/History rows -> `Transition::Typed`; Bookmark rows -> `Transition::Bookmark`.

### 4.2 Address display (`crates/vsesvit-core/src/address.rs`) (KEEP; NEEDS-CEF for readable form)

- `address::simplified_url(shown) -> String`: strips `https://`, leading `www.`, lone trailing `/` (http stays).
- `address::readable_url(url) -> String`: decodes punycode (only safe single-script labels) and non-ASCII percent escapes.
  WinUI uses it on WebView2's encoded URLs; GTK used `webkit::functions::uri_for_display` instead (**WEBKIT-ONLY**).
  **(NEEDS-CEF)** Chromium reports encoded URLs (`CefDisplayHandler::OnAddressChange`), so the new shell needs
  `readable_url` as WinUI does. `about:blank` shows as empty.

### 4.3 Selection search and link menu items (KEEP)

- `Omnibox::for_selection(text) -> Result<Option<SelectionAction>>`, `search::selection_action(text, &SearchEngine)`;
  `SelectionAction { label, url }`: `Search <Engine> for “text…”` (50 chars) or `Go to example.com`; opens in a new tab next
  to the page. GTK: page menu item after Copy (`crates/vsesvit-gtk/src/page_menu.rs`).
- Link menu: Open Link in Private Window after Open Link in New Window; Copy Link Without Tracking after Copy Link Address
  (`clean_url::clean`).
- **(WEBKIT-ONLY)** GTK injects a selection-tracking script because WebKit's context menu has no selection text.
  **(NEEDS-CEF)** `CefContextMenuHandler::OnBeforeContextMenu` with `CefContextMenuParams::GetSelectionText`,
  `GetLinkUrl`, `GetMisspelledWord`, `GetDictionarySuggestions`, `GetFrameUrl`.

### 4.4 History recording on navigation (KEEP; NEEDS-CEF hooks)

GTK `Browser::navigation_committed`: on every committed main-frame navigation, unless it is an error page or a private tab,
`history().record_visit(&url, transition)` (transition from the tab's pending transition, default `Link`); then show the
site's zoom (section 12), update runtime, schedule session save. `Browser::title_changed`: `history().set_title(&url,
&title)` for normal tabs, not error pages, not empty titles (WebKit clears the title before reporting the next commit).
Recordable URLs: `history::is_recordable` = http/https/file only.
**(NEEDS-CEF)** commit: `CefLoadHandler::OnLoadStart`/`OnLoadEnd` for the main frame or `CefDisplayHandler::OnAddressChange`;
title: `CefDisplayHandler::OnTitleChange`; transition: `CefRequest::GetTransitionType` / `CefNavigationEntry::GetTransitionType`
(`TT_LINK`, `TT_TYPED`, `TT_AUTO_BOOKMARK`, `TT_RELOAD`, `TT_FORM_SUBMIT`, redirect flags) (inference about mapping).

### 4.5 New tab page (`crates/vsesvit-core/src/new_tab.rs`) (KEEP; NEEDS-CEF loading)

User-visible: a centered rounded search box "Search with <Engine> or enter address" and up to 8 tiles (two rows of four)
of the most visited sites (letter on a coloured circle, favicon from `<origin>/favicon.ico` over it). The tab still reads as
blank: empty address bar, "New tab" title, not in session or history. Private windows get `PRIVATE_PAGE` ("You're browsing
privately", two cards of what is and isn't kept), dark always.
API: `new_tab::page(&mut Profile, Browsing) -> Result<String>` (`history().top_sites(TILES)` + default engine),
`new_tab::html(&[TopSite], &SearchEngine)`, `new_tab::PRIVATE_PAGE`, `new_tab::TILES = 8`, `TopSite { url, label }`.
GTK loads it with `web_view.load_html(&html, None)` (base `about:blank`) (**WEBKIT-ONLY** mechanism).
**(NEEDS-CEF)** CEF has no load-string API; options: a `vsesvit:` scheme handler (`CefRequestContext::RegisterSchemeHandlerFactory`)
or intercepting `about:blank`/a data URL via `CefResourceRequestHandler::GetResourceHandler`, while keeping the address bar,
title, session and history treating it as blank (inference; design choice for the rewrite).

### 4.6 Homepage and Home button (KEEP)

`go_home`: `homepage_url(keys::HOMEPAGE)` loads with `Transition::Bookmark`, else the new tab page in the selected tab and
focus to the address bar.

### 4.7 View page source (`crates/vsesvit-core/src/view_source.rs`)

User-visible: Ctrl+U, main menu More Tools > View Page Source, page menu item: opens `view-source:<page>` in a new tab next
to the page, for http/https/file pages only.
API: `view_source::source_url(page) -> Option<String>` (KEEP), `view_source::viewed_url(source) -> Option<&str>` (KEEP),
`view_source::source_page(url, source) -> String` (numbered-lines HTML) (**WEBKIT-ONLY**: GTK serves the `view-source`
scheme itself from a tab or a hidden script-less view that loads with the tab's session, tracking protection, cookie rules
and HTTPS-only, `crates/vsesvit-gtk/src/view_source.rs`, plus the `vsesvit_webext::Gate` that refuses it to non-browser
loads). **(NEEDS-CEF)** Chromium renders `view-source:` natively; just load the URL (WinUI does the same with WebView2).

### 4.8 Error pages and interstitials (shell, KEEP wording)

GTK `crates/vsesvit-gtk/src/error_page.rs`: "Security Warning / This connection is not secure" (with certificate reasons),
"Problem Loading Page / Unable to load this page", "Page Crashed / This page crashed"; loaded under the failing URI with no
script. HTTPS-only's warning comes from core (section 11). Certificate errors fail the load (`TLSErrorsPolicy::Fail`).
**(NEEDS-CEF)** `CefRequestHandler::OnCertificateError`, `CefLoadHandler::OnLoadError`,
`CefRequestHandler::OnRenderProcessTerminated`; displaying shell HTML under the failing URL needs a resource-handler
intercept (inference).

### 4.9 Checks

`navigate`, `history_recorded`, `omnibox`, `address_completion`, `search_suggestions`, `selection_search`,
`new_tab_page`, `address_progress`, `page_commands`, `session`, `tabs`, `tab_menu`, `tab_search`, `tab_groups`,
`private_window`.

---

## 5. History (`crates/vsesvit-core/src/history.rs`) (KEEP)

User-visible: History window (`crates/vsesvit-gtk/src/dialogs/history.rs`, Ctrl+H): recent visits (newest first, 300) or a
search (100), open in a new tab, forget one URL; "Clear browsing history" by range ("Last hour", "Last 24 hours", "Last 7
days", "Last 4 weeks", "All time"), which writes a synced deletion directive; above the visits Chrome's "Tabs from other
devices" (per device name, its windows' tabs with stored favicons). Settings > Privacy > "Clear Browsing Data…" confirms
"Clear Browsing Data?" / "History on every synced device, and this device's cookies, site storage and cache." then
`history().delete_range(0, now)` and clears all engine website data, toast "Browsing data cleared".

API: `Profile::history() -> History`; `record_visit(&Url, Transition)`, `set_title(&Url, &str)`,
`search(text, limit) -> Vec<HistoryEntry>`, `top_sites(limit) -> Vec<TopSite>`,
`visits_between(from_ms, to_ms, limit) -> Vec<(HistoryEntry, Visit)>`, `delete_url(&Url)`, `delete_range(from_ms, to_ms)`;
`history::ClearRange::{ALL, label(), start(now_ms)}`; `Transition::{Link, Typed, Bookmark, Reload, Redirect, FormSubmit}`;
`HistoryEntry { url, title, last_visit_ms, visit_count, typed_count }`; `Session::other_devices()`.
**(NEEDS-CEF)** clearing engine data: Chromium has no single CEF call for all website data; options are
`CefCookieManager::DeleteCookies`, `CefRequestContext::ClearHttpCache`, DevTools `Storage.clearDataForOrigin` /
`Network.clearBrowserCache` via `CefBrowserHost::ExecuteDevToolsMethod` (WinUI uses DevTools) (inference).

---

## 6. Bookmarks (`crates/vsesvit-core/src/bookmarks.rs`, `export.rs`, `import.rs`, `favicons/`) (KEEP)

### 6.1 What the user sees

- **Star** in the address bar / Ctrl+D (`Browser::star_clicked`): if the page is bookmarked, opens the "Edit bookmark"
  bubble on it, else adds it at the end of the bookmarks bar (`BookmarkId::TOOLBAR`, `InsertAt::End`, tab title) and opens
  "Bookmark added". The bubble edits name, URL and folder (`crates/vsesvit-gtk/src/bookmark_editor.rs`); an unparsable URL
  disables Save and marks the field. Star state follows every committed navigation (`is_bookmarked`).
- **Bookmarks bar** (`crates/vsesvit-gtk/src/bookmarks_bar.rs`): toolbar folder items as buttons, folders open menus,
  "Other Bookmarks" at the end when it has anything, items that do not fit go into a "»" menu. Drag to reorder or into a
  folder (one `move_to`). Right-click/menu key context menus: on a link Open in New Tab / Open in New Window / Edit… /
  Copy Link / Delete; on a separator Delete; on a folder "Open All (n) in New Tabs" (the folder's own links only), Rename… /
  Delete ("Delete Folder?" confirm); every menu also has Add Page… / Add Folder… ("Add Folder", "New Folder") / Show
  Bookmarks Bar / Bookmark Manager. Folder menus open
  sideways on hover/click/Right, middle-click opens in a new tab, labels ellipsized, menus scroll. Favicons per link
  (generic page icon otherwise).
- **Bookmarks window** (Ctrl+Shift+O, `crates/vsesvit-gtk/src/dialogs/bookmarks.rs`): tree (expand in place), flat search
  (60), new folder, edit, move by menu or drag, delete; main menu "Import Bookmarks…" and "Export Bookmarks…". Import
  lists browsers found (`import::installed_browsers()`: Chrome, Edge, Brave, Vivaldi, Chromium incl. Flatpak/Snap paths;
  Firefox incl. Snap/Flatpak) and a file (HTML export or Chromium `Bookmarks` JSON); each import goes into a new folder
  "Imported from <Browser>" (or "Imported") at the end of the bar. Export opens a save dialog in Documents named
  `bookmarks_<m>_<d>_<yy>.html`.

### 6.2 API

- `Profile::bookmarks() -> Bookmarks`: `get(BookmarkId) -> Option<BookmarkNode>`, `children(folder) -> Vec<BookmarkNode>`,
  `is_bookmarked(&Url)`, `find_by_url(&Url)`, `search(text, limit)`, `add_url(parent, InsertAt, title, &Url) -> BookmarkId`,
  `add_folder(parent, InsertAt, title)`, `add_separator(parent, InsertAt)`, `rename(id, title)`, `set_url(id, &Url)`,
  `move_to(id, parent, InsertAt)` (rejects `WouldCycle`), `remove(id)` (whole subtree), `import(parent, Vec<ImportItem>)`,
  `import_folder(title, Vec<ImportItem>) -> usize`.
- `BookmarkId::{ROOT, TOOLBAR, OTHER, MOBILE}`, `is_root()`, `is_visible_root()`; `NodeKind::{Folder, Url, Separator}`;
  `BookmarkNode { id, kind, parent, index, title, url, added_ms }`; `InsertAt::{Start, End, Index(usize)}`;
  `bookmarks::MAX_DEPTH = 64` (views stop recursing); `BookmarkError::{NotFound, NotAFolder, IsRoot, WouldCycle, NotAUrl}`.
- `export::html(&Bookmarks) -> String`, `export::file_name(y, m, d)`, `export::default_folder()`.
- `import::Source::{File, Chromium, Firefox}`, `Source::read() -> Result<Vec<ImportItem>, ImportError>`,
  `import::Found { name, source }`, `Found::folder_title()`, `import::FILE_FOLDER_TITLE = "Imported"`,
  `import::parse_html`, `import::parse_chromium`.
- Favicons (LOCAL, bookmarked sites only): `Profile::favicons() -> Favicons`; `record(&Url, png: &[u8]) -> Result<bool>`,
  `wanted(&Url) -> bool`, `missing(limit) -> Vec<Url>`, `commit_fetched(Vec<Fetched>) -> Result<bool>`,
  `get(&Url) -> Result<Option<Vec<u8>>>`; `favicons::FaviconFetch::new(pages).run() -> Vec<Fetched>` on a worker;
  `favicons::allow_local_hosts()` (tests). Max 64 KiB, retries after 7 days / 1 hour (offline).
GTK refresh: `Browser::bookmarks_changed()` repaints every bar and star and re-runs favicon preload.

### 6.3 Engine touchpoints

- Tab favicon to PNG for `favicons().record`: GTK encodes `web_view.favicon()` texture (**WEBKIT-ONLY** source).
  **(NEEDS-CEF)** `CefDisplayHandler::OnFaviconURLChange` + `CefBrowserHost::DownloadImage` -> `CefImage::GetAsPNG` (inference).
- Private tabs never record favicons.

Checks: `bookmark`, `star_bubble`, `bookmark_export`, `bookmarks_bar_menus`, `favicon_preload`, `bookmarks_bar_icons` (Windows).

---

## 7. Downloads (`crates/vsesvit-core/src/downloads.rs`) (KEEP; NEEDS-CEF engine)

### 7.1 What the user sees

- A download goes straight into the download folder under a free name (`unique_destination`, never overwriting), or a
  "Save File" dialog when "Ask Where to Save Each File" is on (cancel cancels the download).
- Toasts: "Downloading “<name>”" with Show; "“<name>” downloaded" with Open. Header gets a downloads button once a download
  started this session in a window of that kind.
- Downloads window (Ctrl+J, `crates/vsesvit-gtk/src/dialogs/downloads.rs`): newest first, live status refreshed at least
  every second: `status_line` texts such as "1.3 MB/s - 3.2 MB of 10 MB, 5 secs left", "0 B/s - … , Paused",
  "Interrupted · …", "This type of file can harm your device", "Deleted", "<size> · <host>", "Failed", "Cancelled". Open,
  Show in folder, Keep/Discard for unconfirmed, remove entry (file stays), clear finished.
- Dangerous files (`is_dangerous`: Linux `.appimage .bash .csh .deb .desktop .flatpak .flatpakref .ksh .rpm .run .sh .snap .tcsh .zsh`,
  `.jar .jnlp`, and their MIME types) are written as `<name>.unconfirmed`, end `Unconfirmed`, and the source window shows
  Chrome's warning popover under the downloads button with Keep and Discard. Keep moves to the real name (numbered if taken).
- Linux today cannot pause/resume (WebKit); Windows can (`download_pause`).

### 7.2 API

`Profile::downloads() -> Downloads`: `start(url, &Path, total, now_ms, Browsing) -> Download`,
`update(DownloadId, State, received, total)`, `list(limit, Browsing) -> Vec<Download>` (GTK limit 500), `remove(id)`,
`keep(id) -> Option<PathBuf>`, `discard(id)`, `clear(Browsing)`, `interrupt_stale() -> usize` (once at startup).
`State::{InProgress, Paused, Interrupted, Unconfirmed, Completed, Failed, Cancelled}`, `is_live()`, `is_final()`;
`Download { id, url, path, started_ms, state, received, total }`; `downloads::listed_in(started, window)`;
`unique_destination(dir, suggested, exists)`, `unconfirmed_path(&Path)`, `is_dangerous(name, mime)`, `sanitize(suggested)`,
`describe_size(bytes)`, `zone_identifier(url)` (Windows Mark of the Web only); `Transfer::{new, update, restart, at}`,
`Progress { received, total, per_second }`, `status_line(&Download, Option<Progress>, exists)`.
Private downloads: rows live in memory (negative ids), listed only in private windows; cancelled when the session ends.

### 7.3 CEF

**(NEEDS-CEF)** `CefDownloadHandler::CanDownload`, `OnBeforeDownload` (`CefBeforeDownloadCallback::Continue(path, show_dialog)`:
choose `unique_destination` or the `.unconfirmed` path), `OnDownloadUpdated` (`CefDownloadItem::{IsInProgress, IsComplete,
IsCanceled, IsInterrupted, IsPaused, GetReceivedBytes, GetTotalBytes, GetFullPath, GetUrl, GetMimeType, GetSuggestedFileName}`;
`CefDownloadItemCallback::{Cancel, Pause, Resume}`). Pause/resume become possible on Linux (product: show them, as Windows does).
Also: main-frame responses the engine cannot show become downloads (GTK `decide_policy` Response); Chromium does this itself.

Checks: `download`, `download_safety`, `download_speed`, `download_pause` (Windows), `save_page` (Save Page As writes MHTML
through the downloads list; **(NEEDS-CEF)** DevTools `Page.captureSnapshot` or Chrome's save-page command, inference).

---

## 8. Site permissions, prompts, capture and site info (`crates/vsesvit-core/src/permissions.rs`, `certificate.rs`)

### 8.1 What the user sees

- **Prompt bubble** anchored to the site-info icon at the start of the address bar, one at a time for the selected tab:
  heading e.g. "Use your camera and microphone?", body "“meet.example.com” wants to use your camera and microphone.",
  buttons in core's order: "Allow while visiting the site" (only with a real origin and remembered permissions), "Allow this
  time", "Never allow" (real origin), "Not now".
- **In-use indicator** after the security icon: "Using your camera and microphone", "Sharing your screen", ...
  (`Capturing::description`), opens site info; rows have a Stop button.
- **Site info popover** (security icon, `crates/vsesvit-gtk/src/site_info.rs`): connection secure or not and host; Tracking
  protection switch (section 9); "Cookies and site data" choice (section 10); Permissions section rows (`site_rows`) with a
  choice box (Allowed this time / Ask / Allow / Block) and a reset; for HTTPS the certificate: issued to/by, validity, DNS
  names, SHA-256 fingerprint, chain to the root.
- **Settings > Privacy > Site Permissions** subpage: grouped by site (`by_site`), each setting a combo of
  `Permission::settings()` labels and a Remove button; empty state "No Site Permissions" / "Sites you allow or block show
  here." Picture-in-picture rows are hidden on Linux (`permissions::listed`).

### 8.2 API

- `Profile::site_permissions() -> SitePermissions` (stored, Normal) and `Profile::site_permissions_in(Browsing)`:
  `get(&Origin, Permission) -> Option<Setting>`, `set(&Origin, Permission, Option<Setting>)`, `for_site(&Origin)`,
  `all() -> Vec<SiteSetting>`, `by_site() -> Vec<SiteGroup>`, `reset_site(&Origin)` (keeps trackers and cookies),
  `decide(Option<&Origin>, &[Permission], &TabGrants) -> Decision::{Allow, Block, Ask(Vec<Permission>)}` (private: notifications
  blocked), `answer(Option<&Origin>, &[Permission], Answer, &mut TabGrants) -> Result<bool>` (stores Allow/Block or grants once).
- `permissions::prompt(Option<&Origin>, &[Permission]) -> Prompt { heading, body, answers: Vec<Answer> }`;
  `Answer::{AllowWhileVisiting, AllowThisTime, NeverAllow, Dismiss}` with `label()`.
- `TabGrants` (shell keeps one per tab): `allows`, `committed(&Url)` (call on every commit; grants end when leaving the
  origin), `granted()`, `revoke(p)`.
- `permissions::site_rows(storable, stored, granted, Capturing) -> Vec<SiteRow { permission, current: SiteChoice, choices, live }>`;
  `SiteChoice::{AllowedThisTime, Ask, Allow, Block}` with `label()`, `setting()`.
- `Capturing { camera, microphone, screen }` with `any()`, `uses(p)`, `description()`.
- `Permission::{Camera, Microphone, Location, Notifications, ScreenShare, ClipboardRead, Midi, PictureInPicture, Trackers, Http, Cookies}`
  with `key()` (wire names `camera`, ..., `screen_share`, `clipboard_read`, `midi`, `picture_in_picture`, `trackers`, `http`,
  `cookies`), `label()` ("Camera", ..., "Screen sharing", "Clipboard", "MIDI devices", "Picture-in-picture", "Trackers",
  "Insecure connections", "Cookies and site data"), `asks()`, `in_site_info()`, `remembers_allow()` (not ScreenShare),
  `settings()`. `Setting::{Allow, Block, ClearOnExit}` with `label()` ("Allow", "Block", "Clear on exit").
- `Origin::{of(&Url), parse(&str), as_str(), host_for_display()}` (opaque origins have none).
- `certificate::parse_der(&[u8]) -> Result<Certificate>`; `Certificate { subject, issuer, not_before_unix, not_after_unix, dns_names, serial_hex, public_key, signature_algorithm }`; `Name::display()`.
- `Error::AlwaysAsks(Permission)` (Allow for screen share refused).

### 8.3 Engine touchpoints

- **(WEBKIT-ONLY)** GTK maps `webkit::PermissionRequest` kinds (geolocation, notification, clipboard, user media with
  `display` flag) and takes the origin from the tab's committed URI because WebKit gives none; answers
  `navigator.permissions.query` (`webkit::PermissionStateQuery`) from `decide`; seeds notification permissions into the
  `WebContext`; stops capture with `set_{camera,microphone,display}_capture_state`.
- **(NEEDS-CEF)** `CefPermissionHandler::OnRequestMediaAccessPermission(browser, frame, requesting_origin, requested_permissions, callback)`
  (`CEF_MEDIA_PERMISSION_DEVICE_AUDIO_CAPTURE`/`VIDEO_CAPTURE`/`DESKTOP_*`), `OnShowPermissionPrompt(browser, prompt_id, requesting_origin,
  requested_permissions, callback)` with `CEF_PERMISSION_TYPE_GEOLOCATION`, `NOTIFICATIONS`, `CLIPBOARD`, `MIC_STREAM`, `CAMERA_STREAM`,
  `MIDI_SYSEX`, ... and `OnDismissPermissionPrompt`; capture state `CefDisplayHandler::OnMediaAccessChange(has_video, has_audio)`
  (no separate screen flag, inference). Chromium answers `navigator.permissions.query` from its own content settings, so the shell
  must mirror core's stored settings into `CefRequestContext::SetContentSetting(..., CEF_CONTENT_SETTING_TYPE_GEOLOCATION /
  NOTIFICATIONS / MEDIASTREAM_CAMERA / MEDIASTREAM_MIC / CLIPBOARD_READ_WRITE / MIDI_SYSEX, ALLOW/BLOCK/ASK)` (inference).
  Stopping a live capture has no CEF API (open question).
- **(NEEDS-CEF)** certificate DER: `CefNavigationEntry::GetSSLStatus()` -> `CefSSLStatus::GetX509Certificate()` ->
  `CefX509Certificate::GetDEREncoded()` / `GetDEREncodedIssuerChain()` (WinUI uses DevTools `Network.getCertificate`).

Checks: `site_permissions`, `capture_in_use`, `connection_info`.

---

## 9. Tracking protection (`crates/vsesvit-core/src/trackers.rs`, `trackers.json`)

User-visible: Settings > Privacy > Tracking Protection (Off "Sites can load trackers"; Standard "Blocks known advertising
and analytics trackers on other sites"; Strict "Also blocks social media trackers, such as Like and Share buttons. Some sites
may not work"). Site info switch `trackers::TITLE` = "Tracking protection" with `site_status(on, blocked)`: "Off for this
site", "Blocking known trackers on this site" (Linux, no count), or "N trackers blocked on this page" (Windows). Not offered
in private windows.

API: `TrackerList::bundled()`, `TrackerList::parse(json)`, `with_tracker(domain, Category)` (self-test adds `localhost`),
`domains()`, `blocks(level, page_host, request_host) -> Option<&str>`, `dnr_rules(level, allowed: &[Origin]) -> String`;
`trackers::level_for(&mut Profile, Option<&Origin>)`, `allowed(&mut Profile, &Origin)`, `set_allowed(&mut Profile, &Origin, bool)`
(stores `Permission::Trackers` Allow), `allowed_sites(&mut Profile) -> Vec<Origin>`, `site_status(on, Option<usize>)`;
`TrackingProtection::{ALL, label(), description()}`; `Category::{Advertising, Analytics, Social}` (Standard blocks all but Social).

Classification: list, levels, per-site exceptions, wording **(KEEP)**. `dnr_rules` -> WebKit content blocker via
`vsesvit_webext::dnr` (**WEBKIT-ONLY**). **(NEEDS-CEF)** per-request blocking as WinUI does:
`CefResourceRequestHandler::OnBeforeResourceLoad` returns `RV_CANCEL` when `TrackerList::blocks(level_for(top origin), page_host, request_host)`
matches (IO thread: use a snapshot); this also gives Linux a blocked count for `site_status(on, Some(n))`. Note WebKit judged
frames by the frame's own site, Windows by the tab's page; CEF can do either (tab's page matches Windows).

Check: `tracking_protection`.

---

## 10. Cookie controls (`crates/vsesvit-core/src/cookies.rs`)

User-visible: Settings > Privacy > Cookies radio rows: "Allow third-party cookies", "Block third-party cookies in private
windows" (default), "Block third-party cookies", with descriptions. Site info section "Cookies and site data" with choices
Default / Allow / Block / Clear on exit and a status line (`site_status`): "This site can't use cookies", "This site's
cookies and data are deleted when you close Vsesvit", "This site and sites it embeds can use cookies", "Third-party cookies
are blocked on this site", "This site can use cookies. Third-party cookies are blocked on it". Settings > Privacy > Cookies >
"See All Site Data" ("Cookies and other data sites keep on this device") opens the "Site Data" subpage listing every site's
stored data with what it holds, removed per site or "Remove All…"; empty state "No Site Data" / "Sites that keep cookies or
other data on this device show here." (`crates/vsesvit-gtk/src/dialogs/site_data.rs`; WebKit website-data records today,
**(NEEDS-CEF)** cookies via `CefCookieManager::VisitAllCookies` + `cookies::group_by_site` as WinUI does, other storage via DevTools).
Block and Clear-on-exit sites' data are deleted at exit and again at start.

API: `cookies::third_party_blocked(&mut Profile, Browsing, top: Option<&Origin>) -> bool` (`None` = all sites at once, as
WebKit needs), `setting(&mut Profile, &Origin)`, `set(&mut Profile, &Origin, Option<Setting>)`,
`site_choices(current, exceptions: bool) -> Vec<Option<Setting>>` (Linux passes `false`: Allow offered only if synced in),
`choice_label`, `site_status(third_party_blocked, setting)`, `site_rules(&mut Profile) -> SiteRules` with
`blocked_hosts()`, `blocks_cookie(domain)`, `to_clear()`, `clears_cookie(domain)`, `clears(record_name)`;
`block_script(&[String]) -> Option<String>` (empties `document.cookie`, removes `cookieStore`); `group_by_site(domains) -> Vec<SiteCookies>`
(Windows: cookies listed one by one); `cookies::SITE_TITLE`; `ThirdPartyCookies::{ALL, label, description, blocks(Browsing)}`.

Classification: model and wording **(KEEP)**. WebKit's single accept policy, the content-blocker rule per blocked host, and
`clears(name)` on WebKit website-data records are **(WEBKIT-ONLY)**. **(NEEDS-CEF)** Chromium implements exactly Chrome's
model: pref `profile.cookie_controls_mode` (0 off, 1 block 3p, 2 incognito only; inference about pref name/values) via
`CefRequestContext::SetPreference`; per-site rules via `CefRequestContext::SetContentSetting(url, top_url,
CEF_CONTENT_SETTING_TYPE_COOKIES, CEF_CONTENT_SETTING_VALUE_ALLOW | BLOCK | SESSION_ONLY)` (verified enum names; SESSION_ONLY
= Clear on exit). That makes per-site Allow exceptions possible on Linux (`site_choices(_, true)`). Listing and deleting:
`CefCookieManager::VisitAllCookies`, `DeleteCookies`; other storage via DevTools `Storage.clearDataForOrigin`
(WinUI approach). Or `CefCookieAccessFilter::{CanSendCookie, CanSaveCookie}` per request (IO thread).

Check: `cookies`.

---

## 11. HTTPS-only (`crates/vsesvit-core/src/https_only.rs`)

User-visible: with "Always Use Secure Connections" on, a main-frame http navigation to a public host on port 80 loads https
instead; if that fails or redirects back to http on an upgraded host, the tab shows the warning page at the http address:
`<title>Connection is not secure</title>`, "This site doesn't support a secure connection", body naming the host, "Continue
to site" button, which stores `Permission::Http` = Allow for the site (synced, listed in Settings' site permissions, kept
until removed; in private windows kept in memory).

API: `https_only::upgraded(&Url, Reach) -> Option<Url>`, `upgrade(&mut Profile, Browsing, &Url, Reach) -> Option<Url>`,
`allow(&mut Profile, Browsing, &Url)`, `allowed(&mut Profile, Browsing, &Origin)`, `warning_page(&Url) -> String`,
`Reach::{Public (default), Everywhere (self-test)}`, `Cause::{Link, Redirect, Other}`,
`Next::{Load, Upgrade(Url), Warn(Url), Allow(Url)}`, per-tab `Upgrades { starting(&Url, Cause, Option<Url>) -> Next,
finished(ok) -> Option<Url>, committed(), leave(), warning() -> Option<&Url> }`, `TITLE`, `DESCRIPTION`, `WARNING_TITLE`.

Classification: policy, state machine, warning page **(KEEP)**. GTK's heuristics for "surely the main frame" (WebKit's
`decide-policy` does not say which frame; `holds_no_http_frame`, judging late in `load_changed`) are **(WEBKIT-ONLY)**.
**(NEEDS-CEF)** `CefRequestHandler::OnBeforeBrowse(browser, frame, request, user_gesture, is_redirect)` with
`frame->IsMain()` (UI thread): feed `Upgrades::starting`; on `Next::Upgrade` cancel and `frame->LoadURL(https)`;
redirects via `is_redirect`; failure via `CefLoadHandler::OnLoadError` -> `Upgrades::finished(false)`; commit via load
handler -> `committed()`; typed/reload/back/forward -> `leave()`. The warning page must load under the http URL (WinUI uses
`NavigateToString`; CEF needs a resource-handler intercept, inference). Chromium's own HTTPS-First mode must stay off so it
does not double-handle (inference).

Check: `https_only`.

---

## 12. Zoom per site (`crates/vsesvit-core/src/zoom.rs`) (KEEP)

User-visible: Ctrl+Plus/Equal/KeypadPlus, Ctrl+Minus/KeypadMinus, Ctrl+0/Keypad0; main menu zoom row (Zoom Out, level,
Zoom In, Reset); when not 100% the level shows in the address bar before the star and opens Chrome's zoom bubble. One level
per host (any scheme/port), remembered across pages and restarts; private windows keep theirs in memory.
API: `Profile::site_zoom(Browsing) -> SiteZoom`; `get(&Url) -> Result<f64>`, `set(&Url, f64)` (default forgets);
`zoom::DEFAULT = 1.0`. Steps are shell-side (`crates/vsesvit-gtk/src/zoom.rs`): 0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0,
1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0. GTK re-applies the site level on every new-document commit because WebKit
keeps a view's zoom across pages (**WEBKIT-ONLY** detail).
**(NEEDS-CEF)** `CefBrowserHost::SetZoomLevel/GetZoomLevel` use Chromium's log scale (level = ln(factor)/ln(1.2), inference),
so convert; Chromium also keeps its own per-host zoom map in its prefs, which must not override core's (inference).
Checks: `zoom_indicator`, `zoom_is_remembered_per_site`.

---

## 13. Memory Saver (`crates/vsesvit-core/src/memory_saver.rs`) (KEEP; NEEDS-CEF sleep)

User-visible: a background tab left alone for 6/4/2 hours (Moderate/Balanced/Maximum) sleeps; its icon shows faded in tab
lists; selecting it wakes it. Kept awake: shown, pinned, audible or media player's tab, capturing, a site allowed to notify,
non-web pages, `window.opener` relations, unsaved form input, and (Linux) any edit since load.
API: `memory_saver::Sweep::new(&mut Profile, Instant)`, `Sweep::sleeps(&TabActivity, &mut IdleClock) -> bool`,
`IdleClock::{new, restart}`, `TabActivity { url, shown, pinned, audible, capturing, related }`,
`UNSAVED_INPUT_SCRIPT` (run in the page before sleeping), `has_unsaved_input(result: &str)`, `SWEEP_EVERY = 60 s`,
`TITLE`, `DESCRIPTION`, `MODE_TITLE`, `MemorySaverMode::{ALL, delay(), label(), description()}`.
GTK sleeps with `webkit_web_view_terminate_web_process` and re-runs the sweep judgement after the script answers
(**WEBKIT-ONLY** mechanism; Linux kept the title because WebKit forgets it). WinUI uses `TrySuspendAsync`.
**(NEEDS-CEF, open)** no CEF discard API. Options (inference): DevTools `Page.setWebLifecycleState {state: "frozen"}` via
`ExecuteDevToolsMethod` (keeps memory), or close the `CefBrowser` and recreate on wake (loses back/forward, since CEF cannot
restore history). `TabActivity.audible` needs a "playing audio" signal; implementing `CefAudioHandler` diverts audio output
(inference), so another source is needed (open question).
Check: `memory_saver`.

---

## 14. Spell check (`crates/vsesvit-core/src/spellcheck.rs`)

User-visible: on by default; context menu on a misspelled word lists corrections and "Add to Dictionary". Settings > General >
Spell Check: Check Spelling switch and Languages expander listing installed dictionaries by `display_name` ("English (United
States)"); none installed: "No dictionaries are installed. Install your distribution's Hunspell dictionaries for your
languages." Until the user picks, the system's languages are used.
API: `spellcheck::Dictionaries::new(installed, locales)`, `installed()`, `checked(chosen: Option<&[String]>) -> Vec<String>`,
`choose(chosen, language, on) -> Vec<String>`, `installed_in(dirs)` (Hunspell `.dic`+`.aff` in `~/.config/enchant/hunspell`,
`/usr/share/hunspell`, `/usr/share/myspell`), `system_locales()`, `display_name(language)`.
Classification: synced prefs and Chrome-like defaults **(KEEP)**; Enchant/Hunspell discovery and WebKit's
`set_spell_checking_languages` **(WEBKIT-ONLY)**. **(NEEDS-CEF)** Chromium's spell checker uses `.bdic` dictionaries named
`en-US`, prefs `browser.enable_spellchecking` / `spellcheck.dictionaries` via `CefRequestContext::SetPreference`,
`CefBrowserHost::ReplaceMisspelling` / `AddWordToDictionary` (Chromium's custom dictionary file, not the system's), and
`CefContextMenuParams::GetDictionarySuggestions` (inference about prefs and dictionary download). Language-name mapping
`en_US` <-> `en-US` needed. Check: `spellcheck`.

---

## 15. Search engines (`crates/vsesvit-core/src/search.rs`) (KEEP)

User-visible: Settings > Search: default engine row; "Manage Search Engines" subpage listing name, shortcut, URL; row menu
Make Default / Edit… / Remove (no Remove on the default, labelled "(Default)"); editor `AdwAlertDialog` with three entries
(name, shortcut, URL with `%s`), Save disabled while a field is blank, field errors from core: "Enter a name", "Enter a
shortcut", "A shortcut is one word, without spaces", "Another search engine has this shortcut", "Enter a URL", "Put %s where
the search terms go", "Not a web address". Built-ins: DuckDuckGo `d` (default), Google `g`, Bing `b`, Wikipedia `w`
(Wikipedia has no suggest URL). `fx rust` searches the engine with shortcut `fx` immediately.
API: `Profile::search_engines() -> SearchEngines`: `list()`, `default_engine()`, `set_default(&SearchEngineId)`,
`check(editing: Option<&SearchEngineId>, &EngineForm) -> Result<Vec<FormError>>`, `save(editing, &EngineForm) -> SearchEngineId`,
`remove(&SearchEngineId)` (`Error::RemoveDefaultEngine`), `set_suggest_url` (testkit); `EngineForm { name, keyword, url }`,
`EngineForm::of(&SearchEngine)`, `FormError::{field(), is_blank()}`, `FormField`; `SearchEngine { id, name, keyword,
search_url: UrlTemplate, suggest_url, builtin }`; `UrlTemplate::expand(terms)`.
Check: `search_engines`.

---

## 16. Keyboard shortcuts (`crates/vsesvit-core/src/shortcuts.rs`) (KEEP)

Command table (id, title, defaults), shown in Settings > Shortcuts by `Section::title()` ("Tabs and Windows", "Navigation",
"Page", "General"):

| Section | Command (`id`) | Title | Default chords |
|---|---|---|---|
| Tabs and Windows | NewTab `new-tab` | New tab | Ctrl+T |
| | CloseTab `close-tab` | Close tab | Ctrl+W, Ctrl+F4 |
| | ReopenClosedTab `reopen-closed-tab` | Reopen closed tab | Ctrl+Shift+T |
| | SearchTabs `search-tabs` | Search tabs | Ctrl+Shift+A |
| | NextTab `next-tab` | Next tab | Ctrl+Tab |
| | PreviousTab `previous-tab` | Previous tab | Ctrl+Shift+Tab |
| | SelectTab1..8 `select-tab-1..8` | Go to tab N | Ctrl+1..Ctrl+8 |
| | SelectLastTab `select-last-tab` | Go to the last tab | Ctrl+9 |
| | ToggleTabList `toggle-tab-list` | Show or hide the tab list | Ctrl+S, F9 |
| | NewWindow `new-window` | New window | Ctrl+N |
| | NewPrivateWindow `new-private-window` | New private window | Ctrl+Shift+N |
| | Quit `quit` | Quit | Ctrl+Q |
| Navigation | FocusAddress `focus-address` | Focus the address bar | Ctrl+L, Alt+D, F6 |
| | Back `back` | Back | Alt+Left |
| | Forward `forward` | Forward | Alt+Right |
| | Reload `reload` | Reload | Ctrl+R, F5 |
| | ReloadBypassCache `reload-bypass-cache` | Reload, ignoring the cache | Ctrl+Shift+R, Shift+F5 |
| Page | SavePage `save-page` | Save page as… | Ctrl+Shift+S |
| | Print `print` | Print… | Ctrl+P |
| | Find `find` | Find | Ctrl+F |
| | FindNext `find-next` | Next match | Ctrl+G |
| | FindPrevious `find-previous` | Previous match | Ctrl+Shift+G |
| | BookmarkPage `bookmark-page` | Bookmark this page | Ctrl+D |
| | CopyCleanLink `copy-clean-link` | Copy link without tracking | Ctrl+Shift+C |
| | CopyLink `copy-link` | Copy link | Ctrl+Alt+Shift+C |
| | ViewSource `view-source` | View page source | Ctrl+U |
| | ZoomIn `zoom-in` | Zoom in | Ctrl+Plus, Ctrl+Equal, Ctrl+KeypadPlus |
| | ZoomOut `zoom-out` | Zoom out | Ctrl+Minus, Ctrl+KeypadMinus |
| | ZoomReset `zoom-reset` | Reset zoom | Ctrl+0, Ctrl+Keypad0 |
| | Fullscreen `fullscreen` | Fullscreen | F11 |
| General | ToggleBookmarksBar `toggle-bookmarks-bar` | Show or hide the bookmarks bar | Ctrl+Shift+B |
| | ShowBookmarks `show-bookmarks` | Bookmarks | Ctrl+Shift+O |
| | ShowHistory `show-history` | History | Ctrl+H |
| | ShowDownloads `show-downloads` | Downloads | Ctrl+J |
| | ShowSettings `show-settings` | Settings | Ctrl+Comma |
| | ShowShortcuts `show-shortcuts` | Keyboard shortcuts | Ctrl+Question |
| | DeveloperTools `developer-tools` | Developer tools | Ctrl+Shift+I, F12 |
| | JavaScriptConsole `javascript-console` | JavaScript console | Ctrl+Shift+J |

GTK binds all but SelectTab1..8/SelectLastTab (not offered) and treats NextTab/PreviousTab as fixed built-ins
(`crates/vsesvit-gtk/src/keymap.rs`, `binding`). Shortcut capture dialog: Escape cancels, Enter confirms, Backspace clears
(those keys are not in `Key`); notes from core: `NEEDS_MODIFIER_NOTE` ("A shortcut needs Ctrl or Alt, unless it is a function
key"), `NOT_A_KEY_NOTE`, `Keymap::offer` ("Used by X, which cannot be changed" / "Also used by X. Saving moves it here."),
`Taken` ("Ctrl+T was taken from New tab"); Reset per command and Reset All (`Keymap::reset_all`, also resets extension commands).
API: `Keymap::{default, from_overrides, overrides, chords(cmd), command_for(chord), is_default(cmd), offer, iter, assign, reset, reset_all}`,
`Prefs::keymap()/set_keymap()`, `Chord::{new, mods, key}`, `Chord: Display/FromStr/Serialize` ("Ctrl+Alt+Shift+Key"),
`Mods`, `Key::{ALL, name(), from_name(), is_function()}`, `Command::{ALL, id(), title(), section(), defaults(), from_id()}`.
**(NEEDS-CEF)** while the page has focus Chromium sees keys first: `CefKeyboardHandler::OnPreKeyEvent` must route the keymap's
chords to shell commands (as WinUI routes action shortcuts before the page). Print `CefBrowserHost::Print`, Find
`CefBrowserHost::Find/StopFinding` (+ `CefFindHandler`), DevTools `ShowDevTools`; no CEF API selects the console panel (as
WebView2), inference.
Checks: `shortcuts`, `ctrl_s_toggles_sidebar`, `page_commands`.

---

## 17. Profiles (`crates/vsesvit-core/src/profiles.rs`, `profiles/account.rs`) (KEEP)

User-visible: toolbar avatar (circle in the profile colour with the initial, or the sync account picture); its menu lists
every profile (current checked), "Add Profile…", "Manage Profiles…". Add asks name + colour (`next_name()` "Person N",
`next_color()`), opens the new profile in its own window. Manage Profiles: rows with "This profile" or Open, Edit ("Edit
Profile"), Remove (not the last). Picker ("Who's Using Vsesvit?") and Settings > General > "Show Profile Picker at Startup"
("Ask which profile to open when Vsesvit starts and there is more than one"). Window titles with >1 profile: "Page - Work"
(Linux), private "Page - Work (Private)". Private windows have no profile button. Sync sign-in names/pictures the profile
unless set by hand.
API: `ProfilesDir::{standard, at, path, root, locate, load, opened, add(name, color) -> (ProfileId, Registry),
edit_profile, take_account_details(id, name, picture), picture(&ProfileEntry) -> Option<PathBuf>, remove, sweep, set_show_picker,
startup, is_running}`; `Registry::{profiles, get, show_picker, last_used, is_removed, title_name, can_remove, next_color, next_name, startup}`;
`ProfileEntry { id, name, color }`, `set_by_hand()`, `initial()`; `avatar_initial(name)`;
`ProfileColor::{Blue, Teal, Green, Yellow, Orange, Red, Pink, Purple, Slate}` with `rgb()`, `css()`, `label()`;
`ProfileId::{DEFAULT, default_profile, parse, as_str}`; `profiles::with_profile(page, profile) -> String`; `NAME_MAX = 64`;
`EditError::{EmptyName, LongName, LastProfile, NotListed, Io}`; `AccountDetails::from_claims(&vsesvit_sync_proto::Claims)`,
`AccountPicture::{fetch(&Url) (worker), decode, png}`.
Check: `profiles`.

---

## 18. First-run welcome (`crates/vsesvit-core/src/onboarding.rs`) (KEEP)

User-visible (`crates/vsesvit-gtk/src/dialogs/welcome.rs`): "Welcome to Vsesvit" ("A web browser that runs extensions from the
Chrome Web Store. A few choices and you are ready to go.", button "Get Started"); pages stepped with Back/Next, every choice
applies at once, none required: Search Engine (one checked row per engine), Import Bookmarks (browsers found, "Bookmarks
File"), Extensions ("A few to start with, from the Chrome Web Store. The Extensions page adds more.", one Install button per
recommendation, no install prompt), Default Browser ("Make Vsesvit the Default Browser", Flatpak/no-desktop-entry notes),
Sync ("Sync your browser"), "You’re All Set" ("Start Browsing"). Closing at any page ends the first run; the main menu's
Welcome brings it back. Sync prompts wait until it closes.
API: `onboarding::should_show(&mut Profile)` (`is_new() && !ONBOARDING_DONE`), `onboarding::finish(&mut Profile)`,
`onboarding::RECOMMENDED_EXTENSIONS` = uBlock Origin Lite `ddkjiahejlhfcafbddmgiahcphecmpfh`, Bitwarden
`nngceckbapebfimnlniiiahkandclblb`, Proton Pass `ghmbeldphafepmbegfdlkpapadhbakde` (all Chrome Web Store),
`Recommended::{id(), install_source()}`.
Check: `welcome`.

---

## 19. Sync (core `sync.rs` + `vsesvit_sync`) (KEEP)

User-visible: Settings > Sync shows `vsesvit_sync::status::State::status(now_secs)` (title, subtitle, buttons:
`Action::label()` "Sign In", "Cancel", "Encrypt with a Passphrase…", "Enter Passphrase…", "Sync Now", "Change Passphrase…",
"Sign Out", "Delete Data on Server…" with `DELETE_CONFIRMATION`), "Customize Sync", "Sync Server". Sign-in opens the server's
page in a normal-window tab and finishes on a loopback port. The passphrase offer (`OFFER`: "Encrypt Your Synced Data?",
"Set Passphrase"/"Not Now", once per device) and the enter prompt (only Start Syncing or Sign Out closes it) appear over the
browser window. "Tabs from other devices" in History. Syncs 10 s after the first window, every 60 s, 10 s after a local
change (`Profile::change_seq`), on Sync Now, and once at quit (3 s wait).
Core surface: `Profile::sync() -> SyncStore`: `changes_since(Kind, Seq, limit) -> ChangeBatch`, `apply(Vec<WireRecord>) -> ApplyReport`,
`engine_state/set_engine_state`, `secret_state/set_secret_state`; `Kind::{Bookmarks=1, HistoryPages, HistoryDeletions, Sessions,
Extensions, ExtStorageSync, Prefs, SearchEngines, SitePermissions=12}`; `DataType::{Bookmarks, History, Tabs, Extensions, Settings}`
(`kinds()`, `label()`, `toggled()`, `all_in()`); `Changed { bookmarks, history, sessions, extensions, ext_storage, prefs, search_engines, site_permissions }`;
`Profile::change_seq() -> crdt::Seq`.
`vsesvit_sync` calls: `Http::new()`, `SignIn::start(&http, server)` (worker), `SignIn::authorize_url()`, `SignIn::canceller()`,
`SignIn::finish(&http) -> SignedIn { account, details: AccountDetails }`, `Account::{load, save, save_signed_in, forget, revoke, delete_server_data,
server, name, last_synced, encryption}`, `Round::gather(store, account, &types)` / `Round::run(&http) -> Exchanged` /
`Exchanged::finish(store) -> Finished { account, result: Result<Synced> }`, `Synced::{report, again, refused, outcome(n)}`, `MAX_ROUNDS = 50`,
`PassphraseJob::{new, run} -> NewKeys::finish`, `Passphrase`, `status::{prompt, passphrase_dialog, check_passphrase, ago}`,
`server_input`, `now_secs`.
Refresh after apply (`Browser::sync_applied`): bookmarks, history views, pref views, `extensions().reconcile()` when
`changed.extensions`, runtime storage changes, permission enforcement, trackers, cookies, keymap, theme, engine prefs.
**(risk)** With Chromium owning `chrome.storage`, `Kind::ExtStorageSync` gets no Linux writes (same limitation as Windows,
PLAN.md "Open questions").
Check: `sync_passphrase`.

---

## 20. Self-update (`vsesvit_update`) (KEEP)

Calls: `Installation::detect()`, `Installation::{self_updates, format, variant}`, `Config::builtin()`, `Updater::new(config, version, installation)`,
`Updater::check(channel.name()) -> Option<Available>`, `Available::{release, into_update}`, `Update::download(dir, progress) -> Downloaded`,
`Downloaded::install(&installation, relaunch_args) -> Installed::{ExitNow, Relaunch, NextLaunch}`, `remove_stale_downloads(dir, keep)`,
`DisabledReason`, `cli::{check, update, print}` for `--check-for-updates`/`--update`. GTK checks 30 s after start and every 24 h
(`crates/vsesvit-gtk/src/updates/mod.rs`), shows a banner/restart button, Settings > General > Updates ("Check for Updates",
version, status). Prefs: `updates.automatic`, `updates.channel`. Relaunch args come from the profile location
(`--profile-dir <root>`). Engine-independent except packaging (CEF ships a large runtime; packaging is out of scope here).

---

## 21. Copy link without tracking (`crates/vsesvit-core/src/clean_url.rs`) (KEEP)

`clean_url::clean(url: &str) -> String`: drops tracking parameters (global and per-site rules) and unwraps redirect
wrappers; anything else unchanged. Used by Ctrl+Shift+C, the tab menu's Copy Link, the link menu's Copy Link Without
Tracking; toast "Link copied". Ctrl+Alt+Shift+C copies the plain link.

---

## 22. Extensions (`crates/vsesvit-core/src/extensions/`)

### 22.1 Model

- **Desired state (synced)**: table `extensions`, `ExtensionRecord { id, store: Lww<StoreRef>, installed: Lww<bool>, enabled: Lww<bool>, extra }`,
  only for store-origin extensions (`StoreRef::{ChromeWebStore, EdgeAddons, Amo}`), sync kind 5, merged per register
  (`extensions/sync_table.rs`). Uninstall is `installed = false` (reinstall works).
- **Actual state (local)**: table `extension_installs` (`id, version, dir, source_kind, source JSON, verification JSON, manifest JSON,
  local_enabled, engine_id, installed_ms, granted`), one row per extension on this device. Unpacked and file installs exist only here.
- `ExtensionId` (`[A-Za-z0-9._@{}-]`, 1..=80, not `.`/`..`, no trailing `.`, not a Windows reserved name):
  `parse`, `from_public_key(spki_der)` (sha256[..16], nibbles a..p), `from_crx_id([u8;16])`, `for_unpacked_dir(abs_path)`
  (Chromium's `GenerateIdForPath`: hash of UTF-8 path bytes on POSIX), `is_chrome_style()`, `as_str()`.
- `Verification::{ChromeWebStore, EdgeAddons, AmoHash, LocalCrx, LocalXpi, Unpacked}` with `label()`: "Chrome Web Store,
  publisher verified", "Edge Add-ons, publisher verified", "Firefox Add-ons, hash checked", "Local CRX, signature verified",
  "Local XPI, not verified", "Unpacked folder, not verified". ID hold strength (internal `IdHold`): key-bound (CWS, Edge,
  local CRX) > store (AMO) > nothing (XPI, unpacked); a weaker install can never take a stronger one's id
  (`InstallError::VerifiedIdTaken`), and ids differing only by case are refused (`IdCaseConflict`).
- `InstalledExtension { id, version, dir: PathBuf (absolute), manifest: Manifest (parsed+localized at install), enabled: bool
  (user choice and nothing withheld), withheld: Vec<PermissionMessage>, source: InstallSource, verification, engine_id: Option<String> }`.
  `enabled` comes from the synced register for store installs and from `local_enabled` otherwise.

### 22.2 Install sources (`InstallSource`)

`InstallSource::{ChromeWebStore { id }, EdgeAddons { id }, Amo { slug_or_guid }, CrxFile { path }, XpiFile { path }, Unpacked { dir }}`;
`InstallSource::parse(text) -> Result<_, SourceParseError>` accepts `https://chromewebstore.google.com/detail/<slug>/<id>`
(or `/detail/<id>`), legacy `https://chrome.google.com/webstore/detail/...`, `https://microsoftedge.microsoft.com/addons/detail/...`,
a bare 32-char a..p id (Chrome Web Store), `https://addons.mozilla.org/[<locale>/][firefox/]addon/<slug>/`, a bare gecko id
(`name@domain`, `{uuid}`), `file://` URLs or paths to `.crx`/`.xpi`, a directory with `manifest.json` or the manifest itself.
Errors: "not a Chrome Web Store, Edge Add-ons or addons.mozilla.org extension URL, an extension id, or an extension file";
"the store URL does not contain a valid extension id". `InstallSource::from_path(&Path)` (file picker / drag-and-drop; lexical
absolute normalization), `InstallSource::web_store(CrxStore, id)`, `InstallSource::store() -> Option<StoreRef>`.

### 22.3 The pipeline (what a shell does)

```
UI:      job = profile.extensions().prepare_install(source)?        // cheap; refuses non-Unicode paths
worker:  staged = job.run(&mut |phase: InstallPhase| ...)?          // network, verify, unpack, key, manifest
UI:      if staged.needs_approval() { show install prompt; Cancel => drop staged (staging dir removed by Drop) }
UI:      installed = profile.extensions().commit(staged)?           // Option: None = no longer wanted (reconcile/update)
UI:      re-apply keymap; load into engine if installed.enabled
```

- `InstallJob::run` per source: CWS GET `https://clients2.google.com/service/update2/crx?response=redirect&prodversion=<v>&acceptformat=crx2,crx3&x=id%3D<id>%26uc`
  (204 or empty = `NotAvailable`), CRX3 verify with the CWS publisher key; Edge: POST Omaha 3.1 JSON to
  `https://edge.microsoft.com/extensionwebstorebase/v1/crx` (`)]}'` prefix), GET codebase, check sha256, verify with Edge publisher
  key; AMO: GET `https://addons.mozilla.org/api/v5/addons/addon/<slug_or_guid>/`, GET `current_version.file.url`, check
  `file.hash` ("sha256:<hex>"), id = manifest gecko id (must equal `guid`) else `guid`; local CRX: any valid developer proof;
  local XPI: no signature check, id = gecko id else path hash; unpacked: `Manifest::load(dir)` only, nothing copied.
  `ureq` agent: https only (except testkit stores), connect 30 s, response 60 s, body 600 s. Caps: `MAX_ARCHIVE_BYTES` 256 MiB,
  `MAX_UNPACKED_BYTES` 256 MiB, `MAX_ENTRIES` 20 000, 100x ratio (+1 MiB slack), API answers 1 MiB.
- `InstallPhase::{Resolving, Downloading { received, total }, Verifying, Unpacking, ReadingManifest}`, `describe()`:
  "Looking up the add-on…", "Downloading… 1.3 MB of 9.8 MB" / "Downloading… 500 KB", "Verifying the signature…",
  "Unpacking…", "Reading the manifest…"; `fraction() -> Option<f64>` (progress bar while size known).
- `StagedInstall::{id(), verification(), manifest(), needs_approval()}`: approval needed only for `Intent::User` and not
  unpacked (Chrome's "Load unpacked" and another device's installs go in silently). Both shells install the welcome
  recommendations without the prompt (GTK `Browser::install`, not `install_asking`).
- `Extensions::commit(StagedInstall) -> Result<Option<InstalledExtension>>`: drops reconcile/update results no longer wanted;
  refuses ids it may not take; keeps an installed newer version unless the new one holds the id more firmly (unpacked always
  replaces); renames staging into place unless the content-addressed dir exists; for a user store install sets
  `installed = true`, `store`, and `enabled = true` for new or previously uninstalled records; upserts the row; for updates keeps
  `granted` (what the user approved) so new warnings make it `withheld`; `engine_id` is cleared whenever `dir` changes
  (except withheld updates).
- `InstallError` messages (shown in "Install failed: <e>" toasts): "network: …", "server returned HTTP n", "the store has no
  version of this extension for this browser", "unexpected store response: …", "download larger than the n byte limit",
  "archive expands to more than n bytes", CRX errors ("not a CRX file", "CRX version n is not supported (only CRX3)", "truncated or
  malformed CRX header", "the CRX header contains zip end-of-central-directory magic", "a … signature in the CRX header does not
  verify", "no valid signature by the key the extension id is derived from", "no valid Chrome Web Store/Edge Add-ons publisher
  signature", "CRX is for extension X, expected Y"), "downloaded file does not match the store's sha256", "archive: …", "archive
  entry with unsafe path …", manifest errors ("manifest.json: missing or invalid `field`", "manifest_version n is not supported",
  "invalid file path …", "invalid match pattern …"), "extension id X does not match the expected Y", "only unpacked extensions can
  be reloaded", "extension X comes from a store or a signed package, and an unverified copy cannot take its id", "extension id X
  differs only in letter case from the installed Y", "… is not a valid Unicode path".

### 22.4 On-disk layout of installed extensions (what CEF would load)

Verified (`extensions/mod.rs`, `install.rs`):

```
<root>/staging/<uuid-simple>/root/          job scratch: zip unpacked here; StagingDir::drop removes <uuid>; wiped at open
        │ commit: fs::rename (same volume, atomic; retried 10x on PermissionDenied for Windows AV)
        ▼
<root>/extensions/<id>/<version>_<hash32>/  managed install dir, IMMUTABLE once committed
    manifest.json                           CRX: "key" = base64(SPKI DER of the verified developer key), replacing any key;
                                            XPI: "key" removed. Rewritten (pretty JSON, comments/trailing commas gone)
                                            only when the key changed; otherwise byte-identical to the archive.
    _locales/..., _platform_specific/...    kept; any other top-level "_" entry refuses the install
    (no _metadata/, no __MACOSX/)           skipped at unpack (store signature data)
    ...all other files, original names
```

- `<hash32>` = first 16 bytes (32 hex chars) of SHA-256 of the **downloaded archive bytes** (whole CRX/XPI file), so the same bytes
  map to the same dir (idempotent re-install) and different bytes with the same version never touch an existing dir.
- `<version>` = the manifest's `version` (Chrome grammar 1-4 dotted ints <= 65535; XPI up to 999 999 999 per part).
- `extension_installs.dir` stores `<id>/<version>_<hash32>` (forward slashes) for managed installs, the absolute path for
  unpacked ones (`InstalledExtension.dir` is always absolute).
- Unpacked developer dirs are **loaded in place, never copied or written** (`StagedFiles::InPlace`); their id is the manifest
  `key`'s id if present, else `ExtensionId::for_unpacked_dir(abs path)`. `reload_unpacked(id)` re-reads the manifest
  (id must not change) but **no shell calls it today**.
- Unpack safety: rejects absolute paths, `..`, `\`, NUL, `:`, `<>"|?*`, control chars, trailing dot/space, Windows reserved
  names, case-duplicate names, symlinks, encrypted entries; `manifest.json` must be at the root.
- GC at open (`extensions::on_open`): wipe `staging/`; delete rows whose dir vanished; delete every `extensions/<id>/<version>`
  dir no row references (case-insensitive compare), **except all dirs of an extension with a withheld update** (the engine still
  holds the old version); remove empty `<id>` dirs. Uninstall moves the whole `extensions/<id>` into `staging/` then deletes
  (best effort; never touches an unpacked developer dir). Reconcile removals leave dirs for the next open's GC.
- Manifest stored in the DB is core's parsed+localized `Manifest` (with `raw` JSON); rows from before `commands` existed are
  re-derived from `raw` on read.

Shell-side writes into the "immutable" dir that exist today (verified):
- **WinUI** writes `"key": base64("vsesvit-extension:<id>")` as the first member of a keyless managed install's
  `manifest.json` (XPIs) before WebView2 first loads it, so the engine id (and storage) stays stable across versions
  (`crates/vsesvit-winui/src/extensions.rs`, `ENGINE_KEY_PREFIX`, `engine_key`, `add_manifest_key`). Chromium derives an id from
  any base64 `key` (inference: `ParsePEMKeyBytes` only base64-decodes), so the same trick applies to CEF.
- **Chromium itself** writes `_metadata/` into a dir it loads unpacked (testkit `PROBE_FILES` comment: "without the `_metadata/`
  that Chromium writes into the source dir when it loads the dir unpacked"; e.g. indexed DNR rulesets, inference). Core's GC is by
  dir name, so this is harmless, but "immutable" is not literally true under Chromium.

CEF loading facts: CEF 154's C API has **no extension-loading API** (verified: no `cef_extension_t`, no `load_extension` in the
bindings; `cef_runtime_style_t` has `CHROME`/`ALLOY`). Loading these dirs therefore relies on Chrome-style runtime mechanisms such
as the `--load-extension=<dir>[,<dir>...]` switch at startup (inference; must be verified by the CEF research, including whether
Chromium 154 still honours it and how Chromium persists/forgets command-line extensions). Chromium ids: CRX installs carry `key`,
so Chromium's id equals core's id; keyless unpacked dirs hash the path as core does (Chromium canonicalizes with
`MakeAbsoluteFilePath`, resolving symlinks, while core normalizes lexically, so a symlinked path can give a different id,
inference); keyless XPI dirs would get a per-version path id unless the WinUI key trick is used. `InstalledExtension.engine_id` /
`Extensions::set_engine_id(id, engine_id)` exist for exactly this mapping (WinUI records `CoreWebView2BrowserExtension.Id`).

### 22.5 CRX3 verification (`extensions/crx.rs`) (KEEP)

`crx::parse(&[u8]) -> Crx3` ("Cr24", version 3, protobuf header; rejects zip EOCD magic inside the header);
`crx::verify(&Crx3, &VerifyPolicy) -> VerifiedCrx { id, developer_key, publisher_verified }` and
`verify_with_publisher(.., publisher_key_sha256)`: every RSA-SHA256 and ECDSA-P256-SHA256 proof must verify; one proof key must hash
to `crx_id` (developer key, required); `VerifyPolicy::WebStore { store, expected }` also requires id == expected and a proof by the
store's publisher key (`CWS_PUBLISHER_KEY_SHA256` = `61f7f2a6…67cf`, `EDGE_PUBLISHER_KEY_SHA256` = `675bd8ed…a1dc`);
`VerifyPolicy::AnyDeveloperKey` for local `.crx`. `CrxStore::{ChromeWebStore, EdgeAddons}`. `Stores` (`cws_update_url`,
`edge_update_url`, `amo_api_url`, publisher key hashes, `https_only`) with `cws_download_url`, `amo_api_url`,
`publisher_key_sha256`; `Profile::set_stores` (testkit) points installs at `testkit::FixtureStore`.

### 22.6 Manifest model (`extensions/manifest.rs`) (KEEP; some fields WEBKIT-ONLY consumers)

`Manifest { manifest_version: V2|V3, name, version, description, default_locale, icons: BTreeMap<u32, RelPath>, action:
Option<Action { default_popup, default_title, default_icon }>, background: Option<Background::{ServiceWorker{script, module},
Scripts{scripts, persistent}, Page{page, persistent}}>, content_scripts: Vec<ContentScript>, permissions, host_permissions:
Vec<MatchPattern>, optional_permissions, web_accessible_resources: Vec<WebAccessible>, dnr_rulesets: Vec<DnrRuleset>,
options_page, key, gecko_id, commands: Vec<ManifestCommand>, raw }`.
Normalizations: `browser_action`/`page_action` -> `action` (a bare icon string = size 16); MV2 host patterns in `permissions`
-> `host_permissions`; `optional_host_permissions` merged into `optional_permissions`; `__MSG_x__` in name/description/action
title/command descriptions localized from `_locales/<ui>`, `<language>`, `<default_locale>`; `commands[*].suggested_key` ->
this platform's `Chord` if Chrome would accept it (Ctrl or Alt but not both, max 4); `applications.gecko.id` /
`browser_specific_settings.gecko.id` -> `gecko_id`; comments/trailing commas/BOM tolerated (`parse_tolerant_json`).
API used by shells: `Manifest::{load(dir, ui_locale), parse(text, messages), key_der, key_id}`, `ManifestCommand::activates_action()`,
`manifest::ACTION_COMMANDS` (`_execute_action`, `_execute_browser_action`, `_execute_page_action`), `RelPath::{parse, as_str, resolve}`,
`MatchPattern::{parse, as_str, matches, within, covers, matches_all_hosts, warning_host}`, `chrome_shortcut(&str) -> Option<Chord>`,
`locale_chain`, `read_text`. `extensions::ui_locale()` from `LC_ALL`/`LC_MESSAGES`/`LANG` (`uk_UA.UTF-8` -> `uk_UA`, `C`/`POSIX` -> `en`).
Consumers that vanish with the WebKit runtime (**WEBKIT-ONLY**): `ContentScript`/`RunAt`/`World` injection, `WebAccessible` gating,
`DnrRuleset` translation to WebKit content blockers, `Background` emulation, `MatchPattern::matches` for injection. Under Chromium
the engine reads `manifest.json` itself; core's model is still used for the UI (name, version, icons, action title/popup,
commands, permission warnings, unsupported notice) (inference about the new consumers).

### 22.7 Permissions and prompts (`extensions/permissions.rs`) (KEEP wording; engine part NEEDS-CEF)

- Install prompt: `install_heading(name)` = `Add “<name>”?`, `INSTALL_LEAD` = "It can:", bullets from
  `install_warnings(&Manifest)` (required permissions + content-script sites, Chrome's coalescing: `<all_urls>` absorbs tabs etc.
  -> "Read and change all your data on all websites"; up to three hosts named, more -> "Read and change your data on a number of
  websites" with details). GTK: `adw::AlertDialog` "Cancel" / "Add Extension" (suggested), default Cancel
  (`crates/vsesvit-gtk/src/dialogs/extension_prompts.rs`); plus a warning line "Not supported by the Linux runtime: <list>.
  Features that depend on them will not work." (**WEBKIT-ONLY**, from `vsesvit_webext::unsupported_features`, which lives in
  `crates/vsesvit-webext/src/support.rs`, not core). WinUI shows the prompt inside the Extensions dialog and as a dialog over a
  store page.
- `permissions.request` prompt: `request_heading(name)` = `“<name>” has requested additional permissions.`, `REQUEST_LEAD` =
  "It could:", Deny / Allow. Driven by `permissions::request(&Manifest, active, requested) -> Result<Request::{Held, Grant(set),
  Ask(set, warnings)}, PermissionsError>`; errors in Chrome's words ("Only permissions specified in the manifest may be requested.",
  "You cannot remove required permissions.", "Extension must have file access enabled to request '…'.", "Invalid value for origin
  pattern …").
- Re-enable after an update that asks for more: `re_enable_heading(name)` = `Enable “<name>”?`, `RE_ENABLE_LEAD` = "It can now:",
  button "Re-enable" -> `Extensions::approve_permissions(id)` (grants what the update added, clears `granted`).
- Grants store: `GRANTED_PERMISSIONS` (Local pref); `Extensions::{active_permissions(id, &Manifest) -> PermissionSet,
  grant_permissions(id, &PermissionSet), remove_permissions(id, &Manifest, &PermissionSet) -> Result<PermissionSet, RemoveError>}`;
  forgotten on uninstall. Today only the Linux runtime calls them (`crates/vsesvit-webext/src/bridge.rs`, `runtime.rs`).
- `PermissionSet { apis, origins }` with `from_manifest_list`, `from_request`, `is_empty`, `union`, `contains`;
  `permissions::{required, optional, warnings, update_warnings}`; `PermissionMessage { text, details }`.
- **(NEEDS-CEF, risk)** under Chromium, `permissions.request` grants are Chromium's (its ExtensionPrefs), not core's; core's
  withholding (`commit` reads `GRANTED_PERMISSIONS` as "granted since") would not see them. Chromium's own install/permission/
  re-enable dialogs would need to be suppressed or bridged to the GPUI prompts (open question).

### 22.8 Updates (`extensions/update.rs`) (KEEP)

User-visible: silent periodic updates (first check 1 minute after start if the last check is 5 hours old, then every 5 hours;
logged only); Extensions dialog "Update" button ("Check every extension for a newer version now") shows a toast with
`UpdateReport::summary()`: "Your extensions are up to date", "Updated 2 extensions", "1 extension could not be updated",
"Updated n extensions; m could not be updated", then per withheld update: "The newest version of the extension “X” requires
more permissions, so it has been disabled. It can now: …". An update needing new permissions is installed but off with the
Re-enable row.
API: `Extensions::next_update_check() -> Duration` (meta key `extensions_update_checked_ms`; `FIRST_CHECK_DELAY` 60 s,
`UPDATE_INTERVAL` 5 h), `prepare_update_check() -> UpdateCheck` (records now; store installs the synced record still wants,
enabled or not), `UpdateCheck::{is_empty, run} -> Updates { staged, failed }` (worker: CWS gupdate XML batches of 20 at
`?response=xml&prodversion=…&acceptformat=crx3&x=id%3D…%26v%3D…%26uc`, Edge Omaha 3.1 one request, AMO one GET each; newer =
`cmp_versions` greater; fetched and verified exactly as installs), `commit_updates(Updates) -> UpdateReport { updated, failed }`.
Shell after commit: re-apply keymap, reload each updated extension in the engine, refresh views.
**(NEEDS-CEF)** "reload in the engine" = make Chromium load the new dir in place of the old one (same id via `key`), keeping
storage (risk: depends on the loading mechanism, section 22.4).

### 22.9 Reconcile with other devices (KEEP)

`Extensions::reconcile() -> Reconcile { install: Vec<InstallJob>, removed: Vec<ExtensionId> }` at startup and whenever
`Changed.extensions`: desired-but-missing (or held only by a weaker local copy) -> `Intent::Reconcile` jobs (run, commit, no
prompt; commit returns `None` if uninstalled meanwhile); store installs no longer desired -> rows deleted now, shell unloads them,
dirs GC'd at next open. Reconcile never writes desired state. WinUI runs reconciles one at a time and re-runs if asked meanwhile.

### 22.10 Enable, disable, uninstall, engine id (KEEP)

`Extensions::{list() (install order), get(id), set_enabled(id, bool) (synced register for store installs, local column otherwise),
uninstall(id) (synced installed=false for store installs; deletes row and storage.local; keeps storage.sync; best-effort dir
removal), approve_permissions(id), set_engine_id(id, engine_id), reload_unpacked(id)}`. GTK flow: unload from the runtime before
uninstall; if core refuses, load it again (`Browser::uninstall_extension`). A load failure keeps the extension installed and shows
"Not running" with the error in its row; enabling it toasts "Enabled, but it cannot run: …".

### 22.11 Extension keyboard shortcuts (`extensions/commands.rs`) (KEEP; firing NEEDS-CEF)

User-visible: Settings > Shortcuts lists "Extension shortcuts" after the browser's: per enabled extension command, title
(`ExtensionCommand::title()`: description, else "Activate the extension" for action commands, else the name) and its chord;
changeable and resettable; Reset All resets them too. Browser chords always win; user overrides next; then suggested keys in
install order. Capture notes: `Keymap::offer_extension` ("Used by X, which an extension cannot take" / "Also used by <ext>: <title>.
Saving moves it here."), `ExtensionShortcuts::note_for_browser(chord)` for the browser commands' capture dialog.
API: `commands::extension_commands(&[InstalledExtension]) -> Vec<ExtensionCommand { extension, extension_name, command }>`,
`Keymap::extension_shortcuts(Vec<ExtensionCommand>) -> ExtensionShortcuts`, `Profile::extension_shortcuts()`,
`ExtensionShortcuts::{iter, chord, command_for, is_default, note_for_browser}`, `Keymap::{offer_extension, assign_extension,
reset_extension}`. Overrides stored in `keyboard.shortcuts` under `extension:<id>:<command>`.
Firing: GTK runs `win.extension-command` -> runtime fires `commands.onCommand` (with activeTab) or opens the popup / clicks the
action; WinUI binds only action commands (WebView2 cannot fire `onCommand`). **(NEEDS-CEF, risk)** CEF exposes no way to fire
`chrome.commands.onCommand` or open an action popup (inference); Chromium may handle `commands` itself only inside Chrome-style
browser windows (open question).

### 22.12 Toolbar actions (`extensions/toolbar.rs`) (KEEP; data source NEEDS-CEF)

User-visible (`crates/vsesvit-gtk/src/window/ext_actions.rs`): one button per pinned action (icon + badge) in the synced order;
a puzzle-piece Extensions menu listing every action with a pin toggle; clicking opens the action's popup page in a popover under
its button (or under the puzzle piece when unpinned); drag pinned buttons to reorder; right-click for the extension's own action
menu items (`contextMenus` "action" contexts) and Unpin. Private windows show only extensions allowed there (WinUI shows none).
API: `toolbar::TOOLBAR` pref, `toolbar::layout(available: &[String], saved: &[Entry]) -> Layout { pinned, unpinned }` (new
installs appended pinned; ids of absent extensions kept), `toolbar::set_pinned(available, saved, id, pinned) -> Vec<Entry>`,
`toolbar::move_pinned(available, saved, id, to) -> Vec<Entry>`. `available` = ids with an action, in install order.
WinUI builds buttons from core's manifest (`ExtensionAction::from_installed`: title = `action.default_title` or name, icon =
closest to 32 px from `action.default_icon` else `icons`, popup URL `chrome-extension://<engine_id>/<default_popup>`), shows the
popup in a flyout, and has no badges or action menu items. **(NEEDS-CEF, risk)** live badge text/colour, dynamic icons/titles,
`action.onClicked`, and popup hosting come from Chromium's extension system, which CEF does not expose (inference).

### 22.13 Notifications switch (`extensions/notifications.rs`)

User-visible: Extensions dialog row "Notifications" / "Let this extension show notifications" (only for extensions with the
`notifications` permission); each notification's Settings button opens the Extensions dialog expanded on that extension.
API: `NOTIFICATIONS_OFF` (synced), `Extensions::{notifications_allowed(id), set_notifications_allowed(id, bool)}`.
Today implemented by the Linux runtime (GNotification) only (**WEBKIT-ONLY** implementation). **(NEEDS-CEF, open)** whether
CEF/Chromium shows `chrome.notifications` at all on Linux, and how to switch them off per extension.

### 22.14 Allow in private windows (`extensions/private.rs`)

User-visible: Extensions dialog row "Allow in private windows" / "Vsesvit cannot stop it from recording what you do there";
off by default; allowed extensions run in private tabs and see them; others don't exist there.
API: `ALLOWED_IN_PRIVATE` (synced), `Extensions::{allowed_in_private(id), set_allowed_in_private(id, bool)}`.
**(NEEDS-CEF, open)** Chromium's equivalent is per-extension "Allow in Incognito" in its ExtensionPrefs; how a CEF
off-the-record request context treats extensions is unknown here.

### 22.15 `chrome.storage` backing (`ext_storage.rs`) (WEBKIT-ONLY consumer)

`Profile::ext_storage() -> ExtStorage`: `get(ext, Area::{Local, Sync}, keys)`, `set`, `remove`, `clear`, `bytes_in_use`;
quotas `SYNC_QUOTA_BYTES` 102 400, `SYNC_QUOTA_BYTES_PER_ITEM` 8 192, `SYNC_MAX_ITEMS` 512, `LOCAL_QUOTA_BYTES` 10 MiB;
`StorageChange { key, old_value, new_value }`; `Changed.ext_storage`. Only the Linux runtime uses it. Under Chromium,
`chrome.storage` lives in Chromium's profile (LevelDB), so these tables and sync kind 6 go unused on Linux, as on Windows (risk:
loses Linux-to-Linux `storage.sync` sync).

### 22.16 Shell UI that drives extensions (GTK wording, KEEP)

Extensions dialog (`crates/vsesvit-gtk/src/dialogs/extensions.rs`, main menu > Extensions): group "Install" ("Paste a store link or
id, choose a .crx or .xpi file, or load a folder for development") with entry "Chrome Web Store, Edge Add-ons or Firefox Add-ons
link or ID" (apply button), "Install from File…" (filter "Extension packages (.crx, .xpi)", dialog "Install Extension from File"),
"Load Unpacked Extension…" (folder dialog), progress row "Installing…" + spinner + `InstallPhase::describe()` subtitle (concurrent
installs share it). Toasts: "Installed <name> <version>", "The extension was removed elsewhere while it downloaded", "Installed …,
but it cannot run: …", "Install failed: …". Group "Installed" with "Update" button; empty: "No extensions installed". Each
extension an expander row: icon (largest manifest icon, 32 px), name, subtitle "<version> · <verification label>", Enabled switch
(insensitive while withheld), Remove (trash); expanded rows: unsupported notice (WEBKIT-ONLY), Re-enable row, "Not running" error,
Notifications switch, Allow in private windows, ID (selectable), Location (dir, selectable).
WinUI extras: store-page installs. On `https://chromewebstore.google.com` and `https://microsoftedge.microsoft.com` a main-world
script replaces `chrome.webstorePrivate` (`beginInstallWithManifest3`, `completeInstall`, `getExtensionStatus`, `getFullChromeVersion`,
...) and forwards requests to the shell, which installs through core with the install prompt and answers the page
(`crates/vsesvit-winui/src/store.rs`, `js/store.js`). The GTK shell has no store-page integration. **(NEEDS-CEF, risk)** in a
Chrome-style CEF the store's real `webstorePrivate` may reach Chromium's own installer, bypassing core and sync (open question).

### 22.17 What changes when Linux runs Chromium (summary per extension piece)

| Piece | Today on Linux | With CEF | Class |
|---|---|---|---|
| Install sources, CWS/Edge/AMO fetch, CRX3 verify, unpack, key injection, content-addressed dirs | core | unchanged | KEEP |
| Desired vs actual, reconcile, sync kind 5 | core | unchanged | KEEP |
| Install prompt / Re-enable wording, Update summary, verification labels | core + GTK dialogs | unchanged, in GPUI | KEEP |
| Loading the dir into the engine | `vsesvit_webext::Runtime::load` | Chromium loads unpacked dir (mechanism TBD, no C API) | NEEDS-CEF |
| `engine_id` / `set_engine_id` | unused on Linux | needed if Chromium ids differ (XPI, symlinked paths) | NEEDS-CEF |
| Withheld update keeps old version loaded+off | runtime unloads | do not load the new dir until approved | NEEDS-CEF |
| Content scripts, background, messaging, DNR, webNavigation, cookies, menus, notifications, windows/tabs APIs | home-grown runtime | Chromium native | WEBKIT-ONLY (runtime) |
| "Not supported by the Linux runtime" notice | `vsesvit_webext::unsupported_features` | drop (or replace with a Chromium/CEF gap list) | WEBKIT-ONLY |
| `ext_storage` tables, `Changed.ext_storage`, kind 6 | runtime backing | unused | WEBKIT-ONLY |
| `GRANTED_PERMISSIONS`, `active_permissions`, `permissions::request` | runtime | Chromium owns grants; core withholding loses "granted since" | NEEDS-CEF (risk) |
| `NOTIFICATIONS_OFF`, `ALLOWED_IN_PRIVATE` | runtime enforces | need Chromium equivalents | NEEDS-CEF (risk) |
| Extension commands | runtime fires `onCommand` | no CEF API to fire | NEEDS-CEF (risk) |
| Toolbar layout pref | core + GTK | unchanged; live action state needs Chromium | KEEP + NEEDS-CEF |
| `tab_groups::extension_group_id` | runtime's `chrome.tabs` | Chromium's `chrome.tabs` knows nothing of GPUI tabs/groups | WEBKIT-ONLY |
| MV2 / Firefox XPI add-ons | runtime emulates MV2 and `browser.*` | Chromium 154 likely refuses MV2, has no `browser.*` namespace (inference) | risk |

Checks touching extensions: `install_prompt`, `install_crx`, `engine_loaded_extension`, `content_script`, `extension_port`,
`dnr_blocked`, `dnr_site_allowed`, `dynamic_content_script`, `extension_windows`, `extension_web_navigation`, `extension_cookies`,
`popup`, `extension_toolbar`, `context_menus`, `extension_commands`, `extension_notifications`, `extension_update`, `cws_install`
(`--network`).

---

## 23. Engine-specific pieces inside core (flag list)

| Core item | Specific to | What changes on Chromium |
|---|---|---|
| `lib.rs` docs: `ProfilePaths.engine_data` = WebKit `NetworkSession` data dir, `engine_cache` = WebKit cache dir | WebKit | becomes CEF `root_cache_path`/`cache_path`; old WebKit data needs a migration decision |
| `OpenOptions.chrome_version` default `DEFAULT_CHROME_VERSION = "150.0.0.0"` ("Linux keeps the default") | Linux had no Chromium | pass `154.0.8037.98` from cef-dll-sys constants |
| `session::TabSnapshot.restore_state`, `tab_restore_state` table | WebKit `get_session_state` | unused (CEF cannot restore history); keep column for compatibility |
| `view_source::source_page` | WebKitGTK has no view-source | unused; `source_url`/`viewed_url` stay |
| `trackers::TrackerList::dnr_rules` | WebKit content blocker | unused; use `TrackerList::blocks` per request like Windows |
| `cookies::third_party_blocked(.., top: None)` "as WebKitGTK sets one policy for all"; `site_choices(_, exceptions=false)`; `SiteRules::clears(name)` on WebKit website-data records | WebKit | per-site exceptions possible (`exceptions = true`); clearing by origin |
| `cookies::block_script` | both engines (WebKit + WebView2 lack a cookie-blocking hook) | Chromium content setting COOKIES=BLOCK may make the script unnecessary (inference) |
| `spellcheck` (Hunspell dirs, Enchant, `en_US` names) | Enchant | Chromium `.bdic`, `en-US` names, prefs |
| `memory_saver` docs ("Where sleeping reloads the page (Linux)") | WebKit terminate-web-process | depends on chosen CEF sleep mechanism |
| `ext_storage` ("backing for the Linux runtime"), `sync::Changed::ext_storage` | home-grown runtime | unused |
| `extensions::manifest` docs ("the Linux runtime never has to check manifest_version"; `dnr_rulesets` "translates these to WebKit content-blocker JSON"; `MatchPattern` "decides content-script injection") | home-grown runtime | model kept for UI; runtime consumers gone |
| `extensions::permissions` ("The one place a shell or the Linux runtime reads what an extension may do") | home-grown runtime | see 22.7 |
| `extensions::notifications`, `extensions::private` | enforced by the runtime | need Chromium equivalents |
| `tab_groups::extension_group_id` | runtime's `chrome.tabs` | unused |
| `certificate.rs` docs (WebKit `GTlsCertificate`, WebView2 `Network.getCertificate`) | both | `CefX509Certificate::GetDEREncoded` |
| `history.rs` docs (titles from `notify::title`) | WebKit | `OnTitleChange` |
| `extensions::Verification` / install docs ("WebView2 drops an extension whose files change") | WebView2 | reason for immutability stays valid for Chromium |
| `extensions::InstalledExtension.engine_id` ("recorded by the Windows shell") | WebView2 | may be reused for Chromium ids |
| `favicons/fetch.rs` `USER_AGENT` (Chrome 150 UA string) | none | could follow the CEF version (cosmetic) |
| `memory_saver::TITLE`, `https_only::TITLE` ("sentence case; GTK title-cases it itself") | GTK HIG | GPUI chooses its own casing |
| `vault` (Secret Service on Linux) | Linux, not WebKit | unchanged |

Not in core (but often assumed to be): the list of "APIs the Linux runtime lacks" is `crates/vsesvit-webext/src/support.rs`
(`unsupported_features`, `SUPPORTED_PERMISSIONS`, `UNSUPPORTED_MANIFEST_KEYS`); DNR -> WebKit is `crates/vsesvit-webext/src/dnr.rs`;
the navigation `Gate` is `vsesvit_webext::Gate`.

---

## 24. Core API reference (as the shells call it)

Signatures abbreviated: `Result<T>` = `Result<T, vsesvit_core::Error>` unless stated; `&mut self` on every accessor.

### Profile and process
```rust
Profile::default_root(name: &str) -> PathBuf
Profile::open(root: &Path, opts: OpenOptions) -> Result<Profile, OpenError>   // Locked | TooNew{found,supported} | Db | Io
OpenOptions { time: TimeSource, new_device_id: Option<DeviceId>, chrome_version: String, key_store: vault::KeyStore }
Profile::paths(&self) -> &ProfilePaths   // root, db, extensions, staging, engine_data, engine_cache
Profile::is_new(&self) -> bool;  Profile::device_id(&self) -> DeviceId;  Profile::chrome_version(&self) -> &str
Profile::change_seq(&self) -> crdt::Seq
Profile::end_private_session(&mut self)
Profile::set_stores(&mut self, extensions::Stores)            // feature "testkit"
Profile::{bookmarks, downloads, favicons, history, session, prefs, site_permissions, search_engines, omnibox,
          extensions, ext_storage, sync}()  ;  site_permissions_in(Browsing), site_zoom(Browsing)
Profile::extension_shortcuts() -> Result<ExtensionShortcuts>
vsesvit_core::Error::{Db, Io, Bookmark, Storage, Install, Vault, NotFound, AlwaysAsks(Permission), EngineForm(FormError), RemoveDefaultEngine}
vsesvit_core::Url (= url::Url)
```

### Prefs
```rust
Prefs::get<T: DeserializeOwned>(&Pref<T>) -> T;  set<T: Serialize>(&Pref<T>, &T) -> Result<()>;  reset<T>(&Pref<T>) -> Result<()>
Prefs::keymap() -> Keymap;  set_keymap(&Keymap) -> Result<()>
prefs::{homepage_input(&str) -> Option<HomepageValue>, homepage_url(&str) -> Option<Url>, DEFAULT_SYNC_SERVER, SYNC_TYPES_KEY}
prefs::{Theme, TabsPosition, Startup, UpdateChannel{ALL,name,of_version,of_build}, Pref{key,scope,default}, Scope}
prefs::keys::*   (table in section 2)
```

### Session, tabs
```rust
Session::save(&SessionSnapshot) -> Result<()>;  restore() -> Result<Option<SessionSnapshot>>;  other_devices() -> Result<Vec<DeviceTabs>>
session::{SessionSnapshot, WindowSnapshot, TabSnapshot, TabId(Uuid)::new(), DeviceTabs}
tab_place::TabPlace{index,count,pinned}::{is_pinned, closes_others, closes_after, can_move_out}
tab_search::rows<O,C>(&str, Vec<Listed<O>>, Vec<Listed<C>>) -> Vec<Row<O,C>>;  Listed{key,title,url,used};  Hit::{Open,Closed}
tab_groups::TabGroups<K>::{restore, group_of, get, is_empty, in_order, members, new_group, join, leave, set_collapsed,
    next_shown, after_closing, set_title, set_color, ungroup, adopt, rejoin, settle, rows}
tab_groups::{WindowTabs{order,pinned,active}, Step, Row, TabGroup{id,title,color,collapsed}::name, GroupId::new, GroupColor{ALL,label,rgb,next}, extension_group_id}
private::Browsing::{Normal, Private}::loggable
```

### Omnibox, search, address
```rust
Omnibox::resolve(&str) -> Result<Option<NavTarget>>
Omnibox::suggest(&str, limit: usize, allow_inline: bool) -> Result<Suggestions>
Omnibox::suggest_request(&str, &suggest::Queries, private: bool) -> Result<Option<SuggestRequest>>
Omnibox::for_selection(&str) -> Result<Option<SelectionAction>>
Suggestions{items,inline}::add_search_suggestions(&SearchSuggestions);  Suggestion{source,title,target,fill};  SuggestionSource
suggest::{Queries::default/cancel, SuggestRequest::run -> Option<SearchSuggestions>, SearchSuggestions::is_current, DEBOUNCE}
search::{classify, classify_url, selection_action, ctrl_enter_url, NavTarget::url, SelectionAction{label,url}}
SearchEngines::{list, default_engine, set_default, check(Option<&SearchEngineId>, &EngineForm) -> Result<Vec<FormError>>, save, remove}
search::{SearchEngine, SearchEngineId::{builtin_default,is_builtin}, UrlTemplate::expand, EngineForm::of, FormError::{field,is_blank}, FormField}
address::{readable_url(&str) -> String, simplified_url(&str) -> String, covers(domain, host) -> bool}
new_tab::{page(&mut Profile, Browsing) -> Result<String>, html(&[TopSite], &SearchEngine), PRIVATE_PAGE, TILES, TopSite{url,label}}
view_source::{source_url(&str) -> Option<String>, viewed_url(&str) -> Option<&str>, source_page(url, source) -> String}
clean_url::clean(&str) -> String;  html::escape(&str) -> String
```

### History, bookmarks, favicons, import/export
```rust
History::{record_visit(&Url, Transition), set_title(&Url, &str), search(&str, usize) -> Vec<HistoryEntry>, top_sites(usize),
          visits_between(i64, i64, usize) -> Vec<(HistoryEntry, Visit)>, delete_url(&Url), delete_range(i64, i64)}
history::{Transition, ClearRange{ALL,label,start}, HistoryEntry, Visit, is_recordable, MAX_VISITS}
Bookmarks::{get, children, is_bookmarked, find_by_url, search, add_url, add_folder, add_separator, rename, set_url, move_to,
            remove, import, import_folder}
bookmarks::{BookmarkId::{ROOT,TOOLBAR,OTHER,MOBILE}, BookmarkNode, NodeKind, InsertAt, ImportItem, BookmarkError, MAX_DEPTH, materialize}
Favicons::{record(&Url, &[u8]) -> Result<bool>, wanted(&Url) -> bool, missing(usize) -> Result<Vec<Url>>,
           commit_fetched(Vec<Fetched>) -> Result<bool>, get(&Url) -> Result<Option<Vec<u8>>>}
favicons::{FaviconFetch::new(Vec<Url>).run() -> Vec<Fetched>, Fetched{page,outcome}, Outcome, allow_local_hosts, MAX_BYTES}
export::{html(&Bookmarks) -> String, file_name(i32, u32, u32) -> String, default_folder() -> Option<PathBuf>}
import::{installed_browsers() -> Vec<Found>, Found{name,source}::folder_title, Source::{File,Chromium,Firefox}::read,
         parse_html, parse_chromium, FILE_FOLDER_TITLE, MAX_FOLDER_DEPTH, ImportError}
```

### Downloads, zoom, memory saver, spellcheck
```rust
Downloads::{start(&str, &Path, Option<u64>, u64, Browsing) -> Result<Download>, update(DownloadId, State, u64, Option<u64>),
            list(usize, Browsing) -> Result<Vec<Download>>, remove(DownloadId), keep(DownloadId) -> Result<Option<PathBuf>>,
            discard(DownloadId), clear(Browsing), interrupt_stale() -> Result<usize>}
downloads::{Download, DownloadId::browsing, State::{is_live,is_final}, listed_in, unique_destination, unconfirmed_path,
            is_dangerous, zone_identifier, sanitize, describe_size, Transfer::{new,update,restart,at}, Progress, status_line}
SiteZoom::{get(&Url) -> Result<f64>, set(&Url, f64) -> Result<()>};  zoom::DEFAULT
memory_saver::{Sweep::new(&mut Profile, Instant), Sweep::sleeps(&TabActivity, &mut IdleClock) -> bool, IdleClock::{new,restart},
               TabActivity{url,shown,pinned,audible,capturing,related}, UNSAVED_INPUT_SCRIPT, has_unsaved_input, SWEEP_EVERY,
               TITLE, DESCRIPTION, MODE_TITLE, MemorySaverMode::{ALL,delay,label,description}}
spellcheck::{Dictionaries::{new,installed,checked,choose}, installed_in, system_locales, display_name}
```

### Site permissions, trackers, cookies, HTTPS-only, certificates
```rust
SitePermissions::{get, set, for_site, all, by_site, reset_site, decide, answer}
permissions::{prompt, site_rows, Permission, Setting, Answer, Prompt, Decision, SiteChoice, SiteRow, SiteSetting, SiteGroup,
              Capturing{any,uses,description}, TabGrants{allows,committed,granted,revoke}, Origin{of,parse,as_str,host_for_display}}
trackers::{TrackerList::{bundled,parse,with_tracker,domains,blocks,dnr_rules}, TrackingProtection::{ALL,label,description},
           Category, level_for, allowed, set_allowed, allowed_sites, site_status, TITLE}
cookies::{third_party_blocked, setting, set, site_choices, choice_label, site_status, site_rules,
          SiteRules::{blocked_hosts,blocks_cookie,to_clear,clears_cookie,clears}, block_script, group_by_site, SiteCookies,
          ThirdPartyCookies::{ALL,label,description,blocks}, SITE_TITLE}
https_only::{upgraded, upgrade, allow, allowed, warning_page, Reach, Cause, Next, Upgrades::{starting,finished,committed,leave,warning},
             TITLE, DESCRIPTION, WARNING_TITLE}
certificate::{parse_der(&[u8]) -> Result<Certificate, CertificateError>, Certificate, Name::display}
```

### Shortcuts
```rust
shortcuts::Keymap::{default, from_overrides, overrides, chords, command_for, is_default, offer, iter, assign, reset, reset_all,
                    extension_shortcuts, offer_extension, assign_extension, reset_extension}
shortcuts::{Command{ALL,id,title,section,defaults,from_id}, Section{ALL,title}, Chord{new,mods,key}, Mods, Key{ALL,name,from_name,is_function},
            Overrides, Taken{from,chords}, NEEDS_MODIFIER_NOTE, NOT_A_KEY_NOTE}
```

### Profiles, onboarding, vault
```rust
profiles::ProfilesDir::{standard, at, path, root, locate, load, opened, add, edit_profile, take_account_details, picture, remove,
                        sweep, set_show_picker, startup, is_running}
profiles::{Registry::{profiles,get,show_picker,last_used,is_removed,title_name,can_remove,next_color,next_name,startup},
           Home{dir,id}::root, ProfileId::{DEFAULT,default_profile,parse,as_str}, ProfileEntry::{set_by_hand,initial}, ProfileColor{ALL,rgb,css,label},
           Startup::{Open,Picker}, EditError, NAME_MAX, avatar_initial, with_profile, AccountDetails::from_claims, AccountPicture::{fetch,decode,png}}
onboarding::{should_show, finish, RECOMMENDED_EXTENSIONS, Recommended{name,blurb,store,id}::{id,install_source}}
vault::KeyStore::{System, Basic}
```

### Sync store
```rust
SyncStore::{changes_since(Kind, Seq, usize) -> Result<ChangeBatch>, apply(Vec<WireRecord>) -> Result<ApplyReport>,
            engine_state, set_engine_state, secret_state, set_secret_state}
sync::{Kind{ALL,code,from_code}, DataType{ALL,kinds,of,label,toggled,all_in}, WireRecord{kind,id,body}, ChangeBatch{records,upto,more},
       ApplyReport{merged,unchanged,rejected,changed}, Changed, Rejected}
crdt::Seq
```

### Extensions
```rust
Extensions::{list() -> Result<Vec<InstalledExtension>>, get(&ExtensionId) -> Result<Option<InstalledExtension>>,
             prepare_install(InstallSource) -> Result<InstallJob>, commit(StagedInstall) -> Result<Option<InstalledExtension>>,
             set_enabled(&ExtensionId, bool), uninstall(&ExtensionId), reconcile() -> Result<Reconcile{install,removed}>,
             approve_permissions(&ExtensionId), set_engine_id(&ExtensionId, &str), reload_unpacked(&ExtensionId) -> Result<InstalledExtension>,
             next_update_check() -> Result<Duration>, prepare_update_check() -> Result<UpdateCheck>, commit_updates(Updates) -> UpdateReport,
             active_permissions(&ExtensionId, &Manifest) -> PermissionSet, grant_permissions(&ExtensionId, &PermissionSet),
             remove_permissions(&ExtensionId, &Manifest, &PermissionSet) -> Result<PermissionSet, RemoveError>,
             notifications_allowed(&ExtensionId) -> bool, set_notifications_allowed(&ExtensionId, bool),
             allowed_in_private(&ExtensionId) -> bool, set_allowed_in_private(&ExtensionId, bool)}
extensions::{ExtensionId{parse,from_public_key,from_crx_id,for_unpacked_dir,is_chrome_style,as_str}, InvalidExtensionId,
             StoreRef, ExtensionRecord, Verification::label, InstalledExtension, DEFAULT_CHROME_VERSION, ui_locale()}
extensions::{InstallSource::{parse,from_path,web_store,store}, SourceParseError, Intent::{User,Reconcile,Update},
             InstallJob::{source,run(self, &mut dyn FnMut(InstallPhase)) -> Result<StagedInstall, InstallError>},
             StagedInstall::{id,verification,manifest,needs_approval}, InstallPhase::{describe,fraction}, InstallError,
             MAX_ARCHIVE_BYTES, MAX_UNPACKED_BYTES, MAX_ENTRIES, Stores::{default,cws_download_url,amo_api_url,publisher_key_sha256}}
extensions::{UpdateCheck::{is_empty,run}, Updates{staged,failed}, UpdateReport{updated,failed}::summary, FIRST_CHECK_DELAY, UPDATE_INTERVAL}
extensions::crx::{parse, verify, verify_with_publisher, Crx3, KeyProof, ProofAlgorithm, CrxStore, VerifyPolicy, VerifiedCrx, CrxError,
                  CWS_PUBLISHER_KEY_SHA256, EDGE_PUBLISHER_KEY_SHA256, CRX_MAGIC}
extensions::manifest::{Manifest::{load,parse,key_der,key_id}, ManifestVersion, Action, Background, ContentScript, RunAt, World,
                       WebAccessible, DnrRuleset, RelPath, MatchPattern, ManifestCommand::activates_action, ACTION_COMMANDS,
                       chrome_shortcut, locale_chain, parse_tolerant_json, read_text, ManifestError}
extensions::permissions::{PermissionSet{apis,origins}::{from_manifest_list,from_request,is_empty,union,contains}, PermissionMessage{text,details},
                          install_warnings, update_warnings, warnings, required, optional, request, Request, PermissionsError, RemoveError,
                          install_heading, request_heading, re_enable_heading, INSTALL_LEAD, REQUEST_LEAD, RE_ENABLE_LEAD, GRANTED_PERMISSIONS}
extensions::commands::{extension_commands, ExtensionCommand::title, ExtensionShortcuts::{iter,chord,command_for,is_default,note_for_browser}}
extensions::toolbar::{TOOLBAR, Entry{id,pinned}, Layout{pinned,unpinned}, layout, set_pinned, move_pinned}
extensions::notifications::NOTIFICATIONS_OFF;  extensions::private::ALLOWED_IN_PRIVATE
ext_storage::{ExtStorage::{get,set,remove,clear,bytes_in_use}, Area, StorageChange, quotas}
```

### Testkit (feature `testkit`, used by `--self-test`)
```rust
testkit::{FixtureServer::{start,route,port,origin,url,hits}, FixtureRequest, FixtureResponse, STALLED_SENT, FixtureStore::{start,stores,publish_crx,publish_xpi},
          CrxKey::{probe,second,rsa_pem,ecdsa,...}, write_crx3, sign_crx3, encode_crx3, zip_files, PROBE_ID = "eonajgebgeenbhiiobbhmkafolkeghdb",
          PROBE_FILES, probe_crx(), update_probe_files(version, permissions), reads_speed_and_time_left,
          report::{Report::{new(platform, expected), push, checks}, Check{name,ok,ms,detail}}}
SearchEngines::set_suggest_url(&SearchEngineId, Option<&str>)   // testkit
https_only::Reach::Everywhere;  trackers::TrackerList::with_tracker;  favicons::allow_local_hosts()
```

---

## 25. Self-test interplay (`docs/design/self-test.md`, `crates/vsesvit-gtk/src/self_test.rs`)

The run uses `<out_dir>/profile` (fresh), keeps its profile list in `<out_dir>`, turns search suggestions off, writes
`report.json` via `testkit::report::Report::new("linux", expected)` and `window.png`, exits non-zero on any failure, 15 s per
check. GTK's check order: `install_prompt install_crx engine_loaded_extension favicon_preload navigate history_recorded
content_script extension_port dnr_blocked dnr_site_allowed dynamic_content_script extension_windows extension_web_navigation
extension_cookies bookmark star_bubble bookmark_export tabs tab_animation tab_layout tab_menu tab_search tab_groups memory_saver
popup extension_toolbar context_menus extension_commands extension_notifications extension_update omnibox address_completion
search_suggestions selection_search session download private_window download_safety download_speed new_tab_page address_progress
settings spellcheck search_engines bookmarks_bar_menus ctrl_s_toggles_sidebar shortcuts save_page page_commands zoom_indicator
zoom_is_remembered_per_site connection_info site_permissions tracking_protection https_only cookies capture_in_use
sync_passphrase profiles welcome screenshot cws_install` (plus `profile_open` in the contract).

Core features each check exercises (core side): `install_*`/`extension_update`/`cws_install` -> pipeline, `FixtureStore`,
`probe_crx`, `update_probe_files`, `Verification::{LocalCrx, ChromeWebStore}`, withholding/Re-enable; `navigate`/`history_recorded`
-> `History::record_visit`; `favicon_preload` -> `favicons().missing/commit_fetched`; `bookmark*`/`star_bubble` -> bookmarks,
`export::html`; `tab_*` -> `tab_place`, `tab_search::rows`, `tab_groups`, session groups; `memory_saver` -> `Sweep` (7 hours on);
`omnibox`/`address_completion`/`search_suggestions`/`selection_search`/`search_engines` -> search module (+`set_suggest_url`);
`session` -> `Session::save/restore`; `download*` -> downloads module, `DOWNLOADS_DIR`; `private_window` -> private session,
negative download ids, `end_private_session`; `new_tab_page` -> `new_tab::page`; `settings`/`spellcheck`/`shortcuts` -> prefs, keymap,
`Dictionaries`; `zoom_*` -> `site_zoom`; `connection_info` -> `certificate::parse_der`; `site_permissions`/`capture_in_use` ->
permissions; `tracking_protection` -> `TrackerList::with_tracker("localhost", Analytics)`; `https_only` -> `Reach::Everywhere`;
`cookies` -> cookies module; `sync_passphrase` -> `vsesvit_sync::status`; `profiles` -> `ProfilesDir::at(out_dir)`,
`take_account_details`, `AccountPicture` (`favicons::allow_local_hosts` lets it fetch from the fixture server); `welcome` ->
onboarding. Linux-only checks tied to the WebKit runtime (`dnr_*`, `dynamic_content_script`, `extension_windows`,
`extension_web_navigation`, `extension_cookies`, `context_menus`, `extension_commands`, `extension_notifications`) will need new pass
conditions against Chromium's native APIs (inference).

---

## 26. Persistent state summary

SQLite `vsesvit.db` (schema version 11; `db::MIGRATIONS`): synced tables `bookmarks`, `history_pages` (+ derived stats),
`history_visits`, `history_deletions`, `device_sessions`, `prefs` (`synced` flag), `search_engines`, `ext_storage_sync`,
`extensions`, `site_permissions`; local tables `meta` (`device_id`, `clock_last`, `next_seq`, `created_ms`,
`extensions_update_checked_ms`), `tab_restore_state`, `ext_storage_local`, `extension_installs`, `favicons`, `favicon_failures`,
`downloads`, `site_zoom`, `vault_key`, `sync_state`, `sync_secrets`. Files: `LOCK`, `extensions/`, `staging/`, `engine/`, account
picture PNG, `profiles/profiles.json` + `profiles.lock`, Secret Service item "Vsesvit Safe Storage". Shell-owned state that core
does not persist: closed-tab stacks, tab MRU ticks, per-tab `TabGrants`, `Upgrades`, `IdleClock`s, `Transfer`s, private-session
engine data, `Queries`.

---

## 27. Checklist

- [ ] CLI: `[URL...] [--profile-dir DIR]`, `--self-test OUT_DIR [--network]`, `--check-for-updates`, `--update`, `--help`, `--version`
- [ ] Profile resolution: `ProfilesDir::sweep` + `startup(has_urls)`; picker window "Who's Using Vsesvit?" with Add card and "Show on startup"
- [ ] `Profile::open` with `chrome_version` = CEF's Chromium version; handle `Locked` (forward command line), `TooNew`, others
- [ ] Single instance per profile with command-line forwarding; launching another profile with activation token; `notify` re-read
- [ ] Removed-profile handling at start and when a window comes forward (`opened`, `is_removed`, quit without saving)
- [ ] Engine data dirs from `ProfilePaths.engine_data/engine_cache`; decide fate of old WebKit data
- [ ] Start sequence: theme, keymap, trackers, cookies (+ clear at start), extensions list/load, reconcile, update schedule, favicon preload, Memory Saver timer, `interrupt_stale`
- [ ] Shutdown: save session, clear-on-exit sites, final sync (3 s)
- [ ] Every pref in section 2 readable/writable from Settings with the listed wording and live effect
- [ ] `sync_applied(&Changed)` refresh fan-out
- [ ] Session save (2 s debounce, normal tabs only, never empty-overwrite) and restore (windows, pinned, groups, active, size, maximized)
- [ ] Device name (`device.name` or host name) in the session record
- [ ] Tab context menu with Chrome's items; Close Others / to the Right via `TabPlace`
- [ ] Closed-tab stacks (25, per kind) and Reopen Closed Tab (Ctrl+Shift+T), group rejoin
- [ ] Tab search popover (Ctrl+Shift+A, buttons in sidebar/top bar) via `tab_search::rows`
- [ ] Tab groups (`TabGroups`, settle after every change, headers/chips, collapse, editor, Ctrl+Tab skipping, after-closing pick)
- [ ] Private windows: separate OTR request context, dark chrome, private new tab page, no profile button, no writes, end of session cleanup
- [ ] Omnibox: suggest rows, inline completion, keyboard handling, switch-to-tab rows, forget history row, Ctrl+Enter
- [ ] Search suggestions with debounce, staleness, private/off/plain-search rules
- [ ] Address display: `readable_url` + `simplified_url` (unless full URLs), empty for about:blank
- [ ] Selection search and link menu items (private window, copy without tracking) from `CefContextMenuParams`
- [ ] History recording on commit with transitions; titles on title change; never for private/error pages
- [ ] New tab page and private page loaded so the tab still reads as blank
- [ ] Homepage / Home button (`homepage_url`, Transition::Bookmark)
- [ ] View page source opens `view-source:<page>` in a new tab (native Chromium rendering)
- [ ] Error pages (certificate, load failure, crash) with shell wording
- [ ] History window: recent, search, open, forget, clear range, Tabs from other devices
- [ ] Clear Browsing Data (history range + all engine website data)
- [ ] Star / Ctrl+D with Bookmark added / Edit bookmark bubble
- [ ] Bookmarks bar: buttons, folders as menus, Other Bookmarks, overflow "»", drag-and-drop, context menus
- [ ] Bookmarks window: tree, search, edits, drag, import (browsers + file), export (Documents, Chrome file name)
- [ ] Favicons of bookmarked pages recorded from tabs (PNG) and preloaded without visits
- [ ] Downloads: destination rules, ask-where, toasts, header button, Downloads window with live `status_line`
- [ ] Dangerous downloads as `.unconfirmed`, warning popover with Keep/Discard
- [ ] Download pause/resume (newly possible on Linux with CEF)
- [ ] Save Page As (MHTML) through the downloads list
- [ ] Permission prompts (core wording/answers, one at a time), decide/answer/TabGrants with requesting origin from CEF
- [ ] Mirror stored permission settings into Chromium content settings so `navigator.permissions.query` agrees
- [ ] Capture in-use indicator and Stop
- [ ] Site info popover: connection, tracking switch, cookie rule, permission rows, certificate chain (`CefX509Certificate`)
- [ ] Settings > Site Permissions subpage (by site, combo, remove, empty state)
- [ ] Tracking protection levels and per-site exception, per-request blocking with blocked count
- [ ] Third-party cookie choice and per-site Allow/Block/Clear on exit via Chromium content settings; clear at exit and start
- [ ] Site data subpage (list per site, remove one/all)
- [ ] HTTPS-only upgrade/warn/continue via `Upgrades` on main-frame `OnBeforeBrowse`/load errors; warning page under the http URL
- [ ] Per-site zoom (presets, indicator, bubble, menu row), Chromium zoom-level conversion
- [ ] Memory Saver sweep, unsaved-input script, sleeping/waking mechanism, faded icon
- [ ] Spell check on/off and languages mapped to Chromium dictionaries; Add to Dictionary; suggestions in context menu
- [ ] Search engines editor (list, add, edit, make default, remove except default, field errors)
- [ ] Keymap: all commands with defaults, shortcut capture dialog notes, reset/reset all, routing chords before the page
- [ ] Print, Find bar, Developer tools, JavaScript console, Fullscreen, Reload bypassing cache
- [ ] Profiles: avatar/picture, profile menu, Add, Manage (open/edit/remove), picker switch, titles with profile name
- [ ] First-run welcome pages (engine, import, recommended extensions without prompt, default browser, sync, done) and menu re-entry
- [ ] Sync Settings (status, actions, customize, server), sign-in in a tab, passphrase offer/enter prompts, periodic and change-driven syncs
- [ ] Account details and picture from sync into the profile list
- [ ] Self-updater lifecycle (30 s first check, 24 h interval, banner, restart), Settings > Updates, headless CLI
- [ ] Copy link / copy clean link with "Link copied" toast
- [ ] Extensions dialog: install by link/id, from file, load unpacked; progress text; result toasts
- [ ] Install prompt (`Add “X”?`, "It can:", warnings) before user store/package installs; none for unpacked, reconcile, welcome
- [ ] Commit installs, then load the managed dir into Chromium (mechanism TBD) keeping core's id (`key`), record `engine_id` where it differs
- [ ] Keyless managed installs (XPI) get a stable engine id (WinUI `vsesvit-extension:<id>` key trick or equivalent)
- [ ] Installed list: icon, name, "<version> · <verification label>", Enabled switch, Remove, ID, Location
- [ ] Enable/disable/uninstall with engine kept in step; uninstall failure keeps it running; load errors shown as "Not running"
- [ ] Reconcile at start and after sync (`Changed.extensions`): install missing store extensions, unload removed ones
- [ ] Periodic extension update checks and the Update button with `UpdateReport::summary`
- [ ] Withheld updates: stay off with `Enable “X”?` / "It can now:" / Re-enable (`approve_permissions`); old version not running the new code
- [ ] `permissions.request` prompt wording (`“X” has requested additional permissions.`, "It could:") bridged from Chromium
- [ ] Extension shortcuts: list/change/reset in Settings, resolution order, firing commands
- [ ] Extension toolbar: pinned buttons in synced order, puzzle menu with pin toggles, drag reorder, popup popover, action menu items, badges
- [ ] Per-extension notifications switch (synced) and notification Settings button
- [ ] Allow in private windows switch (synced) enforced in the OTR context
- [ ] Store-page installs (Chrome Web Store / Edge Add-ons "Add" buttons) routed through core, not Chromium's own installer
- [ ] Decide fate of `ext_storage`/kind 6 `storage.sync` sync under Chromium
- [ ] Self-test: reuse `testkit` (FixtureServer, FixtureStore, probe CRX, report) and port every check, with new pass conditions for runtime-specific ones
