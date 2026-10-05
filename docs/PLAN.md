# Vsesvit: plan

Vsesvit is a web browser for Windows 11 and Linux, written in Rust. Each platform gets a native shell around the platform's own web engine. One shared core owns the data, the policy and the extension install pipeline.

| | Windows 11 | Linux |
|---|---|---|
| UI toolkit | WinUI 3 (Windows App SDK 2.5) | GTK 4.22 + libadwaita 1.9 |
| Web engine | WebView2 (Chromium, Evergreen runtime) | WebKitGTK 6.0 (2.52) |
| Extension runtime | WebView2's own Chromium extension system | Vsesvit's WebExtensions runtime (`vsesvit-webext`) |
| Rust bindings | `windows-bindgen` 0.100 generated bindings | `gtk4` 0.11, `libadwaita` 0.9, `webkit6` 0.6 |

## Decisions and the evidence behind them

**WinUI 3 is driven from pure Rust.** No C# or C++ host. A spike bootstrapped the Windows App Runtime from an unpackaged exe and composed `Microsoft.UI.Xaml.Application` from Rust. It then built the UI from XAML strings, created a WebView2 environment with browser extensions enabled, installed an unpacked MV3 extension and opened its popup page (`design/research-winui.md`, screenshot in `design/evidence/winui-spike.png`). Three findings shape the shell:

- The shell loads `Microsoft.Web.WebView2.Core.dll` itself, because the framework package registers it but does not ship it.
- The tab strip is a strip only. Web views live in a separate grid, because a WebView2 inside `TabViewItem` content lays out with zero height.
- There is no bootstrap DLL. Windows 11's dynamic-dependency API does the bootstrap.

**Linux uses WebKitGTK, not Chromium.** WebKitGTK is a real GTK4 widget: it works on Wayland, composes with libadwaita tabs, and is packaged by every distribution. Chromium (through CEF) cannot embed in GTK4 except by off-screen rendering, which costs GPU performance and correct input handling. The price is extension support, because WebKitGTK has no extension runtime. Vsesvit builds one, as GNOME Web does. A spike proved the primitive it needs: a content script in an isolated script world calls into Rust and gets a Promise reply (`design/research-linux-extensions.md`).

**Extensions come from the Chrome Web Store on both platforms.** The same pipeline runs everywhere:

1. Download the CRX3 from the Web Store update endpoint.
2. Verify every signature proof: the developer RSA key bound to the extension id, plus the Web Store's publisher key.
3. Unpack into an immutable directory.
4. Write the public key into `manifest.json` as `key`, so the unpacked copy keeps its store id.

WebView2 then loads the directory natively. On Linux, `vsesvit-webext` runs it. Firefox add-ons from AMO install through the same pipeline as a fallback, with AMO's SHA-256 checked.

**Synced data is stored sync-ready from day one.** Bookmarks, history, open tabs, installed extensions, extension `storage.sync`, preferences and search engines live in one SQLite file per profile. Every synced record is a join-semilattice: last-writer-wins registers stamped with a hybrid logical clock, grow-only sets, and terminal tombstones. Every row carries a change sequence number. A future sync engine lists local changes with `changes_since(kind, seq)` and applies remote batches with `apply(records)`, in any order and any number of times. The bookmark tree shown to the user is computed from the merged records by a pure function, so every device shows the same valid tree (no cycles, no orphans) whatever order changes arrived in. A property test runs three simulated devices with skewed clocks through a server that never merges, and checks that they converge. The design came out of a three-way design arena and a cross-judge (`design/core.md`, `design/arena/`).

