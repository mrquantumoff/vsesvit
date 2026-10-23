# spike-win: GPUI (Windows, DirectComposition) + Chrome-style CEF children

Date: 2026-10-23. Spike code: `docs/design/linux-cef/spikes/win-core` (own `[workspace]`). Scratch, CEF binaries, target dir and run logs: `.audit/scratch/win-spike` (gitignored).
Versions: `cef` 154.5.0+154.0.34 (CEF 154.0.34, Chromium 154.0.8037.98), `gpui-pre` / `gpui-pre-platform` / `gpui-pre-windows` 0.3.9, `gpui-component` 0.7.3, rustc 1.99.0 MSVC, Visual Studio 18 Community (its CMake + Ninja), Windows 11 Pro 10.0.26300, one 3840×2160 monitor at 175 % (168 dpi).

Labels: **VERIFIED** = I ran it and saw the result (command and output given). **INFERRED** = my conclusion, not run. **NOT VERIFIED** = tried, did not work. **INCONCLUSIVE** = could not be run safely on this PC.

## Resume notes

- In flight: nothing. All items written; last self-test 29 passed, 0 failed (`runs/selftest.txt`).
- Open ends if anyone continues: download bubble and `set_preference` returning 0 (§10), 100/150/200 % on a real display (§5), WM_DPICHANGED.
- Every run goes through `scripts/lib.ps1` `Start-Spike` (hidden desktop by default). Run logs land in `.audit/scratch/win-spike/runs/<label>/spike.log`.
- Build: `. .audit\scratch\win-spike\env.ps1; cd docs\design\linux-cef\spikes\win-core; cargo build` (stop the page server first; cargo cannot replace a running `serve.exe`).

---

## 0. Summary

| # | Item | Result |
|---|---|---|
| 1 | Build, binaries, helper, sandbox | VERIFIED. `cef-dll-sys` downloads the minimal CEF build into `CEF_PATH` and copies the runtime next to the exe. Separate helper exe and same exe both work unsandboxed. The sandbox works only through CEF's `bootstrap.exe` loading our code as a DLL (`RunWinMain`): renderers `Lockdown`, `Untrusted` integrity. Clean build 3 min 54 s (CEF archive already present), crate rebuild 15–36 s |
| 2 | Message loop | VERIFIED. Both work. Idle 3.8–5.1 % (external) vs 3.3–3.7 % (multi-threaded) of one core, all processes. External pump needs the GPUI key patch; multi-threaded needs a wait before `cef::shutdown`. Recommended: external pump + patch (§4) |
| 3 | Embedding, resize, HiDPI | VERIFIED: the child renders only through transparent GPUI pixels (GPUI's DirectComposition target is topmost); follows window resize and layout changes; works with or without an intermediate host HWND (host recommended). 175 % VERIFIED (dpr 1.75, page fills the rect exactly). 100/150/200 % INCONCLUSIVE (one 175 % monitor; Chromium's forced scale 1 and 2 fill the same rect) |
| 4 | Tabs, navigation, events | VERIFIED. 3 tabs, switch with ShowWindow + `was_hidden` (page `visibilityState` follows), close with `on_before_close`. 30 open/close cycles in each pump mode: no process, handle, GDI, USER or HWND growth. Title/URL/loading/favicon reach GPUI |
| 5 | Keyboard focus | VERIFIED. GPUI stays active with focus in the child (it handles only WM_ACTIVATE). Ctrl+L from the page and from the chrome, Tab-out of the page (`on_take_focus`) and click-to-page all land right, both pump modes. Needs the GPUI key patch with the external pump. Tab inside GPUI does not hand focus back to the page yet (shell work) |
| 6 | Chrome accelerators | VERIFIED matrix (§8). `on_pre_key_event` catches all eight; without interception Ctrl+T/N, F11 and F12 open Chrome-native windows |
| 7 | Overlays | VERIFIED. (a) `WindowKind::PopUp` is TOPMOST and stays above other apps' windows when ours deactivates; fix: owner = main window and clear TOPMOST, then it sits just above its owner. (b) An in-window GPUI overlay is painted above the page with no cut-out (DirectComposition is topmost), but mouse presses go to Chromium; a `SetWindowRgn` hole in the host routes them to GPUI (§9) |
| 8 | Popups, dialogs | VERIFIED (§10). All reach their handlers. Download bubble and permission bubble are Chrome-native toplevels by default; the download bubble still shows with the handler in place |
| 9 | In-process screenshot | VERIFIED. `PrintWindow(PW_RENDERFULLCONTENT)` of the GPUI toplevel includes the page: 5 captures of 2256×1440 avg 19.9 ms, max 21.3 ms. Windows.Graphics.Capture fails on a non-input desktop |
| 10 | Clean shutdown | VERIFIED. Shell action and WM_CLOSE to our toplevel, both pump modes, 3 runs each with two tabs: every `on_before_close`, `cef::shutdown` 87–129 ms, exit 0 in 377–419 ms, no processes left |
| 11 | Platform seam | INFERRED design (§13): one `vsesvit-cef` glue crate and one GPUI shell crate; a `NativeSurface` trait per platform for content host, geometry, show/hide, focus, input holes, popup fix-up, capture and toplevel-at-point; a `Pump` per platform |

`scripts/selftest.ps1` runs 29 end-to-end checks covering items 1–10: external pump, multi-threaded loop, an unpatched-GPUI negative control, the user-desktop policy, and the sandboxed bundle. Last run: **29 passed, 0 failed** (`runs/selftest.txt`).

---

## 1. How the tests were run, and traps found on the way

### 1.1 A private desktop instead of the user's screen (VERIFIED)

The PC is the user's. Two findings made a separate desktop necessary:

