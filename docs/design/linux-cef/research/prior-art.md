# Prior art: Rust apps that embed CEF, and GPUI webviews

Research date: 2026-10-16. Question: what have other projects learned from embedding CEF (Chromium Embedded Framework) in Rust GUI apps, especially GPUI, and what should Vsesvit's new Linux shell (GPUI chrome, CEF 154 through the `cef` crate 154.5.0+154.0.34, maximal Chrome-extension compatibility) copy or avoid?

Tags used on claims:

- **VERIFIED**: I read the code, document, issue or commit myself. The commit or version is named.
- **REPORTED**: a project's own README, notes or issue says so (they measured it; I did not).
- **INFERRED**: my reasoning from verified material. Nobody has run it.

Nothing was built or run. Every repository was cloned into its own directory under `/tmp/vsesvit-research/prior-art-*/` and read there:

| Project | Local path | Commit read |
|---|---|---|
| longbridge/wef | `prior-art-wef/repo` | `0344897` (2025-10-16) |
| merely-made/wgpu-weld (`welding`) | `prior-art-wgpu-weld/repo` | `b673f36` (2026-10-13) |
| not-elm/bevy_cef | `prior-art-bevy_cef/repo` | `4156034` (2026-10-16) |
| mycrl/wew | `prior-art-wew/repo` | `07ad6c6` (2026-04-13) |
| ekinertac/infiniterm | `prior-art-infiniterm-repo/repo` (plus PR branches `ext-spec-cef-fork` and `cef-patches`) | `1fbc267` (2026-10-16) |
| tauri-apps/cef-rs | `prior-art-cef-rs/repo` | `163744c` (2026-10-12) |
| tauri-runtime-cef (crate) | `prior-art-tauri-runtime-cef/src/tauri-runtime-cef-3.0.0-alpha.6` | crates.io 3.0.0-alpha.6 (2026-10-14) |
| longbridge/gpui-kit (webview crate) | `prior-art-gpui-kit/repo` (sparse) | `8a83be7` (2026-10-16) |
| longbridge/gpui-fast (composition) | `prior-art-gpui-fast/repo` (sparse) | `eeccb27` (2026-10-15) |
| bokuweb/gpui-cef | `prior-art-gpui-cef-bokuweb/repo` | `1e88700` (2026-09-26) |
| HodlOg/gpui-browser | `prior-art-gpui-browser-hodlog/repo` | `e6c5009` (2026-04-22) |
| Glass-HQ/Glass-v1 (browser crate) | `prior-art-glass-v1/repo` (sparse) | `e8ec083` (2026-10-06) |
| GraphiteEditor/Graphite (desktop) | `prior-art-graphite/repo` (sparse) | `46f48a5` (2026-10-15) |
| Loafer19/spatial-browser (+ wiki) | `prior-art-spatial-browser/repo`, `prior-art-spatial-browser-wiki/wiki` | `6231d7e` (2026-10-12) |

Single files were read through the GitHub API for tobagin/karere, makepad/makepad and musichen/qedit. The sibling documents `cef-osr.md`, `cef-windowing.md` and `cef-extensions.md` in this directory cover CEF itself in depth. This document covers what other projects did, and cross-references those documents instead of repeating them.

---

## 1. Executive summary

### 1.1 What this means for a GPUI + CEF Linux browser with maximal Chrome-extension compatibility

1. **Nobody has shipped GPUI + CEF on Linux.** (VERIFIED, survey in section 6.)
   - **Glass v1** (918 stars, GPL-3.0) is the only real GPUI + CEF browser. It is archived, targets macOS and Windows only, and runs CEF 145.
   - **wef / gpui-webview** (Longbridge, the gpui-component authors) was abandoned as "not good enough". Longbridge's own desktop app still uses Wry.
   - **HodlOg/gpui-browser** is the only Linux attempt. It is a one-commit prototype from April 2026: a GPUI fork with a dmabuf surface primitive, plus CEF 145.
   - **infiniterm**, **gpui-cef** (bokuweb) and **ginka** are macOS-only.
   - Vsesvit would be first. The parts are proven separately; the combination is not.

