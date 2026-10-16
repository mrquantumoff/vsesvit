# CEF 154 windowing on Linux for a GPUI browser shell

Research date: 2026-10-16. Scope: how a Chromium page from CEF 154 can sit inside (or next to) a GPUI window on Linux, on X11 and on Wayland, and what each approach costs in Chrome-extension compatibility.

Versions examined:

- CEF `154.0.34+g14c5a08+chromium-154.0.8037.98` (branch `8037`, head `14c5a08`, 2026-10-04). This is the libcef that the `cef` crate `154.5.0+154.0.34` binds (`CEF_VERSION` in `cef-dll-sys-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs:38`). CEF `master` at `a875f40` (2026-10-10) and beta branch `8078` (CEF 156) were checked for changes after 154.
- Chromium `154.0.8037.98` sources, read from `chromium.googlesource.com` at that tag.
- GPUI on crates.io, `0.2.2` (published 2025-10-22), and Zed `main` at `d678e7d` (2026-10-16). On `main`, GPUI is split into `gpui`, `gpui_linux`, `gpui_platform`, `gpui_wgpu` and other crates.
- Local copies used for reading: the CEF trees in `/tmp/vsesvit-research/cef/cef-8037` and `/tmp/vsesvit-research/cef/cef-master`; Chromium files, the `gpui-0.2.2` crate and the Zed `main` files in `/tmp/vsesvit-research/cef-windowing/`; and the `cef` / `cef-dll-sys` crates under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.

Each claim carries a label:

- **VERIFIED**: I read it in source code, a header, an issue thread or a commit.
- **INFERRED**: my reasoning from verified facts, not yet tested in practice.

---

## Executive summary

1. **Stock CEF 154 hosts web content on Linux in exactly three ways.** **VERIFIED.**
   - **(a) Views:** `CefWindow` + `CefBrowserView`. Works on X11 and Wayland, Chrome or Alloy style.
   - **(b) Native child:** `CefWindowInfo::SetAsChild(XID)`. X11 only, Chrome or Alloy style.
   - **(c) Windowless (OSR):** `SetAsWindowless`. Works on X11 and Wayland, but always Alloy style.

   Sources: the PR #4233 design doc table, `cefclient_gtk.cc` on master, and maintainer comments on #2804 and #3687.