- `SetFocus` on our inactive toplevel activates it, and Windows makes it the **foreground window** when the process holds foreground rights. The previous agent's runs (`runs/smoke`, `runs/fg`) logged `foreground before=0x10588 after=<spike hwnd>`. Chromium does the same on its own: a new browser asks `FocusHandler::on_set_focus` (source navigation) and then calls `SetFocus` on its HWND. One run of mine on the user's desktop (`runs/wgc-user`) hit this: `SAFETY: foreground window 0xf1032 belongs to the spike` at 0.77 s, caused by Chromium's own focus, before `on_set_focus` refused it.
- GPUI's own `PlatformWindow::activate()` (`window.rs:815-865`) calls `SendInput` (Alt down/up) and `SetForegroundWindow`. `Window::resize` uses `SetWindowPos` without `SWP_NOACTIVATE` (`window.rs:664`). `toggle_fullscreen` moves the window onto the monitor. Chrome-native windows (DevTools, Ctrl+N) activate themselves.

So the spike runs on a desktop that is never switched to. `src/bin/deskrun.rs` calls `CreateDesktopW("vsesvit-spike")` and starts the spike with `STARTUPINFO.lpDesktop = "WinSta0\vsesvit-spike"`. Windows there can be activated, focused, made fullscreen, and Chrome can open its own windows, and none of it reaches the desktop the user sees. On that desktop `GetForegroundWindow()` returns 0 for the spike (`runs/desk1`: `foreground before=0x0 after=0x0`).

Guards inside the spike (`src/win.rs`):

- `init_desktop_guard()` compares the thread desktop with `OpenInputDesktop`. Only when they differ is `may_activate()` true. `SPIKE_STRICT=1` forces the user-desktop policy.
- `set_focus()` refuses when `!may_activate()` and our toplevel is not already the active window.
- `page_focus_allowed` starts false on the user's desktop, so `on_set_focus` cancels Chromium's own focus.
- Fullscreen and GPUI `resize` are refused or replaced (`win::set_size` with `SWP_NOACTIVATE`).
- A 250 ms watchdog logs `SAFETY:` if the foreground window belongs to the spike, or (user desktop) if a spike toplevel is on-screen. `Check-Safety` in the scripts fails the run on that line.

Input never leaves the process. Keys and clicks are `SendMessage`/`PostMessage` to HWNDs this process owns, `SetKeyboardState` for this thread's modifier state, CEF host APIs and JS. Screenshots are `PrintWindow` of our own toplevel.

### 1.2 Driving the spike (VERIFIED)

`src/ctl.rs` listens on `127.0.0.1:47811` (`SPIKE_CTL_PORT`); `scripts/lib.ps1` `ctl "<line>"` sends one command. The log file (`SPIKE_LOG`) is the oracle. Commands that matter: `state`, `tree`, `tops`, `res`, `new URL`, `switch N`, `close N`, `nav URL`, `key NAME [ctrl] [shift] [alt] [to=page|chrome]` (SendMessage), `keypost ...` (PostMessage, through the queue like typed keys), `typepost TEXT`, `clickel ID` (page reports the element centre, the shell sends WM_LBUTTONDOWN/UP to the page HWND), `omnibox`, `page`, `menu region|plain|popup|off`, `popupfix`, `other`, `zorder`, `hittest X Y`, `shot PATH`, `shotn N`, `content`, `resize W H`, `bar`, `dpi`, `raf`, `stats`, `closewin`, `quit`.

Trap: `key` (SendMessage straight into the window procedure) hides the most important Windows bug in this setup. Only `keypost` meets GPUI's message loop. See §4.

---

## 2. Build (VERIFIED)

`Cargo.toml` (spike): `gpui = { package = "gpui-pre", version = "=0.3.9" }`, `gpui_platform = { package = "gpui-pre-platform", version = "=0.3.9", default-features = false }`, `gpui-component = { version = "=0.7.3", default-features = false }`, `cef = { version = "=154.5.0", default-features = false, features = ["resources"] }`, `windows-sys` 0.61 and `windows` 0.62 (WGC experiment only), `png`, `futures`, `anyhow`, `raw-window-handle`. Binaries: `vsesvit-win-spike` (browser), `vsesvit-win-spike-helper` (CEF sub-processes), `serve` (test pages), `deskrun` (private desktop). `[patch.crates-io] gpui-pre-windows = { path = "vendor/gpui-pre-windows" }`.

Toolchain: MSVC Rust plus CMake and Ninja. `cef-dll-sys` builds `libcef_dll_wrapper` with CMake/Ninja; Visual Studio 18's bundled CMake and Ninja work when put on `PATH` and `CMAKE` (`.audit/scratch/win-spike/env.ps1`).

CEF binaries: `CEF_PATH=.audit\scratch\win-spike\cef`. `cef-dll-sys`'s build script downloads the **minimal** distribution into `CEF_PATH\154.0.34\cef_windows_x86_64` when it is missing (`cef_binary_154.0.34+g14c5a08+chromium-154.0.8037.98_windows64_minimal.tar.bz2`, sha1 `be54bbaf…`). It then copies every runtime file next to the executables in the target dir (`copy_cef_runtime_files`): `libcef.dll`, `chrome_elf.dll`, `d3dcompiler_47.dll`, `dxcompiler.dll`, `dxil.dll`, `vk_swiftshader.dll`, `vk_swiftshader_icd.json`, `vulkan-1.dll`, `icudtl.dat`, `v8_context_snapshot.bin`, `resources.pak`, `chrome_100_percent.pak`, `chrome_200_percent.pak`, `locales\`, and also `bootstrap.exe`/`bootstrapc.exe`. It links `libcef.lib` plus 14 SDK import libs.

Build times on this PC (dev profile, dependencies at opt-level 2): clean build of everything into an empty target dir, with the CEF archive already in `CEF_PATH`, **3 min 54 s** (`env/build-clean.log`, includes the CMake/Ninja build of `libcef_dll_wrapper`); rebuild after touching the spike crate 15–36 s; small incremental changes 6–11 s.

## 3. Process model and sandbox (VERIFIED)

### 3.1 Helper executable or same executable

