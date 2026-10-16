# CEF off-screen rendering (OSR) on Linux, for a GPUI + CEF Vsesvit shell

Research date: 2026-10-16. Scope: CEF windowless rendering on Linux as it stands in CEF 154 (the version the `cef` crate 154.5.0+154.0.34 pins), CEF 156 (beta) and CEF master, plus the `cef` crate's `accelerated_osr` feature and how it fits GPUI.

Legend: **VERIFIED** means I read the source, header or primary page cited next to the claim. **INFERRED** means I reasoned from verified material and did not test it. Nothing here was built or run. The only exception is the local driver probe in §1.6, where I ran `nvidia-smi`, `vulkaninfo` and `eglinfo`.

Sources I read on disk:

- CEF branch `8037` at `14c5a08`: `/tmp/vsesvit-research/cef/cef-8037`. This commit is exactly CEF 154.0.34, the version the crate binds.
- CEF `master` at `a875f40` (2026-10-10): `/tmp/vsesvit-research/cef/cef-master`.
- `cef` crate 154.5.0+154.0.34 and `cef-dll-sys` in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.
- The cef-rs repo at `163744c`: `/tmp/vsesvit-research/cef-rs/repo`.
- `wgpu-hal` 30.0.1, `gpui` 0.2.2, `blade-graphics` 0.7.1 and Zed `main`'s `gpui_wgpu`: `/tmp/vsesvit-research/crates-osr/`.
- Chromium files at tag `154.0.8037.98`: `/tmp/vsesvit-research/chromium-osr/`.

---

## Executive summary

These are the points that matter for a GPUI + CEF Linux browser that aims for maximal Chrome-extension compatibility.

1. **OSR is the only way to put CEF inside a native-Wayland GPUI window, and OSR forces Alloy style.**
   - CEF's native windowed embedding takes an X11 `Window` as parent. `cef_window_handle_t` is `unsigned long` (VERIFIED, `include/internal/cef_types_linux.h:67`).
   - Wayland embedding is still open as issue #2804 (VERIFIED). The maintainer restated in Oct 2026 that "Native windowed embedding remains X11-only" (#3687, VERIFIED).
   - `windowless_rendering_enabled` always yields an Alloy-style browser (VERIFIED, `cef_types_runtime.h:49`, `cef_types_linux.h:147`). Chrome-style OSR was folded into "OSR = Chrome bootstrap + Alloy style" (#3293, VERIFIED) and is not planned.

2. **Consequence for extensions (the biggest risk to "total extension compatibility").**
   - What works: the Chrome extension *engine* runs at the profile level, and Alloy browsers get Chrome's `TabHelpers` (VERIFIED, `libcef/browser/alloy/browser_platform_delegate_alloy.cc:206`). Content scripts, service workers, storage, declarativeNetRequest and webRequest should therefore work in OSR tabs (INFERRED; maintainer: "Extension functionality that works at the RequestContext level ... should already work with Alloy style", #3859, VERIFIED).
   - What is missing: the UI that depends on Chrome's `Browser` object.
     - `chrome.tabs.query` and `chrome.windows` enumerate `GetAllBrowserWindowInterfaces()`, which holds no OSR tabs (VERIFIED, Chromium `tabs_api.cc:1852-1861`). CEF patches only `ExtensionTabUtil::GetTabById` and parts of `tabs.update` for Alloy browsers ("minimal support for tabs API", VERIFIED, `patch/patches/chrome_browser_extensions.patch`).
     - There is no action toolbar or popup UI.
     - Alloy context menus come from CEF's own simple model, so `chrome.contextMenus` items do not appear (VERIFIED that the model is CEF's, `libcef/browser/menu_manager.cc:334+`; INFERRED that the items are absent).
     - `chrome://extensions`, `chrome://settings` and other WebUI hosts are blocked in Alloy browsers (VERIFIED allowlist, `alloy_browser_host_impl.cc:62-101`).
     - Permission prompts default to IGNORE (VERIFIED).
   - "Behaving as in Chrome" therefore needs one of these. The architecture lead must make this call early.
     - (a) A Chromium/CEF patch set that gives GPUI-hosted Alloy tabs a `BrowserWindowInterface`/`TabListInterface` identity.
     - (b) A GPUI X11-only build that hosts *windowed Chrome-style* child browsers. CEF supports a Chrome style child window with an external parent on Linux/X11 (VERIFIED, `chrome_child_window.cc`). This gives up native Wayland and loses GPUI overlays over web content.
     - (c) Accept the Alloy gaps and shim the tabs, windows, action and contextMenus surfaces ourselves.