2. **Every project that draws CEF inside its own UI uses windowless rendering (OSR), and OSR is always Alloy style.** (VERIFIED: `include/internal/cef_types_runtime.h` on branch 8037, "Windowless rendering will always use Alloy style".) Alloy style breaks the browser-level extension APIs.
   - infiniterm measured this on CEF 152:
     - `tabs.query` and `windows.getAll` return `[]`.
     - `tabs.create` and `windows.create` open real native Chromium windows.
     - `chrome.action` state is stored but never drawn.
   - spatial-browser documents "No Chrome Web Store / extensions".
   - tauri-runtime-cef refuses extension attributes outright ("CEF removed its extension loading API").
   - **infiniterm is the only project attacking this.** Its plan is a patched CEF:
     - P1: put Alloy tabs into `tabs.*` and `windows.*`.
     - P2: route `windows.create` and `tabs.create` to the app.
     - P3: report action icon, badge and popup state to the app.
     - P4: dispatch `chrome.commands`.
     - P5: proprietary codecs.
   - Its P1 and P2 patches are drafts that have never been compiled (PR #333, open). (VERIFIED.)
   - **Decision for Vsesvit:** with GPUI-drawn pages, "total extension compatibility" means either carrying a CEF patch set like infiniterm's, or hosting Chrome-style windowed browsers as X11 children. The second option gives up native Wayland. See `cef-extensions.md` and `cef-windowing.md` for the CEF side.

3. **Frames: CPU first, zero-copy dmabuf later, behind a flag.**
   - **CPU path.** `OnPaint` BGRA goes into a GPUI `RenderImage`, which is drawn with `img()` or `paint_image`. It works everywhere and is what most projects ship: wef, infiniterm, gpui-cef by default, bevy_cef on Linux, makepad on Linux. (VERIFIED.)
   - **Atlas leak.** GPUI has no sprite-atlas LRU, so every frame must `drop_image` the previous one or the atlas grows by one texture per frame (infiniterm, VERIFIED in `browser_body.rs`).
   - **Zero-copy dmabuf.** This works on Intel/Mesa and AMD/RADV:
     - welding: X11 and native Wayland, wgpu 30.
     - Graphite: a production app.
     - godot-cef.
   - **NVIDIA.** Every Linux project that tried zero-copy on NVIDIA reports failure or flakiness: welding, karere, HodlOg, Graphite. The root cause is CEF 154 asking for linear GBM buffers (CEF #4237). The fix is only in CEF 156, which then exposed a missing-fence bug on NVIDIA (#4313, open, fix PR #4314). (VERIFIED.)
   - **This machine.** It has an RTX 5080, so the CPU path is the only one that will work with CEF 154 here. (INFERRED from the above. `cef-osr.md` reaches the same conclusion.)

4. **Neither stock GPUI renderer can take an external GPU texture on Linux.**
   - HodlOg forked GPUI to add a `DmaBufFrame` surface primitive and Vulkan import in `gpui_wgpu`. (VERIFIED.)
   - gpui-fast (Longbridge's GPUI fork, from zed#62379) takes a different route: *window composition*.
     - The page lives in a desynchronized `wl_subsurface` (Wayland) or a child window (X11), registered as an `ExternalGpu` surface.
     - GPUI overlays (menus, popovers, dialogs) render in synchronized subsurfaces above it.
     - It is explicitly meant for "CEF off-screen rendering" producers. It has an `on_input` hook for windowless producers' mouse and wheel input. (VERIFIED.)
   - Composition avoids teaching GPUI's renderer about dmabufs, at the cost of rectangular clipping only.

5. **Message loop: avoid fixed-rate timers, and choose between a separate CEF UI thread and pumping both CEF and GLib.**
   - Patterns seen:
     - wef on Linux: a fixed 60 Hz timer.
     - infiniterm: a 4 ms timer.
     - HodlOg: a 0.5 to 4 ms adaptive timer.
     - bevy_cef: scheduled by `on_schedule_message_pump_work`, 4 ms minimum, 30 Hz fallback.
     - tauri: a port of cefclient's GLib external pump.
     - Graphite: `multi_threaded_message_loop` on Linux, with CEF in a separate host process.
   - A cef-rs maintainer says `multi_threaded_message_loop` "works great" on Windows and Linux (cef-rs #277). (VERIFIED, all.)
   - `cef-osr.md` explains why: under `external_message_pump`, Chromium's GLib sources sit on the default GLib context, which GPUI never iterates.

6. **Input is the most under-built area in every GPUI integration.**
   - GPUI 0.2.2 `Keystroke` has no hardware keycode (VERIFIED). So:
     - wef maps 12 keys.
     - HodlOg puts the native keycode into `windows_key_code`.
     - Glass's GPUI fork has `native_key_code`, but always sets it to `None` on Linux. (VERIFIED.)
   - The correct Linux mapping is in bevy_cef. `native_key_code` = evdev code + 8 (the XKB keycode), and each key press sends RAWKEYDOWN, then CHAR, then KEYUP. (VERIFIED.)
   - Composed text goes through GPUI's `EntityInputHandler` to `ImeSetComposition` and `ImeCommitText` (wef, gpui-cef). welding found that a null `replacement_range` makes `ImeCommitText` silently do nothing. (REPORTED.)

7. **Recurring pitfalls, each hit by at least one project:**
   - A fork bomb when `execute_process` is skipped (wef #9).
   - Google sign-in refusing embedded Chromium. infiniterm fixed it with a CDP user-agent brand override; Glass with a UA string.
   - Missing H.264/AAC in Spotify-CDN builds. Glass and infiniterm build CEF themselves.
   - Native DevTools for OSR crashing CEF 151 on Linux (welding). Opening the DevTools frontend in a tab over the remote-debugging port is the working alternative (infiniterm PR #332).
   - Alloy browsers crashing in Chrome code that assumes a tab (CEF #4234, spatial-browser).
   - `OnRenderProcessTerminated` never firing on Linux (welding).
   - The `--disable-features` value being replaced instead of merged (tauri).
   - Chrome-style browsers keeping Chrome's accelerators live, so Ctrl+N opens a Chrome window (tauri).
   - Clipboard not working in OSR on Wayland (spatial-browser).

### 1.2 Patterns to copy

| # | Pattern | Who does it (VERIFIED) | Why |
|---|---|---|---|
| P1 | Call `execute_process` first in `main`, or point `browser_subprocess_path` at a dedicated helper binary. | Every project; bevy_cef ships `bevy_cef_render_process`, Glass ships `glass_helper`. | Subprocesses re-execute the host binary. A dedicated helper avoids running GPUI init in renderers. |
| P2 | Copy or import the `OnAcceleratedPaint` dmabuf **inside the callback**, then return. | Graphite (copy to an owned texture, `device.poll(Wait)`, then ack before returning), godot-cef (sync-file semaphore, foreign-queue release before return). | CEF's contract: "The contents of `info` will be released back to the pool after this callback returns" (`include/cef_render_handler.h`, branch 8037). |
| P3 | `dup(2)` every plane fd before handing it to Vulkan. Import with `VkExternalMemoryImageCreateInfo`, `vkGetMemoryFdPropertiesKHR`, a DRM-modifier tiling chain and a dedicated allocation. | Graphite `frames/import/dmabuf.rs`, welding via `grafting`. | `vkAllocateMemory` takes ownership of the fd. |
| P4 | Treat `DRM_FORMAT_MOD_INVALID` as `DRM_FORMAT_MOD_LINEAR`. Requires `VK_EXT_image_drm_format_modifier`, which wgpu enables from version 30. | welding, verified on AMD/RADV by a whole-texture dump. | AMD hands over an implicit modifier. |
| P5 | Import BGRA/RGBA as **sRGB** formats. | welding, the cef#3687 thread. | CEF renders sRGB; UNORM double-applies gamma. |
| P6 | Pin the wgpu adapter to the same GPU as Chromium's GPU process. Prefer the iGPU, or pass `--gpu-vendor-id` and `--gpu-device-id` to CEF. | spatial-browser, godot-cef. | Cross-device dmabuf import fails on hybrid laptops. |
| P7 | Use `coded_size` for the buffer and `visible_rect` (CEF's content rect) for sampling. | Graphite `ContentRect`; HodlOg's TODO. | Coded size is padded to tile alignment and has junk at the edges. |
| P8 | Keep one shared paint slot per browser, "latest frame wins", and `cx.notify()` only when it is dirty. | infiniterm, HodlOg, bevy_cef. | Avoids work when idle. |
| P9 | Draw `PET_POPUP` (`<select>` and similar) as a separate image at `OnPopupSize`'s rectangle, over the view. | wef `element.rs`, welding `acquire_popup`. | CEF does not composite popups into the view. |
| P10 | Turn `OnBeforePopup` into an app-level tab or window request and return 1. Exception: popups that need `window.opener` (OAuth) must really be created. | infiniterm, wef, Glass (OAuth popups become real native windows). | Otherwise CEF opens native windows outside the app. |
| P11 | Draw context menus, JS dialogs and permission prompts in the host UI from `RunContextMenu`, `OnJSDialog` and the permission handler. | wef (gpui-component modals), Glass, infiniterm. | OSR cannot show CEF's own UI without an X11 parent. |
| P12 | Map `OnCursorChange` to the GPUI `CursorStyle` (about 30 cases). | wef `utils.rs`. | Windowless CEF has no native cursor. |
| P13 | Show DevTools as the local frontend (`/devtools/devtools_app.html?ws=127.0.0.1:<port>/devtools/page/<id>`) in a normal tab. Use a random loopback `remote_debugging_port` and `--remote-allow-origins` naming only that origin. | infiniterm PR #332. | `ShowDevTools` for OSR opens a native window (infiniterm) or crashes (welding, CEF 151 on Linux). |
| P14 | Override `Sec-CH-UA` and `navigator.userAgentData` brands through CDP `Emulation.setUserAgentOverride` before the first navigation. | infiniterm `moat.rs` (Google sign-in verified end to end on macOS). | Google blocks embedded Chromium. |
| P15 | Read a comma-list switch (`--disable-features`, `--enable-features`, `--js-flags`) and merge into it. Never append it fresh. | tauri `switches.rs`, Graphite `browser_process_app.rs`. | CEF pre-fills `--disable-features`; appending replaces its list. |
| P16 | Filter Chrome commands in `CefCommandHandler::OnChromeCommand` for Chrome-style browsers. | tauri `cef_impl/client/command.rs`. | Ctrl+N and Ctrl+T otherwise open Chrome windows and tabs the app does not own. |
| P17 | Run a crash-restart supervisor and pass `--disable-gpu-process-crash-limit`. | spatial-browser. | A browser-process crash ends the app; the GPU process otherwise gives up after a few crashes. |
| P18 | Scripted, headless input batteries (env-var knobs that click, scroll, type, then dump pixels). | welding's `WELD_*` battery, infiniterm `tools/drive`. | Gives a provable input/render check without a human. Fits Vsesvit's self-test. |

### 1.3 Pitfalls to avoid

- **Importing a dmabuf after the callback returns.** welding, HodlOg, the cef-rs example and bevy_cef's macOS IOSurface path all do this. bevy_cef documents it as a "known deviation" that can show torn frames under load. (VERIFIED in code.)
- **Keying an import cache on fd number.** HodlOg caches by `(fd, w, h, modifier)`. Fd numbers are recycled, so a cache hit can bind a stale image. (INFERRED.) Use the dmabuf's inode (`fstat`) if caching at all.
- **The `cef` crate's `osr_texture_import::dmabuf`.** It leaks `VkDeviceMemory` every frame and has no synchronization (VERIFIED; details in `cef-osr.md` §Executive summary, item 4).
- **Assuming shared textures fall back to `OnPaint`.** karere: "Without this pre-browser fence CEF may never call CPU on_paint, leaving a blank first frame". `shared_texture_enabled` is fixed per browser, so probe GPU import capability *before* creating the browser. (REPORTED, karere `src/web_view.rs`.)
- **`disable-gpu` plus `disable-gpu-compositing` for OSR.** wef does this, copied from cefclient (#1257). It also kills WebGL and GPU raster. (VERIFIED.) It is fine for a fallback, not for a browser.
- **Fixed-rate pumping.** wef's 60 Hz `do_message_work` timer, infiniterm's 4 ms timer and HodlOg's 0.5 to 4 ms wake waste CPU when idle and add latency when busy. gpui-cef measured GPUI's foreground executor ticking only every 1 to 5 s when the app was idle on macOS, which stalled page loads. It bypassed GPUI with a `CFRunLoopTimer` (REPORTED, `platform/cef/pump.rs`).
- **Sending only text through the IME path.** wef sends letters only through `ImeCommitText`, so pages see no `keydown` for letters and shortcut handling breaks (wef #14 reports special keys failing). Send RAWKEYDOWN/KEYUP for every key, and commit text either through CHAR events or through IME, never both. gpui-cef notes that both "would insert every character twice". (VERIFIED code, REPORTED behaviour.)
- **wef's C++ input bugs** (VERIFIED in `crates/wef/cpp/wef.cpp`):
  - `std::max(click_count, 3)` turns every click into a triple click.
  - Mouse moves always carry `EVENTFLAG_LEFT_MOUSE_BUTTON`, so every move looks like a drag.
  - `wef_browser_is_audio_muted` has an inverted null check.
- **`RawWindowHandle` on GPUI 0.2.2 X11.** `X11Window::window_handle()` is `unimplemented!()` (VERIFIED, `gpui-0.2.2/src/platform/linux/x11/window.rs:314-317`). Any integration that asks for a parent handle on X11 panics, wef's included (INFERRED). OSR does not need a parent: pass a null or zero handle.
- **GTK in-process.** welding's notes report CEF's GTK3 use clashing with a GTK4 app (from the cef#3687 thread). GPUI uses no GTK, so this does not apply, but do not add GTK4 to the GPUI process. (REPORTED.)
- **CEF's NVIDIA problems are not GPUI's.** On CEF 154 the GPU process fails to create the shared image (#4237). Graphite, karere and HodlOg all fall back to software or warn. (VERIFIED issue, REPORTED by apps.)
- **Licences.** infiniterm is source-available (no redistribution) and Glass/qedit are GPL-3.0. Read them for ideas only; do not copy code into MIT-licensed Vsesvit. welding/grafting are MPL-2.0: fine as dependencies, but copied files stay MPL. (VERIFIED licence files.)

---

## 2. longbridge/wef and the GPUI webview widgets (question 1)

### 2.1 Facts

| Item | Value |
|---|---|
| Repo | https://github.com/longbridge/wef. Extracted from gpui-component on 2025-09-29 (commit `b6454b2`). Crates `wef` 0.1 to 0.6 on crates.io list `github.com/longbridge/gpui-component` as their repository. |
| Last code commit | `0344897`, 2025-10-16. Pushes since then are Dependabot branches only (pushed_at 2026-01-01). |
| Licence | Apache-2.0 (crate metadata; the repo has only `LICENSE-APACHE`) |
| Crates | `wef` (CEF wrapper, own C++ glue), `cargo-wef` (CLI; the older `wef-tool` crate, 0.1 to 0.4 from May and June 2025, is its predecessor), `gpui-webview` (path crate in the repo, not the gpui-kit crate of the same name) |
| CEF | Default download `137.0.10+g7e14fe1+chromium-137.0.7151.69` (`crates/cargo-wef/src/internal/cef_platform.rs`); CI pins 136.1.6 |
| GPUI | `gpui = "0.2.0"`, `gpui-component = "0.2.0"` (`crates/webview/Cargo.toml`) |
| Status | README warning, verbatim: "This is an experimental project, we was tried to build a offscreen rendering WebView for solve GPUI render level issues in game engine, but there still not good enough. For example, the app size (included a CEF Framework increase 1GB), development experience, etc. So, we are still use Wry ... in Longbridge desktop app for now." |

**Does gpui-component have a webview widget?**
- Yes, but it is Wry, not CEF: gpui-kit's `crates/webview` ("WebView support for GPUI, based on Wry", version 0.7.3, previously published as `gpui-wry`). (VERIFIED, `crates/webview/Cargo.toml` and `README.md` at `8a83be7`.)
- On Linux it is WebKitGTK in an X11 child window. "Linux (Wayland) — Not supported; run on XWayland." `WebView::build()` returns an error on a Wayland window. (VERIFIED.)
- With GPUI Fast it places the child in a composition surface, so GPUI overlays render above the page. On X11 this works by cutting overlays out of the child window's SHAPE region. (VERIFIED, `crates/webview/src/linux.rs`.)
- GTK's loop is pumped from GPUI every 16 ms: `gtk::main_iteration_do(false)` in a `cx.spawn` loop. (VERIFIED.)
- The CEF-based GPUI widget exists only inside the wef repo as `crates/webview`. (VERIFIED.)

### 2.2 How wef works (VERIFIED unless marked)

**C++ glue, not cef-rs.**
- `build.rs` compiles `libcef_dll_wrapper` from `$CEF_ROOT` (default `~/.cef`) with `cc`, plus wef's own `cpp/*.cpp`.
- It exposes `extern "C"` functions (`wef_init`, `wef_browser_create`, `wef_browser_send_*`, ...). The Rust side wraps them.
- On Linux it needs `glib-2.0` headers (pkg-config) for its pump.

**Settings** (`cpp/wef.cpp` `wef_init`):
- `windowless_rendering_enabled = true`.
- `external_message_pump = true`.
- `no_sandbox = true` on Linux and Windows.
- Browsers are created with `SetAsWindowless(parent)`, `runtime_style = CEF_RUNTIME_STYLE_ALLOY`, `windowless_frame_rate` from settings and an opaque white background.

**GPU off.** `cpp/app.h` appends `disable-gpu` and `disable-gpu-compositing` in the browser process, with the cefclient comment about #1257. The only paint path is `OnPaint` (CPU BGRA). `OnAcceleratedPaint` is not implemented.

**Message pump.**
- `cpp/external_pump.cpp` is a port of cefclient's `MainMessageLoopExternalPump`, including `MAX_TIMER_DELAY = 1000/60` and reentrancy detection.
- `external_pump_linux.cpp` attaches a GLib `GSource` with a wakeup pipe to `g_main_context_default()`.
- GPUI on Linux does not iterate GLib, so that source never dispatches under GPUI. (INFERRED from GPUI's calloop loop; see `cef-osr.md` §7.)
- The GPUI example therefore pumps by hand (`examples/wef-example/src/main.rs`): on Linux only, a background `Timer::interval(16 ms)` feeds a `flume` channel, and the foreground loop calls `wef::do_message_work()` per tick. This is a fixed 60 Hz pump.
- `OnScheduleMessagePumpWork` is also forwarded to a Rust `ApplicationHandler::on_schedule_message_pump_work` callback, but the GPUI example ignores it.

**Frames into GPUI** (`crates/webview/src/browser_handler.rs` `on_paint`):
- Each paint builds `Arc<RenderImage::new([image::Frame::new(ImageBuffer::from_vec(w, h, bgra.to_vec()))])>`. CEF's BGRA is passed unchanged, because GPUI's `RenderImage` is BGRA ("A cached and processed image, in BGRA format", `gpui-0.2.2/src/assets.rs:41`).
- `FrameView` (`frame_view.rs`) keeps the last rendered image and calls `window.drop_image(old)` when a new frame replaces it, to free the atlas.
- `element.rs` paints with `window.paint_image(bounds, Corners::all(0), image, 0, false)`, sized `image_px / scale_factor`. It calls `browser.resize(physical size)` during paint, and `GetViewRect` returns physical / `device_scale_factor` (`cpp/client.h`).

**Popups.**
- `OnPopupShow` creates or drops a second `FrameView`. `OnPopupSize` stores the rectangle.
- The popup image is painted over the view at that rectangle, with two GPUI box shadows.
- `window.open` (`OnBeforePopup`) is blocked (returns true) and surfaced as `BeforePopupEvent { url }`.

**Cursor.** `OnCursorChange` maps `cef_cursor_type_t` to `gpui::CursorStyle` (`utils.rs`), then `window.set_cursor_style(style, hitbox)`. Custom (`CT_CUSTOM`) cursors are not handled.

**Mouse.**
- `window.on_mouse_event` for move, down and up within the element's bounds.
- Wheel: `ScrollDelta::Lines * 20` or pixels, sent to `SendMouseWheelEvent`.
- Click position is the last move position.
- Bugs: `std::max(click_count, 3)`, and moves always set `EVENTFLAG_LEFT_MOUSE_BUTTON`.

**Keyboard.**
- `on_key_down` and `on_key_up` map only 12 GPUI key names (backspace, delete, tab, enter, page up/down, home, end, arrows) to Windows VK codes.
- Then `native_key_code = windows_key_code` is sent, plus a CHAR event for Enter.
- Every other key, including letters, arrives only through GPUI's IME input handler.
- Modifiers: Shift, Control and Alt (no Meta).

**IME.**
- `WebView` implements `EntityInputHandler`:
  - `replace_text_in_range` → `ImeCommitText(text, InvalidRange, 0)`.
  - `replace_and_mark_text_in_range` → `ImeSetComposition(text, {}, InvalidRange, selection)`.
  - `bounds_for_range` → `None`.
- `OnImeCompositionRangeChanged` computes a union rectangle, but nothing uses it to place the candidate window.

**Focus.** GPUI `on_focus` and `on_blur` call `SetFocus(true/false)`. `OnLoadEnd` reapplies focus.

**Other host UI.**
- JS dialogs become gpui-component modals (alert, confirm, prompt).
- Context menu is built from `ContextMenuParams` and drawn with gpui-component.
- File dialog goes to a Rust callback.
- `OnBeforeUnloadDialog` auto-continues.
- Media permission is always denied (`CEF_MEDIA_PERMISSION_NONE`).

**JS bridge.** Uses `CefMessageRouter` with a `FuncRegistry` that injects JS from the render process on context creation of the main frame.

**Helper process.**
- `wef::launch` calls `exec_process()` (re-executing the same binary) on non-macOS before `init`.
- On macOS, `cargo wef build` creates the `.app` with framework and helper bundles (`add_helper.rs`).
- Issue #9: calling `wef::init()` without the `exec_process` check re-runs `main` recursively and "will result in crashing my PC". This is the CEF fork-bomb trap.

**Packaging on Linux.**
- `cargo wef add-framework` copies `libcef.so`, `libEGL.so`, `libGLESv2.so`, `libvk_swiftshader.so`, `libvulkan.so.1`, `v8_context_snapshot.bin`, `vk_swiftshader_icd.json`, the `.pak` files, `icudtl.dat` and `locales/` next to the binary (`add_cef_framework.rs`).
- No rpath is set in `build.rs` (`cargo:rustc-link-lib=cef` only), so the binary needs `LD_LIBRARY_PATH` or a manual rpath (INFERRED).

**Linux/Wayland reality.**
- The GPUI widget passes `window.window_handle()`. On GPUI 0.2.2 X11 that is `unimplemented!()` (panics). On Wayland, wef maps the non-XCB handle to a null parent (`builder.rs:154-159`), which is fine for OSR. (VERIFIED code, INFERRED behaviour.)
- Open issues: #14 ("Special keys aren't working on CEF", macOS) and #2 (video sites fail: FFmpeg "no supported streams", the proprietary-codec gap).

### 2.3 What to take from wef

- Take the overall shape of the GPUI element:
  - Image swap with `drop_image`.
  - Popup overlay.
  - The cursor table.
  - `EntityInputHandler` for IME.
  - Host-drawn dialogs and menus.
- Leave the rest:
  - The C++ layer (the `cef` crate covers it).
  - GPU-off defaults.
  - The fixed pump.
  - The key mapping.
- Its authors' verdict ("not good enough") is mostly about app size and developer experience, not about a technical dead end.

---

## 3. Zero-copy dmabuf on Linux: welding and bevy_cef (question 2)

### 3.1 welding (merely-made/wgpu-weld)

| Item | Value |
|---|---|
| Repo | https://github.com/merely-made/wgpu-weld; crate `welding` (https://crates.io/crates/welding), 0.14.1 published 2026-09-04, main at 0.15.0 |
| Last activity | 2026-10-13 (`b673f36`) |
| Licence | MPL-2.0 |
| CEF | Main targets `cef` 154.5.0+154.0.34, the **same crate version as Vsesvit**. Hardware receipts were measured on CEF 147 to 151. |
| Host | winit + wgpu 30 (rows for 28 and 29 compile but refuse Linux dmabuf import) |

**How it imports dmabufs** (VERIFIED, `welding/src/linux_cef/cef_backed.rs` `on_accelerated_paint`, `native_frame/vulkan_dmabuf.rs`, `README.md`):

1. Browser settings:
   - `WindowInfo { windowless_rendering_enabled: 1, shared_texture_enabled: 1, external_begin_frame_enabled: 0 }`.
   - `windowless_frame_rate: 60`.
   - One `RequestContext` per producer.
   - Created with `browser_host_create_browser_sync`.
2. In `on_accelerated_paint`:
   - Reject `plane_count == 0` or more than the array.
   - Map `CEF_COLOR_TYPE_RGBA_8888` to `Rgba8UnormSrgb`; anything else to `Bgra8UnormSrgb`.
   - `libc::dup` every plane fd.
   - Wrap them in `DmaBufImage::from_owned_raw_planes`, which deduplicates fds that refer to the same kernel object.
   - Size is `info.extra.coded_size`.
   - Store the image in a `PendingFrameSlot` (separate slots for `PET_POPUP` and the view).
3. Later, on the host's frame (`acquire_frame`), the importer runs. It delegates to the `grafting` 0.6 crate (https://github.com/merely-made/wgpu-graft, MPL-2.0), which:
   - creates a `VK_IMAGE_TILING_DRM_FORMAT_MODIFIER_EXT` image with the explicit modifier;
   - imports the memory;
   - acquires the image from `VK_QUEUE_FAMILY_FOREIGN_EXT`;
   - registers the result with wgpu 30.
4. Modifier policy:
   - Intel/Mesa supplies an explicit modifier.
   - AMD/RADV supplies `DRM_FORMAT_MOD_INVALID`, which welding imports as `DRM_FORMAT_MOD_LINEAR`.
   - That needs `VK_EXT_image_drm_format_modifier`, enabled by wgpu 30 as `Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF`. Create the device with `welding::build_dmabuf_capable_device`.
5. Pumping:
   - `external_message_pump` on Linux, `multi_threaded_message_loop` on Windows (`runtime.rs`).
   - The Linux demo uses winit `ControlFlow::Poll` and calls `do_message_loop_work()` every tick (`demo-weld-linux/src/main.rs:931`).
   - `on_schedule_message_pump_work` is not implemented.

**Contract deviation.**
- The import in step 3 happens after the callback has returned.
- CEF documents that the handle "cannot be accessed outside of this callback" and returns to a pool afterwards (`include/cef_render_handler.h`, 8037).
- `dup` keeps the memory alive but does not stop Chromium writing the next frame into it. (VERIFIED contract, INFERRED risk.)

**Verified hardware** (REPORTED by welding's README, 2026-08-12 to 2026-10):
- Linux: Fedora 44 on Intel/Mesa and AMD Renoir/RADV (Mesa 26.1.5).
- A trusted CI runner runs a "RADV native Wayland pixel fixture" with `WELD_SWITCHES: ozone-platform=wayland` and `WINIT_UNIX_BACKEND: wayland` (VERIFIED in `.github/workflows/hardware-headed.yml`). So Wayland-native OSR works on AMD.
- Feature matrix: mouse, wheel, keyboard, cursor, HiDPI, navigation, cookies, script results, popups (RADV), IME, permissions, context menus, CDP, find, zoom, print-to-PDF, drag and drop, touch and snapshots are verified on Linux.

**NVIDIA on Linux: not supported** (VERIFIED in code comments and docs):
- `demo-weld-linux/src/main.rs`: "NVIDIA proprietary and Wayland are not currently supported by the CEF Linux DMABUF path" (the Wayland part is outdated, see the CI job above). The adapter code says "NVIDIA's proprietary driver is known not to work with the CEF DMABUF path".
- The 2026-05-14 design doc lists "NVIDIA proprietary driver: not viable with DMABUF + Vulkan today", citing a 2024 comment in cef#3687.
- welding's only NVIDIA runner is Windows DX12. The real cause on CEF ≤ 154 is #4237 (section 7).

**Other findings worth knowing** (REPORTED, `welding/README.md`):
- **IME.** In the C API, `ImeCommitText`'s `replacement_range` must be non-null; libcef "returns early on NULL -- silently, in a release build".
  - The `cef` crate signature is `ime_commit_text(text: Option<&CefString>, replacement_range: Option<&Range>, ...)`, and `None` becomes null (VERIFIED in `cef-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs:13592`).
  - Always pass `Some(&Range { from: u32::MAX, to: u32::MAX })` (the invalid range).
- **DevTools.** "On CEF 151, opening DevTools for a windowless browser crashes the host process on Windows and Linux too". Linux crashed with a segmentation fault (exit 139). welding refuses the native window and exposes CDP instead.
- **Renderer crash.** "A renderer crash on Linux does not reach the host... `OnRenderProcessTerminated` never fires", even with `--no-zygote`.
- **Printing.** Linux CEF has no native print dialog; the embedder must implement `CefPrintHandler`.
- **Downloads.** "CEF silently discards a download whose destination has forward slashes" (observed on Windows).
- **Profiles.** "leave the final profile directory absent. CEF creates that directory itself; pre-creating it makes Chrome reject the named profile".
- **First run.** The Linux demo always adds `no-first-run` and `no-default-browser-check`, so a fresh profile cannot block inside `CefInitialize`. tauri hit the same thing (section 4.2).
- **API pinning.** It calls `cef::api_hash(CEF_API_VERSION_LAST, 0)` before any CEF call. Without it, libcef aborts with "called with invalid version -1" (`runtime.rs`).
- **Sandbox.** The Linux demo runs with `CefSandboxMode::Sandboxed` (`no_sandbox = 0`).

### 3.2 bevy_cef (not-elm/bevy_cef)

| Item | Value |
|---|---|
| Repo | https://github.com/not-elm/bevy_cef; crates `bevy_cef` and `bevy_cef_core` 0.14.0 (2026-10-15) |
| Licence | Apache-2.0 OR MIT |
| CEF | `=154.5.0+154.0.34` (same as Vsesvit); Bevy 0.20 |

**No dmabuf import on Linux** (VERIFIED, `crates/bevy_cef_core/src/browser_process/browsers.rs:105-150`, `accelerated_paint/mod.rs`):
- `shared_texture_enabled` is set only under `cfg(target_os = "macos")`.
- macOS imports the IOSurface into Metal and wgpu. The module states its own contract deviation (it keeps a `CFRetain` past the callback; "torn/newer content is possible under heavy load").
- Linux and Windows use CPU `on_paint` into a shared texture slot.
- The README says "CEF runs windowless on Linux, so X11, XWayland, and native Wayland sessions are all supported", for software paint (REPORTED).
- **NVIDIA:** not applicable on Linux, since there is no GPU import.

Other patterns (VERIFIED):
- **Message loop.** `src/common/message_loop.rs` uses `external_message_pump = true` on all platforms.
  - `on_schedule_message_pump_work` sends a timer over an mpsc channel.
  - A Bevy `Main` system runs `do_message_loop_work` when the timer expires, with a 4 ms minimum interval and a 30 Hz maximum-delay fallback.
- **Begin frames.** `EXTERNAL_BEGIN_FRAME = !cfg!(windows)`, so on Linux the host drives frames with `send_external_begin_frame()` every Bevy frame. On Windows, external begin frames plus DevTools "permanently stops the inspected webview's rAF and painting" (CEF 152.0.6).
- **Keyboard.** `browsers/keyboard.rs` is the most correct Linux mapping seen:
  - `to_linux_native_key_code` returns the evdev code + 8 (the XKB keycode).
  - Each press emits RAWKEYDOWN (drives DOM `keydown`), then CHAR for character keys, then KEYUP.
  - The macOS special cases are documented there.
- **Helper.** A dedicated `bevy_cef_render_process` binary is copied next to the executable by `build.rs`. The fallback is `early_exit_if_subprocess()`.
- **Zygote.** Default switches include `no-zygote` on Linux. "Without this Chromium tries to launch a zygote process on Linux even with `no_sandbox: true`, which fails with 'No such file or directory' in `ZygoteHostImpl` (see issue #9)". It is stripped when the sandbox is on (`command_line_config.rs`).
- **Signals.** `disable_signal_handlers` is false on Linux.
- **Command line.** A list of "risky" switches gets a warning (`disable-web-security` and others).
- **Edit commands on macOS.** `EditCommand` (Copy/Cut/Paste/SelectAll/Undo/Redo) is dispatched to the focused frame because "the windowless (OSR) browser has no real `NSView`... shortcuts like ⌘C never reach Blink's editor".

### 3.3 Answer to question 2

- **How they import dmabufs.**
  - welding dups the fds in the callback, then later imports them through Vulkan external memory with a DRM-modifier image and a foreign-queue acquire, via `grafting`.
  - bevy_cef does no dmabuf import on Linux.
- **NVIDIA.** Neither works on NVIDIA Linux.
  - welding explicitly excludes it.
  - bevy_cef never tries.
  - The real fix needs CEF 156 plus #4314 (section 7).

---

## 4. wew, tauri's CEF runtime, cef-rs, and other Rust + CEF projects (question 3)

### 4.1 wew (mycrl/wew)

- https://github.com/mycrl/wew. MIT. Last commit 2026-04-13. The `wew` 0.1.0 crate is from 2025-06-29. (VERIFIED.)
- CEF 137, own C++ glue (`cxx/*.cpp`). README: "platform supported: windows / macOS / linux (x11)".
- OSR uses `OnPaint` only. Windowed mode uses `SetAsChild` with an Xlib window (VERIFIED, `cxx/runtime.cpp`, `src/webview.rs`).
- `RawWindowHandle::Xlib` only on Linux, so no Wayland.
- Message-loop modes are selectable: multi-threaded, external pump, or CEF-owned (`src/runtime.rs:360-415`).
- Packaging is "extensive manual operations"; Cargo cannot do it (README).
- Not useful beyond confirming the basics.

### 4.2 tauri-runtime-cef (Tauri 3 alpha)

| Item | Value |
|---|---|
| Crate | https://crates.io/crates/tauri-runtime-cef 3.0.0-alpha.6 (2026-10-14); source on branch `feat/cef` of https://github.com/tauri-apps/tauri |
| Licence | Apache-2.0 OR MIT |
| CEF | `cef = "=154.5.0"` with features `build-util` and `linux-x11` |
| Linux windowing | GTK4 via `tauri-winit` (`winit` 0.31.0-beta.3, feature `gtk4`); X11 only |

VERIFIED from the crate source:

- **Forces X11** (`src/runtime.rs:3316-3341`):
  - Pushes `--ozone-platform=x11` for every process type ("getting it wrong there breaks rendering outright").
  - Sets `GDK_BACKEND=x11` and calls `gdk::set_allowed_backends("x11")`, because "CEF integration below uses XIDs for child windows/reparenting".
- **Windowed CEF children under a dedicated X11 host window.**
  - `CefX11Host` uses a 24-bit TrueColor visual, because "CEF does not render correctly with the inherited GTK window visual on all X11 setups".
  - The host is positioned over GTK's content box. Browsers are reparented with `XReparentWindow` and moved with `XMoveResizeWindow` (`src/platform/linux/window.rs`, `webview.rs`).
  - This is the X11-only Chrome-style path, where `runtime_style` defaults to CEF's own default.
- **Optional OSR "offscreen" webviews.** In `src/offscreen.rs`, the application supplies a `RenderHandler` factory and owns painting, input and IME.
  - "Offscreen webviews use Alloy style and cannot be reparented."
  - The docstring repeats the contract: "Open and copy them during the callback, respecting CEF's resource lifetime."
- **External pump** (`src/external_message_pump/linux.rs`): a faithful port of cefclient's `main_message_loop_external_pump_linux.cc`. It is a low-priority GSource on GLib's default context with a wakeup pipe and `K_MAX_TIMER_DELAY = 1000/30`.
  - Related open issue: tauri#16189 (2026-10-01) reports the idle main thread spinning at 100% CPU. Chromium's `MessagePumpGlib` work source on the default context returns timeout 0 under `external_message_pump`, so `g_main_context_iteration(TRUE)` never blocks.
- **Switch hygiene** (`src/switches.rs`):
  - "CEF fills `--disable-features` before it calls `on_before_command_line_processing`... an application that appends its own `--disable-features` silently drops all of it." It merges `disable-features`, `enable-features` and `js-flags`.
  - It warns about the dangerous switches from Chrome's `bad_flags_prompt.cc` list.
- **Chrome command filtering** (`src/cef_impl/client/command.rs`): "A Chrome style browser keeps its whole accelerator table live even when it is hosted as a child view with no browser UI, so Ctrl+N opens a real Chrome window next to the app's and Ctrl+T a tab in a window the app does not own." It blocks `IDC_NEW_WINDOW`, `IDC_NEW_TAB` and similar unless the app opts in.
- **No extensions.** `browser_extensions_enabled` and `extensions_path` produce a warning: "CEF removed its extension loading API along with the Alloy runtime" (`src/webview.rs:169-251`).
- **Other fixes.**
  - `--no-first-run`. CHANGELOG: "Fixed CEF failing to start on Linux with `ContentMainRun failed with exit code 28`".
  - `--password-store=basic` on Linux release builds.
  - Profile downgrade handling before `cef::initialize` (`downgrade.rs`).
  - Per-webview user agent served through CDP, because "CEF has no per-browser user agent".

### 4.3 cef-rs examples (tauri-apps/cef-rs)

- https://github.com/tauri-apps/cef-rs (branch `dev`, 480 stars, Apache-2.0 OR MIT, last push 2026-10-13). (VERIFIED.)
- `examples/osr` is winit + wgpu 30 and forces Vulkan on Linux.
- `cef/src/osr_texture_import/dmabuf.rs` (feature `accelerated_osr`) is the reference importer. It is demo-quality (VERIFIED):
  - It allocates a new `VkImage` and `VkDeviceMemory` per frame and never frees them.
  - It has no `VkExternalMemoryImageCreateInfo` and no foreign-queue transfer or fence.
  - It reads plane 0 only.
  - `cef-osr.md` §Executive summary, item 4, lists the full set of defects.
- cef-rs issue #207 ("Implement off-screen renderer sample for Linux with wgpu", closed 2025-10-11) points to Graphite's desktop code as the working Linux implementation.
- cef-rs issue #277 ("Implement external message pump", closed 2026-01-01), maintainer `csmoe`:
  - "Windows and Linux can use the option 2 [multi_threaded_message_loop], it works great."
  - "mac can only choose option 3 [external pump], this is really hard to get it right".
  - Later comments propose running CEF in a child process and sharing GPU handles across the process boundary, citing OpenAI Atlas's architecture (https://openai.com/index/building-chatgpt-atlas/, cited there; I did not read it). A proof of concept followed for macOS with `CALayerHost`.

### 4.4 Graphite editor desktop (production, Linux dmabuf done right)

| Item | Value |
|---|---|
| Repo | https://github.com/GraphiteEditor/Graphite, `desktop/` (27.6k stars, Apache-2.0, pushed 2026-10-16) |
| Approach | winit + wgpu host; CEF runs in a **separate host process** (`desktop/ui/src/remote/`) and talks to the main process over `ipc-channel`. Frames cross as shared memory (CPU) or dmabuf fds (GPU). |

VERIFIED from code:
- **CEF settings** (`ui/src/context.rs:150-176`):
  - `multi_threaded_message_loop: 1` on Linux and Windows.
  - On macOS, CEF's own loop runs in the host process.
  - `no_sandbox: 1` on Linux.
- **GPU switches** (`ui/src/internal/browser_process_app.rs:50-87`):
  - `use-angle=gl-egl`.
  - `--ozone-platform=wayland` when `WAYLAND_DISPLAY` is set. It does **not** force X11, unlike cefclient on 8037.
  - `enable-gpu`, `enable-gpu-compositing`, `enable-begin-frame-scheduling`, `off-screen-rendering-enabled`.
  - `--disable-features` is read and merged, never replaced.
  - Without acceleration: `disable-gpu` and `disable-gpu-compositing`.
- **Frame transport** (`ui/src/frames/plane/linux.rs`):
  - The host dups CEF's plane fds inside `on_accelerated_paint`.
  - It sends a `FrameDescriptor` (seq, modifier, width, height, format, strides, offsets, content rect) plus the fds over an `AF_UNIX SOCK_SEQPACKET` socketpair with `SCM_RIGHTS`.
- **Contract kept** (`frames/streamer.rs`, `frames/sequence.rs`, `frames/surface.rs`):
  - `publish()` runs inside `on_accelerated_paint` (`internal/render_handler.rs:58-59`). It sends the frame, then `claim.wait_for_ack()`, with `FRAME_ACK_TIMEOUT` = 250 ms.
  - The main process imports the dmabuf through Vulkan (`frames/import/dmabuf.rs`: `ExternalMemoryImageCreateInfo`, `get_memory_fd_properties`, dedicated memory, DRM modifier).
  - It then `copy_texture_to_texture`s into an owned texture, or resamples into one when the content rect differs, and `device.poll(Wait)`s before acking.
  - Fully transparent frames are skipped through a readback "blank check".
- **NVIDIA** (`ui/src/platform.rs`): logs "NVIDIA proprietary driver detected, hardware acceleration may be unstable" and continues.
- **Why it matters for Vsesvit.** This is the most complete, contract-correct Linux implementation seen.
  - The separate CEF process also gives crash isolation (a CEF browser-process crash does not kill the UI).
  - It removes all CEF message-loop integration from the UI thread.
  - That is how OpenAI Atlas is described too (INFERRED from the cef-rs #277 discussion).

### 4.5 spatial-browser (CEF 154 + wgpu, Linux only)

| Item | Value |
|---|---|
| Repo | https://github.com/Loafer19/spatial-browser (MIT, last push 2026-10-12); wiki https://github.com/Loafer19/spatial-browser/wiki |
| CEF | 154.5.0+154.0.34, windowless Alloy; winit + wgpu |

VERIFIED from code and wiki:
- **GPU and CPU modes.** GPU shared-texture OSR is the default. `SPATIAL_BROWSER_OSR=cpu` falls back to CPU. `external_begin_frame_enabled = true` with a frame pacer, and an idle canvas draws nothing.
- **Hybrid GPUs** (`crates/compositor/src/output/display.rs`): wgpu prefers `LowPower` "so wgpu lands on the same GPU CEF's GPU process typically uses... DMA-BUF shared-texture import then fails across devices". "Auto turns it off on the discrete GPU".
- **Wiki Limitations, verbatim points:**
  - "No Chrome Web Store / extensions": Alloy OSR has no extension path.
  - "SPA soft-navigation crash". `ReadAnythingSoftNavigationObserver` assumes a real tab (CEF #4234, fixed in M152), but they keep `disable-features=ImmersiveReadAnything` and still see crashes. `scripts/run.sh` auto-relaunches.
  - "CEF's own clipboard does not work in this OSR embedding. Copy/paste are reimplemented (`wl-copy` / `wl-paste` + injected bridge / `Ctrl+V` intercept)". Plausible causes: Wayland clipboard needs a focused surface's input serial, and the GLib sources are never iterated (INFERRED; see `cef-osr.md` §7).
  - No H.264/AAC: Twitch shows "Error #4000". Fixing it needs a custom CEF build.
- **Crash handling** (Architecture wiki): `--disable-gpu-process-crash-limit` ("GPU process isn't usable. Goodbye." otherwise). Per-tile renderer-crash reload with backoff.

### 4.6 Other Rust + CEF projects seen (2025-2026)

Found with a GitHub code search for `on_accelerated_paint` in Rust (2026-10-16):

| Project | Stars / licence / last push | Linux approach | Note |
|---|---|---|---|
| tobagin/karere (https://github.com/tobagin/karere) | 215 / GPL-3.0 / 2026-09-30 | GTK4 + libadwaita + CEF 152.3, OSR into a `GtkGLArea`; EGL dmabuf import (`src/gl_dmabuf.rs`) | Disables GPU OSR on NVIDIA: "NVIDIA can't back CEF's exportable shared images — the GPU process fails SkSurface init and no accelerated frame ever arrives, leaving the chat area black (#167)". Probes `GL_VENDOR` before browser creation, because CEF may never fall back to `on_paint` (VERIFIED `src/web_view.rs:2975-3033`). Wayland-first; Flatpak with `--no-sandbox` (PRD). |
| makepad/makepad `libs/cef` (https://github.com/makepad/makepad/blob/dev/libs/cef/src/native.rs) | 7.2k / MIT / 2026-10-14 | Linux: software paint only (`accelerated_paint_available() == false`), `--ozone-platform=headless`, `--use-angle=vulkan` | "Windowless Linux also runs without an X or Wayland display. ANGLE's GL default requires an X display even with the headless Ozone platform." (VERIFIED.) |
| dsh0416/godot-cef (https://github.com/dsh0416/godot-cef) | 234 / MIT / 2026-10-12 | Vulkan dmabuf import into Godot via a `vkCreateDevice` hook | Docs: NVIDIA needs `nvidia-drm.modeset=1`. Copies during the callback, uses a sync-file semaphore and a foreign-queue release before returning, and pins CEF's GPU with `--gpu-vendor-id`/`--gpu-device-id` (https://godotcef.org/api/vulkan-support.html, https://godotcef.org/api/accelerated-handoff.html). |
| csmoe/iced-webview | 16 / none / 2026-04-28 | iced + CEF | Not examined. |
| water-rs/waterui `components/platform/browser-cef` | 522 / Apache-2.0 / 2026-10-16 | Has `gpu/linux.rs` | Not examined. |
| software-mansion/smelter, jellium-desktop, obs-rs and others | various | OSR for media pipelines | Not examined. |

---

## 5. infiniterm (question 4)

| Item | Value |
|---|---|
| Repo | https://github.com/ekinertac/infiniterm (last push 2026-10-16) |
| Licence | **Source-available, not open source** (`LICENSE.txt`: personal use; "You cannot give the software to anyone else, in source or compiled form"). Learn from it, do not copy it. |
| Platform | macOS only ("Terminal cards on an infinite canvas for macOS") |
| Stack | `gpui = "0.2.2"` (crates.io), `cef = "152.3.0"`; crate `infiniterm-browser` has no GPUI dependency, and `infiniterm-ui` turns frames into textures |

### 5.1 How it embeds CEF in GPUI (VERIFIED)

- **Process** (`infiniterm-browser/src/process.rs`):
  - `early()` runs before GPUI: it loads the framework, calls `api_hash`, then `execute_process`.
  - `Process::start()` runs inside GPUI's run closure, after installing `CefAppProtocol` methods on GPUI's `NSApplication` (`app_protocol.rs`).
  - `Settings { windowless_rendering_enabled: 1, external_message_pump: 1, cache_path, log_file, remote_debugging_port }`.
  - Switches: `no-startup-window`, `noerrdialogs`, `use-mock-keychain`, `--load-extension=<comma-separated dirs>`, and `--remote-allow-origins=http://127.0.0.1:<port>`.
- **Pump** (`infiniterm-ui/src/main.rs:419-428`): `cx.spawn(loop { pump(); background_executor().timer(4 ms).await })`. It is a fixed 4 ms tick; `on_schedule_message_pump_work` is ignored.
- **Frames** (`surface.rs`, `browser_body.rs`):
  - CPU `on_paint` (view only) into an `Arc<Frame { bgra }>` in a shared slot with a dirty flag. The UI builds a `RenderImage` when the frame changed.
  - It explicitly `drop_image`s the previous texture, because "gpui has no atlas LRU... without this an open browser card grows the atlas by one tile per CEF frame".
  - Spike measurement: "the frame is a memcpy: 0.7 to 1.5 ms for 1024x768", and "a 4K card would want `on_accelerated_paint` (IOSurface)... but gpui has no public texture path for" it (`spikes/cef-frame/NOTES.md`).
- **Popups.** `on_before_popup` queues the URL ("A popup (window.open, a target=_blank link, the extension's chrome.windows.create) becomes a card, never a native window") and returns 1.
- **Cursors** via `on_cursor_change`. Context menus are a slim host menu built from `ContextMenuRequest`. Find goes through `OnFindResult`.
- **Keys.** "typed keys are forwarded as CHAR events, the editing keys as RAWKEYDOWN/KEYUP with Windows virtual key codes". Cmd+V had to be forwarded as `frame.paste()` (NOTES).
- **Google sign-in** (`moat.rs`): `Page.enable`, then `Emulation.setUserAgentOverride` with `userAgentMetadata` brands (`Not)A;Brand`, `Chromium`, `Google Chrome`), then `Page.addScriptToEvaluateOnNewDocument` stubbing `window.chrome.app/csi/loadTimes`. All are sent over `send_dev_tools_message` while on `about:blank`, before the first navigation. Verified: "he signed in to his Google account inside this window".
- **Extensions.**
  - The Claude in Chrome extension loads unpacked through `--load-extension`. It keeps its store id because the manifest carries `key`.
  - Native messaging manifests must be in `<cache-path>/NativeMessagingHosts/`: "CEF looks for host manifests in `<cache-path>/NativeMessagingHosts/`, not in `~/Library/Application Support/Google/Chrome/`" (`spikes/cef-extension/NOTES.md`).
  - Its MCP tools work in Alloy windowless mode (cefclient `--off-screen-rendering-enabled --use-alloy-style`).
- **Traps listed in the NOTES:**
  - Two processes on one cache path trigger Chrome's singleton hand-off ("initialize returns 0").
  - Create the browser inside `open_window`'s builder, because the scale factor is needed at creation.
  - GPUI and `cef` both glob-export `App`, `Window`, `Point` and `MouseEvent`.

### 5.2 Issue #310 and the CEF-fork plan (VERIFIED)

- **Issue #310** (https://github.com/ekinertac/infiniterm/issues/310, open, 2026-10-08), "Browser extensions, DevTools and video codecs via our own CEF build". Measured on CEF 152, Alloy, windowless:
  - `chrome.tabs.query` and `chrome.windows.getAll` return nothing.
  - `windows.create` and `tabs.create` open a native window.
  - `chrome.action` state is stored but nothing draws it.
  - H.264, H.265 and AAC do not play.
  - No DevTools.
  - "CEF upstream closed the related issues as not planned" (#4011 and #4255, both VERIFIED closed NOT_PLANNED).
- **Spec PR #311** (https://github.com/ekinertac/infiniterm/pull/311, open; file `docs/superpowers/specs/2026-10-09-browser-extensions-cef-fork-design.md`, copy in `/tmp/vsesvit-research/prior-art-infiniterm-repo/spec.md`). Further probe results from the spec's table:
  - Works: `tabs.get(id)`, `scripting.executeScript({tabId})` and `webNavigation` events, but `active` is always false and `windowId` -1.
  - Never fire: `tabs.onCreated/onUpdated/onActivated`.
  - Work: `bookmarks`, `history`, `topSites`, `sessions`, `cookies`, `management`, `declarativeNetRequest`, `commands.getAll`, `identity`, `downloads.search`.
  - Not supported: Widevine and the platform authenticator.
  - The patch plan is the P1 to P5 list in §1.1.
  - "DevTools windows always use the Chrome style path".
  - Risks: "Alloy style may be removed upstream"; a ~100-150 GB build disk; patch rebases on every security update.
- **Patches PR #333** (https://github.com/ekinertac/infiniterm/pull/333, open, "drafts, not compiled"; `tools/cef/patches/NOTES.md`). Drafted against CEF `708dc140c` / Chromium 152.0.7977.83:
  - P1 adds `CefBrowserHost::SetExtensionTabInfo(int window_id, bool active)` and `static SetExtensionFocusedWindow(int)`. A UI-thread registry gives each `WebContents` a `TabEntry` and sets the `SessionTabHelper` window id. A per-window `extensions::WindowController` registers in `WindowControllerList`. Events go out through new `TabsEventRouter` wrappers.
  - P2 routes `windows.create` and `tabs.create` to a client handler.
  - "All four were applied in order to a scratch copy of the original files with no conflict"; never built.
- **DevTools PR #332** (merged): `ShowDevTools` with a windowless client "opens a real native window" ("Windowless rendering is not supported for this DevTools window"). Instead, a normal card opens `http://127.0.0.1:<port>/devtools/devtools_app.html?ws=127.0.0.1:<port>/devtools/page/<id>`. The target id comes from `Page.getFrameTree` after each load. The port is random and on loopback, with `--remote-allow-origins` set to only that origin.
- **Codecs:** "Build with `proprietary_codecs=true` and `ffmpeg_branding=Chrome`" (P5). Patent licensing is the distributor's risk.

### 5.3 What Vsesvit takes from infiniterm

- It is the closest prior art for "GPUI chrome + CEF OSR + real Chrome extensions".
- Its measurements are the best available inventory of which `chrome.*` APIs work in Alloy OSR.
- Its patch design (window id per GPUI window, active-tab flag, no native windows) is a template, but unbuilt.
- Its probe extensions (`spikes/ext-probe/` on the PR branch) are a ready-made regression idea for Vsesvit's self-test. Rewrite them; do not copy, for licence reasons.

---

## 6. GPUI-based browser and webview projects (question 5)

| Project | Engine / OS | Approach | Status (VERIFIED dates) |
|---|---|---|---|
| **Glass v1** (https://github.com/Glass-HQ/Glass-v1) | CEF 145 via cef-rs tag `cef-v145.6.1+145.0.28`; macOS + Windows | Zed fork with a full browser crate (`crates/browser`: tabs, omnibox, bookmarks, history, downloads, context menus, find, permissions). macOS: IOSurface → `CVPixelBuffer` → GPUI `surface()`. `glass_helper` binary. `script/build-cef` builds CEF with proprietary codecs (branch 7632). | Archived 2026-10-06; v2 is a closed rebuild. GPL-3.0. No extension support found (grep). OAuth popups become **real native windows** (`life_span_handler.rs`: `windowless_rendering_enabled = 0`). UA forced to Chrome 145 stable "or Google's sign-in endpoints reject unrecognized browser versions". |
| **Glass-HQ/gpui** (https://github.com/Glass-HQ/gpui) | GPUI fork | Splits platform crates; adds `Keystroke::native_key_code` (always `None` on Linux in HodlOg's vendored copy). | 143 stars, pushed 2026-05-16. |
| **musichen/qedit** (https://github.com/musichen/qedit) | Glass fork | Same `crates/browser` as Glass, on `Glass-HQ/gpui` rev `3790fca`. | Pushed 2026-08-26. GPL-3.0. |
| **HodlOg/gpui-browser** (https://github.com/HodlOg/gpui-browser) | CEF 145.6.1 (cef-rs git tag); **Linux** | Vendored Glass GPUI with **`DmaBufFrame` / `PixelBuffer` surface sources** (`gpui/src/elements/surface.rs`, `scene.rs`, `window.rs`). Zero-copy Vulkan import in `gpui_wgpu` (`try_dmabuf_zc`, extensions enabled through wgpu-hal `open_with_callback`), with an mmap+upload fallback. Compositor GPU detection through `zwp_linux_dmabuf_feedback_v1` and DRI3. `debug_dmabuf` harness. | One commit, 2026-04-22. Apache-2.0. Known gaps (README): NVIDIA "known-brittle... CEF falls back to the CPU on_paint path"; keyboard puts X11 keycodes in `windows_key_code`; no focus handle; 4 ms pump. |
| **bokuweb/gpui-cef** (https://github.com/bokuweb/gpui-cef) | CEF 151 on macOS; WebView2 (Wry) on Windows | OSR → `RenderImage` copy by default. The zero-copy path is blocked because GPUI's `surface()` "hard-codes NV12". Measured "~38% while the page repaints continuously at 60fps" (debug, 994x786). IME through `EntityInputHandler`; only RAWKEYDOWN is sent. Pumps with its own `CFRunLoopTimer`, because GPUI's idle foreground executor ticked "one tick every one to five seconds". | v0.1.1, 2026-09-26. Used by bokuweb/ginka (PR #61, merged 2026-09-26). |
| **longbridge/wef** `crates/webview` | CEF 137, OSR CPU | Section 2. | Experimental, dormant since 2025-10. |
| **gpui-kit `gpui-webview`** | Wry (WKWebView / WebView2 / WebKitGTK X11) | Native child plus GPUI Fast composition. | 0.7.3, active (2026-10-16). |
| **longbridge/gpui-fast** (https://github.com/longbridge/gpui-fast) | GPUI fork (Apache-2.0, 245 stars) | "Window composition" from zed#62379. Linux Wayland: native and external content in desynchronized `wl_subsurface`s, GPUI overlays in synchronized subsurfaces. X11: child windows with SHAPE cutouts. API: `WindowComposition::create_native_surface`, an `ExternalGpu` surface kind ("A surface whose content is supplied by an external GPU producer"), and `on_input` ("Registers a cooperating windowless producer's mouse/wheel consumer") (`crates/gpui/src/fast/composition.rs`). | 0.1.8, 2026-10-14. Tracks Zed `a1b71072e5`. |
| **zed-industries/zed PR #62379** (https://github.com/zed-industries/zed/pull/62379) | GPUI | The upstream proposal for composition. "A real Linux WebView (WPE WebKit, or CEF off-screen rendering) can plug into the same native surface API later." "Tested on Hyprland (Wayland, 1.6× fractional scale, NVIDIA) and XWayland." | Open (updated 2026-10-15). Related: #61291 (BGRA `CVPixelBuffer` surfaces on macOS, open), #61945 (closed alternative), #52447 (Wry browser tab; "Wayland support would require GPUI to expose subsurface support", closed). |
| **infiniterm** | CEF 152, macOS | Section 5. | Active. |
| worldbreakstudios/pwrde PRs #133/#134; monarchmaisuriya/zedd PR #16 | CEF 154 OSR or windowed in GPUI (per search snippets) | Not verifiable: the repositories return 404 or are private as of 2026-10-16. | Unverified. |

No GPUI browser of any kind runs on Linux today (VERIFIED as far as this survey reaches).

---

## 7. NVIDIA on Linux: the cross-project picture (bears directly on this RTX 5080 machine)

VERIFIED from CEF issues, commits and code:

- **CEF 154 (branch 8037, head `14c5a089e8`, 2026-10-04).** `CefVideoConsumerOSR::SetActive` asks for `kPreferMappableSharedImage` when `use_shared_texture_` (`libcef/browser/osr/video_consumer_osr.cc:49-59`). On Linux that means `SCANOUT_CPU_READ_WRITE` / `GBM_BO_USE_LINEAR`, which "breaks GPU rendering on NVIDIA" (SkSurface/SharedImage write failure). This is CEF issue #4237.
- **Fix.** PR #4238, merged 2026-10-09 to master and branch 8078 (CEF 156). It switches to `kPreferSharedImageWithNativeHandle` and carries a Chromium patch until Chromium CL 8220427 is in (it is in M156). It was **not** merged to 8037.
- **Regression on top of the fix.** CEF #4313 (opened 2026-10-16, RTX 3060, open driver 615.78.08): with native-handle images, "OnAcceleratedPaint doesn't pass a fence... NVIDIA's driver doesn't [attach implicit fences], so the client copies frames that are still being drawn". PR #4314 (open) imports Chromium's write fence into each plane with `DMA_BUF_IOCTL_IMPORT_SYNC_FILE`.
- **Older NVIDIA fix in Chromium.** CL 6681354 ("[NvidiaLinux] Use BufferUsage::SCANOUT for GMB video frames to support NVIDIA drivers", merged 2025-08-06) is in Chromium 154.
- **cefclient's Linux switch defaults.**
  - On 8037 it forces `use-angle=gl-egl` and `ozone-platform=x11` for OSR shared textures (`tests/shared/browser/client_app_browser.cc:75-97`). It explains that without EGL, "The Chromium fallback path uses X11 pixmaps, which are only supported by Mesa drivers".
  - On master (`a875f40`) only `use-angle=gl-egl` remains, because master's cefclient gained Wayland OSR (commits `c7e1a9e5c6` "osr: linux: cefclient: Add OnAcceleratedPaint support (fixes #3687)" and `f922057255`, 2026-10-09/10). That uses a shared GL 3.3 renderer, `GtkGLArea` on Wayland and an EGL window surface on X11.
  - These are cefclient choices, not libcef defaults: libcef 8037 contains no `use-angle` or `ozone` default (grep). The embedder must set them.
- **Project experience:**
  - welding: excluded.
  - karere: disabled, "#167".
  - HodlOg: "known-brittle".
  - Graphite: warns.
  - godot-cef: claims it works with `nvidia-drm.modeset=1`. That parameter is a prerequisite for NVIDIA dmabuf/GBM. Whether it is already set on this Fedora 44 machine was not checked here (INFERRED: recent NVIDIA drivers default to modeset=1).
- **Consequence for Vsesvit** (INFERRED, agrees with `cef-osr.md`):
  - On CEF 154, plan on CPU `OnPaint` on this machine.
  - Treat the dmabuf path as an optimization for Mesa GPUs now, and for NVIDIA once CEF ≥ 156 with #4314 and a matching `cef` crate release exist.
  - Probe before creating browsers (karere's lesson), because there is no automatic fallback.

---

## 8. Message-loop strategies seen, and a recommendation

| Strategy | Used by (VERIFIED) | Pros | Cons |
|---|---|---|---|
| Fixed timer calling `do_message_loop_work` | wef/GPUI Linux (16 ms), infiniterm (4 ms), HodlOg (0.5-4 ms adaptive, 30 Hz idle) | Trivial | CPU when idle, latency when busy. GLib sources never run (`cef-osr.md` §7). |
| `on_schedule_message_pump_work` driving a timer | bevy_cef, spatial-browser (`message_pump::schedule`), cefclient/tauri port | Matches CEF's intent | Under GPUI the timer must wake the foreground; GPUI does not iterate GLib. Spins at 100% when combined with a GLib loop (tauri#16189). |
| `multi_threaded_message_loop` | Graphite (Linux, Windows), welding (Windows), wew (option) | No integration with GPUI's loop; CEF gets its own GLib context | Every callback is on CEF's UI thread, so marshal to GPUI. `OnAcceleratedPaint` copies must run on CEF's thread against a shared wgpu `Device` (`Send + Sync`). Default GTK dialogs are cancelled (`cef-osr.md` §7). |
| Separate CEF host process | Graphite (all platforms), OpenAI Atlas (per cef-rs #277) | Crash isolation, no loop coupling, clean contract-keeping frame handoff | Doubles IPC: input, navigation and every handler result cross processes. dmabufs must be sent with `SCM_RIGHTS`. |

**Recommendation** (INFERRED):
- Start with `multi_threaded_message_loop = true`. Marshal to GPUI through a channel plus a foreground wake.
- Graphite proves the pattern on Linux, and the cef-rs maintainer endorses it for Linux.
- Keep the separate-process architecture as a later option, if crash isolation of the CEF browser process becomes a requirement. spatial-browser's restart loop shows the cost of not having it.

---

## 9. Input checklist distilled from the projects

VERIFIED sources are named.

- **Key events.**
  - Per key press: RAWKEYDOWN, then CHAR (if it produces a character and IME is not composing), then KEYUP (bevy_cef `keyboard.rs`).
  - `windows_key_code` is the layout-independent VKEY.
  - `native_key_code` is the XKB keycode (evdev + 8) on Linux.
  - GPUI must be patched to expose the keycode (wef, HodlOg and Glass all lack it).
- **Text and IME.**
  - Text from IME goes through `EntityInputHandler`: `replace_and_mark_text_in_range` → `ImeSetComposition`, `replace_text_in_range` → `ImeCommitText`, `unmark_text` → `ImeFinishComposingText` (gpui-cef `ime.rs`).
  - Pass a non-null invalid range (welding).
  - Use `OnImeCompositionRangeChanged` rectangles for `bounds_for_range` (gpui-cef does; wef computes them but never uses them).
- **Mouse.**
  - Send the real click count.
  - Set button-down modifier flags only while a button is held (wef bugs).
  - Convert wheel lines to pixels.
  - Forward moves while dragging outside bounds (wef's `event.dragging()` check).
- **Editing shortcuts.** On macOS, OSR needs Copy/Cut/Paste dispatched as frame commands (bevy_cef, infiniterm). On Linux, check that CEF's keybinding path handles Ctrl+C/V. If it does not, map them to `frame.copy()`/`paste()` (INFERRED).
- **Focus.** GPUI `on_focus` and `on_blur` → `SetFocus`; reapply after load (wef). Infiniterm and HodlOg list missing focus handling as a source of lost keys.
- **Cursors.** Map `OnCursorChange`, and render `CT_CUSTOM` from `CursorInfo` (gpui-cef and wef fall back to arrow).

---

## 10. Sources

Projects (all fetched 2026-10-16):
- longbridge/wef: https://github.com/longbridge/wef ; issues https://github.com/longbridge/wef/issues/2 , https://github.com/longbridge/wef/issues/9 , https://github.com/longbridge/wef/issues/14 ; crate https://crates.io/crates/wef
- gpui-kit webview: https://github.com/longbridge/gpui-kit/tree/main/crates/webview
- gpui-fast: https://github.com/longbridge/gpui-fast ; https://crates.io/crates/gpui-fast
- Zed PRs: https://github.com/zed-industries/zed/pull/62379 , https://github.com/zed-industries/zed/pull/61291 , https://github.com/zed-industries/zed/pull/61945 , https://github.com/zed-industries/zed/pull/52447
- welding: https://github.com/merely-made/wgpu-weld ; https://crates.io/crates/welding ; grafting https://github.com/merely-made/wgpu-graft
- bevy_cef: https://github.com/not-elm/bevy_cef ; https://crates.io/crates/bevy_cef
- wew: https://github.com/mycrl/wew
- cef-rs: https://github.com/tauri-apps/cef-rs ; issues https://github.com/tauri-apps/cef-rs/issues/207 , https://github.com/tauri-apps/cef-rs/issues/277
- tauri-runtime-cef: https://crates.io/crates/tauri-runtime-cef ; https://github.com/tauri-apps/tauri/tree/feat/cef/crates/tauri-runtime-cef ; issue https://github.com/tauri-apps/tauri/issues/16189
- Graphite desktop: https://github.com/GraphiteEditor/Graphite/tree/master/desktop
- spatial-browser: https://github.com/Loafer19/spatial-browser ; https://github.com/Loafer19/spatial-browser/wiki/Limitations ; https://github.com/Loafer19/spatial-browser/wiki/Architecture
- infiniterm: https://github.com/ekinertac/infiniterm ; https://github.com/ekinertac/infiniterm/issues/310 ; https://github.com/ekinertac/infiniterm/pull/311 ; https://github.com/ekinertac/infiniterm/pull/333 ; https://github.com/ekinertac/infiniterm/pull/332
- Glass: https://github.com/Glass-HQ/Glass-v1 ; https://github.com/Glass-HQ/gpui ; qedit https://github.com/musichen/qedit
- HodlOg/gpui-browser: https://github.com/HodlOg/gpui-browser
- gpui-cef: https://github.com/bokuweb/gpui-cef ; ginka PR https://github.com/bokuweb/ginka/pull/61
- karere: https://github.com/tobagin/karere (files `src/gl_dmabuf.rs`, `src/web_view.rs`, `PRD-gtk-cef-shell.md`)
- makepad CEF: https://github.com/makepad/makepad/blob/dev/libs/cef/src/native.rs
- godot-cef: https://github.com/dsh0416/godot-cef ; https://godotcef.org/api/vulkan-support.html ; https://godotcef.org/api/accelerated-handoff.html
- Search snippets only (repos 404/private): https://github.com/worldbreakstudios/pwrde/pull/133 , https://github.com/worldbreakstudios/pwrde/pull/134 , https://github.com/monarchmaisuriya/zedd/pull/16

CEF and Chromium (primary):
- CEF issues: https://github.com/chromiumembedded/cef/issues/3687 , https://github.com/chromiumembedded/cef/issues/4237 (PR https://github.com/chromiumembedded/cef/pull/4238), https://github.com/chromiumembedded/cef/issues/4313 (PR https://github.com/chromiumembedded/cef/pull/4314), https://github.com/chromiumembedded/cef/issues/3953 , https://github.com/chromiumembedded/cef/issues/4011 , https://github.com/chromiumembedded/cef/issues/4255 , https://github.com/chromiumembedded/cef/issues/4234
- CEF commits: https://github.com/chromiumembedded/cef/commit/c7e1a9e5c6 , https://github.com/chromiumembedded/cef/commit/f922057255 , https://github.com/chromiumembedded/cef/commit/c5b06184bd , https://github.com/chromiumembedded/cef/commit/cf5fddcb6dbff93d2d253deda8243aa37bbc39bb ; branch 8037 head https://github.com/chromiumembedded/cef/commit/14c5a089e8
- CEF headers read locally at branch 8037 (`/tmp/vsesvit-research/cef/cef-8037`): `include/cef_render_handler.h` (OnAcceleratedPaint contract), `include/internal/cef_types_runtime.h` (windowless is always Alloy), `include/internal/cef_types_linux.h` (`cef_accelerated_paint_info_t`), `libcef/browser/osr/video_consumer_osr.cc`, `tests/shared/browser/client_app_browser.cc`; master at `a875f40`
- Chromium CLs: https://chromium-review.googlesource.com/c/chromium/src/+/6681354 (merged 2025-08-06), https://chromium-review.googlesource.com/c/chromium/src/+/8220427 (merged 2026-09-15 per PR #4238 discussion)
- CEF forum search returned only old Linux OSR threads (for example https://magpcss.org/ceforum/viewtopic.php?f=6&t=19237 , https://www.magpcss.org/ceforum/viewtopic.php?f=6&t=818). Nothing current, so the issue tracker is the primary source here.

Local crate sources: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cef-154.5.0+154.0.34` (wrapper signatures), `/tmp/vsesvit-research/crates-osr/gpui-0.2.2` (`Keystroke`, `RenderImage`, `drop_image`, X11 `window_handle` `unimplemented!()`, blade atlas growth).