Both work. `Settings.browser_subprocess_path` = `vsesvit-win-spike-helper.exe` (`src/bin/helper.rs`: `api_hash`, `execute_process`, a `ChildApp` with no shell state) is the default; `SPIKE_SAME_EXE=1` leaves it empty, so the browser exe re-enters `entry::run` with `--type=...` and returns from `execute_process`. Both load `chrome://sandbox` and show five processes (GPU, network, storage, two renderers) as `Not Sandboxed` (`runs/sandbox-plain-helper`, `runs/sandbox-plain-same-exe`). The helper avoids loading GPUI code into every sub-process. Under the sandbox (3.2) there is no choice: every process is the same executable.

### 3.2 Sandbox: only through `bootstrap.exe` (VERIFIED)

CEF 154 on Windows no longer ships `cef_sandbox.lib`. The distribution has `bootstrap.exe` (GUI) and `bootstrapc.exe` (console) and `include/cef_sandbox_win.h`: the bootstrap, renamed to `<name>.exe`, loads `<name>.dll` from its directory and calls its export `RunWinMain(HINSTANCE, LPWSTR, int, void* sandbox_info, cef_version_info_t*)`. The `cef` crate's `bundle-cef-app` tool does the renaming when its `sandbox` feature is on (`build_util/win/mod.rs` `copy_app`).

The spike: `[lib] name = "winspike", crate-type = ["rlib", "cdylib"]`; `lib.rs` exports `RunWinMain`, which calls `entry::run(sandbox_info)`; `entry::run` passes that pointer to `execute_process` and `initialize` and sets `no_sandbox = 0`, empty `browser_subprocess_path`. `scripts/bundle-sandbox.ps1` lays out `.audit/scratch/win-spike/bundle/` (`bootstrap.exe` → `winspike.exe`, `winspike.dll`, the CEF runtime). No `cef`/`cef-dll-sys` `sandbox` feature was needed.

`runs/sandbox1` (bundle on the private desktop, URL `chrome://sandbox`, exit 0, `cef initialized ... sandbox=true`):

```
52552  GPU       GPU       Limited   S-1-16-4096 Low
43772  Utility   Network Service  Not Sandboxed
38248  Utility   Storage Service  Service  Lockdown  S-1-16-0 Untrusted
52448  Renderer            Renderer  Lockdown  S-1-16-0 Untrusted
46596  Renderer            Renderer  Lockdown  S-1-16-0 Untrusted
features: NetworkServiceSandbox enabled=false, RendererAppContainer enabled=false, ...
```

The network service runs unsandboxed because Chrome's `NetworkServiceSandbox` feature is off by default on Windows, as in Chrome. GPUI works inside the DLL (window, DirectComposition, ctl, shutdown).

What the real crate must do: build the shell as a `cdylib` with `RunWinMain` (and `RunConsoleMain` if a console build is wanted), ship `bootstrap.exe` renamed to the product name, and keep the helper-exe path only for development. INFERRED (not tried): a signed release must sign both the renamed bootstrap and the DLL; whether `bootstrap.exe` enforces matching signatures was not checked. Our DLL ignores `version_info`; the real one should compare it with its own `cef_version_info` and refuse to run on a mismatch.

---

## 4. Message loop (VERIFIED)

Two modes, chosen with `SPIKE_PUMP`:

- **external** (`external_message_pump = 1`, `src/pump.rs`, the Linux design): `on_schedule_message_pump_work` (any thread) feeds a futures channel; a GPUI foreground task calls `do_message_loop_work`, capped at 33 ms while active and 250 ms after 1 s idle. Chromium's HWNDs live on GPUI's main thread, and GPUI's `GetMessageW` loop dispatches their messages.
- **multi-threaded** (`multi_threaded_message_loop = 1`): CEF runs its own UI thread. Browser HWNDs belong to that thread (`runs/mt1`: `tid=48588 main_tid=46912`). They are children of our host HWND, so Windows attaches the two threads' input queues.

Measurements (`scripts/explore-pump.ps1`, output `runs/explore-pump.txt`; one page loaded, 3 s settle, 15 s, CPU summed over all six processes, hidden desktop, dev profile):

| variant | idle CPU (one core) | first load (create → load_end) | 4 navigations (ms) |
|---|---|---|---|
| external | 4.25 %, 4.14 % | 133, 127 ms | 23,17,16,17 / 21,16,18,17 |
| external, no idle back-off | 5.07 %, 3.83 % | 134, 130 ms | 21,17,24,18 / 19,19,18,18 |
| multi-threaded | 3.31 %, 3.73 % | 89, 101 ms | 14,13,12,14 / 16,13,14,12 |

Per process (`runs/cpu-break`): external browser 3.65 %, network 0.94 %, GPU 0.73 %; multi-threaded browser 1.88 %, the rest 0.00 %. Most browser-process idle cost follows window activation, not the pump: with the window inactive (`SPIKE_ACTIVATE=0`) the browser falls from 3.0–3.8 % to 1.9 % (`runs/cpu-base`, `cpu-nowatch`, `cpu-inactive`). The 250 ms watchdog costs nothing measurable. Pump latency: `latency_avg=4–6us`, `work_max` 42–47 ms (page load).

Mode-specific traps:

- **External pump: GPUI's message loop eats keys meant for Chromium.** `translate_accelerator` (`gpui-pre-windows` `platform.rs:502`) sends `WM_GPUI_KEYDOWN` to `msg.hwnd` for every `WM_KEYDOWN`/`WM_SYSKEYDOWN` on the thread and skips `TranslateMessage`/`DispatchMessage` when the result is 0. Chromium's window answers the unknown message with `DefWindowProc` = 0, so the key is dropped. VERIFIED (`runs/keys`): unpatched (`GPUI_SPIKE_UNPATCHED=1`), posted `x` and Ctrl+L never reach the page; `SendMessage`'d `y` does. Patched, all reach `on_pre_key_event` and the page. Patch: `vendor/gpui-windows-keys.patch` (one check, `is_gpui_window(msg.hwnd)` by class name `Zed::Window`). Multi-threaded mode does not need it (Chromium's windows are on another thread's queue), VERIFIED in `runs/mt1` with the patched build only (posted keys reach the page).
- **Multi-threaded: `cef::shutdown` right after the last `on_before_close` hangs.** VERIFIED `runs/mtq0`: log stops at `main: calling cef::shutdown`, 5 processes left. Waiting 200 ms first (`win::drain_messages`, `SPIKE_MT_DRAIN`, it dispatched 0 messages) makes it exit 0 in 108 ms (`runs/mtq1`). `runs/mt1` hung the same way before the fix. With the wait, every multi-threaded shutdown since exited 0 with no processes left: 6 of 6 in `runs/explore-shutdown.txt`, and check B3 in both self-test runs.

