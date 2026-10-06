<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="packaging/icons/DarkBG.svg">
    <img src="packaging/icons/WhiteBG.svg" alt="Vsesvit logo" width="160">
  </picture>
</p>

<h1 align="center">Vsesvit</h1>

<p align="center">A native web browser for Windows 11 and Linux, written in Rust, that installs Chrome Web Store extensions and syncs through a server you can host yourself.</p>

Vsesvit gives each platform a shell built with its own toolkit around the platform's own web engine, on top of one shared Rust core. On Windows it is a WinUI 3 app around WebView2. On Linux it is a GTK4/libadwaita app around WebKitGTK.

## Features

- **Chrome Web Store extensions on both platforms.** Install from the Chrome Web Store, Microsoft Edge Add-ons or Firefox's AMO, or from a local `.crx`/`.xpi` file or an unpacked directory. Every CRX3 signature is verified before anything is unpacked. WebView2 runs the extensions on Windows, and Vsesvit's own WebExtensions runtime (`vsesvit-webext`) runs them on Linux. An extension's own items show in the page's context menu, and on Linux also in its toolbar button's menu. Extensions' keyboard shortcuts work as in Chrome, where the browser's own shortcuts win, and can be changed in Settings. On Windows only the shortcut that opens an extension's popup works, because WebView2 gives an extension's other shortcuts no way to reach it.
- **Vertical tabs.** A sidebar on the left by default. You can move it to the right or go back to a strip along the top. Right-click a tab for Chrome's tab menu: a new tab beside it, move it to a new window, reload, duplicate, pin or mute it, copy its link, close the others or the ones after it, and reopen a closed tab. Pinned tabs stay first and come back pinned. Ctrl+Shift+C copies the page's address without its tracking parameters, and a link's context menu can copy the link that way too.
- **Chrome's page commands and keys.** Print (Ctrl+P), Developer tools (F12, Ctrl+Shift+I), the JavaScript console (Ctrl+Shift+J) and View page source (Ctrl+U) are in the main menu and the page's context menu. Every shortcut can be reassigned in Settings.
- **Search engines with shortcuts.** Add, edit and remove search engines in Settings, as in Chrome. Typing an engine's shortcut and a space in the address bar searches that engine. As you type a search, the address bar lists the default engine's suggestions above your history and bookmarks, unless you turn them off in Settings.
- **Sync you can host.** Bookmarks, history, open tabs, extensions, settings, search engines and site permissions sync through [a small server](server/README.md) (Docker image, SQLite or Postgres) that signs people in with the OpenID Connect provider you choose. The server stores records and never reads or merges them.
- **Tracking protection.** Known advertising and analytics trackers are blocked on other sites by default; Strict also blocks social media trackers such as Like buttons. Turn it off for a site from the site-info popup, which on Windows also counts the trackers blocked on the page. The tracker list is Vsesvit's own, MIT-licensed like the rest of the code, and ships with each release.
- **Always use secure connections.** Turn it on in Settings > Privacy, as in Chrome, and http addresses load over https. A site that doesn't support it shows a warning first, and "Continue to site" remembers your choice for that site, synced with your other site settings. Secure DNS follows the system: on Windows WebView2 uses it whenever your DNS provider supports it, and on Linux your network settings decide.
- **Cookie controls.** Settings > Privacy blocks third-party cookies everywhere, only in private windows (the default) or nowhere. From the site-info popup a site's cookies can be allowed, blocked or cleared when Vsesvit closes, synced with your other site settings, and Settings lists the sites that keep cookies (on Linux, any site data) with a way to delete them. On Windows a site set to Allow also gets its embedded sites' cookies when they are blocked elsewhere.
- **Passwords stay in your password manager.** Vsesvit doesn't save passwords. Use a password manager such as Bitwarden or Proton Pass through its extension; the welcome screen offers both.
- **Sync-ready from the start.** All data lives in one SQLite file per profile, in a format that merges the same way whatever order changes arrive in. Secrets are sealed with DPAPI on Windows and the Secret Service on Linux.
- **Packaged for each system.** An NSIS installer on Windows. deb, rpm, pacman, AppImage and Flatpak on Linux. Everything except Flatpak updates itself ([docs/design/packaging.md](docs/design/packaging.md)).

## Install

Builds are published on [GitHub Releases](https://github.com/mrquantumoff/vsesvit/releases), on four channels: stable, beta, weekly and nightly. Linux needs Ubuntu 26.04 or newer, or an equivalent with GTK 4.22, libadwaita 1.9 and WebKitGTK 2.52. To build from source, see below.

The plan, the decisions and their evidence are in [docs/PLAN.md](docs/PLAN.md).

## Build from source

### Linux

The build needs GTK 4.22, libadwaita 1.9, WebKitGTK 2.52 and GLib 2.80 or newer. Ubuntu 26.04 ships these. On older releases the build fails in a `-sys` crate with a pkg-config version error. Install the build dependencies (Debian/Ubuntu names):

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev
```

For audio and video playback, also install `gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav`.

Then run:

```bash
cargo run -p vsesvit
```

From a Windows checkout you can build and run the Linux version inside WSL. Build output goes to the WSL filesystem:

```bash
bash scripts/wsl.sh run -p vsesvit
```

### Windows 11

You need:

- the Rust MSVC toolchain
- Visual Studio Build Tools with the C++ workload
- the Windows App Runtime 2.5.1 or newer (x64)
- the WebView2 runtime, which ships with Windows 11

The build downloads `Microsoft.Web.WebView2.Core.dll` from NuGet and checks it against a pinned SHA-256.

```bash
cargo run -p vsesvit
```

## Test

```bash
cargo test --workspace
```

`vsesvit --self-test <dir>` runs the end-to-end check described in [docs/design/self-test.md](docs/design/self-test.md). It uses a fresh profile, writes `report.json` and a screenshot to `<dir>`, and exits non-zero if any check fails. Add `--network` to also install uBlock Origin Lite from the real Chrome Web Store.

## Layout

```
crates/vsesvit         binary; picks the shell for the target OS
crates/vsesvit-core    profile store, sync-ready data model, omnibox, extension install pipeline
crates/vsesvit-winui   Windows shell (WinUI 3 + WebView2)
crates/vsesvit-gtk     Linux shell (GTK4 + libadwaita + WebKitGTK)
crates/vsesvit-webext  WebExtensions runtime for WebKitGTK
crates/vsesvit-sync    sync engine: OpenID Connect sign-in, rounds between a profile and a server
server/                self-hostable sync server (Docker image, SQLite or Postgres); see server/README.md
docs/                  plan, design documents, research notes, design arena record
```

## License

MIT. See [LICENSE](LICENSE).
