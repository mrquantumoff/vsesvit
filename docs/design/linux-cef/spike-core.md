# spike-core: GPUI (X11) + Chrome-style CEF children: the foundation of the new Linux shell

Date: 2026-10-16. Spike code: `/tmp/vsesvit-scratch/spike` (builds cleanly; README there lists how to run it).
Versions: `cef` 154.5.0+154.0.34 (CEF 154.0.34, Chromium 154.0.8037.98), `gpui-pre` / `gpui-pre-platform` / `gpui-pre-linux` 0.3.9 (wgpu 29.0.4), `gpui-component` 0.7.3, `x11rb` 0.13.2, rustc 1.99.0, Fedora 44, KWin 6.7.5, Xwayland 24.1.14, RTX 5080.

Labels: **VERIFIED** = I ran it and saw the result (command and output given). **INFERRED** = my conclusion, not run.

---

## 0. Summary

Everything the task asked for works on this machine, with three non-obvious pieces that the real crate must carry:

1. **A 24-bit "content host" X window between GPUI and CEF.** GPUI's toplevel uses a 32-bit ARGB visual. Chromium creates its child window with `CopyFromParent` visual/colormap/border, so `CreateWindow` fails with `BadMatch` under GPUI's window and no page ever appears. Parenting CEF into a 24-bit `InputOutput` window that we create ourselves (root visual, default colormap, `border_pixel` 0) fixes it. (VERIFIED, §5.)
2. **A small vendored patch to GPUI's X11 backend** (`vendor/gpui-x11-focus.patch`, about 80 lines in `client.rs`):
   - it creates an InputOnly **keyboard-focus proxy** child in every GPUI toplevel and maps its key events to the toplevel;
   - it ignores `FocusIn`/`FocusOut` with detail `Inferior`, so the window does not go "inactive" when focus moves into the page.
   Without it, keys typed into the omnibox go to the page whenever the mouse pointer rests over the page (X delivers keys to the descendant under the pointer), and GPUI flips to inactive every time the page is clicked. (VERIFIED, §7.)
3. **An external message pump that polls.** CEF's `MessagePumpExternal` drops the next delayed-work time when its 10 ms slice ends, so `on_schedule_message_pump_work` alone stalls page loads (VERIFIED: 2 requests in 48 s, page never loads). Pump at ≤ 33 ms while active (as CEF's own sample does) and back off to 250 ms after 1 s of inactivity. Also watch the GLib default context's fds from a helper thread, or X input to the page waits for the next tick. Idle cost with one page loaded is **0.20–0.27 % of one core** for the browser process and **0.47 %** for all processes (debug build). Key latency is **3–5 ms**. (VERIFIED, §4.)

| # | Item | Result |
|---|---|---|
| 1 | Build | VERIFIED. Exact `Cargo.toml` in §2. Cold dependency build 1 min 14 s; crate rebuild 3.8 s; incremental 1.0 s |
| 2 | Process model, sandbox | VERIFIED. Separate helper executable (recommended) and same-executable both work. Namespace sandbox + seccomp-BPF without setuid `chrome-sandbox` (`chrome://sandbox`: "adequately sandboxed") |
| 3 | Pump on GPUI's main thread | VERIFIED. Polling is required (see 3 above). Idle 0.47 % total; key latency 3–5 ms with the GLib watcher, 9–196 ms without |
| 4 | Embedding, resize, HiDPI | VERIFIED. Chrome-style child (`runtime_style` 1) at the content rect. Follows window resize and layout changes. Scale 1, 1.5 and 2 are crisp (`devicePixelRatio` 1/1.5/2) |
| 5 | Tabs, navigation, display events | VERIFIED. 3+ tabs, switch with map/unmap + `was_hidden`, close with `on_before_close`. 30 open/close cycles leaked no processes, fds or X windows. Title/URL/loading/favicon reach GPUI |
| 6 | Keyboard focus | VERIFIED. Click page → page, click chrome → GPUI, Ctrl+L / Tab-out → omnibox, no key leaks, window stays active. Needs the GPUI patch |
| 7 | Chrome accelerators | VERIFIED. Without interception Ctrl+T/Ctrl+N open native Chrome windows, F12 opens native DevTools, Ctrl+P/Ctrl+F show Chrome UI inside the page. `on_pre_key_event` + `on_chrome_command` route all of them to the shell |
| 8 | Overlays above the page | VERIFIED. Both work: SHAPE cut-out of the host window (in-window GPUI overlay), and a `WindowKind::PopUp` GPUI window. Screenshots in §9 |
| 9 | Popups, dialogs | VERIFIED. `window.open`/`target=_blank` become child tabs (opener kept). JS dialogs, file chooser, downloads and permission prompts all reach their CEF handlers in Chrome style. Default Chrome UI catalogued in §10 |
| 10 | In-process screenshot | VERIFIED. One `GetImage` of the GPUI toplevel includes the CEF page on Xwayland: 5–9 ms for 1280×800 |
| 11 | Clean shutdown | VERIFIED. From the shell action and from the window close button: every `on_before_close`, GPUI exits, `cef::shutdown` (29 ms), exit 0, no processes left |

`selftest.sh` runs 19 end-to-end checks covering items 4–11. Last run: **19 passed, 0 failed**.

---

## 1. How the tests were run (and traps found on the way)

**No Xvfb on this host.** I did not use the owner's live desktop (`:0`) for input injection. I used a **private headless KWin**: `kwin_wayland --virtual --xwayland` gives rootless Xwayland `:1`. It runs inside its own `dbus-run-session` (script `start-display.sh`). This keeps KWin's real activation and focus behaviour, which matters for §7. (VERIFIED)

**Traps in that setup, all VERIFIED:**

- **XTEST does not work there.** Xwayland forwards XTEST to the compositor through libei. Under the virtual backend, pointer motion (absolute or relative) never moves the pointer, which stays at 800,500. The KWin log shows `[xwayland ei] Unhandled event EI_EVENT_SYNC`.
  - Instead I wrote `tools/kwin-input`, a Wayland client using `org_kde_kwin_fake_input`. KWin needs `KWIN_WAYLAND_NO_PERMISSION_CHECKS=1` to offer that interface. Through it, clicks and keys go through KWin exactly like real input.