**Recommendation: the external pump on GPUI's thread, with the GPUI key patch.** It is the Linux design, so the pump, the glue and the threading contract are shared. CEF, Chromium's HWNDs and GPUI live on one thread: CEF calls from GPUI handlers are direct, `SetWindowPos`/`SetFocus` on the page never cross threads, and there is no shutdown race. Its cost is small: 0.5–1 % of a core more at idle than multi-threaded, and first loads about 35 ms slower (130 vs 95 ms), navigations 17 vs 13 ms. Multi-threaded is the fallback if the key patch cannot be carried. It then needs `on_ui` posting for every CEF call, cross-thread window calls that can deadlock against the CEF UI thread, and the shutdown wait.

Thread-affinity rules with the external pump: see §13.3.

---

## 5. Embedding (VERIFIED; scales other than 175 % INCONCLUSIVE)

Window tree (`runs/desk1`, `ctl tree`):

```
0x90ec8 "Zed::Window" "Vsesvit spike" ex=0x240100 (WS_EX_NOREDIRECTIONBITMAP|APPWINDOW|WINDOWEDGE)
  0x40dd6 "VsesvitSpikeHost" (ours, WS_CHILD|CLIPCHILDREN|CLIPSIBLINGS)
    0x160fa2 "Chrome_WidgetWin_1" (CEF browser, our thread)
      0x71008 "Chrome_RenderWidgetHostHWND" "Chrome Legacy Window"
      0xc0fde "Intermediate D3D Window" (GPU process)
```

**GPUI paints over child HWNDs.** `directx_renderer.rs:1017` creates the DirectComposition target with `CreateTargetForHwnd(hwnd, topmost = true)`. Its visuals compose above every child window. With an opaque GPUI window the page is invisible: `content: non_background=0.000` with PrintWindow (`runs/desk1`, `runs/smoke`) and with WGC (`runs/wgc-user`, screenshot shows only the content div). VERIFIED fix without patching GPUI: `WindowBackgroundAppearance::Transparent`, the gpui-component theme background set to transparent, and no background on the content element (`shell::hole_mode`, default on). Then `content: non_background=1.000` and the screenshot shows the page (`runs/hole1/pw.png`). The chrome bars paint their own backgrounds.

Consequence: anything GPUI draws over the content rect appears above the page with no cut-out (see §9). Mouse input still goes to the child HWND under the point.

`scripts/explore-embed.ps1`, output `runs/explore-embed.txt`, screenshots `runs/embed-*/`.

**Resize and layout (VERIFIED).** The content element is a GPUI `canvas`; its prepaint callback converts the bounds to physical px and `SetWindowPos`es the host (and the active child) only when the rect changed (`shell.rs` `apply_geometry`). Results with the host:

| step | host rect (px) | child rect | page `innerWidth×innerHeight` |
|---|---|---|---|
| 1280×800 client | 2232×1195 at (0,133) | 2232×1195 at (0,0) | 1276×683 |
| resize to 1000×650 | 1750×957 at (0,133) | 1750×957 | 1001×547 |
| 40 px bar inserted above the content | 1750×887 at (0,203) | 1750×887 | 1001×507 |

Trap (VERIFIED): a `SetWindowPos` on the GPUI window made from inside a GPUI update (here the ctl handler) is lost. `SetWindowPos` delivers `WM_SIZE` synchronously while GPUI's state is borrowed, and GPUI never re-lays out: with `SPIKE_RESIZE_INLINE=1` the window grew to 1774×1202 (`resized.png`) but the host stayed 2232×1195. GPUI's own `resize` spawns its `SetWindowPos` on the executor for this reason. The shell must do the same for every Win32 call that sends messages to the GPUI window.

**Host HWND or direct (VERIFIED both work).** `SPIKE_HOST=direct` parents CEF straight into the GPUI window at the content offset: same geometry, `page_visible=1.000`, and a `SetWindowRgn` hole on Chromium's own `Chrome_WidgetWin_1` routes the press to GPUI as well (`hittest ... -> Zed::Window`). Recommendation: keep the host. One region and one rect serve every tab (direct mode would need the region re-applied to each tab's HWND on every switch, and Chromium owns that window); hidden tabs and the pre-load background have a parent we control. No clipping problem was seen either way.

**HiDPI.** 175 % VERIFIED: GPUI `scale=1.75 dpi=168`; page `devicePixelRatio=1.75 inner=1276x683` for a content rect of 1275.4×682.9 DIP. Both sides read the monitor DPI per window (the process is per-monitor-v2: `awareness=2`). 100/150/200 % on a real monitor: INCONCLUSIVE. There is one monitor, and changing the display scale is the user's setting; to run it, set Settings > Display > Scale to 100/150/200 % and run `scripts\explore-embed.ps1`, then check `dpr=` against GPUI's `scale=`. What did run: `--force-device-scale-factor=1` gives `dpr=1 inner=2232x1195` and `=2` gives `dpr=2 inner=1116x598` in the same 2232×1195 px rect, so Chromium fills the physical rect at any scale. `SPIKE_DPI=unaware` could not be tried: `SetProcessDpiAwarenessContext` fails with access denied before any of our code creates a window (INFERRED: libcef.dll, implicitly linked, sets it first). WM_DPICHANGED (moving to another monitor) not tested.

---

## 6. Tabs, navigation, display events (VERIFIED)

