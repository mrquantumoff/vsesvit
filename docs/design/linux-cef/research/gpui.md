# GPUI for the Linux shell (CEF + GPUI): research

Research date: **2026-10-16**. This was a read-only investigation. Nothing was built or run except a 30-line `memcpy` microbenchmark written for this note (in `/tmp/vsesvit-research/gpui-bench/`).

Legend: **[V]** means VERIFIED: I read the code, the doc, or the registry metadata cited. **[I]** means INFERRED: a conclusion or estimate that a spike should confirm.

Sources read, with exact revisions:

| Source | Revision |
|---|---|
| Zed `main` (sparse clone of `crates/gpui*`, `scheduler`, `livekit_client`, `image_viewer`, root `Cargo.toml`) | `d678e7de4d9d` (2026-10-16 14:32 UTC) |
| `gpui` 0.2.2 from crates.io | published 2025-10-22 |
| `gpui-pre` / `-platform` / `-linux` / `-wgpu` / `-scheduler` 0.3.9 from crates.io | snapshot of zed@`93d745668805`, published 2026-10-12 |
| `longbridge/gpui-kit` (gpui-component, gpui-base, gpui-webview) | `8a83be7eb6ec` (2026-10-16) |
| `longbridge/gpui-fast` | `eeccb27f7b7b` (2026-10-15); upstream recorded as zed@`93d745668805` |
| `gpui-ce/gpui-ce` (sparse) | `d52f747a7c4d` (2026-10-14) |
| `bokuweb/gpui-cef` | `1e8870069d87` (v0.1.1, 2026-09-26) |
| `wgpu-hal` 29.0.4 (crates.io); `wgpu-hal` 30.0.1 (already in `/tmp/vsesvit-research/crates-osr`) | — |
| `cef` / `cef-dll-sys` 154.5.0+154.0.34 | `~/.cargo/registry` |

Companion note: `gpt-extension-architecture.md` (same directory) covers what each CEF architecture means for extensions. This note covers the GPUI side and cross-references it where the two meet.

---

## Executive summary

1. **Depend on the `gpui-pre` snapshots, not crates.io `gpui` 0.2.2 and not raw Zed git.** Alias them to the usual crate names and pin them exactly: `gpui = { package = "gpui-pre", version = "=0.3.9" }` plus `gpui_platform = { package = "gpui-pre-platform", version = "=0.3.9" }`. Add `gpui-component = "=0.7.3"`, or the `gpui-kit` facade.
   - **Why not 0.2.2 [V]:** it is a year old. It still uses the blade renderer, which Zed removed in February 2026 over NVIDIA/Wayland freezes. It has no AccessKit, no native popups and no system-notification API. gpui-component stopped supporting it after 0.5.1.
   - **Why not raw Zed git [V]:** gpui-component 0.6+ is built on the `gpui-pre` package, and Cargo cannot unify it with a git `gpui`. Zed's own `[patch.crates-io]` overrides (calloop, async-task, khronos-egl) do not apply to downstream workspaces.
   - **What `gpui-pre` is [V]:** a faithful weekly republish of Zed's GPUI crates. Its `[lib] name` is still `gpui`, so `use gpui::*` code is unchanged.
   - **Keep the alias in one place.** Moving to `gpui-fast` (window composition) or to a vendored, patched `gpui-pre` then takes one line.
2. **GPUI on Linux is solid enough for browser chrome. Every gap listed here is something Vsesvit must build or patch itself.**
   - **What works [V]:**
     - Rendering: wgpu, using Vulkan with a GL fallback.
     - Display servers: Wayland and X11. Wayland is picked when `WAYLAND_DISPLAY` is set. A process uses one display server, chosen when windows open.
     - Windows: client-side decorations (required on GNOME), fractional scaling.
     - System integration: IME through text-input-v3 and XIM, portal file dialogs, D-Bus notifications, Secret Service credentials, AccessKit through AT-SPI.
   - **What is missing [V]:**
     - Menus and popups: no native context menus, no menubar, no tray. Popups stay inside the window, except Wayland-only `WindowKind::AnchoredPopup`.
     - Input: no touch, no pointer lock, no keyboard-shortcut inhibit. Key events carry no physical key codes.
     - Data exchange: drag and drop handles file paths only, and the clipboard has no `text/html`.
     - Platform services: file dialogs take no filters, notifications take no icons, and there is no API to activate an existing window with another process's activation token.
3. **Stock GPUI cannot draw a GPU texture or dma-buf on Linux [V].** `surface()` is macOS/iOS-only, and the wgpu renderer skips `PrimitiveBatch::Surfaces`.
   - **The stock route:** wrap every CEF `OnPaint` frame in a new `Arc<RenderImage>`, draw it with `img()` or `Window::paint_image`, and free the old frame yourself with `Window::drop_image` / `App::drop_image`. GPUI never evicts these frames itself.
   - **What a 4K frame costs on this machine [I, from measured `memcpy`]:** about 3 × 1.3 ms of copying, two of those copies on the GPUI thread. GPUI also creates a new ~33 MB atlas texture every frame. That is fine at 1080p/1440p and marginal at 4K60.
   - **The planned fix:** a small GPUI patch adding an external wgpu-texture surface. `gpui-ce` already has one (`SurfaceSource::Texture` plus `WgpuContextHandle::from_window`).
   - **Feed it zero-copy from CEF:** import `OnAcceleratedPaint` dma-bufs. cef-rs 154 ships a DMA-BUF→wgpu importer behind the `accelerated_osr` feature.
   - **Version catch:** that importer targets wgpu 30, while gpui-pre 0.3.9, gpui-fast and gpui-ce use wgpu 29. Zed main moved to wgpu 30 (git) on 2026-10-15.
4. **The extension-compatibility choice decides which GPUI you need.**
   - **Off-screen rendering means Alloy style [V, CEF 154 header: "Windowless rendering will always use Alloy style"].** Alloy browsers are partial for Chrome extensions; see the companion note.
   - **Chrome-style browsers need a native parent window.** On Linux that is an X11 window, so GPUI must run on X11/XWayland. GPUI exposes the XCB window id through `raw-window-handle` [V]. But GPUI-drawn overlays (omnibox dropdown, menus, dialogs) would be hidden under the page unless the window uses **window composition**.
   - **Window composition today:** `gpui-fast` has it now, and Zed PR #62379 is open. On X11 it uses SHAPE cut-outs, which are rectangular and drop shadows unless a compositing manager runs.
   - **So keep both GPUI routes open:**
     - OSR: `gpui-pre` plus a texture patch.
     - Windowed Chrome style: `gpui-fast`, or #62379 once it merges.
5. **The event loop belongs to GPUI [V].**
   - **No way to add event sources:** the Linux calloop `LoopHandle` is `pub(crate)`, so you cannot register an fd source.
   - **Bridging from other threads:** send work from any thread through a channel that a foreground `cx.spawn` task awaits.
   - **Timers:** `cx.background_executor().timer(d)` gives millisecond-class timers, run on a dedicated calloop timer thread.
   - **Driving CEF:** run CEF's external pump from such a task, outside any `cx.update`. Alternatively, use `multi_threaded_message_loop`, which CEF supports on Linux [V].
   - **Quit handling:** `on_app_quit` futures get only 200 ms (`SHUTDOWN_TIMEOUT`) [V]. Close CEF browsers before calling `cx.quit()`.
6. **Prior art exists. None of it is Linux CEF.**
   - `bokuweb/gpui-cef` does macOS CEF OSR inside GPUI. It covers IME through `EntityInputHandler` and a process-wide pump. It defaults to the CPU copy path, measured at about 38% CPU for continuous 60 fps at 994×786 in a debug build.
   - `ekinertac/infiniterm` does macOS CEF OSR on gpui 0.2.2.
   - `zortax/gpui-video` imports Linux dma-bufs into GPUI through a patched wgpu surface element.
   - gpui-kit's `gpui-webview` puts WebKitGTK in an X11 child window, with GPUI overlays above it through composition.
7. **Licence: Vsesvit is MIT.** GPUI and gpui-component are Apache-2.0. Zed's `ui`, `theme` and `platform_title_bar` crates are **GPL-3.0-or-later** [V], so do not copy Zed's title bar or UI components.

---

## 1. Which GPUI to depend on

### 1.1 Options

