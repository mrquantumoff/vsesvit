<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="packaging/icons/DarkBG.svg">
    <img src="packaging/icons/WhiteBG.svg" alt="Vsesvit logo" width="160">
  </picture>
</p>

<h1 align="center">Vsesvit</h1>

<p align="center">A native web browser for Windows 11 and Linux, written in Rust.</p>

Each platform gets a shell built with its own toolkit around its own web engine, on a shared Rust core: WinUI 3 with WebView2 on Windows, GTK4/libadwaita with WebKitGTK on Linux.

## Features

- **Chrome Web Store extensions** on both platforms, plus Edge Add-ons, Firefox AMO, and local `.crx`/`.xpi` files. Updates install automatically.
- **Vertical tabs** and **tab groups**, with Chrome's tab menu.
- **Private windows** that keep nothing after they close.
- **Memory Saver** puts background tabs to sleep.
- **Downloads** with a check on programs and scripts before they run.
- **Bookmark import and export** from Chrome, Edge, Brave, Vivaldi, Chromium, Firefox, or an HTML file.
- **Profiles** with separate bookmarks, history, settings, extensions, and sync accounts.
- **Self-hosted sync** of bookmarks, history, tabs, extensions, settings, and site permissions, end-to-end encrypted with a passphrase. See [server/README.md](server/README.md).
- **Privacy**: tracker blocking, HTTPS-only mode, and cookie controls. Passwords stay in your password manager.

Plan and design decisions are in [docs/PLAN.md](docs/PLAN.md). Packaging is described in [docs/design/packaging.md](docs/design/packaging.md).

## Install

Download builds from [GitHub Releases](https://github.com/mrquantumoff/vsesvit/releases). Channels: stable, beta, weekly, nightly.

Linux needs Ubuntu 26.04 or newer, or equivalent GTK 4.22, libadwaita 1.9, and WebKitGTK 2.52.

## Build from source

### Linux

Requires GTK 4.22, libadwaita 1.9, WebKitGTK 2.52, and GLib 2.80+. Debian/Ubuntu packages:

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev
```

For audio and video, also install `gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav`.

```bash
cargo run -p vsesvit
```

To build inside WSL from a Windows checkout: `bash scripts/wsl.sh run -p vsesvit`

### Windows 11

Requires the Rust MSVC toolchain, Visual Studio Build Tools with the C++ workload, and the Windows App Runtime 2.5.1 or newer (x64). WebView2 ships with Windows 11.

```bash
cargo run -p vsesvit
```

## Test

```bash
cargo test --workspace
```

`vsesvit --self-test <dir>` runs the end-to-end check in [docs/design/self-test.md](docs/design/self-test.md). It writes `report.json` and a screenshot to `<dir>` and exits non-zero on failure. Add `--network` to install uBlock Origin Lite from the Chrome Web Store.

## Layout

```
crates/vsesvit         binary; picks the shell for the target OS
crates/vsesvit-core    profile store, data model, omnibox, extension install
crates/vsesvit-winui   Windows shell (WinUI 3 + WebView2)
crates/vsesvit-gtk     Linux shell (GTK4 + libadwaita + WebKitGTK)
crates/vsesvit-webext  WebExtensions runtime for WebKitGTK
crates/vsesvit-sync    sync engine
server/                self-hostable sync server
docs/                  plan, design documents, research notes
```

## License

MIT. See [LICENSE](LICENSE).
