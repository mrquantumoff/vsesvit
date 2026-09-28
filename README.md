# Vsesvit

A web browser for Windows 11 and Linux, written in Rust. On Windows it is a WinUI 3 app around WebView2. On Linux it is a GTK4/libadwaita app around WebKitGTK. Both install Chrome Web Store and Microsoft Edge Add-ons extensions, and both store bookmarks, history, open tabs, extensions and settings in a sync-ready format.

The plan, the decisions and their evidence are in [docs/PLAN.md](docs/PLAN.md).

## Build and run

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
docs/                  plan, design documents, research notes, design arena record
```

## License

MIT. See [LICENSE](LICENSE).
