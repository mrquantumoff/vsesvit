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

## Architecture

```
crates/
  vsesvit          the binary: picks the shell for the target OS
  vsesvit-core     platform-agnostic: profile store (SQLite), sync-ready records, omnibox,
                   extension install pipeline (CRX3/XPI/unpacked, manifest model), testkit
  vsesvit-winui    Windows shell: WinUI 3 + WebView2 (compiles to nothing elsewhere)
  vsesvit-gtk      Linux shell: GTK4 + libadwaita + WebKitGTK (compiles to nothing elsewhere)
  vsesvit-webext   Linux WebExtensions runtime on WebKitGTK; its DNR translator is platform-neutral
```

Threading model:

- The `Profile` lives on the UI thread and is `!Send`.
- Every call a shell makes on the UI thread is a local SQLite transaction or an in-memory lookup. Bookmarks are held in memory.
- Slow work is a `Send` value with no database handle: an extension download and verify (`InstallJob::run`) today, and a sync engine's network I/O later. It runs on a worker thread and its result is committed back on the UI thread.
- Nothing is shared between threads, so there are no locks.

Profile directory, one per profile:

```
<data dir>/Vsesvit/profiles/<name>/
  LOCK                                 OS file lock; a second process gets "profile in use"
  vsesvit.db (+ -wal, -shm)            all synced and local records
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
| Site permissions | permission + origin | LWW per (origin, permission): allow or block; screen sharing remembers only block | setting `None` (ask) |
| Passwords, autofill | reserved (kinds 10, 11) | same conventions; secret columns hold ciphertext sealed by DPAPI / libsecret | tombstone |

`storage.sync` data from Windows extensions stays inside WebView2, which owns the extension runtime there, so it syncs only between Linux installs until a bridge exists.

## Tabs

Tabs are vertical by default, in a sidebar on the left. A setting moves the sidebar to the right, or switches back to a horizontal strip at the top. The setting is the synced preference `tabs.position` (`left` | `right` | `top`).

- **Linux.** An `AdwOverlaySplitView` holds a tab list bound to `AdwTabView`'s page model. Its `sidebar-position` places the list at the start or end, and it collapses to an overlay on narrow windows. The top layout uses `AdwTabBar`.
- **Windows.** A collapsible pane holds a reorderable `ListView` of tabs (favicon, title, close button), on either side of the web content. When collapsed, the pane shows only favicons. The top layout uses the `TabView` strip in the title bar. With vertical tabs, the title bar holds the toolbar instead.

## Extensions in detail

Install sources are Chrome Web Store URLs or ids, AMO add-on URLs or gecko ids (a bare slug is not accepted, because it cannot be told apart from a relative path), local `.crx`/`.xpi` files, and unpacked developer directories. Each install records how it was verified.

**Windows.** WebView2 runs extensions natively: MV3 service workers, content scripts, `chrome.storage`, `chrome.tabs` inside extension pages, and declarativeNetRequest. WebView2 has no browser chrome for extensions, so Vsesvit draws the toolbar action buttons and shows each popup page in a flyout. `chrome.tabs` in WebView2 does not know about Vsesvit's tabs (WebView2Feedback #3853 and #3854).

**Linux.** `vsesvit-webext` implements the WebExtensions subset below on WebKitGTK:

- A `chrome-extension://` URI scheme serves extension files. Only `web_accessible_resources` are visible to web pages.
- The background runs as a hidden web view. MV3 service workers are emulated as a page.
- Content scripts run in a per-extension isolated world.
- `chrome.*` and `browser.*` are available with both promises and callbacks. The covered APIs are runtime messaging, storage (local, sync, onChanged), i18n, tabs, scripting, action/browserAction, alarms, permissions.contains and extension.getURL.
- declarativeNetRequest static rules are translated to WebKit content-blocker rules. Rules WebKit cannot express are skipped and logged.
- Not supported: `webRequest` blocking, native messaging, devtools pages and some other surfaces. Heavily API-dependent extensions may therefore work partially. The extensions page shows which APIs an extension requests that the runtime lacks.

## Verification

- **Core.** Lattice-law and tree-validity property tests, the three-device convergence test, CRX3 round-trip and tamper tests, and manifest normalization tests.
- **Each shell.** `vsesvit --self-test <dir>` runs the same scripted end-to-end check on each platform (`design/self-test.md`). It covers installing a signed test CRX, a content script round trip to the background, a declarativeNetRequest block observed at the fixture server, the action popup, bookmarks, tabs, the omnibox, session save and restore, and an in-app screenshot. It writes `report.json` and exits non-zero on any failure.

## Milestones

1. **Spikes (done).** Pure-Rust WinUI 3 + WebView2 + extensions. WebKitGTK isolated-world messaging.
2. **Core.** Data model, sync model and convergence tests, then the install pipeline and testkit.
3. **Shells.** GTK/libadwaita and WinUI UIs: vertical tabs (left by default, right, or top), navigation, omnibox, bookmarks bar, dialogs.
4. **Extensions end to end.** WebView2 loading and action popups on Windows, and the `vsesvit-webext` runtime on Linux.
5. **Self-tests green on both platforms.**
6. **Packaging and updates.** An NSIS installer on Windows. deb, rpm, pacman, AppImage and Flatpak on Linux. Every format but Flatpak updates itself through the Tauri updater protocol, so an existing Tauri update server serves Vsesvit (`design/packaging.md`).
7. **Later.**
   - Sync engine: an end-to-end encrypted record store and a server. Nothing in core's format changes.
   - Passwords and autofill: secret-store integration.
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