Title, URL, loading state and favicon reach GPUI (`runs/desk1`): `cef-ev: address id=1 http://127.0.0.1:8765/page1.html`, `title id=1 "Page One"`, `loading id=1 loading=false`, `favicon id=1 ["http://127.0.0.1:8765/icon.png"]`; the tab strip shows `Page One ★`.

`scripts/explore-tabs.ps1 [-Pump ext|mt]`, output `runs/explore-tabs-ext.txt` and `runs/explore-tabs-mt.txt`:

- Three tabs. Each is its own `Chrome_WidgetWin_1` under the host. Switching hides the others (`ShowWindow(SW_HIDE)` + `was_hidden(1)`) and shows the active one (`SW_SHOWNA` + `was_hidden(0)`), then re-applies geometry and focus. The pages see it: `switch 0` logs `id=3 page: visibility hidden`, `id=1 page: visibility visible`; `state` shows exactly one `visible=true`.
- `close 1`: `close_browser(0)` → `on_before_close id=2` → the tab is removed, the neighbour activated, `tabs left 2`.
- 30 cycles of new tab (page2) → `load_end 200` → close → `tabs left 2`: 30/30 in both modes. Before/after, 4 s settle:
  - external: processes 7/7, `handles=1968 gdi=90 user=89 toplevels=14 windows_under_gpui=6` both times.
  - multi-threaded: processes 7/7, `handles=1943 gdi=90 user=86 toplevels=16 windows_under_gpui=6` both times.
- Quit afterwards: exit 0, no processes left.

---

## 7. Keyboard focus (VERIFIED)

GPUI on Windows has no `WM_SETFOCUS`/`WM_KILLFOCUS` handler; window activation is `WM_ACTIVATE` only (`events.rs:97`, `handle_activate_msg`). Focus moving into a child HWND of the same toplevel does not change the active window, so GPUI stays active. VERIFIED in every run: `state: ... gpui_active=true focus=<CEF child> active_window=<gpui hwnd> page_focused=true`.

VERIFIED (`runs/keys`, `runs/mt1`):
- Ctrl+L posted to the page: `on_pre_key_event ... vk=0x4c mods=0x4` → `action FocusOmnibox` → `SetFocus(gpui)` → `address input focused`; page logs `window blur`; `gpui_active=true omnibox_focused=true`.
- Typing after that (`typepost abc`) lands in the omnibox: `state: address="abc"`.
- Click into the page (`clickel text`): `got_focus` → `key focus Chrome -> Page`, `address input blurred`, page `window focus`.

The shell's focus model (`shell.rs` `KeyFocus`): a mouse-down that reaches GPUI is on the chrome (`capture_any_mouse_down` → `SetFocus(toplevel)`); `on_got_focus` → page; `on_take_focus` → omnibox; activation (`observe_window_activation`) re-applies the last owner, because `DefWindowProc` puts focus on the toplevel on `WM_ACTIVATE`.

`scripts/explore-focus.ps1 [-Pump ext|mt]`, output `runs/explore-focus-ext.txt`, `runs/explore-focus-mt.txt` (same results in both modes):

- Tab-out: 15 posted Tabs walk the page's focus chain; the 15th fires `take_focus id=1 next=true` → `key focus Page -> Chrome` → `address input focused`; `gpui_active=true omnibox_focused=true`.
- Click into the page, then Ctrl+L posted to the page: `got_focus` → Page; `action FocusOmnibox ... via on_pre_key_event` → `address input focused`.
- Tab with the omnibox focused moves GPUI focus off the input (`address input blurred`) and nowhere else: `key_focus=Chrome page_focused=false`. The real shell must make Tab/Shift+Tab at the ends of the chrome's focus order call `set_focus(1)` on the page (Chrome cycles toolbar → page).
- Ctrl+L with the chrome focused: GPUI's `ctrl-l` binding → `focus_omnibox`, no CEF involvement.

User-desktop policy (`SPIKE_STRICT=1` on the hidden desktop, then a navigation and a new tab): `may_activate=false`, `activation skipped`, every shell `SetFocus` refused, Chromium's `set_focus source=0` for both browsers refused by `on_set_focus`, final `gpui_active=false focus=0x0 active_window=0x0`. Nothing activates itself. This is the rule the real shell must follow at startup and for background tabs: never focus a page while our window is inactive, and refuse `on_set_focus` then.

---

## 8. Chrome accelerators inside the page (VERIFIED)

`scripts/explore-accels.ps1`, output `runs/explore-accels.txt`. One fresh spike per case; the key is posted to the page HWND. `SPIKE_KEYS`: `none` = no interception, `cmd` = `on_chrome_command` only, `key` = `on_pre_key_event` only, `both`.

| key | none | cmd | key / both |
|---|---|---|---|
| Ctrl+T | IDC_NEW_TAB, native window "New Tab - Chromium" | NewTab via on_chrome_command | NewTab via on_pre_key_event |
| Ctrl+N | IDC_NEW_WINDOW, native "New Tab - Chromium" | NewWindow | NewWindow |
| Ctrl+W | IDC_CLOSE_TAB, our tab closes (tabs=0) | CloseTab | CloseTab |
| Ctrl+Shift+T | nothing (no command fired) | nothing | ReopenTab |
| F11 | IDC_FULLSCREEN, native window | Fullscreen | Fullscreen |
| Ctrl+P | IDC_PRINT, no native toplevel within 2.5 s | Print | Print |
| Ctrl+F | IDC_FIND, no native toplevel | Find | Find |
| F12 | IDC_DEV_TOOLS_TOGGLE, native "DevTools - …" window, takes activation (`gpui_active=false`) and arrives as an external browser in `on_after_created` | DevTools | DevTools |

With `on_chrome_command` alone the page still sees the key first (`page_saw=[Control,p]`, `[F11]`, `[F12]`), and Ctrl+Shift+T never reaches it when Chrome has nothing to restore. With `on_pre_key_event` the page sees only the modifier keydown. Use both, `on_pre_key_event` first. The net matters: in the self-test, F12 posted to a fresh `about:blank` tab while the omnibox held focus skipped `on_pre_key_event` and arrived as `on_chrome_command ... IDC_DEV_TOOLS_TOGGLE` (`runs/st-ext`), which the shell then took.

