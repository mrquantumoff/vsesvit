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

- **Chrome Web Store extensions on both platforms.** Install from the Chrome Web Store, Microsoft Edge Add-ons or Firefox's AMO, or from a local `.crx`/`.xpi` file or an unpacked directory. Every CRX3 signature is verified before anything is unpacked. Before an extension from a store or a file goes in, Vsesvit lists what it can do in Chrome's words ("Read and change all your data on all websites"), as Chrome's install prompt does, and on Linux also what the runtime lacks. Store extensions update themselves every few hours, or at once from the Extensions dialog's Update button, and an update that can do more than you allowed stays off until you re-enable it, as in Chrome. WebView2 runs the extensions on Windows, and Vsesvit's own WebExtensions runtime (`vsesvit-webext`) runs them on Linux. An extension's own items show in the page's context menu, and on Linux also in its toolbar button's menu. Extensions' keyboard shortcuts work as in Chrome, where the browser's own shortcuts win, and can be changed in Settings. On Windows only the shortcut that opens an extension's popup works, because WebView2 gives an extension's other shortcuts no way to reach it. On Linux an extension's notifications show as desktop notifications, and the Extensions dialog has a switch that turns each extension's off. Linux extensions also get declarativeNetRequest's dynamic and session rules, `storage.session` and content scripts registered at run time, which uBlock Origin Lite's per-site switch and cosmetic filters use. They see the browser's real windows through `chrome.windows`, and can move tabs between them. They can follow every frame's navigations through `chrome.webNavigation`, and read and change cookies through `chrome.cookies`, except for sites you set to block cookies. They can ask for their optional permissions with `chrome.permissions.request`, which shows Chrome's prompt and keeps what you allow until the extension gives it back or is removed.
- **Vertical tabs.** A sidebar on the left by default. You can move it to the right or go back to a strip along the top. Right-click a tab for Chrome's tab menu: a new tab beside it, move it to a new window, reload, duplicate, pin or mute it, copy its link, close the others or the ones after it, and reopen a closed tab. Pinned tabs stay first and come back pinned. Ctrl+Shift+C copies the page's address without its tracking parameters, and a link's context menu can copy the link that way too. Ctrl+Shift+A, or the search button at the top of the tab list, searches the open tabs of every window and the recently closed ones by title or address; Enter or a click goes to the tab, or reopens a closed one.
- **Tab groups.** As in Chrome, a tab's menu adds it to a new group or one the window already has, or takes it out. A group has a name and one of Chrome's nine colours, which its header opens to change, and its tabs carry its colour. A click on the header collapses or expands the group in the vertical tab list; the header also opens a new tab in the group, ungroups it or closes its tabs. Dragging a tab into the middle of a group adds it, and dragging it away takes it out. Ctrl+Tab, Ctrl+Shift+Tab and closing the selected tab pass over a collapsed group's tabs, and a tab closed from a group goes back into it when reopened. Groups come back at the next start and reach your other devices with your open tabs. On Linux the tab strip along the top shows each group as a chip before the tabs and a dot on its tabs, and cannot hide a collapsed group's tabs.
- **Private windows.** Ctrl+Shift+N, the main menu or a link's context menu opens a private window, with dark header bars and a private icon on Linux and a dark window with a Private badge on Windows. Its pages keep cookies and site data in memory only, and nothing it does reaches your history, open tabs, sync or the session restored at the next start. Site choices, zoom and its downloads list last until the last private window closes, and its downloads show in private windows only; downloaded files stay, except one still waiting for Keep or Discard, which is deleted. Extensions stay out of private windows: on Linux you can allow one in the Extensions dialog, as in Chrome.
- **Memory Saver.** As in Chrome, a tab left in the background for a few hours goes to sleep and frees its memory, and its icon fades. Going back to it brings the page back where it was, with its history. Tabs on screen, pinned, playing sound, using the camera, microphone or screen, from a site allowed to notify, or holding a form you haven't sent stay awake. Settings > General turns it off or picks how soon tabs sleep: after 6, 4 or 2 hours.
- **Chrome's page commands and keys.** Print (Ctrl+P), Developer tools (F12, Ctrl+Shift+I), the JavaScript console (Ctrl+Shift+J) and View page source (Ctrl+U) are in the main menu and the page's context menu. Every shortcut can be reassigned in Settings.
- **Search engines with shortcuts.** Add, edit and remove search engines in Settings, as in Chrome. Typing an engine's shortcut and a space in the address bar searches that engine. As you type a search, the address bar lists the default engine's suggestions above your history and bookmarks, unless you turn them off in Settings.
- **Spell check.** Misspelled words in a page's text fields are underlined, and right-clicking one offers corrections and Add to Dictionary. On Linux, Settings > General turns it off or picks its languages from the installed Hunspell dictionaries, starting from your system's languages, and the choice syncs. On Windows WebView2 checks spelling itself, with no setting to turn it off.
- **Downloads with a second look at programs.** The Downloads list shows how fast each download is going and how long it has left, in Chrome's words ("1.3 MB/s - 3.2 MB of 10 MB, 5 secs left"). A downloaded program, script or installer (Chrome's dangerous file types for your system) waits under a name that can't run until you choose Keep or Discard, in a warning under the downloads button or in the Downloads list. On Windows a download can be paused and resumed, or resumed after a network error, and every download is marked as coming from the Internet, so SmartScreen checks it when you open it. WebKitGTK cannot pause a download, so Linux offers no pause.
- **Bookmarks in and out.** Import bookmarks from Chrome, Edge, Brave, Vivaldi, Chromium or Firefox on the same machine, or from a bookmarks file. Export them from the Bookmarks manager to an HTML file that any browser can import.
- **Profiles.** Keep work and personal browsing apart, as in Chrome: each profile has its own bookmarks, history, settings, extensions and sync account, and opens in windows of its own. The profile button in the toolbar switches between them, adds one or manages them, and with more than one profile a picker asks which to open when Vsesvit starts, unless you turn it off.
- **Sync you can host.** Bookmarks, history, open tabs, extensions, settings, search engines and site permissions sync through [a small server](server/README.md) (Docker image, SQLite or Postgres) that signs people in with the OpenID Connect provider you choose. Set a sync passphrase and everything is encrypted on your devices before it leaves them, so the server stores records it cannot read; without one, whoever runs the server can read your synced data. Vsesvit offers a passphrase once on each device, and Settings > Sync has it at any time; the server's README says what the server can still see.
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
