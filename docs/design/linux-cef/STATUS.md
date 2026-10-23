# CEF + GPUI shell rewrite: status

Last updated: 2026-10-23. Only design work is on this branch; no shell code has been written yet. Decisions and results are logged in `log.tsv`.

## Decisions

- Stock CEF (`cef` crate 154.x; CEF 154.0.34, Chromium 154.0.8037.98) and GPUI (`gpui-pre` 0.3.9 + `gpui-component` 0.7.3).
- On Linux the whole app runs on X11 (XWayland under Wayland).
- Each tab is a Chrome-style CEF browser, embedded as a native child window (X11 on Linux, HWND on Windows) of the GPUI window.
- The new crate will be `crates/vsesvit-gpui`. It replaces `crates/vsesvit-gtk`, and `crates/vsesvit-webext` goes away with it.
- The GPUI shell is also an option on Windows (2026-10-23). There it is a build-time choice: a cargo feature on `crates/vsesvit` swaps `vsesvit-winui` for `vsesvit-gpui`. WinUI stays the default on Windows until the GPUI shell reaches parity.
- One crate for both platforms. The CEF glue, state and GPUI views are shared. Only native embedding (host window, focus, overlays, screenshots, top-level-at-point) and the message pump differ, behind one platform module with an X11 and a Windows backend.
- Open: H.264/AAC. Stock CEF builds leave these codecs out, so a self-built CEF (`proprietary_codecs=true ffmpeg_branding=Chrome`) is needed. The code takes the CEF build from `CEF_PATH`, so a different build can be dropped in later.

## Done

- `inventory/`: everything the GTK shell, the WebKit extension runtime, the core surface and packaging/CI provide today, each file ending in a feature checklist.
- `research/`: CEF extensions, windowing, OSR, a cef-rs API cheat-sheet (gotchas G1–G17), GPUI, prior art, and a GPT cross-check of the extension architecture.
- `ui-architecture.md`: the GPUI chrome design.
- `spike-core.md`, with its code in `spikes/core`: all 11 items verified, and its self-test passed 19 of 19. The real crate needs three things:
  - a 24-bit host X window between GPUI and CEF, because CEF's child window fails with `BadMatch` under GPUI's ARGB window;
  - the GPUI X11 focus patch, `spikes/core/vendor/gpui-x11-focus.patch`;
  - a message pump that polls (at most 33 ms while active, 250 ms when idle) and also watches the GLib fds.

## Stopped part-way (no report written)

### `spikes/ext-load`: loading and managing extensions

Findings so far:
- Chromium records `--load-extension` installs as location 8 (command line).
- Their `chrome.storage` data survives runs where the extension is left out, including runs past the 30 s garbage-collection delay.
- External Extensions JSON installs happen without a prompt.
- `management.uninstall` always shows Chrome's own "Remove …?" dialog inside the child browser, even with `showConfirmDialog: false`.
- Toggling incognito access on a command-line extension left it disabled with `DISABLE_UNSUPPORTED_DEVELOPER_EXTENSION` (`1 << 24`), and it was still disabled after a restart. This was under investigation when the spike stopped.
- Still open: CDP `Extensions.*` over `--remote-debugging-pipe` (the client exists in the spike), storage access via CDP, and updates.

### `spikes/ext-api`: what extensions see

Findings so far:
- `--noerrdialogs` keeps a failed `--load-extension` from showing a modal.
- Chrome-created native windows (from `tabs.create` / `windows.create`) can be hidden by reparenting them synchronously into an unmapped holder. They can then be embedded as GPUI tabs with Chrome's UI cropped off (85 px at scale 1), and they stay interactive.
- At startup, the first browser stays Chrome's active window until a real focus change.
- An Alloy-style action popup works as an overlay, and `window.close()` closes it, but activeTab is not granted.
- The Chrome-style popup test was in progress when the spike stopped.

Throwaway extension signing keys were left out of the copies. Regenerate them, or drop the manifest `key`s, before rerunning ext-api.

## Next

1. Spikes, run in parallel (2026-10-23):
   - `spikes/win-core` and `spike-win.md`: the Windows version of every spike-core item (embedding in GPUI's DirectComposition window, message loop, focus, overlays, screenshots, shutdown) and the platform-seam sketch.
   - Finish `spikes/ext-load` and `spikes/ext-api` in WSL under a headless weston + Xwayland, with reports `spike-ext-load.md` and `spike-ext-api.md`.
2. Write `design.md` for both platforms, then review it with a Sonnet/Opus subagent and with GPT (`codex`).
3. Implement in waves. Linux and Windows build in every wave; each wave ends with its checks green on both.
   - foundation: crate, platform seam with both backends, engine core, boot, window and tabs;
   - the `gpui` cargo feature on `crates/vsesvit` for Windows;
   - feature waves, each reviewed;
   - extension integration;
   - self-test port (Linux headless; Windows off-screen, no OS input);
   - packaging/CI for both;
   - docs: PLAN.md sections, README, an ADR;
   - delete `vsesvit-gtk` and `vsesvit-webext`.
4. Final full-diff review with GPT-6-Astra.