- **A private D-Bus session leaks the owner's display into the services it activates.** The daemon's environment is passed on to activated services, and the first version of the script still carried `WAYLAND_DISPLAY=wayland-0` and `DISPLAY=:0`. So xdg-desktop-portal-kde, kwalletd6, ksecretd and kioworkers came up bound to the **owner's** desktop.
  - The portal file chooser opened by a test page therefore ran against the owner's display. I killed all of those processes and fixed the script: `env DISPLAY=:1 WAYLAND_DISPLAY=wayland-vscore dbus-run-session …`.
  - **Lesson for the real self-test harness:** sanitize the environment of the private bus, not just of the app.
- **Chrome's default download directory is `$HOME/Downloads`.** A default-mode download test wrote `spike-download.txt` into `/home/dy/Downloads`; I deleted it. `run.sh` now sets `HOME` to a scratch directory.

---

## 2. Build (VERIFIED)

`/tmp/vsesvit-scratch/spike/Cargo.toml`:

```toml
[package]
name = "vsesvit-spike"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
gpui = { package = "gpui-pre", version = "=0.3.9" }
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.9", default-features = false, features = ["x11"] }
gpui-component = { version = "=0.7.3", default-features = false }
cef = { version = "=154.5.0", default-features = false, features = ["resources"] }
x11rb = { version = "0.13", features = ["shape", "xtest", "resource_manager", "allow-unsafe-code"] }
raw-window-handle = "0.6"
futures = "0.3"
png = "0.17"
anyhow = "1"
libc = "0.2"

[profile.dev]
opt-level = 1
[profile.dev.package."*"]
opt-level = 2

[patch.crates-io]
gpui-pre-linux = { path = "vendor/gpui-pre-linux" }   # focus patch, §7
```

`build.rs`: `println!("cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");`. `readelf -d` shows `RUNPATH [$ORIGIN]` on both binaries.

**Build notes:**

- `gpui_platform` with only `features = ["x11"]` compiles the Wayland backend out entirely. `gpui_platform::linux(gpui::WindowingModes::X11)` additionally refuses Wayland at run time.
- gpui-component 0.7.3 builds cleanly next to it, without warnings.
- The X11 backend's raw-window-handle gives `RawWindowHandle::Xcb(h)` with `h.window` the toplevel XID.
  - Call the trait explicitly: `raw_window_handle::HasWindowHandle::window_handle(window)`. `Window::window_handle()` is also an inherent GPUI method that returns an `AnyWindowHandle`.
- CEF from `CEF_PATH=/home/dy/.cache/vsesvit-cef`; no CMake needed.
- The vendored `gpui-pre-linux` emits two pre-existing dead-code warnings. Our crate has none.

**Timings on 32 cores (`CARGO_TARGET_DIR=/tmp/vsesvit-scratch/target-core`):**

| Build | Time |
|---|---|
| First build, all dependencies, CEF already downloaded | 1 min 14 s wall, 1638 s CPU |
| `cargo clean -p vsesvit-spike && cargo build` (3 binaries) | 3.75 s |
| Incremental (`touch src/shell.rs`) | 1.04 s |

**Sizes:**

- Target directory: 9.3 GB. That is 1.5 GB of CEF runtime copied by the build script, plus debug info.
- Main binary: 968 MB file, 45.5 MB `.text`. Helper binary: 620 MB file, **2.5 MB `.text`**. The difference is debug info; use `debug = "line-tables-only"` or split debuginfo in the real crate.
- Helper `NEEDED`: only `libcef.so`, libc, libgcc_s. No xcb, xkbcommon or glib, and **0 GPUI symbols** (`nm | grep gpui` = 0).

---

## 3. Process model, settings, sandbox

### 3.1 Choice: separate helper executable (VERIFIED working; both modes tested)

`src/bin/vsesvit-spike-helper.rs` (complete):

```rust
use vsesvit_spike::cefglue::app::ChildApp;
fn main() -> std::process::ExitCode {
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();
    let mut app = ChildApp::new();
    let code = cef::execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());
    std::process::ExitCode::from(code.clamp(0, 255) as u8)
}
```

**How the browser process starts** (`src/main.rs`):

1. `api_hash`.
2. `execute_process` with `ChildApp`. This is still needed for `SPIKE_SAME_EXE=1`, and returns -1 in the browser process.
3. Decide the scale factor (§5.4).
4. `cef::initialize`, **before** GPUI creates its dispatcher threads.
5. `gpui_platform::linux(WindowingModes::X11).run(…)`.
6. After `run()` returns: `cef::shutdown()`.

The App is split in two. `BrowserApp` (command line + cached `BrowserProcessHandler`) goes to `initialize`. `ChildApp`, which has no shell state, goes to `execute_process`. Different App instances for the two calls work fine.

**Processes seen** (`ps`, VERIFIED, helper mode, one page loaded):

| Process | Role, as identified from thread names and parentage | RSS (debug) | Seccomp |
|---|---|---|---|
| `vsesvit-spike` | browser process; GPUI + CEF UI thread on the main thread; `libgtk-3` mapped (GtkUi on) | 428 MB | 0 |
| `helper --type=zygote --no-zygote-sandbox` | unsandboxed zygote | 75 MB | 0 |
| `helper --type=zygote` (pid 1 in new user/pid/net namespaces) | sandboxed zygote, plus its init process | 75 MB / 20 MB | 0 |
| child of the unsandboxed zygote, `VizCompositorThread` | GPU process | 250 MB | 2 |
| child of the GPU process | probably the GPU sandbox's broker (INFERRED) | 40 MB | 2 |
| children of the sandboxed zygote, `Compositor` thread | renderers; one more is probably a sandboxed utility (INFERRED) | 77–127 MB | 2 |
| `helper --type=utility --utility-sub-type=network.mojom.NetworkService` | network service | 110 MB | 0 |

Forked children keep the zygote's `argv` in `/proc/*/cmdline`. Identify them by thread names (`CrRendererMain`, `VizCompositorThread`).

**Same-executable mode** (`SPIKE_SAME_EXE=1`, empty `browser_subprocess_path`) also works: same process tree, zygotes 79 MB instead of 75 MB. (VERIFIED)

**Why the helper anyway:**

- The sub-process image is 2.5 MB of code instead of 45 MB, and no GPUI/X11/Vulkan crates live in renderers.
- Nothing runs before `execute_process`.
- Test binaries can create browsers by pointing `browser_subprocess_path` at the helper.

### 3.2 Settings and switches (VERIFIED in use)