---

## 9. Overlays above the page (VERIFIED)

`scripts/explore-overlays.ps1`, output `runs/explore-overlays.txt`, screenshots in `runs/overlay-ext/`. The menu is a 260×220 GPUI element at the top right of the content rect; probe points are logical client px; `hittest` reports both `ChildWindowFromPointEx` descent and the system's `WindowFromPoint`.

### 9.1 In-window overlay

| menu mode | colour at menu centre (PrintWindow) | `WindowFromPoint` at menu centre | at a page point |
|---|---|---|---|
| off | `#fafafa` (page) | `Chrome_RenderWidgetHostHWND` | `Chrome_RenderWidgetHostHWND` |
| plain (no region) | `#fff7d6` (menu) | `Chrome_RenderWidgetHostHWND` | `Chrome_RenderWidgetHostHWND` |
| region (hole in host) | `#fff7d6` (menu) | `Zed::Window` | `Chrome_RenderWidgetHostHWND` |

So on Windows the overlay is painted above the page for free (`menu-plain.png`), the opposite of X11. What the cut-out buys is input: without it a click on the menu lands in the page. With `SetWindowRgn(host, full − hole)` (`win::region_exclude`, `win: region on <host> hole PRect { x: 1742, y: -25, w: 483, h: 413 }`) the press reaches GPUI. The real crate needs the region for every interactive overlay over the content rect (menus, omnibox suggestions, permission chips), and none for passive ones (tooltips, toasts that ignore the mouse). INFERRED: the region also clips what Chromium paints, which does not matter here because GPUI paints the overlay pixels anyway.

### 9.2 `WindowKind::PopUp` window

GPUI creates it with `WS_EX_TOOLWINDOW | WS_EX_TOPMOST` and no owner (`window.rs:492`): `popup window ... ex=0x200188 owner=0x0`. Its own screenshot is `popup.png` (PrintWindow, 455×385). Z-order (0 = top of the desktop):

- popup open, main active: `popup=Some(0) (topmost=true) main=Some(10)`.
- another toplevel created and activated (`other`, a plain window of this process standing in for another app): `popup=Some(0) other=Some(10) main=Some(11) gpui_active=false`. The popup floats above the other app. That is the bug.
- `popupfix` (`SetWindowLongPtrW(GWLP_HWNDPARENT, main)` + `SetWindowPos(HWND_NOTOPMOST)`): `owner=0x1c0fc8 exstyle=0x200180 topmost=false`. Then `other` again: `popup=Some(8) other=Some(7) main=Some(9)`. The popup is below the other app and directly above its owner, as a menu should be.

The real crate must set the owner and drop TOPMOST right after GPUI creates the popup (or patch `window.rs` to use the owner from `WindowOptions`). `AnchoredPopup` is not available on Windows in this GPUI (`PopupNotSupportedError`), so popups are positioned in screen coordinates by the shell (`win::move_to`) as the spike does.

---

## 10. Popups, new windows, dialogs (VERIFIED)

`scripts/explore-dialogs.ps1`, output `runs/explore-dialogs.txt`; one spike per case, the element is clicked with WM_LBUTTONDOWN/UP (user gesture). `SPIKE_DIALOGS=default` leaves CEF's handling; `shell` handles in the spike.

- `window.open` and `target=_blank`: `on_before_popup ... disposition=3 gesture=1`; with `window_info.parent_window = host` and Chrome runtime style the popup arrives as `tab 1 <- browser id=2 popup=true opener=1 parent=<host> (ours=true)` on our thread. `window.open` returns a window object.
- JS `alert`/`confirm`/`prompt`: `on_jsdialog` is called in both modes. Default: no new toplevel (Chrome draws a tab-modal dialog in the browser view). Shell: callback answers; page logs `alert returned`, `confirm -> true`, `prompt -> spike-answer`.
- File chooser: `on_file_dialog mode=0`. Default: a native `#32770 "Open"` dialog. Shell: page logs `file -> picked.txt`.
- Download: `can_download` then `on_before_download`. Default saves to the user's Downloads folder (two of my runs wrote `C:\Users\mrqua\Downloads\spike-download.txt`; deleted each time) and shows a native toplevel "Recent download history". Shell: saved under the run's root, **but the "Recent download history" bubble still appears**. Suppressing it: NOT VERIFIED. `SPIKE_DISABLE_FEATURES=DownloadBubble,DownloadBubbleV2` changes nothing (`runs/dlg-nobubble-shell-dl`). Setting `download_bubble.partial_view_enabled=false` and `download.default_directory` with `RequestContext::set_preference` in `on_context_initialized` (`SPIKE_PREFS`) returned 0 with an empty error; the bubble stayed, and default mode still saved into the user's Downloads folder (`runs/dlg-prefs-*`). The real crate must always handle `on_before_download` itself and find where Chrome style accepts profile prefs.
- Geolocation and notifications: `on_show_permission_prompt perms=0x100 / 0x8000`. Default: native toplevel `"http://127.0.0.1:8765 wants to"`. Shell: accepted, `on_dismiss_permission_prompt result=0`, page `notification -> granted`, geolocation succeeds (the page logs no coordinates).

Chrome-native toplevels present in every run, hidden: `Chrome_WidgetWin_0` 2880×1523, `Chrome_StatusTrayWindow`, `Base_PowerMessageWindow`, a 23×40 `Chrome_WidgetWin_1` after popups.

---

## 11. Screenshots from inside the process (VERIFIED)

`PrintWindow(gpui_hwnd, PW_RENDERFULLCONTENT)` returns GPUI's DirectComposition content and the page (GPU-process child included) once the hole exists: `runs/hole1/pw.png`, 2256×1440 in 19.0 ms; `runs/mt1` 20.3 ms. Windows.Graphics.Capture (`src/wgc.rs`) works on the user's desktop (`runs/wgc-user`, 86 ms including device setup) but fails on a non-input desktop (`capture session: 0x8007139F`, `runs/hole1`). Recommendation: PrintWindow. Repeated timing in the self-test (`shotn 5`): `ok=5 avg=19.9ms max=21.3ms`.

