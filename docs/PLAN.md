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

**Sync signs in with OpenID Connect and keeps a dumb server.** The server (`server/`) stores the last uploaded body of each record per account and never merges, which is the server `sync.rs` was designed for. Every write takes the account's next sequence number, so a download is a cursor over writes. Anyone can host one: SQLite or Postgres through SeaORM, one Docker image, and the OpenID Connect provider the operator names (there is no default). The browser talks to the sync server only, so it works with any server and needs no client id: the server is an OAuth authorization server of its own, and signs people in with its provider as that provider's client. The browser runs the authorization code flow with PKCE against the server, in a Vsesvit tab, and receives the code on a loopback port of its choosing; the server's callback learns who signed in from the provider's userinfo `sub` (an access token's own `sub` need not be the user) and hands back a one-time code that only the browser holding the PKCE verifier can trade for a session. Requests for records use that session and never reach the provider. The server address is the local pref `sync.server`, by default `https://vsesvit-service.mrquantumoff.dev`.

**Sync can be end-to-end encrypted with a sync passphrase, opt-in.** An account without a passphrase syncs unencrypted, as before, and whoever runs the sync server can read it; signing in or updating never waits for a passphrase. Each device offers one once (Set Passphrase or Not Now, remembered in the local pref `sync.passphrase_offered`), and Settings > Sync has "Encrypt with a Passphrase…" at any time. Setting one makes the engine seal every record with XChaCha20-Poly1305 under a random key, and the server then sees each as kind 201 under an HMAC of its kind and id. The keys travel in one more record, kind 200, wrapped by an Argon2id key from the passphrase; opening it is the check that a passphrase is right. The device that sets it first seals a copy of every plaintext record in the account, then sends the key record, and leaves the plaintext, which only "Delete Data on Server" removes. From then on a device without the passphrase uploads and applies nothing, and asks for it in a prompt that only entering it or signing out closes. Changing the passphrase makes a new key and seals everything again in the same slots; other devices take a new key only if it descends from theirs. Turning encryption off needs a sync reset, as in Chrome. The server needed no change. `design/sync-encryption.md` has the formats and states, and what a malicious server can still do: read any unencrypted account, see metadata, guess the passphrase offline, withhold or replay records, and keep what it copied before a change.

**Tracking protection blocks Vsesvit's own tracker list in both shells.** Settings > Privacy picks Off, Standard (the default: advertising and analytics trackers) or Strict (social media trackers too), a synced pref. The site-info popup turns it off for a site, stored as the site's Trackers setting with its other site permissions, so it syncs the same way. The list (`crates/vsesvit-core/src/trackers.json`) groups tracker domains by the company that runs them, and a company's trackers load on its own sites. It is written for Vsesvit under the MIT licence because the well-known lists are not: DuckDuckGo's, Disconnect's and Ghostery's are CC BY-NC-SA 4.0, and EasyPrivacy is GPL-3.0 or CC BY-SA 3.0. It ships with each release and is never fetched. On Linux core turns it into declarativeNetRequest rules, which the extension translator (`vsesvit-webext/src/dnr.rs`) compiles into a WebKit content blocker. On Windows the tab asks core about each request WebView2 reports for a listed domain and answers the tracker's with a 403. WebView2's own tracking prevention (None, Basic, Balanced, Strict) would map onto Off, Standard and Strict, but it has no per-site exceptions and reports nothing it blocked, so Vsesvit sets it to None and blocks the list itself, which also gives Windows a count of blocked trackers in the popup. WebKitGTK reports no content-blocker hits, so Linux shows no count.

**HTTPS-only follows Chrome's HTTPS-First mode, decided in core.** Settings > Privacy's "Always use secure connections" (a synced pref, off by default) makes each main-frame http navigation load its https URL instead, for public sites on port 80 as Chrome does: localhost, private and link-local addresses, single-label and local names (`.local`, `.internal`, `.home.arpa`, ...) and other ports load as asked. `vsesvit_core::https_only::Upgrades` follows each tab from the upgrade to the https load's end; when that fails (a network, TLS or certificate error) or redirects back to http on a host it already upgraded, the tab shows core's warning page at the http address, "This site doesn't support a secure connection", whose "Continue to site" stores Allow on the site's Insecure connections setting, synced with the other site permissions and listed in Settings' site list, where it can be removed. Chrome's exceptions lapse after 15 days; Vsesvit's stay until removed. Windows stops the navigation in `NavigationStarting` and loads the https URL, and shows the warning with `NavigateToString`, keeping it out of the History list. WebKitGTK's `decide-policy` asks about subframes too without saying which frame a navigation is for, so Linux upgrades there only what is surely the main frame's: what the browser started, or anything while the page on screen is https (where an http frame would be blocked mixed content), blank, or one of Vsesvit's own pages. A link from an http page is upgraded once its load starts, so that one http request may already be out, though its page never shows.

**Secure DNS is the engine's and the system's.** WebView2 runs DNS-over-HTTPS in automatic mode, upgrading to the system's DNS provider's DoH endpoint where the provider has one; Chromium reads the mode and the provider only from its local-state preferences and enterprise policy, which WebView2 does not let an app set, and no browser argument or feature switch reaches them, so Windows Settings describes this instead of offering a provider choice. WebKitGTK uses the system resolver, so Linux Settings says the system's network settings control DNS.

**Cookie controls are Chrome's, enforced where each engine can.** Settings > Privacy's "Third-party cookies" (a synced pref) is Allow, Block in private windows (the default, as in Chrome) or Block; `vsesvit_core::cookies::third_party_blocked` is the one question both shells and private windows ask, with `Browsing::Private` for the latter. A site's cookie rule is its Cookies and site data setting: Allow, Block or Clear on exit, synced and listed with the other site permissions and set from a section of the site-info popup. Private windows' tabs follow the stored rules, and their site info offers no rule of its own, as it offers no tracking protection exception: Linux would have to compile it into the blocker on disk. Block and Clear on exit sites have their data deleted when Vsesvit closes and again when it starts, in case it did not close cleanly. WebView2 has no cookie API beyond its cookie manager, so Windows works through each tab's DevTools sessions, the page's own and each frame's: `Network.setCookieControls` (which acts only with the Network domain on) blocks third-party cookies, set again as each navigation starts so a site set to Allow keeps its embedded sites' cookies; a blocked site's documents run core's script that empties `document.cookie`, and its cookies are deleted when the rule is set, after each page load and at startup, because WebView2 cannot keep a response from storing them; `Storage.clearDataForOrigin` clears sites, and Settings lists each site's cookies from `Network.getAllCookies`. Clearing at exit is best effort there, since the last tab's engine view closes right after; the clearing at the next start, before any page loads, is the one that is sure. WebKitGTK has one accept policy per network session, so Linux blocks third-party cookies for every site at once (no Allow exception, so Linux offers Allow only when it was synced from Windows); a blocked site gets a WebKit content blocker that keeps its requests from sending or storing cookies, plus the same script; and the website data manager clears sites and lists the data each keeps.

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

Profile directory, one per profile, next to the profile list (`profiles.json`, see Profiles):

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
| Site permissions | permission + origin | LWW per (origin, permission): allow or block; screen sharing remembers only block; picture-in-picture, trackers, insecure connections and cookies are never asked for (Allow on trackers turns tracking protection off for the site; Allow on insecure connections is HTTPS-only's exception, which has no Block; cookies also have Clear on exit) | setting `None` (ask; for picture-in-picture and trackers, block; for insecure connections, HTTPS-only applies; for cookies, the third-party cookies choice) |

`storage.sync` data from Windows extensions stays inside WebView2, which owns the extension runtime there, so it syncs only between Linux installs until a bridge exists.

## Tabs

Tabs are vertical by default, in a sidebar on the left. A setting moves the sidebar to the right, or switches back to a horizontal strip at the top. The setting is the synced preference `tabs.position` (`left` | `right` | `top`).

- **Linux.** An `AdwOverlaySplitView` holds a tab list bound to `AdwTabView`'s page model. Its `sidebar-position` places the list at the start or end, and it collapses to an overlay on narrow windows. The top layout uses `AdwTabBar`. Both show `AdwTabView`'s tab menu, set up per tab, with Chrome's items: new tab to the right (below in the sidebar), move to a new window, reload, duplicate, pin, mute, copy link (without tracking parameters), close, close others and close to the right (both leave pinned tabs open), and reopen closed tab. Pinned tabs are `AdwTabView`'s pinned pages, which lead the list and are saved in the session. A muted tab, or one playing sound, shows a speaker that mutes or unmutes it.
- **Windows.** A collapsible pane holds a reorderable `ListView` of tabs (favicon, title, close button), on either side of the web content. When collapsed, the pane shows only favicons. The top layout uses the `TabView` strip in the title bar. With vertical tabs, the title bar holds the toolbar instead. Both fill the same `MenuFlyout` tab menu as it opens, with Chrome's items: new tab to the right (below in the pane), split view, move to a new window, reload, duplicate, pin, mute, copy link, close, close others and close to the right, and reopen closed tab. WebView2's XAML control cannot move its engine to another window, so Move to a new window opens the tab's address there, pinned if the tab was, and Duplicate opens the address again; neither keeps the back/forward history, which WebView2 cannot restore.

Which tabs Close Other Tabs and Close Tabs to the Right close (never pinned ones), and when Move to a new window is on, is `vsesvit-core`'s `tab_place`, shared by both shells.

### Tab search

Ctrl+Shift+A, Chrome's key, opens a box over two lists: Open tabs, from every window, and Recently closed. Each row shows the tab's icon, its title and its site. Core's `tab_search::rows` narrows and orders the rows. With nothing typed, every tab is listed, the most recently used first. Typed text keeps the tabs whose title or address has it, ranked as the omnibox ranks bookmarks: title prefix, then title, then address. Open tabs always come before closed ones. "Most recently used" is a counter in the shell that ticks when a tab opens, is selected or its window comes to the front, and when a tab closes. A closed tab keeps its tick as its key in the closed-tab stack, so a row names the same tab however the stack changes. The first row is selected. Up and Down move the selection and wrap around at the ends. Enter or a click selects an open tab and brings its window forward, or reopens a closed one in the current window. Rows have no close button and no match highlighting, and closed tabs show no time.

- **Linux.** A `GtkPopover` holds a `GtkSearchEntry` over a `GtkListBox`, whose header function draws the two headings. It opens from a Search Tabs button at the top of the tab sidebar, or at the end of the `AdwTabBar` with tabs on top. A reopened tab keeps its back/forward history, as Ctrl+Shift+T does.
- **Windows.** A `Flyout` holds a `TextBox` over a `ListView`, with the headings as items that can't be selected. It opens from a search button next to the pane's collapse button, or after the new tab button on the top strip. The collapsed pane has no room for the button, so the shortcut opens the flyout under the toolbar there. A reopened tab opens its address again.

## Private windows

Ctrl+Shift+N, Chrome's key, opens a private window; so do New private window in the main menu, after New window, and Open link in private window in a link's context menu. A window is normal or private for its whole life (core's `private::Browsing`), and so is each of its tabs. As Chrome keeps one off-the-record profile, a process has one private session: it starts with the first private window and ends when the last one closes.

- **Engine.** Linux gives the session one `NetworkSession::new_ephemeral()`, set up as the profile's (TLS errors fail, favicons on) with its own download handler, and drops it at the end; views keep the default web context, so extension pages, view-source and spell checking work. Windows creates private tabs from the one environment with controller options `IsInPrivateModeEnabled`; WebView2 drops the InPrivate data when its last view closes, and the InPrivate profile gets its own setup.
- **Nothing written.** Every core write a tab causes goes through one shell function per kind of write, which leaves private tabs out: no history visits or titles (so nothing for the omnibox to learn), no favicons, nothing in the session file or in the open tabs sync publishes. A save with no normal window open writes nothing, so the last normal window's session stays to restore. The omnibox asks the search engine for no suggestions.
- **Kept in memory.** Core's `Profile` holds the private session's site choices (prompt answers, site-info changes, HTTPS-only exceptions), zoom and download rows, read over the stored ones and dropped by `end_private_session`. Notification requests are blocked without asking, as in Chrome. Site info offers no tracking protection switch and no cookie rule, in both shells: private tabs follow the stored ones, since Linux would have to compile a private exception into a content blocker on disk.
- **Apart from normal windows.** Private tabs have their own recently closed stack, and tab search, Ctrl+Shift+T and the omnibox's switch-to-tab rows see only the tabs of the window's kind. A tab moved to a new window or dragged out keeps its kind, and on Linux a tab dropped onto a window of the other kind goes back to one of its own. Command lines, extensions and sync open pages in a normal window.
- **Extensions.** Off in private windows, as Chrome's Allow in Incognito, which core keeps as a synced list. Linux shows a switch per extension in the Extensions dialog; an allowed extension's content scripts and rules run in private tabs and it sees them in `chrome.tabs` with `incognito: true`, while the others see none. WebView2 has no such switch, and adding an extension to the InPrivate profile may install it into the normal one, so Windows touches no extension there and hides extension buttons in private windows.
- **The new tab page** is core's `PRIVATE_PAGE`: what private browsing does and does not keep, with no search box and no most-visited tiles.

When the last private window closes, its running downloads are cancelled, its closed tabs and what core kept are forgotten, and the engine data goes. Until then, downloads started in a private window show in private windows only, in the downloads list and the downloads button: a private window lists them among the profile's, as Chrome's incognito downloads page does, and a normal window never sees them (core's `downloads::listed_in`).

## Memory Saver

Chrome's Memory Saver, on by default: a background tab left alone for the delay goes to sleep, and wakes when it is shown again. Settings > General > Memory has the switch and the choice of Moderate, Balanced or Maximum, which sleep tabs after 6, 4 or 2 hours, Chrome's delays. Both are local preferences (`performance.memory_saver`, `performance.memory_saver.mode`), as in Chrome: how much memory to free is a property of the device.

The policy is `vsesvit-core`'s `memory_saver`. Each shell keeps an idle clock per tab and sweeps its tabs every minute; the sweep says which tabs sleep. What keeps a tab awake restarts its clock, as Chrome's discard eligibility does: being on screen (selected, in a split view or in picture-in-picture), being pinned, playing sound or being the media player's tab, capturing, a site allowed to notify, a page outside the web (a new tab, an extension's page, a file), and a `window.opener` link to another tab. Before a tab sleeps, the shell runs core's script that compares the page's form fields with what it loaded, and a page with unsent input stays awake.

- **Linux.** A tab sleeps by ending its web process (`webkit_web_view_terminate_web_process`). WebKit keeps the back/forward list and the icon, and forgets the title, so the tab keeps that. Its icon shows faded in the tab list and on the tab bar. Selecting it reloads the page in a new web process at the same place in its history.
- **Windows.** A tab sleeps with WebView2's own sleeping tabs (`TrySuspendAsync`), which keeps the page and its state and resumes it when it is shown or navigates; its icon shows faded in both tab lists.

A sleeping tab keeps its address, title and history, so the session, tab search and the open tabs sync publishes list it as before.

## Page commands

Print, Developer tools, the JavaScript console and View page source are rows in core's shortcut table, with Chrome's keys. Like every other shortcut they can be reassigned. View page source opens `view-source:<page>` in a new tab next to the page, for http, https and file pages.

- **Windows.** While the page has focus, WebView2 handles Ctrl+P, F12, Ctrl+Shift+I and Ctrl+Shift+J itself. The menus and the window's accelerators call `ShowPrintUI` and `OpenDevToolsWindow`. WebView2 has no API that picks the console panel. WebView2 renders `view-source:` pages natively but reports the address of the page inside, so the tab keeps the `view-source:` address the shell loaded.
- **Linux.** Print uses WebKit's print dialog, and Developer tools toggles WebKit's inspector. WebKitGTK has no view-source, so the shell serves the `view-source` scheme itself. It renders the page's main resource with core's `view_source::source_page`. That resource comes from a tab already showing the page, or else from a hidden view with scripts off. The scheme is registered as local, so web pages can neither open nor embed it, and each tab's navigation gate refuses it to every load the browser did not start, which keeps local `file://` pages out too, as in Chrome.


## Spell check

Spell checking is on by default, as in Chrome, and underlines misspelled words in a page's text fields. The context menu on a misspelled word lists the engine's corrections and Add to Dictionary, which adds the word to the system's personal dictionary; Vsesvit keeps no dictionary of its own. Whether it is on (`spellcheck.enabled`) and its languages (`spellcheck.languages`) are synced settings.

- **Linux.** WebKitGTK checks spelling through Enchant. Settings > General > Spell Check lists the Hunspell dictionaries installed where Enchant looks (`~/.config/enchant/hunspell`, `/usr/share/hunspell`, `/usr/share/myspell`), each with a switch and named as Chrome names languages. Until the user picks some, it checks the dictionaries for the system's languages (`LANGUAGE`, then `LC_ALL`, `LC_MESSAGES` or `LANG`): the locale's own, else its language's (`fr` for `fr_CA`), else the one for the region the language most likely means (`en_US` for `en`). A choice synced from another device keeps the languages this one has no dictionary for and checks the rest. Core's `spellcheck::Dictionaries` makes these choices. With no language left, checking is off, because WebKit given none checks the system's. WebKit's Learn Spelling item is renamed Add to Dictionary, Chrome's name for it.
- **Windows.** WebView2 checks spelling itself and has no API to turn it off or choose its languages, so Settings has no row for it. The synced settings stay for the user's Linux devices.

## Search engines

Settings > Search lists every engine with its name, shortcut and URL, as Chrome's "Manage search engines" does. You can add an engine, edit one, make one the default or remove one. Built-in engines can be removed too, but the default can't. The editor asks for a name, a shortcut and a URL with `%s` where the search terms go. Core checks the form (`SearchEngines::check`): every field is filled in, the shortcut is one word that no other engine has, and the URL is an http or https address. A URL typed without a scheme gets `https://`. Engines sync as part of Settings. The address bar reads the engine list each time it classifies what was typed, so a new shortcut works at once (`fx rust` searches the engine whose shortcut is `fx`).

- **Linux.** The Search page has the default engine's row and a Manage Search Engines row, which opens a subpage listing the engines. Each engine row has a menu with Make Default, Edit… and Remove. The editor is an `AdwAlertDialog` with three entry rows.
- **Windows.** The Search page lists the engines under the default engine's box. Each row has a More actions menu with Make default, Edit and Delete. The editor is a flyout, because a second dialog can't open over Settings.

### Search suggestions

As the user types a search, the address bar asks the default engine for suggestions at its `suggest_url` and reads the OpenSearch JSON answer, as Chrome does. Core does the whole exchange (`search::Omnibox::suggest_request`, `suggest::SuggestRequest::run`): it waits 100 ms after a keystroke, sends nothing if the user typed again meanwhile, and drops an answer that arrives after a newer keystroke. It fetches on a worker thread with a 5 s limit. Like current desktop Chrome, the list keeps its default match first and puts the search rows (the typed search and up to four suggestions) above the URL rows from history and bookmarks. The rows join the open list when the answer arrives, and the highlighted row and the text in the box stay as the user has them. `suggest_request` decides what may leave the device. It sends nothing when Settings > Search > Search suggestions is off (a synced setting, on by default), when the default engine has no suggestion URL, or in a private window. It also sends nothing unless the text is a plain search of the default engine, so a URL being typed, a file path, a `file:` URL or a shortcut search of another engine never leaves the address bar. Engines added in the editor have no suggestion URL, as in Chrome, so only the built-in engines suggest.

## Bookmark export

Export bookmarks writes every bookmark to a bookmarks HTML file, the Netscape format every browser imports. The file is laid out as Chrome writes it: the bookmarks bar as the toolbar folder, then the items of Other bookmarks, then Mobile bookmarks as a folder if it has any. Core's `export::html` writes it, and the importer reads it back as the same tree. The save dialog opens in Documents with Chrome's name for the day, such as `bookmarks_10_6_26.html`. The file has no favicons.

- **Linux.** The Bookmarks window's main menu holds Import Bookmarks… and Export Bookmarks…, like the menu in Chrome's bookmark manager.
- **Windows.** The Bookmarks dialog has an Export bookmarks… button under Import.

## Downloads

Core keeps the downloads list (`vsesvit_core::downloads`, local to the device) and the states both shells show: in progress, paused, interrupted (a network or server error the engine can resume from), unconfirmed, completed, failed and cancelled. A download the engine still held when the browser exited reads as failed at the next start.

A file of a type that runs code when opened is held back, as in Chrome. `downloads::is_dangerous` decides by extension and MIME type from a table modelled on Chrome's dangerous download types, one set for Windows (`.exe`, `.msi`, `.bat`, `.ps1`, `.lnk`, ...) and one for Linux (`.sh`, `.deb`, `.rpm`, `.desktop`, `.AppImage`, ...), with `.jar` on both. The shell has the engine write such a file beside its destination under its unconfirmed name (`setup.exe.unconfirmed`), which no program opens, and when it is complete the entry waits as unconfirmed. The window it came from shows Chrome's warning under the downloads button, with Keep and Discard, and the Downloads list offers both too. Keep moves the file to its name (numbered if a file took that name meanwhile) and Discard deletes it and its entry. An unconfirmed file still waits after a restart.

- **Windows.** The Downloads dialog pauses and resumes a download (`CoreWebView2DownloadOperation.Pause` and `Resume`), and resumes one an error interrupted when WebView2 says it can. Every finished file gets the Mark of the Web, a `Zone.Identifier` stream naming the Internet zone and the source address as Chrome writes it, unless WebView2 already wrote one; a kept file takes it along, so SmartScreen checks the file when it is opened. WebView2's own SmartScreen check of downloads stays on.
- **Linux.** WebKitGTK has no way to pause a download, so nothing offers to, and a failed download cannot be resumed.

## Profiles

A profile is a directory of its own, with its own bookmarks, history, settings, extensions, sync sign-in and passphrase, and one process runs it: `Profile::open` locks the directory, and a second launch on the same profile hands its command line to the process that has it. The profiles of an install sit side by side in `profiles/`, and core's `profiles` keeps their list in `profiles/profiles.json`: each profile's directory name (`Default`, `Profile 1`, ...), the name and avatar colour the user gave it, the profile used last, Chrome's "Show on startup" switch for the picker, and removed profiles whose data is not deleted yet. Every profile's process reads and writes the file, holding the lock on `profiles/profiles.lock` from reading it to replacing it, so no change is lost; nothing else is shared between them. The process that opens a profile lists it if it is not listed yet ("Person 1", "Person 2", as Chrome names them) and marks it last used, which it also does whenever one of its windows comes forward, when it reads the list again for changes made in other processes.

- **Profile button.** The toolbar shows the profile's avatar, a circle in its colour with its initial. Its menu lists every profile with the current one checked, then Add profile and Manage profiles. A private window has no profile button, as Chrome's incognito windows have none. Choosing another profile starts `vsesvit --profile-dir <its directory>`: if that profile already runs, the new process finds it locked and forwards its empty command line, which brings the running profile's window forward. On Linux the process is started through the display's launch context, so on Wayland it carries an activation token for the focus; on Windows the new process gets the right to take the foreground and passes it on.
- **Add profile** asks for a name and a colour and opens the new profile in a window of its own, as Chrome does. **Manage profiles** lists the profiles to open, rename and recolour, or remove. Removing one deletes its data on this device once no process has it open; its windows close, as the process that has it is told through a forwarded command line, and the last profile cannot be removed. What it synced stays on the sync server.
- **Startup.** A launch that names no profile opens the profile used last. With more than one profile, the picker switched on (the default, as in Chrome) and no profile running, it shows the picker instead ("Who's using Vsesvit?"), which starts the chosen profile and closes. A launch with addresses, such as a link from another app, skips the picker. The switch is in Settings > General and in the picker.
- **Titles.** With more than one profile, window titles add the profile's name: "Page - Work" on Linux, "Page - Work - Vsesvit" on Windows. A private window in a profile names both: "Page - Work (Private)" and "Page - Work - Vsesvit (Private)".
- A `--profile-dir` outside `profiles/` is a profile of its own outside the list, with no profile button. The self-test keeps its list in its output directory.

## Extensions in detail

Install sources are Chrome Web Store URLs or ids, AMO add-on URLs or gecko ids (a bare slug is not accepted, because it cannot be told apart from a relative path), local `.crx`/`.xpi` files, and unpacked developer directories. Each install records how it was verified.

Extensions from a store update themselves, as in Chrome. Each device asks the stores a minute after startup when its last check is five hours old, and then every five hours (`extensions::update`): the Chrome Web Store through its gupdate XML check, Edge Add-ons through an Omaha 3.1 check, and AMO through its add-on API. A newer version is downloaded and verified exactly as an install is, and keeps the extension's id and storage. Linux reloads it in the runtime, which fires `runtime.onInstalled` with `update`; Windows loads its folder into WebView2, which replaces the old version in place. The Extensions dialog's Update button checks at once. A version that Chrome would warn about anew (`extensions::permissions::update_warnings`, measured against what the user approved and the permissions they granted it since) is installed but stays off on that device, with Chrome's `Enable “X”?` and the new warnings under its row and a Re-enable button, until the user re-enables it, which grants what it added. Local and unpacked installs have no store and are not checked.

Extensions' keyboard shortcuts come from the manifest's `commands`, with Chrome's rules for suggested keys: Ctrl or Alt but not both, at most four per extension. Core resolves them against the browser's shortcuts (`extensions::commands`). The browser's own shortcuts always win. The user's changes come next, then suggested keys in install order, so the first extension installed keeps a contested key. The user's changes are stored with the browser's shortcut overrides in the synced `keyboard.shortcuts` register. Settings > Shortcuts lists them under Extension shortcuts, where they can be changed or reset, and Reset All resets them too.

**Install prompt (both shells).** A user's install from a store or a package waits, once the package is verified, for Chrome's install prompt: `Add "<name>"?`, then "It can:" and the permission warnings. Core words them (`extensions::permissions`) with Chrome's message rules, coalesced as Chrome coalesces them, so `<all_urls>` with `tabs` reads as "Read and change all your data on all websites" alone; a content script's sites count too. The GTK shell shows an Adwaita alert, with what the Linux runtime lacks. The WinUI shell shows the prompt inside the Extensions dialog, since no dialog may open over it, and as a dialog over a store page. Loading an unpacked folder and installing what another device asked for do not ask, as in Chrome.

**Windows.** WebView2 runs extensions natively: MV3 service workers, content scripts, `chrome.storage`, `chrome.tabs` inside extension pages, and declarativeNetRequest. WebView2 has no browser chrome for extensions, so Vsesvit draws the toolbar action buttons and shows each popup page in a flyout. `chrome.tabs` in WebView2 does not know about Vsesvit's tabs (WebView2Feedback #3853 and #3854). WebView2 puts extensions' `contextMenus` items in the page's context menu itself. It offers no way to read an extension's items for its action, so the toolbar button's menu has none. WebView2 does not run extensions' keyboard shortcuts (`commands`) and offers no way to fire `commands.onCommand`, so Vsesvit binds only the action shortcuts (`_execute_action` and its MV2 names), which open the popup as the toolbar button does.

**Linux.** `vsesvit-webext` implements the WebExtensions subset below on WebKitGTK:

- A `chrome-extension://` URI scheme serves extension files. Only `web_accessible_resources` are visible to web pages.
- The background runs as a hidden web view. MV3 service workers are emulated as a page.
- Content scripts run in a per-extension isolated world: the manifest's, and those an extension registers with `scripting.registerContentScripts`. As in Chrome, a registered script runs only where the extension has host permissions, and one registered to persist across sessions lasts until the extension is updated. Content-script CSS is a user-level style sheet, because WebKitGTK ignores author-level ones on standards-mode pages, so a page's own rules beat it unless it is `!important`.
- `chrome.*` and `browser.*` are available with both promises and callbacks. The covered APIs are runtime messaging and ports (`runtime.connect`, `tabs.connect`, messages and connections from other extensions as `externally_connectable` allows), storage (local, sync, session, onChanged), i18n, tabs, scripting, action/browserAction, alarms, context menus (`contextMenus`, and Firefox's `menus`), commands (`getAll`, `onCommand`), notifications, permissions (`getAll`, `contains`, `request`, `remove`, `onAdded`, `onRemoved`), extension.getURL and `runtime.getBackgroundPage` (in the background page and its popups).
- `chrome.windows` reports the browser's own windows and opens, changes and closes them, with its events. `tabs.move` takes tabs within a window or to another one. A tab's window id, the current window and the tab events (created, moved, detached, attached, removed) hold across windows. A `popup` or `panel` window opens as a normal browser window. Private windows exist for an extension only once the user allowed it in them, as in Chrome: until then it neither lists them nor hears their events, and focus going to one reaches it as `WINDOW_ID_NONE`. It may still open one with `incognito: true`, though not on its own pages, and gets no window back. No tab moves between a normal and a private window. Wayland does not tell a window where it is on screen, so every window reports itself at the origin, and moving one is ignored.
- `chrome.webNavigation` fires Chrome's events for the loads of every frame in a tab, in Chrome's order and with its URL filters, and `getFrame`/`getAllFrames` list a tab's frames. WebKit reports loads for the top frame only, so a script in its own world in every frame reports the rest. A subframe's `onBeforeNavigate` therefore comes when its document arrives, and a subframe whose load fails reports nothing. Frame ids name frames in these events only: messages, ports and `scripting` still reach a tab's top frame alone.
- `chrome.cookies` reads and changes the browser's cookie store with Chrome's defaults, errors and host permissions, and `onChanged` reports every change on the hosts an extension may reach, a page's own cookies included. WebKit says only that the store changed, so the runtime reads it again and fires the difference: an overwrite is a removal and an addition, as in Chrome, and a cookie gone is `explicit` or `expired` but never `evicted`. An extension cannot set a cookie for a site the user set to block cookies; Chrome answers that as any cookie it cannot store. The normal windows' store is `"0"`. Private windows' store, `"1"`, is there only for an extension the user allowed in private windows, while one is open: other extensions cannot name it, `getAllCookieStores` leaves it out, and only an allowed extension hears of its changes through `onChanged`, as store `"1"`. WebKitGTK keeps no partitioned cookies, so a `partitionKey` naming a site reaches none, and `SameSite` "unspecified" is stored as `lax`.
- Extensions' context menu items show in the page's context menu, by what was clicked and in which frame, grouped under the extension's name when it has several, and those for the action show in its toolbar button's menu. A service worker's items are kept across restarts, as in Chrome.
- Extensions' keyboard shortcuts (`commands`) work while a Vsesvit window has focus, `"global": true` ones included. A named command fires `commands.onCommand` with the selected tab and grants `activeTab` on it. An action command opens the popup, or clicks the action when it has none.
- Extensions' notifications (`chrome.notifications`) show through GNotification, which reaches the desktop portal inside a Flatpak. They are laid out as Chrome lays them out on Linux: a progress notification's title starts with its percentage, a list's items follow the message, and an image notification shows no image. Each one has a Settings button that opens the Extensions dialog on its extension. There a Notifications switch turns that extension's notifications off, like Chrome's per-extension switch; the choice syncs (`extensions.notifications_off`). GNotification does not say when the user dismisses a notification, so it stays in `notifications.getAll` until the extension clears or replaces it, as with Chrome on the portal.
- declarativeNetRequest rules are translated to WebKit content-blocker rules: the enabled static rulesets with the extension's dynamic and session rules, rebuilt off the UI thread when they change, so an allow rule an extension adds (uBlock Origin Lite's per-site switch is one) lifts its static blocks. Rules WebKit cannot express are skipped and logged. WebKit reports nothing a blocker matched, so there is no blocked count on the badge.
- Optional permissions work as in Chrome. `permissions.request`, during a user gesture and for permissions the manifest lists, asks the user with Chrome's prompt and its warnings unless the extension holds them already or they have no warning. The prompt shows over the window the request came from, or else over the last focused window the extension may know, so never over a private window it does not run in. Core keeps the grants (a local preference of the profile, not synced, as in Chrome) until `permissions.remove` takes them back or the extension is uninstalled. Granted sites reach the extension's registered content scripts, declarativeNetRequest rules, cookie calls and events, tabs and cross-origin fetches at once. WebKit cannot take back a cross-origin exception, so a site removed again stays fetchable from the extension's pages until their web process ends.
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