2. **Maximal Chrome-extension compatibility requires Chrome style, so OSR is out for the main page view.**
   - OSR always uses Alloy style. **VERIFIED:** `SetAsWindowless` forces `runtime_style = ALLOY` in `cef_linux.h` and in the Rust crate's `window_info.rs`; `IsChromeStyle()` returns false for windowless browsers.
   - The CEF maintainer, 2026-08-26: "Alloy style only supports extension APIs that are required by the embedded components like the PDF Viewer." ([#4255](https://github.com/chromiumembedded/cef/issues/4255#issuecomment-5429597039))
   - The maintainer, 2024-12-18: "Extension functionality that requires Chrome UI (like toolbars, etc) will not be supported with Alloy style browsers." ([#3859](https://github.com/chromiumembedded/cef/issues/3859#issuecomment-2551686037))

3. **The only stock-CEF way to put a Chrome-style page inside a GPUI window is a native X11 child, so the whole app must run on X11 (XWayland on this Fedora 44 GNOME machine).**
   - Chrome style with a native parent has worked on Linux since CEF 102 (2022). **VERIFIED.**
   - Under Ozone/Wayland the parent handle is meaningless. The maintainer, 2026-10-09: "Native windowed embedding remains X11-only." ([#3687](https://github.com/chromiumembedded/cef/issues/3687#issuecomment-6090603340))
   - CEF 154 picks the **Wayland** Ozone backend by default in a Wayland session (`XDG_SESSION_TYPE=wayland`). The app must pass `--ozone-platform=x11`. **VERIFIED.**
   - GPUI picks Wayland whenever `WAYLAND_DISPLAY` is set. It must be forced to X11 too. **VERIFIED.**

4. **GPUI on crates.io (0.2.2) cannot hand out its X11 window id.** **VERIFIED.**
   - `impl HasWindowHandle for X11Window` is `unimplemented!()` and panics.
   - Zed PR [#50768](https://github.com/zed-industries/zed/pull/50768) (merged 2026-03-05) fixed this on `main`: it now returns an `XcbWindowHandle`.
   - `main` can also be pinned to X11 with `gpui_platform::linux(WindowingModes::X11)`.
   - So the shell must use GPUI from git, not 0.2.2. That fits anyway: 0.2.2 is a year old, and `main` has replaced Blade with wgpu and added AccessKit.

5. **Chrome style in a native child still falls short of "Chrome extensions behave as in Chrome".** These are windowing facts that limit extensions:
   - The child always has **no Chrome toolbar**. CEF's internal `ChildBrowserViewDelegate` does not override `GetChromeToolbarType`, so it gets the default `CEF_CTT_NONE`. **VERIFIED.**
   - Every Views-hosted or child Chrome-style browser is a Chromium `Browser` of **`TYPE_POPUP`** holding a single tab. **VERIFIED** (`chrome_browser_host_impl.cc`).
   - `TYPE_POPUP` never supports `kFeatureToolbar`, so `chrome.action.openPopup()` fails with "Browser window has no toolbar." (Chromium `extension_action_api.cc` and `window_feature_controller.cc`). **VERIFIED** in source; not yet tested at run time.
   - To extensions, every tab looks like a separate popup window. **INFERRED.**
   - CEF 154 has **no public extension API** to list actions, badges or icons, or to open action popups. **VERIFIED:** there is no `cef_extension.h` and no `LoadExtension`.

   Full parity therefore probably needs a patched CEF or Chromium, whatever windowing is chosen. The extension research should confirm this.

6. **A Wayland-native Chrome-style embedding is not on the horizon.**
   - CEF #2804 ("embedded Ozone/Wayland windows") has been open since 2019.
   - Its first concrete implementation, PR [#4233](https://github.com/chromiumembedded/cef/pull/4233) (open since 2026-08-04, unmerged), carries a 2,726-line Chromium patch.
   - It changes only the **Alloy** native delegate (`browser_platform_delegate_native_linux.cc`), not the Chrome-style child path (`chrome_child_window.cc`, `window_view.cc`). **VERIFIED** from the PR file list.
   - It also needs the embedder's own `wl_display`, set before `CefInitialize`.

7. **A separate CEF Views window composited next to GPUI's window cannot work on Wayland.**
   - `xdg_toplevel` has no request to set a window's position.
   - `xdg-foreign` only sets up a stacking parent.
   - A `wl_subsurface` must share one `wl_display` connection with its parent.

   **VERIFIED** from the protocol specs and the #2804/#4233 analysis. On X11 it is possible but strictly worse than a real child window.

8. **Pros and cons of the X11-child design.**
   - **Pros** (INFERRED): Chromium's native input, IME, a11y, popups and GPU presentation; Chrome-style extension machinery.
   - **Known problems, VERIFIED:**
     - Keyboard focus hand-over between embedder and child has been buggy on X11 for 10 years: #2026 was closed without a fix on 2026-10-10, and PR #4120 is still open.
     - GPUI's X11 backend treats a `FocusOut` or pointer-leave into a child window as the window going inactive (`x11/client.rs`).
     - GPUI's X11 backend does not implement `WindowKind::AnchoredPopup`.
     - IME on Ozone/X11 exists only through GtkUi. CEF turns GtkUi off with `multi_threaded_message_loop`, so the CEF UI thread must be the main thread, pumped from GPUI's loop.
   - **Known problems, INFERRED:** fractional scaling under XWayland is blurry or depends on the compositor.

9. **Recommendation.** It follows from the directive "total extension compatibility" plus stock CEF 154.
   - Primary: **Option B**. One Chrome-style CEF child per tab, parented into a GPUI (git) window; the whole process on X11/XWayland; CEF on the main thread with `external_message_pump`.
   - Treat Wayland-native as a later milestone, gated on upstream CEF work or our own patches.
   - Prototype B first on this RTX 5080 machine. Run the extension checks in a child browser on day one, because item 5 may force CEF or Chromium patches regardless of windowing.

---

## 1. Chrome style as a child of a native X11 window (`SetAsChild`)

### 1.1 It works in CEF 154, and has since CEF 102

- **VERIFIED.** In CEF 154 a `CefWindowInfo` with a non-zero `parent_window` and `runtime_style` of `DEFAULT` or `CHROME` produces a **Chrome-style** browser.
  - `CefBrowserCreateParams::IsChromeStyle()` treats `DEFAULT` and `CHROME` alike ([`libcef/browser/browser_host_create.cc`](https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/browser_host_create.cc)).
  - `CefBrowserHostBase::Create` then calls `chrome_child_window::MaybeCreateChildBrowser()`.
  - The only platform with an exception is macOS: "Chrome style is not supported with native parent on MacOS. See issue #3294."
- **VERIFIED.** On Linux, `cef_window_handle_t` is `unsigned long` (an XID): `pub type cef_window_handle_t = c_ulong` in `cef-dll-sys` bindings line 17388. The Rust `WindowInfo::set_as_child(parent, &bounds)` (in `cef/src/window_info.rs`) only sets `parent_window` and `bounds`.
- **VERIFIED.** The feature arrived in commit [`3000bc8748d8`](https://github.com/chromiumembedded/cef/commit/3000bc8748d818fa554b25d59c9ea09e47db6b9d) (2022-04-08), titled "chrome: win/linux: Add support for browser with native parent (see issue #3294)".
  - The GitHub compare API shows branch `5005` (CEF/Chromium 102) contains it and branch `4951` (101) does not.
  - It was then behind `--enable-chrome-runtime`.
  - The Chrome bootstrap became the sample-app default in 125.0.14 and the **only** bootstrap from M128 ([#3685](https://github.com/chromiumembedded/cef/issues/3685); the M125 style/bootstrap split is in [#3681](https://github.com/chromiumembedded/cef/issues/3681)).
- **VERIFIED.** How it is built, from [`chrome_child_window.cc`](https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/views/chrome_child_window.cc):
  - CEF creates a `CefBrowserView` with an internal `ChildBrowserViewDelegate`.
  - It puts that view in a `CefWindowImpl` whose widget is created with `params.parent_widget = <your XID>`, `params.child = true`, `TYPE_CONTROL`, opaque and frameless ([`window_view.cc` `CefWindowView::CreateWidget`](https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/views/window_view.cc)).
  - CEF's patch to Chromium's `X11Window::CreateXWindow` makes `properties.parent_widget` the real X parent of the new window. Menu windows are the exception and still go to the root window ([`patch/patches/views_widget.patch`](https://github.com/chromiumembedded/cef/blob/8037/patch/patches/views_widget.patch)).
  - So the page lives in a genuine X11 child window of the GPUI window, created by Chromium over its own X connection.
- **VERIFIED.** The child is a full Chrome-layer `Browser` but of **`TYPE_POPUP`**, created with `is_trusted_source = true` ("Don't show most controls" / "Don't show title bar or address") ([`chrome_browser_host_impl.cc` `CreateBrowser`](https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/chrome_browser_host_impl.cc)).
  - It is not the full "Chrome UI" window with a tab strip; you only get that with an empty `CefWindowInfo` (see §3.4).
- **VERIFIED.** The child has **no Chrome toolbar**. `ChildBrowserViewDelegate` overrides only the popup callbacks, and `CefBrowserViewDelegate::GetChromeToolbarType()` defaults to `CEF_CTT_NONE` ([`include/views/cef_browser_view_delegate.h`](https://github.com/chromiumembedded/cef/blob/8037/include/views/cef_browser_view_delegate.h)).
  - The embedder cannot supply its own delegate on this path.
  - The public `CefWindow::CreateTopLevelWindow(delegate)` has no parent parameter ([`include/views/cef_window.h`](https://github.com/chromiumembedded/cef/blob/8037/include/views/cef_window.h)).

### 1.2 Limitations and behaviour

**Focus**
- **VERIFIED** (history): keyboard focus between embedder widgets and the CEF X11 window has been a known problem since 2016.
  - [#2026](https://github.com/chromiumembedded/cef/issues/2026) was closed without comment on 2026-10-10.
  - The fix PR [#4120](https://github.com/chromiumembedded/cef/pull/4120) is still open. It says: "Mouse crossing into Cef browser view will take the keyboard focus…", "CefFocusHandler can not properly report OnGotFocus()…".
  - [#3426](https://github.com/chromiumembedded/cef/issues/3426) is a related revert: native-parented browsers lost focus on move.
- **INFERRED.** These reports mostly concern the Alloy `CefWindowX11` path. The Chrome-style child uses Chromium's own `X11Window`, which shares the X11 focus model, so expect to write explicit hand-over code:
  - `CefBrowserHost::SetFocus`, `CefFocusHandler`, and `XSetInputFocus` back to the GPUI window when GPUI chrome takes focus.
  - Keyboard shortcuts typed while the page has focus go to Chromium, not GPUI. Forward them from `CefKeyboardHandler::OnPreKeyEvent`.
- **VERIFIED, GPUI side.**
  - `main` `crates/gpui_linux/src/linux/x11/client.rs`: `Event::FocusOut` calls `window.set_active(false)` without checking `NotifyInferior`, and `XinputLeave` clears hover without checking the detail.
  - So once focus or the pointer moves into the CEF child, GPUI thinks its window is inactive or unhovered.
  - This needs a small GPUI patch. **INFERRED.**

**IME**
- **VERIFIED.** On Ozone/X11 the IME context comes only from `LinuxUi` (GtkUi).
  - [`ui/base/ime/linux/linux_input_method_context_factory.cc`](https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/base/ime/linux/linux_input_method_context_factory.cc) tries `LinuxUi::instance()->CreateInputMethodContext()`, then an Ozone factory, then `FakeInputMethodContext`.
  - The X11 Ozone platform registers no factory; only Wayland does (`WaylandInputMethodContext`, in [`ozone_platform_wayland.cc`](https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/ozone/platform/wayland/ozone_platform_wayland.cc)).
  - GtkUi implements it in [`ui/gtk/gtk_ui.cc`](https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/gtk/gtk_ui.cc) (`GtkUi::CreateInputMethodContext`).
- **VERIFIED.** CEF suppresses GtkUi when `CefSettings.multi_threaded_message_loop` is on (`SetMultiThreadedMessageLoopX11/Wayland` in [`patch/patches/linux_gtk_theme_3610.patch`](https://github.com/chromiumembedded/cef/blob/8037/patch/patches/linux_gtk_theme_3610.patch), called from [`libcef/browser/ui_thread.cc`](https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/ui_thread.cc)).
- **INFERRED, high confidence.** X11 + `multi_threaded_message_loop` leaves the page with **no IME** (fake context), and also no GTK file and print dialogs.
  - For Option B, run CEF's UI thread on the main thread.
  - Pump it from GPUI's calloop loop with `external_message_pump` + `CefDoMessageLoopWork()` (as the [general usage doc](https://github.com/chromiumembedded/cef/blob/master/docs/general_usage.md) describes, re-pumping at least every ~33 ms).

**Resizing and visibility**
- **VERIFIED.** The embedder moves and resizes the child itself through the browser's XID: `XConfigureWindow` on `browser->GetHost()->GetWindowHandle()`. It shows and hides it with `_NET_WM_STATE_HIDDEN` / mapping. This is what cefclient does ([`tests/cefclient/browser/browser_window_std_gtk.cc`](https://github.com/chromiumembedded/cef/blob/8037/tests/cefclient/browser/browser_window_std_gtk.cc)).
- **VERIFIED.** On the Chrome-style path, `ChildWindowDelegate::OnWindowBoundsChanged` calls `NotifyScreenInfoChanged()`, and initial bounds must not be empty (`DCHECK`).
- **VERIFIED.** There is an open HiDPI bug, but only for the **Alloy** native path: content is DSF times too large until the first resize ([#4208](https://github.com/chromiumembedded/cef/pull/4208), fixes #3396).

**Popups (`window.open`, `target=_blank`)**
- **VERIFIED.** For each popup the client must supply a parent again via `OnBeforePopup` → `SetAsChild`. Otherwise `OnPopupBrowserViewCreated` returns false and CEF creates a default Views top-level window (`chrome_child_window.cc`; commit messages on [#3294](https://github.com/chromiumembedded/cef/issues/3294)).
- **VERIFIED.** The cefclient native-parent popup test still crashes with `BadWindow` on Linux ([#3165](https://github.com/chromiumembedded/cef/issues/3165), open, "Reproduces with native parent… Does not reproduce when adding `--use-default-popup`").
- **VERIFIED.** Install X error handlers as cefclient and cefsimple do (`XSetErrorHandler` / `XSetIOErrorHandler` in [`cefclient_gtk.cc`](https://github.com/chromiumembedded/cef/blob/8037/tests/cefclient/cefclient_gtk.cc)). Otherwise Xlib's default handler can terminate the process on transient `BadWindow` errors. A 2026 CEF-based Rust app hit exactly this: [openhuman #2463](https://github.com/tinyhumansai/openhuman/issues/2463).

**Select, context-menu and tooltip popups**
- **VERIFIED.** The CEF patch parents menu-type X windows to the root window "to prevent them from being clipped to a parent window's bounds" (`views_widget.patch`).

**DevTools**
- **VERIFIED.** DevTools popups with a native parent are supported. The maintainer on #3294: "Support for DevTools popups with native parent was added in #3600" ([#3600](https://github.com/chromiumembedded/cef/issues/3600), Nov 2023).
- **INFERRED.** DevTools can therefore be created as another `SetAsChild` browser inside a GPUI panel.

**Fullscreen**
- **VERIFIED** (docs): "With Chrome style the fullscreen transition will be triggered automatically" (`CefDisplayHandler::OnFullscreenModeChange`, [`include/cef_display_handler.h`](https://github.com/chromiumembedded/cef/blob/8037/include/cef_display_handler.h)).
- **INFERRED.** For a non-top-level child the automatic transition cannot fullscreen the toplevel. The embedder must react to `OnFullscreenModeChange`: fullscreen the GPUI window, hide the chrome, and resize the child. Test this early.

**Wayland**
- **VERIFIED.** Not supported.
  - cefclient on master now exits: "Native parent windows are not supported with Wayland. Run with --ozone-platform=x11 (uses XWayland) or without --use-native." ([`tests/cefclient/cefclient_gtk.cc` on master](https://github.com/chromiumembedded/cef/blob/master/tests/cefclient/cefclient_gtk.cc)).
  - PR #4233's demo of stock CEF on Wayland: "the panel stays empty and the browser opens somewhere else entirely".

**Tabs**
- **VERIFIED** (docs on `cef_runtime_style_t`): "a Chrome style Window can host at most one Chrome style BrowserView".
- **INFERRED.** A tabbed shell will therefore create one Chrome-style child browser (one X window, one `TYPE_POPUP` Browser) per tab and switch tabs by mapping and unmapping.

---

## 2. Wayland status of CEF 154

**Default backend in a Wayland session: Wayland, not XWayland.** **VERIFIED.**
- CEF's `ChromeMainDelegateCef::PreSandboxStartup()` calls `ChromeMainDelegate::PreSandboxStartup()` ([`libcef/common/chrome/chrome_main_delegate_cef.cc`](https://github.com/chromiumembedded/cef/blob/8037/libcef/common/chrome/chrome_main_delegate_cef.cc)).
- For the browser process, that calls `ui::SetOzonePlatformForLinuxIfNeeded()` ([`chrome/app/chrome_main_delegate.cc`](https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/chrome/app/chrome_main_delegate.cc), line ~1602).
- In Chromium 154 that function appends `--ozone-platform=wayland` when `XDG_SESSION_TYPE == "wayland"`, and `x11` otherwise, unless `--ozone-platform` was already given ([`ui/linux/display_server_utils.cc`](https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/linux/display_server_utils.cc)).
- Both Ozone backends are compiled into Linux desktop builds ([`build/config/ozone.gni`](https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/build/config/ozone.gni)), and CEF does not patch this selection (grep of the CEF 154 tree).
- CEF master's own helper mirrors it: "Same default as ui::SetOzonePlatformForLinuxIfNeeded()" ([`tests/shared/browser/util_linux.cc`](https://github.com/chromiumembedded/cef/blob/master/tests/shared/browser/util_linux.cc)).
- Background: Chrome switched to auto-detecting Wayland in M140 (Aug 2025) ([OMG! Ubuntu](https://www.omgubuntu.co.uk/2025/08/chrome-140-wayland-auto-detection-linux)). Note that cefclient **154** still forced GDK to X11 (`gdk_set_allowed_backends("x11")`) while libcef chose Wayland; master fixed that mismatch in [`f922057`](https://github.com/chromiumembedded/cef/commit/f922057255).
- **Consequence:** on this machine an unconfigured CEF 154 app runs Ozone/Wayland. Option B must append `--ozone-platform=x11` in `CefApp::OnBeforeCommandLineProcessing`, or on the real command line.

**Views windows under Wayland: working and maintained, with rough edges.**
- **VERIFIED.** The maintainer, 2022-01-31: "Ozone builds can currently run with different platform backends (Wayland, X11, etc). Usage of the Views framework is required…" ([#2804](https://github.com/chromiumembedded/cef/issues/2804#issuecomment-1465020578)).
- **VERIFIED.** CEF 154's test suite carries Wayland-specific handling (e.g. `IsRunningOnWayland()` skips in `tests/ceftests/views/*`). `CefWindowImpl` simulates input on Wayland with `EventGenerator` (`libcef/browser/views/window_impl.cc`).
- **VERIFIED.** Master fixed Wayland issues as recently as [`124316c`](https://github.com/chromiumembedded/cef/commit/124316c82d) (2026-10-04, shadow artifacts with `multi-threaded-message-loop`).
- **VERIFIED, open rough edges:**
  - Frameless Views windows ask for server-side decorations on Wayland ([#4273](https://github.com/chromiumembedded/cef/issues/4273), CEF 152).
  - The legacy `DefaultFrameView` decoration ([#4061](https://github.com/chromiumembedded/cef/issues/4061), fixed upstream in about M149).
  - GTK-styled CSD is proposed but rejected in its current form ([#4178](https://github.com/chromiumembedded/cef/pull/4178)).
- **INFERRED.** "Supported" is de facto rather than declared: there is no formal support statement, but the maintainer actively fixes and tests Wayland Views. An old 2022 forum admin reply ("We don't currently test the Wayland-only ozone configuration", [forum t=18901](https://www.magpcss.org/ceforum/viewtopic.php?t=18901)) concerned `ozone_platform_x11=false` builds and is outdated.

**Windowless (OSR) under Wayland: works; the sample app gained native Wayland OSR after 154.**
- **VERIFIED.** The maintainer, 2026-10-09: the cefclient change "adds native OSR support on both Wayland and X11, including accelerated rendering through DMA-BUFs… Native windowed embedding remains X11-only." ([#3687](https://github.com/chromiumembedded/cef/issues/3687#issuecomment-6090603340))
  - The commits ([`c7e1a9e`](https://github.com/chromiumembedded/cef/commit/c7e1a9e5c6), [`f922057`](https://github.com/chromiumembedded/cef/commit/f922057255)) change only `tests/` and the build files, not `libcef/`.
  - They are cherry-picked to branch 8078 (CEF 156), not 8037.
- **INFERRED.** The libcef OSR in 154 already runs under Ozone/Wayland, and only the samples lacked it. The PR #4233 design doc says the same ("Windowless | works").
- **VERIFIED.** OSR `parent_window` is an XID or nothing. On Wayland pass 0, which the header warns may break "some functionality that requires a parent window" ([`include/internal/cef_linux.h`](https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_linux.h)).
- **VERIFIED, accelerated OSR caveats for this machine (NVIDIA RTX 5080).**
  - Shared-texture OSR needs `--use-angle=gl-egl`. Chromium's GBM `SCANOUT_CPU_READ_WRITE` path "breaks GPU rendering on NVIDIA" ([#3953](https://github.com/chromiumembedded/cef/issues/3953), [#4237](https://github.com/chromiumembedded/cef/issues/4237)).
  - The fix ([`c5b0618`](https://github.com/chromiumembedded/cef/commit/c5b06184bd), "Prefer native-handle SharedImages") landed 2026-10-09 in master and in 8078 as `3adefb8`, **not in 8037/CEF 154**.
  - cefclient 154 also forces `--ozone-platform=x11` for shared-texture OSR (`tests/shared/browser/client_app_browser.cc` in 8037); master dropped that.

**Native windowed embedding under Wayland: not possible with stock CEF.**
- **VERIFIED.** The maintainer, 2026-06-01: everything X11-specific (`CefWindowX11`, about 540 lines) is `#ifdef`'d under `SUPPORTS_OZONE_X11`, and a Wayland equivalent needs a `wl_subsurface` design plus large Ozone changes ([#2804 comment](https://github.com/chromiumembedded/cef/issues/2804#issuecomment-4594624031)).
- **VERIFIED.** Contributor PR [#4233](https://github.com/chromiumembedded/cef/pull/4233) (2026-08-04, open, no review yet) implements subsurface embedding.
  - It needs process-wide `cef_set_wayland_display(wl_display*)` before `CefInitialize`, `parent_window` holding a `wl_surface*`, an optional `parent_xdg_surface` for popups, and a new `CefBrowserHost::SetWindowBounds()`.
  - Known limits: one frame of resize lag; popups without an `xdg_surface` are clipped and lose grabs.
  - It changes only the Alloy native delegate.
- **VERIFIED.** The Phoronix claim (2026-05-26) that ANGLE's Wayland support was "the last blocker" ([Phoronix](https://www.phoronix.com/news/ANGLE-Merges-Wayland)) was corrected by the maintainer: "ANGLE-Wayland is a rendering concern and doesn't affect the embedder windowing mechanism needed here."

---

## 3. CEF Views with Chrome style: toolbars, own UI, extension buttons, and coexistence with GPUI

### 3.1 Chrome toolbar types

**VERIFIED** ([`chrome_browser_view.cc` `OverrideCreateToolbar`](https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/views/chrome_browser_view.cc); [`patch/patches/chrome_runtime_views.patch`](https://github.com/chromiumembedded/cef/blob/8037/patch/patches/chrome_runtime_views.patch)):
- `CEF_CTT_NORMAL` maps to `ToolbarView::DisplayMode::kNormal`.
- `CEF_CTT_LOCATION` maps to `kLocation`.
- `CEF_CTT_NONE` (the default) means no configured toolbar. CEF still builds a hidden "fallback toolbar"; the patch comment says "CEF initializes a fallback toolbar even when none is configured."
- The toolbar is a `CefView` from `CefBrowserView::GetChromeToolbar()`, available after `OnWindowChanged`. The embedder places it anywhere in its Views layout ([`include/views/cef_browser_view.h`](https://github.com/chromiumembedded/cef/blob/8037/include/views/cef_browser_view.h)).
- Parts can be hidden through `CefCommandHandler`: `IsChromeToolbarButtonVisible` (media, battery saver, avatar), `IsChromePageActionIconVisible` and `IsChromeAppMenuItemVisible` ([`include/cef_command_handler.h`](https://github.com/chromiumembedded/cef/blob/8037/include/cef_command_handler.h)).

### 3.2 Extension action buttons and popups

- **VERIFIED.**
  - In Chromium 154 [`toolbar_view.cc`](https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/chrome/browser/ui/views/toolbar/toolbar_view.cc), `ToolbarView::Init()` builds `ExtensionsToolbarDesktop` (puzzle menu and pinned actions) only on the `kNormal` path, and only for non-guest profiles when `IsWebUIExtensionsContainerEnabled()` is off; otherwise the container sits in the WebUI toolbar.
  - The `kLocation` branch returns early after adding only `PinnedToolbarActionsContainer` ("so that downloads can be shown in popups").
- **So:**
  - `CEF_CTT_NORMAL` shows Chrome's extension buttons, and clicking one opens its popup through Chrome's normal UI. This is how it reads in the source; I have not run it.
  - `CEF_CTT_LOCATION` and `CEF_CTT_NONE` show **no** extension action buttons.
- **VERIFIED in source; not yet run.** The API path `chrome.action.openPopup()` / `browserAction.openPopup()` in [`extension_action_api.cc` `OpenPopupInBrowser`](https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/chrome/browser/extensions/api/extension_action/extension_action_api.cc) fails with "Browser window has no toolbar." unless the Browser supports `kFeatureToolbar` and the toolbar is visible.
  - [`window_feature_controller.cc`](https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/chrome/browser/ui/window_feature_controller/window_feature_controller.cc): `PopupBrowserSupportsWindowFeature` returns false for `kFeatureToolbar`.
  - CEF makes every Views-hosted Chrome browser `TYPE_POPUP`.
  - CEF's `ChromeBrowserDelegate::SupportsWindowFeature` overrides only `kFeatureTitleBar`, for picture-in-picture.
  - So `openPopup()` fails in all Views-hosted and native-child CEF browsers, even with `CEF_CTT_NORMAL`. Only full "Chrome UI" windows (`TYPE_NORMAL`, §3.4) avoid this.
- **VERIFIED.** CEF 154 exposes no public extension API (no `include/cef_extension.h`, no `CefRequestContext::LoadExtension`). The only Chrome hooks are `ExecuteChromeCommand` and `CanExecuteChromeCommand` with IDC ids ([`include/cef_browser.h`](https://github.com/chromiumembedded/cef/blob/8037/include/cef_browser.h)).
- **VERIFIED** (maintainer, 2023): "Using the Chrome runtime is required for extension support" ([#3294](https://github.com/chromiumembedded/cef/issues/3294#issuecomment-1465030786)).

### 3.3 Can the embedder draw its own UI with Views?

- **VERIFIED, only from CEF's fixed widget set:** `CefPanel`, `CefLabelButton`, `CefMenuButton`, `CefTextfield`, `CefScrollView`, box and fill layouts, `CefWindow::AddOverlayView` → `CefOverlayController`, `ShowMenu`, and draggable regions ([`include/views/`](https://github.com/chromiumembedded/cef/tree/8037/include/views)).
- **VERIFIED.** There is no custom-paint callback (`CefViewDelegate` has only size, focus, layout and theme hooks) and no external-texture view. GPUI cannot render inside a CEF Views window.
- **VERIFIED.** Mixing is allowed one way: a Chrome-style `CefWindow` may host one Chrome-style `CefBrowserView` plus several Alloy-style ones, but an Alloy `CefWindow` may host only Alloy views (`cef_runtime_style_t` docs in [`include/internal/cef_types.h`](https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types.h)).

### 3.4 "Chrome UI" windows

- **VERIFIED.** Creating a browser with an empty `CefWindowInfo` (no parent, no Views) still gives "a fully-featured Chrome browser window": `TYPE_NORMAL`, tab strip, Chrome toolbar with extensions (commit message of `3000bc8`; #3685 "Fully styled Chrome UI window: Add `--use-native` (cefsimple only)").
- **VERIFIED.** CEF 154 is still fixing it, e.g. [#4285](https://github.com/chromiumembedded/cef/issues/4285) (new-tab button crash, fixed in 8037).
- This mode gives the best extension parity, but the chrome is Chrome's own, not GPUI. It is useful as a baseline in tests and possibly for `chrome://extensions`, the Web Store and extension options. Note also: the maintainer suggests WebUI pages such as `chrome://settings` belong in Chrome-style browsers ([#3763](https://github.com/chromiumembedded/cef/issues/3763#issuecomment-2273935544)), and Alloy browsers block `chrome://extensions` ([#3859](https://github.com/chromiumembedded/cef/issues/3859)).

### 3.5 GPUI window next to a CEF Views window

**X11.** Two separate X11 toplevels in one process is fine (GPUI on x11rb/XCB, Chromium on its own `x11::Connection`).
- **INFERRED.** The app could put a frameless CEF Views window over a region of the GPUI window and track moves and resizes. This depends on the window manager, lags, adds a second taskbar and alt-tab entry, and makes activation and focus flip between toplevels.
- **INFERRED.** It is strictly worse than §1's true child window.
- **INFERRED, untested hack.** Reparent the CEF Views top-level XID (`CefWindow::GetWindowHandle()`) into the GPUI window, the way cefclient reparents popups. This would allow `CEF_CTT_NORMAL` plus overlays inside the GPUI window. Chromium's `DesktopWindowTreeHostLinux` assumes it is top-level, so expect focus, `_NET_WM` state and coordinate bugs.

**Wayland.** It cannot be composited. **VERIFIED** from the protocols:
- [`xdg_toplevel`](https://wayland.app/protocols/xdg-shell) has 14 requests and none sets a global position. `set_parent` only affects stacking.
- [`xdg-foreign-unstable-v2`](https://wayland.app/protocols/xdg-foreign-unstable-v2) only gives other clients' toplevels a `set_parent_of` stacking relationship.
- `wl_subcompositor_get_subsurface()` requires parent and child on the **same** `wl_display` connection. GPUI and Chromium each open their own (PR #4233 design doc; #2804).

Coexistence on Wayland therefore means independent windows only; for example, CEF Views windows for DevTools or extension pages next to GPUI browser windows.

---

## 4. GPUI on Linux: X11 window id and forcing X11

**Backend selection.**
- **VERIFIED, gpui 0.2.2:** `guess_compositor()` returns Wayland if `WAYLAND_DISPLAY` is non-empty, else X11 if `DISPLAY`, else headless (`gpui-0.2.2/src/platform.rs`).
- **VERIFIED, Zed `main`:** `select_backend(WindowingModes, env)` uses the same order (`crates/gpui_linux/src/linux/display_connection.rs`). `main` adds `gpui_platform::linux(gpui::WindowingModes::X11)` and `Application::with_windowing(..)` to restrict to X11 without touching environment variables (`crates/gpui_platform/src/gpui_platform.rs`, `crates/gpui/src/app.rs`).
- **VERIFIED, alternative for 0.2.2:** build without the `wayland` feature (`default-features = false, features = ["x11", "font-kit"]`), or clear `WAYLAND_DISPLAY` before `Application::new`.

**Raw X11 window id.**
- **VERIFIED, gpui 0.2.2:** `impl rwh::HasWindowHandle for X11Window { fn window_handle(..) { unimplemented!() } }` (`src/platform/linux/x11/window.rs:314`). `gpui::Window::window_handle()` delegates to it (`src/window.rs:4845`), so it **panics** on X11.
  - The internal `RawWindow` used by the Blade renderer has the XID but is not public.
  - Wayland returns a `WaylandWindowHandle` (`wl_surface*`).
- **VERIFIED, Zed `main`:** fixed by [PR #50768](https://github.com/zed-industries/zed/pull/50768), "gpui_linux: Implement raw-window-handle traits for X11Window", merged 2026-03-05. Its motivation was embedding a child-window 3D viewport.
  - `X11Window::window_handle()` now returns `XcbWindowHandle::new(x_window)` (`crates/gpui_linux/src/linux/x11/window.rs:340`).
  - That `u32` is exactly what `WindowInfo::set_as_child(parent as c_ulong, &bounds)` needs.

**Other GPUI `main` facts that matter for embedding.**
- **VERIFIED.** The X11 backend prefers a 32-bit ARGB visual for its windows (`find_visuals`, `x11/window.rs`). **INFERRED:** a 24-bit Chromium child inside a 32-bit parent is legal X11 when visual, colormap and border are set, which Chromium's `CreateXWindow` does. Watch for alpha artifacts.
- **VERIFIED.** On X11, `WindowKind::AnchoredPopup` returns `PopupNotSupportedError`; only `WindowKind::PopUp` (override-redirect) exists. Wayland implements `AnchoredPopup` with `xdg_popup` (`crates/gpui/src/platform.rs`; `x11/window.rs:479`; `wayland/window.rs:207`). **INFERRED:** GPUI overlays that must cover web content (omnibox suggestions, menus) need separate `PopUp` windows placed in screen coordinates, or a GPUI patch.
- **VERIFIED.** `main` renders with wgpu (`gpui_wgpu`, a git revision of wgpu) and has AccessKit (`accesskit_unix`), per `crates/gpui_linux/Cargo.toml`. 0.2.2 used blade-graphics and had no AccessKit.
- **VERIFIED.** `PaintSurface` (external-texture primitive) carries data only on macOS/iOS (`CVPixelBuffer`), and the wgpu renderer's `PrimitiveBatch::Surfaces` arm is a no-op (`crates/gpui/src/scene.rs:768`; `crates/gpui_wgpu/src/wgpu_renderer.rs:1608`). There is no zero-copy way to show a DMA-BUF in GPUI on Linux without patching GPUI (relevant to Option A).
- **VERIFIED.** GPUI starts background threads as soon as the platform is created (`LinuxDispatcher::new` spawns `available_parallelism` workers plus a timer thread). cefclient notes: "The Chromium sandbox requires that there only be a single thread during initialization" (`cefclient_gtk.cc`). **INFERRED:** call `CefInitialize` before creating the GPUI `Application`.

**What breaks if the whole app is forced to X11 (XWayland).** All INFERRED unless marked.
- **Fractional and per-monitor scaling.**
  - X11 has one global scale: GPUI uses Xft.dpi, `GPUI_X11_SCALE_FACTOR`, or `randr` (**VERIFIED**, `get_scale_factor` in `x11/client.rs`).
  - Under GNOME fractional scaling, XWayland clients are upscaled and blurry unless the experimental Mutter `xwayland-native-scaling` flag is on. Sources are user forums only, e.g. [Arch BBS 2026](https://bbs.archlinux.org/viewtopic.php?id=313256), so treat this as low confidence.
  - Mixed-DPI multi-monitor setups degrade.
  - GPUI and Chromium must agree on the scale; both read X resources.
- **Display features.** No Wayland-only features for either toolkit: HDR/color-management paths, fractional-scale-v1, per-surface scale, xdg-activation tokens and similar.
- **Security.** X11 has no isolation between XWayland clients (input snooping among X clients).
- **IME.** Works through XIM (GPUI) and GTK IM modules (Chromium, GtkUi only, §1.2) via ibus or fcitx X11 frontends.
- **Screen capture.** WebRTC `getDisplayMedia` on X11 Ozone may fall back to X11 capture, which only sees X clients, unless the PipeWire capturer is enabled. CEF enabled PipeWire capture for Wayland in [#4053](https://github.com/chromiumembedded/cef/issues/4053), closed 2025-12. Behaviour on X11 Ozone inside a Wayland session needs testing.
- **Rendering.** Rootless XWayland composites X child windows into the toplevel's buffer: probably one extra GPU blit per frame, with no direct scanout or overlays for video. NVIDIA on XWayland needs explicit sync (driver 555+), which Fedora 44's driver has.
- **Platform direction.** Long-term Wayland-only desktops (GNOME 50 has no X11 session) still ship XWayland, but it is legacy.

---

## 5. Realistic options: a Chromium page in a GPUI window on Linux

| | **A. Windowless / OSR (Alloy) into GPUI** | **B. Native child, Chrome style, X11** | **C. Separate CEF Views window next to GPUI** |
|---|---|---|---|
| Wayland | Yes, natively (libcef 154 OSR works under Ozone/Wayland; INFERRED from the #3687 commits and PR #4233 doc) | **No.** Whole app on XWayland (`--ozone-platform=x11` + GPUI X11 backend) | Separate windows only; no composition (no toplevel positioning, no cross-connection subsurfaces) |
| X11 | Yes | Yes (since CEF 102) | Possible but WM-dependent and laggy; strictly worse than B |
| Page input | Embedder forwards everything (`SendMouse*`, `SendKeyEvent`, touch, DnD through `StartDragging`/`DragTarget*`, cursors through `OnCursorChange`) | Native Chromium X11 input; focus hand-over is manual and historically buggy (#2026, #4120); GPUI needs a focus/hover patch | Native |
| IME | Embedder: GPUI `InputHandler` (XIM or text-input-v3) → `ImeSetComposition`/`ImeCommitText`, caret from `OnImeCompositionRangeChanged` (APIs VERIFIED) | Native through GtkUi; requires **not** using `multi_threaded_message_loop` (VERIFIED pieces, §1.2) | Native (Wayland text-input or GTK) |
| Accessibility | Only `CefAccessibilityHandler::OnAccessibilityTreeChange/LocationChange` (raw `CefValue` trees, VERIFIED). Bridging to AccessKit/AT-SPI is a large project | Chromium's own ATK/AT-SPI tree, separate from GPUI's AccessKit tree (INFERRED: two AT-SPI app roots from one process) | Native |
| Popups, menus, dialogs | `<select>` arrives as `PET_POPUP` paints the embedder composites; context menus and file dialogs need handlers or a parent window, which is absent on Wayland (VERIFIED header warning) | Chromium X windows (menus parented to root); `window.open` needs `SetAsChild` in `OnBeforePopup`; #3165 crash risk | Native |
| GPU efficiency | Software `OnPaint` means GPU readback + CPU copy + upload every frame. Accelerated `OnAcceleratedPaint` gives DMA-BUF planes you must copy within the callback (VERIFIED). On **NVIDIA it needs CEF ≥156** (#4237 fix not in 154) plus `--use-angle=gl-egl`. GPUI needs a patch to draw external textures (`Surfaces` batch is a no-op). The `cef` crate's `accelerated_osr` importer targets **wgpu 30** (crates.io); GPUI `main` pins a git wgpu, so versions must match | Chromium presents straight to its X window; probably one XWayland composite copy (INFERRED). No CPU copies | Best: native Wayland toplevel |
| Extensions | **Weak.** Alloy style: "only supports extension APIs that are required by the embedded components like the PDF Viewer" (#4255); RequestContext-level features work (#3859). Content scripts, messaging, `tabs.get`/`executeScript` by id work, but `windowId=-1` and the tab is in no window (#4255 table). No action UI. `chrome://extensions` blocked in Alloy browsers | **Good engine-side, incomplete UI-side.** Chrome-style `Browser` per tab, but `TYPE_POPUP`, no toolbar (`CEF_CTT_NONE` fixed), `chrome.action.openPopup` → "Browser window has no toolbar."; each tab looks like its own popup window; no public CEF extension-action API. Action UI must be rebuilt in GPUI, and popups emulated (or CEF patched) | Same engine-side as B. With `CEF_CTT_NORMAL` you get Chrome's own extension buttons and click-to-open popups, but the chrome is Chrome's, not GPUI's, and the API popup path still fails (`TYPE_POPUP`) |
| Fit with "GPUI for all chrome" | Full | Full (GPUI around the page; overlays above the page need GPUI `PopUp` windows) | Poor |

**Not in the table:**
- **D.** Full "Chrome UI" windows: best extension parity, but no GPUI chrome.
- **E.** Carrying PR #4233 or an equivalent Wayland subsurface patch in a custom CEF build. It is Alloy-only today, needs the embedder's `wl_display` before `CefInitialize` (which conflicts with initialising GPUI after CEF, INFERRED), and needs GPUI to share its connection.

### Suggested first prototypes

These are INFERRED priorities. Each answers a question that decides the architecture.

1. **B on this machine.**
   - Set up: GPUI from git, `WindowingModes::X11`; CEF 154; `--ozone-platform=x11`; `external_message_pump` on the main thread; `SetAsChild(x_window)`.
   - Check:
     - focus round trips between the GPUI omnibox and the page;
     - ibus IME in a page `<input>`;
     - resizing and HiDPI under fractional scaling;
     - `<select>` and context menus;
     - `window.open`;
     - HTML5 fullscreen;
     - DevTools as a second child.
2. **Extension checks in a B child browser.**
   - `chrome.tabs.query({active:true,currentWindow:true})`, `chrome.windows.getAll()`, `chrome.action.openPopup()`.
   - Installing from chromewebstore.google.com.
   - The `_execute_action` command.
   - Measure how far stock CEF is from "behaving as in Chrome" before committing to custom patches.
3. **A, CPU path only, as a control.** Measure frame cost at 4K and confirm what an Alloy browser loses compared with B.

---

## Open questions

- Does Chrome-style native-child fullscreen do anything sensible, or must the embedder drive it all (`OnFullscreenModeChange`)?
- Does `chrome.action.openPopup` really fail in `CEF_CTT_NORMAL` Views browsers, or does CEF override `kFeatureToolbar` somewhere I missed? Test at run time.
- Will CEF accept a small patch letting native-child browsers use `TYPE_NORMAL` or a configurable toolbar type, or exposing an extension-action model? Ask the maintainer.
- How does Orca handle two AT-SPI application roots (GPUI AccessKit + Chromium ATK) from one process?
- How good is XWayland fractional scaling on GNOME 50 / Fedora 44, with or without `xwayland-native-scaling`?
- Will PR #4233 grow a Chrome-style path, and on what timescale?

---

## Sources

**CEF source, branch 8037 (CEF 154), unless noted**
- https://github.com/chromiumembedded/cef/tree/8037 (head 14c5a08, "Update to Chromium version 154.0.8037.98")
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/views/chrome_child_window.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/views/window_view.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/browser_host_create.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/chrome_browser_host_impl.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/views/chrome_browser_view.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/chrome_browser_delegate.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/extensions/chrome_extension_util.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/ui_thread.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/common/chrome/chrome_main_delegate_cef.cc
- https://github.com/chromiumembedded/cef/blob/8037/patch/patches/views_widget.patch
- https://github.com/chromiumembedded/cef/blob/8037/patch/patches/chrome_runtime_views.patch
- https://github.com/chromiumembedded/cef/blob/8037/patch/patches/chrome_browser_extensions.patch
- https://github.com/chromiumembedded/cef/blob/8037/patch/patches/linux_gtk_theme_3610.patch
- https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types.h (`cef_runtime_style_t`, `cef_chrome_toolbar_type_t`, `cef_chrome_toolbar_button_type_t`, `windowless_frame_rate`)
- https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_linux.h
- https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types_linux.h
- https://github.com/chromiumembedded/cef/blob/8037/include/views/cef_browser_view_delegate.h
- https://github.com/chromiumembedded/cef/blob/8037/include/views/cef_browser_view.h
- https://github.com/chromiumembedded/cef/blob/8037/include/views/cef_window.h
- https://github.com/chromiumembedded/cef/blob/8037/include/cef_display_handler.h
- https://github.com/chromiumembedded/cef/blob/8037/include/cef_render_handler.h
- https://github.com/chromiumembedded/cef/blob/8037/include/cef_accessibility_handler.h
- https://github.com/chromiumembedded/cef/blob/8037/include/cef_command_handler.h
- https://github.com/chromiumembedded/cef/blob/8037/tests/cefclient/cefclient_gtk.cc
- https://github.com/chromiumembedded/cef/blob/8037/tests/cefclient/browser/browser_window_std_gtk.cc
- https://github.com/chromiumembedded/cef/blob/8037/tests/shared/browser/client_app_browser.cc
- https://github.com/chromiumembedded/cef/blob/master/tests/cefclient/cefclient_gtk.cc (master, Wayland refusal)
- https://github.com/chromiumembedded/cef/blob/master/tests/shared/browser/util_linux.cc
- https://github.com/chromiumembedded/cef/blob/master/docs/architecture.md
- https://github.com/chromiumembedded/cef/blob/master/docs/general_usage.md

**CEF commits**
- https://github.com/chromiumembedded/cef/commit/3000bc8748d818fa554b25d59c9ea09e47db6b9d (Chrome native parent, 2022-04-08; in branch 5005/M102)
- https://github.com/chromiumembedded/cef/commit/124316c82d (Wayland shadow fix, 2026-10-04)
- https://github.com/chromiumembedded/cef/commit/c5b06184bd (NVIDIA shared-texture OSR, master; 8078 as 3adefb829c)
- https://github.com/chromiumembedded/cef/commit/c7e1a9e5c6 and https://github.com/chromiumembedded/cef/commit/f922057255 (cefclient Wayland OSR, master/8078 only)

**CEF issues and PRs**
- #2804 https://github.com/chromiumembedded/cef/issues/2804 (Wayland embedding; maintainer comments 2022-01-31, 2026-06-01)
- PR #4233 https://github.com/chromiumembedded/cef/pull/4233 (Wayland subsurface embedding, open)
- #3294 https://github.com/chromiumembedded/cef/issues/3294 (Chrome native parent; DevTools #3600)
- #3165 https://github.com/chromiumembedded/cef/issues/3165 (native-parent popup BadWindow, open)
- #2026 https://github.com/chromiumembedded/cef/issues/2026 and PR #4120 https://github.com/chromiumembedded/cef/pull/4120 (X11 focus)
- #3681 https://github.com/chromiumembedded/cef/issues/3681, #3685 https://github.com/chromiumembedded/cef/issues/3685 (style/bootstrap split; Alloy bootstrap removal)
- #3263 https://github.com/chromiumembedded/cef/issues/3263, #3293 https://github.com/chromiumembedded/cef/issues/3293 (OSR via Ozone; OSR = Chrome bootstrap + Alloy style)
- #3687 https://github.com/chromiumembedded/cef/issues/3687 (Linux OnAcceleratedPaint sample; "Native windowed embedding remains X11-only")
- #3953 https://github.com/chromiumembedded/cef/issues/3953, #4237 https://github.com/chromiumembedded/cef/issues/4237 (OSR shared textures, ANGLE EGL, NVIDIA)
- #2575 https://github.com/chromiumembedded/cef/issues/2575 (OnAcceleratedPaint origin, April 2024)
- #3859 https://github.com/chromiumembedded/cef/issues/3859, #4255 https://github.com/chromiumembedded/cef/issues/4255 (Alloy-style extension limits)
- #3763 https://github.com/chromiumembedded/cef/issues/3763 (WebUI in Alloy style)
- #4061 https://github.com/chromiumembedded/cef/issues/4061, #4273 https://github.com/chromiumembedded/cef/issues/4273, #4178 https://github.com/chromiumembedded/cef/pull/4178 (Wayland decorations)
- #4208 https://github.com/chromiumembedded/cef/pull/4208 (Alloy native HiDPI)
- #4285 https://github.com/chromiumembedded/cef/issues/4285 (Chrome UI window in 154)
- #4053 https://github.com/chromiumembedded/cef/issues/4053 (PipeWire capture on Wayland)
- CEF Forum t=18901 (2022, outdated) https://www.magpcss.org/ceforum/viewtopic.php?t=18901

**Chromium 154.0.8037.98**
- https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/linux/display_server_utils.cc
- https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/chrome/app/chrome_main_delegate.cc
- https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/build/config/ozone.gni
- https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/chrome/browser/ui/views/toolbar/toolbar_view.cc
- https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/chrome/browser/extensions/api/extension_action/extension_action_api.cc
- https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/chrome/browser/ui/window_feature_controller/window_feature_controller.cc
- https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/base/ime/linux/linux_input_method_context_factory.cc
- https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/ozone/platform/x11/ozone_platform_x11.cc
- https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/ozone/platform/wayland/ozone_platform_wayland.cc
- https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/ui/gtk/gtk_ui.cc

**GPUI / Zed**
- https://crates.io/crates/gpui/0.2.2 (`src/platform.rs` `guess_compositor`; `src/platform/linux/x11/window.rs:314` `unimplemented!()`; `src/window.rs:4845`; `src/platform/linux/dispatcher.rs`)
- https://github.com/zed-industries/zed/pull/50768 (X11 raw-window-handle, merged 2026-03-05)
- https://github.com/zed-industries/zed/blob/main/crates/gpui_linux/src/linux/x11/window.rs (XcbWindowHandle; AnchoredPopup unsupported; ARGB visual)
- https://github.com/zed-industries/zed/blob/main/crates/gpui_linux/src/linux/x11/client.rs (FocusOut/XinputLeave handling; scale factor)
- https://github.com/zed-industries/zed/blob/main/crates/gpui_linux/src/linux/display_connection.rs (`select_backend`)
- https://github.com/zed-industries/zed/blob/main/crates/gpui_linux/Cargo.toml (wgpu, AccessKit)
- https://github.com/zed-industries/zed/blob/main/crates/gpui_platform/src/gpui_platform.rs (`linux(WindowingModes)`)
- https://github.com/zed-industries/zed/blob/main/crates/gpui/src/platform.rs (`WindowKind`)
- https://github.com/zed-industries/zed/blob/main/crates/gpui/src/scene.rs and https://github.com/zed-industries/zed/blob/main/crates/gpui_wgpu/src/wgpu_renderer.rs (`PaintSurface` macOS-only; Surfaces batch no-op)

**cef-rs (Rust bindings)**
- Local: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cef-154.5.0+154.0.34/src/window_info.rs` (`set_as_child`, `set_as_windowless` forces ALLOY), `src/osr_texture_import/dmabuf.rs` (DMA-BUF → wgpu 30 via Vulkan), `Cargo.toml` (`linux-x11` is only a bundler passthrough feature)
- Local: `cef-dll-sys-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs` (`cef_window_handle_t = c_ulong`, `cef_get_xdisplay`, `CEF_VERSION`)
- https://github.com/tauri-apps/cef-rs/tree/main/examples/osr (winit + wgpu accelerated OSR, `external_message_pump`)

**Wayland protocols and other**
- https://wayland.app/protocols/xdg-shell
- https://wayland.app/protocols/xdg-foreign-unstable-v2
- https://www.phoronix.com/news/ANGLE-Merges-Wayland (2026-05-26; claim corrected by maintainer on #2804)
- https://www.omgubuntu.co.uk/2025/08/chrome-140-wayland-auto-detection-linux
- https://bbs.archlinux.org/viewtopic.php?id=313256 (XWayland scaling on GNOME 50; low trust)
- https://github.com/tinyhumansai/openhuman/issues/2463 (X BadWindow terminating a CEF Rust app; context for X error handlers)
- https://github.com/leandromqrs/cef-wayland-embed-sample (Rust/winit test rig for PR #4233; not evaluated)
