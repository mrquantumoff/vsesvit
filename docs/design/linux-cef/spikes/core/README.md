# Vsesvit Linux shell spike (spike-core)

GPUI (forced to X11) draws the browser chrome. Every tab is a **Chrome-style CEF 154
browser embedded as a native X11 child window**. Findings are written up in
`/home/dy/git/vsesvit-cef/docs/design/linux-cef/spike-core.md`.

## Layout

| Path | What |
|---|---|
| `Cargo.toml` | gpui-pre =0.3.9, gpui-pre-platform =0.3.9 (x11 only), gpui-component =0.7.3, cef =154.5.0, x11rb 0.13; `[patch.crates-io]` for the vendored GPUI X11 backend |
| `build.rs` | `-Wl,-rpath,$ORIGIN` so the binaries find `libcef.so` next to them |
| `src/main.rs` | browser process: `api_hash`, `execute_process`, scale factor, `cef::initialize`, GPUI on X11, `cef::shutdown` |
| `src/bin/vsesvit-spike-helper.rs` | CEF sub-process executable (`Settings.browser_subprocess_path`) |
| `src/cefglue/` | CEF side, no `gpui::*` imports: `app.rs` (App, BrowserProcessHandler, switches), `client.rs` (Client + all handlers), `mod.rs` (`CefEvent`, `Shared`), `idc_names.rs` |
| `src/pump.rs` | CEF external message pump + GLib default-context integration driven from a GPUI foreground task |
| `src/xwin.rs` | the shell's own x11rb connection: 24-bit content host window, geometry, map/unmap, SHAPE, input focus, screenshots |
| `src/shell.rs` | GPUI view: tab strip, toolbar with a gpui-component `Input`, content area, overlays, focus model, control commands |
| `src/ctl.rs` | line-based control FIFO (`SPIKE_CTL`) used by the scripts |
| `vendor/gpui-pre-linux/` + `vendor/gpui-x11-focus.patch` | GPUI X11 backend with the focus patch (focus proxy + Inferior focus events) |
| `src/bin/xctl.rs` | test tool: X tree, focus, focus/key watch, screenshots, WM_DELETE_WINDOW |
| `tools/kwin-input/` | test tool: inject pointer/keyboard input into a private KWin (org_kde_kwin_fake_input) |
| `testpages/` | local pages (served on 127.0.0.1:8765) |
| `*.sh` | run/drive/test scripts (below) |

## Build

```sh
cd /tmp/vsesvit-scratch/spike
export CEF_PATH=/home/dy/.cache/vsesvit-cef CARGO_TARGET_DIR=/tmp/vsesvit-scratch/target-core
cargo build                                   # vsesvit-spike, vsesvit-spike-helper, xctl
(cd tools/kwin-input && cargo build)          # kwin-input (same target dir)
```

## Run (private headless display, nothing on the owner's desktop)

```sh
# 1. private KWin (virtual backend) + rootless Xwayland :1 inside a private D-Bus session
./start-display.sh &        # KWIN_WAYLAND_NO_PERMISSION_CHECKS=1 for kwin-input
# 2. local test pages
python3 -I -m http.server 8765 --bind 127.0.0.1 --directory /tmp/vsesvit-scratch/spike/testpages &
# 3. the spike (log: /tmp/vsesvit-scratch/core-env/spike.log, control FIFO: core-env/ctl)
./run.sh SPIKE_URLS="http://127.0.0.1:8765/page1.html https://example.org" &
# 4. drive it
source drive.sh; ctl state; ctl "new https://example.org"; ctl "shotraw /tmp/x.png"; $K click 760 170; $K key ctrl+l
./kill.sh                                               # kill every spike process
```

`run.sh` points `HOME` at a scratch directory, drops `WAYLAND_DISPLAY`/`GDK_BACKEND`, and uses the private D-Bus.

### Environment switches

| Variable | Values | Default |
|---|---|---|
| `SPIKE_URLS` | space-separated initial tabs | `https://example.org` |
| `SPIKE_KEYS` | `none` / `cmd` / `key` / `both`: shortcut interception | `both` |
| `SPIKE_POPUPS` | `child` / `default` / `tab`: what `on_before_popup` does | `child` |
| `SPIKE_DIALOGS` | `default` (Chrome UI) / `shell` (handlers answer) | `default` |
| `SPIKE_PUMP_MAX_MS` / `SPIKE_PUMP_IDLE_MS` / `SPIKE_GLIB_TICK_MS` | pump caps (active / idle) and GLib tick | 33 / 250 / 250 |
| `SPIKE_NO_GLIB_WATCH` | disable the GLib fd watcher (experiment) | unset |
| `SPIKE_SCALE` | device scale for both GPUI and CEF | Xft.dpi/96 |
| `SPIKE_WIN` | window size `WxH` (logical px) | `1280x800` |
| `SPIKE_SAME_EXE` | `1` = no helper, sub-processes re-run the main binary | unset |
| `SPIKE_PASSWORD_STORE` | `basic` / `auto` | `basic` |
| `SPIKE_CEF_ARGS` | extra Chromium switches | unset |

### Control commands (`ctl ...`)

`new URL`, `nav URL`, `switch N`, `close [N]`, `back`, `fwd`, `reload`, `stop`, `js CODE`,
`omnibox`, `page`, `menu shape|plain|popup|off`, `bar`, `resize W H`, `fullscreen`,
`shot PATH` (GetImage + paste children), `shotraw PATH` (one GetImage), `tree`, `roottree`,
`state`, `stats`, `chrome IDC_NAME`, `closeext`, `unfullscreen`, `quit`.

## Test scripts

| Script | Checks |
|---|---|
| `selftest.sh` | 19 end-to-end checks: tabs, titles, favicon, focus both ways, Ctrl+L, no key leak, shortcuts, popups, dialogs, overlay screenshot, resize, clean shutdown |
| `accel-test.sh none|cmd|both` | Ctrl+T/N/W/Shift+T, F11, Ctrl+P, Ctrl+F, F12 with each interception mode |
| `dialog-test.sh default|shell` | alert/confirm/prompt, file input, geolocation, download, notification permission, fullscreen, `<select>` |
| `cpu-test.sh "ENV=.." label` | per-process CPU over 15 s with one page loaded, pump rates |
| `latency-test.sh [ENV]` | key press -> `on_pre_key_event` latency after idling |
| `cpu.sh N` | per-process CPU over N seconds |