```rust
let settings = cef::Settings {
    no_sandbox: 0,
    external_message_pump: 1,
    multi_threaded_message_loop: 0,          // keeps GtkUi (IME, GTK dialogs) alive
    browser_subprocess_path: CefString::from(helper.to_str().unwrap()),
    root_cache_path: CefString::from(root.as_str()),     // /tmp/vsesvit-scratch/core-env/cef-root-<pid>
    cache_path: CefString::from(format!("{root}/Default").as_str()),
    log_file: CefString::from(format!("{root}/cef.log").as_str()),
    log_severity: cef::LogSeverity::WARNING,
    ..Default::default()
};
```

Switches come from `App::on_before_command_line_processing`, browser process only, i.e. when `process_type` is empty (`src/cefglue/app.rs`):

- `--ozone-platform=x11`. Required: CEF 154 otherwise picks Wayland in a Wayland session.
- `--password-store=basic`. On KDE, Chromium otherwise uses KWallet for OSCrypt. In a session where the wallet does not answer, **every network request stalls** while `data:` URLs still load (VERIFIED: `org.kde.kwalletd6` was activated, no HTTP request reached the server). The real browser must choose between libsecret/KWallet and basic deliberately, and must not block startup on it.
- `--force-device-scale-factor=<s>`, so Chromium uses GPUI's scale (§5.4).

**Notes:**

- CEF itself adds `--disable-features=GlicActorUi,AutofillActorMode,LensOverlay,KillOnInvalidNavigationHeaders` (seen on `chrome://gpu`). Append to that list rather than replacing it.
- `on_before_child_process_launch` sees `zygote`, `gpu-process`, `utility` and `renderer` launches.

### 3.3 Sandbox (VERIFIED)

`no_sandbox: 0` with the build script's non-setuid `chrome-sandbox` (`-rwxr-xr-x dy dy`). `chrome://sandbox` in a tab reports:

- Layer 1 Sandbox: **Namespace**;
- PID namespaces: Yes; Network namespaces: Yes;
- Seccomp-BPF: Yes, with TSYNC: Yes;
- "You are adequately sandboxed."

Yama ptrace protection shows No; that is a host setting. `/proc/<pid>/ns/{user,pid,net}` of the renderers differ from the browser's, and `Seccomp: 2`.

`chrome://gpu`: compositing, rasterization, WebGL and WebGPU are hardware accelerated. Vulkan is disabled (Chromium default).

**Startup** (VERIFIED, from the log; debug build): `cef::initialize` takes 76–95 ms. The GPUI window exists after about 0.3 s, `on_after_created` follows 10–20 ms after `create_browser`, and a local page's title arrives about 0.4 s after process start.

---

## 4. Message pump on GPUI's main thread

### 4.1 Design (`src/pump.rs`)

**Pieces** (VERIFIED working):

- `BrowserProcessHandler::on_schedule_message_pump_work(delay_ms)` runs on **any thread** and does `shared.pump.unbounded_send(delay_ms)`, a `futures::channel::mpsc` sender. Sending wakes the GPUI foreground task that awaits the receiver; GPUI's dispatcher pings the calloop main loop.
- **One** GPUI foreground task, `cx.spawn(...).detach()`, is the only caller of `cef::do_message_loop_work()`, so the call is never re-entered. It runs outside any `cx.update`, which means CEF callbacks fired inside it never re-enter GPUI's `RefCell`.
- Every iteration first services the **GLib default context** by hand: `g_main_context_acquire` → `prepare` → `query` → `poll(…, 0)` → `check` → `dispatch`, repeated while sources are ready, then `release`.
- A **`glib-fd-watch` thread** `poll(2)`s the fds that `g_main_context_query` returned. When one is readable it sends a wake into the same task, then stays disarmed until the main thread has iterated GLib and re-armed it with the fresh fd set (eventfd).
- **GLib's own timeout is unusable.** Chromium's `MessagePumpGlib` work source returns timeout 0 from `prepare` when `state_` is null, and under `MessagePumpExternal` it always is (`message_pump_glib.cc` `HandlePrepare`). A slow GLib tick (250 ms) covers pure GLib timers instead: GTK dialogs, IME.

**Loop core, excerpt:**

```rust
loop {
    let wait = min(glib_due, cef_due) - now;
    futures::select_biased! {
        msg = rx.next() => { last_activity = now(); cef_due = Some(now + min(delay, cap)); /* drain: latest wins */ }
        _ = wake_rx.next() => { last_activity = now(); }      // GLib fd readable (X11 events for Chromium)
        _ = executor.timer(wait).fuse() => {}
    }
    let fds = glib_service(&stats);                         // dispatch Chromium's X11/GDK/D-Bus sources
    if cef_due <= now {
        let idle = last_activity.elapsed() > 1s;
        cef_due = Some(now + if idle { 250ms } else { 33ms }); // see 4.2: polling is required
        cef::do_message_loop_work();
    }
    watcher.arm(fds);
}
```

### 4.2 Why `on_schedule_message_pump_work` alone is not enough (VERIFIED + source)

- **With pure on-demand pumping** (`SPIKE_PUMP_MAX_MS=0`): `schedule_requests=2 cef_work=2` after 48 s. The page stayed "Loading…" and the CEF child stayed black.
- **With a 33 ms cap:** example.org reaches `load_end` about 0.4 s after `create_browser`. While idle, CEF itself requested only **3–5 schedules per 15 s**; the cap does the rest.
- **The cause**, from `libcef/browser/browser_message_loop.cc`: `MessagePumpExternal::Run` loops `DoWork` for at most a 10 ms slice and then returns. It discards the `delayed_run_time` that `DoWork` returned. `ScheduleDelayedWork` is only called when a *new*, earlier delayed task is posted, so the pending one is never scheduled.
- CEF's own `main_message_loop_external_pump.cc` re-arms a 1000/30 ms timer after every `DoWork` for exactly this reason. The adaptive back-off (33 ms within 1 s of activity, otherwise 250 ms) is my compromise between that and idle CPU.

### 4.3 Measurements (VERIFIED)

**Method:**

- example.org loaded; wait 8 s, then measure 15 s;
- per-process `utime+stime` delta from `/proc/<pid>/stat`, i.e. what `top` shows (`ps -o %cpu` is a lifetime average);
- debug build, one tab (`cpu-test.sh`).

| Pump config | Browser process | All processes | `do_message_loop_work`/s |
|---|---|---|---|
| cap 33 ms (CEF-sample behaviour) | 0.80 %, 0.47 % (2 runs) | 1.13 %, 0.67 % | 31 |
| cap 100 ms | 0.47 % | 0.80 % | 10.5 |
| cap 1000 ms | 0.20 % | 0.53 % | 1.2 |
| **adaptive 33 / 250 ms (default)** | **0.20 %, 0.20 %, 0.27 %** (3 runs) | **0.53 %, 0.47 %, 0.47 %** | 6.7 |

