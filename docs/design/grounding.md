# Vsesvit design grounding

Vsesvit is a new web browser written in Rust. The repository is empty apart from this file. This document is the shared input for design candidates.

## Product requirements (from the owner)

- Runs on Windows 11 and Linux.
- Windows shell: WinUI 3 (Windows App SDK, `Microsoft.UI.Xaml`) with the WebView2 control.
- Linux shell: GTK4 + libadwaita with WebKitGTK 6.0 (`webkitgtk-6.0`). Chromium/CEF was rejected on Linux because it cannot embed natively in GTK4.
- Written in Rust.
- Extension support. Installing from the Chrome Web Store is preferred; Firefox AMO add-ons are an acceptable fallback. Unpacked developer extensions should also load.
- Bookmarks, installed extensions and other commonly synced things must be stored in a format that lets a sync engine be added later without a data migration or redesign. Sync itself (server, protocol, accounts) is out of scope now.

## Environment facts

- Rust 1.98 stable, edition 2024. Windows host is `x86_64-pc-windows-msvc`. Linux is WSL2 Ubuntu 26.04 (GTK 4.22, libadwaita 1.9, WebKitGTK 2.52). Both run `cargo test`.
- Crate versions available today: rusqlite 0.40 (has a `bundled` feature), ureq 3.4, zip 9.0-pre, rsa 0.10-rc, sha2 0.11, uuid 1.26, serde_json 1.0, thiserror 2, url 2.5, directories 6, base64 0.23, proptest 1.11, gtk4 0.11, libadwaita 0.9, webkit6 0.6, windows 0.62 / windows-core 0.100.
- WebView2 can load an unpacked Chromium extension folder with `CoreWebView2Profile.AddBrowserExtensionAsync(path)` once `AreBrowserExtensionsEnabled` is set on the environment. It then owns that extension's runtime (background, content scripts, chrome.* APIs, chrome.storage).
- WebKitGTK has no browser-extension runtime. The Linux shell will implement a subset of the WebExtensions API itself (content scripts via user scripts in an isolated script world, a hidden background web view, runtime messaging, `storage`, action popups, and declarativeNetRequest translated to WebKit content-blocker rules). Epiphany (GNOME Web) takes the same approach.
- The extension id of a Chrome Web Store extension is derived from its public key (first 16 bytes of SHA-256 of the DER public key, hex digits mapped to `a`..`p`). An unpacked copy keeps the store id only if the manifest carries the `key` field, so the installer writes the CRX public key into `manifest.json` when unpacking.
- Chrome Web Store download: `https://clients2.google.com/service/update2/crx?response=redirect&prodversion=<chrome version>&acceptformat=crx2,crx3&x=id%3D<id>%26uc` returns a CRX3 file (magic `Cr24`, version 3, header length, protobuf `CrxFileHeader`, then a zip).

## Proposed workspace (candidates may change it with reasons)

```
crates/vsesvit-core    platform-agnostic library: data model, profile store, sync metadata,
                       omnibox parsing, extension install pipeline (CRX/XPI/unpacked), manifest model
crates/vsesvit-gtk     Linux shell, compiles to nothing on Windows
crates/vsesvit-winui   Windows shell, compiles to nothing on Linux
crates/vsesvit         the binary; picks the shell for the target OS
```

## What the core must serve

Both shells are thin. They own the engine views (WebView2 / WebKitWebView) and the widgets, and call into core for everything that is policy or data. Dominant access patterns:

1. On every committed navigation: record a history visit; look up whether the URL is bookmarked (star state).
2. Omnibox: turn typed text into a navigation target (URL, or a search on the default search engine); suggest from bookmarks and history.
3. Bookmark CRUD from the star button and a bookmarks bar/menu: add, rename, move (between folders and reorder), delete; list children of a folder in order.
4. Extensions: install from a store URL or id (download, verify, unpack, register), list installed, enable/disable, uninstall, and hand each shell the on-disk directory plus the parsed manifest. Re-running an install of the same version must converge to the same state. The Linux runtime also needs `chrome.storage.local` and `chrome.storage.sync` backing; `storage.sync` should be synced data.
5. Settings/preferences: typed key/value (default search engine, homepage, theme, etc.).
6. Session: the open windows and tabs, restored on startup, and published later as "tabs on this device" for sync.
7. A later sync engine must be able to: enumerate local changes since its last sync, apply a batch of remote changes idempotently and in any order, and converge with other devices. Deletions must propagate. Bookmark trees edited concurrently on two devices (moves, reorders, deletes of a folder while a child is added elsewhere) must converge to a valid tree (no cycles, no orphans).

Data kinds that are commonly synced by browsers: bookmarks, history, open tabs (per device), extensions (installed set + enabled state), extension settings (`storage.sync`), preferences, search engines, passwords, autofill, reading list. Passwords and autofill need an OS secret store and are deferred; the format should leave room for them.

## Constraints

- Single process per profile. Only one browser process opens a profile at a time (enforce with a lock).
- The shells call core from their UI thread. Network downloads (extension install) must not block the UI thread.
- Data must survive a crash mid-write.
- Keep the design small. No sync server, no plugin systems, no speculative abstraction layers.