**Sync signs in with OpenID Connect and keeps a dumb server.** The server (`server/`) stores the last uploaded body of each record per account and never merges, which is the server `sync.rs` was designed for. Every write takes the account's next sequence number, so a download is a cursor over writes. Anyone can host one: SQLite or Postgres through SeaORM, one Docker image, and the OpenID Connect provider the operator names (there is no default). The browser talks to the sync server only, so it works with any server and needs no client id: the server is an OAuth authorization server of its own, and signs people in with its provider as that provider's client. The browser runs the authorization code flow with PKCE against the server, in a Vsesvit tab, and receives the code on a loopback port of its choosing; the server's callback learns who signed in from the provider's userinfo `sub` (an access token's own `sub` need not be the user) and hands back a one-time code that only the browser holding the PKCE verifier can trade for a session. Requests for records use that session and never reach the provider. The server address is the local pref `sync.server`, by default `https://vsesvit-service.mrquantumoff.dev`. Bodies are opaque to the server, so end-to-end encryption can come later without a server change.

**Tracking protection blocks Vsesvit's own tracker list in both shells.** Settings > Privacy picks Off, Standard (the default: advertising and analytics trackers) or Strict (social media trackers too), a synced pref. The site-info popup turns it off for a site, stored as the site's Trackers setting with its other site permissions, so it syncs the same way. The list (`crates/vsesvit-core/src/trackers.json`) groups tracker domains by the company that runs them, and a company's trackers load on its own sites. It is written for Vsesvit under the MIT licence because the well-known lists are not: DuckDuckGo's, Disconnect's and Ghostery's are CC BY-NC-SA 4.0, and EasyPrivacy is GPL-3.0 or CC BY-SA 3.0. It ships with each release and is never fetched. On Linux core turns it into declarativeNetRequest rules, which the extension translator (`vsesvit-webext/src/dnr.rs`) compiles into a WebKit content blocker. On Windows the tab asks core about each request WebView2 reports for a listed domain and answers the tracker's with a 403. WebView2's own tracking prevention (None, Basic, Balanced, Strict) would map onto Off, Standard and Strict, but it has no per-site exceptions and reports nothing it blocked, so Vsesvit sets it to None and blocks the list itself, which also gives Windows a count of blocked trackers in the popup. WebKitGTK reports no content-blocker hits, so Linux shows no count.

**HTTPS-only follows Chrome's HTTPS-First mode, decided in core.** Settings > Privacy's "Always use secure connections" (a synced pref, off by default) makes each main-frame http navigation load its https URL instead, for public sites on port 80 as Chrome does: localhost, private and link-local addresses, single-label and local names (`.local`, `.internal`, `.home.arpa`, ...) and other ports load as asked. `vsesvit_core::https_only::Upgrades` follows each tab from the upgrade to the https load's end; when that fails (a network, TLS or certificate error) or redirects back to http on a host it already upgraded, the tab shows core's warning page at the http address, "This site doesn't support a secure connection", whose "Continue to site" stores Allow on the site's Insecure connections setting, synced with the other site permissions and listed in Settings' site list, where it can be removed. Chrome's exceptions lapse after 15 days; Vsesvit's stay until removed. Windows stops the navigation in `NavigationStarting` and loads the https URL, and shows the warning with `NavigateToString`, keeping it out of the History list. WebKitGTK's `decide-policy` asks about subframes too without saying which frame a navigation is for, so Linux upgrades there only what is surely the main frame's: what the browser started, or anything while the page on screen is https (where an http frame would be blocked mixed content), blank, or one of Vsesvit's own pages. A link from an http page is upgraded once its load starts, so that one http request may already be out, though its page never shows.

**Secure DNS is the engine's and the system's.** WebView2 runs DNS-over-HTTPS in automatic mode, upgrading to the system's DNS provider's DoH endpoint where the provider has one; Chromium reads the mode and the provider only from its local-state preferences and enterprise policy, which WebView2 does not let an app set, and no browser argument or feature switch reaches them, so Windows Settings describes this instead of offering a provider choice. WebKitGTK uses the system resolver, so Linux Settings says the system's network settings control DNS.

**Passwords are left to password managers.** Vsesvit stores, fills and syncs no passwords. WebView2's password saving is always off on Windows (its form autofill for addresses stays, behind a Privacy switch), and WebKitGTK has no password store. The first start of a Windows profile on a version with this change deletes the passwords WebView2 saved before it, and nothing else, retrying on later starts until that succeeds. Settings > Privacy says so and suggests a password manager's extension, such as Bitwarden or Proton Pass, which the welcome screen also offers. Sync kinds 10 and 11, once reserved for passwords and autofill, stay unused.

## Architecture

```
crates/
  vsesvit          the binary: picks the shell for the target OS
  vsesvit-core     platform-agnostic: profile store (SQLite), sync-ready records, omnibox,
                   extension install pipeline (CRX3/XPI/unpacked, manifest model), testkit
  vsesvit-winui    Windows shell: WinUI 3 + WebView2 (compiles to nothing elsewhere)
  vsesvit-gtk      Linux shell: GTK4 + libadwaita + WebKitGTK (compiles to nothing elsewhere)
  vsesvit-webext   Linux WebExtensions runtime on WebKitGTK; its DNR translator is platform-neutral
  vsesvit-update   self-updater: the client side of the Tauri updater protocol
  vsesvit-sync     sync engine: OpenID Connect sign-in and the rounds between a profile and a server
  vsesvit-sync-proto  the sync server's HTTP API, shared by the engine and the server
server/            the self-hostable sync server (its own Cargo workspace, SeaORM on SQLite or Postgres)
```

Threading model:

- The `Profile` lives on the UI thread and is `!Send`.
- Every call a shell makes on the UI thread is a local SQLite transaction or an in-memory lookup. Bookmarks are held in memory.
- Slow work is a `Send` value with no database handle: an extension download and verify (`InstallJob::run`) today, and a sync engine's network I/O later. It runs on a worker thread and its result is committed back on the UI thread.
- Nothing is shared between threads, so there are no locks.
- Secrets (sync tokens) are sealed with one random key per profile (`vault.rs`), fetched on the UI thread on first use and kept for the process. On Linux that first use may wait on a keyring unlock prompt.

Profile directory, one per profile:

```
<data dir>/Vsesvit/profiles/<name>/
  LOCK                                 OS file lock; a second process gets "profile in use"
  vsesvit.db (+ -wal, -shm)            all synced and local records; `vault_key` holds the profile's key wrapped by
                                       DPAPI (Windows), or notes it is in the Secret Service as "Vsesvit Safe Storage"
                                       (Linux), or holds it unprotected where there is neither (on Linux, until a Secret Service is reachable)
  extensions/<id>/<version>_<sha256 prefix, 32 hex>/   unpacked extensions, immutable once committed
  staging/                             in-flight installs, wiped at open
  engine/                              WebView2 user data folder / WebKit network session data
```

## Synced data at a glance

| Kind | Key | Merge | Deletion |
|---|---|---|---|
| Bookmarks | UUID | per-field LWW; placement (parent + fractional position) is one register | terminal tombstone for the subtree the deleting device could see |
| History pages | URL | title LWW; newest 64 visits (union, then top 64) | synced deletion directives (grow-only) |
| Sessions (open tabs) | device id | whole snapshot LWW, one writer per record | forget device |
| Extensions | extension id | installed / enabled / store LWW | `installed = false` |
| Extension `storage.sync` | extension + key | LWW per key | value `None` |
| Preferences | key | LWW per key; local-only prefs never exported | value `None` (default) |
| Search engines | id | per-field LWW | tombstone |
| Site permissions | permission + origin | LWW per (origin, permission): allow or block; screen sharing remembers only block; picture-in-picture, trackers and insecure connections are never asked for (Allow on trackers turns tracking protection off for the site; Allow on insecure connections is HTTPS-only's exception, which has no Block) | setting `None` (ask; for picture-in-picture and trackers, block; for insecure connections, HTTPS-only applies) |

`storage.sync` data from Windows extensions stays inside WebView2, which owns the extension runtime there, so it syncs only between Linux installs until a bridge exists.

## Tabs

Tabs are vertical by default, in a sidebar on the left. A setting moves the sidebar to the right, or switches back to a horizontal strip at the top. The setting is the synced preference `tabs.position` (`left` | `right` | `top`).

- **Linux.** An `AdwOverlaySplitView` holds a tab list bound to `AdwTabView`'s page model. Its `sidebar-position` places the list at the start or end, and it collapses to an overlay on narrow windows. The top layout uses `AdwTabBar`. Both show `AdwTabView`'s tab menu, set up per tab, with Chrome's items: new tab to the right (below in the sidebar), move to a new window, reload, duplicate, pin, mute, copy link (without tracking parameters), close, close others and close to the right (both leave pinned tabs open), and reopen closed tab. Pinned tabs are `AdwTabView`'s pinned pages, which lead the list and are saved in the session. A muted tab, or one playing sound, shows a speaker that mutes or unmutes it.
- **Windows.** A collapsible pane holds a reorderable `ListView` of tabs (favicon, title, close button), on either side of the web content. When collapsed, the pane shows only favicons. The top layout uses the `TabView` strip in the title bar. With vertical tabs, the title bar holds the toolbar instead.

## Page commands

Print, Developer tools, the JavaScript console and View page source are rows in core's shortcut table, with Chrome's keys. Like every other shortcut they can be reassigned. View page source opens `view-source:<page>` in a new tab next to the page, for http, https and file pages.

- **Windows.** While the page has focus, WebView2 handles Ctrl+P, F12, Ctrl+Shift+I and Ctrl+Shift+J itself. The menus and the window's accelerators call `ShowPrintUI` and `OpenDevToolsWindow`. WebView2 has no API that picks the console panel. WebView2 renders `view-source:` pages natively but reports the address of the page inside, so the tab keeps the `view-source:` address the shell loaded.
- **Linux.** Print uses WebKit's print dialog, and Developer tools toggles WebKit's inspector. WebKitGTK has no view-source, so the shell serves the `view-source` scheme itself. It renders the page's main resource with core's `view_source::source_page`. That resource comes from a tab already showing the page, or else from a hidden view with scripts off. The scheme is registered as local, so web pages can neither open nor embed it.


## Search engines

Settings > Search lists every engine with its name, shortcut and URL, as Chrome's "Manage search engines" does. You can add an engine, edit one, make one the default or remove one. Built-in engines can be removed too, but the default can't. The editor asks for a name, a shortcut and a URL with `%s` where the search terms go. Core checks the form (`SearchEngines::check`): every field is filled in, the shortcut is one word that no other engine has, and the URL is an http or https address. A URL typed without a scheme gets `https://`. Engines sync as part of Settings. The address bar reads the engine list each time it classifies what was typed, so a new shortcut works at once (`fx rust` searches the engine whose shortcut is `fx`).

- **Linux.** The Search page has the default engine's row and a Manage Search Engines row, which opens a subpage listing the engines. Each engine row has a menu with Make Default, Edit… and Remove. The editor is an `AdwAlertDialog` with three entry rows.
- **Windows.** The Search page lists the engines under the default engine's box. Each row has a More actions menu with Make default, Edit and Delete. The editor is a flyout, because a second dialog can't open over Settings.

### Search suggestions

As the user types a search, the address bar asks the default engine for suggestions at its `suggest_url` and reads the OpenSearch JSON answer, as Chrome does. Core does the whole exchange (`search::Omnibox::suggest_request`, `suggest::SuggestRequest::run`): it waits 100 ms after a keystroke, sends nothing if the user typed again meanwhile, and drops an answer that arrives after a newer keystroke. It fetches on a worker thread with a 5 s limit. Like current desktop Chrome, the list keeps its default match first and puts the search rows (the typed search and up to four suggestions) above the URL rows from history and bookmarks. The rows join the open list when the answer arrives, and the highlighted row and the text in the box stay as the user has them. `suggest_request` decides what may leave the device. It sends nothing when Settings > Search > Search suggestions is off (a synced setting, on by default), when the default engine has no suggestion URL, or in a private window. It also sends nothing unless the text is a plain search of the default engine, so a URL being typed, a file path, a `file:` URL or a shortcut search of another engine never leaves the address bar. Engines added in the editor have no suggestion URL, as in Chrome, so only the built-in engines suggest.

## Extensions in detail

Install sources are Chrome Web Store URLs or ids, AMO add-on URLs or gecko ids (a bare slug is not accepted, because it cannot be told apart from a relative path), local `.crx`/`.xpi` files, and unpacked developer directories. Each install records how it was verified.

Extensions' keyboard shortcuts come from the manifest's `commands`, with Chrome's rules for suggested keys: Ctrl or Alt but not both, at most four per extension. Core resolves them against the browser's shortcuts (`extensions::commands`). The browser's own shortcuts always win. The user's changes come next, then suggested keys in install order, so the first extension installed keeps a contested key. The user's changes are stored with the browser's shortcut overrides in the synced `keyboard.shortcuts` register. Settings > Shortcuts lists them under Extension shortcuts, where they can be changed or reset, and Reset All resets them too.

**Windows.** WebView2 runs extensions natively: MV3 service workers, content scripts, `chrome.storage`, `chrome.tabs` inside extension pages, and declarativeNetRequest. WebView2 has no browser chrome for extensions, so Vsesvit draws the toolbar action buttons and shows each popup page in a flyout. `chrome.tabs` in WebView2 does not know about Vsesvit's tabs (WebView2Feedback #3853 and #3854). WebView2 puts extensions' `contextMenus` items in the page's context menu itself. It offers no way to read an extension's items for its action, so the toolbar button's menu has none. WebView2 does not run extensions' keyboard shortcuts (`commands`) and offers no way to fire `commands.onCommand`, so Vsesvit binds only the action shortcuts (`_execute_action` and its MV2 names), which open the popup as the toolbar button does.

**Linux.** `vsesvit-webext` implements the WebExtensions subset below on WebKitGTK:

- A `chrome-extension://` URI scheme serves extension files. Only `web_accessible_resources` are visible to web pages.
- The background runs as a hidden web view. MV3 service workers are emulated as a page.
- Content scripts run in a per-extension isolated world.
- `chrome.*` and `browser.*` are available with both promises and callbacks. The covered APIs are runtime messaging and ports (`runtime.connect`, `tabs.connect`, messages and connections from other extensions as `externally_connectable` allows), storage (local, sync, onChanged), i18n, tabs, scripting, action/browserAction, alarms, context menus (`contextMenus`, and Firefox's `menus`), commands (`getAll`, `onCommand`), permissions.contains, extension.getURL and `runtime.getBackgroundPage` (in the background page and its popups).
- Extensions' context menu items show in the page's context menu, by what was clicked and in which frame, grouped under the extension's name when it has several, and those for the action show in its toolbar button's menu. A service worker's items are kept across restarts, as in Chrome.
- Extensions' keyboard shortcuts (`commands`) work while a Vsesvit window has focus, `"global": true` ones included. A named command fires `commands.onCommand` with the selected tab and grants `activeTab` on it. An action command opens the popup, or clicks the action when it has none.
- declarativeNetRequest static rules are translated to WebKit content-blocker rules. Rules WebKit cannot express are skipped and logged.
- Not supported: `webRequest` blocking, native messaging, devtools pages, messages from web pages (`externally_connectable.matches`) and some other surfaces. Heavily API-dependent extensions may therefore work partially. The extensions page shows which APIs an extension requests that the runtime lacks.

## Verification

- **Core.** Lattice-law and tree-validity property tests, the three-device convergence test, CRX3 round-trip and tamper tests, and manifest normalization tests.
- **Each shell.** `vsesvit --self-test <dir>` runs the same scripted end-to-end check on each platform (`design/self-test.md`). It covers installing a signed test CRX, a content script round trip to the background over a message and a port, a declarativeNetRequest block observed at the fixture server, the action popup, bookmarks, tabs, the omnibox, session save and restore, and an in-app screenshot. It writes `report.json` and exits non-zero on any failure.

## Milestones

1. **Spikes (done).** Pure-Rust WinUI 3 + WebView2 + extensions. WebKitGTK isolated-world messaging.
2. **Core.** Data model, sync model and convergence tests, then the install pipeline and testkit.
3. **Shells.** GTK/libadwaita and WinUI UIs: vertical tabs (left by default, right, or top), navigation, omnibox, bookmarks bar, dialogs.
4. **Extensions end to end.** WebView2 loading and action popups on Windows, and the `vsesvit-webext` runtime on Linux.
5. **Self-tests green on both platforms.**
6. **Packaging and updates.** An NSIS installer on Windows. deb, rpm, pacman, AppImage and Flatpak on Linux. Every format but Flatpak updates itself through the Tauri updater protocol, so an existing Tauri update server serves Vsesvit (`design/packaging.md`).
7. **Later.**
   - End-to-end encryption of sync records. The server stores bodies it never reads, so only the engine changes.
   - Extension auto-update.

## Dependencies

Linux build. It needs GTK 4.22, libadwaita 1.9, WebKitGTK 2.52 and GLib 2.80 or newer (Ubuntu 26.04 or newer), because the bindings are built with those version features and their build scripts check the system libraries through pkg-config. Ubuntu/Debian package names:

```
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev
```

`libwebkitgtk-6.0-dev` pulls in `libsoup-3.0-dev` and `libjavascriptcoregtk-6.0-dev`. SQLite is bundled and TLS uses rustls, so neither `libsqlite3-dev` nor `libssl-dev` is needed. For audio and video playback, also install `gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav`.

Windows build:

- The Rust MSVC toolchain.
- Visual Studio 2022+ Build Tools with the C++ workload.
- The WebView2 Evergreen runtime, which ships with Windows 11.
- The Windows App Runtime 2.5.1 or newer (x64). The shell checks this at startup and explains how to install it.

The build fetches `Microsoft.Web.WebView2.Core.dll` from NuGet, pinned by SHA-256.

## Open questions

- Should a Linux install auto-install synced Chrome extensions that the Linux runtime supports only partly, or ask first?
- Bookmark conflict policy: cycle losers and orphans land in "Other bookmarks". Is that the right landing place?
- `storage.sync` on Windows would need a bridge that intercepts WebView2's `chrome.storage.sync`. WebView2 exposes no hook for that today.