- The GPU process accounts for a constant ~0.33 % in every configuration, so it does not depend on the pump.
- Resolution is 1 tick = 0.067 %.

**Responsiveness:**

- Latency from a pump request to `do_message_loop_work`: average 5–8 µs, maximum 40–440 µs. One `do_message_loop_work` takes 180–310 µs on average; the worst case is about 20 ms, during startup.
- **Key press after 1.5 s idle → `on_pre_key_event`** (`latency-test.sh`, 6 samples each):

| Config | Latency |
|---|---|
| With the GLib fd watcher | **3, 5, 3, 4, 3, 4 ms**. This includes kwin-input → KWin → Xwayland → Chromium |
| `SPIKE_NO_GLIB_WATCH=1` | 35, 121, 25, 110, 196, 9 ms. The key waits for the next tick |

- Typing 10 characters into the page caused 26 watcher wakes. This shows Chromium's X11 events arrive through GLib sources on the default context.
- On the page, keystrokes echo back as `keydown` console messages 1–4 ms after `on_pre_key_event`.

---

## 5. Embedding the page

### 5.1 X window hierarchy (VERIFIED, `ctl tree`)

```
0xe00001  GPUI toplevel, depth 32 (ARGB), GPUI's XCB connection
├── 0xe00012  "gpui-focus-proxy", InputOnly 1x1 at -1,-1 (patched GPUI, §7)
└── 0x1400000 content host, depth 24, root visual    (shell's x11rb connection; created once per GPUI window)
    ├── 0x400003  CEF tab 1 (Chromium X11Window, depth 24, browser-process connection)
    │   └── 0xc00004  depth 32, created by the GPU process (presentation)
    └── 0x40000c  CEF tab 2 (unmapped while inactive)
```

### 5.2 The BadMatch problem and the host window (VERIFIED)

The first attempt used `set_as_child(<GPUI XID>, rect)`:

- `on_after_created` arrived with an XID (`host.window_handle()`), but the window never existed (`query_tree` failed).
- `cef.log`: `X error received. Request: CreateWindowRequest, Error: MatchError{… .bad_value = 14680065 (=0xe00001) …}`, followed by `WindowError`s for every later request on that XID.
- Cause: GPUI picks a transparent 32-bit visual (`find_visuals` → `visual_set.transparent`), and Chromium's child uses CopyFromParent visual/colormap/border with a different depth.

The fix keeps GPUI's ARGB window (needed for CSD shadows) and inserts a host window:

```rust
// src/xwin.rs
pub fn create_host(&self, parent: u32, r: XRect, bg: u32) -> Result<u32> {
    let s = &self.conn.setup().roots[self.screen];
    let wid = self.conn.generate_id()?;
    let aux = xproto::CreateWindowAux::new().background_pixel(bg).border_pixel(0)
        .colormap(s.default_colormap).bit_gravity(xproto::Gravity::NORTH_WEST);
    self.conn.create_window(s.root_depth, wid, parent, r.x as i16, r.y as i16, r.w.max(1) as u16,
        r.h.max(1) as u16, 0, xproto::WindowClass::INPUT_OUTPUT, s.root_visual, &aux)?.check()?;
    self.conn.map_window(wid)?; self.conn.flush()?; Ok(wid)
}
```

The host selects no events, so clicks on it (only visible before a tab maps) propagate to GPUI.

### 5.3 Creating a tab and keeping it sized (VERIFIED)

```rust
let wi = cef::WindowInfo { runtime_style: cef::RuntimeStyle::CHROME, ..Default::default() }
    .set_as_child(host_xid as _, &cef::Rect { x: 0, y: 0, width: w_dip, height: h_dip }); // DIP, host-relative
cef::browser_host_create_browser(Some(&wi), Some(&mut client), Some(&CefString::from(url)),
    Some(&cef::BrowserSettings::default()), None, None);
```

**What happens:**

- `on_after_created` arrives about 10–20 ms later on the UI thread. `browser.host().window_handle()` is the child's XID and `runtime_style()` is 1 (CHROME).
- The rectangle is in DIP: `ChildWindowDelegate::GetInitialBounds`, `chrome_child_window.cc`.

**Keeping the geometry right:**

- The content area is a GPUI `canvas` that fills the flex space. In its **prepaint** closure it stores the bounds in an `Rc<RefCell<Layout>>` and calls `apply_geometry`, which does two things whenever the values changed:
  - `XConfigureWindow` of the host to `round(bounds * scale)`, GPUI-window-relative physical pixels;
  - configure of the active child to `0,0,w,h`.
- These are only X requests on our own connection; no GPUI state is touched.
- Chromium follows from the `ConfigureNotify`. **No `was_resized` is needed for windowed browsers**: CEF documents it as OSR-only, and the page's `innerWidth`/`innerHeight` update on their own.

**Checks:**

- Window resize to 1000×650 and toggling a 40 px bar (`ctl bar`) re-layout the child (screenshots `s3.png`; log `configure host … 1000x507+0+116`).
- Resize at scale 1.5 → `inner=901x498 dpr=1.5` reported by the page.

### 5.4 HiDPI (VERIFIED)

- **Two different rules.** GPUI's X11 scale is `GPUI_X11_SCALE_FACTOR`, else Xft.dpi/96, else **RandR**. Chromium does not use RandR.
- So the shell computes one scale before anything else (same rule, minus RandR) and passes it to both: `GPUI_X11_SCALE_FACTOR` in the environment and `--force-device-scale-factor` to Chromium.
- At scale 2 (window 760×480 logical): host 1520×747 physical; page `devicePixelRatio=2 inner=760x374`; crisp (`hidpi2.png`).
- At scale 1.5: 1500×775 physical; `dpr=1.5 inner=1001x518`. CSS rounding is off by one pixel, which is harmless.
- **INFERRED:** a runtime scale change, such as moving to another monitor or an Xft.dpi change, needs `host.notify_screen_info_changed()` and a relaunch-free path in GPUI. Not tested.

---

## 6. Tabs, navigation, display events (VERIFIED)

**Lifecycle:**