---

## 12. Clean shutdown (VERIFIED)

Sequence (`shell.rs` `begin_quit` / `finish_quit`, `entry.rs`): close every browser with `close_browser(1)`; on the last `on_before_close` stop the pump task and `cx.quit()`; after `run()` returns, pump CEF for 200 ms (external) or wait 200 ms (multi-threaded, §4), drop our CEF references, `cef::shutdown()` on the main thread. The window close button path is GPUI's `on_window_should_close`: it returns false while tabs exist and starts `begin_quit`, so the window stays until the browsers are gone.

`scripts/explore-shutdown.ps1`, output `runs/explore-shutdown.txt`. Two tabs open; `ctl quit` (shell action) or `ctl closewin` (`PostMessageW(gpui_hwnd, WM_CLOSE)`); 3 runs per cell:

| pump | trigger | exit code | command → process exit | `cef::shutdown` | processes left |
|---|---|---|---|---|---|
| external | quit | 0, 0, 0 | 399, 389, 385 ms | 94, 88, 95 ms | 0 |
| external | WM_CLOSE | 0, 0, 0 | 386, 408, 393 ms | 93, 114, 96 ms | 0 |
| multi-threaded | quit | 0, 0, 0 | 388, 419, 377 ms | 91, 129, 89 ms | 0 |
| multi-threaded | WM_CLOSE | 0, 0, 0 | 388, 395, 384 ms | 94, 93, 93 ms | 0 |

Each run logs `on_before_close` twice. The exit code is the browser's, passed through `deskrun`.

---

## 13. Platform seam: Linux X11 and Windows in one shell (INFERRED design)