3. **Zero-copy GPU frames exist on Linux but are not usable on this machine with CEF 154.**
   - `OnAcceleratedPaint` delivers dmabuf planes (fd, stride, offset, size), a DRM modifier and BGRA/RGBA format. It has done so since CEF 124 (VERIFIED). The buffers come from Chromium's `FrameSinkVideoCapturer` (VERIFIED).
   - CEF 154 asks for *mappable* buffers (`kPreferMappableSharedImage`, which means `GBM_BO_USE_LINEAR`). That breaks GPU rendering on NVIDIA (#4237, VERIFIED in code: `video_consumer_osr.cc:49-59` on 8037).
   - The fix (`kPreferSharedImageWithNativeHandle`) landed on master and on CEF 156's branch 8078 on 2026-10-09. It is first in build 156.0.3 (VERIFIED). It was not backported to 8037, whose head is still `14c5a08`.
   - Even on CEF 156, NVIDIA does not attach implicit fences to these dmabufs. Clients then read half-drawn frames (#4313, opened 2026-10-16 on driver 615.78.08, the exact driver on this machine; fix PR #4314 is open).
   - Recommendation: build the CPU `OnPaint` path first. It is always available, and CEF falls back to it whenever GPU compositing is disabled (VERIFIED). Keep the dmabuf path behind a flag until CEF ≥ 156 with #4314 and a matching `cef` crate exist.

4. **The `cef` crate's `accelerated_osr` importer is demo-quality.**
   - It imports dmabufs into a `wgpu` 30 texture through the Vulkan HAL. It has these defects (VERIFIED by reading `src/osr_texture_import/dmabuf.rs`):
     - It leaks a `VkDeviceMemory`, and with it the dmabuf, on every frame.
     - It omits `VkExternalMemoryImageCreateInfo` and `vkGetMemoryFdPropertiesKHR`.
     - It has no dedicated allocation, no foreign-queue acquire and no synchronization.
     - It imports only plane 0.
     - Its "CPU fallback" is an empty texture.
   - Its own example keeps the imported texture past the callback, which CEF forbids.
   - `wgpu-hal` 30 already ships a spec-correct single-plane `vulkan::Device::texture_from_dmabuf_fd` (VERIFIED). Use that instead.
   - Neither GPUI renderer can take the result today:
     - gpui 0.2.2 (crates.io) renders with blade 0.7, whose dmabuf import hard-codes `OPTIMAL` tiling.
     - Zed `main` renders Linux with wgpu, but its renderer drops `PrimitiveBatch::Surfaces` on Linux and requests only `DUAL_SOURCE_BLENDING` (all VERIFIED).
     - So a GPUI patch is needed in either case: an external-texture primitive plus the `VULKAN_EXTERNAL_MEMORY_DMA_BUF` device feature.

5. **Input is Windows-shaped, and the Linux key path has known bugs in 154.**
   - `native_key_code` must be the XKB keycode, because CEF calls `KeycodeConverter::NativeKeycodeToDomCode` (VERIFIED). `windows_key_code` must be the layout-independent VKEY.
   - For key-down and key-up, CEF derives the character and `DomKey` from `windows_key_code` through a US keysym table when Ozone/X11 support is compiled in (VERIFIED, `browser_platform_delegate_native_linux.cc:244-290`). Non-US text and `KeyboardEvent.key` are therefore wrong (#3273).
   - #3273 was closed on 2026-10-10 in favour of a new unified input API, #4306, milestoned M158.
   - GPUI's `Keystroke` carries no hardware keycode (VERIFIED). `KeyboardEvent.code` needs a GPUI patch; the keycode is available inside `keystroke_from_xkb` (VERIFIED).
   - Send typed text through `ImeCommitText` rather than `KEYEVENT_CHAR`. `character` is a single `char16_t`, so emoji and other non-BMP characters cannot be sent as `KEYEVENT_CHAR`. Mouse back and forward buttons do not exist in the 154 API (`MBT_LEFT/MIDDLE/RIGHT` only, VERIFIED).

6. **Everything Chrome would draw, the GPUI host must draw.**
   - Context menus: the default OSR menu needs an X11 parent handle (VERIFIED).
   - JavaScript dialogs: same requirement (VERIFIED).
   - File chooser: the default in 154 is the xdg-desktop-portal dialog (VERIFIED). It is cancelled under `multi_threaded_message_loop` (VERIFIED).
   - Permission prompts, tooltips (the return value is ignored in OSR, VERIFIED), select and date popups (composited as `PET_POPUP`), drag and drop, find bar and downloads UI.
   - Accessibility: OSR only emits a JSON-ish tree to `CefAccessibilityHandler`, with no AT-SPI (VERIFIED).
   - Not available in OSR at all: print preview (VERIFIED) and video picture-in-picture (VERIFIED). DevTools always opens as a separate windowed Chrome-style browser (VERIFIED).

7. **The message loop needs a GLib decision.**
   - GPUI's Linux loop is calloop and links no GLib (VERIFIED).
   - With `external_message_pump`, CEF's pump subclasses `MessagePumpGlib`. Chromium's X11 and Wayland event sources and its work source are attached to `g_main_context_default()` on the main thread (all VERIFIED). The host must therefore iterate the default GLib context as well as calling `CefDoMessageLoopWork()`.
   - That idle work source spins at 100% CPU because it always asks for a zero timeout (tauri-apps/tauri#16189, VERIFIED report).
   - Not iterating GLib plausibly explains reports that X11 clipboard data never reaches other apps (INFERRED).
   - `multi_threaded_message_loop` is supported on Linux (VERIFIED, `context.cc:366-371`). CEF's UI thread then gets its own GLib context and nothing touches GPUI's thread. The cost is no GtkUi, so default file dialogs are cancelled, and every callback must be marshalled to GPUI.
   - Recommendation: `multi_threaded_message_loop = true`. Implement `CefDialogHandler` with ashpd, which GPUI already depends on.

---

## 0. Versions in play

| Item | Value | Status |
|---|---|---|
| `cef` crate / `cef-dll-sys` | 154.5.0+154.0.34 (latest on crates.io; no 156 release yet) | VERIFIED (crates.io API, local registry) |
| CEF build bound by the crate | `154.0.34+g14c5a08+chromium-154.0.8037.98` (`CEF_VERSION` in `cef-dll-sys/src/bindings/x86_64_unknown_linux_gnu.rs:38`) | VERIFIED |
| CEF branch | 8037. Its head on GitHub is still `14c5a08` today. | VERIFIED (`git ls-remote`) |
| Newest CEF Linux builds | 154.0.34 stable (2026-10-05). 156.0.3 beta (2026-10-10, `g3adefb8`, includes the NVIDIA fix). 156.0.7 beta (2026-10-11). | VERIFIED (https://cef-builds.spotifycdn.com/index.json) |
| CEF master | Chromium 156.0.8078.0 (`CHROMIUM_BUILD_COMPATIBILITY.txt`). Branch 8078 is 156.0.8078.17. | VERIFIED |
| `accelerated_osr` deps | `ash` 0.38, `wgpu` 30, `libc`, `thiserror`, `tracing` (`cef/Cargo.toml`) | VERIFIED |
| gpui 0.2.2 (crates.io, 2025-10-22) | Linux renderer is `blade-graphics` 0.7. Event loop is calloop. No GLib. | VERIFIED (`gpui-0.2.2/Cargo.toml`) |
| Zed `main` GPUI | Linux renderer is `gpui_wgpu`, on wgpu git rev `3b3506ea` (workspace version 30.0.0, 2026-09-12). Event loop is calloop 0.14. | VERIFIED |

Upgrading CEF means new `cef-dll-sys` bindings. The crate's `build.rs` checks the downloaded archive against the package version (`download_cef::check_archive_json`). A CEF 156 binary therefore cannot simply be dropped under the 154 crate (VERIFIED that the check exists; INFERRED that it would reject the binary). cef-rs ships an `update-bindings` tool for this.

---

## 1. Accelerated OSR on Linux (`OnAcceleratedPaint`, dmabuf)

### 1.1 API surface (CEF 154)

- **Turning it on (VERIFIED, `include/internal/cef_types_linux.h`):**
  - Set `CefSettings.windowless_rendering_enabled = 1`.
  - Per browser, set `cef_window_info_t.windowless_rendering_enabled = 1` and `shared_texture_enabled = 1`.
  - The header comment still says "Currently only supported on Windows (D3D11)" (line ~130). It is stale: the Linux path is implemented.
- **Callback (VERIFIED, `cef_render_handler.h:170`):** `CefRenderHandler::OnAcceleratedPaint(browser, PaintElementType type, const RectList& dirtyRects, const CefAcceleratedPaintInfo& info)`. The documented contract is quoted below.

  > "The underlying implementation uses a pool to deliver frames... The handle's resource cannot be cached and cannot be accessed outside of this callback. It should be reopened each time this callback is executed and the contents should be copied to a texture owned by the client application. The contents of |info| will be released back to the pool after this callback returns."

  The implementation enforces it: `ScopedVideoFrameDone` calls `Done()` when `OnFrameCaptured` returns (`libcef/browser/osr/video_consumer_osr.cc:15-26, 96`).
- **Linux struct (VERIFIED, `cef_types_linux.h`; Rust in `cef` bindings `AcceleratedPaintInfo`, `AcceleratedPaintNativePixmapPlaneInfo`):**
  - `cef_accelerated_paint_native_pixmap_plane_t { uint32_t stride; uint64_t offset; uint64_t size; int fd; }` ("Sync with native_pixmap_handle.h").
  - `cef_accelerated_paint_info_t { size_t size; planes[kAcceleratedPaintMaxPlanes=4]; int plane_count; uint64_t modifier; cef_color_type_t format; cef_accelerated_paint_info_common_t extra; }`.
  - `extra` (`cef_types_osr.h`) carries `timestamp`, `coded_size`, `visible_rect`, `content_rect`, `source_size`, `capture_update_rect`, `region_capture_rect`, `capture_counter` and `has_*` flags.
- **Format (VERIFIED, `video_consumer_osr.cc:128-133`):** `CEF_COLOR_TYPE_BGRA_8888` for `PIXEL_FORMAT_ARGB` (DRM `ARGB8888`) and `CEF_COLOR_TYPE_RGBA_8888` for `PIXEL_FORMAT_ABGR` (DRM `ABGR8888`). The source comments that "On Linux, X11 lacks support for RGBA_8888 so it might be BGRA."
- **fds and planes (VERIFIED, `video_consumer_osr.cc:186-207`):** the fds are `plane.fd.get()`, owned by CEF and valid only during the callback. `dup()` them if the importer takes ownership. Vulkan's `vkAllocateMemory` consumes the fd, as noted in #3687 by a user and in `wgpu-hal` docs. Plane count is usually 1 for ARGB. Compressed modifiers can add auxiliary planes (INFERRED).
- **Modifier:** `native_pixmap.modifier` may be `DRM_FORMAT_MOD_INVALID`. Master's reference importer omits the modifier attributes in that case and relies on the implicit layout (VERIFIED, `tests/shared/browser/osr_renderer_gl_linux.cc:138-161` on master).
- **Use `extra.visible_rect`:** the dmabuf is `coded_size`, but only `visible_rect` holds the image. Master's sample copies exactly `visible_rect` (VERIFIED, `osr_renderer_gl_linux.cc:530-555`).

### 1.2 How frames are produced (VERIFIED)

Both OSR paths now run through Chromium's viz `FrameSinkVideoCapturer`, not a software output device.

- `CefVideoConsumerOSR` starts the capturer with `media::PIXEL_FORMAT_ARGB`, auto-throttling off and a fixed resolution equal to the view size in pixels (`video_consumer_osr.cc:30-45`).
- The capturer is started with a buffer preference.
  - CEF 154 asks for `kPreferMappableSharedImage` when `use_shared_texture_` is set and `kDefault` (shmem) otherwise (`video_consumer_osr.cc:49-59`, 8037).
  - Master and 8078 ask for `kPreferSharedImageWithNativeHandle` on Linux (diff in §1.3).
- The CPU path maps the read-only shmem region and calls `OnPaint` with a BGRA buffer (`video_consumer_osr.cc:212-244`). With GPU compositing this is a GPU→CPU readback every frame (INFERRED).
- When GPU compositing is disabled (blocklist, `--disable-gpu-compositing`, repeated GPU crashes), no video consumer is created. `CefHostDisplayClientOSR`/`SoftwareOutputDeviceProxy` then call `OnPaint` *even if `shared_texture_enabled` was set* (`render_widget_host_view_osr.cc:427-436, 1891-1898`; `host_display_client_osr.cc:101-103`).
  - **A host must implement both `OnPaint` and `OnAcceleratedPaint`.**
  - The `OnPaint` doc comment says it is "only called when shared_texture_enabled is set to false". That does not cover this fallback.
- `docs/general_usage.md` still says "Off-screen rendering does not currently support accelerated compositing". That is stale: the capturer path is GPU-composited.

### 1.3 History and current state

| When | What | Source |
|---|---|---|
| 2024-04-27 (CEF 124, branch 6367) | "osr: Implement shared texture support (fixes #1006, fixes #2575)" by reito. Commit message: "Verified to work on macOS and Windows. Linux support is present but not implemented for cefclient, so it is not verified to work." Branch 6312 (123) lacks the Linux struct; 6367 has it. | VERIFIED: commit 77c1e82 via GitHub API; raw headers of branches 6312 and 6367 |
| ~CEF 132 (branch 6834) | `extra` (`cef_accelerated_paint_info_common_t`) appears in the Linux struct. | VERIFIED for 6834/6943/6998/7103/7151 headers. The 7049 fetch returned no match, probably a fetch glitch. |
| 2025-06/07 | #3953: shared-texture OSR on Linux fails unless `--use-angle=gl-egl`, because `OzoneImageBacking::ProduceSkia()` needs `GetGLOzone()`. GTK on Wayland asserted, so `--ozone-platform=x11` was chosen. Fixed in cf5fddc by making **cefclient** add these switches. | VERIFIED: #3953 page; `tests/shared/browser/client_app_browser.cc:76-99` on 8037 |
| 8037 (CEF 154, the crate's version) | `kPreferMappableSharedImage`, which maps to `SCANOUT_CPU_READ_WRITE` / `GBM_BO_USE_LINEAR`. On NVIDIA: "Unable to initialize SkSurface", so frames are empty. | VERIFIED: code; #4237 |
| 2026-10-09 | 3adefb8 "osr: linux: Prefer native-handle SharedImages (fixes #4237) (#4238)" on master and applied to 8078 ("Done"). It needs a Chromium build containing crrev.com/c/8220427, which M156 has. **Not on 8037.** | VERIFIED: commit API; PR #4238; `diff` of the 8037 and master sources |
| 2026-10-10 | cefclient gains Linux `OnAcceleratedPaint`: a GL 3.3 renderer that imports via EGL, with GtkGLArea on Wayland and an EGL window surface on X11. It also implements `--external-begin-frame-enabled` and makes GDK follow the Ozone platform. Master drops the forced `--ozone-platform=x11`, but still forces `--use-angle=gl-egl`. Tested only on VMware SVGA3D/Mesa. Milestone M156. | VERIFIED: commits c7e1a9e, 7219f4e, e35fdcb; #3687; master `client_app_browser.cc` |
| 2026-10-16 (open) | #4313: with native-handle images, `CopyOutputRGBAInTexture` waits for the GPU only for mappable destinations. CEF passes no fence, and NVIDIA attaches no implicit fence, so clients see partly drawn frames ("12 to 14 of 72 screenshots"). Fix PR #4314 imports Chromium's write fence into each dmabuf with `DMA_BUF_IOCTL_IMPORT_SYNC_FILE` (kernel ≥ 6.0). | VERIFIED: issue page |

### 1.4 Required flags and settings

All of these are VERIFIED unless marked.

- `CefSettings.windowless_rendering_enabled = 1`. Creating a windowless browser without it logs an error (`browser_host_create.cc:73-76`).
- `CefWindowInfo.windowless_rendering_enabled = 1` and `shared_texture_enabled = 1`.
  - `external_begin_frame_enabled = 1` is optional (§1.7).
  - `parent_window` is an X11 XID. On Wayland pass 0; the consequences are in §4.
- GPU compositing must stay on. Do not pass `--disable-gpu` or `--disable-gpu-compositing`.
- `--use-angle=gl-egl` must be added by the app in `CefApp::OnBeforeCommandLineProcessing`. libcef does not add it; only the cefclient sample does (`client_app_browser.cc:76-99`).
- `--ozone-platform`:
  - On 154, the sample adds `x11` "to ensure that X11 semantics are preserved, which is necessary for compatibility with some GDK/X11 integrations (e.g. Wayland with AMD)".
  - On master, the sample no longer adds it, and native Wayland OSR with DMA-BUF is tested.
  - On a Wayland session, `x11` means CEF talks to XWayland (`:0` here).
- No `--enable-features=...` switch is used by CEF or its sample for this path. I found none in `libcef` or `tests/shared` (VERIFIED by grep). VMs need `--ignore-gpu-blocklist` (c7e1a9e message).

### 1.5 Consumer requirements (driver side)

- **EGL import** (master sample): `EGL_EXT_image_dma_buf_import`, plus `EGL_EXT_image_dma_buf_import_modifiers` when a modifier is given. After the copy it calls `glFenceSync` and `glClientWaitSync`, because "CEF reuses the buffer after the callback returns" (`osr_renderer_gl_linux.cc:251-262`).
- **Vulkan import:** `VK_KHR_external_memory_fd`, `VK_EXT_external_memory_dma_buf` and `VK_EXT_image_drm_format_modifier`. The modifier must be supported for the format with `SAMPLED`/`TRANSFER_SRC` usage. Correct use also calls for an acquire from `VK_QUEUE_FAMILY_FOREIGN_EXT` (needs `VK_EXT_queue_family_foreign`) and a wait on the implicit fence through `DMA_BUF_IOCTL_EXPORT_SYNC_FILE`. That wait is useless on NVIDIA until #4314 lands (INFERRED from #4313).
- The producer is Chromium's GPU process and the consumer is GPUI's device. Both must be the same physical GPU unless the buffer is linear (INFERRED). This machine has one dGPU plus llvmpipe.

### 1.6 This machine (local probe, VERIFIED)

- NVIDIA GeForce RTX 5080, NVIDIA Open Kernel Module **615.78.08**. Kernel 7.2.10-200.fc44. KDE Plasma Wayland (`XDG_CURRENT_DESKTOP=KDE`), `WAYLAND_DISPLAY=wayland-0`, `DISPLAY=:0`.
- Vulkan (NVIDIA driver 615.78.8.0, API 1.4.354) exposes `VK_EXT_external_memory_dma_buf`, `VK_EXT_image_drm_format_modifier`, `VK_KHR_external_memory_fd`, `VK_EXT_queue_family_foreign`, `VK_KHR_external_semaphore_fd` and `VK_KHR_external_fence_fd`.
- EGL (NVIDIA, GBM/Wayland/X11 platforms) exposes `EGL_EXT_image_dma_buf_import`, `..._modifiers`, `EGL_ANDROID_native_fence_sync` and `EGL_MESA_image_dma_buf_export`.
- So the consumer side can import. The producer side is the problem:
  - CEF 154 allocates linear CPU-mappable buffers, which NVIDIA cannot render into (#4237).
  - CEF 156 delivers frames without fences on exactly this driver version (#4313).

### 1.7 Frame rate and BeginFrame control (VERIFIED)

- `CefBrowserSettings.windowless_frame_rate` defaults to 30. `osr_util::ClampFrameRate` maps values below 1 to 30 and has **no upper cap**; branches 6367 through 8037 are identical here.
- `SetFrameRate()` applies it to `compositor_->SetDisplayVSyncParameters()` and `video_consumer_->SetFrameRate()` (min capture period). See `render_widget_host_view_osr.cc:1702-1729`.
- `CefBrowserHost::SetWindowlessFrameRate(n)` changes the rate at runtime through `UpdateFrameRate()`, which also updates guest views.
- cefclient defaults to 60 with shared textures and 30 without (`main_context_impl.cc:75-77`).
- `external_begin_frame_enabled`: the client calls `CefBrowserHost::SendExternalBeginFrame()`.
  - Each call issues one `viz::BeginFrameArgs` (with `DefaultInterval()`) to the compositor and forwards it to the popup view.
  - Calls are ignored while one is pending; `begin_frame_pending_` clears in `OnFrameComplete` (`render_widget_host_view_osr.cc:1182-1217`).
- For GPUI there are two choices.
  - Set `windowless_frame_rate` to the output's refresh rate and update it when the window moves between monitors.
  - Or enable external BeginFrames and call `SendExternalBeginFrame()` from GPUI's frame callback. Frames still arrive asynchronously, one or more frames later (INFERRED).
- `WasHidden(true)` stops painting. Use it for background tabs; the "Memory Saver" logic is separate.

### 1.8 Fallback strategy (recommended)

- CEF does not fall back automatically when the client fails to import a dmabuf. `OnPaint` is called only on the software-compositing path (VERIFIED, §1.2).
- A robust host should:
  1. Probe: create browsers with `shared_texture_enabled` only if the import self-test passes and the CEF version is known good for the vendor.
  2. If import fails at runtime, recreate the browser with `shared_texture_enabled=0`.
  3. Always implement `OnPaint` (INFERRED design).
- For NVIDIA with CEF 154, start with `shared_texture_enabled=0`.

---

## 2. The `cef` crate's `accelerated_osr` feature

### 2.1 What it is (VERIFIED, `cef-154.5.0+154.0.34/src/osr_texture_import/`)

- Feature `accelerated_osr` turns on `ash`, `libc`, `wgpu` (30), `thiserror` and `tracing`, plus the Windows and macOS crates. It compiles `pub mod osr_texture_import` (`lib.rs:18-19`).
- `OnAcceleratedPaint` itself is in the generated bindings regardless of the feature (`ImplRenderHandler::on_accelerated_paint(&self, browser, type_, dirty_rects: Option<&[Rect]>, info: Option<&AcceleratedPaintInfo>)`).
- Public API:
  - `SharedTextureHandle::new(&AcceleratedPaintInfo) -> SharedTextureHandle`, an enum `DmaBuf | D3D11 | IOSurface | Unsupported`.
  - `.import_texture(self, &wgpu::Device) -> Result<wgpu::Texture, TextureImportError>`.
  - Trait `TextureImporter { new, import_to_wgpu, supports_hardware_acceleration }`.
  - The module doc mentions sub-features `accelerated_paint_dmabuf` and similar. They do not exist in `Cargo.toml`; there is only `accelerated_osr`.
- Linux flow (`dmabuf.rs`):
  1. Copy the fds, strides and offsets for `plane_count` planes. Take `modifier`, and take `width`/`height` from `extra.coded_size`.
  2. Check `supports_hardware_acceleration`: every fd passes `fcntl(F_GETFD)` and the device is the Vulkan backend (`device.as_hal::<Vulkan>()`).
  3. `vkCreateImage` with `tiling = DRM_FORMAT_MODIFIER_EXT`, `VkImageDrmFormatModifierExplicitCreateInfoEXT` holding all planes' layouts, and usage `SAMPLED|COLOR_ATTACHMENT`.
  4. `vkGetImageMemoryRequirements`, then `dup(fds[0])`, then `VkImportMemoryFdInfoKHR(DMA_BUF_EXT)`. Pick the first memory type in `memoryTypeBits` with no property filter, then `vkAllocateMemory` and `vkBindImageMemory`.
  5. `Device::texture_from_raw(image, hal desc{usage COPY_DST|RESOURCE}, drop_callback: None, TextureMemory::External)`.
  6. `device.create_texture_from_hal(..., wgpu desc{usage TEXTURE_BINDING}, TextureUses::RESOURCE)`.
  7. On any failure, `texture::create_fallback` creates a new, **empty** `TEXTURE_BINDING|COPY_DST` texture and logs a warning. Nothing is uploaded into it.

### 2.2 Defects

All are VERIFIED against the code. The consequences are INFERRED where noted.

1. **Per-frame leak.** `wgpu-hal` 30 documents that with `TextureMemory::External` "the memory must remain valid until `drop_callback` is called", and that it does not own it (`wgpu-hal-30.0.1/src/vulkan/device.rs:287-295`, `mod.rs:805-814`). The importer passes `drop_callback: None` and never frees `device_memory`. That memory owns the dup'd fd, so every frame leaks a VkDeviceMemory and pins a dmabuf. At 60 fps this exhausts memory quickly (INFERRED).
2. **No `VkExternalMemoryImageCreateInfo{DMA_BUF}` on the image.** The Vulkan spec requires the imported handle type to be declared at image creation (VUID-VkBindImageMemoryInfo-memory-02989; INFERRED from the spec).
3. **Memory type ignores `vkGetMemoryFdPropertiesKHR`, and there is no `VkMemoryDedicatedAllocateInfo`.** Many drivers require dedicated allocations for modifier images (INFERRED).
4. **Multi-plane handling is wrong.** It declares N plane layouts but binds only `fds[0]` at offset 0, without the disjoint flag.
5. **No layout, ownership or sync handling.** It reports the texture as already in `RESOURCE` state, while the VkImage is really in `UNDEFINED` layout and owned by a foreign queue. It never waits on the dmabuf's implicit fence and never ensures the GPU copy completes before the callback returns (the CEF contract, §1.1).
6. **Usage mismatch.** The wgpu texture is only `TEXTURE_BINDING`, so `copy_texture_to_texture` out of it is invalid. You must sample it in a render pass.
7. **Ignores `visible_rect`.** Content can be letterboxed inside `coded_size`.
8. **`tracing::info!` on every successful frame.**
9. **The example misuses it.** The cef-rs `examples/osr` (`webrender.rs:172-260`) stores a bind group of the imported texture in a thread-local and draws it later, outside the callback. It ignores `PET_POPUP` and calls `do_message_loop_work()` from a winit poll loop with `sleep(1000/17 ms)` (`main.rs:396-410`).

### 2.3 A better building block that already exists (VERIFIED)

`wgpu-hal` 30.0.1 adds `#[cfg(unix)] pub unsafe fn texture_from_dmabuf_fd(&self, fd: OwnedFd, desc: &TextureDescriptor, drm_modifier: u64, stride: u64, offset: u64) -> Result<vulkan::Texture, DeviceError>` (`src/vulkan/device.rs:514-605`). It:

- requires `wgt::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF`, which is reported when `external_memory_fd`, `external_memory_dma_buf` and `image_drm_format_modifier` are all present (`adapter.rs:1062-1065`), and which must be **requested** at device creation, because `DeviceShared.features` is the requested set;
- chains `VkExternalMemoryImageCreateInfo` and the explicit modifier;
- intersects `vkGetMemoryFdPropertiesKHR` bits;
- uses `VkMemoryDedicatedAllocateInfo`;
- returns `TextureMemory::Dedicated`, so wgpu frees the memory;
- handles fd ownership on failure;
- supports a single plane only.

ARGB from CEF is normally a single plane (INFERRED).

### 2.4 Can it be reused with GPUI's renderer?

- **gpui 0.2.2 (crates.io).** The Linux renderer is blade-graphics 0.7.1. blade can import `ExternalMemorySource::Dma(fd)`, but `create_texture` hard-codes `vk::ImageTiling::OPTIMAL` (`blade-graphics-0.7.1/src/vulkan/resource.rs:366-380`). It cannot express an explicit DRM modifier, and there is no wgpu device to hand to the cef crate. Reuse would mean porting the ash code into blade or patching blade (VERIFIED code; INFERRED conclusion).
- **Zed `main` GPUI (git).** Linux renders through `gpui_wgpu` on wgpu git rev `3b3506ea` (VERIFIED). Three obstacles:
  1. The crate graph would have two `wgpu`s, crates.io 30.0.1 for `cef` and git for GPUI. That needs a `[patch.crates-io] wgpu = { git = ..., rev = "3b3506ea..." }`, plus `wgpu-hal`/`wgpu-core`/`wgpu-types`/`naga` to match (INFERRED).
  2. `WgpuContext` requests only `DUAL_SOURCE_BLENDING` (`gpui_wgpu/src/wgpu_context.rs:426-459`), so `VULKAN_EXTERNAL_MEMORY_DMA_BUF` is not enabled (VERIFIED).
  3. The renderer ignores `PrimitiveBatch::Surfaces` on Linux: "Surfaces are macOS-only for video playback and are not implemented by the WGPU renderer" (`wgpu_renderer.rs:1606-1608`, VERIFIED). GPUI has no way to draw an app-owned GPU texture on Linux.
  - `WgpuContext` does expose `pub device: Arc<wgpu::Device>` and `pub queue` (VERIFIED), so a GPUI patch could share them.
- **Recommended pipeline for the dmabuf path (INFERRED design):**
  1. Patch GPUI (wgpu flavour) to request `VULKAN_EXTERNAL_MEMORY_DMA_BUF` when available. Force the Vulkan backend for this path, not GLES.
  2. Add a "texture surface" primitive: implement `PrimitiveBatch::Surfaces` on Linux, or add a new primitive that samples an app-owned `wgpu::Texture`.
  3. In `OnAcceleratedPaint`:
     1. Call `dup` and `texture_from_dmabuf_fd`.
     2. Barrier from `QUEUE_FAMILY_FOREIGN_EXT`, then draw `visible_rect` into a long-lived owned texture of the right size (`Bgra8Unorm`, `RENDER_ATTACHMENT|TEXTURE_BINDING`), keeping one per browser and one for `PET_POPUP`.
     3. Submit, then `device.poll(wait for that submission)` before returning.
     4. Notify GPUI to redraw.
  4. Draw the owned texture in the web-view element and the popup texture at `OnPopupSize`.
- **CPU path for any GPUI version (INFERRED):**
  - Copy `OnPaint`'s BGRA buffer, or just the damaged rect (`dirtyRects` has one rect), into a persistent texture with `queue.write_texture`.
  - GPUI's `img(RenderImage)` reallocates atlas space for every new image, so per-frame `RenderImage`s will churn. A small GPUI patch for a dynamic texture is still advisable.
  - Bandwidth at 2560×1440×4 B×60 fps is about 885 MB/s for the readback in viz and the same again for the upload.

---

## 3. Input and host integration in OSR

All calls take the browser's `CefBrowserHost`. Coordinates are view-relative **DIP integers** (`cef_mouse_event_t { int x, y; uint32_t modifiers; }`, VERIFIED `cef_types.h:2073-2088`).

### 3.1 Mouse and wheel (VERIFIED unless marked)

- `SendMouseClickEvent(event, MBT_LEFT|MBT_MIDDLE|MBT_RIGHT, mouseUp, clickCount)`. The enum has only those three buttons (`cef_types.h:2064-2066`). Back and forward (X1/X2) arrive with PR #4305, which depends on M158 Chromium. Until then, map GPUI's `MouseButton::Navigate(Back/Forward)` to `GoBack()`/`GoForward()`. Pages will not see buttons 3 and 4.
- `SendMouseMoveEvent(event, mouseLeave)`.
- `SendMouseWheelEvent(event, deltaX, deltaY)`.
  - On Linux the deltas are passed through unchanged as a `gfx::Vector2d` pixel offset (`browser_platform_delegate_native_aura.cc:231-253`).
  - Set `EVENTFLAG_PRECISION_SCROLLING_DELTA` for touchpad pixel deltas. cefclient converts one GTK wheel tick to 40 px.
  - OSR synthesizes wheel phases with a timer (`mouse_wheel_phase_handler_...`, not fling-capable).
  - For GPUI, map `ScrollDelta::Pixels` directly and `ScrollDelta::Lines` at ×40 to ×53 px (INFERRED).
- CEF routes mouse events itself: those over an open popup widget go to the popup with translated coordinates, and those over guest views go to the guest (`render_widget_host_view_osr.cc:1253-1299`).
- `SendCaptureLostEvent()` releases implicit capture. Call it when GPUI loses the pointer grab (INFERRED usage).
- Modifier flags: `EVENTFLAG_{CAPS_LOCK_ON, SHIFT_DOWN, CONTROL_DOWN, ALT_DOWN, LEFT/MIDDLE/RIGHT_MOUSE_BUTTON, COMMAND_DOWN, NUM_LOCK_ON, IS_KEY_PAD, IS_LEFT, IS_RIGHT, ALTGR_DOWN, IS_REPEAT, PRECISION_SCROLLING_DELTA, SCROLL_BY_PAGE}` (`cef_types.h:2186-2203`). Button-down state must be included in move events for drags to work (cefclient does).

### 3.2 Keyboard

- `cef_key_event_t { type; uint32_t modifiers; int windows_key_code; int native_key_code; int is_system_key; char16_t character; char16_t unmodified_character; int focus_on_editable_field; }` (VERIFIED, `cef_types.h:2372-2425`).
- Linux translation in CEF 154, `CefBrowserPlatformDelegateNativeLinux::TranslateUiKeyEvent` (VERIFIED, lines 244-290):
  - `dom_code = KeycodeConverter::NativeKeycodeToDomCode(native_key_code)`. On Linux the native code is the **XKB keycode** (evdev + 8), because `DOM_CODE(usb, evdev, xkb, ...)` maps to `{usb, xkb, code}` on Linux (Chromium `keycode_converter.cc:31-33`, VERIFIED). This drives `KeyboardEvent.code`.
  - `key_code = windows_key_code`. This drives `keyCode` and must be layout-independent: on a Hebrew or Cyrillic layout, the US-A key must still be `VK_A`, as cefclient's comment explains.
  - Builds with `SUPPORTS_OZONE_X11` (CEF's Linux builds have it) compute `keysym = XKeysymForWindowsKeyCode(key_code, shift)` and `character = GetUnicodeCharacterFromXKeySym(keysym)`. They also set `dom_key = XKeySymToDomKey(keysym, character)` for key-down and key-up.
  - So `KeyboardEvent.key` on non-US layouts and AltGr characters comes from the US table, not from the real layout. This is CEF #3273 ("Linux: Fix Unicode character input with OSR"). The maintainer closed it on 2026-10-10 as to be fixed by #4306 "Unified input API design", milestoned M158. #4306 adds `CefKeyboardInputEvent { dom_code, dom_key, ..., text[] }`, and "applications build the payload themselves" on Linux (VERIFIED, issue pages).
  - For `KEYEVENT_CHAR`, `NativeWebKeyboardEvent(ui_event, key_event.character)` uses the provided character (VERIFIED). Text from plain CHAR events is therefore right only for BMP characters.
- **Recommended mapping from GPUI on CEF 154 (INFERRED):**
  - `windows_key_code`: from the xkb keysym of the *unshifted base layout*, using a port of Chromium's `KeyboardCodeFromXKeysym`. Fall back to the hardware-keycode table for non-Latin layouts, as cefclient's `GdkEventToWindowsKeyCode` does (`browser_window_osr_gtk.cc:668-820`). Collapse L/R modifiers with `GetWindowsKeyCodeWithoutLocation`.
  - `native_key_code`: the XKB keycode. **GPUI does not expose it.** `Keystroke { modifiers, key, key_char }` has no keycode (Zed `main` `crates/gpui/src/platform/keystroke.rs:18-33`, VERIFIED). `gpui_linux::keystroke_from_xkb(state, modifiers, keycode: Keycode)` receives it and drops it (`gpui_linux/src/linux/platform.rs:1406-1414`, VERIFIED). A small GPUI patch should add `native_keycode` and `keysym` to `KeyDownEvent`/`KeyUpEvent`.
  - Sequence: `KEYEVENT_RAWKEYDOWN`, then for printable keys `KEYEVENT_CHAR` with `character`. With Ctrl held, send the control characters cefclient computes in `GetControlCharacter`. Then `KEYEVENT_KEYUP` (cefclient, `browser_window_osr_gtk.cc:1554-1610`). Set `EVENTFLAG_IS_KEY_PAD` for keypad keysyms and `is_system_key` when Alt is down, as cefclient does.
  - Committed text (GPUI `InputHandler::replace_text_in_range` with no marked text) is better sent with `ImeCommitText(text, CefRange::InvalidRange(), 0)`. That sidesteps the US-table `character` and handles emoji and other non-BMP text (INFERRED). `KeyboardEvent.key` stays wrong until M158.
- Shortcut routing: `CefKeyboardHandler::OnPreKeyEvent`/`OnKeyEvent` let the shell see keys before and after the page. `focus_on_editable_field` is filled for these callbacks (VERIFIED field doc; INFERRED usage for extension `commands` and browser shortcuts).

### 3.3 Touch (VERIFIED)

- `SendTouchEvent(cef_touch_event_t { int id; float x, y, radius_x, radius_y, rotation_angle, pressure; type CEF_TET_PRESSED|MOVED|RELEASED|CANCELLED; modifiers; pointer_type })`. "A maximum of 16 concurrent touches will be tracked."
- OSR runs a gesture provider (pinch is gated by `pinch_zoom_enabled_`).
- Touch-selection handles are client-drawn through `GetTouchHandleSize`/`OnTouchHandleStateChanged`.
- Zed `main` GPUI has `TouchEvent { id, phase, position, predicted_position, ... }` (VERIFIED), so mapping is direct (INFERRED).

### 3.4 IME (VERIFIED API; mapping INFERRED)

- Calls:
  - `ImeSetComposition(text, underlines, replacement_range, selection_range)`.
  - `ImeCommitText(text, replacement_range, relative_cursor_pos)`.
  - `ImeFinishComposingText(keep_selection)`.
  - `ImeCancelComposition()`.
- The header says replacement ranges and cursor position are "only used on OS X". The 154 code forwards them to `RenderWidgetHostImpl::ImeSetComposition/ImeCommitText` on every platform (`render_widget_host_view_osr.cc:841-881`), so the doc looks stale (VERIFIED code, INFERRED effect).
- Callbacks:
  - `CefRenderHandler::OnImeCompositionRangeChanged(browser, selected_range, character_bounds)` gives per-character bounds in view coordinates, for placing the candidate window. Chromium drives it internally through `RequestImeCompositionUpdate` while a composition is active (`render_widget_host_view_osr.cc:1900-1925`). The client has no API to request it.
  - `OnTextSelectionChanged(browser, selected_text, selected_range)`.
  - `OnVirtualKeyboardRequested(browser, input_mode)`. It fires from `OnUpdateTextInputStateCalled` **only when `state->show_ime_if_needed`**. The mode is `CEF_TEXT_INPUT_MODE_NONE` when focus leaves editable content (`render_widget_host_view_osr.cc:1509-1532`).
- GPUI mapping (INFERRED):

  | GPUI `InputHandler` | CEF |
  |---|---|
  | `replace_and_mark_text_in_range` | `ImeSetComposition`, with selection from the given range |
  | `replace_text_in_range` | `ImeCommitText` |
  | `unmark_text` | `ImeFinishComposingText(false)` |
  | `bounds_for_range` | cached `character_bounds` |
  | `selected_text_range` / `text_for_range` | only what `OnTextSelectionChanged` gave |

  CEF exposes no surrounding text. Wayland `text-input-v3` surrounding-text support will be partial, and the IME will not know which field type is focused beyond `input_mode`.
- Problem: `OnVirtualKeyboardRequested` is filtered on `show_ime_if_needed`. Programmatic `element.focus()` may not tell the host to enable IME (INFERRED). Plan to enable the GPUI input handler whenever the web view has focus and let CEF ignore IME calls on non-editable targets.

### 3.5 Cursor (VERIFIED)

- `CefDisplayHandler::OnCursorChange(browser, cef_cursor_handle_t, cef_cursor_type_t type, const CefCursorInfo& custom_cursor_info)`. It covers about 45 types, from `CT_POINTER` to `CT_CUSTOM` and `CT_DND_*`. `custom_cursor_info` carries the bitmap, size, hotspot and scale for CSS `cursor: url()`.
- In OSR the display is a "fake Display" built from screen info (`cursor_util_aura.cc`).
- GPUI's `CursorStyle` has 21 variants (Zed `main` `platform.rs:2942-3023`, VERIFIED). There is no wait, progress, help, zoom, move, all-scroll, cell or no-drop, and no custom bitmap cursor. Expect a GPUI patch or lossy mapping (INFERRED).

### 3.6 Drag and drop (VERIFIED API; flow INFERRED)

- **Web content as drag source:** `CefRenderHandler::StartDragging(browser, drag_data, allowed_ops, x, y)`.
  - Return true, run the OS drag, then call `DragSourceEndedAt(x, y, op)` and `DragSourceSystemDragEnded()`. `UpdateDragCursor(op)` reports the operation.
  - If the page is both source and target, call all `DragTarget*` before `DragSource*`.
- **Drop target:**
  - Call `DragTargetDragEnter(drag_data, event, allowed_ops)`, then `DragTargetDragOver`, then `DragTargetDragLeave` or `DragTargetDrop`.
  - Build `drag_data` with `CefDragData::Create()` plus `AddFile`, `SetFragmentText`, `SetLinkURL` and similar.
  - "|drag_data| should not contain file contents" (use `ResetFileContents`).
- Minimum viable: return true from `StartDragging` and run an *in-window* drag loop that feeds GPUI mouse moves into `DragTargetDragOver` on the same browser. Without it, HTML5 drag and drop inside pages (sortable lists, upload drop zones) breaks.
- GPUI on Linux accepts external *file* drops (`FileDropEvent` with paths). I did not find an OS-level drag-source API, and text or URL drops from other apps are not covered. Verify in the GPUI research (INFERRED).

### 3.7 Popups: select, date and colour (VERIFIED for select)

- `<select>` dropdowns are a separate popup widget.
  - `OnPopupShow(browser, show)` and `OnPopupSize(browser, rect)` give the rect in view DIP coordinates.
  - Pixels arrive via `OnPaint`/`OnAcceleratedPaint` with `type == PET_POPUP`.
  - The host composites the popup over the view. CEF handles input routing (§3.1).
- cefclient clamps the popup inside the view (`osr_renderer.cc` `GetPopupRectInWebView`). The master sample invalidates the cached popup on show and resize.
- Blink "page popups" such as date and time pickers use the same popup-widget path (INFERRED).
- Scrolling inside select popups needs a correct `GetScreenPoint` (header note on `SendMouseWheelEvent`).

### 3.8 Tooltips (VERIFIED)

- `CefDisplayHandler::OnTooltip(browser, text)`: "When window rendering is disabled the application is responsible for drawing tooltips and the return value is ignored."

### 3.9 Screen info, DPI and coordinates (VERIFIED)

- `GetViewRect(browser, rect)` is required and returns DIP.
- `GetRootScreenRect` is optional and returns screen DIP.
- `GetScreenPoint(browser, viewX, viewY, screenX, screenY)`: "Windows/Linux should provide screen device (pixel) coordinates".
- `GetScreenInfo(browser, CefScreenInfo{device_scale_factor, depth, depth_per_component, is_monochrome, rect, available_rect})`. `rect` and `available_rect` "are used to determine the available surface for rendering popup views".
- Pixel size is `ScaleToCeiledSize(view DIP, device_scale_factor)`. Fractional scale factors are fine.
- Call `WasResized()` on size change. CEF holds the resize until a frame of the expected size arrives (`hold_resize_`).
- Call `NotifyScreenInfoChanged()` on scale or monitor change.
- Wayland gives clients no global coordinates (INFERRED). Report the view at a constant origin, with the output size as `rect`/`available_rect`, and accept that `window.screenX/Y` are synthetic. The current Linux plan already accepts this for `chrome.windows`.

### 3.10 Focus (VERIFIED API)

- `SetFocus(bool)`.
- `CefFocusHandler::OnTakeFocus(next)` fires when Tab leaves the page, so the shell can move focus to the GPUI chrome. `OnSetFocus` and `OnGotFocus` are also available.
- With OSR, `CefBrowserHost::SetFocus` must track GPUI's focus of the web-view element and of the window (INFERRED).

---

## 4. Missing or worse in Linux OSR

| Area | OSR / Alloy behaviour on Linux | Host work | Evidence |
|---|---|---|---|
| **Extensions: tabs and windows** | `tabs.query` and `windows.*` do not see OSR tabs. CEF patches only `GetTabById` and parts of `tabs.update` for Alloy. | Patch Chromium, or shim (Executive summary §2) | VERIFIED: `tabs_api.cc:1852-1861`; `chrome_browser_extensions.patch`; `chrome_extension_util.cc`; #3681 |
| **Extensions: action UI, chrome://extensions** | No toolbar or popups. `chrome://extensions` and other non-allowlisted WebUI are blocked in Alloy. Maintainer: "Extension functionality that requires Chrome UI (like toolbars, etc) will not be supported with Alloy style browsers". | Draw the action toolbar and host popups (`chrome-extension://…/popup.html`) in OSR browsers yourself. Use our own Extensions page, as today. | VERIFIED: `alloy_browser_host_impl.cc:62-101, 563-569`; #3859; cef-announce 2024-05-03 (older, partly superseded) |
| **Extension context menu items** | The Alloy menu model is CEF's own (`CefMenuManager::CreateDefaultModel`). `chrome.contextMenus` items are not added. | Implement `RunContextMenu`, and find a way to read extension items. CEF has no API for that. | VERIFIED model code; INFERRED item absence |
| **Accessibility** | `SetAccessibilityState(STATE_ENABLED)` gives `kAXModeWebContentsOnly`. The tree and location updates go to `CefAccessibilityHandler::OnAccessibilityTreeChange/OnAccessibilityLocationChange` as `CefValue`. No platform (AT-SPI) objects are created. | Build AccessKit or AT-SPI nodes from the CefValue tree. cefclient has helpers for Windows and macOS, none for Linux. | VERIFIED: `browser_platform_delegate.cc:510-533`; `cef_browser.h:960-987`; master `tests/shared/browser/osr_accessibility_*` |
| **Fullscreen** | Alloy: "the client is responsible for triggering the fullscreen transition". `OnFullscreenModeChange(fullscreen)`, `ExitFullscreen(will_cause_resize)`. | Make the GPUI window fullscreen, then `WasResized()`. | VERIFIED: `cef_display_handler.h:75-87` |
| **Video performance** | Every frame goes through the capturer copy, then a client copy (GPU) or readback plus upload (CPU). No overlays or direct scanout. Capped by `windowless_frame_rate`. | Set the frame rate to the display rate. Prefer the dmabuf path once usable. | VERIFIED pipeline; INFERRED cost |
| **WebGL / WebGPU** | Work in OSR when GPU compositing is on. Same GPU process. The cefclient sample disables the GPU in CPU OSR mode for speed and notes "This will also disable WebGL". | Do not pass `--disable-gpu`. | VERIFIED: `client_app_browser.cc:49-60` |
| **Media capture prompts** | Alloy default is deny unless `CefPermissionHandler::OnRequestMediaAccessPermission` handles it (`default_disallow=true`). Other permission prompts default to **IGNORE** in Alloy ("The default UI prompt is not supported because there is no Chrome Browser object"). Screen-share grant picks the full desktop or the requested `DesktopMediaID`. | Implement `OnRequestMediaAccessPermission` and `OnShowPermissionPrompt` with GPUI UI. Wayland screen capture goes through Chromium's PipeWire/portal picker (INFERRED). | VERIFIED: `alloy_browser_host_impl.cc:1118-1123`; `media_access_query.cc:403-405, 206-216`; `permission_prompt.cc:315-323` |
| **File chooser** | `CefDialogHandler::OnFileDialog(browser, mode, title, default_file_path, accept_filters, accept_extensions, accept_descriptions, callback)`. The default is Chromium's `SelectFileDialogLinuxPortal` (xdg-desktop-portal; `CreateSelectFileDialog` returns it unconditionally with `USE_DBUS`). OSR passes `browser->GetWindowHandle()` (an X11 id) as owner. Under `multi_threaded_message_loop` the default is cancelled: "Default dialog implementation is not available". Since 2022: "OSR on Linux GTK dialogs won't be modal to application windows". | Implement `OnFileDialog` with ashpd `OpenFileRequest`/`SaveFileRequest`, parented to the GPUI window via xdg-foreign. | VERIFIED: `file_dialog_manager.cc:402-436`; Chromium `shell_dialog_linux.cc:17-29`; #3293 |
| **Print / print preview** | "Not supported with windowless rendering". Print preview is also off by default for Alloy. `Print()` / `window.print()` uses the system path: no `CefPrintHandler` means Chromium's dialog selection "including the show_system_dialog based XDG portal branch". `PrintToPDF` works. | Use the portal print dialog, or implement `CefPrintHandler`. Build our own preview from `PrintToPDF` if wanted. | VERIFIED: `browser_platform_delegate.cc:535-565`; `printing/print_dialog_linux.cc:177-266` |
| **Context menus** | The default OSR menu is a Views menu that needs an X11 parent ("Window handle is required for default OSR context menu."). On Wayland there is none. | `CefContextMenuHandler::OnBeforeContextMenu` plus `RunContextMenu(browser, frame, params, model, callback)`; return true, then `callback->Continue(command_id, event_flags)` or `Cancel()`. Params carry link, media, selection, edit flags and `dictionary_suggestions`. | VERIFIED: `menu_runner_views_aura.cc:30-36`; `menu_manager.cc:170-200` |
| **JS dialogs** | Default Chrome tab-modal dialogs need a parent handle in OSR: "Default dialog implementation requires a parent window handle; canceling the JS dialog". | `CefJSDialogHandler::OnJSDialog`, `OnBeforeUnloadDialog`, `OnResetDialogState`, `OnDialogClosed`. | VERIFIED: `javascript_dialog_manager.cc:328-337`; patch `chrome_browser_dialogs_jsmodal` (#3818, #3921 "Fix crash with OSR browsers on Linux") |
| **DevTools** | Always a separate **Chrome-style windowed** browser, created through `ChromeBrowserDelegate::CreateDevToolsBrowser`. A windowless `windowInfo` gets "Windowless rendering is not supported for this DevTools window". A parent handle works only if the opener has an external parent. Under Ozone X11 or Wayland it is a separate toplevel. | Accept a separate window (`ShowDevTools`), or use `remote_debugging_port` and load the DevTools frontend in an OSR tab (INFERRED). `ExecuteDevToolsMethod` and `AddDevToolsMessageObserver` work for protocol access. | VERIFIED: `chrome_browser_delegate.cc:53-200`; `devtools_window_runner.cc` |
| **PaymentRequest** | No CEF code found. Its UI is Chrome Views dialogs, so it is likely unavailable in Alloy. | Treat as unsupported. Test. | INFERRED (unverified) |
| **Picture-in-picture** | Video PiP: `IsPictureInPictureSupported()` returns `!IsWindowless()`, "Not currently supported with OSR". Document PiP (`NEW_PICTURE_IN_PICTURE`) gets default Browser creation only for Chrome style. In Alloy it goes through `OnBeforePopup` (INFERRED). | Treat video PiP as unsupported. Handle document PiP as a windowless popup. | VERIFIED: `alloy_browser_host_impl.cc:482-485`; `browser_info_manager.cc:152-155` |
| **window.open popups** | `CefLifeSpanHandler::OnBeforePopup(..., target_disposition, user_gesture, popupFeatures, windowInfo, client, settings, extra_info, no_javascript_access)`. If it returns false *without a parent handle*, a "Native-hosted Alloy style parent: Creates a native popup window" results: an X11 window, impossible on Wayland. | Always `windowInfo.SetAsWindowless(0)` with `shared_texture_enabled` as for tabs, and host the popup in a GPUI tab or window, as cefclient's `GetPopupConfig` does. Return true (cancel) only for dispositions you open as a plain new tab without an opener. | VERIFIED: `cef_life_span_handler.h:56-98`; `browser_window_osr_gtk.cc:1015-1032` |
| **Extension-initiated windows** | `chrome.windows.create` / `tabs.create` with no Alloy target go through Chrome's own `Browser` creation, so they open as Chrome-style windowed browsers outside GPUI. | Must be intercepted. Open question. | INFERRED |
| **Find in page** | `Find`/`StopFinding` plus `CefFindHandler::OnFindResult` work. There is no Chrome find bar in Alloy. | GPUI find bar. | VERIFIED API (`cef_browser.h:560-569`) |
| **Clipboard** | Chromium's X11 clipboard owns `CLIPBOARD` with a hidden "Chromium Clipboard Window". It serves `SelectionRequest` only when its X11 GLib source is dispatched (§5). The Wayland clipboard calls `wl_data_device_set_selection(..., serial)`, and a CEF connection that never receives input has no serial. A LiquidBounce PR reports OSR clipboard not reaching other apps on KDE Wayland; the PR was rejected and is unverified. | Test early. With external pump, iterate the GLib default context. With ozone=wayland expect failures and consider a host-side clipboard bridge. | VERIFIED: Chromium `x11_clipboard_helper.cc:159-174`, `wayland_data_device.cc:71-74`; INFERRED consequences |
| **Autofill / datalist UI** | Chrome-runtime features ("datalist/autofill ... standard Chrome UI toolbars and dialogs") per `docs/architecture.md`. Popup Views anchor to Browser windows. | Likely absent or partial in OSR; test. Vsesvit does not store passwords anyway. | INFERRED |

---

## 5. Message loop integration on Linux

### 5.1 The three CEF modes (VERIFIED)

- **`CefRunMessageLoop()`** blocks the main thread in CEF's loop. That is unusable here, because GPUI's `Application::run` owns the main thread.
- **`external_message_pump = 1` + `CefDoMessageLoopWork()`.**
  - CEF calls `CefBrowserProcessHandler::OnScheduleMessagePumpWork(delay_ms)` "from any thread". `delay_ms <= 0` means "reasonably soon"; a positive value means "after the delay, and cancel any pending call".
  - `CefDoMessageLoopWork()` must run on the thread that called `CefInitialize` and "will not block" (`cef_app.h:105-121`). Internally it is `base::RunLoop().RunUntilIdle()` (`context.cc:270-285`).
  - The pump is `MessagePumpExternal : base::MessagePumpForUI`, with `max_time_slice_ = 0.01` s. It overrides `Run`, `ScheduleWork` (calls `OnScheduleMessagePumpWork(0)`) and `ScheduleDelayedWork` (`browser_message_loop.cc:21-112`).
  - cefclient's reference scheduler caps waits at `kMaxTimerDelay = 1000/30` ms and handles reentrancy (`tests/shared/browser/main_message_loop_external_pump.cc:19-104`). On Linux it iterates `g_main_context_iteration(g_main_context_default())` and wakes it through a pipe (`main_message_loop_external_pump_linux.cc`).
- **`multi_threaded_message_loop = 1`** is "only supported on Windows and Linux".
  - `context.cc:366-371` rejects it on other platforms.
  - `CefUIThread` runs the browser UI thread separately. Before initializing it calls `ui::SetMultiThreadedMessageLoopWayland()` and `ui::SetMultiThreadedMessageLoopX11()`, which skip the `LinuxUiDelegate`, so there is no GtkUi: "We can't use GtkUi in combination with multi-threaded-message-loop because Chromium's GTK implementation doesn't use GDK threads. Light/dark theme changes will still be detected via DarkModeManagerLinux." (`ui_thread.cc:70-85`; patch `linux_gtk_theme_3610.patch`).
  - Default file dialogs are refused in this mode (`file_dialog_manager.cc:402-411`).
  - `CefInitialize`/`CefShutdown` still run on the main thread (`general_usage.md`, Message Loop Integration).
  - cefclient's OSR on Linux was tested with `--multi-threaded-message-loop` in Oct 2026 (c7e1a9e message).

### 5.2 GLib facts that matter (VERIFIED)

- Chromium Linux desktop builds set `use_glib = is_linux && !is_castos` (`build/config/ui.gni:34`). `MessagePumpForUI` is then `MessagePumpGlib` (`base/message_loop/message_pump_for_ui.h`).
- `MessagePumpGlib`'s constructor uses `g_main_context_default()` on the main thread. Otherwise it creates and pushes a new thread-default context. It attaches its observer and work sources to that context. `FdWatchController::Attach` also attaches fd watches to `pump->context_` (`message_pump_glib.cc:456-494, 586`).
- Ozone's event sources attach to the thread-default or default context:
  - X11: `XSourcePrepare/Check/Dispatch` GSource (`ui/events/platform/x11/x11_event_watcher_glib.cc:22-111`).
  - Wayland: `wayland_event_watcher_glib.cc:102-106`.
- Consequence with `external_message_pump` on the main thread (INFERRED from the above):
  - Display-server events (clipboard `SelectionRequest`, any CEF-owned windows such as DevTools and menus, screen changes) are processed **only if the host iterates the default GLib context**. CEF's `MessagePumpExternal::Run` never calls `g_main_context_iteration`.
  - GPUI does not use GLib (VERIFIED: no glib in gpui 0.2.2 or `gpui_linux` dependencies; calloop 0.14).
- Known bug (VERIFIED report, tauri-apps/tauri#16189, 2026-10-01, CEF 152):
  - The `MessagePumpGlib` work source stays on the default context with `state_ == nullptr`, so `HandlePrepare` always returns timeout 0.
  - Any host doing blocking `g_main_context_iteration(TRUE)` spins at 100% CPU (382k `ppoll` calls per 20 s, drops to 2.4k with the patch).
  - The proposed workaround finds and `g_source_destroy`s that source after `cef::initialize`, matching a libcef `prepare` pointer, `G_PRIORITY_DEFAULT_IDLE` and a FIFO poll fd. A cleaner fix belongs in CEF. Not merged.

### 5.3 Options for GPUI

**Option A, recommended: `multi_threaded_message_loop = 1`.** All INFERRED design, built on the VERIFIED facts above.

- CEF's UI thread owns its own GLib context, so Ozone events and the clipboard run there. GPUI's calloop thread is untouched, and the CEF loop never blocks GPUI frames.
- Every `CefClient` callback, including `OnPaint`/`OnAcceleratedPaint`, arrives on the CEF UI thread.
  - For the GPU path, do the dmabuf import and copy on that thread. `wgpu::Device` and `Queue` are `Send + Sync`, and this requires the GPUI patch that shares them.
  - For the CPU path, copy into a staging buffer or texture there.
  - Then wake GPUI through a `Send` channel polled by a foreground task.
- Calls into CEF from GPUI are mostly thread-safe and post to TID_UI internally. Many `CefBrowserHost` methods post themselves (for example `ImeSetComposition`, `alloy_browser_host_impl.cc:631-640`). Prefer `CefPostTask(TID_UI, ...)` for sequences such as key-down followed by char.
- Cost: no GtkUi, so default file dialogs are cancelled. We implement `OnFileDialog` with ashpd regardless, and GPUI already depends on it. No GTK theme or font integration in Chromium beyond `DarkModeManagerLinux` and fontconfig.

**Option B: `external_message_pump = 1` on GPUI's main thread.** All INFERRED.

- Call `CefInitialize` inside GPUI's `run` closure, on the main thread.
- `OnScheduleMessagePumpWork(delay)` sends `delay` over a `Send` channel to a GPUI foreground task. That task runs `CefDoMessageLoopWork()` now or arms a GPUI timer, capped at 33 ms like cefclient.
- Also add a calloop event source that drives the default `GMainContext`: `g_main_context_prepare`, then `query` (fds and timeout) registered as calloop `Generic` fds and a timer, then `check` and `dispatch`.
- Apply the #16189 workaround, or accept that the idle source forces zero timeouts.
- Benefit: CEF callbacks arrive on the GPUI thread, which makes OSR texture handoff simple. Cost: GLib bridging code and fragile internals.

**Option C: `CefRunMessageLoop`.** Not compatible with GPUI owning the loop.

Process startup (VERIFIED general CEF rule; INFERRED placement):

- Call `cef::execute_process()` first in `main`, before GPUI initialises, so that renderer, GPU and utility subprocess launches of the same binary exit early.
- Alternatively set `browser_subprocess_path` to a small helper binary.

---

## 6. Things to test first (open items that block design)

1. On this RTX 5080 with CEF 154: confirm `shared_texture_enabled` produces empty or failed frames (#4237), and that the CPU `OnPaint` path renders at 60 fps with acceptable CPU at the target resolution.
2. With a CEF 156 build (cefclient master or an `update-bindings` build): measure tear frequency from #4313. Check whether PR #4314 makes NVIDIA clean.
3. Choose `--ozone-platform=x11` (XWayland) or `wayland` for the OSR process. Test clipboard both ways (copy from page to KWrite, and paste in), DevTools window behaviour, and `<select>` popups.
4. Test GLib integration choice (MTML vs external pump + GLib bridge): idle CPU, clipboard, and IME latency.
5. Check how much of `chrome.tabs`, `windows`, `action`, `contextMenus`, `commands`, `scripting.executeScript({tabId})` and `webNavigation` works against OSR tabs with real Web Store extensions: uBlock Origin Lite, Bitwarden, Dark Reader, a tab manager.
6. Run non-US layout and AltGr typing tests with the recommended key mapping, plus `ImeCommitText` text injection.

---

## Sources

### CEF source (GitHub, branch 8037 = CEF 154.0.34, and master)

- Branch 8037 tree: https://github.com/chromiumembedded/cef/tree/8037
- Rendering:
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/osr/video_consumer_osr.cc
  - https://github.com/chromiumembedded/cef/blob/master/libcef/browser/osr/video_consumer_osr.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/osr/render_widget_host_view_osr.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/osr/browser_platform_delegate_osr.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/osr/osr_util.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/osr/host_display_client_osr.cc
- Native delegate:
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/native/browser_platform_delegate_native_linux.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/native/browser_platform_delegate_native_aura.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/native/menu_runner_views_aura.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/native/cursor_util_aura.cc
- Browser and dialogs:
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/browser_platform_delegate.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/browser_platform_delegate_create.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/file_dialog_manager.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/javascript_dialog_manager.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/permission_prompt.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/media_access_query.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/menu_manager.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/printing/print_dialog_linux.cc
- Alloy and Chrome style:
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/alloy/alloy_browser_host_impl.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/alloy/browser_platform_delegate_alloy.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/chrome_browser_delegate.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/extensions/chrome_extension_util.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/views/chrome_child_window.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/devtools/devtools_window_runner.cc
- Message loop:
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/browser_message_loop.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/ui_thread.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/context.cc
- Patches:
  - https://github.com/chromiumembedded/cef/blob/8037/patch/patch.cfg
  - https://github.com/chromiumembedded/cef/blob/8037/patch/patches/chrome_browser_extensions.patch
  - https://github.com/chromiumembedded/cef/blob/8037/patch/patches/linux_gtk_theme_3610.patch
- Public headers:
  - https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types_linux.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types_runtime.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/cef_render_handler.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/cef_browser.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/cef_display_handler.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/cef_life_span_handler.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/cef_dialog_handler.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/cef_app.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/cef_browser_process_handler.h
- cefclient samples:
  - https://github.com/chromiumembedded/cef/blob/8037/tests/cefclient/browser/browser_window_osr_gtk.cc
  - https://github.com/chromiumembedded/cef/blob/8037/tests/shared/browser/client_app_browser.cc
  - https://github.com/chromiumembedded/cef/blob/master/tests/shared/browser/client_app_browser.cc
  - https://github.com/chromiumembedded/cef/blob/8037/tests/shared/browser/main_message_loop_external_pump_linux.cc
  - https://github.com/chromiumembedded/cef/blob/8037/tests/shared/browser/main_message_loop_external_pump.cc
  - https://github.com/chromiumembedded/cef/blob/master/tests/shared/browser/osr_renderer_gl_linux.cc
- Docs:
  - https://github.com/chromiumembedded/cef/blob/8037/docs/general_usage.md (Message Loop Integration, Off-Screen Rendering; the OSR section is stale about accelerated compositing)
  - https://github.com/chromiumembedded/cef/blob/8037/docs/architecture.md

### CEF commits, issues, announcements

- Commit 77c1e82, "osr: Implement shared texture support (fixes #1006, fixes #2575)", 2024-04-27: https://github.com/chromiumembedded/cef/commit/77c1e82898a7f46164e0b997431318bcf9743592
- Linux OSR shared textures and NVIDIA:
  - #2575 Viz implementation for OSR: https://github.com/chromiumembedded/cef/issues/2575
  - #3953, defaults `use-angle=gl-egl` and `ozone-platform=x11` with shared textures: https://github.com/chromiumembedded/cef/issues/3953
  - #4237, prefer native-handle SharedImages (NVIDIA): https://github.com/chromiumembedded/cef/issues/4237
  - PR #4238: https://github.com/chromiumembedded/cef/pull/4238
  - Commit 3adefb8: https://github.com/chromiumembedded/cef/commit/3adefb829c
  - #4313, frames read before drawn on NVIDIA (open): https://github.com/chromiumembedded/cef/issues/4313
  - #3687, cefclient Linux `OnAcceleratedPaint`: https://github.com/chromiumembedded/cef/issues/3687
  - Commits c7e1a9e, 7219f4e, e35fdcb: https://github.com/chromiumembedded/cef/commit/c7e1a9e5c6, https://github.com/chromiumembedded/cef/commit/7219f4ea17, https://github.com/chromiumembedded/cef/commit/e35fdcb895
- Wayland, runtime style and extensions:
  - #2804, Wayland embedding (open): https://github.com/chromiumembedded/cef/issues/2804
  - #3293, Chrome-runtime OSR, which became "OSR = Chrome bootstrap + Alloy style": https://github.com/chromiumembedded/cef/issues/3293
  - #3681, Alloy-style windows and browsers: https://github.com/chromiumembedded/cef/issues/3681
  - #3859, `chrome://extensions` in Alloy (not planned): https://github.com/chromiumembedded/cef/issues/3859
  - cef-announce, 2024-05-03, "Alloy style is supported in M125..." (older; extension statements partly superseded by the minimal tabs patch): https://groups.google.com/g/cef-announce/c/s1WaovAopFo/m/LV5eiNX1BgAJ
- Input:
  - #3273, Linux OSR Unicode input: https://github.com/chromiumembedded/cef/issues/3273
  - #4306, unified input API, M158: https://github.com/chromiumembedded/cef/issues/4306
  - PR #4305, mouse back/forward and gestures: https://github.com/chromiumembedded/cef/pull/4305
- CEF builds index: https://cef-builds.spotifycdn.com/index.json

### Chromium (tag 154.0.8037.98)

- `base/message_loop/message_pump_for_ui.h`: https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/base/message_loop/message_pump_for_ui.h
- `base/message_loop/message_pump_glib.cc`: https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/base/message_loop/message_pump_glib.cc
- `build/config/ui.gni` (`use_glib`): https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/build/config/ui.gni
- `ui/events/platform/x11/x11_event_watcher_glib.cc`: https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/events/platform/x11/x11_event_watcher_glib.cc
- `ui/events/platform/wayland/wayland_event_watcher_glib.cc`: https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/events/platform/wayland/wayland_event_watcher_glib.cc
- `ui/events/keycodes/dom/keycode_converter.cc`: https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/events/keycodes/dom/keycode_converter.cc
- `ui/shell_dialogs/shell_dialog_linux.cc`: https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/shell_dialogs/shell_dialog_linux.cc
- `chrome/browser/extensions/api/tabs/tabs_api.cc`: https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/chrome/browser/extensions/api/tabs/tabs_api.cc
- `ui/base/x/x11_clipboard_helper.cc`: https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/base/x/x11_clipboard_helper.cc
- `ui/ozone/platform/wayland/host/wayland_data_device.cc`: https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/ozone/platform/wayland/host/wayland_data_device.cc

### Crates and Rust sources

- `cef` 154.5.0+154.0.34 (local): `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cef-154.5.0+154.0.34/src/osr_texture_import/{mod,common,dmabuf,shared_texture_handle}.rs`, `Cargo.toml`, `src/lib.rs`, `src/bindings/x86_64_unknown_linux_gnu.rs`. Repo: https://github.com/tauri-apps/cef-rs
- `cef-dll-sys` 154.5.0+154.0.34 (local): `build.rs`, `src/bindings/x86_64_unknown_linux_gnu.rs`
- cef-rs `examples/osr` at 163744c: https://github.com/tauri-apps/cef-rs/tree/163744c8286d5f593d89202c7807751a7898db28/examples/osr
- `wgpu-hal` 30.0.1, `src/vulkan/{device,adapter,mod}.rs`: https://docs.rs/wgpu-hal/30.0.1 (source from https://static.crates.io/crates/wgpu-hal/wgpu-hal-30.0.1.crate)
- `gpui` 0.2.2 `Cargo.toml`: https://static.crates.io/crates/gpui/gpui-0.2.2.crate
- `blade-graphics` 0.7.1, `src/vulkan/resource.rs`: https://static.crates.io/crates/blade-graphics/blade-graphics-0.7.1.crate
- Zed `main`:
  - https://github.com/zed-industries/zed/blob/main/crates/gpui_wgpu/src/wgpu_renderer.rs
  - https://github.com/zed-industries/zed/blob/main/crates/gpui_wgpu/src/wgpu_context.rs
  - https://github.com/zed-industries/zed/blob/main/crates/gpui_wgpu/Cargo.toml
  - https://github.com/zed-industries/zed/blob/main/crates/gpui/src/platform/keystroke.rs
  - https://github.com/zed-industries/zed/blob/main/crates/gpui/src/platform.rs
  - https://github.com/zed-industries/zed/blob/main/crates/gpui/src/interactive.rs
  - https://github.com/zed-industries/zed/blob/main/crates/gpui_linux/src/linux/platform.rs
  - https://github.com/zed-industries/zed/blob/main/crates/gpui_linux/Cargo.toml

### Third-party reports (lower trust, cited where used)

- tauri-apps/tauri#16189, idle 100% CPU from the `MessagePumpGlib` work source under `external_message_pump`: https://github.com/tauri-apps/tauri/issues/16189
- CCBlueX/LiquidBounce PR #9265, CEF OSR clipboard on KDE Wayland (rejected, analysis unverified): https://github.com/CCBlueX/LiquidBounce/pull/9265