- Create as in §5.3.
- **Switch:** `XUnmapWindow` the other children and call `host.was_hidden(1)` on them; configure + `XMapWindow` the new one, `was_hidden(0)`, then re-apply keyboard focus (§7). Pages see `visibilitychange` to hidden (logged).
- **Close:** `host.close_browser(0)` (runs beforeunload); `close_browser(1)` at quit. `on_before_close` arrives 2–5 ms later; then drop the `Browser`. Chromium destroys its X window itself.

**Navigation:**

- `frame.load_url`, `browser.go_back()`, `go_forward()`, `reload()`, `stop_load()`.
- `stop` on a slow load gives `on_load_error` `ERR_ABORTED` (-3).

**Events reaching GPUI**, through `CefEvent` over a futures channel into a `cx.spawn_in` task that calls `WeakEntity::update_in`:

| Handler | Data |
|---|---|
| `on_title_change` | title |
| `on_address_change` | URL, main frame only |
| `on_loading_state_change` | `is_loading`, `can_go_back`, `can_go_forward` |
| `on_favicon_urlchange` | icon URLs; take the list with `std::mem::take`, a clone loses it (G5) |
| `on_load_end` / `on_load_error` | status, error |
| `on_console_message` | message (used by the tests) |

**Leak check** (30 browsers in 10 cycles of 3 opens + 3 closes):

| | Before | After |
|---|---|---|
| Processes | 10 | 10 |
| Browser fds | 254 | 255 |
| Threads | 84 | 83 |
| RSS | 427.5 MB | 440.4 MB |

`on_after_created` fired 31 times and `on_before_close` 30 times; the one remaining browser is the original tab. X tree: one child under the host.

---

## 7. Keyboard focus (VERIFIED; needs the GPUI patch)

### 7.1 What X11 does, and what broke

1. **Unpatched GPUI goes "inactive" whenever the page has focus.**
   - Clicking the page or creating a browser moves X focus to the CEF child. GPUI's toplevel then receives `FocusOut` with detail `Inferior`, and stock `client.rs` calls `set_active(false)`.
   - Log: `gpui: window activation changed active=false x_focus=0x400003`, right after `on_after_created`.
   - **What depends on `is_window_active()` / `keyboard_focused_window`** (read in the gpui-pre / gpui-component sources):
     - gpui-base `Input` hides the caret and pauses blinking (`show_cursor` requires an active window) and dims the selection painting;
     - `WindowBorder` paints its inactive style; `img` animations pause; AccessKit reports the window unfocused;
     - `inactive_frame_interval` (33 ms by default) throttles GPUI animations;
     - with `keyboard_focused_window = None`: `ModifiersChanged` events are dropped, XIM is reset, `open_window` gets no transient parent for dialogs, `active_window()` is None, and portal dialogs get no parent window id.
2. **Keys follow the pointer when focus is on an ancestor.** With X focus on GPUI's toplevel, X delivers key events to the **deepest descendant under the pointer** that selects them.
   - Seen: after clicking the omnibox, a key typed with the pointer resting over the page reached the page (`keydown` logged by the page). X focus was `0xe00001` the whole time.
   - Chromium also counts itself focused when the pointer is in it ("pointer focus"): CEF fires `on_got_focus` and the page fires `focus`.