| Option | Latest | Renderer on Linux | Used by | Verdict |
|---|---|---|---|---|
| crates.io `gpui` | **0.2.2, 2025-10-22**; nothing since [V crates.io API] | blade-graphics 0.7, Vulkan [V `Cargo.toml`, `src/platform/blade/`] | 139 reverse dependencies, e.g. infiniterm [V] | Stale. No AccessKit, no `AnchoredPopup`, no system notifications, no `request_windowing` [V grep]. Blade had NVIDIA freezes, which is why Zed replaced it. |
| Zed git `main` | `d678e7de` today; workspace `gpui` still says `version = "0.2.2"`, `publish = true` [V] | `gpui_wgpu`, wgpu 30 from git rev `3b3506ea`, since #65649 on 2026-10-15; before that crates.io wgpu 29.0 [V] | bokuweb/gpui-cef: `gpui = { git = … }` [V] | Newest, but see the pitfalls in 1.3. Needs Rust 1.98.1 (`rust-toolchain.toml`) [V]. |
| **`gpui-pre`** (+ `-platform`, `-linux`, `-wgpu`, `-macros`, …) | **0.3.9, 2026-10-12** = zed@`93d7456`; weekly since 0.3.4 (Sep 7, 14, 21, 28, Oct 5, 12) [V] | wgpu **29.0** (crates.io) [V] | gpui-component/gpui-kit 0.6+, 52 reverse dependencies; many forks patched in via `[patch.crates-io]` [V] | **Recommended base.** Published by Longbridge's `script/bump-gpui.ts` [V]. |
| `gpui-fast` | 0.1.8, 2026-10-14; upstream zed@`93d7456` [V `UPSTREAM`] | wgpu 29 | `gpui-kit` feature `gpui-fast`; `gpui-webview` requires it [V] | Longbridge fork adding retained mode, scroll layers and **window composition** (port of zed#62379). Experimental. Candidate if CEF runs windowed. |
| `gpui-ce` | git HEAD `d52f747a`; crates.io `gpui-ce` 0.2.2 (2026-08-28) [V] | wgpu 29.0.3 | gpui-tray, oxideterm [V] | Community fork that is "mostly API compatible with upstream, but that is changing!" [V README]. Has **`SurfaceSource::Texture`** (wgpu texture) and **`WgpuContextHandle`** on Linux [V]. Reference for the texture patch rather than a base. |
| `gpui-unofficial` | 1.25.0-pre, 2026-10-14 [V] | — | small | Another republish by iamnbutler; ignore. |

**What `gpui-pre` is [V `script/bump-gpui.ts` header].**
- Zed publishes `gpui` to crates.io only occasionally, so Longbridge republishes any Zed commit.
- Each crate is renamed `gpui` → `gpui-pre`, `gpui_platform` → `gpui-pre-platform`, `collections` → `gpui-pre-collections`, and so on. The original crate name stays as the `[lib]` name.
- All crates get one version. Git-only optional dependencies are dropped; a non-optional git dependency aborts the run.
- The script checks that no GPL crate enters the closure, then builds and tests gpui-kit before publishing.
- The crate description names the Zed commit, for example "gpui-pre snapshot of zed@93d7456".
- gpui-kit pins exact versions (`=0.3.9`) because a caret requirement once moved applications onto a snapshot gpui-component did not compile with (issue #3156) [V gpui-kit `Cargo.toml` comment].

I diffed the gpui-pre 0.3.9 sources against Zed main from four days later [V]. `assets.rs`, `executor.rs` and `elements/surface.rs` are identical. `window.rs` differs by about 256 diff lines, `platform.rs` by 84 and `input.rs` by 133. The snapshot already has every Linux feature discussed below: `AnchoredPopup`, `accesskit_unix`, and the `Surfaces` no-op in the renderer.

### 1.2 API differences: 0.2.2 vs main / gpui-pre 0.3.9 (all [V])

| Area | 0.2.2 | main / 0.3.9 |
|---|---|---|
| Crates | One `gpui` crate, platform code inside | `gpui` + `gpui_platform` (picks the backend) → `gpui_linux`, `gpui_wgpu`, `gpui_macos`, `gpui_windows`, `gpui_web`, `gpui_ios`; `scheduler` crate |
| Boot | `Application::new().run(…)` | `gpui_platform::application().run(…)`, or `gpui_platform::linux(WindowingModes::X11)`; `Application::with_platform`, `with_windowing` |
| Linux renderer | blade (`platform/blade/blade_renderer.rs`) | wgpu (`gpui_wgpu/src/wgpu_renderer.rs`), Vulkan with GL fallback |
| `Window::focus` | `focus(&handle)` | `focus(&handle, cx)` |
| `anchored().anchor(…)` | `Corner::TopLeft` | `Anchor::TopLeft` |
| `AsyncApp::update` | returns `Result<R>` | returns `R` (`AsyncWindowContext::update` still returns `Result<R>`) |
| Accessibility | none | AccessKit: `.role(Role::…)`, `Element::a11y_role / write_a11y_info / a11y_synthetic_children` |
| Window kinds | `Normal`, `PopUp`, `Floating` | adds `AnchoredPopup(PopupOptions)`, `Dialog`, `LayerShell(…)` |
| New options | — | `WindowOptions::inactive_frame_interval` (default **33.3 ms**), `app_owns_titlebar_drag`, `is_movable` |
| System notifications | none | `App::show_system_notification` etc. |
| External drag | none | `.external_drag_payload(…)` after `.on_drag(…)` (files only) |
| Display server | chosen once at start | `WindowingModes`, `App::request_windowing` (switch only while no window is open) |
| `EntityInputHandler` | 8 required methods | same 8 required, plus defaults: `paste`, `selection_bounds_for_range`, `set_selected_text_range`, `text_length_utf16`, `accepts_text_input`, `text_input_configuration`, … |

### 1.3 Pitfalls of depending on Zed git directly

- **[V]** Zed's root `[patch.crates-io]` swaps in forks of `calloop`, `async-task`, `async-process`, `khronos-egl`, `notify` and others. Cargo applies patches only from the top-level workspace, so a downstream git dependency builds GPUI against the crates.io versions instead. That is a configuration Zed never tests. [I]
- **[V]** Zed main pins wgpu to a git rev. The comment says "30.0.1 lacks the Vulkan surface-drop panic fix: gfx-rs/wgpu#10230". Any crate depending on crates.io `wgpu = "30"`, such as cef-rs `accelerated_osr`, resolves to a *different* `wgpu` unless your workspace patches crates.io wgpu to the same rev.
- **[V]** gpui-component 0.6+ depends on the package `gpui-pre`, so it cannot be combined with a git `gpui`.
- **[V]** Zed main requires Rust 1.98.1. Vsesvit's workspace says `rust-version = "1.98"`, and this machine has rustc 1.99.0.

### 1.4 gpui-component (now part of "GPUI Kit")

- **Version [V crates.io]:** `gpui-component` **0.7.3**, released **2026-10-12**. The repository moved to `github.com/longbridge/gpui-kit`; `longbridge/gpui-component` redirects there. Licence Apache-2.0.
- **Which GPUI it needs [V crates.io dependency API]:**
  - 0.2.0–0.5.1 (Oct 2025–Feb 2026): `gpui ^0.2.x`.
  - 0.6.0+ (Sep 2026): `gpui-pre`.
  - 0.7.3: `gpui-pre =0.3.9`, `gpui-pre-macros =0.3.9`, `gpui-pre-sum-tree =0.3.9`, plus optional `gpui-fast ^0.1.7`.
- **Layering [V README]:** `gpui-kit` is the facade. It re-exports gpui, `gpui-base` (unstyled behaviour), `gpui-component` (styled) and `gpui-kit-assets` (Lucide icons). Setup is `gpui_kit::init(cx)` and `gpui_kit::open_window(…)`, which wraps your view in `base::Root`. The Root hosts dialogs, notifications and sheets.
- **Widgets [V `crates/component/src/` and `website/component/*.md`]:**
  - Inputs and forms: input/textarea/number/OTP inputs and the code editor, select (dropdown), combobox, form, settings (`Settings`, `SettingPage`, `SettingGroup`, `SettingItem`, `SettingField`), switch, checkbox, radio, slider, toggle, date picker, color picker.
  - Lists and tables: list, virtual list, data table (virtual rows and columns, resizable columns), tree.
  - Overlays and menus: popover, hover card, tooltip, menu (`PopupMenu`, `ContextMenu`, `DropdownMenu`, `AppMenuBar`), dialog, alert dialog, sheet, notification (in-app), command palette.
  - Layout and navigation: tabs (`TabBar`, horizontal; no vertical mode found [I]), sidebar (`Sidebar::side`, `collapsible`, `collapsed`), resizable panels, dock (resizable panels, draggable tabs, splits), scrollable, title bar (CSD window controls on Linux), window border.
  - Content and display: kbd, badge, avatar, progress, skeleton, spinner, charts and plot, markdown/HTML `TextView`, theme (38 presets, JSON themes), i18n.
- **Native menus [V `native_menu/mod.rs`, `fallback.rs`]:** `NativeMenu` is OS-native on macOS and Windows only. On Linux it falls back to a GPUI-drawn `PopupMenu`, "clipped to the window".
- **Accessibility:** AccessKit roles are built in [V README]. The project's own comparison rates its accessibility "Partial" [V `website/docs/comparison.md`].

### 1.5 Other GPUI widget libraries ([V] crates.io search; all small)

- **`gpui-ce`'s examples.** A fork, not a widget library.
- **`gpui-ui-kit` 0.5.10.** Last release January 2026; around 200 recent downloads.
- **`fanta-gpui-ui` 0.5.0.** About 20 recent downloads.
- **`open-gpui-ui-*` 0.2.0.** Released July 2026.
- **`gpui-luma` 0.1.10.** Released 2026-10-16.
- **`gpui-omarchy`.** Built on gpui-kit.
- **`native-theme-gpui`.** Reads desktop themes.
- **`gpui-tray` 0.1.4.** A StatusNotifierItem tray through zbus; works with `gpui ^0.2.2`, `gpui-ce` and `gpui-pre ^0.3.4`.
- **`gpui-updater-pre`, `swr-gpui-pre`.** Small utilities.

Only gpui-component has the breadth and activity for a browser UI.

### 1.6 Recommendation and Cargo snippet

```toml
# workspace Cargo.toml: change GPUI in one place
[workspace.dependencies]
gpui          = { package = "gpui-pre",          version = "=0.3.9" }
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.9", features = ["wayland", "x11", "font-kit"] }
gpui-component = { version = "=0.7.3", default-features = false }   # or gpui-kit = "=0.7.3"
# Optional: gpui-fast for window composition (windowed Chrome-style CEF), selected through gpui-kit's "gpui-fast" feature.
# A texture patch: vendor gpui-pre(+-wgpu,+-linux) and use [patch.crates-io] gpui-pre = { path = "third_party/gpui-pre" },
# as many projects already do (alphabet-h/gpui-pre, feigeCode/gpui-pre "dynamic-texture",
# NortheLabs/gpui-pre "native-surface patch", ducktape-industries/gpui-pre, …) [V GitHub repo descriptions].
```

**Fedora build dependencies [V Zed `script/linux`; `rpm -q` on this machine].** `wayland-devel`, `libxcb-devel`, `libxkbcommon-x11-devel`, `fontconfig-devel`, `vulkan-loader`, `alsa-lib-devel`, `openssl-devel`, `clang`, `cmake`. All are installed here; GTK is not needed.

---

## 2. GPUI's Linux platform (main ≈ gpui-pre 0.3.9)

### 2.1 Renderer

- **[V]** `gpui_wgpu` (`WgpuRenderer`, `WgpuAtlas`, `WgpuContext`). On Linux it enables wgpu features `vulkan`, `gles`, `wgsl`, `metal`, `webgpu`.
- **[V]** The instance is created with `Backends::VULKAN | Backends::GL`.
- **[V]** `ZED_DEVICE_ID` picks the adapter.
- **[V]** Present mode defaults to `Fifo` with `desired_maximum_frame_latency: 2`.
- **[V]** Alpha mode is `PreMultiplied` when the window is transparent.
- **History [V]:** blade was removed by #46758 ("gpui: Remove blade, reimplement linux renderer with wgpu", merged 2026-02-13), which cites NVIDIA freezes and niri issues.
- Open follow-ups: #63313 skip GL when Vulkan works, #63346 skip software Vulkan ICDs [V].
- On this machine (RTX 5080, driver 615.78.08, PCIe 5.0 x16), Vulkan should be selected. [I]

### 2.2 Wayland vs X11 selection [V `gpui_linux/src/linux/display_connection.rs`, `linux.rs`]

- **Selection rule.** `select_backend` picks Wayland if `WAYLAND_DISPLAY` is non-empty and `WindowingModes::WAYLAND` is allowed. Otherwise it picks X11 if `DISPLAY` is non-empty. Otherwise the app runs headless.
- **Headless override.** `ZED_HEADLESS` forces headless.
- **Forcing X11.** `gpui_platform::linux(gpui::WindowingModes::X11)`. gpui-webview does exactly this for WebKitGTK [V].
- **One connection per process.** `App::request_windowing` switches only while no window is open.
- **0.2.2 [V `guess_compositor`]:** chose by the same environment variables.
- **X11 scale.** Comes from Xft resources, with `GPUI_X11_SCALE_FACTOR` as an override [V].

### 2.3 Windows

- **Multiple windows.** `cx.open_window(WindowOptions, |window, cx| cx.new(…)) -> Result<WindowHandle<V>>` opens as many as you like [V]. Entities can be re-hosted in another window (`examples/move_entity_between_windows.rs`) [V]. With OSR, "move tab to new window" can keep the same CEF browser alive. [I]
- **Decorations [V `wayland/window.rs`].**
  - GPUI asks for server-side decorations through `zxdg_decoration_manager_v1` when the compositor offers it, and otherwise falls back to `WindowDecorations::Client`.
  - With client-side decorations, the app draws the title bar, controls, resize edges and shadow. It calls `Window::start_window_move`, `start_window_resize(ResizeEdge)`, `show_window_menu`, `set_client_inset`, and reads `window_decorations() -> Decorations::Client { tiling }`.
  - X11 sets `_MOTIF_WM_HINTS` and `_GTK_FRAME_EXTENTS`.
  - On GNOME (no xdg-decoration), the client always draws its own decorations. gpui-component's `TitleBar` and `WindowBorder` cover this [V docs].
- **App id [V].** `WindowOptions::app_id` and `Window::set_app_id` map to `xdg_toplevel.set_app_id`, or `WM_CLASS` on X11. Set it: Wayland `activate()` does nothing without an app id.
- **Activation [V `wayland/client.rs`, `window.rs`].**
  - At startup GPUI reads `XDG_ACTIVATION_TOKEN`, removes it from the environment, and spends it on the first window.
  - `Window::activate_window()` asks for a new `xdg_activation_v1` token using the last mouse-press serial. The code comments: "the activation is probably going to be rejected anyway".
  - `open_url` and `reveal_path` request a token for the launched app.
  - A token from another process can only be passed through `GraphicalEnvironment::activation_token` when switching from headless to windowed.
  - **There is no API to raise an existing window with another process's token.** That matters for "open a URL in the running browser" from a second launch. [V]
- **Fractional scaling [V].** Wayland uses `wp_fractional_scale_v1` with `wp_viewporter`. `Window::scale_factor()` returns an `f32`.
- **Window kinds.** `Normal`, `PopUp`, `Floating`, `Dialog` (uses `xdg_dialog_v1` modal hints on Wayland [V imports]) and `LayerShell`. `AnchoredPopup(PopupOptions)` takes `parent`, `anchor_rect`, `anchor`, `gravity`, `constraint_adjustment`, `offset` and `grab` [V `platform/popup.rs`].
  - **AnchoredPopup is implemented only on Wayland**, through `xdg_popup` and `xdg_positioner`. X11, Windows and macOS return `PopupNotSupportedError` [V].
  - It landed in #60232 on 2026-07-08 [V]. A macOS version is open as #64353 [V].
  - A grabbing popup must be opened from a mouse-down handler [V doc comment].
- **Inactive windows** throttle animation frames to `inactive_frame_interval`, 33.3 ms by default [V]. Set it to `None` for browser windows that play video while unfocused. [I]
- **Not in GPUI [V absent from `wayland/client.rs` protocol imports]:** `xdg_toplevel_drag_v1` (tab tear-off following the pointer), `zwp_pointer_constraints_v1`/`relative_pointer` (Pointer Lock API), `keyboard_shortcuts_inhibit` (full-screen web apps or remote desktop capturing Super), `wl_touch` (no touch events at all; X11 has no XI touch either), idle-inhibit protocol. `prevent_idle_sleep` uses the portal `Inhibit` with the **Suspend** flag only, which does not keep the screen awake for video.

### 2.4 Input

- **Keyboard [V `platform/keystroke.rs`, `interactive.rs`].**
  - `KeyDownEvent { keystroke: Keystroke { modifiers, key, key_char }, is_held, prefer_character_input }`.
  - **There is no physical or native key code.** CEF `KeyEvent.native_key_code` and `windows_key_code` must be derived from the `key` name, which loses information. gpui-cef does this with lookup tables [V `input.rs`]. Chromium then gets no reliable `KeyboardEvent.code` on non-US layouts. [I]
- **IME [V].**
  - Wayland uses `zwp_text_input_v3`: preedit maps to `replace_and_mark_text_in_range` and commit to `replace_text_in_range`. X11 uses XIM through `zed-xim`, with `handle_ime_commit`, `handle_ime_preedit` and so on.
  - Non-IME keys that no binding handles reach the input handler as `replace_text_in_range(None, key_char)`.
  - The candidate window position comes from `InputHandler::bounds_for_range` and `selected_bounds` → `update_ime_position`.
  - A view takes text by implementing `EntityInputHandler` and calling `window.handle_input(&focus_handle, ElementInputHandler::new(bounds, entity), cx)` in `paint` (snippet in §7.5).
- **Mouse and scroll.**
  - Events: `MouseDownEvent { button, position, modifiers, click_count, first_mouse }`, `MouseUpEvent`, `MouseMoveEvent { pressed_button, … }`.
  - Scroll: `ScrollWheelEvent { position, delta: ScrollDelta::{Pixels, Lines}, modifiers, touch_phase }`.
  - Gestures: Wayland pinch through `zwp_pointer_gestures_v1`.
  - Back/forward buttons: `MouseButton::Navigate`.
  - [V]

### 2.5 Clipboard and drag and drop

- **Clipboard [V].** `cx.write_to_clipboard(ClipboardItem)` / `read_from_clipboard()`, and the primary selection through `write_to_primary` / `read_from_primary`.
  - Entries: `String`, `Image` (PNG/JPEG/WebP/GIF/SVG/BMP/TIFF/ICO/PNM) and `ExternalPaths`.
  - Wayland accepts `text/plain;charset=utf-8` and `UTF8_STRING`; there is **no `text/html`** [V `wayland/clipboard.rs`].
  - Web content's clipboard is Chromium's own, not GPUI's. [I]
- **Drag in [V].** `FileDropEvent::{Entered{paths}, Pending, Submit, Exited, Ended}` carries **file paths only** (`text/uri-list`). Dropping text or a URL from another app into the window is not supported.
- **Drag out [V].** `.on_drag(value, …).external_drag_payload(|v, w, cx| Some(ExternalDragPayload::Files(…)))`. When the drag leaves the window, GPUI hands it to the platform.
  - **Files only. Implemented on Wayland and macOS; X11 and Windows do not implement it.**
  - CEF OSR `StartDragging` (links, text, images out of the page) therefore has no GPUI outlet.
- **Inside the app [V].** GPUI drags (`on_drag`, `on_drop`, `can_drop`, `drag_over`) work within one window.

### 2.6 Cursors [V]

- `CursorStyle` has 21 variants: Arrow, IBeam, Crosshair, Closed/OpenHand, PointingHand, eight resize kinds, ResizeColumn/Row, vertical IBeam, OperationNotAllowed, DragLink, DragCopy, ContextualMenu.
- They map to XCursor/`wp_cursor_shape_v1` names copied from Chromium's cursor factory.
- **There is no custom image cursor and no wait/progress/help/move/zoom/cell/none.** CEF `CT_CUSTOM` and several CSS cursors fall back.
- APIs: `window.set_cursor_style(style, &hitbox)` and `hide_cursor_until_mouse_moves`.

### 2.7 Dialogs, notifications, tray, keyring [V `gpui_linux/src/linux/platform.rs`]

- **File dialogs.** `cx.prompt_for_paths(PathPromptOptions { files, directories, multiple, prompt })` and `prompt_for_new_path(dir, suggested_name)` go through `ashpd` FileChooser (xdg-desktop-portal).
  - The dialog is modal, with the window identifier taken from the connection.
  - **There is no filter or MIME parameter.** HTML `accept=` from CEF `FileDialogHandler` would need ashpd called directly.
  - When the portal is missing, the call errors with `FILE_PICKER_PORTAL_MISSING`.
  - `can_select_mixed_files_and_dirs()` returns false.
- **Notifications.** `show_system_notification(SystemNotification { tag, title, body, actions })` goes through `notify-rust` (org.freedesktop.Notifications), not the portal.
  - **No icon or image field.** Web notifications and `chrome.notifications` with icons need notify-rust or ashpd directly.
  - Responses arrive through `on_system_notification_response`.
- **Tray.** Not in GPUI (no StatusNotifierItem code). The third-party `gpui-tray` crate exists.
- **App menus.** `set_menus` only stores the menus, which `get_menus` returns. Nothing is drawn on Linux. gpui-component's `AppMenuBar` draws them inside the window.
- **Credentials.** `write_credentials`, `read_credentials` and `delete_credentials` use Secret Service through `oo7`. Vsesvit's vault has its own.
- **URL schemes.** `register_url_scheme` is unimplemented on Linux.

### 2.8 Actions, keybindings, key contexts [V `action.rs`, `keymap/binding.rs`, `window.rs::dispatch_key_event`]

- **Declaring actions.** `actions!(namespace, [A, B])`, or `#[derive(Action)]` with `#[action(namespace = …)]`; `no_json` skips serde and schemars.
- **Binding keys.** `cx.bind_keys([KeyBinding::new("ctrl-shift-a", TabSearch, Some("BrowserWindow"))])`. Modifier names: `ctrl`, `alt`, `shift`, `super`/`win`/`cmd`, `fn`, and `secondary` (cmd on macOS, ctrl elsewhere).
- **Contexts.** `div().key_context("BrowserWindow").track_focus(&fh).on_action(cx.listener(Self::f))`. Predicates can be expressions over the context stack.
- **Dispatch order for a key down:**
  1. keystroke interceptors (`App::intercept_keystrokes`);
  2. the keymap matches the focus path and dispatches actions, bubbling;
  3. if nothing stopped propagation, element `capture_key_down` / `on_key_down` listeners run;
  4. the input handler receives text.

  **Consequence:** a binding such as `ctrl-l` beats the page. To let the page see keys first, as Chrome does (except for reserved keys like Ctrl+T/W/N/Tab), either bind browser shortcuts in contexts that do not include the web view, or intercept the keystroke when the web view is focused, send it to CEF, and re-dispatch the action from CEF's `OnPreKeyEvent`/`OnKeyEvent`. [I]
- **Observing.** `cx.observe_keystrokes` sees keystrokes after dispatch.

### 2.9 Overlays, menus, tooltips, focus [V]

- **Overlays.** `deferred(child).priority(n)` paints a child after the rest of the frame, so it sits above. `anchored().anchor(Anchor::TopLeft).position(p).offset(o).snap_to_window_with_margin(px(8.))` positions it. Nested deferred overlays work (`examples/popover.rs`). Close with `.on_mouse_down_out(…)`.
  - **These overlays are clipped to the window.** Only `WindowKind::AnchoredPopup` escapes, and only on Wayland.
  - gpui-component's PopupMenu, ContextMenu and Popover are deferred overlays, plus `AnchoredPopup` for editor completion menus [V].
- **Tooltips.** `.tooltip(|window, cx| AnyView)`, `hoverable_tooltip`, `tooltip_show_delay`, all inside the window.
- **Focus.** `FocusHandle` (`cx.focus_handle()`), `.track_focus(&fh)`, the `Focusable` trait, `window.focus(&fh, cx)`, `cx.on_focus/on_blur/on_focus_in/on_focus_out`, tab stops (`tab_stop.rs`, `focus_visible`), and `cx.observe_window_activation(window, …)`. Forward these to CEF with `host.set_focus(1/0)`. [I]

### 2.10 Accessibility [V]

- **Status.** AccessKit merged as #56065 on 2026-05-27. Later work includes #60397 (landmarks and menus, July 2026) and #61926 (accessibility identifiers, September 2026).
- **Linux.** Windows on X11 and Wayland create an `accesskit_unix::Adapter` (AT-SPI).
- **Exposing elements.** An element appears in the tree only when it has both an `id` and a `role`.
- **Custom subtrees.** `Element::a11y_synthetic_children(&mut self, prepaint, &mut A11ySubtreeBuilder)` can inject nodes that have no GPUI element. With OSR, this is the hook for mirroring CEF's page accessibility tree (CEF `AccessibilityHandler`) into AT-SPI. [I]
- **Maturity.** gpui-kit rates accessibility "Partial".

---

## 3. Drawing CEF frames inside a GPUI element

### 3.1 How `img()` / `RenderImage` / the atlas work (main, gpui-pre 0.3.9) [V]

- **`RenderImage`** (`assets.rs`) is `{ pub id: ImageId, scale_factor, data: SmallVec<[image::Frame; 1]> }`, "a cached and processed image, in BGRA format". `RenderImage::new(frames)` takes the next id from a global atomic, and the data cannot be changed afterwards.
  - CEF `OnPaint` delivers "a BGRA image with an upper-left origin", the whole image every time, plus `dirtyRects` [V CEF 154 header], so the bytes go in unchanged. gpui-cef does exactly that [V `handlers.rs`].
  - Use the `image` 0.25 crate, the same one GPUI uses, for `image::Frame` and `ImageBuffer`.
- **`Window::paint_image(bounds, image_bounds, corner_radii, Arc<RenderImage>, frame_index, grayscale) -> Result<()>`** calls `sprite_atlas.get_or_insert_with(AtlasKey::Image(RenderImageParams { image_id, frame_index }), …)`. Painting the same id again costs nothing. A new id is a new upload.
- **`AtlasState::get_or_insert_with`** builds and inserts only on a miss. `remove(key)` frees the tile.
- **`WgpuAtlasTextures::insert`** allocates a tile with etagere, then calls `upload_texture`.
  - `upload_texture` calls `swizzle_upload_data(bytes, format)`, which **copies the whole buffer** (`bytes.to_vec()`, or swaps R and B if the atlas is `Rgba8Unorm`).
  - The copy waits in `pending_uploads` until `before_frame` → `queue.write_texture`, where wgpu **copies again** into a staging buffer before the GPU copy.
- **Large images.** An image bigger than any free space gets a **new atlas texture of its own size**, at least 1024² (`push_texture`). When the last tile in a texture is removed, the texture is freed. So a 4K page drawn as a new `RenderImage` every frame creates one ~33 MB `wgpu::Texture` per frame and frees the one from two frames earlier.
- **Sampling.** Sprites use a **linear** sampler, and `paint_image` snaps bounds to device pixels. Make the CEF buffer exactly `round(bounds × scale_factor)` pixels or the page blurs. Under fractional scaling that means CEF `device_scale_factor = window.scale_factor()`, with the view rectangle in DIP. [I]
- **Shader.** The polychrome-sprite shader samples the texture and applies opacity and corner-radius coverage. Opaque pages are unaffected; for transparent CEF backgrounds, check premultiplied alpha. [I]
- **`surface()` / `Window::paint_surface`** exist only for `cfg(any(target_os = "macos", target_os = "ios"))` and take a `CVPixelBuffer`, hard-coded NV12 [V gpui-cef README]. The wgpu renderer has `PrimitiveBatch::Surfaces(_surfaces) => {}` with the comment "Surfaces are macOS-only … not implemented by the WGPU renderer".
  - **Upstream GPUI has no public way to draw a GPU texture or dma-buf on Linux.**
  - `canvas(prepaint, paint)` exposes only `paint_quad`, `paint_path`, `paint_image` and similar. No GPU hooks.

### 3.2 Freeing frames: `drop_image` [V]

- **The two calls:**
  - `Window::drop_image(&mut self, Arc<RenderImage>) -> Result<()>` removes every frame of that image from the window's atlas.
  - `App::drop_image(&mut self, image, current_window: Option<&mut Window>)` does it for every window. The window being updated is temporarily out of `App.windows`, hence the parameter.
- **Images passed as `ImageSource::Render(Arc<RenderImage>)` are never freed automatically.** Only `ImageCache`/asset images are managed for you (`elements/image_cache.rs` calls `cx.drop_image` on eviction). Without dropping, every frame leaks a texture. [V]
- **Do not drop the frame on screen.** If a stale scene, such as a cached view replaying an old paint, references a freed texture, `draw_sprites` silently skips it ("The atlas has released this texture; the batch belongs to a stale paint"). The result is a blank flash, not a crash.
- **Zed's own pattern for Linux video frames** (`livekit_client/src/remote_video_track_view.rs`): keep `current_rendered_frame` and `previous_rendered_frame`. When rendering a new frame, drop the *previous* one unless it is the current one, then shift. That frees frame N-2 while N-1 may still be on screen. gpui-cef instead drops the replaced frame on the next pump tick (`shared.rs::take_stale_cpu_frames` → `cx.drop_image(frame, None)`), which can drop a frame that was never painted. [V; the flash risk is I]
- **The `id` trick does not help.** `RenderImage::id` is a `pub` field, so a new `RenderImage` could reuse an old id after `drop_image`. GPUI would still reallocate and re-upload. [I]

### 3.3 Cost of a 4K frame

Measured on this machine (Ryzen 9 9950X, DDR5), one `memcpy` of a BGRA frame from my benchmark [V]:

| Frame | Bytes | One copy | Throughput |
|---|---|---|---|
| 3840×2160 | 31.6 MiB | **1.30–1.37 ms** | ~25 GB/s (DRAM-bound) |
| 2560×1440 | 14.1 MiB | 0.29–0.32 ms | ~46–50 GB/s (partly in L3) |
| 1920×1080 | 7.9 MiB | 0.12–0.13 ms | ~64–70 GB/s |

Estimated cost per 4K frame through stock `img()` [I]:

| Step | Cost |
|---|---|
| `OnPaint` buffer → `Vec<u8>` for `RenderImage` (copy 1) | ~1.3 ms. Runs on CEF's UI thread: the GPUI thread with the external pump, or CEF's own thread with `multi_threaded_message_loop`. |
| Atlas `swizzle_upload_data` `to_vec` (copy 2, GPUI thread, during paint) | ~1.3 ms |
| `queue.write_texture` staging copy (copy 3, GPUI thread, during draw) | ~1.3 ms |
| New atlas texture each frame (Vulkan image plus memory allocation) and freeing the old one | small but not free |
| PCIe upload, 31.6 MiB | ~0.6–1 ms at ~35–50 GB/s; GPU time, same queue |
| **Total** | **~2.6 ms on the GPUI thread, ~3.9 ms of `memcpy` overall**, and about 6 GB/s of extra DRAM traffic at 60 fps |

That is 15–25% of a 16.7 ms frame. CEF's own readback is extra. At 1080p it is 10× smaller.

For comparison, gpui-cef reports about 38% CPU across all processes for a continuously repainting 994×786 page at 60 fps, in a debug macOS build [V README]. infiniterm reports 0.7–1.5 ms per changed 1024×768 frame on an M4 [V `spikes/cef-frame/NOTES.md`].

### 3.4 Options for getting pixels on screen

| # | Approach | GPUI change | Copies per frame | GPUI can clip, round, overlay? | Status |
|---|---|---|---|---|---|
| A | New `Arc<RenderImage>` per `OnPaint` → `img()`/`paint_image`; two-slot `drop_image` | none | 3 CPU + upload | yes (it is a sprite) | Works today [V]; cost in §3.3 |
| A′ | Tiles: split the page into e.g. 256² `RenderImage`s and replace only tiles that `dirtyRects` touch | none | proportional to dirty area | yes | [I] A scroll still dirties everything; watch tile seams (atlas-bleed PR #62557 is open) |
| B | **Dynamic texture**: stable id, fixed size, partial `update` (zed PR **#64061**, "gpui: Add updatable dynamic textures", open, 2026-10-14; adds `Window::update_dynamic_texture / paint_dynamic_texture / drop_dynamic_texture`, wgpu/Metal/DX11 backends) | patch: vendor gpui-pre and apply #64061 (feigeCode/gpui-pre carries it) | 1 CPU (to staging) of the dirty rectangles | yes | Open PR [V]. Simplest CPU-path fix. |
| C | **External wgpu texture surface**: GPUI exposes its `wgpu::Device`/`Queue` and a `SurfaceSource::Texture { texture: Arc<wgpu::Texture>, size }` primitive | patch: port gpui-ce #39 (2026-06-07, "Add wgpu Surface element and GPU device requirements API for Linux") and #237 (2026-09-09, "Add custom GPU rendering API": `WgpuContextHandle::from_window`, `WgpuRenderTarget`) | **0 CPU** with dma-buf import; 1 GPU blit | yes | In gpui-ce [V]. The upstream Zed attempt #52669 was auto-closed as stale on 2026-05-24 [V]. zortax/gpui-video uses it for GStreamer dma-bufs [V]. |
| C′ | Feed C from CEF **`OnAcceleratedPaint`**. On Linux the info "contains several planes, each with an fd to the underlying system native buffer", plus a `modifier` and a `format`. The buffer "will be released back to the pool after this callback returns", so the contents must be copied into a texture you own inside the callback [V CEF 154 header] | as C | 0 CPU; GPU copy of ~66 MB in VRAM (~0.07 ms at about 960 GB/s) [I] | yes | cef-rs 154 has `osr_texture_import` (feature `accelerated_osr`): `SharedTextureHandle::new(info).import_texture(&wgpu::Device)` builds a `VkImage` with `DRM_FORMAT_MODIFIER_EXT`, imports the fd, and wraps it with `texture_from_raw` / `create_texture_from_hal` [V]. **It needs wgpu 30** [V cef `Cargo.toml`]. wgpu-hal 30 enables `VK_EXT_image_drm_format_modifier` automatically; **wgpu-hal 29 does not** [V adapter.rs]. Both enable `VK_KHR_external_memory_fd` and `VK_EXT_external_memory_dma_buf`. CEF's own `shared_texture_enabled` doc still says "Currently only supported on Windows (D3D11)", yet the Linux struct carries dma-buf planes [V]. Settle this in the CEF research. |
| D | **Window composition**: a native or external-GPU surface placed between GPUI's base and overlay layers (zed PR **#62379**, open, updated 2026-10-15; shipped in **gpui-fast** as `Window::enable_window_composition()`, `create_native_surface()`, `create_external_gpu_surface(Rc<dyn PlatformSurfaceAttachment>)`, `on_input`, `place_above/below`) | use gpui-fast or wait for #62379 | 0 CPU; the compositor composites | **overlays yes; clipping and rounded corners of the page no** (rectangular surfaces); X11 cut-outs drop shadows without a compositing manager | [V] Wayland: desynchronized `wl_subsurface`, exposed as `RawWindowHandle::Wayland`. X11: child window, exposed as `RawWindowHandle::Xcb`, with GPUI content above it cut out through SHAPE. #62379 says "A real Linux WebView (WPE WebKit, or CEF off-screen rendering) can plug into the same native surface API later". **Also the way to host *windowed Chrome-style* CEF (X11 parent) under GPUI overlays.** |
| E | macOS `surface()` | — | — | — | Not available on Linux [V] |

### 3.5 Recommendation [I]

- **Spike first with A at 1080p.** It needs no GPUI change. Use the two-slot drop and run the pump from GPUI (§4). That proves input, IME and the frame lifecycle.
- **For the real browser with OSR, carry one small vendored GPUI patch and leave the rest stock:**
  - **C:** an external wgpu texture surface plus device access, ported from gpui-ce.
  - **C′:** feed it from CEF `OnAcceleratedPaint`, through cef-rs `accelerated_osr` or an equivalent importer written against GPUI's wgpu version.
  - **B as the fallback** wherever dma-buf import fails (no Vulkan, mismatched modifiers), using `dirtyRects`.
- **Align wgpu versions before C′.** Wait for a gpui-pre snapshot on wgpu ≥30, or patch crates.io `wgpu` to Zed's git rev in our workspace so cef-rs and GPUI share one `wgpu`.
- **If the extension work picks windowed Chrome-style browsers instead,** the page cannot be a texture: it is an X11 child window. Use D through gpui-fast, which gpui-kit's `gpui-webview` already does for WebKitGTK [V].

---

## 4. Async, timers, executors, CEF's message pump

### 4.1 What GPUI provides [V `executor.rs`, `gpui_linux/src/linux/dispatcher.rs`, `platform.rs`]

- **Foreground executor (`!Send`).**
  - `cx.spawn(async move |cx: &mut AsyncApp| …)` on `App`.
  - `cx.spawn(async move |this: WeakEntity<T>, cx: &mut AsyncApp| …)` on `Context<T>`.
  - `cx.spawn_in(window, async move |this, cx: &mut AsyncWindowContext| …)`.
  - `ForegroundExecutor::spawn_when_idle`.
  - Each returns a `Task<R>`. Dropping it cancels; `.detach()` keeps it running.
- **Background executor (`Send`).** `cx.background_executor().spawn(fut)` and `.timer(Duration) -> Task<()>`.
  - The Linux dispatcher runs worker threads, one per CPU (`available_parallelism`), plus a **"Timer" thread with its own calloop `EventLoop`**.
  - `dispatch_after` inserts a `calloop::timer::Timer::from_duration(d)` there, so millisecond timers are accurate to a millisecond or better. [I for exact resolution]
- **Getting onto the main thread.** Foreground runnables are scheduled through `dispatch_on_main_thread`, a priority queue plus a `calloop::ping` registered on the main `EventLoop`. They run as **idle callbacks** after pending events.
  - A waker fired from any thread (`futures::channel::mpsc`, `async_channel`, `oneshot`, `smol::Async` …) re-queues the foreground task and pings the main loop.
  - **That is how to run code on the GPUI thread from another thread:** a foreground task awaits a channel.
- **`AsyncApp::update(|cx| …)` borrows `App` through a `RefCell`.** Calling it while `App` is already borrowed panics. CEF handlers invoked *synchronously from inside a GPUI update* must not touch `App`; they should only write to shared `Rc<Cell>`/`RefCell` state. gpui-cef hit exactly this ("RefCell already borrowed") [V `pump.rs`]. Its macOS workaround (a `CFRunLoopTimer`, because "gpui's foreground executor barely makes progress" when idle) is macOS-specific. The Linux path is ping-based. [I]
- **There is no public way to add an fd or calloop source to GPUI's Linux loop.** `LinuxPlatform` keeps `event_loop` and `loop_handle` private, and `LinuxCommon::register_sources` is `pub(crate)`. Use a reactor that wakes a foreground task: `smol::Async<OwnedFd>`, whose async-io reactor thread wakes the waker [I], or a blocking thread plus a channel.
- **Frame timing.**
  - `window.on_next_frame(|w, cx| …)` runs after the next frame.
  - `window.request_animation_frame()` notifies the current view next frame.
  - `window.refresh_interval(cx)` gives the display refresh interval.
  - Use these to call CEF `send_external_begin_frame()` when `external_begin_frame_enabled` is on. [V APIs; I usage]
- **Quit.** `cx.on_app_quit(…)` futures are polled for at most `SHUTDOWN_TIMEOUT = 200 ms`. Implement Quit as: close every CEF browser, keep pumping until each `OnBeforeClose`, then `cx.quit()`. Use `on_window_should_close` to postpone a window close until its browsers have closed. [V APIs; I flow]

### 4.2 CEF pump choices (GPUI side)

- **External pump** (`CefSettings.external_message_pump = true`). CEF calls `OnScheduleMessagePumpWork(delay_ms)` from any thread. Forward the delay through an unbounded channel. A foreground task calls `cef::do_message_loop_work()` immediately, or after a timer, outside any `cx.update`. Sketch in §7.7. Everything runs on one thread; long CEF tasks stall the UI. [I]
- **`multi_threaded_message_loop = true`.** "Only supported on Windows and Linux" [V CEF 154 header]. CEF runs its own UI thread. `OnPaint` and other handlers arrive there; copy pixels there, off the GPUI thread, and send `Arc<RenderImage>` or GPU textures to a foreground task. Thread-affinity rules for CEF calls made from the GPUI thread belong to the CEF research. [I]
- **Process start.** `cef::execute_process(…)` must run first in `main` (subprocess re-exec), before `gpui_platform::application()`. Call `cef::shutdown()` after `Application::run` returns. That is gpui-cef's order [V README]. cef-rs 154 exports `execute_process`, `initialize`, `do_message_loop_work` and `shutdown` [V bindings].

---

## 5. Raw window handles [V]

`impl HasWindowHandle for Window` and `impl HasDisplayHandle for Window` (raw-window-handle 0.6) delegate to the platform window. 0.2.2 has the same impls.

- **X11:** `RawWindowHandle::Xcb(XcbWindowHandle { window: NonZeroU32, visual_id })` and `RawDisplayHandle::Xcb(XcbDisplayHandle { connection, screen })`. The XCB window id is the value CEF's `cef_window_info_t.parent_window` (`c_ulong`, an X11 `Window`) takes [V CEF bindings], both for windowed browsers and as the OSR "parent for dialogs/context menus/monitor info". gpui-webview converts it to an Xlib handle for Wry [V `linux.rs`].
- **Wayland:** `RawWindowHandle::Wayland(WaylandWindowHandle { surface: *wl_surface })` and `RawDisplayHandle::Wayland(*wl_display)`. CEF cannot use these as a parent. [I; see the CEF windowing research]
- The headless window returns `HandleError::NotSupported`.
- gpui-fast composition surfaces expose `platform_handle() -> Box<dyn Any>`, holding a `RawWindowHandle` on Linux [V].

---

## 6. Known limitations of GPUI apps on Linux that matter for a browser

| Limitation | Evidence | What Vsesvit must do [I] |
|---|---|---|
| No native context menus or menubar; popups inside the window only, except Wayland `AnchoredPopup`; nothing on X11 | §2.3, §2.9 [V] | Draw page context menus (CEF `RunContextMenu`), `<select>` popups (OSR `PET_POPUP` frames) and extension action popups (up to 800×600) in GPUI. Use `AnchoredPopup` on Wayland where the popup may leave the window. Small windows clip popups on X11. |
| No external texture or dma-buf drawing upstream | §3.1 [V] | Patch (§3.5) or pay the CPU-copy cost. |
| Key events have no physical codes | §2.4 [V] | Lossy VK/native code mapping; DOM `code` is unreliable. Patching gpui_linux to carry the xkb keycode is a small fork. |
| Drag and drop: files only; no outbound drag on X11 | §2.5 [V] | No drags of links, text or images out of the page, and no text/URL drops in, without platform code outside GPUI. |
| Clipboard: no `text/html` | §2.5 [V] | Chromium handles web clipboard itself; the chrome's text fields only need plain text. |
| No touch, pointer lock, shortcut inhibit or `xdg_toplevel_drag` | §2.3 [V] | Pointer Lock (games, remote desktop) and touch input are missing. Tab tear-off cannot follow the cursor on Wayland. |
| File dialogs take no filters; portal required | §2.7 [V] | Call ashpd directly for `<input accept>`; the window identifier must be built from the raw handles. |
| Notifications have no icons; tray missing | §2.7 [V] | Call notify-rust or ashpd directly; gpui-tray if a tray is needed. |
| Wayland focus stealing: `activate()` needs `app_id` and is usually refused; no way to use another process's token | §2.3 [V] | A single-instance handoff ("open URL in the running browser") may only get attention, not focus. |
| Accessibility is young | §2.10 [V] | Label the chrome with roles; mirror the OSR page tree yourself. |
| `inactive_frame_interval` defaults to 30 fps | §2.3 [V] | Set it to `None` for browser windows. |
| Cursor set is limited | §2.6 [V] | Map CEF cursor types to the nearest; no custom cursors. |
| Pre-1.0, weekly breaking changes (e.g. `Corner` → `Anchor`, `AsyncApp::update` result type) | §1.2 [V] | Pin exact versions; update deliberately. |
| GPL Zed UI crates | §Summary [V] | Use gpui-component (Apache-2.0) or our own code. |

---

## 7. Code patterns (as spelled in gpui-pre 0.3.9 / Zed main)

These are sketches checked against the 0.3.9 signatures. They were not compiled. CEF calls use cef-rs 154 names [V bindings].

### 7.1 Boot (Linux, CEF first)

```rust
use gpui::{App, AppContext as _, Bounds, WindowBounds, WindowDecorations, WindowOptions, px, size};

fn main() {
    // 1. CEF subprocesses re-exec this binary: call cef::execute_process(...) here and
    //    exit if it returns >= 0, before GPUI connects to a display server.
    // 2. cef::initialize(...) here, or inside `run` (gpui-cef does it inside on macOS).
    // Wayland when WAYLAND_DISPLAY is set; for windowed (Chrome-style) CEF children use:
    //   let app = gpui_platform::linux(gpui::WindowingModes::X11);
    let app = gpui_platform::application();
    app.run(|cx: &mut App| {
        gpui_component::init(cx); // or gpui_kit::init(cx)
        browser::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_id: Some("dev.mrquantumoff.Vsesvit".into()), // required for Wayland activation
                window_decorations: Some(WindowDecorations::Client),
                inactive_frame_interval: None,                    // don't throttle video in background windows
                ..Default::default()
            },
            |window, cx| cx.new(|cx| browser::BrowserWindow::new(window, cx)),
        )
        .expect("open window");
        cx.activate(true);
    });
    // 3. cef::shutdown() once run() returns (all browsers closed beforehand).
}
```

### 7.2 A stateful view with actions and key contexts

```rust
use gpui::{App, Context, Entity, FocusHandle, Focusable, KeyBinding, Window, actions, div, prelude::*};

actions!(browser, [NewTab, CloseTab, FocusOmnibox, Reload, TabSearch]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-t", NewTab, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("ctrl-l", FocusOmnibox, Some("BrowserWindow")),
        KeyBinding::new("ctrl-shift-a", TabSearch, Some("BrowserWindow")),
        KeyBinding::new("f5", Reload, Some("BrowserWindow")),
    ]);
}

pub struct BrowserWindow {
    focus_handle: FocusHandle,
    tabs: Vec<Entity<WebTab>>,
    active: usize,
}

impl BrowserWindow {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self { focus_handle: cx.focus_handle(), tabs: vec![cx.new(WebTab::new)], active: 0 }
    }
    fn new_tab(&mut self, _: &NewTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.push(cx.new(WebTab::new));
        self.active = self.tabs.len() - 1;
        cx.notify();
    }
}

impl Focusable for BrowserWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle { self.focus_handle.clone() }
}

impl Render for BrowserWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .key_context("BrowserWindow")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::new_tab))
            // .child(tab_sidebar.cached(style))  // cached views aren't re-rendered on page frames
            .child(div().flex_1().child(self.tabs[self.active].clone())) // WebTab: Render
    }
}
```

### 7.3 A web-view element painting CEF frames (stock CPU path, option A)

```rust
use std::{rc::Rc, sync::Arc};
use gpui::{App, Bounds, Corners, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
           Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, Pixels, RenderImage,
           Style, Window, relative};

pub struct WebSurface { pub tab: Entity<WebTab> }

impl IntoElement for WebSurface { type Element = Self; fn into_element(self) -> Self { self } }

impl Element for WebSurface {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;
    fn id(&self) -> Option<ElementId> { None }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> { None }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>,
                      window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>,
                bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> Hitbox {
        // Read by CEF's GetViewRect/GetScreenInfo from a shared cell (never borrows App).
        // Call host.was_resized() / notify_screen_info_changed() when this changes.
        self.tab.read(cx).shared.set_layout(bounds, window.scale_factor());
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>,
             bounds: Bounds<Pixels>, _: &mut (), hitbox: &mut Hitbox, window: &mut Window, cx: &mut App) {
        let tab = self.tab.read(cx);
        if let Some(frame) = tab.current_frame.clone() {
            // frame is exactly bounds × scale_factor device pixels → 1:1, no blur
            window.paint_image(bounds, bounds, Corners::default(), frame, 0, false).ok();
        }
        window.set_cursor_style(tab.shared.cursor(), hitbox);
        let focus = tab.focus_handle.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.tab.clone()), cx);
        // Mouse → CEF: window.on_mouse_event(move |e: &gpui::MouseDownEvent, phase, window, cx| { ... })
        // with `phase == DispatchPhase::Bubble && hitbox.is_hovered(window)`; translate to view DIP.
    }
}

/// CEF OnPaint (CEF thread) → this, through a channel into a foreground task:
fn rgba_frame(width: u32, height: u32, bgra: &[u8]) -> Option<Arc<RenderImage>> {
    let buf = image::ImageBuffer::from_raw(width, height, bgra.to_vec())?; // GPUI treats bytes as BGRA
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buf)])))
}
```

**Frame swap.** This follows Zed's `remote_video_track_view.rs` two-slot pattern. `WebTab::render` runs it each time a new frame has arrived:

```rust
// fields: latest: Option<Arc<RenderImage>>, current_frame, previous_frame
if let Some(latest) = self.latest.clone() {
    if let Some(cur) = self.current_frame.take() {
        if let Some(prev) = self.previous_frame.take() {
            if prev.id != cur.id { window.drop_image(prev).ok(); }   // free frame N-2 only
        }
        self.previous_frame = Some(cur);
    }
    self.current_frame = Some(latest);
}
// on release: drop both current_frame and previous_frame (cx.on_release / window.drop_image).
```

### 7.4 External texture variant (options C/C′), spelled as in gpui-ce

```rust
// gpui-ce, not upstream: Linux, wgpu 29. Draw a texture GPUI's device owns.
let ctx = gpui_wgpu::WgpuContextHandle::from_window(window).expect("wgpu backend");
// In OnAcceleratedPaint: import the dma-buf, then GPU-copy it into `target` before returning.
let target = gpui_wgpu::WgpuRenderTarget::new(&ctx, frame_size_in_device_pixels);
// in render/paint:
target.surface().object_fit(gpui::ObjectFit::Fill).size_full()
// cef-rs 154 (feature "accelerated_osr", wgpu 30):
//   let tex = cef::osr_texture_import::SharedTextureHandle::new(info).import_texture(ctx.device())?;
//   encoder.copy_texture_to_texture(tex.as_image_copy(), target_texture.as_image_copy(), extent);
```

### 7.5 Input handler bridging GPUI's IME to CEF (after gpui-cef `ime.rs`, MIT/Apache)

```rust
impl gpui::EntityInputHandler for WebTab {
    fn text_for_range(&mut self, r: std::ops::Range<usize>, adj: &mut Option<std::ops::Range<usize>>,
                      _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        Some(self.composition.utf16_slice_clamped(r, adj))     // mirror of the preedit only
    }
    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<gpui::UTF16Selection> {
        let caret = self.composition.utf16_len();
        Some(gpui::UTF16Selection { range: caret..caret, reversed: false })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<std::ops::Range<usize>> {
        let n = self.composition.utf16_len(); (n > 0).then_some(0..n)
    }
    fn unmark_text(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.composition.clear();
        cx.defer_in(w, |this, _, _| if let Some(h) = this.host() { h.ime_finish_composing_text(0) });
    }
    fn replace_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, text: &str,
                             w: &mut Window, cx: &mut Context<Self>) {
        // composing → host.ime_commit_text(text, invalid range, 0); plain key → KEYEVENT_CHAR events
        let (text, composing) = (text.to_owned(), !self.composition.is_empty());
        self.composition.clear();
        cx.defer_in(w, move |this, _, _| this.commit(&text, composing));
    }
    fn replace_and_mark_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, text: &str,
                                      sel: Option<std::ops::Range<usize>>, w: &mut Window, cx: &mut Context<Self>) {
        self.composition.set(text);                         // empty text = cancel → ime_cancel_composition()
        let text = text.to_owned();
        cx.defer_in(w, move |this, _, _| this.compose(&text, sel)); // host.ime_set_composition(...)
    }
    fn bounds_for_range(&mut self, r: std::ops::Range<usize>, element: gpui::Bounds<gpui::Pixels>,
                        _: &mut Window, _: &mut Context<Self>) -> Option<gpui::Bounds<gpui::Pixels>> {
        // from RenderHandler::on_ime_composition_range_changed, in view coordinates
        self.shared.composition_bounds(r).map(|mut b| { b.origin += element.origin; b })
    }
    fn character_index_for_point(&mut self, _: gpui::Point<gpui::Pixels>, _: &mut Window,
                                 _: &mut Context<Self>) -> Option<usize> { None }
}
```

`cx.defer_in` is used because CEF calls must not run while GPUI is inside the platform input callback. Only `RAWKEYDOWN`/`KEYUP` go to CEF for keys; text arrives through the handler [V gpui-cef README].

### 7.6 Overlay (omnibox dropdown) inside the window

```rust
div().relative().child(omnibox_input).when(self.suggestions_open, |el| {
    el.child(gpui::deferred(
        gpui::anchored().anchor(gpui::Anchor::TopLeft).snap_to_window_with_margin(gpui::px(8.))
            .child(suggestion_list.on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.suggestions_open = false; cx.notify();
            }))),
    ).priority(1))
})
```

### 7.7 CEF external pump driven from GPUI (sketch, Linux)

```rust
use futures::{StreamExt as _, channel::mpsc, future::{self, Either}};
use std::time::Duration;

/// `tx` goes to BrowserProcessHandler::on_schedule_message_pump_work: tx.unbounded_send(delay_ms).
pub fn start_cef_pump(cx: &mut gpui::App, mut rx: mpsc::UnboundedReceiver<i64>) {
    const FALLBACK: Duration = Duration::from_millis(1000 / 30); // safety tick when nothing is scheduled
    cx.spawn(async move |cx| {
        let mut wait = Duration::ZERO;
        loop {
            let timer = cx.background_executor().timer(wait);
            match future::select(rx.next(), timer).await {
                Either::Left((Some(delay_ms), _cancelled_timer)) => {
                    wait = Duration::from_millis(delay_ms.max(0) as u64);
                    if !wait.is_zero() { continue; }      // re-arm with the shorter delay
                }
                Either::Left((None, _)) => break,           // CEF shut down
                Either::Right(((), _)) => {}
            }
            // App is NOT borrowed here, so CEF callbacks may use a stored AsyncApp to update GPUI.
            cef::do_message_loop_work();
            wait = FALLBACK;
        }
    })
    .detach();
}
```

---

## 8. Open questions and suggested spikes

1. **Which CEF architecture wins** (Alloy OSR vs Chrome-style windowed; see `gpt-extension-architecture.md`)? That picks gpui-pre plus a texture patch, or gpui-fast plus composition on X11.
2. **Will the next gpui-pre snapshot handle Zed's move to a git wgpu 30** (#65649, 2026-10-15), given the script aborts on non-optional git dependencies? Does a crates.io wgpu 30.0.x with the Vulkan surface-drop fix exist yet?
3. **Does CEF 154 `OnAcceleratedPaint` work on Linux with the NVIDIA 615 proprietary driver,** and which DRM modifiers and formats does it produce? Can GPUI's wgpu device import them? (gpui-video lists NVIDIA VA-API dma-buf as "GPU texture upload", not zero-copy.)
4. **Upstream status of #62379** (composition), **#64061** (dynamic textures) and a Linux surface/texture API.
5. **Measure option A's real cost** at 1080p and 4K with GPUI's atlas, including the per-frame texture allocation, on this machine.
6. **Wayland-native GPUI with CEF OSR:** does CEF need an X11 display (XWayland) for clipboard, IME, dialogs? If so, IME and clipboard live partly in Chromium's X11 path and partly in GPUI's Wayland path.
7. **Keyboard fidelity:** is a small gpui_linux patch acceptable to carry raw xkb keycodes in `KeyDownEvent` for CEF?

---

## Sources

Registries and crate sources:
- crates.io API: `gpui` <https://crates.io/api/v1/crates/gpui>, `gpui-pre` <https://crates.io/crates/gpui-pre>, `gpui-component` <https://crates.io/crates/gpui-component> (with its dependency endpoints for 0.2.0–0.7.3), `gpui-fast` <https://crates.io/crates/gpui-fast>, `gpui-ce` <https://crates.io/crates/gpui-ce>, `gpui-tray` <https://crates.io/crates/gpui-tray>, reverse dependencies of `gpui` and `gpui-pre`.
- `gpui` 0.2.2 crate source, read locally from the crates.io download.
- `gpui-pre*` 0.3.9 crate sources, read locally from the crates.io downloads.
- `wgpu-hal` 29.0.4 and 30.0.1 `src/vulkan/adapter.rs`, `device.rs`: <https://docs.rs/wgpu-hal/29.0.4>, <https://docs.rs/wgpu-hal/30.0.1>.
- cef-rs 154.5.0+154.0.34 (`src/osr_texture_import/dmabuf.rs`, bindings) and cef-dll-sys 154.5.0+154.0.34 bindings (`_cef_render_handler_t`, `_cef_accelerated_paint_info_t`, `cef_runtime_style_t`, `_cef_window_info_t`, `_cef_settings_t` docs): <https://github.com/tauri-apps/cef-rs>, <https://docs.rs/cef/154.5.0+154.0.34>. Upstream header: <https://github.com/chromiumembedded/cef/blob/master/include/internal/cef_types_runtime.h>.

Zed sources (rev `d678e7de`):
- <https://github.com/zed-industries/zed/tree/main/crates/gpui>: `README.md`, `src/{assets.rs,window.rs,app.rs,executor.rs,input.rs,element.rs,action.rs,platform.rs,platform/popup.rs,platform/keystroke.rs,elements/{img.rs,surface.rs,anchored.rs,deferred.rs,canvas.rs},view.rs,_accessibility.rs}`, `examples/{hello_world,input,popover,move_entity_between_windows}.rs`.
- <https://github.com/zed-industries/zed/tree/main/crates/gpui_linux>: `src/linux.rs`, `display_connection.rs`, `dispatcher.rs`, `platform.rs`, `wayland/{client,window,clipboard}.rs`, `x11/{client,window}.rs`, `system_notifications.rs`.
- <https://github.com/zed-industries/zed/tree/main/crates/gpui_wgpu>: `wgpu_atlas.rs`, `wgpu_renderer.rs`, `wgpu_context.rs`, `shaders.wgsl`.
- <https://github.com/zed-industries/zed/tree/main/crates/gpui_platform>: `gpui_platform.rs`.
- <https://github.com/zed-industries/zed/blob/main/crates/livekit_client/src/remote_video_track_view.rs>, `livekit_client/playback.rs`.
- Zed root `Cargo.toml` (wgpu git rev, `[patch.crates-io]`), `rust-toolchain.toml`, `script/linux`.

Zed pull requests:
- #46758 wgpu replaces blade: <https://github.com/zed-industries/zed/pull/46758>
- #56065 AccessKit: <https://github.com/zed-industries/zed/pull/56065>
- #60232 AnchoredPopup: <https://github.com/zed-industries/zed/pull/60232>
- #62379 window composition: <https://github.com/zed-industries/zed/pull/62379>
- #61945 alternative to #62379, closed: <https://github.com/zed-industries/zed/pull/61945>
- #64061 dynamic textures: <https://github.com/zed-industries/zed/pull/64061>
- #52669 wgpu surface element, closed: <https://github.com/zed-industries/zed/pull/52669>
- #65649 wgpu 30 git: <https://github.com/zed-industries/zed/pull/65649>
- #64353 macOS anchored popups: <https://github.com/zed-industries/zed/pull/64353>
- #62557 atlas bleeding: <https://github.com/zed-industries/zed/pull/62557>
- #63313 skip GL when Vulkan works: <https://github.com/zed-industries/zed/pull/63313>
- #63346 skip software Vulkan ICDs: <https://github.com/zed-industries/zed/pull/63346>
- GPUI site: <https://www.gpui.rs/>

GPUI forks and libraries:
- GPUI Kit: <https://github.com/longbridge/gpui-kit>: `README.md`, root `Cargo.toml`, `script/bump-gpui.ts`, `crates/kit/src/lib.rs`, `crates/component/src/**`, `crates/component/src/native_menu/{mod,fallback}.rs`, `crates/webview/{README.md,src/linux.rs,src/composition.rs}`, `website/docs/{webview,comparison}.md`, `website/component/*.md`.
- GPUI Fast: <https://github.com/longbridge/gpui-fast>: `README.md`, `UPSTREAM`, `crates/gpui/src/fast/composition.rs`, `crates/gpui_linux/src/fast/composition/{wayland,x11}.rs`, `crates/gpui_perf/examples/native_webview.rs`.
- GPUI CE: <https://github.com/gpui-ce/gpui-ce>: README, `crates/gpui/src/elements/surface.rs`, `crates/gpui_wgpu/src/wgpu_renderer/surfaces/texture.rs`, `crates/gpui_wgpu/src/wgpu_context.rs`; PRs <https://github.com/gpui-ce/gpui-ce/pull/39> and <https://github.com/gpui-ce/gpui-ce/pull/237>.
- gpui-cef (bokuweb): <https://github.com/bokuweb/gpui-cef>: README, `crates/gpui-cef/src/platform/cef/{pump,ime,input,handlers,shared,mod}.rs`.
- gpui-video: <https://github.com/zortax/gpui-video>: README, `Cargo.toml`, `src/dmabuf.rs`.
- infiniterm: <https://github.com/ekinertac/infiniterm>, `spikes/cef-frame/NOTES.md`.
- gpui-pre forks (repository descriptions): <https://github.com/alphabet-h/gpui-pre>, <https://github.com/feigeCode/gpui-pre>, <https://github.com/NortheLabs/gpui-pre>, <https://github.com/ducktape-industries/gpui-pre>, <https://github.com/ginwzy/gpui-pre>.

Local measurement: `/tmp/vsesvit-research/gpui-bench/memcpy_bench.c`, built with `gcc -O2` and run twice on this machine (Ryzen 9 9950X). `nvidia-smi` reports RTX 5080, PCIe gen 5 x16, driver 615.78.08.