What the two spikes share: the CEF glue (`diff` of `spikes/core/src/cefglue/client.rs` against this spike's: the native handle conversion, popup `style`/`ex_style` in `on_before_popup`, the `os_event` type `MSG` vs `XEvent`, one modifier cast, and the download path; `mod.rs` adds `NativeHandle`, `PumpMode` and `page_focus_allowed`), the `CefEvent` channel and `Shared`, the shortcut table and IDC map, the tab model, the `KeyFocus` model, the GPUI views, the shutdown sequence, and the pump's polling policy (`pump.rs` is the same file minus the GLib watcher). What differs is small and native:

| concern | X11 (spike-core) | Windows (this spike) |
|---|---|---|
| content host | 24-bit InputOutput window (BadMatch otherwise) | plain child HWND (optional, recommended) |
| GPUI paint under the page | irrelevant (children are above) | must be transparent (DirectComposition is topmost) |
| geometry, show/hide | `ConfigureWindow`, map/unmap | `SetWindowPos`, `ShowWindow(SW_HIDE/SW_SHOWNA)`; deferred out of GPUI updates |
| input holes for overlays | SHAPE (also needed to paint) | `SetWindowRgn` on the host (input only; paint is free) |
| focus | patched GPUI focus proxy, `XSetInputFocus` | `SetFocus`, refused while our toplevel is inactive |
| PopUp windows | works | owner + clear TOPMOST after creation |
| pump extras | GLib fd watcher thread | none; GPUI's loop dispatches Chromium's messages (needs the key patch) |
| screenshot | `GetImage` of the toplevel | `PrintWindow(PW_RENDERFULLCONTENT)` |
| sandbox | namespaces + seccomp, separate helper | `bootstrap.exe` + `RunWinMain` DLL, single image |
| GPUI patch | `gpui-pre-linux` focus patch | `gpui-pre-windows` key patch |

### 13.1 Crates and modules

```
crates/vsesvit-cef/                 platform-neutral CEF glue (never `use gpui::*`)
  app.rs client.rs handlers/* shortcuts.rs events.rs (CefEvent, Shared) pump_policy.rs
  native.rs                         NativeHandle (u64), window_info(parent, rect_dip) per OS (cfg)
crates/vsesvit-shell/               GPUI shell, one crate for both OSes
  shell/                            BrowserWindow, TabStrip, Toolbar/Omnibox, ContentArea, OverlayManager, FocusModel
  platform/mod.rs                   trait NativeSurface, trait Pump, fn entry()
  platform/x11/                     XSurface (x11rb), GlibPump
  platform/win/                     WinSurface (windows-sys), WinPump, RunWinMain export, bootstrap bundling
crates/vsesvit-cef-helper/          Linux helper binary (Windows uses the single bootstrap image)
vendor/gpui-pre-linux, vendor/gpui-pre-windows (+ patches, each per-target [patch.crates-io])
```

### 13.2 The trait

```rust
/// One per GPUI browser window. All methods run on GPUI's main thread, outside GPUI updates
/// when they send messages to the GPUI window (the shell calls them from a spawned task).
pub trait NativeSurface: Sized + 'static {
    fn attach(window: &gpui::Window) -> anyhow::Result<Self>;        // create the content host
    fn parent_for_cef(&self) -> NativeHandle;                          // host XID / HWND
    /// Whether GPUI must leave the content rect transparent (Windows: yes).
    const NEEDS_TRANSPARENT_CONTENT: bool;
    fn set_content_rect(&mut self, rect: PhysRect);                    // host + active child
    fn show_page(&mut self, page: NativeHandle, visible: bool);        // plus was_hidden in the glue
    fn set_input_holes(&mut self, holes: &[PhysRect]);                 // SHAPE / SetWindowRgn
    fn focus(&mut self, target: FocusTarget) -> FocusOutcome;          // Chrome | Page(handle); Refused if inactive
    fn adopt_popup(&mut self, popup: &gpui::Window);                   // transient-for / owner + no TOPMOST
    fn capture(&self) -> anyhow::Result<RgbaImage>;                    // whole window incl. page
    fn toplevel_at(screen: PhysPoint) -> Option<NativeHandle>;         // tab drag-out target
}

pub trait Pump {
    fn start(cx: &mut gpui::App, wake: UnboundedReceiver<i64>) -> gpui::Task<()>;
    fn before_shutdown(&self);                                         // final pump / drain
}
```

The shell is generic, `BrowserWindow<S: NativeSurface>`, with `S` chosen by `cfg` in one place. `FocusOutcome::Refused` feeds `Shared.page_focus_allowed` so `on_set_focus` refuses too.

### 13.3 Threads on Windows (external pump)

| Thread | Runs |
|---|---|
| main | GPUI's `GetMessageW` loop, CEF's UI thread inside `do_message_loop_work`, and every Chromium browser HWND (`tid` = `main_tid` in every log) |
| GPUI dispatcher/timer threads | wake the pump task |
| CEF IO and other threads | handlers that only touch `Arc<Shared>` and send events |

Rules the shell must follow:

1. Call CEF only from the main thread (it is the CEF UI thread); `on_ui` posts when not.
2. Never make a Win32 call that sends messages to the GPUI window (SetWindowPos on it, ShowWindow, DestroyWindow) from inside a GPUI update; spawn it (§5 resize trap).
3. Never SetFocus, ShowWindow with activation, or `Window::activate_window` while our toplevel is inactive, and refuse `on_set_focus` then (§1.1, §7). GPUI's `activate_window` uses `SendInput`; do not call it.
4. Keep the GPUI key patch (§4) or every key typed into the page is lost.

---

## 14. What the real crate must carry on Windows

1. **The GPUI key patch** (`vendor/gpui-windows-keys.patch`, one `is_gpui_window` check in `translate_accelerator`) whenever CEF's windows share GPUI's thread (external pump). Without it, every key typed into a page is dropped (§4). Upstream it; it is a plain bug for any foreign child window.
2. **A transparent content rect.** `WindowBackgroundAppearance::Transparent`, no theme background behind the content area, nothing painted over the page rect except deliberate overlays. GPUI's DirectComposition target is topmost; anything opaque there hides the page (§5).
3. **A content host HWND** per GPUI window, parent of every tab's CEF child, sized in physical px from the content element's prepaint; tabs switched with `ShowWindow(SW_HIDE/SW_SHOWNA)` + `was_hidden` (§5, §6).
4. **Deferred Win32 calls.** Any call that sends messages to the GPUI window (SetWindowPos on it, ShowWindow, DestroyWindow) runs from a spawned task, never inside a GPUI update (§5 resize trap).
5. **Focus discipline.** Never call `Window::activate_window` (GPUI's Windows implementation uses `SendInput` and `SetForegroundWindow`). Never `SetFocus` while our toplevel is inactive; keep `page_focus_allowed` false until the window is active, so `on_set_focus` refuses Chromium's own focus on new browsers and navigations (§1.1, §7). Tab/Shift+Tab at the ends of the chrome's focus order must hand focus back to the page.
6. **Keyboard interception with both hooks**: `on_pre_key_event` for the reserved shortcuts (the page never sees them), `on_chrome_command` as the net with default-deny for Chrome-native UI (§8). Chrome-created windows still arrive through `default_client`; adopt or close them.
7. **Input holes for interactive overlays**: `SetWindowRgn` on the host for menus, omnibox suggestions and anything clickable over the page (§9.1). Painting needs nothing.
8. **PopUp fix-up**: after GPUI creates a `WindowKind::PopUp`, set its owner to the browser window and clear TOPMOST (§9.2). Position it in screen coordinates; `AnchoredPopup` is unsupported on Windows.
9. **Handlers for every prompt** (JS dialogs, file chooser, downloads, permissions) drawn in GPUI. Always handle `on_before_download`: Chrome's default saves into the user's Downloads folder, and `set_preference("download.default_directory")` did not take in this spike. The "Recent download history" toplevel still appears with the handler in place; that needs a fix or an accepted exception (§10).
10. **Sandbox packaging**: the shell as a `cdylib` exporting `RunWinMain`, shipped with CEF's `bootstrap.exe` renamed to the product exe; one image for all processes (§3.2). Plain exe + helper only for development.
11. **Shutdown**: close all browsers, wait for every `on_before_close`, quit GPUI, pump 200 ms (external) or wait 200 ms (multi-threaded; without it `cef::shutdown` hangs), then `cef::shutdown` on the main thread (§4, §12).
12. **Runtime files next to the exe** as `cef-dll-sys` lays them out (§2), and `--disable-features=CalculateNativeWinOcclusion` only for tests (the spike needs it off-screen; a real window should keep Chromium's occlusion tracking).
13. **Screenshots** with `PrintWindow(PW_RENDERFULLCONTENT)` of the toplevel (§11).

Not tested here: IME in the page, drag and drop, context menus, DevTools as a child, WM_DPICHANGED / multi-monitor, 100/150/200 % on a real display, release-build CPU, real mouse input on the user's desktop (all input here was posted messages), and the user's own desktop beyond one accidental run (§1.1).

---

## Appendix: evidence

All under `.audit/scratch/win-spike/runs/`: `desk1`, `hole1`, `keys`, `mt1`, `mtq0`, `mtq1`, `wgc-user`, `wgc-hidden`, `sandbox1`, `sandbox-plain-*`, `cpu-*`, `pump-*`, `acc-*`, `dlg-*`, `tabs-*`, `overlay-ext`, `focus-*`, `embed-*`, `down-*`, `st-*` (self-test), and the summaries `explore-pump.txt`, `explore-accels.txt`, `explore-dialogs.txt`, `explore-tabs-ext.txt`, `explore-tabs-mt.txt`, `explore-overlays.txt`, `explore-focus-ext.txt`, `explore-focus-mt.txt`, `explore-embed.txt`, `explore-shutdown.txt`, `selftest.txt`. Build logs: `.audit/scratch/win-spike/env/build*.log`. Earlier runs by the previous agent: `smoke`, `smoke2`, `nodcomp`, `fg`.