3. **An app-created focus proxy cannot work.** I first created an InputOnly proxy from our own connection and gave it focus. Keys then went nowhere: key events **do not propagate past the focus window** (the server's `stopAt` is the focus window), and the proxy is not selected by GPUI's connection.
   - `xctl keywatch` selecting KeyPress on GPUI's toplevel received nothing while focus sat on the proxy.
4. **Hover.** `XinputLeave` with detail `Inferior` when the pointer moves onto the page gives GPUI `MouseExited` and `hovered=false`. That is the correct outcome for hover styles, so it is not patched.

### 7.2 The GPUI patch (`vendor/gpui-x11-focus.patch`, applied to `gpui-pre-linux` 0.3.9 `src/linux/x11/client.rs`)

- `open_window`: after `X11Window::new`, create a child `InputOnly` 1×1 window at (-1,-1).
  - Use `event_mask(KEY_PRESS | KEY_RELEASE)` on **GPUI's** connection, set `WM_NAME "gpui-focus-proxy"`, map it, and record `focus_proxies[proxy] = toplevel`.
- Event loop: `map_focus_proxy(event)` rewrites `KeyPress`/`KeyRelease` `event.event` from the proxy to the toplevel before the normal XIM/key handling.
- `FocusIn`: call `set_active(true)` only when `detail != Inferior`.
- `FocusOut` with `detail == Inferior`: keep the window active and `keyboard_focused_window` set; reset only compose, pre-edit and IME state.

```rust
Event::FocusOut(event) => {
    let window = self.get_window(event.event)?;
    if event.detail == xproto::NotifyDetail::INFERIOR {   // focus moved into a child we embed
        /* reset compose / pre-edit / IME only */ return Some(());
    }
    window.set_active(false); …
```

The shell finds the proxy with `query_tree` + `WM_NAME` (`XConn::find_child_named`).

**Upstreaming (INFERRED):** a cleaner upstream API would be a `Window::x11_focus_proxy()` or `Window::take_keyboard_focus()` plus the Inferior rule. This is a plausible small Zed PR.

### 7.3 Focus model in the shell (`src/shell.rs`)

`enum KeyFocus { Chrome, Page }`. `apply_x_focus()` puts X focus on the proxy or on the active tab's child, **never on the toplevel**. It only moves focus within our own toplevel; see the focus-fight note below.

| Trigger | Action |
|---|---|
| any mouse-down reaching GPUI (`.capture_any_mouse_down` on the root). Page clicks never reach GPUI, they go to the child window | `KeyFocus::Chrome` → `XSetInputFocus(proxy)` |
| CEF `on_got_focus` (click in the page) | `KeyFocus::Page` → `XSetInputFocus(child)` + `host.set_focus(1)`; `window.focus(root)` so the omnibox drops its caret |
| Ctrl+L / Alt+D / F6 in the page (`on_pre_key_event`) | `KeyFocus::Chrome`, focus the omnibox, `select_all` |
| CEF `on_take_focus` (Tab out of the page) | same as Ctrl+L |
| Enter in the omnibox | `KeyFocus::Page`, then `load_url` |
| tab switch | after mapping the child, `apply_x_focus()` |
| GPUI window activated by the WM (`observe_window_activation`, active) | `apply_x_focus()` |
| `FocusHandler::on_set_focus` (CEF wants focus on its own: navigation = 0, system = 1) | return 1 (refuse) while `KeyFocus::Chrome`, via the `Shared::page_focus_allowed` atomic. Ctrl+T's new tab then keeps the omnibox |

**Focus-fight bug, found and fixed (VERIFIED):** re-applying focus on every activation while a native Chrome window was WM-active made our `XSetInputFocus` and KWin fight. Thousands of activation flips in about 15 s stalled the GPUI thread. The guard is that `apply_x_focus` only acts when the current X focus window is inside our toplevel (`XConn::is_within`).

### 7.4 Results (VERIFIED; screenshots `f2.png`, `f3.png`, `f4.png`)

| Scenario | Result |
|---|---|
| startup | X focus on the child, `gpui_active=true` |
| click the omnibox, move the pointer over the page, type `zz` | X focus = proxy (`0xe00012`); `zz` is in the omnibox (`f4.png`); the page logged nothing |
| click the page, move the pointer over the toolbar, type `w` | X focus = child; page `keydown key=w` |
| Ctrl+L in the page | `on_pre_key_event … vk=0x4c mods=0x4` → `FocusOmnibox`; X focus = proxy; URL selected with caret; the next key replaced it |
| Tab from the page's last link | `on_take_focus next=true` → omnibox |
| GPUI `is_window_active()` | stays true through all of the above |

`watch` output for a chrome click:

```
FocusOut win=0x400003 detail=ANCESTOR
FocusOut win=0x1400000 detail=VIRTUAL
FocusIn win=0xe00001 detail=INFERIOR
```

**IME:** GtkUi is loaded (`libgtk-3` mapped in the browser process, external pump). No IM server ran in the private session, so ibus/fcitx composition in the page was **not tested**.

---

## 8. Chrome accelerators inside the page (VERIFIED, `accel-test.sh`)

### 8.1 What Chrome does when nothing is intercepted

`SPIKE_KEYS=none`, page focused:

| Key | `on_chrome_command` (id, name) | Effect |
|---|---|---|
| Ctrl+T | 34014 `IDC_NEW_TAB` | **opens a native "New Tab - Chromium" window** (TYPE_NORMAL Chrome UI with tab strip, omnibox and "Sign in to Chromium"), 1050×980; `on_after_created` arrives through `default_client`; its window's parent is the root window |
| Ctrl+N | 34000 `IDC_NEW_WINDOW` | native Chrome window, likewise |
| Ctrl+W | 34015 `IDC_CLOSE_TAB` | closes our browser (`on_before_close`) |
| Ctrl+Shift+T | *none* | nothing (the page receives the keydown) |
| F11 | 34030 `IDC_FULLSCREEN` | separate 382×55 Chrome toplevel "To exit full screen, press and hold Esc"; the GPUI window is unchanged |
| Ctrl+P | 35003 `IDC_PRINT` | Chrome's **tab-modal print preview inside the child**, listing the host's real CUPS printer |
| Ctrl+F | 37000 `IDC_FIND` | Chrome's **find bar** drawn inside the child (top right) |
| F12 | 40237 `IDC_DEV_TOOLS_TOGGLE` | **native DevTools toplevel** ("DevTools - 127.0.0.1:8765/page1.html", 640×640); `on_after_created popup=1 xid=0` |

More keys with no interception:

| Keys | Result |
|---|---|
| Ctrl+R, F5 | 33002 `IDC_RELOAD` |
| Ctrl+U | 35002 `IDC_VIEW_SOURCE` |
| Ctrl+Shift+I | 40004 `IDC_DEV_TOOLS` |
| Ctrl+H | 40010 `IDC_SHOW_HISTORY` |
| Ctrl+J | 40012 `IDC_SHOW_DOWNLOADS` |
| Ctrl+S | 35004 `IDC_SAVE_PAGE` |
| Ctrl+O | 40000 `IDC_OPEN_FILE` |
| Ctrl+L, Alt+D, F6, Ctrl+Tab, Alt+Left, Ctrl+D, Ctrl+Q | **no command**: disabled in a `TYPE_POPUP` Browser with no toolbar or tab strip |

The page receives keydown before Chrome for the non-reserved keys (F11, Ctrl+P, Ctrl+F, F12). It never sees Ctrl+T, Ctrl+N or Ctrl+W.

### 8.2 Interception

- **`on_chrome_command` only** (`SPIKE_KEYS=cmd`): Ctrl+T/N/W, F11, Ctrl+P/F and F12 are all caught and become shell actions. **Ctrl+Shift+T is missed** (no command is dispatched).
- **`on_pre_key_event` + `on_chrome_command`** (`both`): every key above becomes a shell action. No extra toplevel appeared in any case. Ctrl+T/N create a child tab and Ctrl+W closes the active tab through the shell.

```rust
// on_pre_key_event: RAWKEYDOWN/KEYDOWN; return 1 = consumed (down/up/char of shell keys swallowed)
fn shortcut_for(ev: &KeyEvent) -> Option<ShellAction>   // ctrl+L/alt+D/F6, ctrl+T, ctrl+shift+T, ctrl+N, ctrl+W/ctrl+F4,
                                                        // F11, ctrl+P, ctrl+F, F12/ctrl+shift+I, ctrl+U, F5/ctrl+R,
                                                        // alt+Left/Right, ctrl+(shift+)Tab — Windows VK codes
// on_chrome_command: map IDC names (via cef_id_for_command_id_name, reverse table of all 425 IDC_* names)
// to shell actions and return 1; return 1 for every other command too, except IDC_ZOOM*, IDC_CONTENT_CONTEXT*,
// IDC_STOP, cut/copy/paste — so no other Chrome-native UI can open.
```

**Recommendation (INFERRED from the results):**

- Use `on_pre_key_event` for **reserved** browser shortcuts the page must never see, plus the ones Chrome does not dispatch in a TYPE_POPUP browser: Ctrl+L, Ctrl+Shift+T, Ctrl+Tab, Alt+Left/Right.
- Use `on_chrome_command` for everything else. That keeps Chrome's semantics: the page can `preventDefault` non-reserved keys like Ctrl+F or Ctrl+P.
- Default-deny unknown IDC commands.

---

## 9. Overlays above the page (VERIFIED, screenshots)

GPUI's normal deferred overlays are painted on GPUI's surface, **under** the native CEF child. `ctl menu plain`: only the 6 px of the dropdown above the content area are visible (`ov-plain.png`).

### 9.1 SHAPE cut-out (recommended for in-window dropdowns, menus, omnibox suggestions)

- The overlay panel contains a `canvas` whose prepaint computes the panel rectangle in host-relative physical pixels.
- It then applies `shape_rectangles(SO::SET, SK::BOUNDING and SK::INPUT, host, full − hole)` (x11rb `shape`) to the **host** window, which clips every CEF child inside it.
- Closing the menu does `shape_mask(SET, …, NONE)`.

**Results:**

- `ov-shape-raw.png`: the GPUI dropdown is fully visible over the page.
- A click inside the hole reaches GPUI (`chrome click` logged; focus went to the proxy). Pointer input in the hole goes to GPUI too.

**Limits:**

- The cut-out is binary. No alpha blending over page pixels, so soft shadows and rounded corners show GPUI's own background in the hole.
  - Fix: paint a matching background behind the panel, or keep the hole exactly the panel rectangle. My 8 px shadow padding showed GPUI's dark content background.
- The page under the hole is not visible at all, so a translucent scrim over the page is impossible.
- One X request per change.

### 9.2 Separate `WindowKind::PopUp` GPUI window

- `cx.open_window(WindowOptions { kind: WindowKind::PopUp, focus: false, window_bounds: Windowed(screen rect) … })`.
- Screen origin = `translate_coordinates(gpui_xid, root)` / scale + panel offset.
- GPUI makes it override-redirect with `_NET_WM_WINDOW_TYPE_NOTIFICATION`.

**Results:**

- `ov-popup-screen.png`, composed from every toplevel: the popup sits exactly over the page at (1170,184). Hover highlight works.
- It can extend past the window edge, and with a compositor it can use real alpha.

**Limits:**

- It is a separate toplevel. It does not follow window moves (track `ConfigureNotify`/close on move).
- It stacks above **all** windows (close it on deactivation).
- It gets no WM keyboard focus, so the omnibox must keep the keyboard.
- Every popup is another GPUI window with its own wgpu surface.
- `WindowKind::AnchoredPopup` returns `PopupNotSupportedError` on X11.

**Recommendation (INFERRED):** use SHAPE for overlays that stay inside the window (omnibox dropdown, menus, panels, in-window dialogs). Use PopUp windows only for things that must leave the window (tooltips near the edge, large extension popups).

---

## 10. Popups, new windows, dialogs (VERIFIED, `dialog-test.sh`)

### 10.1 `window.open` / `target=_blank`

`on_before_popup` with `PopupMode::Child`:

```rust
if let Some(wi) = window_info {          // assign fields; replacing the struct would also write window_name (G3)
    wi.parent_window = shared.parent() as _;          // the content host XID
    wi.bounds = *shared.content_rect_dip.lock().unwrap();   // host-relative DIP
    wi.runtime_style = RuntimeStyle::CHROME;
    wi.windowless_rendering_enabled = 0;
}
if let Some(c) = client { *c = Some(new_client(shared.clone())); }
0
```

**Results:**

| Trigger | Disposition | Result |
|---|---|---|
| Button `window.open('page2.html','_blank')` | 3 (`NEW_FOREGROUND_TAB`), gesture=1 | `on_after_created popup=1 opener=1`, parent = host; the shell opens it as a new tab; page2 reports **`opener=present`**; `window.open` returned a window |
| `target=_blank` link | | child tab, `opener=null` (`noopener` by default), new renderer process |

- `execute_java_script("window.open(…)")` also counts as gesture=1.
- No `BadWindow` crash (CEF #3165) in 4 popups.
- `PopupMode::Default` (return 0 without a parent) opens a native "Page Two - Chromium" Chrome-UI window instead.

### 10.2 What Chrome style shows by default, and whether the handler sees it first

| Page action | Handler called in Chrome style? | Default UI when the handler returns 0 | Shell mode (handler returns 1) |
|---|---|---|---|
| `alert`/`confirm`/`prompt` | **yes**: `JsdialogHandler::on_jsdialog` (type 0/1/2) | Chrome tab-modal dialog **inside the child** ("127.0.0.1:8765 says"); Esc cancels | `callback.cont(1, …)` → `alert returned`, `confirm -> true`, `prompt -> spike-answer` |
| `<input type=file>` | **yes**: `DialogHandler::on_file_dialog` (mode 0) | **xdg-desktop-portal FileChooser** (KDE "Open File" toplevel, 1186×986) | `callback.cont([/etc/hostname])` → `file -> hostname` |
| geolocation | **yes**: `PermissionHandler::on_show_permission_prompt` (perms 0x100) | Chrome permission bubble ("Know your location", 3 buttons) **inside the child**, top-left | `callback.cont(ACCEPT)`; then `on_dismiss_permission_prompt result=0` |
| `Notification.requestPermission()` | **yes**: same, perms 0x8000 | bubble | `notification -> granted` |
| `<a download>` | **yes**: `can_download`, then `on_before_download` | saves **automatically to `$HOME/Downloads`**, plus a download bubble | `callback.cont(path, 0)` → file in our directory; `on_download_updated` complete |
| `requestFullscreen()` | `DisplayHandler::on_fullscreen_mode_change(true)` | Chrome "press and hold Esc" bubble as a separate toplevel; the shell must fullscreen the GPUI window itself (it did: `window.toggle_fullscreen()`) | same |
| `<select>` | none | separate override-redirect toplevel parented to the root (106×74), correctly placed over the page | same |
| link hover | `on_status_message` | Chrome **status bubble as a separate toplevel** (285×25, bottom-left of our window) | still shown: **open issue** |

### 10.3 Chrome-native UI that still appears with all handlers in place (open issues)

- Status bubble and fullscreen-exit bubble (separate toplevels).
- Find bar, print preview and tab-modal dialogs inside the child (only if not intercepted).
- `<select>` popups (fine).
- Context menus were not examined here: `run_context_menu` exists, see `cef-rs-api.md`.

---

## 11. Screenshots from inside the process (VERIFIED)

- **`shotraw`: one `GetImage(ZPixmap)` of the GPUI toplevel.** Xwayland composites the 24-bit and 32-bit children into the toplevel's backing pixmap, so the image already contains the page. It also respects SHAPE cut-outs.
  - 1280×800 in **4.6–7 ms**; 1520×954 in 8.7 ms.
- **`shot`: `GetImage` of the toplevel plus each mapped descendant pasted on top**, honouring the bounding SHAPE of the window and its ancestors. 20–25 ms.
  - This is the portable fallback. On a non-compositing Xorg, the core protocol leaves children of a different depth "undefined" in a parent `GetImage`.
  - The first version ignored ancestor SHAPE and drew the page over the hole; fixed.
- **`xctl shotscreen`** composes all toplevels. Rootless Xwayland returns `BadMatch` for `GetImage` on the root window.

---

## 12. Clean shutdown (VERIFIED)

```
[3.988] ctl> quit                       (or WM_DELETE_WINDOW: "close button pressed; closing browsers first")
[3.989] cef: on_before_close id=1       close_browser(1) on every tab
[3.992] cef: on_before_close id=2
[3.994] cef: on_before_close id=3
[3.998] gpui: all browsers closed; quitting GPUI     (stop pump flag, cx.quit())
[4.049] main: GPUI run() returned
[4.282] main: cef shutdown in 28.9ms; exiting       (after 200 ms pump_blocking, drop App + Shared)
exit_code=0, no spike processes left
```

**Window close button:**

- `window.on_window_should_close` returns false while tabs exist, starts `close_browser(1)` on all of them, and `cx.quit()` runs after the last `on_before_close`.
- Tested by sending `WM_PROTOCOLS/WM_DELETE_WINDOW` (`xctl close`), which is what KWin's close button sends.
- GPUI's `Application::run` returns after `quit()` on Linux, so `cef::shutdown()` can run on the main thread afterwards.

---

## 13. Recommended structure for the real crate

### 13.1 Crates and modules

```
crates/vsesvit-shell-linux/            # bin "vsesvit"
  src/main.rs           api_hash → (no execute_process needed with helper) → scale → CEF init → GPUI run → CEF shutdown
  src/cef/              (never `use gpui::*`; wrap_* macros need unqualified CEF names)
    app.rs              BrowserApp (switches, cached BrowserProcessHandler, default_client)
    client.rs           one Client per GPUI window or profile; handlers built once, returned by clone (G8)
    handlers/{life_span,display,load,keyboard,command,focus,jsdialog,dialog,download,permission,context_menu,request}.rs
    shortcuts.rs        reserved-key table (pre-key) + IDC→ShellAction map + default-deny
    events.rs           CefEvent + Sink (futures unbounded); Shared (atomics/Mutex read by handlers synchronously)
    pump.rs             CEF + GLib pump task, fd watcher thread, adaptive caps, stats
  src/x11/              own x11rb connection: content host per window, geometry, map/unmap, SHAPE, focus, capture, error drain
  src/shell/            GPUI: BrowserWindow, TabStrip, Toolbar/Omnibox, ContentArea (canvas prepaint → geometry),
                        OverlayManager (SHAPE vs PopUp), FocusModel (KeyFocus), actions + keymap, dialogs drawn in GPUI
crates/vsesvit-cef-helper/             # bin; depends only on cef + the shared ChildApp/RenderProcessHandler crate
vendor/gpui-pre-linux/ (+ patch)       # via [patch.crates-io]; keep the patch small, try to upstream
```

### 13.2 Threads

| Thread | Runs |
|---|---|
| main | GPUI's calloop loop, **and** CEF's UI thread (inside `do_message_loop_work`), **and** Chromium's GLib sources (X11, GDK/GtkUi, AT-SPI, GIO) |
| `glib-fd-watch` | `poll(2)` on GLib's fds → wake |
| GPUI timer thread | calloop timers → wakes the pump task |
| GPUI background workers | not used for CEF |
| CEF IO and other threads | request/resource handlers. They may only touch `Arc<Shared>` and send events |

### 13.3 Event flow

- **CEF → GPUI:** a handler (UI thread inside the pump, or IO thread) sends a `CefEvent` over an unbounded channel. A GPUI foreground task (`cx.spawn_in(window, …)`) calls `WeakEntity::update_in(…, on_cef)` → entity state → `cx.notify()` → render → the content area's prepaint pushes X geometry.
  - Handlers never call GPUI. CEF callbacks run inside `do_message_loop_work`, which runs outside any GPUI update, so GPUI's `RefCell` is never re-entered.
- **Decisions a handler must make synchronously** are read from `Shared`, which GPUI keeps current:
  - `on_set_focus` → `page_focus_allowed`;
  - `on_before_popup` → host XID + content rect (DIP);
  - `on_pre_key_event` → static shortcut table;
  - permission, dialog and download prompts → keep the callback (`Clone`) inside the event and answer later from GPUI UI.
- **GPUI → CEF:** direct calls on the main thread from GPUI handlers. This is the CEF UI thread, so `browser_host_create_browser`, `close_browser`, `load_url`, `set_focus`, `was_hidden` and `execute_chrome_command` are all legal there. Never call CEF from other threads.
- **Per GPUI window** keep: one content host, the GPUI focus proxy, and a `browser_id → (window, host)` map so popups land in their opener's window.

### 13.4 Things the real crate must decide or still verify

1. **Carry or upstream the GPUI focus patch.** It is the only GPUI change needed for this architecture.
2. **Password store** (KWallet/libsecret vs basic) without blocking networking; download directory pref (`download.default_directory`); `default_client` handling of Chrome-created windows: adopt them, close them, or prevent them.
3. **Chrome toplevels that cannot be intercepted:** the status bubble and fullscreen bubble. Look for prefs or features, or accept them.
4. **Not tested:**
   - IME (ibus/fcitx) in the page through GtkUi;
   - DevTools as a child (`show_dev_tools` with `set_as_child`);
   - context menus with extension items;
   - drag and drop between GPUI and the page;
   - multi-monitor or runtime DPI changes;
   - behaviour on the owner's real KWin/Xwayland session (only the private virtual KWin was used);
   - release-build CPU and memory numbers (all numbers above are from debug builds with dependencies at opt-level 2).
5. The **pump caps** (33 ms active / 250 ms idle) are a policy choice. Delayed browser-side UI tasks can run up to 250 ms late while idle; any activity switches back to 33 ms within one tick.

---

## Appendix: evidence files (not committed)

- Logs and screenshots: `/tmp/vsesvit-scratch/core-env/`.
  - `s2.png`: first page in GPUI.
  - `f1`–`f4.png`: focus.
  - `accel-{none,cmd,both}/`: accelerators.
  - `ov-plain.png`, `ov-shape(-raw).png`, `ov-popup-screen.png`: overlays.
  - `dialogs-{default,shell}/`: dialogs.
  - `hidpi2.png`, `hidpi15.png`: HiDPI.
  - `sandbox-helper.png`, `chrome-gpu.png`.
  - `cpu-*.txt`: CPU measurements.
- Scripts: `/tmp/vsesvit-scratch/spike/*.sh`; see the README there.
