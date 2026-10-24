# vsesvit-gpui: implementation design

Resume notes: first draft 2026-10-23. Revised 2026-10-24 for review round 1 (`.audit/scratch/design/review-{gpt,opus}.md`), dispositions in §10.1. Revised again 2026-10-24 for review round 2 (`.audit/scratch/design/review-gpt-r2.md`): all 14 findings and the 25 rows it did not mark resolved are fixed, dispositions in §10.2, and the §8 ownership check is now a script whose run is recorded in §8.11. Not committed. Next: a round-2 check of this revision against the review's wave-0 verdict, then dispatch wave 0 (WX.0 is already running and must report against the §5.10 matrix). New units since the draft: W0.5 (test environment), WX.0/WX.1 (extension spike in wave 0, extension contract freeze), lane C (C.0 to C.6, core moves B), W2.6 (page prompts), W4.5 (extension migration).

## 0. Scope and sources

Date: 2026-10-23, revised 2026-10-24 after two adversarial reviews (§10.1) and again after a second-round review (§10.2). Status: design, nothing built. Branch `linux-cef-gpui`.

This document is the implementation design for `crates/vsesvit-gpui`, the GPUI + CEF browser shell.
- On Linux it replaces `crates/vsesvit-gtk` and `crates/vsesvit-webext`. The whole app runs on X11 (XWayland under Wayland).
- On Windows it is a build-time alternative to `crates/vsesvit-winui`.
- On both OSes the GPUI shell is chosen by the same `gpui` cargo feature on `crates/vsesvit` (§6.1). Until the switch-over wave (wave 7, §8.10) the default build is unchanged: GTK on Linux, WinUI on Windows. No user sees the GPUI shell before it reaches parity. Wave 7 flips the Linux default; the Windows default stays WinUI past these waves, and flipping it needs the Windows-only parity items of §4.2.1.

It builds on `ui-architecture.md` and does not repeat it. That document stays the reference for the view tree, entities, the feature map (its §2), theme (§3) and the frozen interfaces (§5.2). The engine contract is UA §4.2 as amended in §1.5 and §5.4; where they differ, this document wins. UA paragraphs this document supersedes are listed below so an implementer reading UA alone is not misled. This document replaces UA's work breakdown (§5.3, §5.4) with the waves in §8.

**Sources and their short names.**

| Short | File | Used for |
|---|---|---|
| ST | `STATUS.md`, `log.tsv` | decisions so far |
| UA | `ui-architecture.md` | GPUI chrome, entities, engine contract |
| SC | `spike-core.md` | Linux foundation; §13 recommended structure |
| SW | `spike-win.md` | Windows; §13 seam sketch, §14 must-carry list |
| EL | `spike-ext-load.md` | extension install, uninstall, update |
| EA | `spike-ext-api.md` | extension popups, commands, context menus |
| API | `research/cef-rs-api.md` | cef-rs gotchas G1 to G17 |
| CE, GX, GP, W, O, P | `research/cef-extensions.md`, `gpt-extension-architecture.md`, `gpui.md`, `cef-windowing.md`, `cef-osr.md`, `prior-art.md` | background |
| inv/… | `inventory/*.md` | feature checklists |

Digests of the inventories and research are in `.audit/scratch/design/` (not committed). Section numbers below point at the source documents, not the digests.

**What the spikes and the reviews changed in UA.** Every row is a decision this document takes and supersedes the UA paragraph it names; the rest of UA stands.

| UA said | Now | Why |
|---|---|---|
| Overlays default to GPUI `PopUp` windows; XShape cut-out is the fallback (UA §1.7) | Overlays are in-window GPUI elements plus a hole in the content host. PopUp windows only for surfaces that must leave the window | SC §9 recommends SHAPE for in-window surfaces. SW §9 shows in-window overlays paint over the page for free on Windows and need only an input hole. One mechanism serves both OSes |
| Activation from `_NET_ACTIVE_WINDOW`; GPUI's `is_window_active()` is wrong (UA §1.6) | GPUI's own activation, on both OSes | The X11 focus patch keeps the window active when focus enters the page (SC §7.2). Windows GPUI never loses activation to a child (SW §7) |
| Focus patch "only if the spikes demand it" (UA §1.10) | Carried: `gpui-x11-focus.patch` on Linux, `gpui-windows-keys.patch` on Windows | SC §7, SW §4 |
| `vsesvit-cef` owns an x11rb connection and does geometry, map/unmap and shape (UA §4.4) | The shell's `platform` module owns every native window call. The CEF glue only creates browsers into a parent handle and calls CEF | One seam per OS instead of two; SW §13 |
| Chromium's native page context menu, shell items added (UA §0.8); `ShellHooks::context_menu_items` and `ContextMenuCommand` (UA §4.2) | GPUI-drawn menu built from an owned copy of `CefMenuModel` in `run_context_menu`, sent as a typed prompt; shell items are added and run by the shell (§1.5) | Verified path that keeps extension items and their `activeTab` grant (EA §5); the native menu in a toolbar-less child is unverified |
| No request/answer protocol for JS dialogs, beforeunload, file choosers, auth (UA §4.2) | One prompt protocol with ids, cancellation and exactly-once completion for all of them and for permissions (§1.5) | GPT review 4; SW §10 shows native fallbacks otherwise |
| Hooks and handlers run "outside any GPUI update" (UA §4.1 items 3 and 4, the `ShellHooks` doc comment) | Handlers can run synchronously inside any engine call and, on Windows, inside GPUI's message dispatch; hooks read snapshots only, never core (§1.6) | SW §4, §13.3; Opus review 5 |
| Extensions loaded with `sync_extensions` (UA §4.2); `ExtensionLoadFailed` | External Extensions install, helper extension, `uninstallSelf`, shell-hosted Alloy popups (§5); `ExtensionEvent::LoadFailed` | EL §9, EA §7 |
| `ForeignWindowAdopted { url }`, closed by the engine (UA §4.2, §4.4) | a synchronous neutralize hook with a result, then `ForeignWindowOpened { id, context }` and `ForeignWindowReady { id, url }`; the shell reopens the URL as a tab in the same browsing context and closes the foreign browser (§5.7) | GPT review 11, Opus review's note on it, R2-5 |
| `close_all` → `AllTabsClosed` (UA §4.2, §1.6 "Quit") | `begin_shutdown` → `UserBrowsersClosed`, then `close_internal` → `AllBrowsersClosed` (§1.5, §3.6) | the hidden browsers outlive the user's for clear on exit (R2-4) |
| `TabEvent::Closed` is the only way a tab ends (UA §4.2) | `TabEvent::Detached` ends a tab whose browser stays parked for its downloads (§4.2) | Opus review 20, R2-6 |
| `chrome_status_bubble` on (UA §4.4) | off; the shell's link preview instead (Q13) | Chrome's bubble is a separate toplevel on X11 (SC §10.3) |
| `ZoomChanged`, `AudioStateChanged` (UA §4.2, [S:14]) | `ZoomChanged` comes from the shell's own interception (Q10); `AudioStateChanged` from a document-start script (Q11) | no CEF callback |
| Memory Saver by discard (UA §2.3) | `Engine::set_frozen` in the contract; discard optional (Q9) | CEF has no discard |
| `NativeParent::X11(u32)`, `DeviceRect`, `set_bounds`, `set_occlusions`, `reparent` | `NativeHandle` (an XID or an HWND), `PhysRect`; geometry is the shell platform's (§1.4, §1.5) | Windows; one seam per OS |
| FakeSurface by feature (none in UA; draft of this document) | `cfg(test)` plus a `test-support` module that never changes the real alias (§1.4) | Opus review 24 |

## 1. Crate and module layout, platform seam

### 1.1 Crates

"One crate for both platforms" (ST) means one shell crate, not one crate per OS. The CEF glue and the engine contract stay separate crates, as UA §1.1 and SW §13.1 both propose. The crate boundaries enforce two rules that would otherwise be review rules: the CEF glue never names a GPUI type (G7: `wrap_*` macros expand to unqualified names such as `App` and `Window`), and the shell never names a CEF type, so its tests run without libcef.

```
crates/vsesvit (bin "vsesvit")      wires engine + shell; picks the shell by the `gpui` feature on both OSes (§6.1)
 ├─► vsesvit-cef                    CEF glue; implements the contract. Deps: cef, vsesvit-cef-child, vsesvit-engine, vsesvit-core (types only).
 │                                  Dev-deps (the smoke example only): vsesvit-gpui with test-support, vsesvit-testenv (§6.2)
 │      └─► vsesvit-cef-child       ChildApp, custom-scheme table, RenderProcessHandler, OwnedArgs. Deps: cef only
 └─► vsesvit-gpui                   the shell. Deps: vsesvit-engine, vsesvit-core, vsesvit-sync, vsesvit-update, vsesvit-testenv, gpui, gpui-component
        └─► vsesvit-engine          contract: types, Engine trait, events, prompts, hooks, FakeEngine. Deps: vsesvit-core, url, futures-channel, serde_json (§5.4's results)
crates/vsesvit-testenv              the private display/desktop identity and the guard every window-creating test binary calls (§7.3). Deps: x11rb (Linux), windows-sys (Windows); no gpui, no cef
crates/vsesvit-helper (bin "vsesvit-helper")   Linux CEF sub-process image: api_hash + execute_process(ChildApp). Deps (Linux only): vsesvit-cef-child
crates/vsesvit-sandbox (cdylib "vsesvit_app")  Windows sandboxed entry: exports RunWinMain. Deps (Windows only): vsesvit-cef, vsesvit-gpui
```

| Crate | Exists today | Compiles on | Notes |
|---|---|---|---|
| `vsesvit-engine` | no | both | UA §4.2 with the changes in §1.5 and §5.4. `xtask check-deps` rejects a `gpui` or `cef` dependency (UA U00 asked for a CI grep) |
| `vsesvit-cef` | no | both | SC §13.1 `src/cef/`, SW §13.1 `vsesvit-cef`. Never `use gpui` |
| `vsesvit-cef-child` | no | both | SC §13.1: the helper "depends only on cef + the shared ChildApp/RenderProcessHandler crate". Keeps core's dependency graph (and any `-l` it emits, or a static initialiser) out of every renderer. `check-deps` rejects any dependency but `cef`; CI runs `readelf -d` on the helper and fails if `NEEDED` grows past libcef, libc, libm, libgcc_s (SC §2, §3.1) |
| `vsesvit-gpui` | no | both | UA §5.1 tree with the `platform` changes of §1.3. Never names `cef` |
| `vsesvit-testenv` | no | both | one guard for the self-test, every example and every window-creating test binary (§7.3). `xtask` uses it too, to write the identity it later checks |
| `vsesvit-helper` | no | both; real `main` on Linux only, `fn main() {}` elsewhere | SC §3.1: the helper keeps GPUI, X11 and Vulkan code out of renderers (2.5 MB `.text` against 45 MB). Windows needs none (below) |
| `vsesvit-sandbox` | no | both; empty off Windows | SW §3.2: Windows' sandbox runs only through CEF's `bootstrap.exe`, which loads `<exe>.dll` and calls `RunWinMain`. The lib is named `vsesvit_app`, not `vsesvit`, so its PDB does not collide with the `vsesvit` bin's (cargo #6313). Packaging copies it to `vsesvit.dll` next to the renamed bootstrap (§6.4). It has no features; its Windows dependencies are unconditional (§6.2) |

`vsesvit-sandbox/Cargo.toml`, in full except the package keys:

```toml
[lib]
name = "vsesvit_app"
crate-type = ["cdylib"]
path = "src/lib.rs"                 # off Windows: an empty file behind #![cfg(windows)]

[target.'cfg(windows)'.dependencies]
vsesvit-cef = { workspace = true }
vsesvit-gpui = { workspace = true }
```

The export is `#[no_mangle] pub extern "C" fn RunWinMain(instance: HINSTANCE, cmd: *mut u16, show: i32, sandbox_info: *mut c_void, version: *const cef_version_info_t) -> i32` (SW §3.2).

**Process images per build.**

| Build | Browser process | CEF sub-processes | Sandbox |
|---|---|---|---|
| Linux, any | `vsesvit` (crates/vsesvit) | `vsesvit-helper` beside it (`browser_subprocess_path`) | namespace + seccomp-BPF (SC §3.3) |
| Windows GPUI, release package | `vsesvit.exe` = CEF `bootstrap.exe` renamed, loading `vsesvit.dll` (= `vsesvit_app.dll`) | the same image | `bootstrap.exe` + `RunWinMain` (SW §3.2) |
| Windows GPUI, `cargo run` | `vsesvit.exe` (crates/vsesvit) | the same exe (empty `browser_subprocess_path`) | off (`no_sandbox = 1`); dev only (SW §3.1) |
| Windows WinUI | `vsesvit.exe` (crates/vsesvit) | WebView2's own | WebView2's own |

Same-exe mode on Windows dev builds avoids a second Windows binary that release packages would never ship. Linux keeps the helper because release builds use it too.

**Finding the Linux helper.** `settings.rs` resolves `vsesvit-helper` beside `current_exe()`. When it is missing, a debug build logs a warning and uses same-exe mode (empty `browser_subprocess_path`; SC §3.1 ran both modes), so `cargo run -p vsesvit --features gpui` works without a second command. A release build refuses before any window opens, with "vsesvit-helper is missing next to <exe>" on stderr and through `platform::fatal_message` (§1.3), and exits 1. `xtask selftest`, `xtask probe` and `xtask package` always build `-p vsesvit-helper` into the same target directory and set `LD_LIBRARY_PATH` to it, so they test the shipped layout and also work outside Cargo (Opus review 15).

Every entry point runs the same two calls in the same order. `vsesvit_cef::run_subprocess(&sandbox)` comes first: it calls `api_hash` (G1) and `execute_process`, and returns `Some(code)` in a sub-process. `vsesvit_gpui::run(&factory)` follows in the browser process. On Linux with the helper, `execute_process` returns -1 at once; it stays so that same-exe mode and stray `--type=` relaunches behave (SC §3.1). The arguments come from one process-wide `OwnedArgs` built from `std::env::args_os()` (in `vsesvit-cef-child`), never `cef::args::Args`, which panics on a non-UTF-8 argument (G9) such as a file name passed by the desktop file's `%U`.

### 1.2 `vsesvit-cef` module tree

Built from SC §13.1 and SW §13.1. Thread affinity is per method, in the table after the tree. The owner column is the unit of §8 that fills the file; W0.1 creates every file as a stub.

```
crates/vsesvit-cef-child/src/lib.rs   ChildApp (custom schemes, RenderProcessHandler), the scheme table, OwnedArgs   W1.3
crates/vsesvit-cef/
  build.rs                      Linux: nothing (rpath is set by the bins). Windows: nothing; cef-dll-sys copies the runtime   W1.3
  src/
    lib.rs                      run_subprocess(&Sandbox) -> Option<ExitCode>; CefEngineFactory; Sandbox            W1.3
    app.rs                      BrowserApp (switches, cached BrowserProcessHandler, default_client); the
                                child-side app comes from vsesvit-cef-child, same schemes in every process (API §3.4)   W1.3
    settings.rs                 EngineConfig -> cef::Settings + the switch table of §3.3, per OS; helper lookup     W1.3
    driver/mod.rs               CefDriver: impl EngineDriver (wakes, do_work, stop, shutdown)                     W1.3
    driver/glib.rs              Linux: GLib default-context service + `glib-fd-watch` thread, joined on stop      W1.3
    engine.rs                   CefEngine: impl vsesvit_engine::Engine; every method is one line that calls
                                the owning module below (find::find, download::cont, …)                           W1.3
    registry.rs                 the main-thread registry: browsers by TabId and browser id, kinds, in-flight
                                creations, prompt callbacks, hooks (§1.6); the live count for shutdown (§3.6)     W1.3
    events.rs                   Sink: unbounded sender of BrowserEvent (any thread)                               W1.3
    client.rs                   one Client per browser kind and Browsing, handlers built once, returned by clone (G8)   W1.3
    handlers/life_span.rs       on_before_popup, on_after_created, on_before_close, popup-abort accounting        W1.3
    handlers/{display,load,keyboard,command,focus,request}.rs   (request: on_before_browse, on_open_urlfrom_tab,
                                certificate and render-process callbacks; it delegates auth and resource
                                requests to the files below)                                                      W1.3
    handlers/favicon.rs         on_favicon_urlchange → download_image → get_as_png (display.rs delegates)          W3.2
    handlers/find.rs            find, stop_finding, on_find_result                                                 W2.4
    handlers/context_menu.rs    run_context_menu → owned MenuModel prompt (§1.5)                                  W2.5
    handlers/{jsdialog,dialog,auth}.rs   JS dialogs and beforeunload, file chooser, get_auth_credentials (IO)      W2.6
    handlers/permission.rs      permission and media-access prompts (§1.5)                                        W3.1
    handlers/{resource_request,cookie_access}.rs   IO-thread filters over the privacy snapshot                     W3.1
    handlers/download.rs        can_download, on_before_download, on_download_updated, keep-alive (§4.2)          W3.3
    prompts.rs                  PromptId allocation, the callback registry's take-once rule, cancellation (§1.5)  W1.3
    contexts.rs                 RequestContext per Browsing; the private context's lifetime                       W1.3
    native.rs                   window_info_child(parent, rect, scale) and popup window_info fix-ups, per OS (cfg) W1.3
    idc.rs                      IDC name table (from cef_id_for_command_id_name) and the default-deny map (SC §8.2)  W1.3
    devtools.rs                 execute_dev_tools_method + one DevToolsMessageObserver; RequestId <-> message id  W1.3
    hidden.rs                   the maintenance browser and short-lived management browsers in the hidden host (§3.7)   W1.3
    site_data.rs                Storage.clearDataForOrigin and the cookie calls; clear at start and at exit (§3.7)   W3.1
    prefs.rs                    apply_prefs → RequestContext::set_preference, color scheme                         W3.5
    lifecycle.rs                set_frozen (Page.setWebLifecycleState), the audio script (Q9, Q11)                W3.8
    schemes.rs                  vsesvit:// handler factory: new tab page, error and warning pages (IO)            W2.5
    ext/                        extension bridge, External Extensions writer, uninstall machine (§5)              W4.2
    strings.rs                  G3/G4/G5 helpers: owned out-param strings, non-null error buffers, take() for lists   W1.3
    teardown.rs                 Running -> Closing -> Drained -> Shutdown, or Stuck (G16, §3.6)                   W1.3
```

**Thread affinity per method** (API §1.4, §3.5; CEF 154 header comments). "UI" is the main thread (§3.4).

| Interface | UI | IO | Any thread |
|---|---|---|---|
| `Client` getters (`get_*_handler`) | | | yes: build nothing there, return clones (G8) |
| `BrowserProcessHandler` | `on_context_initialized`, `get_default_client`, `on_already_running_app_relaunch` | | `on_schedule_message_pump_work` |
| `LifeSpanHandler` | all | | |
| `RequestHandler` | `on_before_browse`, `on_open_urlfrom_tab`, `on_certificate_error`, `on_render_process_terminated` | `get_resource_request_handler`, `get_auth_credentials` | |
| `ResourceRequestHandler`, `CookieAccessFilter`, scheme handlers | | all | |
| `Display`, `Load`, `Keyboard`, `Focus`, `JsDialog`, `Dialog`, `Download`, `Permission`, `ContextMenu`, `Find`, `CommandHandler`, `DevToolsMessageObserver` | all | | |
| callbacks we hold (`*Callback::cont`, `cancel`) | we call them on UI only, even where CEF allows any thread | | |

IO-thread methods read only `Arc` snapshots and atomics, and reach the shell only by sending an event; an IO method that needs a shell answer (auth) posts to UI with `post_task(TID_UI)` and keeps the callback in the prompt registry there.

### 1.3 `vsesvit-gpui` module tree

UA §5.1 stands, with these changes:

| UA path | Change |
|---|---|
| `platform/{mod,x11,watch,capture,activation,dirs,portal,default_browser}.rs` | replaced by the tree below |
| `boot/cli.rs` (a port of GTK `cli.rs`) | thin: parsing moves to `vsesvit_core::cli` (§4.3) |
| `state/closed_tabs.rs`, `state/session.rs` plan part | use `vsesvit_core::closed_tabs`, `vsesvit_core::session::plan` (§4.3) |
| `overlay/{popup,cutout}.rs` | `overlay/{inline,window}.rs`: in-window overlay with a host hole (default), PopUp window (leaves the window) |
| `engine_bridge/focus.rs` | the one caller of both `platform::Surface::focus` and `Engine::focus_page`/`set_page_focus` (§1.5) |
| `self_test/desktop.rs` | gone: the guard is `vsesvit_testenv::require()` (§7.3), called by every browser-process entry point after the sub-process return and before any window |
| `examples/x11_probe.rs` (UA §5.1, U03) | replaced by `examples/native_probe/` (one probe, both OSes, W0.1/W1.1/W1.2); `x11_probe.rs` is never created (R2-2) |
| `platform/dirs.rs` (Downloads folder) | `vsesvit_core::os::downloads_dir` (§4.3 row 15 gains the Linux half: xdg-user-dirs) |

```
crates/vsesvit-gpui/src/platform/
  mod.rs          NativeSurface + Desktop traits, PhysRect/PhysPoint/Hole, `pub type Surface`, `pub type Os`,
                  fatal_message(text) for startup failures (stderr, plus a message box on Windows)       W1.1
  x11/            mod.rs (one x11rb connection, error drain), surface.rs (24-bit content host, ConfigureWindow,
                  map/unmap, SHAPE holes, aux hosts), focus.rs (focus proxy lookup, XSetInputFocus, is_within guard),
                  capture.rs (GetImage, ancestor-SHAPE-aware fallback), desktop.rs (transient-for, top-level at point,
                  pointer presses via XI2 raw events, startup id, the hidden host, neutralize), instance.rs
                  (Unix socket), scale.rs (one scale rule)                                                W1.1
  windows/        mod.rs, surface.rs (content host HWND, SetWindowPos/ShowWindow deferred out of GPUI updates,
                  SetWindowRgn holes, aux hosts), focus.rs (SetFocus, refused while inactive), capture.rs
                  (PrintWindow PW_RENDERFULLCONTENT), desktop.rs (owner fix-up for PopUp and dialog windows,
                  WindowFromPoint, WH_MOUSE thread hook for pointer presses, AllowSetForegroundWindow, the hidden
                  host, neutralize), instance.rs (named pipe), console.rs (AttachConsole for headless commands),
                  frame.rs (caption buttons, snap layouts, resize hit-testing where the content host meets the
                  border: `WM_NCHITTEST` on the GPUI window, the host clipped one border width in)            W1.2
  fake.rs         FakeSurface, FakeDesktop. `#[cfg(any(test, feature = "test-support"))]`, a separate type that
                  never replaces the `Surface` alias (below)                                               W1.1
  files.rs        file dialogs with filters: ashpd portal on Linux, `GetOpenFileNameW`/`GetSaveFileNameW` on
                  Windows (GPUI's prompt has no filters, GP §2.10). `OpenFolder` needs no filter and uses GPUI's
                  own `prompt_for_paths` with `directories: true` on both OSes, because `GetOpenFileNameW`
                  cannot pick a folder (R2-13)                                                              W2.6
  shell_open.rs   open a file or folder, reveal in the file manager, per OS                                W3.3
  default_browser.rs   state and make-default, per OS (Settings > General row on both)                     W3.5
```

**The hidden host.** `Desktop` owns one process-wide hidden native window, created at `Desktop::init` and destroyed after the last browser closed (§3.6). X11: an unmapped 24-bit toplevel on our own connection. Windows: a hidden top-level HWND owned by nobody (not message-only; CEF needs a real parent). It parents the maintenance and management browsers (§3.7), download keep-alive browsers (§4.2) and neutralized Chrome-created windows (§5.7). It never depends on which browser window is open (Opus review 16).

### 1.4 The platform seam

All native window work is in `vsesvit-gpui::platform`. Each OS has one implementation, chosen by `cfg` in `platform/mod.rs`. The traits exist so both backends must compile against the same signatures. The shell holds a window's surface as `Box<dyn SurfaceOps>` and the desktop as `Os`, never as a generic parameter (`BrowserWindow<S>` from SW §13.2 would spread the parameter through every view for one implementation per build). The trait object is also the seam tests use, so no feature has to swap the alias (below).

```rust
// platform/mod.rs
pub use vsesvit_engine::{NativeHandle, PhysRect};      // physical px, relative to the GPUI window's client area
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub struct PhysPoint { pub x: i32, pub y: i32 }   // screen px; never in the engine
                                                       // contract, which has its own page-relative PagePoint (§1.5)
/// A rectangle over the content rect that GPUI paints into. `interactive` holes also take the mouse.
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub struct Hole { pub rect: PhysRect, pub interactive: bool }
pub enum FocusTarget { Chrome, Page(NativeHandle) }
pub enum FocusOutcome { Done, Refused }                 // Refused: our toplevel is not the active window
pub struct AuxHost(NativeHandle);                       // a second host above the content host (extension popups)

/// One per GPUI browser window. Main thread only. Calls that send messages to the GPUI window
/// are deferred by the implementation out of the current GPUI update (SW §5 resize trap).
pub trait NativeSurface: Sized + 'static {
    /// Windows: GPUI's DirectComposition target is topmost, so the content rect must stay transparent (SW §5).
    const TRANSPARENT_CONTENT: bool;
    fn attach(window: &gpui::Window) -> Result<Self, PlatformError>;   // creates the content host
    fn host(&self) -> NativeHandle;                                    // parent for every tab's CEF child
    fn set_content_rect(&mut self, rect: PhysRect);                    // host, then the shown page at 0,0,w,h
    fn show_page(&mut self, page: NativeHandle, visible: bool);        // map/unmap or SW_SHOWNA/SW_HIDE
    fn set_holes(&mut self, holes: &[Hole]);                           // X11: SHAPE of all; Windows: region of interactive ones
    fn focus(&mut self, target: FocusTarget) -> FocusOutcome;
    fn move_page_to(&mut self, page: NativeHandle, to: &mut Self) -> Result<(), PlatformError>;  // tab to another window
    /// Download keep-alive (§4.2): hide the page and reparent it into Desktop's hidden host. Returns only after
    /// the native reparent is done, so the caller may then detach this surface.
    fn park_page(&mut self, page: NativeHandle, hidden_host: NativeHandle) -> Result<(), PlatformError>;
    fn open_aux(&mut self, rect: PhysRect) -> Result<AuxHost, PlatformError>;
    fn set_aux_rect(&mut self, aux: &AuxHost, rect: PhysRect);
    fn close_aux(&mut self, aux: AuxHost);
    fn capture(&self) -> Result<image::RgbaImage, PlatformError>;    // whole window, page included
    fn detach(self);                                                   // after every page of this window closed
}

/// Process-wide. Main thread only.
pub trait Desktop: Sized + 'static {
    fn init() -> Result<Self, PlatformError>;
    fn adopt_popup(&self, popup: &gpui::Window, owner: &gpui::Window);   // Windows: owner + clear TOPMOST (SW §9.2)
    fn set_dialog_parent(&self, dialog: &gpui::Window, parent: &gpui::Window); // X11 WM_TRANSIENT_FOR; Windows owner
    fn toplevel_at(&self, at: PhysPoint) -> Option<NativeHandle>;        // our browser window under the pointer
    fn pointer_presses(&self) -> futures::channel::mpsc::UnboundedReceiver<PhysPoint>;  // outside-click dismissal
    fn raise_for_invocation(&self, window: &gpui::Window, token: Option<&str>);         // forwarded launch
    fn hidden_host(&self) -> NativeHandle;                               // see "The hidden host" (§1.3)
    /// Synchronous and App-free: called from inside `on_after_created` through `ShellHooks::neutralize_foreign`.
    /// X11: unmap, then reparent into the hidden host. Windows: SW_HIDE, then SetParent into the hidden host.
    /// An error (the window is gone, a call failed) makes the glue close the foreign browser at once (§5.7).
    fn neutralize(&self, window: NativeHandle) -> Result<(), PlatformError>;
}

#[cfg(target_os = "linux")] pub type Surface = x11::XSurface;
#[cfg(target_os = "linux")] pub type Os = x11::XDesktop;
#[cfg(windows)]             pub type Surface = windows::WinSurface;
#[cfg(windows)]             pub type Os = windows::WinDesktop;
```

**Fakes never replace the real backend.** The aliases depend on the OS only. A cargo feature that swapped them would not be additive: `--all-features`, or any workspace build in which one crate's dev-dependency enables it, would build a `vsesvit` binary that starts and never shows a page (Opus review 24). So the code that needs a fake is generic where it touches the surface: `BrowserWindow` holds a `Box<dyn SurfaceOps>` (`SurfaceOps` is the object-safe part of `NativeSurface`, implemented for `Surface` and for `FakeSurface`), built by `platform::attach(window)` in the binary and by `fake::attach(window)` in tests. `fake.rs` compiles under `cfg(test)` for vsesvit-gpui's own tests and under the `test-support` feature for other crates' tests. `test-support` only adds the fake types; it changes no alias and no default, so turning it on in a shipped build changes nothing that runs. `xtask check-deps` also rejects `test-support` in any crate's normal (non-dev) dependencies. CI builds `cargo build --workspace --all-features` and runs the foundation self-test on that binary (§6.3), which catches a regression of this rule.

What differs per OS, and where (SW §13 table, extended):

| Concern | X11 (`platform/x11`) | Windows (`platform/windows`) | Source |
|---|---|---|---|
| Content host | 24-bit `InputOutput` window, root visual, default colormap, border 0, selects no events | plain child HWND, `WS_CHILD \| WS_CLIPCHILDREN \| WS_CLIPSIBLINGS` | SC §5.2, SW §5 |
| GPUI under the page | irrelevant; children are above | transparent: `WindowBackgroundAppearance::Transparent`, no theme background behind the content element | SW §5, §14.2 |
| Geometry, show/hide | `ConfigureWindow`, map/unmap, from the content element's prepaint | `SetWindowPos`, `ShowWindow(SW_HIDE/SW_SHOWNA)`, spawned out of the GPUI update | SC §5.3, SW §5 |
| Overlay holes | SHAPE bounding+input on the host, for every overlay (paint and input) | `SetWindowRgn` on the host, for interactive overlays only; paint is free | SC §9.1, SW §9.1 |
| Focus | `XSetInputFocus` to GPUI's focus proxy (patched GPUI) or the page child; acts only when the X focus is inside our toplevel | `SetFocus`; refused while our toplevel is inactive; never `activate_window` (it calls `SendInput`) | SC §7.3, SW §7, §13.3 |
| Chromium's own focus requests | refused in `on_set_focus` unless the tab's page-focus policy allows it (§1.5) | same; the policy starts refused on every new browser (SW §1.1, §14.5) | SW §1.1, `spikes/win-core/src/cefglue/client.rs` |
| PopUp windows | work as is (override-redirect) | owner = browser window, clear `WS_EX_TOPMOST` after creation | SC §9.2, SW §9.2 |
| Dialog parent | `WM_TRANSIENT_FOR` set by us (GPUI picks none while the page has focus) | owner via `GWLP_HWNDPARENT` | UA §1.8 |
| Top-level at point | `QueryPointer` + stacking order of root children | `WindowFromPoint` + `GetAncestor(GA_ROOT)`, class `Zed::Window`, our pid | UA §1.6 |
| Pointer presses | XI2 raw button events on root, own connection, watcher thread | `WH_MOUSE` hook on the main thread (in-process only) | UA §1.7 |
| Screenshot | one `GetImage` of the toplevel (5–9 ms); per-child fallback on non-compositing Xorg | `PrintWindow(PW_RENDERFULLCONTENT)` (about 20 ms) | SC §11, SW §11 |
| Scale | one scale for both sides: `GPUI_X11_SCALE_FACTOR` + `--force-device-scale-factor` | per-monitor-v2; each side reads the monitor DPI | SC §5.4, SW §5 |
| Single instance | Unix socket `$XDG_RUNTIME_DIR/vsesvit/<hash>.sock`, `SO_PEERCRED` | named pipe `\\.\pipe\vsesvit-<hash>`, same-user DACL; client calls `AllowSetForegroundWindow` for the server's pid | UA §1.2 |
| Raising on a forwarded launch | `_NET_ACTIVE_WINDOW` with the forwarded startup id | `SetForegroundWindow` after the client granted the right | UA §1.10, SW §1.1 |
| Pump extras | GLib service + fd watcher (in `vsesvit-cef/driver/glib.rs`) | none; GPUI's `GetMessageW` loop dispatches Chromium's HWND messages (needs the key patch) | SC §4, SW §4 |
| GPUI patch | `gpui-pre-linux` focus patch | `gpui-pre-windows` key patch | SC §7.2, SW §4 |

The pump is the one OS difference outside `platform`: it lives in `vsesvit-cef/driver`, because the GLib sources it services are Chromium's (SC §4.1).

### 1.5 Changes to the engine contract (UA §4.2)

UA §4.2 stays the contract, with these edits:

```rust
// vsesvit-engine
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)] #[repr(transparent)]
pub struct NativeHandle(u64);                // an X11 XID or an HWND; constructors per OS
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct PhysRect { pub x: i32, pub y: i32, pub width: u32, pub height: u32 }
/// Physical pixels relative to the top-left of a page's view. The glue converts CEF's view coordinates
/// (`get_xcoord`: "relative to the associated RenderView's origin") with the browser's scale factor;
/// the shell adds the content rect's origin. Distinct from the platform's screen `PhysPoint` (R2-13).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct PagePoint { pub x: i32, pub y: i32 }

pub struct TabSpec {
    pub tab: TabId, pub kind: TabKind, pub browsing: Browsing,
    pub parent: NativeHandle,                // the window's content host, or an AuxHost for extension popups
    pub bounds: PhysRect, pub scale_factor: f32, // the glue converts to what CEF wants (DIP on Linux, SC §5.3)
    pub visible: bool, pub page: InitialPage, pub opener: Option<TabId>,
}
pub enum TabKind { Page, ExtensionPopup { min: (u32, u32), max: (u32, u32) } }   // ExtensionPopup = Alloy style (EA §7.2)

pub enum TabEvent {
    Created { native: NativeHandle },
    Detached,                                // after `detach_tab`: the tab is gone, its browser lives on parked (§4.2)
    /* … the rest of UA §4.2 … */
}
pub enum PopupDecision { Block, AllowAsTab { tab: TabId, parent: NativeHandle, bounds: PhysRect, scale_factor: f32 } }
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)] pub struct ParkedId(pub u64);

pub trait Engine {
    // removed: set_bounds, set_occlusions, reparent  (the shell's platform does these on the page handle)
    fn set_visible(&self, tab: TabId, visible: bool);         // was_hidden only
    fn notify_moved(&self, tab: TabId);                       // notify_move_or_resize_started: closes <select> popups
    fn notify_screen_info_changed(&self, tab: TabId);         // scale change (SC §5.4, SW §5)
    fn set_hidden_host(&self, host: NativeHandle);            // once, at app start, before any tab (§1.3, §3.7)
    fn set_page_focus(&self, tab: TabId, focus: PageFocus);   // the focus policy below
    fn set_frozen(&self, tab: TabId, frozen: bool);           // Memory Saver (Q9): Page.setWebLifecycleState
    fn answer_prompt(&self, id: PromptId, answer: PromptAnswer) -> Result<(), PromptError>;
    /// The shell has already moved the page into the hidden host (§4.2). The tab ends (`TabEvent::Detached`);
    /// the browser stays counted until its last download ends, then the glue closes it.
    fn detach_tab(&self, tab: TabId) -> Result<ParkedId, EngineError>;
    /// Shutdown, two phases (§3.6). They replace UA's `close_all` and `AllTabsClosed`.
    fn begin_shutdown(&self);                                 // Running → ClosingUser; → BrowserEvent::UserBrowsersClosed
    fn close_internal(&self);                                 // ClosingUser → ClosingInternal; → BrowserEvent::AllBrowsersClosed
    // removed: close_all, answer_permission (now a prompt), sync_extensions, loaded_extensions;
    // added: the extension calls of §5.4
    /* … the rest of UA §4.2 … */
}

enum BrowserEvent {
    UserBrowsersClosed,                       // every user browser has run on_before_close (§3.6 step 3)
    AllBrowsersClosed,                        // the live count is zero (§3.6 step 6)
    /// A message from a page, sent through the renderer process. `origin` and `main_frame` come from the browser
    /// process's own frame record (`on_process_message_received`'s `frame`), never from the payload.
    PageMessage { tab: TabId, origin: Origin, main_frame: bool, channel: PageChannel, payload: String },
    /* … Prompt and PromptEnded below, the rest of UA §4.2 … */
}
pub enum PageChannel { Store }               // the Web Store shim (§5.7, A.12)
```

**Page messages** (A.12). The store shim of §5.7 needs a page → shell channel whose sender the shell can trust. Console messages are not one: any script can write any text. The channel is CEF's process messages. In the renderer, `vsesvit-cef-child`'s `RenderProcessHandler::on_context_created` binds `vsesvit.post(json)` in a main frame only when the frame's origin is on the channel's allow list, which the browser passes to the renderer in `extra_info` at browser creation. The function calls `frame.send_process_message(PID_BROWSER, "vsesvit.store", json)`. In the browser process, `Client::on_process_message_received` takes the origin from the `frame` argument, checks it against the same list again (a compromised renderer can send anything) and emits `PageMessage`. A message from another origin or a subframe is dropped and logged. The shell's answer goes back with `engine.eval` on that tab, after the shell has checked the tab's current origin again. W0.2 freezes the event; W1.3 builds the generic channel in `vsesvit-cef-child` and `client.rs`; W4.3's shim is its first user.

**Prompts: one request/answer protocol.** Every question Chromium asks and waits on goes through one shape. It replaces UA's `PermissionRequested`/`answer_permission`/`PermissionWithdrawn`, `ShellHooks::context_menu_items` and `ContextMenuCommand`, and adds what UA lacked (GPT review 4; SW §10 and §14.9 show the native fallbacks otherwise).

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)] pub struct PromptId(pub u64);

pub enum PromptRequest {
    ContextMenu { tab: TabId, at: PagePoint, params: ContextMenuParams, model: MenuModel },
    JsDialog { tab: TabId, origin: Option<Origin>, kind: JsDialogKind, message: String },
    BeforeUnload { tab: TabId, is_reload: bool },
    FileChooser { tab: TabId, mode: FileChooserMode, title: Option<String>, default_path: Option<PathBuf>,
                  accept: Vec<AcceptFilter> },
    /// `requester` is CEF's `origin_url`, the page that made the request; `challenger` is the server or proxy
    /// that sent the challenge (CEF's `host`, `port`, `isProxy`). The dialog names the challenger (R2-3).
    Auth { tab: Option<TabId>, requester: Option<Url>, challenger: AuthChallenger,
           realm: Option<String>, scheme: Option<String> /* CEF: null for FTP */ },
    Permission(PermissionRequest),                              // UA §4.2's struct, without its own id
}
pub struct AuthChallenger { pub host: String, pub port: u16, pub is_proxy: bool }
pub enum JsDialogKind { Alert, Confirm, Prompt { default: String } }
pub enum FileChooserMode { Open, OpenMultiple, OpenFolder, Save }
pub struct AcceptFilter { pub label: Option<String>, pub patterns: Vec<String> /* "image/*", ".png" */ }

/// An owned, engine-neutral copy of Chromium's menu, extension items included (EA §5).
pub struct MenuModel { pub entries: Vec<MenuEntry> }
pub enum MenuEntry {
    Command { id: MenuCommandId, label: String, enabled: bool, checked: Option<bool>, radio_group: Option<i32> },
    Submenu { label: String, enabled: bool, entries: Vec<MenuEntry> },
    Separator,
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub struct MenuCommandId(pub i32);   // the engine's id, never invented by the shell

pub enum PromptAnswer {
    Menu(Option<MenuCommandId>),                // None: dismissed, or the user chose a shell item (the shell runs it)
    JsDialog { accept: bool, text: Option<String> },
    BeforeUnload { leave: bool },
    Files(Option<Vec<PathBuf>>),
    Auth(Option<(String, String)>),
    Permission(PermissionAnswer),
}
pub enum PromptEnd { Navigated, TabClosed, Withdrawn, EngineReset, MenuDismissed, Shutdown }
pub enum PromptError { Unknown, WrongKind, AlreadyEnded }

enum BrowserEvent {
    Prompt { id: PromptId, request: PromptRequest },
    PromptEnded { id: PromptId, why: PromptEnd },       // the engine cancelled it; any later answer is AlreadyEnded
    /* … */
}
```

Rules, enforced in `vsesvit-cef/src/prompts.rs` and mirrored by `FakeEngine`:
- The glue keeps each CEF callback in the main-thread registry under its `PromptId`. The shell never sees a CEF type.
- **Scope.** Every prompt belongs to one browser and one document. The glue keeps two values per browser that any thread can read: `generation: Arc<AtomicU64>`, raised on main when the main frame commits a new document, and `closing: Arc<AtomicBool>`, set when the glue calls `close_browser` and in `on_before_close`. A prompt's scope is (browser id, generation) at the moment CEF asked.
- **Admission.** A prompt enters the registry only through `prompts::admit(scope, request, callback)`, on main. Admission refuses when the browser is closing or gone, when its generation differs from the scope's, or when teardown has left Running (§3.6). A refused prompt never reaches the shell: the glue runs its cancel action (table below) at once. UI-thread prompts are admitted inside their handler. `get_auth_credentials` arrives on IO: it reads the scope from the atomics, returns true, and posts the request to UI, where the task admits it. A tab that closes or navigates before that task runs therefore never shows a stale prompt (R2-3).
- **Exactly once.** `answer_prompt` first looks the entry up. An unknown id gets `Unknown`, an ended one `AlreadyEnded`. A wrong answer kind gets `WrongKind` and leaves the entry in place, so the right answer can still follow. Otherwise it takes the entry out and calls the answer action. An end takes it the same way. Whichever comes first wins. An entry still present when shutdown begins ends with `Shutdown`.
- **Ends.** The tab's main frame commits a new document (`Navigated`); the browser closes (`TabClosed`); `on_dismiss_permission_prompt` (`Withdrawn`); `JsDialogHandler::on_reset_dialog_state` or `on_dialog_closed` (`EngineReset`); `on_context_menu_dismissed` before an answer (`MenuDismissed`; CEF calls it after every menu, "irregardless of whether the menu was canceled or a command was selected", so after an answer it finds no entry). After an engine-side end (`Withdrawn`, `EngineReset`, `MenuDismissed`) CEF has already given up the callback, so the glue drops it without calling it. Every other end runs the cancel action.
- **Thread.** Every prompt is admitted, answered and ended on main.

| Kind | Answer action | Cancel action |
|---|---|---|
| ContextMenu | `Menu(Some(id))` → `cont(id, 0)`; `Menu(None)` → `cancel()` | `cancel()` |
| JsDialog | `cont(accept, text)` | `cont(false, "")` |
| BeforeUnload | `cont(leave, "")` | `cont(false, "")`: the page stays |
| FileChooser | `Files(Some(paths))` → `cont(paths)`; `Files(None)` → `cancel()` | `cancel()` |
| Auth | `Auth(Some((user, pass)))` → `cont(user, pass)`; `Auth(None)` → `cancel()` | `cancel()` |
| Permission | `cont(result)` as UA §4.2 maps it | permission prompt: `cont(DISMISS)`; media access: `cancel()` |
- **Menus.** The shell shows `model` plus its own items (UA §2.3's list: open link in private window, copy clean link, search selection, print, view source) in the GPUI menu (W2.1). An engine item answers `Menu(Some(id))`, which the glue passes to `callback.cont(id, 0)`, so extension items keep their dispatch and `activeTab` grant (EA §5). A shell item answers `Menu(None)` and then runs in the shell. `params` keeps everything the shell items need.
- `FakeEngine` gets `push_prompt(request) -> PromptId`, `queue_io_prompt(tab, request) -> Queued` and `admit(Queued)` (so a test can close or navigate the tab in between), `commit_document(tab)`, records answers, and lets a test end a prompt with any `PromptEnd`. W0.2's tests cover, for every kind: answer, cancel-then-answer, answer-then-cancel, an unknown id, a duplicate answer (`AlreadyEnded`), a wrong kind (`WrongKind`, then the right answer succeeds). Also: an IO prompt admitted after its tab closed, after a new document, and after `begin_shutdown` (never shown; cancel action run); a menu ended by `MenuDismissed` and answered afterwards (`AlreadyEnded`, callback not called). W1.3 and W2.6 run the same sequences against the real glue.

**The page-focus policy.** Chromium focuses new browsers and navigations itself, which steals the foreground and the omnibox's keyboard (SW §1.1, §14.5). `Surface::focus` cannot stop that, because Chromium's own requests never call it (GPT review 9). The shell owns one policy per tab, as the three facts the decision needs:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PageFocus { pub window_active: bool, pub tab_selected: bool, pub keyboard: KeyboardOwner }
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyboardOwner { Chrome, Page, Overlay }      // Overlay: an interactive overlay or an extension popup has it
impl PageFocus {
    pub const NEW_BROWSER: Self = Self { window_active: false, tab_selected: false, keyboard: KeyboardOwner::Chrome };
    pub fn allows(self) -> bool { self.window_active && self.tab_selected && self.keyboard == KeyboardOwner::Page }
}
```

- `engine_bridge/focus.rs` publishes it with `set_page_focus` synchronously: `NEW_BROWSER` before `create_tab`, then before every `load`, and on every activation, tab switch and keyboard-owner change.
- The glue stores it in the registry. `on_set_focus` cancels every request for which `allows()` is false, whatever its source. There is no exception for `FOCUS_SOURCE_SYSTEM`: CEF defines it as "a system-generated focus event" (`cef_focus_source_t`), which also covers programmatic focus changes, so it says nothing about a click (R2-11).
- **A click hands the page the keyboard through the shell.** A press reaches `focus.rs` through `Desktop::pointer_presses` (XI2 raw presses on X11, the `WH_MOUSE` hook on Windows; §1.4). A press inside the selected tab's page rect in the active window sets `keyboard = Page`, publishes the policy, then calls `Surface::focus(Page)` and `focus_page`. Chromium's own request from the same click may come first and be refused; the shell's focus follows within one turn of the main loop. The mouse event itself reaches the page either way, because input does not need focus.
- **Focus that bypasses `on_set_focus`.** On X11 a click makes Chromium take X focus itself (UA §1.6), so `TabEvent::FocusChanged { focused: true }` can arrive before the press does. When it arrives for a tab whose last published policy does not allow focus, `focus.rs` first drains `pointer_presses`. A press into that page, pending or less than 500 ms old, makes it the click hand-off above. Otherwise `focus.rs` gives the keyboard back to its owner (`Surface::focus(Chrome)`, or the overlay) and logs it. W1.6 tests both orders.
- `focus.rs` is the only caller of `focus_page`/`blur_page`, and it calls `focus_page` only after `Surface::focus` returned `Done`, so there is one focus path, not two.

`allows()` is pure, so W0.2 tests it over every combination. The `page_focus_policy` self-test check (§7.2) covers a background tab, a background navigation, an inactive window, the omnibox focused, an extension popup focused, a programmatic focus into the page (Windows `SetFocus` on the page's HWND, X11 `XSetInputFocus` on the page's window) while the active window's omnibox has focus, and a real click into a page text field after which typed keys reach it.

**Hooks.** `ShellHooks` keeps `navigation_policy` and `popup_policy`, loses `context_menu_items` (now a prompt), and gains `neutralize_foreign(window: NativeHandle) -> Result<(), NeutralizeFailed>` (§5.7). `NeutralizeFailed { reason: String }` is an engine type; the shell maps its `PlatformError` into it, so the contract names no platform type. Hooks read only snapshots: the HookSnapshot of UA §4.3 and the privacy snapshot (§4.3 row 11). They never borrow core, so a core borrow held by the caller can no longer turn `try_borrow_mut` into a silent `Allow` (Opus review 5).

`EngineConfig` gains the paths and switches of §3.2 and §3.3. The Windows sandbox pointer is not part of the contract: `CefEngineFactory::new(Sandbox)` carries it.

**What W0.2 freezes, and what waits.** W0.2 freezes everything above and the rest of UA §4.2: `PagePoint`, the prompt kinds with `AuthChallenger`, scope, admission and the per-kind actions, `PageFocus`, `detach_tab` and `TabEvent::Detached` (§4.2), the two shutdown phases and their late-creation rules (§3.6), and `PageMessage`. Restart intent is not part of the engine contract; it lives in the shell (§3.6). The extension part of the contract (§5.4: `ExtensionCall`, `ExtensionEvent`, `TabKind::ExtensionPopup`, `ForeignWindowOpened`) lives in its own file, `vsesvit-engine/src/extension.rs`, and stays provisional until the extension spike WX.0 reports (§5.10, §8.2). WX.1 then freezes it. No unit before WX.1 codes against it except FakeEngine stubs.

### 1.6 Ownership and threads

| Thread | Runs | Owns | Rule |
|---|---|---|---|
| main | GPUI's loop (calloop on Linux, `GetMessageW` on Windows); CEF's UI thread inside `do_message_loop_work`; on Windows every Chromium browser HWND; on Linux Chromium's GLib sources | `vsesvit_core::Profile` (`Rc<RefCell<…>>` in the `Shell` global, UA §1.4), every entity, `Rc<dyn Engine>`, `Surface`s, `Desktop`, the glue's registry | the re-entrancy rules below |
| GPUI background executor | core's slow `Send` jobs: installs, favicon fetch, suggest, sync rounds, update download/install, imports | the job values | result committed on main through `cx.spawn` (UA §1.3) |
| GPUI timer thread | wakes | — | — |
| `glib-fd-watch` (Linux) | `poll(2)` on GLib's fds, wakes the pump task | eventfd | stays disarmed until main re-arms (SC §4.1) |
| CEF IO and other CEF threads | `resource_request`, `cookie_access`, scheme handlers, `on_schedule_message_pump_work` | `Arc` snapshots (the privacy snapshot, `Arc<dyn RequestFilter>`), atomics, the event `Sink` | never `Profile`; only atomics, Mutexes, channels (G6) |
| instance accept thread | blocking accept on the socket or pipe | — | started after `cef::initialize` (UA §1.2 step 7) |
| X11 watcher thread (Linux) | XI2 raw presses on our own connection | — | results through a channel |

**Re-entrancy.** CEF callbacks do not only run inside `do_work`. They also run synchronously inside engine calls the shell makes (`host.set_focus(1)` sends `WM_SETFOCUS` and fires `on_got_focus`; `close_browser` can fire `on_before_close`; a browser-initiated `load_url` can run `on_before_browse`, inferred, checked in W1.3), and on Windows inside GPUI's own `GetMessageW` dispatch, because Chromium's HWNDs live on the main thread (SW §4, §13.3). Any of these can happen inside a GPUI update. The design therefore assumes nothing about where a callback runs, and keeps four rules (Opus review 5):
1. A handler never touches `gpui::App`. It reads the registry or a snapshot and sends a `BrowserEvent`; the router delivers it later from its own task.
2. The glue never holds a borrow of its own state across a CEF call. It clones the `Browser` out of the registry, releases the borrow, then calls.
3. The shell never holds a core borrow across an engine call (inv/shell-core §5), and hooks never borrow core at all (§1.5).
4. `Engine` methods may be called from inside a handler-triggered path; each is written to be re-entrant under rule 2.

W1.3 adds a test that calls every `Engine` method from inside every UI-thread handler path it can trigger, and W1.2's probe issues a `SetFocus` into a foreign child from inside a GPUI update.

**The registry, not a `Send` cell.** Handler objects are reference-counted by CEF, and their last release, with the drop of every field, can happen on any thread (API §1.3). So no handler holds non-`Send` state, not even behind a checked cell (GPT review 5, Opus review 6). Handlers hold only `Send + Sync` values: their `TabId` or browser kind, the event `Sink`, `Arc` snapshots and atomics. Everything else lives in `registry.rs`: a `thread_local!` registry on the main thread, keyed by `TabId` and CEF browser id, that holds the `Browser` handles, the `Rc<dyn ShellHooks>`, the prompt callbacks and the page-focus policy. `registry::with(|r| …)` first compares the current thread with the main thread id recorded at `initialize`. On any other thread it logs once and returns `None`; it never panics, so nothing unwinds through FFI. The registry is emptied on main in shutdown step 6, before `cef::shutdown` (§3.6). There is no `unsafe impl Send` or `Sync` anywhere in `vsesvit-cef`; `xtask check-deps` greps for one.

## 2. Dependencies, versions and vendored GPUI patches

### 2.1 Versions

All exact pins are the versions the four spikes built and ran with (SC §2, SW §2, code survey of `spikes/*/Cargo.toml`). Each is declared once in `[workspace.dependencies]`, so a bump is one line plus a re-run of the checks in §7.

| Dependency | Pin | Used by | Notes |
|---|---|---|---|
| `cef` | `=154.5.0` (lock `154.5.0+154.0.34`), `default-features = false, features = ["resources"]` | vsesvit-cef | CEF 154.0.34, Chromium 154.0.8037.98. `CEF_PATH` must hold exactly 154.0.34 (G2). The `dox` feature is for `cargo check` without the download (G14) |
| `gpui` = `gpui-pre` | `=0.3.9` | vsesvit-gpui | the lib name stays `gpui` (GP §1.6) |
| `gpui_platform` = `gpui-pre-platform` | `=0.3.9`, `default-features = false`; `features = ["x11"]` on Linux | vsesvit-gpui | Linux: the Wayland backend is compiled out, and `gpui_platform::linux(WindowingModes::X11)` refuses Wayland at run time (SC §2) |
| `gpui-component` | `=0.7.3`, `default-features = false` | vsesvit-gpui | |
| `gpui-kit-assets` | `=0.7.3` | vsesvit-gpui | Lucide icons, ISC (UA §3.4) |
| `x11rb` | `0.13`, features `shape`, `xinput`, `resource_manager`, `allow-unsafe-code` | vsesvit-gpui, vsesvit-testenv (Linux) | `xtest` only in `vsesvit-testenv`, whose `PrivateInput` can be built only from a validated `TestEnv` (§7.3). vsesvit-gpui's `self-test` feature (`default = ["self-test"]`, `self-test = ["vsesvit-core/testkit", "dep:vsesvit-testenv"]`, as GTK's feature today) pulls it in |
| `windows-sys` | `0.61` | vsesvit-gpui, vsesvit-sandbox (Windows) | same major as core and vsesvit-update today. Features: Foundation, UI_WindowsAndMessaging, Graphics_Gdi, UI_HiDpi, UI_Input_KeyboardAndMouse, System_StationsAndDesktops, System_Pipes, System_Console, System_Threading, Storage_FileSystem, UI_Shell, Security |
| `raw-window-handle` | `0.6` | vsesvit-gpui | call `HasWindowHandle::window_handle` explicitly; GPUI also has an inherent `window_handle()` (SC §2) |
| `futures` | `0.3` | vsesvit-cef, vsesvit-gpui | the pump and event channels |
| `ashpd` | the current release | vsesvit-gpui (Linux) | portal file dialogs with filters (UA §1.1); GPUI's own prompt has no filters (GP §2.10) |
| `glib-2.0` | system library, linked by `#[link(name = "glib-2.0")]` in `driver/glib.rs` | vsesvit-cef (Linux) | as SC `pump.rs`; no glib crate |
| `rusty-leveldb` | the current release | vsesvit-cef | seeds Chromium's extension storage before its first start (§5.9); declared by W0.1 so no later unit edits the manifest |
| `serde_json` | the workspace's existing pin | vsesvit-engine | `RequestResult::Extension` carries a `serde_json::Value` (§5.4) |

The workspace `rust-version` moves from 1.98 to 1.99. gpui-pre needs at least 1.98.1 (GP §1.2) and every spike was built with 1.99.0 or 1.99.1.

Build profile additions, from SC §2 (the spike's debug binary was 968 MB, almost all dependency debug info):

```toml
[profile.dev.package."*"]
opt-level = 2                      # GPUI and CEF bindings are slow at opt-level 0; every spike used this
debug = "line-tables-only"         # keeps workspace crates fully debuggable, trims dependency debug info
```

This applies to WinUI dependencies too. A clean dev build of WinUI gets slower once, and runs faster after.

### 2.2 The vendored GPUI patches

Two patches, one per OS. Each is applied to a vendored copy of one GPUI backend crate.

| Patch | Crate | Size | What it does | Without it |
|---|---|---|---|---|
| `gpui-x11-focus.patch` | `gpui-pre-linux` 0.3.9, `src/linux/x11/client.rs` | about 80 lines | an InputOnly 1×1 `gpui-focus-proxy` child per toplevel whose key events are mapped to the toplevel; `FocusIn`/`FocusOut` with detail `Inferior` leave the window active and reset only compose, pre-edit and IME | keys typed in the omnibox go to the page under the pointer; GPUI turns inactive on every page click (SC §7) |
| `gpui-windows-keys.patch` | `gpui-pre-windows` 0.3.9, `platform.rs` `translate_accelerator` | 31 lines | sends `WM_GPUI_KEYDOWN` only to windows of class `Zed::Window` | every key posted to a page is dropped (SW §4) |

**Layout.**

```
third_party/
  README.md                    what is vendored, why, the upstream PR for each patch, how to bump
  gpui-pre-linux/              crates.io 0.3.9 with the patch applied
  gpui-pre-linux.patch         the diff against crates.io (moved from spikes/core/vendor/gpui-x11-focus.patch)
  gpui-pre-windows/            crates.io 0.3.9 with the patch applied
  gpui-pre-windows.patch       moved from spikes/win-core/vendor/gpui-windows-keys.patch
```

Root `Cargo.toml`:

```toml
[workspace]
members = ["crates/*", "xtask"]
exclude = ["docs/design/linux-cef/spikes"]     # the Linux spikes have no [workspace] and would attach to this one

[patch.crates-io]
gpui-pre-linux = { path = "third_party/gpui-pre-linux" }
gpui-pre-windows = { path = "third_party/gpui-pre-windows" }
```

**Why both patches can sit at the root.** `[patch]` is workspace-wide and target-independent; it cannot be per-target or per-crate. That is fine here:
- Only `gpui-pre-platform` depends on the two backend crates, under `cfg(target_os = "linux")` and `cfg(windows)` respectively. So the Linux backend is never compiled on Windows and the reverse, even though both patches resolve in the one lock file.
- Only `vsesvit-gpui` (and `vsesvit-sandbox` through it) depends on GPUI. WinUI, core, sync and update are untouched by either patch.
- Both crates are in the dependency graph on every target, so Cargo reports no "unused patch" warning.

**Keeping the vendored trees honest.** A new xtask command does the vendoring, so nobody edits the trees by hand (principle: build the lever). `Cargo.lock` cannot be the reference: a `[patch]`ed path crate is recorded with name and version only, no source and no checksum (both reviews; confirmed by the Opus review's Cargo probe and the spike lock files). So a committed provenance file is:

```toml
# third_party/vendor.toml
[gpui-pre-linux]
version = "0.3.9"
crate_sha256 = "<from the crates.io index at bump time>"
patch_sha256 = "<sha256 of third_party/gpui-pre-linux.patch>"
[gpui-pre-windows]
# same keys
```

- `cargo xtask vendor-gpui --bump <crate> <version>` reads the checksum from the crates.io sparse index, downloads the `.crate`, verifies it, unpacks it into `third_party/<crate>/`, applies the patch and rewrites both digests in `vendor.toml`. This is the only step that trusts the network, and its diff to `vendor.toml` is reviewed.
- `cargo xtask vendor-gpui --check` downloads the `.crate`, checks it against `crate_sha256`, checks the patch against `patch_sha256`, applies the patch in a temp dir and fails if the result differs from `third_party/<crate>/`. It runs in CI (§6.3).
- `third_party/** -text` in `.gitattributes`, so a Windows checkout's CRLF conversion cannot fail the check.
- A GPUI bump is: `--bump`, fix the patch if it no longer applies, re-run §7.

**Upstreaming.** Both patches are plain bugs for any embedder of foreign child windows (SC §7.2, SW §14.1). Each gets a Zed PR in wave 1. When one lands in a gpui-pre release, its `[patch]` line and vendored tree are deleted in the same commit as the version bump.

**Flatpak.** The cargo-sources generator skips packages without a `source` in the lock file, which includes path patches (`xtask/src/linux/flatpak.rs`, `write_cargo_sources`, the `None => continue` arm), and the vendored trees are part of the repo checkout the Flatpak build already uses. No git dependency is introduced, which the generator would reject. W6.3 adds a generator test with a `[patch]` path entry, and adds `docs/design/linux-cef/spikes` to the manifest's `skip` list so the spikes' own vendored trees and `target/` stay out of the build.

## 3. CEF lifecycle per OS

### 3.1 Boot order

UA §1.2 stands. The changes are the first step, the extension step and the Windows details.

| # | Step | Linux | Windows | Source |
|---|---|---|---|---|
| 0 | `run_subprocess(&sandbox)`: `api_hash` first, then `execute_process(args, ChildApp)`; return in a sub-process | in `main`, before anything else | in `RunWinMain` (release) or `main` (dev) | G1, SC §3.1, SW §3.2 |
| 0a | Windows release only: compare the `cef_version_info` that `RunWinMain` receives with our own; refuse to start on a mismatch | — | yes | SW §3.2 |
| 1 | logging (`VSESVIT_LOG`); one backend on both OSes, log file `<profile>/vsesvit.log` as WinUI has | yes | yes | inv/shell-core §19 |
| 2 | parse the CLI with `vsesvit_core::cli` (§4.3); errors exit 2 | yes | yes | UA §1.2 |
| 2a | `--self-test`: `vsesvit_testenv::require()` validates the private display or desktop (§7.3) and exits 3 on any doubt, before step 4. Nothing below runs on the user's display | yes | yes | §7.3 |
| 3 | headless commands (`--help`, `--version`, `--check-for-updates`, `--update`): no profile, no display, no CEF | yes | `AttachConsole(ATTACH_PARENT_PROCESS)` first; the bootstrap image is a GUI-subsystem exe | inv/winui-engine, `winui/platform.rs:148` |
| 4 | profile resolution. `Picker` skips steps 5 to 9: it runs GPUI with the picker view only, no CEF, no lock, and on a choice re-executes itself with `--profile-dir <root>` and exits | yes | yes | UA §1.2 |
| 5 | `Profile::open(root, OpenOptions { chrome_version: "154.0.8037.98", key_store: System, .. })`; `Locked` forwards over the instance channel and exits with the owner's status | Unix socket | named pipe | UA §1.2, §1.4 |
| 6 | bind the instance listener; no accept thread yet | yes | yes | UA §1.2 step 7 |
| 7 | extensions, thread-free and metadata only: `reconcile()`, then write the External Extensions file of every record the publish rule of §5.5 allows and remove the others, because Chromium reads them only at start (§5.2); the helper's own JSON too (§5.3); seed Chromium's storage for an id published for the first time (§5.9). A record without an archive is marked "needs restoring" for step 10a. No network, no UI, no thread (R2-7) | yes | the route WX.0 picks (Q5) | EL §6.2, §9.1 |
| 7a | the pre-start `Preferences` editor (`vsesvit-cef/src/ext/prefs_file.rs`): drop re-installed ids from `external_uninstalls` (§5.2 step 6); for ids already in Chromium, write the desired disabled state and private-window permission (§5.5; inferred, W4.2 checks). Runs only when the file exists and Chromium's singleton lock is free | yes | yes | EL §9.6 |
| 8 | Linux only: compute the scale once; set `GPUI_X11_SCALE_FACTOR` and pass `--force-device-scale-factor`. Read the password store from `<root_cache_path>/password-store` (Q8) | yes | — (per-monitor DPI) | SC §5.4, SW §5 |
| 9 | `factory.initialize(EngineConfig)` = `cef::initialize` with §3.2 and §3.3; `0` means early exit, with no further CEF call; a failure is shown with `platform::fatal_message` (a GUI-subsystem exe has no visible stderr) | yes | yes | API §3.1, `winui/platform.rs:122-163` |
| 10 | GPUI: `gpui_platform::linux(WindowingModes::X11)` / the Windows platform, `.run(app::start)`. `app::start` creates `Desktop`, calls `engine.set_hidden_host`, starts the maintenance browser and runs the start-up clear (§3.7) before the first restored tab loads | yes | yes | UA §1.2 step 9 |
| 10a | after the first window shows: the extension migration state of §5.9 for records marked at step 7 (network refetch, file reselection, approval, in the Extensions window); what it restores takes effect after a `Relaunch` (§3.6) | yes | unless outcome C | R2-7 |
| 11 | after `run` returns: the shutdown sequence of §3.6 | yes | yes | |

**No extra threads before step 9.** The Linux sandbox wants a single thread while CEF initializes (UA §1.2, from cefclient). Steps 1 to 8 must not start one. Two traps: `vault_key()` with the Secret Service store starts an async-io reactor on first use, so nothing may touch the vault before step 9; and the instance accept thread starts in `app::start`. The password-store choice therefore cannot come from probing the vault (GPT review 13); it is a persisted value (Q8). `CefEngineFactory::initialize` counts `/proc/self/task`. Above 1 it logs an error in every build, and under `--self-test` the `single_thread_init` check fails, so the invariant is enforced by the gates of §7.5, not by a debug log.

### 3.2 Paths per vsesvit profile

One process holds one vsesvit profile (inv/core-surface §1.2), so one CEF user-data root per process.

| Path | Value (both OSes) | Notes |
|---|---|---|
| profile root | `<ProfilesDir>/<ProfileId>`: `~/.local/share/vsesvit/profiles/<id>` on Linux, `%LOCALAPPDATA%\Vsesvit\data\profiles\<id>` on Windows | `ProfilePaths.root` |
| `root_cache_path` | `<root>/engine/cef` | under `ProfilePaths.engine_data`, so `ProfilesDir` deletion removes it with the profile. The `cef` subfolder keeps it apart from WebKit's data (Linux, existing profiles) and WebView2's user-data folder (Windows, same profile opened by WinUI) |
| global context `cache_path` | `<root>/engine/cef/Default` | must sit at or below `root_cache_path`, or CEF silently goes in-memory (API §3.6). Chrome's own profile layout |
| private windows | one `request_context_create_context` with an empty `cache_path`, created at the first private tab, dropped after its last browser closed | in-memory (UA §4.4) |
| External Extensions | `<root_cache_path>/External Extensions/<id>.json` | Linux, verified (EL §0). Windows open (Q5) |
| CEF log | `<root_cache_path>/cef.log`, severity `WARNING`; `VSESVIT_CEF_LOG=verbose` raises it | without it CEF writes `debug.log` beside the exe (G15) |
| core lock / CEF singleton | `<root>/LOCK` (core) / Chromium's lock in `root_cache_path` | core's is taken first, so a second process forwards before CEF starts (UA §1.2). `on_already_running_app_relaunch` returns 1 and is handled like a socket invocation, as the fallback |
| password store (Linux) | `<root_cache_path>/password-store`, one word: `basic` or `gnome-libsecret` | written once at the first GPUI start of a profile, never changed automatically (Q8) |
| self-test | `OUT_DIR/profile/engine/cef` | never touches user data (G15) |

Chromium's downloads default to `$HOME/Downloads` (SC §1) or the user's Downloads folder (SW §10) whenever a handler lets it. The shell always handles `on_before_download` (§4.2), and the self-test also points `HOME` (Linux) at its scratch dir.

### 3.3 Settings and switches

`cef::Settings` (API §3.2):

| Field | Linux | Windows | Source |
|---|---|---|---|
| `no_sandbox` | 0 | 0 in the bootstrap package; 1 for `cargo run` | SC §3.3, SW §3.2 |
| `external_message_pump` / `multi_threaded_message_loop` | 1 / 0 | 1 / 0 | SC §4, SW §4 (multi-threaded is the documented fallback, §9) |
| `browser_subprocess_path` | absolute path of `vsesvit-helper` beside the exe | empty: the same image | SC §3.1, SW §3.1 |
| `root_cache_path`, `cache_path`, `log_file`, `log_severity` | §3.2 | §3.2 | |
| `persist_session_cookies` | 1 when the startup pref is "continue where you left off", read at start | same | Chrome's behaviour |
| `background_color` | the theme's page background | same | avoids a white flash |
| `remote_debugging_port` | 0. Never a TCP port (EL §9.3, CE §8) | 0 | |
| `windowless_rendering_enabled` | 0 | 0 | windowed only |
| `locale` | ignored by CEF on Linux; `--lang` instead (G15) | from the OS UI language | |

Switches, added in `App::on_before_command_line_processing` for the browser process only (empty `process_type`):

| Switch | OS | Why | Source |
|---|---|---|---|
| `--ozone-platform=x11` | Linux | CEF 154 picks Wayland in a Wayland session otherwise | SC §3.2, G12 |
| `--force-device-scale-factor=<s>` | Linux | one scale for GPUI and Chromium | SC §5.4 |
| `--password-store=<the persisted value>` | Linux | KWallet that does not answer stalls every request; never KWallet; the choice is Q8 | SC §3.2 |
| `--lang=<locale>` | Linux | `locale` is ignored there | G15 |
| `--noerrdialogs` | both | no modal on a failed extension load | EA §0 item 1 |
| `--disable-features=<CEF's list>,<ours>` | both | merge into CEF's own list, never replace it | SC §3.2 |
| `--disable-gpu-process-crash-limit` | both | a GPU process crash loop otherwise disables GPU for the session | P (prior art) |
| `--disable-smooth-scrolling`, `--disable-gpu` | both | the `scrolling.smooth` and `system.hardware_acceleration` prefs; restart-only (UA §1.2 8b) | inv/tab-engine §1.2 |
| `--load-extension=<dir>` | both | only when the command line carries `--load-extension`, typed by a developer or added by "Load unpacked"'s `Relaunch` (session-only, §5.2 step 5); never with Enhanced Safe Browsing on, which blocks it (EL §2); the helper is not loaded this way (§5.3) | EL §2, §9.10 |
| `--disable-features=CalculateNativeWinOcclusion` | Windows, self-test only | Chromium treats an off-screen window as occluded | SW §14.12 |
| `--use-fake-device-for-media-stream`, `--auto-select-desktop-capture-source="Entire screen"` | both, self-test only | capture checks without devices, as WinUI's scripted runs. Never `--use-fake-ui-for-media-stream`: it auto-accepts, and `capture_in_use`/`site_permissions` assert the shell's prompt (Opus review 13) | inv/self-test §5 (line 646), inv/winui-engine (line 82), inv/omnibox-privacy (line 891) |
| `--extensions-update-frequency=86400` (Chromium's switch; its effect under CEF is inferred and checked in W4.4) | both, self-test only | Chromium's `ExtensionUpdater` (3.4 s after start, EL §6.1) would race `extension_update`; the check triggers updates itself | EL §6.1; Opus review appendix A.10 |

### 3.4 The message pump

One design on both OSes, from SC §4 and SW §4. CEF's UI thread is GPUI's main thread, which is also the thread that owns `Profile`.

```
on_schedule_message_pump_work(delay)  (any thread) ──► Wake::Cef(delay) ─┐
glib-fd-watch thread (Linux)          (fd readable) ──► Wake::Io ────────┼─► one GPUI foreground task (bridge/pump.rs)
GPUI timer                            (cap reached) ─────────────────────┘     driver.do_work():
                                                                                 Linux: service GLib default context, re-arm the watcher
                                                                                 cef::do_message_loop_work()
                                                                               router task drains BrowserEvents after it
```

| Rule | Why | Source |
|---|---|---|
| exactly one caller of `do_message_loop_work`, a `cx.spawn` task outside any `cx.update` | the pump itself never re-enters GPUI's `RefCell`. Callbacks can still run synchronously inside engine calls and Windows message dispatch; §1.6's rules cover those | SC §4.1, GP §4.1 |
| poll: at most 33 ms while active, 250 ms after 1 s without activity | `MessagePumpExternal` drops its pending delayed work after a 10 ms slice; on-demand pumping alone stalled loads (2 requests in 48 s) | SC §4.2 |
| Linux: service the GLib default context by hand each iteration (acquire, prepare, query, poll 0, check, dispatch, release) and ignore its 0 ms prepare timeout | Chromium's X11, GDK/GtkUi, D-Bus and AT-SPI sources live there; GPUI never iterates it | SC §4.1, G13 |
| Linux: `glib-fd-watch` thread polls GLib's fds and wakes the task | key latency 3–5 ms with it, 9–196 ms without | SC §4.3 |
| Windows: no GLib work; GPUI's `GetMessageW` loop dispatches Chromium's HWND messages | Chromium's browser HWNDs live on the main thread (`tid == main_tid`) | SW §4, §13.3 |
| Windows: the key patch is required | GPUI's `translate_accelerator` eats keys meant for Chromium | SW §4 |
| GPUI → CEF calls are direct, from main; never from another thread | main is the CEF UI thread | SC §13.3, SW §13.3 rule 1 |

The timing policy (33/250 ms) lives in the shell's `bridge/pump.rs`; the OS work lives in `vsesvit-cef/driver`. Measured idle cost: Linux 0.47 % of one core for all processes (debug build, SC §4.3); Windows 3.8–5.1 %, most of it from window activation rather than the pump (SW §4). Release-build numbers are part of wave 1's definition of done (§8).

**Why not `multi_threaded_message_loop`.** It disables GtkUi on Linux (IME, GTK dialogs; G13). On Windows it measured slightly cheaper (SW §4), but every CEF call would then post to another thread, cross-thread window calls can deadlock against CEF's UI thread, and `cef::shutdown` needs a 200 ms wait or it hangs. It stays the documented fallback for Windows if the key patch ever cannot be carried.

### 3.5 Sandbox and process images

| | Linux | Windows |
|---|---|---|
| Mechanism | user namespaces + seccomp-BPF, no setuid needed where unprivileged user namespaces are allowed ("adequately sandboxed", SC §3.3) | CEF 154 ships no `cef_sandbox.lib`; only `bootstrap.exe` renamed to the product name, loading `<name>.dll` and calling `RunWinMain(HINSTANCE, LPWSTR, int, void* sandbox_info, cef_version_info_t*)` (SW §3.2) |
| Our image | `vsesvit` (browser) + `vsesvit-helper` (all sub-processes); rpath `$ORIGIN` on both, set by their `build.rs` (G14) | one image: `vsesvit.exe` (bootstrap) + `vsesvit.dll`; renderers run `Lockdown` at `Untrusted` integrity; the network service is unsandboxed, as in Chrome (SW §3.2) |
| Fallbacks per package | setuid `chrome-sandbox` 4755 or an AppArmor userns profile (Ubuntu 23.10+); AppImage cannot setuid (nosuid mount), so where user namespaces are blocked it runs unsandboxed only with a visible warning (Q29), never silently; Flatpak needs zypak (§6.4) | none needed |
| Dev builds | same as release | `no_sandbox = 1`, same exe |

`chrome-sandbox` and the CEF assets are found relative to `libcef.so`, not the exe (API §3.1), so they sit beside it.

### 3.6 Shutdown

UA §1.6 "Quit" plus G16 and both spikes' measured sequences (SC §12, SW §12). `vsesvit-cef/teardown.rs` holds the states Running → ClosingUser → ClosingInternal → Drained → Shutdown, with Stuck as the one failure exit from either Closing state. The two Closing states are the two phases of the contract (§1.5): `begin_shutdown` closes the user's browsers while the maintenance browser still serves the final jobs, and `close_internal` closes the hidden ones after those jobs (R2-4).

**The live-browser count.** G16 forbids dropping CEF references or calling `cef::shutdown` before every browser's `on_before_close`. Both spikes quit only after the last close callback; their extra 200 ms is drainage after that, not a substitute (SC §12, SW §12, §14.11). So `registry.rs` counts every browser from the moment the glue asks CEF to create it until its `on_before_close`: tabs, popups (counted in `on_before_popup` when allowed, uncounted if the popup is aborted), extension popups, DevTools browsers (created with our client, so their life span reaches us), Chrome-created windows (counted in `on_after_created` through `default_client`), download keep-alive browsers, and the maintenance, bridge and management browsers. A failed `browser_host_create_browser` uncounts at once. User browsers and internal browsers are counted apart, so the glue can report each phase's end.

| # | Step | Cap | Source |
|---|---|---|---|
| 1 | trigger: Ctrl+Q (there is no Quit menu item), the last window's close button (`on_window_should_close` returns false while it has tabs), a restart of any kind (the exit intents below), the end of a self-test, removing the current profile | | inv/shell-core (lines 465, 1801), SC §12, SW §12 |
| 2 | save the session now (normal windows only); skipped for a removed profile | | inv/shell-core §9, line 1802 |
| 3 | hide the windows; `engine.begin_shutdown()`: teardown → ClosingUser, every open prompt ends with `Shutdown`, `close_browser(1)` on every user browser (tabs, popups, extension popups, DevTools, Chrome-created windows, parked download browsers; their downloads end interrupted). Wait for `BrowserEvent::UserBrowsersClosed`, pumping as usual | | UA §1.6 |
| 4 | clear on exit through the maintenance browser (§3.7) and the final sync round, side by side; both skipped for a removed profile | 3 s each | inv/extensions-sync-updates C.12, inv/omnibox-privacy §5.2 |
| 5 | `engine.close_internal()`: teardown → ClosingInternal, `close_browser(1)` on the maintenance, bridge and management browsers | | §3.7 |
| 6 | wait for `BrowserEvent::AllBrowsersClosed` (the live count is zero), pumping as usual | 15 s from step 3, for steps 3 to 6 together | G16 |
| 7 | count reached zero: teardown → Drained. `PumpHandle::stop` ends the pump task after its current step and hands back the driver; `cx.quit()`; GPUI's `run` returns on both OSes | | SC §12, SW §12 |
| 8 | `driver.shutdown()`: stop and join `glib-fd-watch`, pump for 200 ms, empty the registry, drop every CEF reference (browsers, frames, contexts, callbacks, observer `Registration`s), `cef::shutdown()` on main | 29 ms Linux, 87–129 ms Windows measured | G16, SC §12, SW §12 |
| 9 | drop the `Profile`; assert it was the last strong reference (frees `LOCK`) | | UA §1.2 step 10 |
| 10 | carry out the exit intent (below) | | inv/shell-core (line 1803), `winui/updates.rs:586-608`, inv/extensions-sync-updates D.6 |
| 11 | return the exit code | | |

**Late creations and late requests.** Leaving Running must stop new work, or the count never reaches zero (R2-4). After Running the glue:
- refuses `create_tab` and every hidden-browser creation with `EngineError::ShuttingDown`; the bridge is not recreated after a crash;
- closes at once (`close_browser(1)`) every browser whose `on_after_created` arrives after Running: a creation requested earlier, a Chrome-created window (neutralized first, no `ForeignWindowOpened`), a DevTools window. Each stays counted until its `on_before_close`;
- blocks popups in `on_before_popup` (nothing counted);
- refuses every prompt at admission (§1.5), including auth requests posted from IO before `begin_shutdown` and admitted after it;
- answers `extension_call` with an error.

**The exit intent.** Restarting is not the same as installing an update (R2-4). The shell's `app/quit.rs` carries one intent through the sequence:

```rust
pub enum ExitIntent {
    Quit,
    RemoveProfile,                          // step 10 deletes the profile's data
    Relaunch(RelaunchArgs),                 // the same image again; needs no update artifact
    ApplyUpdateAndRelaunch(Downloaded),     // "Restart to update"
    QuitInstallingUpdate(Downloaded),       // Windows: quitting with an update ready
}
pub struct RelaunchArgs { pub profile_root: PathBuf, pub phase: Option<String>, pub load_extensions: Vec<PathBuf> }
```

| Intent | Step 10 | Used by |
|---|---|---|
| `Quit` | nothing | Ctrl+Q, last window, end of a self-test |
| `RemoveProfile` | delete the profile's data | removing the current profile |
| `Relaunch` | spawn the current image with `--profile-dir <root>` (plus `--phase`, `--load-extension` when set): Linux `current_exe()`, an AppImage `$APPIMAGE`, Windows `current_exe()` (the renamed bootstrap in a release package, the cargo exe in dev) | "Restart to finish installing" (§5.2), the welcome install's Restart (§5.9), the migration restart (§5.9), "Load unpacked" (§5.2), the self-test's restart phases |
| `ApplyUpdateAndRelaunch` | `Downloaded::install(&installation, relaunch_args)` (`vsesvit-update/src/install.rs:41`), then what its `Installed` result says | "Restart to update" |
| `QuitInstallingUpdate` | `Downloaded::install_on_exit(&installation)` | quitting on Windows with an update ready |

`Relaunch` is W1.4's (the self-test phases need it in wave 1). The two update intents call into W3.7's `updates/` through a dispatch stub (§8.1).

**When the cap is reached** (step 6 times out), teardown → Stuck. Calling `cef::shutdown` with live browsers is the error G16 forbids, and waiting forever would leave a windowless process that blocks the update restart. So the shell:
1. logs every live registry entry (kind, `TabId`, URL, age, last callback seen);
2. makes no further CEF call: no `cef::shutdown`, no reference drops;
3. drops the `Profile` (core flushes and frees `LOCK`) and flushes the log;
4. skips step 10 (a removed profile's data, a relaunch and an update wait for the next start); if steps 4 and 5 never ran, clear on exit did not happen either, and the next start's start-up clear covers it (§3.7);
5. exits with `EXIT_SHUTDOWN_STUCK = 70` through `std::process::exit`. CEF's sub-processes exit when their browser process goes.

`EXIT_SHUTDOWN_STUCK` is distinct from the self-test's 0/1/2/3, so the self-test and the updater can tell it apart.

**Who owns the driver at the end.** UA §4.3 detaches the pump task, which would leave §3.6 nothing to shut down. Instead `engine_bridge::start` returns a `PumpHandle` that the `Shell` global keeps, and `vsesvit_gpui::run` creates an `Rc<RefCell<Option<Box<dyn EngineDriver>>>>` before `.run()`. Step 7 moves the driver into it, and `run` takes it out after GPUI returns and runs step 8. GPUI's `on_app_quit` gives only 200 ms (GP §4.1), so none of this runs there.

The restart in step 10 must follow step 8, so that both `LOCK` and Chromium's singleton are free before the new process starts (inv/extensions-sync-updates D.6).

**Tests** (W1.4 builds the mechanism; W1.8's checks run it as self-test phases that re-execute the binary on scratch profiles, so the parent can read the exit code): one browser's close delayed by 8 s, past the draft's old 5 s cap and under the new one, ends with exit 0 and a logged `cef::shutdown`; one live entry pinned past the cap ends with exit 70, the entry in the log, no `cef::shutdown` in the log and `LOCK` free; a tab whose `on_after_created` is held until after `begin_shutdown` is closed at once and the run still ends with exit 0 (`shutdown_late_creation`); the maintenance browser is still alive when step 4 runs and gone at step 6. All use `Engine::test_shutdown(TestShutdown)`, compiled only with the `self-test` feature. W1.4 also tests `Relaunch` with no update downloaded (an injected spawner records the image and arguments, on both OSes), and W4.4's restart check drives "Restart now" with no browser update.

### 3.7 Hidden browsers and site data

Some engine work needs a browser that is not a tab (GPT review 10, Opus reviews 11 and 16). All of them live in `Desktop`'s hidden host (§1.3), so no browser window's close affects them, and `hidden.rs` owns their lifetime.

| Browser | Style, URL | Lifetime | Used for |
|---|---|---|---|
| maintenance | Alloy, `about:blank`, normal context | from `app::start` (before the first restored tab) to shutdown step 5 | the DevTools target for `Storage.clearDataForOrigin`: Settings' site-data deletion, the start-up clear and clear on exit |
| bridge | Alloy, `chrome-extension://<helper>/bridge.html` | from helper start to shutdown step 5; recreated only if it crashes | the helper's two-way channel (§5.3) |
| uninstall page | Alloy, `chrome-extension://<id>/manifest.json` | one call | `uninstallSelf` (§5.4) |
| management | **Chrome style**, `chrome://extensions` | one call: open, call, close | `developerPrivate` (§5.4). Alloy cannot load it: `chrome://extensions` is not on Alloy's WebUI allowlist (CE §2.4, §3.6; GPT review 3) |

Chrome style makes the management browser a one-tab `"popup"` window in every extension's `tabs` and `windows` model while it lives (EA §2). Keeping it short-lived bounds that. Whether the Alloy browsers appear in `tabs.query({})` is not known: EA §3 showed only that an Alloy popup's `tabs.getCurrent()` has `windowId: -1` (Opus review 27). WX.0 measures both, and whether `developerPrivate` works over a page session at all (EL §8.1 used the pipe).

**Site data.** `Storage.clearDataForOrigin` needs a live browser's DevTools session; a `RequestContext` alone has none (API §5.4). The maintenance browser is that session for the normal context. The private context is in memory and needs no clearing.
- Start-up clear: `app::start` runs it on the maintenance browser before session restore issues its first load, capped at 5 s as WinUI (inv/omnibox-privacy, line 974). Restored tabs are created first and load when it finishes.
- Clear on exit: shutdown step 4, user browsers already closed, the maintenance browser still alive.
- Both clear cookies through `CookieManager` and every other storage type (localStorage, IndexedDB, cache storage, service workers) through `Storage.clearDataForOrigin` with `storageTypes: "all"`. That this works on a CEF page session is inferred (WinUI does it on WebView2, inv/winui-engine §19). W3.1's self-test proves it with localStorage and IndexedDB as well as cookies.

## 4. vsesvit-core integration

### 4.1 Rules

Core already fits a third shell (inv/core-surface §0; code survey §1):
- **One `Profile` per process, on the main thread.** It is `!Send` and every accessor takes `&mut self`. The shell holds it as `Rc<RefCell<Profile>>` in the `Shell` global (UA §1.4) and borrows it for one call at a time, never across an engine call. CEF handlers and hooks never borrow it; they read snapshots the shell publishes (§1.5, §1.6).
- **Core never calls back.** Writes return what changed; sync returns `ApplyReport.changed`. The fan-out to views is the shell's `BrowserStateEvent` (UA §1.4).
- **Slow work is a `Send` job.** Built on main, run on GPUI's background executor, committed on main. This replaces `gio::spawn_blocking` (GTK) and WinUI's `exec.rs`.
- **IO-thread handlers never see `Profile`.** Tracker blocking, cookie access and HTTPS-only decisions that CEF asks for on its IO thread read a `Send + Sync` snapshot that main publishes on every relevant change. The snapshot type is new and lives in core (§4.3 row 11), because WinUI's per-request tracker check needs the same thing.
- **The shell supplies** the timers, the Chromium version for `OpenOptions.chrome_version` (`154.0.8037.98`; the GTK default `150.0.0.0` goes), `KeyStore::System`, the Downloads folder, and single-instance forwarding (inv/core-surface §1.2).

### 4.2 Feature areas

UA §2 maps every inventory checklist item to a `vsesvit-gpui` module. This table adds the core entry points, the CEF mechanism and what differs on Windows. "→ core" marks logic that moves (§4.3).

| Area | Core / sync / update API | CEF mechanism | Windows difference |
|---|---|---|---|
| Startup, CLI, instances | `cli` (→ core), `ProfilesDir::{sweep, startup, locate, opened}`, `Profile::open`, `OpenError::Locked` | `on_already_running_app_relaunch` as fallback | named pipe; `AttachConsole` for headless commands |
| Session and closed tabs | `session::{save, restore, other_devices}`, `session::plan` (→ core), `closed_tabs` (→ core), `tab_groups`, `tab_place`, `tab_search` | none: CEF cannot restore back/forward history, so tabs restore by URL (Q7) | window positions are real; `WindowSnapshot.bounds` filled |
| Window title | `profiles::with_profile` + one title format (→ core) | `on_title_change` | GTK and WinUI disagree today; core settles one |
| Navigation, history, transitions | `history::{record_visit, set_title}`, `search::classify_url`, `address::readable_url` | `on_load_start` (transition), `on_address_change`, `on_loading_state_change`, `on_title_change`; same-document = address change without load start | — |
| Omnibox | `search::Omnibox::{resolve, suggest, suggest_request}`, `suggest::SuggestRequest`, edit model (→ core) | — | — |
| New tab page, error and warning pages | `new_tab::page`, `https_only::warning_page`, error-page HTML (ported from GTK `error_page.rs` into `vsesvit-gpui/tab/error_page.rs`; WinUI has none) | `vsesvit://` scheme handler; the omnibox shows `about:blank` / the failed URL (UA S:9) | — |
| HTTPS-only | `https_only::Upgrades` per tab | `on_before_browse` (main frame, gesture, redirect), `on_load_error`; Chromium's own HTTPS upgrades stay off | — |
| Tracking protection | `trackers::TrackerList::blocks`, privacy snapshot (→ core) | `resource_request::on_before_resource_load` → cancel (IO); a blocked count becomes possible | same code; WinUI uses the same snapshot |
| Cookies, site data, clear on exit | `cookies::{third_party_blocked, site_rules, to_clear, group_by_site}` | pref `profile.cookie_controls_mode`, COOKIES content setting, `CookieManager` visit/delete, DevTools `Storage.clearDataForOrigin` on the maintenance browser; clear at start and at exit (§3.7) | WinUI's `dialogs/site_data.rs` plan carries over as is |
| Permissions and capture | `permissions::{decide, answer, prompt, TabGrants, SitePermissions}`, prompt queue (→ core) | `on_show_permission_prompt` (return 1), `on_request_media_access_permission`, `on_dismiss_permission_prompt`, `on_media_access_change`, all as `PromptRequest::Permission` (§1.5); mirror stored settings with `set_content_setting`. Types without a policy today get one in W3.1: storage access, local and loopback network (the sync loopback callback needs it), keyboard lock, file system access (inv/omnibox-privacy, lines 505-507); "Stop sharing" for screens uses the `DESKTOP_*` request state because `on_media_access_change` has no screen flag | without the handler Windows shows Chrome-native prompt toplevels (SW §10) |
| JS dialogs, beforeunload, file choosers, HTTP auth | — | `PromptRequest::{JsDialog, BeforeUnload, FileChooser, Auth}` (§1.5) | — |
| Downloads | `downloads::*`, download tracker (→ core), `zone_identifier` | `can_download` → 1 (the cef-rs default cancels), `on_before_download` → `cont(path, 0)` always, `on_download_updated`, item callbacks. **Keep-alive:** CEF stops reporting a download once its browser is destroyed (inv/bookmarks-downloads, line 967), so closing a tab must not destroy a browser with a running download. Closing the tab and closing the browser are separate (below the table) | Mark of the Web written (→ `core::os`); the "Recent download history" bubble still shows (Q2) |
| Bookmarks and favicons | `bookmarks::*`, menu/bar/editor/star/drag models (→ core), `favicons::{record, missing, commit_fetched}`, PNG fit to 32 px (→ core) | `on_favicon_urlchange` + `download_image` + `get_as_png` | WinUI drops icons over 64 KiB today; the core fit fixes it |
| Zoom | `zoom::SiteZoom`, presets and `step`/`percent` (→ core) | `set_zoom_level(ln f / ln 1.2)`; no change callback (Q10) | — |
| Find, print, DevTools, view source, save page | `view_source::source_url`, `downloads::sanitize` | `host.find`, `host.print` (Chrome print preview in the child), `show_dev_tools`, `view-source:` (Chromium renders it), DevTools `Page.captureSnapshot` (MHTML) | — |
| Keyboard | `shortcuts::{Command, Keymap, Chord}`, in-page policy per command (→ core) | `on_pre_key_event` for reserved keys, `on_chrome_command` as the net with default-deny (SC §8.2, SW §8) | CEF reports Windows VK codes on both OSes, so the table is shared |
| Context menu | `clean_url::clean`, `Omnibox::for_selection` | `run_context_menu` with the `CefMenuModel`, shell items inserted, `callback.cont(id)` (EA §5) | — |
| Memory Saver | `memory_saver::{Sweep, IdleClock, UNSAVED_INPUT_SCRIPT}` | no discard in CEF (Q9) | — |
| Private windows | `private::Browsing`, `Profile::end_private_session` | the off-the-record request context (§3.2) | — |
| Profiles UI | `profiles::{Registry, ProfilesDir, EditError, AccountPicture}`, launch/notify conventions | — | `AllowSetForegroundWindow` for the child process |
| Settings | `prefs::*`, `spellcheck`, `search::SearchEngines` | `RequestContext::set_preference` (spell check, password manager off, cookie mode, autofill), `set_chrome_color_scheme` | — |
| Sync | `vsesvit_sync::{SignIn, Account, Round, PassphraseJob, status}`, scheduler (→ vsesvit-sync) | the sign-in tab only | — |
| Updates | `vsesvit_update::{Installation, Updater, Downloaded}`, lifecycle (→ vsesvit-update) | restart after `cef::shutdown` (§3.6) | NSIS `install_on_exit`; flavour variant (§6.5) |
| Extensions | `extensions::*` (§5) | §5 | §5, Q5 |
| Welcome, onboarding, import/export | `onboarding`, `import`, `export` | — | — |
| Self-test | `testkit::{FixtureServer, FixtureStore, Report, probe_crx}`, check-name registry (→ core) | DevTools `Runtime.evaluate` for page eval | private desktop (§7.3) |
| Omnibox engine schemes | `chrome-extension:`, `blob:`, `mailto:` typed in the omnibox go to the engine (`winui/omnibox.rs:12-24` `ENGINE_SCHEMES`); moves with row 7 | — | — |
| Crashed page | — | `on_render_process_terminated` → `RenderProcessGone`; the sad-tab page with Reload (W2.5) | — |

**Download keep-alive.** A tab is the shell's; a browser is CEF's. A tab with a running download can end while its browser lives on, parked in `Desktop`'s hidden host (Opus review 20, R2-6). The steps, for a user closing the tab or its window (the shell starts the close):
1. The shell's close path (W1.5 for a tab, W1.7 for a window) asks `downloads::park_decision(tab)` in W3.3's `downloads/keep_alive.rs`. It answers `Park` when the core tracker has a running download for that tab in the normal context. Until W3.3 lands, the stub answers `Close`.
2. On `Park`, the shell calls `Surface::park_page(page, desktop.hidden_host())` (§1.4). It returns after the native reparent: X11 unmaps and reparents into the hidden host; Windows `SW_HIDE`, then `SetParent`. The window's host stays alive until this returns.
3. The shell calls `engine.detach_tab(tab)`. The glue moves the browser in its registry from the `TabId` to a `ParkedId`, mutes it, navigates it to `about:blank` (that the download survives this is inferred; W3.3 checks it), keeps it counted, and emits `TabEvent::Detached`. The shell then removes the tab. The browser is not closed, so no `TabEvent::Closed` follows.
4. The glue's `download.rs` closes a parked browser when its last download ends. A `detach_tab` that finds no running download closes the browser at once.
5. Only after every page of a window was parked or closed does the shell call `Surface::detach` (§1.4) and close the GPUI window.

If `park_page` fails, the shell closes the tab as usual; the download ends interrupted and the downloads list says so.

A **page-initiated close** cannot be parked: `window.close()` on a Chrome-style browser closes it without asking, because `do_close` runs only for Alloy browsers (API G16), and Chromium may close a blank tab that only started a download. W3.3 measures whether CEF keeps reporting such a download after its browser closed. If it does, nothing more is needed. If not, the download is shown as interrupted, with the downloads list's usual actions for an interrupted download. Either way the result is recorded in `log.tsv` and `parity.md`.

**Private downloads** are never parked. Ending a private session cancels its private downloads through their kept callbacks first, then closes the last private browser, then drops the context (line 926).

W3.3's tests: closing a downloading tab (the download completes and is reported); closing a non-last window that holds a downloading tab (same); a `target=_blank` link whose popup only downloads, and a popup that calls `window.close()` after starting a download (the measured result above); ending a private session with a running private download (cancelled first).

#### 4.2.1 What differs by OS in scope

UA §2 is a Linux feature map: D15 leaves split view and the media player out "on Linux in v1", and UA U17 even asserts `picture_in_picture_synced_from_windows_is_not_listed`. The Windows product has them (inv/window §23; inv/winui-engine §17.9, §18.3, §19). These waves deliver the GPUI Windows flavour as an opt-in build. The Windows default stays WinUI past these waves (§0), and flipping it needs every row marked "flip" below; that work is a later plan, recorded in `parity.md` by W5.2 (GPT review 14, Opus review 18).

| Item | Linux GPUI | Windows GPUI in these waves | Needed to flip Windows |
|---|---|---|---|
| Split view, collapsed favicon pane | not in v1 (D15) | not in these waves | flip |
| Media player, picture-in-picture, their pref and permission gating, Memory Saver exemptions | not in v1 (D15) | not in these waves | flip |
| UA's Linux-only negative tests (`picture_in_picture_synced_from_windows_is_not_listed`) | kept | `#[cfg(target_os = "linux")]` | — |
| Native messaging hosts | `<root_cache_path>/NativeMessagingHosts` + Chromium's paths (§5.8) | HKCU `Software\Chromium\NativeMessagingHosts` (inferred), W4.3 links Chrome's and Edge's registered hosts | in these waves |
| Default-browser row in Settings > General | `platform/default_browser.rs` | same file, W3.5 | in these waves |
| A visible message when start-up fails | stderr + `platform::fatal_message` | message box, as `winui/platform.rs:122-163` | in these waves (W1.4) |
| File dialogs with filters; a folder dialog | ashpd portal; GPUI's prompt for folders | `GetOpenFileNameW`/`GetSaveFileNameW`; GPUI's prompt for folders (§1.3) | in these waves (W2.6) |
| Window frame: caption buttons, snap layouts, resize hit-testing | GPUI CSD | `platform/windows/frame.rs` (§1.3) | in these waves (W1.2, W1.7) |

### 4.3 What moves into core

**Criterion.** Logic moves into core (or `vsesvit-sync` / `vsesvit-update`) when WinUI has its own copy of it and it is free of toolkit and engine types once the shell passes plain values. The unit that moves it migrates WinUI in the same commit and deletes WinUI's copy (principle: migrate callers, then delete). GTK is not migrated: it is deleted at the switch-over (§8 wave 7), so GPUI ports what only GTK has straight into `vsesvit-gpui`. One exception is new: the privacy snapshot, which nobody has yet.

| # | Logic | Today | Moves to | Unit | Source |
|---|---|---|---|---|---|
| 1 | CLI grammar, `USAGE`, errors, run config (superset. Per shell: `--load-extension <dir>` is accepted by WinUI and GPUI, and GPUI passes it to Chromium as its session-only switch (§3.3, §5.2 step 5; R2-12); `--ui-smoke` stays WinUI-only; `--self-test`'s `--phase <name>` for the re-executed phases of §3.6 and §7.2. The run config says which shell accepts each flag, and a flag a shell does not accept exits 2 with a message) | `gtk/cli.rs`, `winui/cli.rs` + `config.rs` | `core::cli` | W0.3 | code survey §3 |
| 2 | startup plan and session snapshot builder | `winui/session.rs` (pure `startup_plan`, `WindowPlan`), `gtk/session.rs` | `core::session::plan` | W0.3 | code survey §3 |
| 3 | closed-tab stack, cap 25, normal and private, with a neutral icon type (WinUI's `ClosedTab` carries a XAML `ImageSource`) | `gtk/closed_tabs.rs` (generic); WinUI: types at `winui/browser.rs:115-150`, cap at `:60`, trim at `:871-872` | `core::closed_tabs` | W0.3 | inv/tab-engine §23 |
| 4 | zoom presets, `step`, `percent` | `gtk/zoom.rs`, `winui/zoom.rs` `LEVELS` | `core::zoom` | W0.3 | inv/tab-engine §18 |
| 5 | window title format incl. the private suffix | both shells, different formats | `core::profiles::window_title` | W0.3 | inv/window §3 |
| 6 | in-page key policy per `Command` (`Native`, `Reserved`, `Overridable`) and capture rules over neutral keys | `winui/shortcuts.rs` `InPage`, `gtk/keymap.rs` | `core::shortcuts` | W0.3 | code survey §3 |
| 14 | self-test check registry: a shared list (43 names) plus per-flavour extras, so `Report::complete` never demands a check the running flavour lacks | `gtk/self_test.rs:83-146`, `winui/report.rs:5-53` | `core::testkit::checks` | C.0 | code survey §3; Opus review 21 |
| 7 | omnibox edit model (`Edit`, `Key`, `Outcome`, refill) and the engine-scheme list | `winui/omnibox.rs` (pure), `gtk/address_bar.rs:77` | `core::omnibox_edit` | C.1 | inv/omnibox-privacy §1.4 |
| 8 | permission prompt queue (max 8, per-document dismissal, holds, guards) | `gtk/permissions.rs`, `winui/permissions.rs` + `window/permissions.rs` | `core::permissions::queue` | C.2 | inv/omnibox-privacy §4.3 |
| 11 | privacy snapshot for IO threads and hooks (tracker level and list, allowed origins, blocked cookie hosts, HTTPS-only pref and exceptions) | WinUI's `Protection` struct, partly | `core::privacy::Snapshot` | C.2 | inv/omnibox-privacy §6.2 |
| 9 | download tracker (reservations, `.unconfirmed` hold, `Transfer`, 250 ms throttle with an injected clock, list limit, engine-state mapping) | `gtk/downloads.rs` (815), `winui/downloads.rs` (664); limits 500 vs 200 | `core::downloads::Tracker` | C.3 | inv/bookmarks-downloads §7 |
| 15 | small OS helpers: download origin marking (Windows: Mark of the Web; Linux: the `user.xdg.origin.url` and `user.xdg.referrer.url` extended attributes, as Chrome writes them; the shell calls it for normal-browsing downloads only, never private ones; a core unit test reads the attribute back and skips where the temp file system has no user xattrs; A.22), Downloads folder (Windows known folder; Linux xdg-user-dirs, inv/shell-core lines 288-291), open/reveal in the file manager, default-browser state | `winui/downloads.rs:529`, `winui/platform.rs`, `gtk` dirs code | `core::os` | C.3 | inv/winui-engine |
| 10 | bookmark menu model, bar snapshot and `fitting`, editor `folder_choices` and save, star flow; favicon `fit_png`. Drag `zone`/`destination` stay in the GPUI shell: WinUI has no bookmark drag, so by the criterion they do not move | both shells; menus already drifted (WinUI lacks Add Page/Folder; labels cut at 40 vs 50) | `core::bookmarks::{menu, bar, editor}`, `core::favicons::fit_png` | C.4 | inv/bookmarks-downloads §1–§6 |
| 12 | sync scheduler (`due`, `settle`, constants 10 s / 2 s / 60 s / 3 s / 120 s) as a sans-IO machine with an injected clock | `gtk/sync.rs` (842 lines), `winui/sync.rs` (1060 lines); two state models | `vsesvit_sync::schedule` | C.5 | inv/extensions-sync-updates C.2–C.4 |
| 13 | update lifecycle (`step(Event) -> Effect`, banner, status, channel change, 1 h retry, install-on-exit) | `gtk/updates/lifecycle.rs` (pure), `winui/updates.rs:33-316` | `vsesvit_update::lifecycle` | C.6 | inv/extensions-sync-updates D.2 |

Not moved, on purpose: the extension driver (its engine half changes completely, §5; it can move after wave 4 if it still matches WinUI), Memory Saver orchestration (no common engine operation), toolkit-shaped code (popover anchoring, widgets), and anything holding a CEF or WebView2 object.

**Lane C.** Moving logic into core edits its WinUI callers too, and those overlap: `winui/browser.rs`, `dialogs/settings.rs`, `lib.rs`, `tab.rs` and `window/mod.rs` are callers of several rows (Opus review 2). So rows 7 to 15 do not ride inside the parallel feature units. They form one serial lane of units, C.0 to C.6, in that order, that runs beside GPUI waves 1 and 2 (§8.4). A C unit owns its core module and every WinUI file it edits while it runs. Because the lane is serial and no other unit edits WinUI between W0.3 and wave 7, no two concurrent units ever touch the same WinUI file. Each GPUI unit that consumes a moved module depends on its C unit.

### 4.4 What core stops needing

These lose their last caller when GTK and `vsesvit-webext` go (code survey §1; inv/core-surface §23). WinUI uses none of them (checked by grep; `cookies::block_script` is not on the list because WinUI uses it, and `restore_state` stays a field that WinUI and GPUI write as `None`). They are deleted in the switch-over wave, in the same commit as GTK:
- the `ext_storage` code and sync `Kind::ExtStorageSync` (§5.6 keeps `storage.sync` local, so nothing adopts the kind; sync code 6 joins the "Retired" comment). The two `ext_storage` tables stay in `schema.sql`: dropping them needs a migration, and a newer schema makes older builds refuse the database as `TooNew` (`db.rs:59-63`). W4.5 reads both tables, `ext_storage_local` and `ext_storage_sync`, before this deletion (§5.9);
- `trackers::TrackerList::dnr_rules`;
- `view_source::source_page` (Chromium renders `view-source:`; `source_url` stays);
- the code that reads `TabSnapshot.restore_state` (old WebKit blobs are ignored);
- `tab_groups::extension_group_id`;
- the `chrome.permissions` request pieces in `extensions::permissions` (`request`, `Request`, `REQUEST_LEAD`, …), but only if WX.0 shows Chromium's own prompt appears for a toolbar-less child. If it does not, the pieces stay and the decision of §5.7 applies (GPT review 15, Opus review 10);
- the WebKit-only manifest normalizations (`dnr_rulesets`, `WebAccessible`, `Background`, content-script run-at/world). `content_scripts[].matches` stays: `permissions::prompted` reads it for WinUI's install prompt (`extensions/permissions.rs:153`).

Core's own extension updater (`extensions/update.rs`) stays: WinUI uses it. The GPUI shell does not call it (§5.5).

## 5. Extensions

### 5.1 Who does what

CEF 154 has no extension API (CE §1.1). Chromium runs extensions natively in Chrome-style browsers; the shell feeds it installs and draws the UI Chromium cannot draw in a toolbar-less child (EA §0). Two records exist and must not be confused (GPT review 7):
- the **package record** is core's: the verified CRX, its version, manifest and withheld permissions, as installed. WinUI runs it, and the External Extensions JSON points at its archive;
- the **runtime record** is Chromium's: the version actually running (Chromium updates store extensions itself), its manifest, permissions, enabled state, disable reason and load errors. The GPUI shell reads it from Chromium at every start and keeps it in memory. It never writes it into core.

| Concern | Owner | Mechanism | Source |
|---|---|---|---|
| Desired set (synced), verification, install prompt wording | core | `extensions::{ExtensionRecord, reconcile, prepare_install, InstallJob, commit}`, `crx.rs`, `permissions::install_warnings` | EL §8 |
| Package archive | core | the original CRX bytes in `<root>/extension-archives/` (§5.2) | GPT review 6 |
| Install into Chromium | shell (vsesvit-cef `ext/external.rs`) | External Extensions JSON pointing at the archive; takes effect at the next start | EL §0, §9.1; Windows per WX.0 (§5.10) |
| Running, storage, permissions at run time, the runtime record | Chromium | native; read through the helper and the management browser | |
| Updates of store extensions | Chromium | its `ExtensionUpdater` (3.4 s after start, then every 5 h) | EL §6.1 |
| Enable, disable, listing, install/uninstall events | helper extension | `chrome.management` | EL §8, §9.5 |
| Incognito, shortcut changes, load and runtime errors, commands | shell | `developerPrivate` from a short-lived Chrome-style `chrome://extensions` browser (§3.7) | EL §8.1; GPT review 3 |
| Uninstall | shell | `uninstallSelf({showConfirmDialog: false})` inside the extension, then delete the JSON; the archive goes with the record | EL §7, §9.4 |
| Toolbar, puzzle menu, popups, Extensions window | shell (GPUI) | core `toolbar.rs` for the pinned layout; buttons from the **runtime** manifest; popups are shell-hosted Alloy child browsers | EA §7 |
| Page context-menu items, command shortcuts | Chromium, through the shell | the `MenuModel` prompt (§1.5); Chromium's own command handling in the page | EA §5, §7.6–7.7 |

### 5.2 Install

1. Unchanged up to verification: `prepare_install` → `InstallJob::run` on a background task → install prompt (core wording) → `commit`. Sources: Chrome Web Store, Edge Add-ons and local `.crx` files.
2. **The archive.** `commit` first writes the original CRX bytes, byte-identical, to `<root>/extension-archives/<id>/<version>_<hash32>.crx` (temp file, then rename), and only then commits the database record. The file name is the record's existing directory name plus `.crx`, so the record finds its archive without a schema change. The archive store is outside `extensions/<id>/`, which is the only place today's collector looks: `extensions::on_open` keeps the children of `extensions/<id>/` that `extension_installs.dir` references and deletes the rest with `remove_whole` (`crates/vsesvit-core/src/extensions/mod.rs`, `on_open` at line 888, `remove_whole` at line 757). So neither WinUI nor an older binary opening the same profile can delete an archive (GPT review 6). `on_open` gains a second pass that deletes archives no record references, which also removes the orphan a crash between rename and commit leaves. "References" follows the first pass exactly: an archive is kept while the directory of the same name is kept, so the withheld exception that keeps a record's last approved directory also keeps that version's archive (§5.5).
3. The shell writes `<root_cache_path>/External Extensions/<id>.json`: `{"external_crx": "<absolute archive path>", "external_version": "<archived version>"}`. Chromium installs it silently as a packed extension (location 2), which survives the incognito toggle and every reload (EL §3).
4. Chromium reads the folder only at start (EL §6.2). After an install the Extensions window says "Restart Vsesvit to finish installing", with a "Restart now" button that quits with `ExitIntent::Relaunch` (§3.6; no update artifact involved) and restores the session (Q6). Startup step 7 (§3.1) re-runs `reconcile()` and rewrites every JSON idempotently, so a crash between commit and restart loses nothing (principle: idempotent operations).
5. Sources External JSON cannot take. Unpacked folders load for one session only, through Chromium's `--load-extension` switch (EL §2 shows such loads reload as unpacked and get disabled). Chromium has no way to load one into a running session through stock CEF (page-session `loadUnpacked` answers "Method not available", EL §5), so there is no hot load (R2-12):
   - **CLI:** `vsesvit --load-extension <dir>` works in the GPUI shell on both OSes (§4.3 row 1).
   - **The Extensions window's "Load unpacked…"** picks a folder, says "Vsesvit will restart with this extension loaded. It stays loaded until you quit.", and on confirmation quits with `ExitIntent::Relaunch` carrying the folder in `load_extensions` (§3.6). The session is restored. The next ordinary start does not load it.
   - With Enhanced Safe Browsing on, the button is disabled with the reason ("Not available with Enhanced Safe Browsing"), because the switch is ignored then (EL §2, `q1-esb.sh` run 2).
   - A session-loaded extension is Chromium's alone: the shell lists it from the runtime record, marks it "Loaded for this session", and never writes it into core.

   WinUI's meaning of the button (a persistent install, inv/winui-extensions lines 142-144) does not carry over. XPI and AMO sources are refused with a message, and the "Install from File (.xpi)" button goes: Chromium cannot keep Gecko ids (inv/webext §5.2).
6. A re-install of an id the user removed must first drop the id from `external_uninstalls` in Chromium's `Preferences` file, before `cef::initialize` (step 7a; EL §9.6; inferred, checked in WX.0).
7. **One profile's set never reaches another.** Any install route must be scoped to one vsesvit profile's `root_cache_path`. A per-user registry key or a file beside `libcef` would apply one profile's extensions to every profile and every other CEF app of the user, so such routes are accepted only if WX.0 shows a per-profile form (GPT review 8).
8. Windows: whether Chromium on Windows reads the per-user External Extensions folder, and whether it prompts (`prompt_for_external_extensions` defaults on for Windows, CE §3.3), is decided by WX.0 under the gate of §5.10.

### 5.3 The helper extension

A small MV3 extension shipped with the browser, source in `crates/vsesvit-cef/ext/helper/`. It has a fixed manifest `key`, so its id is fixed.

- **Installed** as a packed CRX through the same External Extensions JSON as user extensions, written at step 7. `xtask` packs it with a committed key: the id is public anyway, and replacing the CRX on disk already needs write access to the profile. This replaces the draft's `--load-extension`, which Enhanced Safe Browsing blocks: with ESB on, no command-line extension loads (EL §2, `q1-esb.sh` run 2), and turning the pref off from `on_context_initialized` would only take effect at the next start (Opus review 8). EL §9.10 recommends this route. So the user's ESB choice stays theirs, and Vsesvit changes no security pref. On Windows the helper goes through the user extensions' route: the External CRX under outcome A, the second backend's `loadUnpacked` under outcome B (WX.0 must show it loads there with ESB on, §5.10), and nothing under outcome C, where there are no user extensions to manage.
- If the helper does not report within 5 s, the shell logs it, shows "Extension management is unavailable" in the Extensions window, and runs without it.
- **Does:** `management.getAll`, `management.setEnabled`, and forwards `management.onInstalled`, `onUninstalled`, `onEnabled`, `onDisabled` (with the disable reason) to the shell, filtering out its own id. `onInstalled` carries an `ExtensionInfo`, not an install reason (Chrome's `management` API; GPT review 7). The shell derives the reason by comparing the info with its runtime snapshot: unknown id = install, known id with a new version = update. Every other operation is a page-session CDP call (§5.4).
- **Transport, both directions over stock CEF:**
  - shell → helper: the bridge browser (§3.7) at `chrome-extension://<helper>/bridge.html`; `execute_dev_tools_method("Runtime.evaluate", {expression: "vsesvitBridge.call(<json>)", awaitPromise: true, returnByValue: true})`; the result arrives in `DevToolsMessageObserver::on_dev_tools_method_result` and completes the `RequestId`;
  - helper → shell: the bridge page logs `console.debug("vsesvit-bridge " + json)`; `DisplayHandler::on_console_message` of that browser parses it into `BrowserEvent::Extension`. Console messages from any other browser are never parsed.
- **Why not native messaging or the CDP pipe.** Native messaging needs a host manifest and a separate process per OS. The browser-target pipe is verified on Linux only (EL §4). The page session works the same on both OSes; EL §5 verified storage calls through `ExecuteDevToolsMethod` on a page session, and WX.0 verifies the bridge calls. The pipe (`--remote-debugging-pipe`, `spikes/ext-load/src/cdp.rs`) stays the Linux fallback, and its Windows form is outcome B's second route (§5.10).

### 5.4 Engine contract additions

Provisional in `vsesvit-engine/src/extension.rs` until WX.1 freezes it (§1.5).

```rust
// vsesvit-engine
pub enum ExtensionCall {
    List,                                              // helper: management.getAll → Vec<RuntimeInfo>
    Manifest { id: String },                           // the running manifest: hidden chrome-extension://<id>/manifest.json page, its text
    Details,                                           // management browser: developerPrivate.getExtensionsInfo (errors, commands)
    SetEnabled { id: String, enabled: bool },          // helper: management.setEnabled
    SetIncognito { id: String, allowed: bool },        // management browser: developerPrivate.updateExtensionConfiguration
    SetCommand { id: String, command: String, chord: Option<Chord> },   // developerPrivate.updateExtensionCommand
    RequestUpdateCheck { id: String },                 // in the extension: runtime.requestUpdateCheck()
    Uninstall { id: String },                          // re-enable if disabled, hidden manifest.json page, uninstallSelf
}
pub struct RuntimeInfo { pub id: String, pub version: String, pub enabled: bool, pub disabled_reason: Option<String>,
                         pub permissions: Vec<String>, pub host_permissions: Vec<String>, pub install_type: String }
pub enum ExtensionEvent {
    HelperReady, HelperUnavailable,
    Installed(RuntimeInfo), Uninstalled { id: String }, Enabled { id: String }, Disabled { id: String, reason: String },
    LoadFailed { id: String, error: String },          // from Details; replaces UA's ExtensionLoadFailed
}
pub struct ForeignId(pub u64);
pub enum ForeignContext { Normal, Private, Other }   // Other: a request context the shell did not create
trait Engine {
    fn extension_call(&self, call: ExtensionCall) -> RequestId;   // result: RequestResult::Extension(Result<serde_json::Value, String>)
    fn close_foreign(&self, id: ForeignId);
}
enum BrowserEvent {
    Extension(ExtensionEvent),
    ForeignWindowOpened { id: ForeignId, context: ForeignContext },   // neutralized; its URL may not be known yet (§5.7)
    ForeignWindowReady { id: ForeignId, url: Option<Url> },          // first committed URL; None: it settled on about:blank
    ForeignWindowRefused { reason: String },                         // neutralizing failed or no URL came; the glue closed it
    /* … */
}
```

`Uninstall` never resolves on its own: the extension's worker dies during the call (EL §7). The engine completes it on `ExtensionEvent::Uninstalled`, with a 20 s timeout, as WinUI guards its engine calls (inv/winui-extensions §1). The shell then deletes the JSON and calls `Extensions::uninstall`, which removes the record and, with it, the archive.

### 5.5 Updates

- Store extensions (every CRX with an `update_url`): Chromium checks, downloads, verifies against the installed key and applies when the worker idles (about 30 s) or at the next start (EL §6.1). The GPUI shell does not run core's updater for them; core's `update.rs` stays for WinUI (§4.4).
- **The package record does not follow Chromium's updates.** The GPUI shell never writes `extension_installs.version`, the directory, the manifest or the withheld permissions. Writing a new version next to old extracted code would mislead WinUI on the same profile (GPT review 7). The External JSON keeps pointing at the archived CRX and its version. Chromium keeps an installed version newer than `external_version` and does not downgrade (inferred; WX.0 checks it with an update followed by a restart).
- **The UI follows the runtime record.** At `HelperReady` the shell calls `List`, then `Manifest` for each extension, and builds buttons, popups and commands from that manifest, not core's parsed one. Every `Installed` event refreshes both for its id. An update that changed the popup file, icons, commands or permissions is therefore shown as it runs. An update that happened while the bridge was absent (before start, or during a crash) is caught by the next start's `List`, compared with the in-memory snapshot of the previous run's last `List`, persisted in `<root_cache_path>/vsesvit-runtime.json` for exactly this diff.
- Core-managed CRXs without an `update_url`: a new version is a new `commit` (new archive, new JSON version), applied at the next start (EL §6.2).
- The "Update" button calls `RequestUpdateCheck` for every enabled extension and reports what `Installed` events arrive within 30 s.
- Permission-raising updates: Chromium disables such an extension (`onDisabled` with reason `permissions_increase`, inferred). The Extensions window then shows core's re-enable prompt wording (`re_enable_heading`) with the permissions from the runtime record (what Chromium will grant), not core's, and calls `SetEnabled`. Whether that also triggers Chrome's own Views dialog is checked in WX.0.
- **Desired state reaches Chromium only after approval** (R2-8). Core's record holds the desired state: installed, enabled by the user, withheld permissions (not yet approved), allowed in private windows, and supported or not. Today's contract is that a withheld version never runs, because adding it starts it (`InstalledExtension.enabled` includes `!withheld`; inv/winui-extensions lines 102, 344, 694; core `extensions/mod.rs:432-446`). A Chromium install also starts the extension, and the helper's 5 s start-up makes a later disable no barrier. So the writer of step 7 follows a publish rule:
  - **An id not yet in Chromium** (absent from the last runtime snapshot in `vsesvit-runtime.json`) is published only if it is supported, enabled, has no withheld permissions and has an archive. Otherwise it stays out of Chromium, and the Extensions window shows it as "Off" or "Needs your approval". Turning it on, or approving it in core's prompt, records the desired state in core first; the JSON follows at the next start, which the window offers through `Relaunch` (§3.6).
  - **An id already in Chromium** keeps its JSON while it is installed: removing the JSON would uninstall it and its storage (EL §7 run 3). Its enabled state is Chromium's runtime state. The Extensions window's switch writes core's desired `enabled` first (it syncs), then calls `SetEnabled`. A disable that Chromium makes itself (`permissions_increase`, corruption) changes nothing in core. Re-enabling such an extension shows core's re-enable prompt; only the user's answer counts as approval, and `SetEnabled(true)` follows it. The helper never re-enables on its own.
  - **A core-managed package update with more permissions** is withheld by core; the JSON keeps pointing at the last approved archive, which the archive collector keeps while the record's withheld directory exists (§5.2 step 2).
  - **Before start**, step 7a writes Chromium's per-extension disabled state and private-window permission for ids already in Chromium whose desired state changed while the profile was closed (sync from another device). That Chromium honours these `Preferences` entries is inferred; W4.2 checks it. If it does not, the start-up reconciliation below applies them, and the brief run of a remotely disabled extension at start is recorded in `parity.md`.
  - **Start-up reconciliation**, at `HelperReady` after `List`: for each id in Chromium, `SetEnabled` and `SetIncognito` bring Chromium to core's desired state; an id disabled for `permissions_increase` gets the prompt instead. The shell creates the private request context only after this pass finishes or the helper is declared unavailable, so no extension reaches a private window against the user's choice. New Chromium installs are not allowed in private windows until `SetIncognito` says so (Chromium's default).
  - W4.2 tests: a desired-disabled and a desired-withheld record never published (a sentinel extension whose worker requests `/ran?id=<id>` from the fixture server makes no request); a remote disable applied before start; a re-enable after `permissions_increase` without the prompt is impossible through the shell; a private window opened during start-up sees no extension the user kept out of private windows.
- Switching flavours on Windows: WinUI sees core's package record and its own extracted directory, which agree. GPUI's Chromium copy may be newer; it lives in the GPUI engine data (§3.2). W4.4 tests GPUI → WinUI → GPUI on one profile.

### 5.6 Storage

Chromium owns `chrome.storage`. `storage.sync` stays local in the GPUI shell, as on WinUI (inv/winui-extensions §12). It is not bridged into vsesvit sync in these waves; a bridge through each extension's page session (`Extensions.getStorageItems`, EL §5) is a later owner call (Q20). Existing storage from GTK profiles, both `storage.local` and `storage.sync`, moves once, before the extension first runs (§5.9).

### 5.7 Toolbar, popups, commands, menus

- **Buttons** come from the runtime manifest (`action.default_icon`, `default_title`, `default_popup`) and core's pinned layout (`toolbar.rs`). An extension that `List` does not report as enabled shows no button; one with a `LoadFailed` shows an error badge in the puzzle menu with the error text. Dynamic state (badge, `setIcon`, `setPopup`, enable/disable) has no embedder API in any style (CE §1.3), so it is not shown (Q19).
- **Click with a popup**: a `TabKind::ExtensionPopup` browser, Alloy style (so `tabs.query({active, currentWindow})` finds the real tab, EA §3), in an `AuxHost` above the content host, right edge pinned to the button, `set_auto_resize_enabled(25×25, 800×600)` and resized from `on_auto_resize`. It closes on `window.close()` (`on_before_close`), Escape, an outside press and window deactivation (EA §7.1). Keyboard focus moves into it (`FocusTarget::Page`). It gets no `activeTab` grant (EA §3), which is accepted and documented (EA §7.4). Links and `window.open` from the popup open as a tab in the last focused normal window, as today (inv/webext §5.3); its client's `on_before_popup` and `on_open_urlfrom_tab` route them there. A navigation inside the popup itself (`<a target="_self">`, `location.href`) reaches its `on_before_browse` (A.5): a URL of the same extension (`chrome-extension://<id>/…`) stays in the popup; any other URL is cancelled there and opened as a tab in that window, because the popup has no address bar to show a web page's origin. W4.4's `popup` check covers both.
- **Click without a popup**: Chromium offers no way to fire `action.onClicked` here (EA §4); the button opens its menu with a line explaining it (Q19).
- **`_execute_action` shortcut**: intercepted by the shell, opens the shell popup (EA §7.3).
- **Extension commands**: Chromium runs them while the page has focus and grants `activeTab` (EA §5). `on_pre_key_event` never swallows their chords (EA §7.7). The shortcut editor refuses a chord Vsesvit reserves, and removing a binding calls `SetCommand` with `None` so Chromium's binding goes too (inv/dialogs §6). Bindings that already exist are reconciled too (A.6): after `Details` at each start, and whenever Vsesvit's keymap changes, the shell compares every extension command's active chord (from its manifest's suggestion or a stored Chromium binding) with the chords Vsesvit reserves. A conflicting extension binding is cleared with `SetCommand(None)`, and the Extensions window shows "Shortcut removed: it is used by <Vsesvit command>". W4.4 tests a manifest that suggests a reserved chord and a keymap change that newly reserves an extension's chord. With chrome focus there is no API; UA §1.9's fallback (focus the page and replay) stays provisional.
- **Context-menu items** arrive in the `MenuModel` prompt (ids from 49000); the GPUI menu shows them and answers with the engine's id (§1.5, EA §5).
- **Windows Chromium opens itself** (`tabs.create`, `windows.create`, `openOptionsPage`). Chromium shows such a window and focuses it before any event could reach the shell, so the first step is synchronous (GPT review 11). `on_after_created` of a browser the shell did not create (reached through `default_client`) calls `ShellHooks::neutralize_foreign(native)`, which runs `Desktop::neutralize`: X11 unmaps it and reparents it into the hidden host, Windows hides it and sets the hidden host as parent. No `gpui::App` is touched. Then (R2-5):
  1. **Neutralizing failed:** the glue closes the browser at once and emits `ForeignWindowRefused`; nothing is reopened, and the log names the reason.
  2. **Neutralized:** the glue registers it as a live browser, finds its context by comparing its request context with the global and the private one (`is_same`), and emits `ForeignWindowOpened { id, context }`.
  3. **Waiting for a URL.** Creation does not mean the window has a URL. The glue waits for the main frame's first committed URL that is neither empty nor `about:blank`, as `spikes/ext-api/src/shell.rs` does with its `pending_redirect` (line 490, waiting at lines 576-587), and then emits `ForeignWindowReady { id, url: Some(url) }`. If the main frame finishes loading `about:blank` and no navigation starts within 2 s, it emits `Ready` with `None`. If neither happens within 10 s, it closes the browser and emits `Refused`.
  4. **Reopening keeps the context.** On `Ready`, the shell opens the URL, or its new tab page for `None`, as a foreground tab: `Normal` in the last focused normal window; `Private` in the last focused private window, or a new private window if none is open, never a normal one; `Other` (a context the shell did not create, such as a Chromium incognito profile CEF made on its own) is never reopened. Then it calls `close_foreign`.
  5. **What is lost.** CEF reports no opener and no window type for a browser it created itself, so the reopened tab has no opener, and a requested popup window (`windows.create({type: "popup"})`) opens as a tab. Both are recorded in `parity.md`, beside the accepted loss of the returned tab id.

  This close-and-reopen is the default on both OSes. Embedding the window with Chrome's UI cropped off kept the extension's tab identity in one earlier X11 run (EA §0 item 2), but it depends on scale and Chrome's layout, and the cropped window keeps a live tab strip and omnibox that can take keyboard focus (Opus review, note on GPT 11). So embedding is not built in these waves. The cost: an extension that keeps the tab id `tabs.create` returned sees that tab close. WX.0 checks on both OSes that neutralizing shows no flash and takes no focus. DevTools windows are ours, not foreign, and stay native windows.
- **Web Store "Add to Chrome"**: WinUI's store shim (`winui/store.rs` + `js/store.js`, inv/winui-extensions §5) is ported, injected with `Page.addScriptToEvaluateOnNewDocument` on the store origins, so store installs go through core's pipeline and prompt. Its safeguards come with it: the page → shell channel authenticates the sender origin, the "Cancel" default in the confirm, and the origin is checked again when the answer arrives (inv/winui-extensions lines 205-221). The channel is `BrowserEvent::PageMessage` on `PageChannel::Store` (§1.5): its origin comes from the browser process's frame record, never from the page's payload or a console string (A.12). Without the shim Chromium's own `webstorePrivate` flow and Views dialog would install behind core's back (CE §3.5) (Q21).
- **`chrome://extensions`** is not linked from the UI. Its Remove button goes through Chrome's dialog (EL §9.11).
- **Per-extension private windows**: core's `allowed_in_private` → `SetIncognito` (safe for packed installs, EL §3).
- **Notifications and optional permissions.** Chromium shows extension notifications and `permissions.request` dialogs itself. The request dialog is a Views dialog with no CEF hook; `on_show_permission_prompt` covers web permissions only (CE §4). Neither is verified in a toolbar-less child (GPT review 15, Opus review 10). WX.0 tests, on both OSes: an extension notification appears, and its click and its close (by the user and by `notifications.clear`) reach the extension; a per-extension block works (a NOTIFICATIONS content setting for `chrome-extension://<id>`, inferred); `permissions.request` shows a dialog parented to our window and its answer reaches the extension (§5.10 item 9). W4.3 tests the rest (buttons, `onPermissionLevelChanged`, routing to Settings). W4.3 starts only after a decision per control: a verified backend, or the control is removed from the GPUI UI, the matching check (`extension_notifications`, the optional-permission check) is redefined as "unsupported, documented in parity.md", and core's request pieces stay (§4.4).

### 5.8 Native messaging

Chromium looks for native messaging hosts in `<root_cache_path>/NativeMessagingHosts` and `/etc/chromium/native-messaging-hosts` on Linux (CE §1.5), and under HKCU/HKLM `Software\Chromium\NativeMessagingHosts` on Windows (inferred). Password-manager desktop integrations need their manifests linked there; WebView2 picked up Edge's hosts for free, so without this Bitwarden's desktop integration would regress on Windows (Opus review 18). W4.3 adds a Settings note and a "Link native apps" action that copies or registers known host manifests from the Chrome, Chromium and Edge locations, on both OSes.

### 5.9 Existing profiles

Today core unpacks every install into `extensions/<id>/<ver>_<hash>/` and keeps no CRX (inv/winui-extensions lines 535, 551; inv/extensions-sync-updates line 290). Repacking a modified directory cannot reproduce the publisher's signature. So every profile created by the GTK shell, and every Windows profile shared with WinUI, has records with nothing for `external_crx` to point at (GPT review 16, Opus review 9). W4.5 migrates them, in two parts that fit the boot order (R2-7):
- **Before CEF starts** (step 7, thread-free): a record without an archive is marked "needs restoring" in `<root_cache_path>/vsesvit-runtime.json`, with no network and no UI. Its JSON is not written, so it does not run this session.
- **After the first window shows** (step 10a): `extensions/restore.rs` runs the restore jobs on GPUI's background executor and commits their results on main (§4.1). The browser window never waits for them. The Extensions window shows "Restoring your extensions…" and each record's state. A record restored this session becomes active at the next start; the window offers "Restart to finish restoring" (`Relaunch`, §3.6).
- **Store-sourced records** (Chrome Web Store, Edge Add-ons): the CRX is fetched again by id from the store's update endpoint through core's verified pipeline and archived. A newer version goes through core's checks; more permissions mean core's re-enable prompt in the Extensions window before the commit, and a declined prompt leaves the record off. Offline or on an error, the record shows "Could not be restored" with Retry, keeps its mark, and the next start retries it on its own.
- **Local CRX records**: the original file is looked up at its recorded source path. If it is gone, the record shows "Needs the original file" with "Choose file…", which runs a normal install.
- **XPI, Gecko-id and MV2 records**: kept, shown as "Not supported in this version of Vsesvit" with one notice at first start. WinUI and a downgrade still see them.
- **The publish rule of §5.5 applies to restored records**: a record the user had turned off, or one with withheld permissions, is restored into an archive but not published until the user turns it on or approves it.

**What is preserved** (R2-9). Every record keeps a valid extracted directory, so the GTK shell (before wave 7) and WinUI keep working on the same profile. A store refetch that brings a newer version replaces the record's directory, as an ordinary update does in those shells: `commit` replaces the install row's directory (`extensions/mod.rs:418-450`) and the next `on_open` removes the old one (`:895-925`). Rolling back to the pre-migration version of an extension is not offered. The round-1 promise that nothing is deleted is withdrawn; this is the guarantee.

**Storage** (R2-9). GTK profiles keep extension storage in core's `ext_storage_local` and `ext_storage_sync` tables. WinUI keeps it inside WebView2's data, which the GPUI shell cannot read, so extensions from a WinUI-only profile start with empty storage (recorded in `parity.md` and the release notes).
- **When:** at step 7 of the start that first publishes the id, so Chromium has not run the extension yet and nothing races the import.
- **How:** the writer seeds Chromium's own stores, `<cache_path>/Local Extension Settings/<id>/` from `ext_storage_local` and `<cache_path>/Sync Extension Settings/<id>/` from `ext_storage_sync`. They are LevelDB databases whose keys are the storage keys and whose values are the values' JSON text (inferred from Chromium's value store; W4.5 checks it by reading the values back through the extension). The writer is `rusty-leveldb` (§2.1), a pure-Rust LevelDB that compacts on the calling thread, so step 7 stays thread-free (inferred from the crate; `single_thread_init` checks it). It lives in `crates/vsesvit-cef/src/ext/storage_seed.rs`, W4.5's file. It writes `<dir>.tmp` and renames it into place.
- **Crash safety:** the rename is the marker. A crash leaves either no directory, and the next start writes it again, or a complete one.
- **Never overwrites:** if Chromium's directory already exists, nothing is written; Chromium's data wins.
- **Fallback**, if W4.5 shows Chromium does not read a seeded store: after `HelperReady`, `Extensions.getStorageItems`, then `Extensions.setStorageItems` with only the keys Chromium lacks, for both areas, and a marker in `vsesvit-runtime.json` after both. A crash before the marker repeats an import that skips present keys. Its limit: the extension has already run once, so defaults it wrote on install win over old values for the same keys. The owner accepts or rejects that limit in `log.tsv` before W4.5 lands. `setStorageItems` over a page session is WX.0 item 11.

W4.5's tests use profiles created by the current GTK and WinUI shells, not only freshly committed probe CRXs: an offline first start (the window opens, records stay marked, the next online start restores them); a missing local file ("Choose file…" restores it); a store version with more permissions (prompt; declined leaves it off); existing-disabled and existing-withheld records (restored, not published; the sentinel extension makes no request); storage from both GTK areas visible to the extension at its first run; an interrupted import (killed between the temp write and the rename, then restarted); an existing Chromium store left untouched; and a reopen through today's collector code afterwards (every record still has its directory).

Mid-session sync: a remote install commits and shows the restart notice; a remote removal calls `Uninstall`, because removing the JSON alone takes effect only at the next start (EL §7 run 3). A welcome-screen "Add extensions" install says "Installed, active after restart" with the Restart button (Q6).

### 5.10 The extension spike and its decision gate (WX.0)

WX.0 runs in wave 0, beside everything else, because §5 rests on it and the extension part of the contract waits for it (§1.5; GPT review 8, Opus review 12). It touches only `docs/design/linux-cef/spikes/ext-win/**` (its own `[workspace]`) and `spike-ext-win.md`. Windows runs use the sandboxed bootstrap bundle (SW's `bundle-sandbox.ps1`). Every item is answered on Linux and Windows, with two vsesvit profiles open at once whose extension sets conflict.

**Harness.** WX.0 started before W0.5 exists, so it cannot use W0.5's launcher. It copies the earlier spikes' harnesses into `spikes/ext-win/` and hardens the copies to §7.3's rule before its first run (R2-1). On Linux the harness starts its own Xvfb on a free display, sets a fresh token on its root window and refuses to launch unless the target display carries that token; it takes no display from the environment and never reuses a running server. On Windows it refuses to launch unless the thread desktop's name is the private desktop it created; there is no `-Desktop ''`. Native input goes only to the spike's own windows. A result from a run without these checks is not evidence.

| # | Item | Shapes |
|---|---|---|
| 1 | Install route per profile, in order: the per-user folder under `root_cache_path`; a per-profile form of the registry route; `<libcef dir>/extensions/external_extensions.json` only if per-profile (§5.2 step 7). Whether a prompt appears, and whether it can be pre-acknowledged in `Preferences`; whether a Chromium-branded build on Windows accepts an off-store external CRX at all | §5.2, Q5 |
| 2 | With the route: restart and reload persistence, enable/disable, `uninstallSelf` through a hidden Alloy `manifest.json` page, a store update followed by a restart (no downgrade), a permission-raising update (and whether Chrome's Views dialog shows), editing `external_uninstalls` before start | §5.2, §5.5 |
| 3 | The helper as an External CRX loads with Enhanced Safe Browsing on | §5.3 |
| 4 | The page-session bridge: `Runtime.evaluate` on the Alloy bridge page, console messages back | §5.3 |
| 5 | `developerPrivate` over the page session of a short-lived Chrome-style `chrome://extensions` browser in a hidden host; whether hidden Alloy and Chrome-style browsers appear in `tabs.query({})`, `windows.getAll`, `tabs.onCreated` | §3.7 |
| 6 | The Alloy popup in an `AuxHost` above the content host on Windows | §5.7 |
| 7 | Chrome-created windows: `neutralize` shows no flash and takes no focus; close-and-reopen works. Also: a window whose URL is not yet committed at `on_after_created` (the reopen waits for the first committed URL, §5.7); an explicit `about:blank`; an incognito opener (`windows.create({incognito: true})`): which request context the foreign browser is in, and whether the reopened tab is in the same one (R2-5) | §5.7 |
| 8 | The Web Store shim runs before the store's own scripts in a Chrome-style child | §5.7, Q21 |
| 9 | Extension notifications: one appears; a click reaches `notifications.onClicked`; closing it, by the user and by `notifications.clear`, reaches `onClosed`; a per-extension block stops new ones. `permissions.request`: the dialog appears, its parent window, and the answer reaching the extension (GPT 15, A.9) | §5.7 |
| 10 | Extensions in private windows; `chrome.cookies.set` against a site the user blocked (the IO cookie filter does not see extension API writes) | §5.7 |
| 11 | `Extensions.setStorageItems` over a page session | §5.9 |

**The capability matrix.** WX.0 records one pass or fail per row and OS, for the route it tests, in `spike-ext-win.md` (R2-10). The class says what a failure means.

| Capability | Item | Class |
|---|---|---|
| Silent install, scoped to one vsesvit profile; two profiles with conflicting sets stay apart | 1 | fundamental |
| Persistence across restart, extension reload and the incognito toggle | 2 | fundamental |
| Enable and disable | 2 | fundamental |
| Uninstall without a Chromium dialog | 2 | fundamental |
| Updates: no downgrade after an update and a restart | 2 | fundamental |
| Approval: no Chromium prompt on install (item 1); a permission-raising update stops the extension until the shell re-approves it | 1, 2 | fundamental |
| Re-install after removal (`external_uninstalls`) | 2 | fundamental |
| The helper loads through the same route with Enhanced Safe Browsing on | 3 | fundamental |
| The bridge works both ways | 4 | fundamental |
| `developerPrivate`: incognito toggle, shortcuts, load errors | 5 | omission allowed: the controls leave the UI; extensions then never run in private windows (the safe default) |
| Hidden browsers in the `tabs`/`windows` model | 5 | measurement: documented, not a pass or fail |
| The Alloy popup in an `AuxHost` | 6 | mechanism: a failure needs a changed mechanism in §5.7 before WX.1, or the backend is blocked |
| Chrome-created windows: no flash, no focus, reopen with URL and context kept | 7 | mechanism, as above |
| The store shim runs first | 8 | mechanism, or omission allowed: if the shim cannot run first, the store pages' own install path is blocked by navigation policy and installs go through the Extensions window only |
| Notifications: appear, click, close, block | 9 | omission allowed |
| `permissions.request` | 9 | omission allowed (core keeps its request pieces, §4.4) |
| Extensions in private windows; `cookies.set` against a blocked site | 10 | omission allowed: off in private windows; the cookie write is documented as a limitation |
| `Extensions.setStorageItems` | 11 | omission allowed: only the fallback import route of §5.9 needs it |

A **fundamental** failure blocks the route on that OS. A **mechanism** failure needs a changed mechanism in this section before WX.1. An **omission allowed** failure needs the feature marked unsupported in `parity.md`, its check redefined, and the owner's acceptance recorded in `log.tsv` before WX.1. A row cannot be skipped: an untested row counts as a failure.

**The gate.** WX.0 ends with one outcome, recorded in `spike-ext-win.md` and `log.tsv`. "NOT VERIFIED" alone unlocks nothing: WX.1 and wave 4 start only after the outcome is recorded.
- **A.** The External Extensions route passes every fundamental row on both OSes, item 2's lifecycle included, and every other row has a pass or a recorded decision. WX.1 freezes §5.4 as written.
- **B.** Linux as in A. On Windows the External Extensions route fails a fundamental row, and the second route passes every fundamental row on Windows. The second route is the browser-target CDP pipe (`--remote-debugging-io-pipes=<read>,<write>` with inherited anonymous pipe handles, inferred from Chromium's Windows pipe code) with `Extensions.loadUnpacked` at every start from the archive unpacked by core. Page-session `loadUnpacked` is not an option: it answers "Method not available" (EL §0 item 14, §5). Opening the pipe is not enough. WX.0 tests the whole backend against the matrix: two profiles, the loads repeated at every start, persistence across an extension reload and the incognito toggle despite EL §3's caveat (unpacked loads get disabled without developer mode), enable, disable, uninstall, updates (unpacked loads get no Chromium updates, so core's updater must update them: new archive, loaded at the next start), and the helper loaded by the same backend with Enhanced Safe Browsing on. Then WX.1 freezes a contract with a per-OS install backend behind the same `ExtensionCall`s, and rewrites §5.3, §5.5 and W4.2 for the Windows route.
- **C.** Linux as in A; neither route passes every fundamental row on Windows. The Windows GPUI flavour ships without user extensions: the Extensions window says "Extensions are not available in this build", `parity.md` records it as a blocker for flipping the Windows default, and WinUI stays the default. Wave 4 still builds and tests everything on Linux, and the Windows build compiles the same code with the install backend returning that message. There is no helper on Windows.
- **D.** A fundamental row fails on Linux. Wave 4 does not start. The owner chooses between a redesign of §5 (for example a CEF patch) and a Linux GPUI shell without user extensions; the second blocks wave 7 unless the owner accepts it in `log.tsv`.

The notification behaviours that WX.0 does not test (buttons, `onPermissionLevelChanged`, routing to Settings from a notification) are W4.3's: it tests each on the outcome's backend, and each that fails is named unsupported in `parity.md` (GPT 15, A.9).

## 6. The `gpui` feature on `crates/vsesvit`, CI and packaging

### 6.1 The features

One mechanism on both OSes (orchestrator decision after the Opus review's finding 1). `gpui` selects the GPUI shell; `gtk` and `winui` select today's shells and are both on by default, each a no-op on the other OS. The default build of every OS stays the shipped shell until wave 7, so a nightly built from `main` keeps shipping GTK on Linux and WinUI on Windows while the GPUI shell is a skeleton. A runtime switch was rejected: it would ship WebView2 and CEF (about 200 MB more) in one package for no user gain (log.tsv, 2026-10-23).

```toml
# crates/vsesvit/Cargo.toml, waves 0 to 6
[features]
default = ["gtk", "winui"]
gtk = ["dep:vsesvit-gtk"]                         # the Linux shell until wave 7; nothing on Windows
winui = ["dep:vsesvit-winui"]                     # the Windows shell; nothing on Linux
gpui = ["dep:vsesvit-gpui", "dep:vsesvit-cef"]    # the GPUI shell, on either OS

[dependencies]
vsesvit-gpui = { workspace = true, optional = true }
vsesvit-cef = { workspace = true, optional = true }

[target.'cfg(target_os = "linux")'.dependencies]
vsesvit-gtk = { workspace = true, optional = true }

[target.'cfg(windows)'.dependencies]
vsesvit-winui = { workspace = true, optional = true }

[target.'cfg(windows)'.build-dependencies]
winresource = "0.1.31"
```

The Opus review's Cargo probe (`.audit/scratch/design/cargo-probe`, cargo 1.99.0) confirmed that `dep:` features over per-OS optional dependencies resolve as intended, that `gpui` alone and `gpui` with the defaults both build the GPUI `main`, and that no feature at all fires the `compile_error!`. W0.1 repeats `cargo tree --target x86_64-unknown-linux-gnu` and `--target x86_64-pc-windows-msvc` with the real crates, for the default set and for `--features gpui`.

```rust
// crates/vsesvit/src/main.rs, waves 0 to 6
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
use std::process::ExitCode;

#[cfg(all(feature = "gpui", any(target_os = "linux", windows)))]
fn main() -> ExitCode {
    let sandbox = vsesvit_cef::Sandbox::none();                 // Windows release enters through vsesvit-sandbox instead
    if let Some(code) = vsesvit_cef::run_subprocess(&sandbox) { return code; }
    vsesvit_gpui::run(&vsesvit_cef::CefEngineFactory::new(sandbox))
}

#[cfg(all(not(feature = "gpui"), target_os = "linux", feature = "gtk"))]
fn main() -> ExitCode { vsesvit_gtk::run() }

#[cfg(all(not(feature = "gpui"), windows, feature = "winui"))]
fn main() -> ExitCode { vsesvit_winui::run() }

#[cfg(all(not(feature = "gpui"), target_os = "linux", not(feature = "gtk")))]
compile_error!("enable the `gtk` or the `gpui` feature");
#[cfg(all(not(feature = "gpui"), windows, not(feature = "winui")))]
compile_error!("enable the `winui` or the `gpui` feature");

#[cfg(not(any(target_os = "linux", windows)))]
fn main() -> ExitCode { eprintln!("vsesvit supports Windows and Linux only"); ExitCode::FAILURE }
```

- `gpui` wins when it is on, so `--features gpui` and `--all-features` build the GPUI browser (with the old shell linked in for nothing). The development command is `cargo run -p vsesvit --features gpui`; packaging uses `--no-default-features --features gpui`.
- **Wave 7 flips Linux** (W7.1): `gtk` and `vsesvit-gtk` go, the Linux GPUI dependencies become plain under `cfg(target_os = "linux")` (the arrangement the probe also confirmed), `default = ["winui"]`, and the GPUI `main` becomes `#[cfg(any(target_os = "linux", all(windows, feature = "gpui")))]`. Windows is untouched by the flip.
- `crates/vsesvit-sandbox` exports `RunWinMain`, which runs the same two calls with the sandbox pointer it receives: `run_subprocess(&Sandbox::from_raw(info))`, then `vsesvit_gpui::run(…)`.
- `crates/vsesvit/build.rs` keeps the `winresource` icon for both Windows flavours and adds `cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN` on Linux (G14); `crates/vsesvit-helper/build.rs` adds the same rpath.

### 6.2 xtask

| Command | Change |
|---|---|
| `package <formats> [--flavour gpui]` | Linux formats with `--flavour gpui` build `-p vsesvit --no-default-features --features gpui` and `-p vsesvit-helper`, and stage the CEF runtime (§6.4); without it they build GTK as today, until W7.1 makes GPUI the only Linux flavour. New Windows format `nsisgpui` builds `-p vsesvit-sandbox` (no feature flags: the crate has none, and Cargo rejects `--features gpui` on it, as the probe showed) into `target/gpui` (its own target dir, so the two Windows flavours do not rebuild each other) and stages the bootstrap image. `nsis` stays WinUI and unchanged |
| `vendor-gpui [--bump <crate> <version>] [--check]` | new (§2.2) |
| `check-deps` | new, from `cargo metadata` plus `syn` and greps. **Scope:** workspace member crates (`crates/*`, `xtask`); never `third_party/` (vendored GPUI and its examples) or the excluded `docs/design/linux-cef/spikes`. **Normal dependencies:** `vsesvit-engine` on no `gpui` or `cef`; `vsesvit-gpui` on no `cef` or `vsesvit-cef`; `vsesvit-cef` on no `gpui`; `vsesvit-cef-child` on nothing but `cef`; no crate has `test-support` in its normal dependencies. **Dev-dependencies**, an allow list: `vsesvit-cef` → `vsesvit-gpui` with `test-support` and `vsesvit-testenv` (the `smoke` example, W1.3); `vsesvit-gpui` → `vsesvit-testenv`; any crate → `vsesvit-core` with `testkit`. Anything else that crosses a layer rule fails. **Source:** no `unsafe impl Send` or `Sync` in `vsesvit-cef`; every example `main` and every window-opening test binary in scope starts with the prologue of §7.3 (the sub-process return then `require()` in a crate that links `cef`, `require()` alone elsewhere). The rules of §1 as a check, not a convention (R2-2) |
| `testenv` | new: the launcher half of §7.3, in `xtask/src/testenv/{mod,linux,windows}.rs` (Linux: Xvfb, its auth file and root property, a private `dbus-run-session`; Windows: the private desktop of SW §1.1, moved from the spike, and Q22's Windows Sandbox route). The commands below use it; it has no subcommand of its own |
| `selftest [--flavour gpui\|winui] [--network]` | new: builds (including `-p vsesvit-helper` for GPUI), then runs `--self-test OUT_DIR` inside a fresh test environment, prints `report.json` (§7.3) and fails unless the report lists every check of the flavour's expected list. W0.5 builds the `winui` branch; the `gpui` branch exits 4 "not available yet" until W1.8 |
| `probe <example> [args]` | new: builds and runs one example of `vsesvit-testenv`, `vsesvit-gpui` or `vsesvit-cef` inside a fresh test environment, building `-p vsesvit-helper` only for a `vsesvit-cef` example. The only documented way to run an example |
| `hand-test` | new: starts the real browser with a scratch profile on the user's own desktop for `hand-tests.md` (§7.4). No scripted input, no self-test code |
| `helper-check` | new (W1.3): `readelf -d` on the built `vsesvit-helper`; fails if `NEEDED` lists anything beyond libcef, libc, libm and libgcc_s (§1.1) |
| `pack-helper` | new (W4.2): packs `crates/vsesvit-cef/ext/helper/` into `helper-extension.crx` with the committed key (§5.3) |
| `stamp-exe <exe> --icon … --version …` | new, Windows: writes our icon and version resources into the renamed `bootstrap.exe` with `UpdateResourceW` (Q15) |
| `bundle-sandbox` | new, Windows: lays out the sandboxed image (renamed bootstrap, `vsesvit.dll`, CEF runtime) in `target/gpui/bundle`, as SW's `bundle-sandbox.ps1`; used by W1.3's checks and by `package nsisgpui` |

### 6.3 CI

There is no test workflow today; `release.yml` only packages (inv/packaging-ci; code survey). W0.1 adds `ci.yml`, run on pushes and pull requests. Its steps appear when the unit that makes them pass lands (§7.5), never earlier, and no step runs a stub. `ci.yml`'s owners, in order: W0.1, W0.5, W1.8, W6.6, W7.1 (§8.1).

| Job | Runner | Steps |
|---|---|---|
| `linux` | `ubuntu-latest`, container `ubuntu:26.04` | apt: today's GTK shell build set (`libgtk-4-dev`, `libadwaita-1-dev`, `libwebkitgtk-6.0-dev`, `libglib2.0-bin` and the rest of `release.yml`'s list), kept while `vsesvit-gtk` and `vsesvit-webext` are workspace members (GPT review 18, Opus review 1); plus build-essential, pkg-config, libxkbcommon-dev, libxkbcommon-x11-dev, libxcb1-dev, libfontconfig-dev, libglib2.0-dev; the CEF runtime set libnss3, libatk-bridge2.0-0, libcups2, libgbm1, libasound2, libgtk-3-0 (Chromium's GtkUi), mesa-vulkan-drivers (lavapipe); xvfb, xauth, ibus (the IME check). `actions/cache` for `CEF_PATH` keyed `cef-154.0.34-linux`. Steps: `xtask vendor-gpui --check`; `xtask check-deps`; `cargo clippy --workspace --all-features`; `cargo test --workspace`; from W1.8, which adds every wave-1 step in one edit, `xtask helper-check` (W1.3's `readelf -d` check), `xtask selftest --flavour gpui`, and `cargo build --workspace --all-features` with the foundation checks run on that binary (§1.4) |
| `windows-winui` | `windows-latest` | `cargo test --workspace` with default features (today's WinUI coverage; the CEF crates build too because they are members, cached the same way); from W0.5, `xtask selftest --flavour winui` on the runner's private desktop, if Q22's private-desktop route works (Windows Sandbox is not available on hosted runners) |
| `windows-gpui` | `windows-latest` | CMake and Ninja (for `libcef_dll_wrapper`, SW §2); `CEF_PATH` cache keyed `cef-154.0.34-windows`; `cargo build -p vsesvit --no-default-features --features gpui`; `cargo test -p vsesvit-engine -p vsesvit-cef -p vsesvit-gpui`; from W1.8, `xtask selftest --flavour gpui` |

A CI runner's desktop is not a user's, but the self-test still runs in a test environment there, so the same command behaves the same on a developer's PC (§7.3).

`release.yml` changes in W6.6:
- The published set does not change before wave 7: GTK packages on Linux, `nsis` (WinUI) on Windows. W6.6 adds the GPUI builds as workflow artifacts only: Linux formats with `--flavour gpui`, and `nsisgpui` in its own `windows-gpui` job (CMake, Ninja, the CEF cache), so a GPUI failure cannot block the WinUI release.
- `sign`: the glob `*-setup.exe` (`release.yml:218`) already matches `…_x64-gpui-setup.exe`.
- `release`: `xtask manifest` iterates `Format::ALL`, which gains `NsisGpui`; the manifest lists it only once it is published.
- `flatpak` job: the new manifest of §6.4 builds as an artifact; it stays skipped on nightly.
- `xtask/tests/release_workflow.rs` pins every new action by SHA, as it does today.

W7.1 then switches what is published in one commit with the GTK removal: the Linux formats become GPUI, the GTK apt packages leave both workflows, and `nsisgpui` joins the published assets.

### 6.4 Packages

The CEF runtime is copied beside the exe by `cef-dll-sys` (SC §2, SW §2) and staged from there. Sizes are measured from the 154.0.34 distributions (inv/packaging-ci §0.1; SW §2).

| Format | Layout | Sandbox | Dependencies and notes |
|---|---|---|---|
| deb, rpm, pacman | `/usr/lib/vsesvit/`: `vsesvit`, `vsesvit-helper`, `libcef.so` (stripped: 272 MB, about 83 MB zstd), `chrome-sandbox`, `*.pak`, `icudtl.dat`, `v8_context_snapshot.bin`, `locales/`, `libvk_swiftshader.so`, `libvulkan.so.1`, `vk_swiftshader_icd.json`, `helper-extension.crx`, `CREDITS.html`, `package-format`; `/usr/bin/vsesvit` symlink | namespace; `chrome-sandbox` installed 4755 as the fallback (deb `set_mode`, rpm `%attr(4755,root,root)`); an AppArmor profile allowing user namespaces for Ubuntu 23.10+ in the deb | new runtime dependency table (nss, nspr, glib, atk/at-spi, dbus, cups, X11 libs, gbm, expat, xkbcommon-x11, udev, alsa, vulkan loader, fontconfig/freetype; P §8.2). **GTK 3 stays a dependency:** Chromium loads it as GtkUi, the only source of page IME on Ozone/X11 and of its GTK dialogs (SC §3.1 saw `libgtk-3` mapped; W §1.2; GPT review 12). The IM bridges (`ibus-gtk3`, `fcitx5-frontend-gtk3`) are recommends, as the desktop installs them. GTK 4, libadwaita, WebKitGTK and GStreamer go. rpm has `AutoReqProv: no`, so every library is a written `Requires:` |
| AppImage | same tree under `usr/lib/vsesvit/` | namespace only (the mount is `nosuid`); where user namespaces are blocked, the owner call of Q29 applies, never a silent `--no-sandbox` | drop the WebKit relocation and GStreamer bundling; reconcile the sysroot's host-provided list with the AppImage excludelist for NSS, atk, dbus, cups and GTK 3 with its IM modules (host-provided, P §10.3) |
| Flatpak | `/app/lib/vsesvit/` | CEF cannot nest its namespace sandbox in Flatpak's; use zypak (Chromium base app) as the launcher | the 326 MB CEF tarball becomes an offline manifest source with a SHA-256 we compute; `CEF_PATH` set; unset the `FLATPAK` env var that makes `cef-dll-sys` look in `/usr/lib` (P §9). `--socket=x11` replaces `fallback-x11` + `wayland`: in a Wayland session `fallback-x11` grants no X socket, and this app is X11-only (Opus review 19). The build runs `-p vsesvit -p vsesvit-helper` and skips `docs/design/linux-cef/spikes` (§2.2). GTK 3 comes from the runtime |
| NSIS (WinUI, `nsis`) | unchanged | WebView2 | |
| NSIS (GPUI, `nsisgpui`) | `$LOCALAPPDATA\Programs\Vsesvit\`: `vsesvit.exe` (renamed `bootstrap.exe`, stamped), `vsesvit.dll`, `libcef.dll`, `chrome_elf.dll`, `d3dcompiler_47.dll`, `dxcompiler.dll`, `dxil.dll`, `vk_swiftshader.dll`, `vk_swiftshader_icd.json`, `vulkan-1.dll`, `icudtl.dat`, `v8_context_snapshot.bin`, `*.pak`, `locales\` (`File /r`), `helper-extension.crx`, `vsesvit.ico`, `package-format` = `nsisgpui` | bootstrap | about 423 MB installed; the LZMA installer must stay under the updater's 512 MiB cap (measured in W6.4); the Windows App Runtime check is compiled out; `CloseApp` waits for every `vsesvit.exe` process, since CEF's children are the same image |

**Metadata.** The `.desktop` file keeps its 16 MIME types and gains `StartupWMClass` matching GPUI's X11 class; `desktop-file-validate` runs in W6.1. The metainfo, `deb.rs` and `rpm.rs` descriptions lose "built on WebKitGTK" (`metainfo.xml:13`, `deb.rs:27`, `rpm.rs:37`). `Installation::detect` is checked under the zypak launcher in W6.3.

**One Vsesvit per user on Windows.** Both NSIS flavours install into the same directory under the same uninstall key, ProgIDs and `StartMenuInternet` entry. Each installer first deletes the other flavour's payload list (xtask knows both and passes them as defines). Running the other installer switches flavour in place. The profile is shared; engine data is not (§3.2), so site logins do not carry over between flavours.

**Licences.** CEF and Chromium add BSD-3 and Chromium's notices. `CREDITS.html` ships in every package, and the metainfo licence field gains them (P §3; `ctx.rs:148` test updated).

### 6.5 The updater and the flavour

Linux needs nothing new: from wave 7 the GPUI shell replaces GTK under the same `deb`, `rpm`, `pacman` and `appimage` variants, so existing installs update in place to GPUI then, and not before. Windows separates the flavours:

| Change | Where | Why |
|---|---|---|
| `Installation::NsisGpui`, `Format::NsisGpui`, marker and variant `nsisgpui` (no hyphen) | `vsesvit-update` `installation.rs`, `updater.rs`; `xtask` `ctx.rs` | the variant is the third `-` part of the manifest key; the manifest test at `manifest.rs:220-223` checks 2 or 3 parts |
| artifact `Vsesvit_<v>_x64-gpui-setup.exe`, key only `windows-x86_64-nsisgpui` | `xtask` `ctx.rs` | the plain `windows-x86_64` key is WinUI's fallback |
| flavoured variants never fall back to `<target>-<arch>` | `release.rs:94` | otherwise a GPUI install whose release lacks its key silently installs WinUI; signature, version and the `MZ` check (`installation.rs:120`) all pass |
| download-cache names parsed longest suffix first | `updater.rs:417` | `-setup.exe` would otherwise match `…-gpui-setup.exe` as version `1.0.0-gpui` |
| the update check never runs in a CEF sub-process | `vsesvit-gpui` boot (sub-processes return at step 0 anyway) | inv/extensions-sync-updates D.9 |
| restart after `cef::shutdown`; none after a stuck shutdown | §3.6 | both locks free |

Two issues grow with CEF's size and are Q16: verification reads the whole artifact into memory twice (`updater.rs:446`), and `rpm -U` resolves no new dependencies on the GTK → GPUI update.

## 7. Testing

### 7.1 Layers

| Layer | What | Runs where | Needs |
|---|---|---|---|
| core unit tests | every moved module of §4.3 brings its tests from GTK or WinUI (the 8 `cli.rs` tests, `ClosedTabs`, the startup plan, the omnibox edit model's 8 tests, the 11 `permissions.rs` tests, the download tracker, `lifecycle.rs`, sync `due`/`settle`), plus the archive store and its collector (§5.2) | `cargo test`, every OS | nothing |
| `vsesvit-engine` | `FakeEngine` behaviour (UA §4.5), the prompt protocol's exactly-once and cancellation rules (§1.5) | `cargo test` | nothing |
| `vsesvit-testenv` | the guard's refusals (§7.3) | `cargo test`, every OS | nothing; the tests fake the environment, they never open a window |
| `vsesvit-gpui` | views and entities in `gpui::TestAppContext` with `FakeEngine` and `FakeSurface` (`cfg(test)`): UA §5.3 acceptance tests (layout `classify`, close rules, title cases, `TabListModel`, router delivery, `Hooks` never borrows `App` or core, keymap and routing tables, overlay bookkeeping, the page-focus policy) | `cargo test`, every OS | no display, no CEF, no X server (`xtask check-deps` keeps it so) |
| `vsesvit-cef` | pure parts: switch table per OS, IDC default-deny map, reserved-key table, transition and disposition mapping, `MenuModel` conversion from a recorded model, `strings.rs` helpers, teardown state machine, the registry's count and thread check. None creates a browser | `cargo test` | libcef to link (cached in CI) |
| platform probes | `crates/vsesvit-gpui/examples/native_probe/`: a GPUI window with a foreign child (an x11rb window or a plain HWND standing in for CEF); geometry follows layout, show/hide, holes route a press to GPUI (hit test), focus both ways, capture shows the child's colour, PopUp owner fix-up, `toplevel_at`, `neutralize`, a `SetFocus` inside a GPUI update | `xtask probe` only (§7.3) | no CEF |
| smoke | `crates/vsesvit-cef/examples/smoke.rs` (§8.3, W1.3) | `xtask probe` only | CEF |
| self-test | `vsesvit --self-test OUT_DIR [--network]`: the real binary, in process, scripted (§7.2) | `xtask selftest` only | CEF |
| hand tests | real mouse, IME, other display scales, multi-monitor (§7.4) | the user's own desktop, only when the user runs them | the user |

### 7.2 The self-test port

The contract stays `docs/design/self-test.md` and inv/self-test §10: same flags, exit codes, stdout lines, `report.json`, per-check timeouts with "last observed", cleanup guards, scripted mode, fixture server, probe extension. Check names come from `core::testkit::checks` (§4.3 row 14): the shared list plus the GPUI flavour's extras, so `Report::complete` never asks for a WinUI-only check. The guard test that scans the source for `ctx.check("` is ported.

How each harness need is met under CEF:

| Need | GTK / WinUI today | GPUI + CEF |
|---|---|---|
| page eval with a result | `evaluate_javascript_future` / WebView2 | `engine.eval` → DevTools `Runtime.evaluate` through `execute_dev_tools_method` (API §3.7) |
| keys and clicks into the page | WebKit / DevTools input | CEF `send_key_event` / `send_mouse_click_event` for page-level checks. Key-routing regressions need the native path: Linux XTEST into the private X server through `vsesvit_testenv::PrivateInput` (works on Xvfb, EA §1); Windows `PostMessage` to our own HWNDs (the `keypost` of SW §1.2), which is the only route that exercises the GPUI key patch |
| keys into the chrome | GTK actions | `window.dispatch_keystroke` and actions |
| screenshots with popovers | GSK render + popover composite | `Surface::capture` (includes the page, SC §11, SW §11) + PopUp windows composited at their positions |
| extension install | in-process | the probe CRX is committed in step 7 before `cef::initialize`, so it loads at start. The restart path (install while running → restart → loaded) runs as a phase: the process re-executes itself as `vsesvit --self-test OUT_DIR --phase restart` on the same profile and merges the reports. The reviewer still runs one command |
| shutdown | the process's own exit | phases `shutdown-delayed` and `shutdown-stuck` on scratch profiles; the parent asserts exit codes 0 and 70 and the log (§3.6) |
| capture devices | WebKit mock devices | `--use-fake-device-for-media-stream` and `--auto-select-desktop-capture-source` (§3.3); real prompts |

Checks that change meaning (inv/self-test §5):
- `memory_saver` asserts the frozen state of Q9, not a terminated web process.
- `session` and the reopen checks assert URLs, not back/forward lists (Q7).
- `install_crx`, `cws_install`, `engine_loaded_extension`, `extension_update` follow §5: archive and JSON written, loaded at start, Chromium's running version reported, core's package record unchanged by an update.
- `popup`, `extension_toolbar` assert the shell-hosted Alloy popup and buttons from the runtime manifest.
- `extension_windows` asserts EA §2's model (one popup window per tab). `extension_web_navigation`, `extension_cookies`, `context_menus`, `extension_commands` compared shell tab ids with the ids extensions see. Chromium's tab ids are not the shell's `TabId`s (Opus review appendix A.11), and a URL is no identity: two tabs, or a private and a normal tab, can share one, and a redirect changes it (R2-14). So the harness builds an explicit map when it creates each tab: it opens the fixture URL with a unique token (`?t=<check>-<n>`), the probe extension reports `tabs.onCreated`/`onUpdated` with the tab id, the `incognito` flag and the URL, and the harness maps that token to the shell's `TabId` and browsing. Later steps use the map, not the URL, and a token that matches no tab or two tabs fails the check. Each of these checks includes a duplicate-URL case: two tabs navigated to the same final URL after creation, where the command, menu item, cookie or webNavigation event must land on the intended one.
- `spellcheck` uses Chromium's dictionaries; `tracking_protection` can now assert a blocked count.
- `cookies` and site-data checks assert localStorage and IndexedDB, not only cookies (§3.7).
- Windows-only checks a Chromium shell can adopt (`bookmarks_bar_icons`, `download_pause`, `shortcuts_sync`) join the GPUI list. `passwords_purged` does not: the purge is WebView2-only (inv/winui-engine, lines 1450-1452).

New checks: `single_thread_init` (§3.1), `shutdown_delayed`, `shutdown_stuck`, `shutdown_late_creation` (§3.6), `page_focus_policy` (the cases listed in §1.5, including a programmatic focus into the page while the omnibox has focus, and a real click that focuses a page field; another of our windows active takes neither focus nor activation), `js_dialogs`, `beforeunload`, `file_chooser`, `http_auth` (each answered, and each cancelled by navigation; `http_auth` also through a fixture proxy, asserting the dialog names the proxy's host and port, not the page; §1.5), `crashed_page`, `ime_compose` (ibus composition into the page and the omnibox inside the test environment; Q12; W2.5 owns it).

### 7.3 Where tests run, and never run

The PC this is developed on is the user's. Two spikes showed why tests must not touch its display: Chromium focuses new browsers itself and steals the foreground (SW §1.1), and a private D-Bus session leaked the owner's display into portal dialogs (SC §1). The rule: **no self-test, example or test binary that creates a window ever runs on the user's display or desktop, and there is no override.** The draft's `VSESVIT_SELFTEST_ON_MY_DESKTOP` is gone (GPT review 1, Opus review 3). The user's own runs are hand tests (§7.4), a separate mode with no scripted input.

**The test environment** is made by `xtask/src/testenv` (W0.5) for `xtask selftest` and `xtask probe`, and checked by `vsesvit_testenv::require()`.

| | Linux | Windows |
|---|---|---|
| Display | **Xvfb** on a free display `:N`, `-nolisten tcp`, with a fresh MIT cookie in a private auth file. Xvfb is what the extension spikes ran XTEST on (EA §1, EL §1) and installs in CI. Headless Weston with Xwayland is not used: no spike ran it, and under Xwayland (KWin) XTEST pointer motion did not work (SC §1; Opus review 4). Xvfb has no window manager, and activation "does not settle" there (EA §4); W1.1's probe decides whether the activation and focus checks need one, and if so the launcher starts openbox on the same Xvfb, which keeps XTEST in-server. Never `:0`, never WSLg | a private desktop created with `CreateDesktopW` and the process started with `STARTUPINFO.lpDesktop` (SW §1.1). Windows can activate, focus and go fullscreen there without reaching the user's screen |
| Identity the launcher creates | a fresh 128-bit token, set as the `_VSESVIT_TESTENV` property on the private server's root window, and written with the display, the Xvfb pid and the private D-Bus address to `OUT_DIR/testenv/env.json` (mode 0600) | the desktop name `vsesvit-test-<token>`, written to `OUT_DIR/testenv/env.json` |
| Environment | `env -i`, a private `dbus-run-session`, `DISPLAY=:N`, `XAUTHORITY` = the private file, no `WAYLAND_DISPLAY`, `HOME = OUT_DIR/home`, lavapipe, `VSESVIT_TESTENV` = the path of `env.json` | `OUT_DIR` for every path; `--disable-features=CalculateNativeWinOcclusion`; `VSESVIT_TESTENV` |
| What `require()` checks | `env.json` exists, is ours and mode 0600; `DISPLAY` is its display; `WAYLAND_DISPLAY` is unset and the D-Bus address is its own; the X server named by `DISPLAY` answers and its root window's `_VSESVIT_TESTENV` equals the token; the server's pid from `/tmp/.X<N>-lock` is the recorded one and `/proc/<pid>/exe` is Xvfb | the thread desktop's name (`GetThreadDesktop` + `GetUserObjectInformationW(UOI_NAME)`) is `vsesvit-test-<token>` from `env.json`; `OpenInputDesktop` succeeds and its name differs. Names are compared, never handle values. The Windows Sandbox route of Q22 (WinUI's self-test only, if XAML refuses a private desktop) instead requires the token file in the folder the launcher mapped and the account `WDAGUtilityAccount` |
| Input | XTEST only through `PrivateInput`, which only a `TestEnv` can build | `PrivateInput` posts messages only to HWNDs of our own process (`GetWindowThreadProcessId`); never `SendInput` |

The guard ties the run to the display or desktop the launcher created, not to a variable anyone can set: an inherited `DISPLAY=:0`, a copied `env.json` or a stale token fails, because the user's X server has no such root property and the user's desktop has another name. Every failure, including a query that errors, refuses: exit 3 with the reason on stderr.

**Where `require()` runs.** `require()` guards browser-process entry points only. A CEF sub-process is not an entry point: it must run `api_hash` and `execute_process` before anything else (§1.1), its sandbox may forbid the desktop query, and it opens no top-level window. Sub-processes inherit their parent's desktop (Windows, `STARTUPINFO.lpDesktop` left null) or `DISPLAY` and `XAUTHORITY` (Linux), and their parent passed the guard (R2-1).
- **The browser binary under `--self-test`:** boot step 0 (`run_subprocess`) returns in a sub-process; the browser process reaches step 2a (§3.1), which calls `require()` before `cef::initialize` and before any GPUI window.
- **Examples that link CEF** (`smoke`): `main` has exactly this prologue, and nothing before it:
  ```rust
  fn main() -> ExitCode {
      if let Some(code) = vsesvit_cef::run_subprocess(&Sandbox::none()) { return code; }   // sub-process: no guard
      let env = vsesvit_testenv::require();                                               // browser process: exit 3 on doubt
      …
  }
  ```
- **Examples without CEF** (`native_probe`, `theme_gallery`, `overlay_demo`, `vsesvit-testenv`'s `raw_window`) and every test binary that opens a window: `require()` is the first statement.
- **Re-executed self-test phases** (`--phase`) are browser processes: they pass step 0 and run step 2a again in the inherited environment.

`xtask check-deps` checks the prologue by parsing each `main` with `syn`: a crate that depends on `cef` (normally or as a dev-dependency) must start with the two statements above; any other must start with `require()`. The check covers the `examples/` and window-opening tests of workspace member crates only (§6.2). A direct `smoke --type=renderer` from a shell takes the sub-process branch, finds no browser to connect to and exits; it opens no window. W0.5's negative tests include: an example started without the launcher exits 3 before any window; a re-executed phase passes; a sub-process of a guarded run starts and renders (W1.3's `smoke` run shows it).

**Input boundary.** `TestEnv` has a private constructor, so code cannot send XTEST or post input without a passed guard. `clippy.toml` `disallowed-methods` bans windows-sys `SendInput`, `keybd_event`, `mouse_event` and `SetForegroundWindow`, x11rb's `xtest_fake_input` outside `vsesvit-testenv`, and `gpui::Window::activate_window` (it calls `SendInput` and `SetForegroundWindow`, SW §1.1). Clippy cannot see GPUI's internal activation (`WindowOptions { focus: true }`, `Window::resize` without `SWP_NOACTIVATE`, SW §1.1). Those are harmless inside a private desktop, and the Windows launcher keeps SW §1.1's watchdog: it records the user input desktop's foreground window before and after each run and fails the run if it changed to one of ours.

**Negative tests** (W0.5, in `vsesvit-testenv` and `xtask`): token mismatch; `DISPLAY` pointed at another server after launch; `env.json` missing, foreign-owned or world-readable; `VSESVIT_TESTENV` inherited into a shell with `DISPLAY=:0`; a Windows desktop query that fails; a thread desktop equal to the input desktop; a re-executed phase that still passes.

**WinUI's self-test** keeps its checks; only where it runs changes. W0.3 and lane C, which migrate WinUI code (§4.3), run it through `xtask selftest --flavour winui`, which W0.5 builds (§8.2): the default Windows binary started with `--self-test OUT_DIR` on the launcher's private desktop, and the report validated against WinUI's expected check list (47 names, from `winui/report.rs` until C.0 moves the list into core). Zero checks, a missing check or a missing report fails the run. Whether XAML runs on a private desktop is not known (Q22). W0.5 finds out, before W0.3 can land:
- XAML runs there: done.
- It does not: the launcher's Windows Sandbox route runs the same command in a disposable VM (Q22). That route needs the Windows Sandbox feature turned on, which is the owner's action, not a test.
- Neither works: the WinUI self-test gate stays open. W0.3 and lane C do not land until one route works. No scripted run on the user's desktop is a fallback, with or without the user's permission (R2-1).

**Old spike harnesses.** The spikes' own scripts predate this rule. `spikes/core/run.sh`, `spikes/ext-load/env.sh` and `spikes/ext-api/env.sh` take any display from `SPIKE_DISPLAY`; `spikes/ext-load/start-xvfb.sh` treats an already running server on that display as its own; `spikes/win-core/scripts/lib.ps1` runs on the user's desktop with `-Desktop ''` (R2-1). W0.5 removes those branches: the Linux scripts take no display from the environment and refuse to reuse a running server, and `lib.ps1` refuses an empty `-Desktop`. WX.0 does not run the old scripts. It runs its own hardened copies (§5.10).

### 7.4 Hand tests

Only the user runs these, on the user's own desktop, with `xtask hand-test` (the real browser with a scratch profile, no scripted input), at the end of the waves that make them meaningful: real mouse and wheel input; IME (ibus or fcitx on Linux, Microsoft IME on Windows) in the omnibox and in the page; 100, 150 and 200 % display scale and moving a window between monitors (SW §5 could test only 175 %); the owner's real KWin session (SC §13.4 used only a virtual one); forwarded-launch activation under a Wayland session through XWayland (inv/shell-core, lines 274-277); drag and drop between the page and GPUI; a bookmark dragged from one window's bar to another's (A.18). Each has a written script in `docs/design/linux-cef/hand-tests.md`, created by W2.5.

### 7.5 Gates

Gates grow with the waves; a unit is held only to the gates that exist when it lands (GPT review 20).

| From | Gate |
|---|---|
| every unit | builds: Linux default (GTK), Linux `--features gpui`, Windows default (WinUI, unchanged behaviour), Windows `--no-default-features --features gpui`; `cargo test --workspace` on both OSes; `cargo clippy --workspace --all-features`; `xtask check-deps`; `xtask vendor-gpui --check` |
| W0.5 | every runtime acceptance (an example, a probe, a capture) runs through `xtask probe` or `xtask selftest`; nothing else opens a window. A unit that edits WinUI passes `xtask selftest --flavour winui` with all 47 checks; a zero-check or "not available yet" result never passes a gate |
| W1.8 | the self-test with every check that exists so far passes on Linux and Windows; `cargo build --workspace --all-features` passes the foundation checks (§1.4) |
| W1.8 | standing regression checks in the self-test, below |

Standing regression checks:
- **Resources.** 5 warm-up open/close cycles, then a baseline: browser-process thread count, child process count, open fds (Linux) or handles plus GDI and USER objects (Windows). 25 more cycles, a 2 s settle, then the child process count equals the baseline, and fds or handles are at most 16 above it (Chromium's caches; SC §6, SW §6 saw no growth).
- **Shutdown.** Quit leaves no process and exits 0 (SC §12, SW §12); `shutdown_delayed`, `shutdown_stuck` and `shutdown_late_creation` (§3.6).
- **Keys.** A key posted to the page reaches it; a key typed in the omnibox with the pointer over the page stays in the omnibox (the two GPUI patches).
- **Focus.** `page_focus_policy` (§1.5).
- **Start-up.** `single_thread_init` (§3.1).

## 8. Implementation waves

### 8.1 Rules

- **Every unit is independently landable.** It builds, passes the gates of §7.5 that exist when it lands, and is committed on its own. A unit that cannot be finished is not landed half-way; its stubs stay.
- **One owner per file at a time.** W0.1 creates every new file of §1.2, §1.3 and UA §5.1 (as amended here) with its frozen stub, and each starts with `//! Owned by <unit>.` (`check-deps` checks the line exists). W0.1 also declares every new module of `vsesvit-core`, `vsesvit-sync` and `vsesvit-update` as an empty file, so no later unit edits a `lib.rs` to add one. A later unit fills only its own files. A file has one owner at a time: two units that can run at the same time never share a file. A file may pass to a later unit only when that unit depends on the earlier one, directly or through the chain; the tables name every owner (`ci.yml`: W0.1 → W0.5 → W1.8 → W6.6 → W7.1; `xtask/src/selftest.rs`: W0.1 → W0.5 → W1.8; `xtask/src/testenv/linux.rs`: W0.5 → W1.1). §8.11's script checks this rule over the tables (R2-2).
- **Dispatch to stubs.** A wave-1 file that must reach a later feature (`window/actions.rs` → print, view source, zoom; `app/quit.rs` → final sync, clear on exit, the two update intents (§3.6); the tab and window close paths of W1.5/W1.7 → `downloads::park_decision` in W3.3's `downloads/keep_alive.rs` (§4.2); `boot/` step 7 → `vsesvit-cef/src/ext/external.rs` (W4.2) and step 10a → `extensions/restore.rs` (W4.5) (§3.1, §5.9); `state/prefs.rs` → every pref's fan-out; `engine.rs` → every `Engine` method; the router → every event) calls a function in the later unit's own file. W0.1 writes that function's signature with a body that does nothing or returns `Unavailable`, never `todo!()` on a path wave 1 can reach. The later unit replaces the body and edits nothing else (Opus review 2).
- **Parallel units touch disjoint files.** The table below is the ownership map; §8.11 checks it.
- **WinUI belongs to lane C.** From the end of W0.3 to wave 7, `crates/vsesvit-winui/**` is owned by lane C, whose units run one at a time (§4.3). No other unit edits WinUI.
- **Self-test checks** go in the unit's own file, `self_test/checks/<unit>.rs` (declared by W0.1), so the self-test grows with the features.
- **`log.tsv` is append-only.** Units add lines and never edit one; it is the one shared file, and appends cannot conflict in meaning.
- **Each wave ends with a review** (an Opus subagent over the wave's diff), then the next wave starts.

Paths without a crate are in `crates/vsesvit-gpui/src/`.

### 8.2 Wave 0: skeleton, contracts, test environment, extension spike

| Unit | Files | Depends | Done when |
|---|---|---|---|
| W0.1 skeleton | root `Cargo.toml` (`exclude`, `[patch.crates-io]`, workspace deps, rust-version, dev profile); `.gitattributes`; `clippy.toml` (the full ban list of §7.3); `third_party/**`; `xtask/src/main.rs` with every new command, `xtask/src/{vendor_gpui,check_deps}.rs`, and a stub file for each later command; `crates/vsesvit/**`; every new crate's `Cargo.toml` and stub tree; `crates/vsesvit-gpui/{Cargo.toml, build.rs, src/lib.rs, src/app/actions.rs, examples/native_probe/main.rs}`; the empty `self_test/checks/<unit>.rs` of every unit that adds checks (§8.1); `.github/workflows/ci.yml` with only the steps that can pass in wave 0 (§6.3: no `selftest` and no `helper-check` step; the stubs of those commands exit 4, "not available yet", never 0); the new core, sync and update module files | — | the four builds of §7.5 compile; existing tests pass; Linux default runs GTK and Windows default runs WinUI, unchanged (their existing test suites pass, and `cargo tree -p vsesvit` shows no CEF crate in either default); the `cargo tree --target` checks of §6.1 pass; `cargo tree -i gpui-pre-linux` and `-i gpui-pre-windows` show the `third_party` path; `vendor-gpui --check` and `check-deps` pass in CI |
| W0.2 engine contract | `crates/vsesvit-engine/**` except `src/extension.rs` | W0.1 | §1.5 and the rest of UA §4.2 compile with `#![deny(missing_docs)]`; `FakeEngine` (UA §4.5) with the prompt tests of §1.5 (every kind; duplicate and wrong-kind answers; IO prompts admitted after close, navigation and `begin_shutdown`; menu dismissal), `PageFocus::allows` over every combination, the two shutdown phases with a creation that completes after `begin_shutdown` (closed at once, counted until its close), and `detach_tab` |
| W0.3 core moves A | core: `cli` (with `--phase`), `session/plan`, `closed_tabs`, `zoom` presets, `profiles::window_title`, `shortcuts` in-page policy and capture rules (§4.3 rows 1–6); WinUI: `cli.rs`, `config.rs`, `session.rs`, `browser.rs` (closed tabs), `zoom.rs`, `shortcuts.rs`, the title code | W0.1, W0.5, W0.6 | moved tests pass in core; the per-shell flag table of §4.3 row 1 has tests (`--load-extension <dir>` parses for WinUI and GPUI; `--ui-smoke` for GPUI exits 2); WinUI uses the core versions and a grep finds none of its old copies; `cargo test -p vsesvit-winui` passes; `xtask selftest --flavour winui` passes with all 47 checks reported, on the private desktop or the Windows Sandbox route (Q22). No other route satisfies this gate |
| W0.4 theme | `theme/**`, `crates/vsesvit-gpui/assets/**`, `examples/theme_gallery.rs` | W0.1, W0.5 | UA U02 acceptance; `xtask probe theme_gallery` captures light and dark on both OSes |
| W0.5 test environment | `crates/vsesvit-testenv/**`; `xtask/src/testenv/**`; `xtask/src/{probe,hand_test,selftest}.rs` (of the last, the WinUI branch; W1.8 adds the GPUI branch); `.github/workflows/ci.yml` (adds the `windows-winui` self-test step); WinUI's two test entries, `crates/vsesvit-winui/src/selftest.rs` (`prepare`) and `crates/vsesvit-winui/src/automation/mod.rs` (`prepare`, for `--ui-smoke`), which call `require()` first, and `crates/vsesvit-winui/Cargo.toml` (`vsesvit-testenv` under its `self-test` feature); the old spike scripts of §7.3: `docs/design/linux-cef/spikes/{core/run.sh,ext-load/env.sh,ext-load/start-xvfb.sh,ext-api/env.sh,win-core/scripts/lib.ps1}` | W0.1 | `xtask probe raw_window` runs `vsesvit-testenv`'s own `examples/raw_window.rs` (an x11rb window, a plain HWND) inside Xvfb and inside a private desktop, and the example's `PrivateInput` delivers a key to it; the negative tests of §7.3 pass, including the example started without the launcher; the Windows watchdog fails a run whose foreground window changes; `xtask selftest --flavour winui` reports 47 checks from WinUI's binary on the private desktop, or Q22's Windows Sandbox route is recorded as the working route, or Q22's "neither" is recorded in `log.tsv` (then W0.3 waits); `xtask selftest --flavour gpui` exits 4 "not available yet"; a report with zero or missing checks fails `xtask selftest` (a unit test over recorded reports); the old spike scripts refuse an inherited display, an existing server and an empty `-Desktop` |
| W0.6 WinUI self-test on a private desktop | `crates/vsesvit-winui/**` (until W0.3 starts); `README.md` (its self-test paragraph); `docs/design/self-test.md` | W0.5 | `xtask selftest --flavour winui` passes all 47 checks on the private desktop (Q22's first route, log.tsv 2026-10-24: 30 of 47 passed because Windows.Graphics.Capture refuses windows off the input desktop and WebView2 stopped answering after `profiles`); the capture fallback works on and off the input desktop; the root cause of the `profiles` stall is recorded; README and `self-test.md` say to run the self-test through `xtask selftest`, since a direct `--self-test` now exits 3 |
| WX.0 extension spike | `docs/design/linux-cef/spikes/ext-win/**` (its own `[workspace]`, including its hardened copies of the earlier harnesses), `docs/design/linux-cef/spike-ext-win.md` | — (already running; it does not wait for W0.5, so it hardens its own copies first, §5.10) | every item of §5.10 is answered on Linux and Windows with evidence, from runs whose harness passed the fail-closed checks of §5.10; the per-OS capability matrix and the gate outcome are recorded in `spike-ext-win.md` and `log.tsv` |

W0.1 lands first. W0.2 and W0.5 follow in parallel; W0.3 and W0.4 follow W0.5. WX.0 starts at once and runs beside waves 0 and 1. Exit code 4 ("not available yet") is the only answer an unbuilt `xtask` command gives; no gate accepts it, and no CI step calls a command before the unit that builds it lands (R2-2).

### 8.3 Wave 1: foundation

| Unit | Files | Depends | Done when |
|---|---|---|---|
| W1.1 platform X11 | `platform/{mod,fake}.rs`, `platform/x11/**`, `examples/native_probe/x11.rs`, `xtask/src/testenv/linux.rs` (only to start a window manager, if the probe needs one) | W0.1, W0.5 | every `native_probe` check of §7.1 passes under `xtask probe` on Xvfb; whether a window manager is needed is decided and recorded in `log.tsv`, and the launcher starts one if so; `park_page` moves a child into the hidden host, the source window detaches, and the child is still alive; `neutralize` on a destroyed window returns an error; `FakeSurface` and `FakeDesktop` pass their trait tests; the focus patch's upstream PR is open (linked in `log.tsv`) |
| W1.2 platform Windows | `platform/windows/**`, `examples/native_probe/windows.rs` | W0.1, W0.5 | every `native_probe` check passes under `xtask probe` on a private desktop, including a `SetFocus` into the child from inside a GPUI update, the frame's hit-test regions, `park_page` and `neutralize` as on X11, and a posted mouse press reaching `pointer_presses` (the self-test's clicks are posted messages, §7.2; whether `WH_MOUSE` sees them is inferred); the key patch's upstream PR is open (linked in `log.tsv`) |
| W1.3 CEF engine core | `crates/vsesvit-cef-child/**`; the `crates/vsesvit-cef` files marked W1.3 in §1.2; `crates/vsesvit-cef/examples/smoke.rs`; `crates/vsesvit-helper/**`; `crates/vsesvit-sandbox/**`; `xtask/src/windows/bundle_sandbox.rs`; `xtask/src/helper_check.rs` | W0.2; W1.1 and W1.2 for the smoke example, which hosts browsers in `platform::Surface` through a dev-dependency on `vsesvit-gpui` with `test-support` (`check-deps` allows it in dev-dependencies only) | the unit tests of §7.1, including the re-entrancy test of §1.6; `xtask probe smoke` on both OSes (an ordinary bin, so the same-exe re-exec on Windows works; libtest binaries cannot be re-executed as `--type=renderer`, API §3.1) creates the maintenance browser, creates, navigates, switches and closes three tabs, reaches a live count of zero through both shutdown phases and exits 0 with no processes left; the `smoke` prologue of §7.3 (a direct start without the launcher exits 3; its sub-processes start); a page-initiated `window.close()` on a Chrome-style tab arrives as UA's unrequested `TabEvent::Closed` and sends no close to the GPUI window (`do_close` runs only for Alloy, API G16); the `PageMessage` channel delivers from an allowed origin and drops a message from any other origin or a subframe; `xtask helper-check` passes; the Windows sandboxed bundle shows renderers `Lockdown` in `chrome://sandbox`; Linux shows "adequately sandboxed" |
| W1.4 boot | `boot/**` except `boot/webkit_import.rs`; `app/quit.rs` | W0.3, W1.1, W1.2, W1.3 | boot order §3.1 including the picker branch (with the stub picker view), step 2a, step 7's metadata-only pass and step 10a's hand-off to the migration stub; the shutdown mechanism of §3.6 (`PumpHandle`, both phases, the late-creation rules, the cap, `Stuck`, exit 70) and the exit intents (`Relaunch` with no update artifact, through an injected spawner); instance tests on both OSes (forward to an in-process listener, exit status and stderr relayed, the unreachable message); headless commands work without a display; an initialization failure reaches `fatal_message`; a release build without the helper refuses with its message, a debug build falls back to same-exe mode |
| W1.5 state and tab core | `app/{mod,shell}.rs`, `state/**` except `state/memory_saver.rs`, `tab/{mod,model,navigation}.rs`, `util/**` | W0.2, W0.3 | the UA U05 and U06 acceptance tests that need no later feature (session throttle, last-tab-in-session, active window first, pinned and grouped restore, `TabListModel` rules, invocation outcomes, `releasing_the_profile_frees_its_lock`), with `FakeEngine`; ending a private session calls the downloads stub before closing the last private tab, then ends the context (§4.2); closing a tab asks `downloads::park_decision` first, and on `Park` calls `park_page` then `detach_tab` and removes the tab on `TabEvent::Detached` (tested with a stub that answers `Park`) |
| W1.6 bridge | `engine_bridge/**` | W0.2, W0.3 | UA U07 acceptance; the routing table is built from core's in-page policy; with `FakeEngine` and `FakeDesktop`, `set_page_focus(PageFocus::NEW_BROWSER)` precedes every `create_tab`, a policy precedes every `load`, the policy follows activation, tab switches and keyboard-owner changes, a pointer press in the page rect of the active window hands the page the keyboard, and a `FocusChanged { focused: true }` the policy did not allow returns the keyboard to its owner (§1.5); every `Prompt` reaches its owner's stub and an unanswered prompt ends on `PromptEnded`; `PumpHandle::stop` returns the driver |
| W1.7 window and minimal chrome | `window/**` except `window/fullscreen.rs`, `tabs_ui/{mod,row,strip,sidebar}.rs`, `toolbar/mod.rs` (back, forward, reload, a plain address field) | W1.1, W1.2, W1.5, W1.6 | the UA U08 TestAppContext tests that apply; closing a window parks every tab the downloads stub marks `Park` before `Surface::detach` (§4.2; `FakeSurface`); on Windows the window's background is `Transparent` and nothing paints the content rect (a TestAppContext assertion); its runtime acceptance is W1.8's foundation checks |
| W1.8 self-test foundation | `self_test/{mod,report,runner,probes,capture}.rs`, `self_test/checks/foundation.rs`, `test_support.rs`, `xtask/src/selftest.rs` (the GPUI branch), `.github/workflows/ci.yml` (the wave-1 steps of §6.3) | W1.1–W1.7, C.0 | `xtask selftest` passes on Linux and Windows with `profile_open`, `navigate`, `tabs`, `tab_layout`, `session`, `screenshot`, a `shortcuts` subset and every standing regression check of §7.5 (resources, shutdown including `shutdown_delayed`, `shutdown_stuck` and `shutdown_late_creation`, keys, `page_focus_policy`, `single_thread_init`); the `--all-features` binary passes the same foundation checks; release-build idle CPU is recorded in `log.tsv` for both OSes (Q23) |
| WX.1 extension contract freeze | `crates/vsesvit-engine/src/extension.rs` (including FakeEngine's extension half), and the extension lines it adds to `crates/vsesvit-engine/src/{command,event,fake}.rs` (`extension_call`, `close_foreign`, `BrowserEvent::Extension`, the ForeignWindow events, FakeCommand variants; W0.2 left them out, log.tsv 2026-10-24); `docs/design/linux-cef/design.md` §5 and §9 | WX.0, W0.2 | §5.4 is frozen as the gate outcome dictates and `design.md` §5 is updated from `spike-ext-win.md`; FakeEngine's extension calls and events have tests; every §5.10 item that failed has its recorded decision |

W1.1, W1.2, W1.3, W1.5, W1.6 and WX.1 run in parallel (W1.3's smoke check waits for W1.1 and W1.2; WX.1 waits for WX.0). W1.4 and W1.7 follow; W1.8 closes the wave. **End of wave 1:** a browser window with tabs, navigation, correct focus and a correct shutdown on both OSes, behind `--features gpui`.

### 8.4 Lane C: core moves B (serial, beside waves 1 and 2)

| Unit | Core files | Done when (each) |
|---|---|---|
| C.0 check registry | `core::testkit::checks` (§4.3 row 14) | moved tests pass in core; WinUI uses the core version and a grep finds none of its old copy; `cargo test -p vsesvit-winui` passes; `xtask selftest --flavour winui` passes with every check of the moved registry's WinUI list (47) |
| C.1 omnibox model | `core::omnibox_edit` (row 7) | same, plus WinUI's 8 omnibox tests pass on the core model |
| C.2 permissions and privacy | `core::permissions::queue`, `core::privacy` (rows 8, 11) | same |
| C.3 downloads and OS helpers | `core::downloads::Tracker`, `core::os` (rows 9, 15) | same |
| C.4 bookmarks | `core::bookmarks::{menu, bar, editor}`, `core::favicons::fit_png` (row 10) | same |
| C.5 sync scheduler | `vsesvit_sync::schedule` (row 12) | same |
| C.6 update lifecycle | `vsesvit_update::lifecycle` (row 13), in its own new file `crates/vsesvit-update/src/lifecycle.rs` | same |

The lane owns `crates/vsesvit-winui/**` from W0.3's end to wave 7, and each C unit edits whichever WinUI callers its move needs (§4.3). Order: C.0 → C.1 → … → C.6, starting after W0.3. Consumers wait for their C unit: W1.8 for C.0, W2.2 for C.1, W3.1 for C.2, W3.3 for C.3, W3.2 for C.4, W3.6 for C.5, W3.7 for C.6.

### 8.5 Wave 2: chrome and page prompts

| Unit | Files | Depends | Done when |
|---|---|---|---|
| W2.1 overlays and dialogs | `overlay/**`, `dialogs/{mod,host,prompts,library}.rs`, `examples/overlay_demo.rs` | W1 | `xtask probe overlay_demo` over a foreign child on both OSes: a menu with a submenu, checked and radio items, whose items take clicks over the page; a passive list driven from an entry; outside-press dismissal; bookkeeping tests (UA U04) |
| W2.2 omnibox | `omnibox/{mod,edit,field,suggestions,progress,zoom_bubble}.rs`, `self_test/checks/omnibox.rs` | W2.1, C.1 | UA U11 acceptance; engine schemes go to the engine; self-test `omnibox`, `address_completion`, `search_suggestions`, `address_progress` |
| W2.3 tabs UI | `tabs_ui/{tab_menu,tab_search,group_editor,drag}.rs`, `self_test/checks/tabs_ui.rs` | W2.1 | UA U09 acceptance; tab drag between windows with `move_page_to`, falling back to reopen (UA S:10); Q25's link-drag result recorded; self-test `tab_menu`, `tab_search`, `tab_groups`, `ctrl_s_toggles_sidebar` |
| W2.4 toolbar menu and find | `toolbar/main_menu.rs`, `find_bar.rs`, `crates/vsesvit-cef/src/handlers/find.rs`, `self_test/checks/menu_and_find.rs` | W2.1 | UA U10 acceptance except the update banner (W3.7); self-test find and menu checks |
| W2.5 page features | `tab/{content,error_page}.rs`, `window/fullscreen.rs`, `context_menu/**`, `save_page.rs`, `crates/vsesvit-cef/src/{schemes.rs, handlers/context_menu.rs}`, `docs/design/linux-cef/hand-tests.md`, `self_test/checks/page.rs` | W2.1 | the GPUI context menu from the `MenuModel` prompt with Chromium's items and ours, an engine item answered by id and a shell item run by the shell; print, DevTools, view source, MHTML save; zoom with core presets (Q10 decided); fullscreen and notice; link preview with Chrome's status bubble off; new tab page, error, certificate, HTTPS-only and crashed-page views under `vsesvit://`; self-test `new_tab_page`, `https_only`, `zoom_indicator`, `page_commands` (for view source it records both the URL `on_before_browse` saw and the URL the omnibox shows; A.16), `save_page`, `selection_search`, `crashed_page`, `ime_compose` (Q12); `hand-tests.md` written |
| W2.6 page prompts | `tab/prompts.rs`, `platform/files.rs`, `crates/vsesvit-cef/src/handlers/{jsdialog,dialog,auth}.rs`, `self_test/checks/page_prompts.rs` | W2.1 | JS alert, confirm and prompt, beforeunload, the file chooser in every `FileChooserMode` (portal on Linux, `GetOpenFileNameW`/`GetSaveFileNameW` on Windows, with filters; `OpenFolder` through GPUI's prompt) and HTTP auth (the dialog names the challenger, server or proxy) work through `PromptRequest`; the prompt sequences of §1.5 against the real glue; self-test `js_dialogs`, `beforeunload`, `file_chooser` (including `<input webkitdirectory>`), `http_auth`, each answered and each cancelled by navigation |

All six run in parallel after W2.1's frozen overlay API (W2.1 lands first or its stubs are used). **End of wave 2:** a browser that is usable for daily browsing on both OSes.

### 8.6 Wave 3: features

| Unit | Files | Depends | Done when |
|---|---|---|---|
| W3.1 privacy | `omnibox/{site_info,permission_prompt}.rs`, `tab/permissions.rs`, `privacy/**`, `dialogs/settings/{privacy,capture,site_data,site_permissions}.rs`, `crates/vsesvit-cef/src/{handlers/permission.rs, handlers/resource_request.rs, handlers/cookie_access.rs, site_data.rs}`, `self_test/checks/privacy.rs` | W2, C.2 | UA U12 acceptance; the start-up clear runs before the first restored load and clear on exit runs at shutdown step 4; the permission types of §4.2 have a policy; self-test `site_permissions`, `tracking_protection`, `cookies` (localStorage and IndexedDB too), `connection_info`, `capture_in_use`, `private_window` |
| W3.2 bookmarks | `bookmarks/**`, `favicons.rs`, `dialogs/{bookmarks,history}.rs`, `crates/vsesvit-cef/src/handlers/favicon.rs`, `self_test/checks/bookmarks.rs` | W2, C.4 | UA U13 acceptance; a page with only `/favicon.ico` gets its icon; a bookmark dragged from one window's bar and released over another window's bar moves there (a TestAppContext test with two windows and `FakeDesktop::toplevel_at` naming the target; A.18), and the real pointer in §7.4's hand tests; self-test bookmark checks |
| W3.3 downloads | `downloads/**` (including `downloads/keep_alive.rs`), `toolbar/downloads_button.rs`, `dialogs/downloads.rs`, `platform/shell_open.rs`, `crates/vsesvit-cef/src/handlers/download.rs`, `self_test/checks/downloads.rs` | W2, C.3 | UA U14 acceptance; the keep-alive tests of §4.2 (tab close, non-last window close, a download-only popup, a self-closing popup measured and recorded, private session); a normal download carries `user.xdg.origin.url` on Linux and a private one does not (A.22); no file ever lands in the real Downloads folder; Q2 resolved; self-test `download`, `download_safety`, `download_speed`, `download_pause` |
| W3.4 profiles | `profiles/**`, `toolbar/profile_button.rs`, `self_test/checks/profiles.rs` | W2 | UA U15 acceptance; self-test `profiles` |
| W3.5 settings | `dialogs/settings/{mod,general,appearance,search,shortcuts,search_engines}.rs`, `dialogs/{about,shortcuts_help}.rs`, `platform/default_browser.rs`, `crates/vsesvit-cef/src/prefs.rs`, `self_test/checks/settings.rs` | W2 | UA U17 acceptance, with its Linux-only negative tests under `cfg(target_os = "linux")` (§4.2.1); the default-browser row works on both OSes; self-test `settings`, `spellcheck`, `search_engines` |
| W3.6 sync and welcome | `sync.rs`, `dialogs/{welcome,passphrase}.rs`, `dialogs/settings/sync.rs`, `self_test/checks/sync.rs` | W2, C.5 | UA U18 sync acceptance; Q26 checked with the sign-in flow; self-test `sync_passphrase`, `welcome` |
| W3.7 updates | `updates/**`, `toolbar/update_banner.rs`, `dialogs/settings/updates.rs`, `self_test/checks/updates.rs` | W2, C.6 | UA U18 update acceptance; banner and Settings rows on both OSes |
| W3.8 Memory Saver and audio | `state/memory_saver.rs`, `tab/sleep.rs`, `crates/vsesvit-cef/src/lifecycle.rs`, `self_test/checks/memory_saver.rs` | W2 | Q9 decided and implemented with `set_frozen`; Q11 implemented with the document-start script; self-test `memory_saver` |

All eight run in parallel; their files are disjoint as listed, and none edits core or WinUI (lane C did). **End of wave 3:** feature parity with the GTK shell's checklists except extensions.

### 8.7 Wave 4: extensions

Wave 4 starts after WX.1, so it is planned on the recorded outcome of §5.10, not on an unknown.

| Unit | Files | Depends | Done when |
|---|---|---|---|
| W4.1 core extension adaptation | `crates/vsesvit-core/src/extensions/**` except `migrate.rs` | WX.1 | the archive store and the collector's second pass of §5.2 with tests: install → reopen → archive kept and loaded at start; an orphaned archive removed; a withheld update keeps the last approved version's archive; a profile opened by today's collector code keeps the archive; core and WinUI tests pass |
| W4.2 CEF extension glue | `crates/vsesvit-cef/src/ext/**` except `ext/storage_seed.rs`, `crates/vsesvit-cef/ext/helper/**`, `xtask/src/pack_helper.rs` | WX.1 | `extension_call` and `ExtensionEvent` (§5.4) work against the probe extension on Linux, and on Windows unless the outcome is C; the helper installs through the outcome's route (External CRX on Linux and under A; the second backend on Windows under B) and reports with Enhanced Safe Browsing on; management browsers are closed after each call; the desired-state reconciliation of §5.5 with its tests |
| W4.3 extension UI | `extensions/**` except `extensions/restore.rs`, `dialogs/{extensions,extension_prompts}.rs` | WX.1 (W4.2 for integration; codes against `FakeEngine` first) | UA U16 acceptance with `sync_extensions` replaced by the runtime snapshot (`List`, `Manifest`, `Installed`); restart-to-finish flow; popups anchored and dismissed per §5.7; the store shim with its safeguards; "Link native apps" on both OSes; the notification and optional-permission decisions of §5.7 applied, and the notification behaviours §5.10 leaves to W4.3 (buttons, `onPermissionLevelChanged`, routing to Settings) each tested on the outcome's backend or named unsupported in `parity.md`; the store shim's messages arrive only as `PageMessage` from a store origin (a message from another origin is dropped) |
| W4.5 extension migration | `crates/vsesvit-core/src/extensions/migrate.rs`, `extensions/restore.rs`, `crates/vsesvit-cef/src/ext/storage_seed.rs`, `crates/vsesvit-core/tests/fixtures/profiles/**` | W4.1, W4.2 | §5.9's tests on fixture profiles made by the current GTK and WinUI shells: store records restored after the first window, offline first start, missing local CRX, permission increase, XPI and MV2 shown as unsupported, existing-disabled and existing-withheld records not published (sentinel), both GTK storage areas seeded before the first run, interrupted import, existing Chromium store untouched, every record still has a directory after a reopen; the storage route used (seeding or the fallback with its accepted limit) recorded in `log.tsv` |
| W4.4 extension checks | `self_test/checks/extensions.rs` | W4.1–W4.3, W4.5 | the extension checks of inv/self-test §5, as redefined in §7.2, pass (both OSes, or Linux plus the "not available" check on Windows under outcome C); an update that changes the popup file and permissions is shown as it runs; an update while the bridge was absent is caught at the next start; GPUI → WinUI → GPUI on one Windows profile keeps both flavours working; "Restart now" after an install relaunches with no browser update downloaded and the extension is loaded (§3.6); "Load unpacked…" relaunches with the folder loaded, and the following ordinary restart does not load it (R2-12); no Chromium update check runs during the run (the fixture store's update endpoint logs no request before the check triggers one), which shows `--extensions-update-frequency` works, not only that it was passed (A.10); the identity rules of §7.2 |

### 8.8 Wave 5: self-test completion and parity

| Unit | Files | Done when |
|---|---|---|
| W5.1 complete the self-test | `self_test/checks/remaining.rs` | every check of the GPUI list (the shared 43, the GPUI extras, the adopted Windows checks, the new checks of §7.2) runs and passes on Linux and Windows; the ported guard tests pass |
| W5.2 parity audit | `docs/design/linux-cef/parity.md` | every item of every inventory checklist is marked done, changed on purpose (with the reason), or a gap; every gap is classified **release-blocking** or **accepted** (an accepted gap names the owner's decision in `log.tsv`); the Windows flip blockers of §4.2.1 and §5.10 are listed. An issue alone never makes a gap accepted |
| W5.3 hand tests | (none; findings become issues) | the user has run `hand-tests.md` on both OSes and every finding is filed and classified like a W5.2 gap |

### 8.9 Wave 6: packaging and CI

Wave 6 touches only `xtask`, `packaging/`, `vsesvit-update`'s format files and the workflows, so its scaffolding can start any time after wave 1 and run beside waves 2 to 5. Starting is not landing (R2-2): a unit lands only when its whole done-when passes, and some acceptances need later units. The Depends column says what a unit needs to start; "lands after" names what its acceptance needs. Every self-test named below means the checks that exist when the unit lands. Wave 6 must finish before wave 7. Nothing it builds is published before wave 7 (§6.3).

| Unit | Files | Depends | Done when |
|---|---|---|---|
| W6.5 updater and formats | `crates/vsesvit-update/src/{installation,updater,release,install}.rs` (the last because its `apply`, `install_on_exit` and `package_command` match on `Installation` and `Format`, lines 51-69 and 117-124, so `NsisGpui` and Q16 edit it; R2-2), `xtask/src/{ctx,manifest}.rs` | W1 | §6.5's changes with tests; `Downloaded::install` and `install_on_exit` work for `NsisGpui` (unit tests over the installer parameters, as `passive_mode_relaunches_with_escaped_args` does today); Q16 resolved |
| W6.1 Linux native packages | `xtask/src/linux/{mod,util,elf,stage,deb,rpm,pacman,deps}.rs`, `packaging/linux/**` except `packaging/linux/appimage/` | W6.5; lands after W2.5 (`ime_compose`) and W4.2 (`pack-helper`) | `--flavour gpui` deb, rpm and pacman install on a clean container, start, show "adequately sandboxed", and pass the self-test from the installed path inside a test environment; `ime_compose` passes from the installed artifact on a minimal image (GTK 3 runtime declared); `verify.sh` rewritten with per-format sandbox checks; `desktop-file-validate` passes; no "WebKitGTK" left in package metadata |
| W6.2 AppImage | `xtask/src/linux/{appimage,sysroot}.rs`, `packaging/linux/appimage/**` | W6.5; lands after W4.2 (`pack-helper`) | runs on a clean container; sandboxed, or the Q29 behaviour where user namespaces are blocked |
| W6.3 Flatpak | `xtask/src/linux/flatpak.rs`, `packaging/flatpak/**` | W6.5; lands after W4.2 (`pack-helper`) | offline build with the CEF source; zypak launcher; `--socket=x11`; the helper is built; the spikes are skipped; the generator test with a `[patch]` path entry passes; the self-test passes inside the sandbox; `Installation::detect` reports Flatpak |
| W6.4 Windows GPUI installer | `xtask/src/windows/{mod,stamp_exe}.rs`, `packaging/windows/**` | W6.5; lands after W1.3 (`bundle-sandbox`) and W4.2 (`pack-helper`) | `nsisgpui` installs, upgrades, switches flavour in both directions, uninstalls clean; the stamped exe shows our icon and version and still loads `vsesvit.dll` sandboxed (Q15); installer size recorded and under 512 MiB |
| W6.6 workflows | `.github/workflows/**`, `xtask/tests/release_workflow.rs` | W6.1–W6.5 | a dispatch run of `release.yml` publishes exactly today's set and uploads every GPUI artifact as a workflow artifact; `ci.yml` jobs of §6.3 green |

W6.5 lands first, because W6.1 to W6.4 read its formats; then W6.1, W6.2, W6.3 and W6.4 run in parallel and land when their "lands after" units have landed; W6.6 closes the wave.

### 8.10 Wave 7: switch-over and docs

| Unit | Files | Done when |
|---|---|---|
| W7.1 flip Linux, delete the old shell | root `Cargo.toml`; `crates/vsesvit/**`; `crates/vsesvit-gtk/**` and `crates/vsesvit-webext/**` (deleted); core's dead code of §4.4; `.github/workflows/**` | entry: W5.2 has no Linux release-blocking gap, W5.3 is done, wave 6 is done. Then: §6.1's flip and the GTK removal in one commit, with core's §4.4 deletions; both workflows lose the GTK apt packages and publish GPUI Linux packages and `nsisgpui`; no Cargo manifest names `gtk4`, `libadwaita`, `webkit6` or `javascriptcore6`, and no Rust file imports their workspace aliases `gtk`, `adw`, `webkit`, `javascriptcore` (a grep for the word `webkit` cannot pass: Chrome's user agent string in `core/src/favicons/fetch.rs` and comments contain it) |
| W7.2 profile data migration | `boot/webkit_import.rs` | Q14 implemented; old WebKit data under `<root>/engine` handled as decided |
| W7.3 docs | `docs/PLAN.md`, `README.md` (the lines inv/packaging-ci §16–19 lists), `docs/design/packaging.md`, an ADR in `docs/adr/`, `docs/design/linux-cef/STATUS.md` | each file describes the GPUI + CEF shell and the Windows flavour; the ADR records the decision |
| W7.4 final review | (none) | the full-diff review of ST "Next" step 4 has no open blocker |

### 8.11 Critical path and the ownership check

Critical path: W0.1 → W0.2 → W1.3 → W1.4 → W1.8 → W2.1 → W2.5 → (wave 3) → W4.1 → W4.5 → W4.4 → W5.1 → W5.2 → W7.1. Three chains run beside it and must arrive in time: WX.0 → WX.1 (from wave 0; before wave 4), lane C (from W0.3; C.0 before W1.8, C.1 before W2.2, C.2 to C.6 before wave 3), and wave 6 (from wave 1; before wave 7). The riskiest item is WX.0, and it now runs first: nothing frozen depends on its outcome until WX.1 (Opus review 12).

The ownership check is a script over this document's tables, not a claim (R2-2; principle: build the lever). `docs/design/linux-cef/tools/check_ownership.py`, run as `python -I docs/design/linux-cef/tools/check_ownership.py .` from the repo root. It belongs to this document, not to a unit; whoever edits a §8 table reruns it. It parses every unit row of §8 (files, with brace expansion and `except` clauses; dependencies, with ranges and whole waves), adds the implicit edges (W0.1 before everything, each main wave after the previous one, wave 7 after wave 6, lane C in order after W0.3), and checks:
1. every dependency names a unit that exists;
2. two units whose file lists overlap are ordered by the dependency closure, so a shared file only passes from an earlier owner to a later one;
3. every owned entry of the §1.2 and §1.3 trees is in its owner's file list;
4. every UA §5.1 path has an owner, unless §1.3's change table removes it.

Its run on this revision: 54 units, 48 tree entries, 137 UA §5.1 paths (10 removed by §1.3), 92 overlapping file pairs, all ordered; OK. A mutation run that gave W2.4 one of W2.2's files reported "unordered units share a file: W2.2 and W2.4". What the script does not check: that prose acceptances name a real command or check. That stays a reading check, below.
- W0.1 owns the stubs and the files nobody fills later (`lib.rs`, `app/actions.rs`, `native_probe/main.rs`, `xtask/src/main.rs`).
- The file handoffs the round-2 review found are explicit: `xtask/src/selftest.rs` (W0.5's WinUI branch, W1.8's GPUI branch), `xtask/src/testenv/linux.rs` (W0.5, then W1.1 for a window manager), `ci.yml` (W0.1, W0.5, W1.8, W6.6, W7.1), `vsesvit-update/src/install.rs` (W6.5), `examples/x11_probe.rs` (replaced by `native_probe`), and wave 6's "lands after" edges for `ime_compose` (W2.5) and `pack-helper` (W4.2).
- Within each wave the parallel units' file lists are disjoint. The pairs the reviews found overlapping are resolved: `save_page.rs` is W2.5's; `toolbar/update_banner.rs` and `dialogs/settings/updates.rs` are W3.7's; settings pages are split among W3.1, W3.5, W3.6 and W3.7; `state/private_session.rs` is W1.5's and `state/memory_saver.rs` W3.8's; CEF handler files belong to the units whose features use them; `platform/{mod,fake,files,shell_open,default_browser}.rs` have owners; `vsesvit-update` is split by file between C.6 (`lifecycle.rs`) and W6.5; `xtask/src/ctx.rs` is W6.5's alone and W6.4 waits for it; WinUI is lane C's alone; the audio indicator no longer needs the helper (Q11).
- Every "Done when" names a command, a test, a check name or a recorded artifact. "Reviewed by" and "works" without a test are gone. No done-when accepts a stub's result (exit 4, zero checks).

## 9. Open questions and risks

Each row has a proposed resolution or the spike that settles it, and the unit that must have it settled. "Owner call" marks a product, legal or preference decision no experiment can make.

| # | Question or risk | Proposed resolution or spike | Settled by |
|---|---|---|---|
| Q1 | **H.264/AAC.** Stock CEF builds leave them out (ST), so many sites' video fails | Ship stock CEF first; VP9, AV1 and Opus cover most sites. Spike: build CEF 154.0.34 with `proprietary_codecs=true ffmpeg_branding=Chrome` for both OSes (`automate-git.py`), host the archives as private release assets, and point `CEF_PATH` at them; the Flatpak source, the CI cache key and the licence text change with it. Shipping the codecs is an owner call (patent licensing) | owner, before W6 |
| Q2 | **Windows download bubble.** "Recent download history" still shows with our handler (SW §10) | First retry `set_preference("download_bubble.partial_view_enabled", false)` with a non-null error buffer: the spike's `set_preference` returned 0 with an empty error, which is G4's symptom. If that fails, look for the feature or pref that turns the bubble off in Chromium 154's source; last, accept it as a known exception | W3.3 |
| Q3 | **Display scales.** Windows ran only at 175 %; `WM_DPICHANGED` and Linux runtime scale changes untested (SW §5, SC §5.4) | Hand test (§7.4) at 100/150/200 % with SW's `explore-embed.ps1`; handle `WM_DPICHANGED` and Xft.dpi changes by calling `notify_screen_info_changed` and re-applying geometry; on Linux a scale change needs a relaunch of Chromium's side unless `notify_screen_info_changed` suffices (spike it in W2) | W2 hand test |
| Q4 | **Wayland-native.** Chrome-style children need an X11 parent; CEF #2804 is open (G12) | Stay on X11/XWayland. The seam allows a `platform/wayland` later, once CEF can parent to a Wayland surface or OSR becomes viable (`cef-osr.md`: CEF 156 with the fence fix #4314, GPUI texture surfaces). Revisit at each CEF bump | each CEF bump |
| Q5 | **Windows extension install route.** The per-user External Extensions folder is verified on Linux only; on Windows Chromium may read only the registry, and `prompt_for_external_extensions` defaults on (CE §3.3) | WX.0 item 1 tests the per-profile routes in order, then the capability matrix and gate of §5.10 decide: A (route passes every fundamental row), B (the Windows browser-target pipe with `Extensions.loadUnpacked` at every start, tested as a whole; page-session `loadUnpacked` is unavailable, EL §5), C (Windows GPUI ships without user extensions, WinUI stays the default, `parity.md` records the blocker), or D (a Linux fundamental fails; owner call). Routes that are not per-profile are rejected (§5.2 step 7) | WX.0, before wave 4 |
| Q6 | **Install while running.** Chromium reads the folder only at start (EL §6.2) | "Restart to finish" with a session-restoring Restart button (§5.2). Alternatives are a CEF patch or session-only `loadUnpacked`. UX is an owner call | owner, W4.3 |
| Q7 | **No back/forward restore.** CEF can serialize nothing back (inv/tab-engine §13) | Restore URL only, as WinUI does. Reopen Closed Tab and moving tabs between windows (when the reparent fails) lose history. Accept | decided |
| Q8 | **Linux password store.** A KWallet that does not answer stalls every request (SC §3.2); core's vault is opened lazily, and probing it before `cef::initialize` would start a thread (§3.1; GPT review 13) | A persisted choice in `<root_cache_path>/password-store`, written at a profile's first GPUI start and never changed automatically, because changing it makes Chromium's encrypted cookies unreadable (the user is signed out of every site). Proposed default: `basic`. Chromium's password manager is off (`credentials_enable_service=false`), so the store protects cookies only; with `basic` that is a fixed key, as in Chromium without a keyring, and no worse than the WebKit shell today. Never KWallet. `gnome-libsecret` as the default would need a bounded, thread-free preflight that does not touch core's vault; owner call | owner, before W1.3 |
| Q9 | **Memory Saver without discard.** CEF has no discard (inv/tab-engine §12.3) | Freeze, not discard: `was_hidden(1)` plus DevTools `Page.setWebLifecycleState({state: "frozen"})` stops CPU and keeps history, but keeps memory. Optional real discard (close the browser, keep the URL) for tabs idle more than a configurable time, which loses history (Q7). The contract carries `set_frozen` either way (§1.5), and discard needs only `close_tab`, so the owner call no longer blocks the W0.2 freeze | owner, before W3.8 |
| Q10 | **Zoom.** No zoom-changed callback; Chromium's per-host zoom map overlaps core's `site_zoom` (inv/tab-engine §18.3) | Core stays the source of truth: apply core's level on every commit; intercept Ctrl+wheel and the zoom keys in the shell (`on_pre_key_event`, and the wheel through a document-start script reporting `ctrlKey` wheels, or by polling `get_zoom_level` 100 ms after a wheel event); spike in W2.5 which of the two catches every path | W2.5 |
| Q11 | **Audio indicator.** No playing-audio callback (inv/tab-engine §11.1) | A document-start script watching media `play`, `pause` and `volumechange`, reported through `on_console_message` as the bridge does; it needs nothing from wave 4. The helper's `tabs.onUpdated` (`audible`) stays a later option. Without either, no speaker icon (mute still works) | W3.8 |
| Q12 | **IME.** Untested in the page on both OSes and in the omnibox on Windows (SC §7.4, SW §14) | Linux: GtkUi is loaded under the external pump, which is what IME needs (G13), and the packages keep GTK 3 for it (§6.4); the `ime_compose` check runs ibus inside the test environment, from the build and from the installed package (W6.1). Windows: hand test | W2.5, W6.1, hand test |
| Q13 | **Chrome toplevels on X11.** The status bubble and the fullscreen-exit bubble are separate Chrome windows (SC §10.3) | Status bubble: `chrome_status_bubble` off in `BrowserSettings` (this overrides UA §4.4, §0), our link preview instead (W2.5). Fullscreen bubble: keep Chromium's and drop our notice if it cannot be turned off (UA S:7) | W2.5 |
| Q14 | **Existing Linux profiles.** `<root>/engine` holds WebKit's data; site logins do not carry over to Chromium | Proposed: import WebKit's cookies once at first GPUI start (its `cookies.sqlite` → `CookieManager::set_cookie`), then leave the WebKit data for one release (downgrade path) and delete it in the next. Without the import, users are signed out of every site once; owner call | owner, W7.2 |
| Q15 | **The renamed bootstrap exe.** Its icon and version resources are CEF's; signing and whether `bootstrap.exe` checks the DLL's signature are unknown (SW §3.2) | `xtask stamp-exe` rewrites the resources (§6.2); a small spike in W6.4 checks that the stamped exe still loads `vsesvit.dll` and runs sandboxed. Authenticode for both files when signing exists (none today, so SmartScreen warns on a 400 MB unsigned installer: an owner call) | W6.4 |
| Q16 | **Updater at CEF size.** Verification reads the whole file into memory, twice (`updater.rs:446`); `rpm -U` resolves no new dependencies on the GTK → GPUI update | Stream the hash for verification if the signature scheme is prehashed, else verify once and keep the bytes; for rpm use `pkexec dnf install -y <file>` (or `zypper`) where present, falling back to `rpm -U` | W6.5 |
| Q17 | **Accessibility.** Two trees per window: GPUI's AccessKit and Chromium's own (AT-SPI on Linux, UIA on Windows) (`cef-windowing.md` risks; GP §2.9) | Check with Orca and Narrator in W5.3 that both are reachable and that focus moves announce; mirroring Chromium's tree into AccessKit is not planned | W5.3 |
| Q18 | **Chromium opens windows itself** (`tabs.create`, `windows.create`) | Neutralize synchronously, wait for the first committed URL, then close and reopen as a tab in the same browsing context, on both OSes (§5.7). Embedding with Chrome's UI cropped is not built in these waves. WX.0 checks no flash, no focus steal, a late URL, an explicit `about:blank` and an incognito opener. DevTools windows are ours and stay native | WX.0 |
| Q19 | **Extension action limits.** No badge or dynamic icon, no `action.onClicked`, no `activeTab` from the button (EA §4, CE §1.3); Chromium's handling of a permission-raising update is unverified | Accept and document (EA §7.4); the only fix is a CEF patch exposing the action model, which is out of scope. WX.0 checks what re-enabling after a permission-raising update shows | WX.0 |
| Q20 | **`storage.sync`** no longer syncs between devices for GTK users who had it | Local only, as on WinUI (§5.6). A bridge through each extension's page session can come later; owner call on priority | owner |
| Q21 | **Web Store installs.** Chromium's own `webstorePrivate` flow would bypass core (CE §3.5) | Port WinUI's store shim with its safeguards (§5.7); WX.0 verifies the shim runs before the store's own scripts in a Chrome-style child | WX.0 |
| Q22 | **WinUI's self-test on a private desktop** is unknown; units that migrate WinUI code need it | W0.5 tries it through its own `xtask selftest --flavour winui` (§7.3). If XAML refuses a non-input desktop, the launcher's second route runs the same command inside Windows Sandbox: `xtask` writes a `.wsb` file that maps the built binary and `OUT_DIR`, and `require()` accepts that run only when it finds the launcher's token file in the mapped folder and runs as the sandbox's fixed account `WDAGUtilityAccount`. These are properties of the machine, not variables a caller sets. Turning the Windows Sandbox feature on is the owner's action. If neither route works, the gate stays open and W0.3 and lane C wait. Scripted runs on the user's desktop are never the fallback (R2-1) | W0.5, before W0.3 lands |
| Q23 | **Windows idle CPU.** 3.8–5.1 % of one core in debug builds, most from window activation (SW §4); release untested | Measure release builds in W1.8; if the active-window cost stays above 2 %, profile GPUI's frame scheduling with the window active and the page idle | W1.8 |
| Q24 | **GPUI churn.** gpui-pre breaks weekly (GP §1.2); our two patches must be re-applied on every bump | Bump deliberately, not continuously; `vendor-gpui --bump` makes the re-apply mechanical and records the provenance (§2.2); push both upstream PRs in wave 1 | W1.1, W1.2 |
| Q25 | **Drag and drop between the page and GPUI.** GPUI drags are window-local and file-only (GP §2.10); CEF's `StartDragging` has no outlet | Chromium handles drags within the page natively; links dragged from the page onto the tab strip need a spike in W2.3; until then unsupported | W2.3 |
| Q26 | **Sites that refuse embedded browsers** (Google sign-in) (P) | Keep Chrome's own user agent (no "CEF" token); if a site still refuses, a per-site CDP `Network.setUserAgentOverride` as prior art does. Test with the sync sign-in flow in W3.6 | W3.6 |
| Q27 | **Origin: null on extension service-worker fetches** (CEF #4309) breaks some password managers (CE §8) | Not fixable from handlers; track the upstream issue; list it in the release notes | upstream |
| Q28 | **The key patch cannot be carried** (it no longer applies, or upstream refuses it) | Fall back to `multi_threaded_message_loop` on Windows (SW §4): `post_task` for every CEF call, the 200 ms wait before `cef::shutdown`. Linux is unaffected (GtkUi needs the external pump) | if it happens |
| Q29 | **AppImage where user namespaces are blocked** (Ubuntu 24.04+ by default). A desktop-launched AppImage has no visible stderr, so a silent `--no-sandbox` would leave the user unsandboxed without knowing (Opus review 28) | Proposed: start unsandboxed with a persistent warning bar in every window ("This browser is running without its sandbox") and a link to the fix (the deb or rpm, or the AppArmor profile). The alternative is to refuse to start with the same explanation. Never silent. Owner call | owner, W6.2 |
| Q30 | **Chromium's own models behind extension APIs.** `chrome.bookmarks`, `history`, `downloads`, `sessions`, `topSites` and `tabGroups` answer from Chromium's models, which are empty or unrelated to core's; `chrome.downloads.download` from a service worker has no browser and CEF cancels it (inv/bookmarks-downloads, lines 952-966; Opus review appendix A.4) | Accept for these waves; document the user impact in `parity.md` and the release notes; a bridge from core is a later plan | W5.2 |

## 10. Review dispositions

### 10.1 Round 1 dispositions

Round 1: `.audit/scratch/design/review-gpt.md` (GPT-6 Astra, 22 findings) and `.audit/scratch/design/review-opus.md` (28 findings, a Cargo probe, a section on the GPT review, and an inventory appendix A.1–A.25). Orchestrator decisions 1 to 8 are applied as given. The rows below are the round-1 record; where round 2 changed a fix (for example GPT 16's "nothing deleted", now withdrawn), §10.2 says so. "Merged" means the finding shares its root cause with an earlier row, and the fix there covers it; any extra the merged finding raised is fixed where its row says.

#### 10.1.1 GPT review

| # | Finding (short) | Disposition |
|---|---|---|
| GPT 1 | self-test override; guard trusts a boolean, not the display | fixed in §7.3 (no override; launcher-created identity checked before `cef::initialize` and any window, fails closed, covers examples), §3.1 step 2a, §1.1 `vsesvit-testenv`, §7.4 (hand tests as a separate mode) |
| GPT 2 | `cef::shutdown` can run with live browsers | fixed in §3.6 (live count over every browser kind including in-flight creations, Chrome-created and DevTools; Drained only at zero; `PumpHandle` returns the driver; delayed-close tests). GPT's "never on elapsed time" is kept for Drained, but after the cap the process goes to Stuck and exits 70 without `cef::shutdown`, as the Opus review proposed (orchestrator decision 3): waiting forever leaves a windowless process that blocks the update restart |
| GPT 3 | `chrome://extensions` cannot load in Alloy | fixed in §3.7 (Chrome-style, short-lived management browser), §5.1, §5.4 |
| GPT 4 | no request/answer protocol for menus, JS dialogs, beforeunload, file chooser, auth | fixed in §1.5 (owned `MenuModel`, typed `PromptRequest`/`PromptAnswer`, ids, cancellation, exactly-once, IO-thread auth), W0.2 and W2.6 |
| GPT 5 | `MainThread<T>` unsound; `request (UI)` misclassified | fixed in §1.6 (main-thread registry keyed by ids; handlers hold only `Send + Sync` values; no `unsafe impl`) and §1.2 (thread affinity per method; `get_resource_request_handler`, `get_auth_credentials` on IO) |
| GPT 6 | core's collector deletes the kept CRX | fixed in §5.2 step 2 (archive store outside `extensions/<id>/`, named from the existing record, collector's second pass), W4.1 tests |
| GPT 7 | Chromium updates leave core's manifest stale; `management.onInstalled` has no reason | fixed in §5.1 (package record vs runtime record), §5.3 (reason derived from the snapshot), §5.5 (UI from the runtime manifest; core never written; restart diff), W4.4 tests |
| GPT 8 | W4.0 late, no gate, no real fallback | fixed in §5.10 (WX.0 in wave 0, per-profile requirement, two profiles, sandboxed bundle, outcomes A/B/C), §1.5 (extension contract frozen only by WX.1), §8.2, Q5 |
| GPT 9 | `Surface::focus` does not stop Chromium's own focus | fixed in §1.5 (page-focus policy, `set_page_focus`, enforced in `on_set_focus`, one focus path), §1.4 table, §7.2 `page_focus_policy` |
| GPT 10 | clear-on-exit has no live DevTools target | fixed in §3.7 (maintenance browser; start-up clear before the first load; localStorage and IndexedDB tested), §3.6 step 4, §3.1 step 10 |
| GPT 11 | Chrome-created window adoption needs a synchronous step | fixed in §5.7 (`ShellHooks::neutralize_foreign` → `Desktop::neutralize`, then `ForeignWindowOpened`). The async adoption descriptor for embedding is not built: the default is close-and-reopen, as the Opus review argued (see §10.1.3) |
| GPT 12 | packages drop GTK 3, which GtkUi/IME needs | fixed in §6.4 (GTK 3 and IM bridges declared on every format), §6.3 (CI), Q12, W6.1 (`ime_compose` from the installed artifact) |
| GPT 13 | password-store choice depends on a probe forbidden before init | fixed in §3.1, §3.2, Q8 (persisted choice, no vault probe, readability on change stated; thread count enforced by a self-test check) |
| GPT 14 | Windows parity features missing (split view, media player, PiP) | fixed in §4.2.1 (OS scope matrix; Windows flip blockers; Linux-only negative tests scoped by `cfg`) |
| GPT 15 | extension notifications and optional-permission UI assumed | fixed in §5.7, §5.10 item 9, §4.4 (core's request pieces kept until verified; decision before W4.3) |
| GPT 16 | no CRX source for existing profiles | fixed in §5.9 and W4.5 (re-fetch store CRXs, reselect local ones, unsupported XPI/MV2 kept, storage import, nothing deleted, fixture profiles from both shells) |
| GPT 17 | `vendor-gpui --check` has no checksum in a patched lock | fixed in §2.2 (`third_party/vendor.toml` provenance, `--bump`, `.gitattributes`) |
| GPT 18 | Linux CI cannot build GTK members without their packages | fixed in §6.3 (GTK packages kept while GTK is a member, removed with it in W7.1) and §6.1 |
| GPT 19 | parallel units overlap and depend on later work | fixed in §8 (ownership map, dispatch-to-stub rule, lane C, `save_page.rs`, banner and settings split, W6.5 before W6.4, Q11 without the helper), §8.11 check |
| GPT 20 | some definitions of done impossible or permissive | fixed in §7.5 (phase-specific gates; resource baseline with warm-up and tolerance), §8.2 (W0.5 before W0.3/W0.4; `selftest` a passing stub until W1.8), W4.3 (U16 amended), W5.2 (release-blocking vs accepted; an issue alone never accepts a gap) |
| GPT 21 | `--features gpui` on `vsesvit-sandbox` | fixed in §1.1 (its manifest, no features) and §6.2 |
| GPT 22 | no dev recipe for the Linux helper | fixed in §1.1 "Finding the Linux helper" (debug same-exe fallback, release refuses before any window, xtask builds both) |

#### 10.1.2 Opus review

| # | Finding (short) | Disposition |
|---|---|---|
| Opus 1 | W0.1 makes Linux the skeleton GPUI shell while nightlies ship; CI drops GTK deps | merged with GPT 18; its release half fixed in §0 and §6.1 (`gtk`/`winui`/`gpui` features, defaults unchanged until wave 7), §6.3 (nothing GPUI published before W7.1), §6.5 (orchestrator decision 4) |
| Opus 2 | one-owner rule unexecutable: handlers, hook sites, orphans, double owners, WinUI overlap | merged with GPT 19; its specifics fixed in §1.2 (handler files per feature unit), §1.3 (platform files owned), §8.1 (one owner per file per wave; dispatch to stubs), §4.3 and §8.4 (lane C owns WinUI) |
| Opus 3 | guard self-declared, override enables XTEST, examples unguarded | merged with GPT 1; its additions fixed in §7.3 (every example and window-opening test binary calls `require()`, enforced by `check-deps`; `PrivateInput` only from a `TestEnv`; watchdog for GPUI-internal activation) |
| Opus 4 | headless Weston + Xwayland + XTEST never ran | fixed in §7.3: Xvfb (orchestrator decision 2), with W1.1 deciding whether a window manager is needed |
| Opus 5 | callbacks also run inside engine calls and Windows dispatch | fixed in §1.6 (re-entrancy rules, tests), §1.5 (hooks never borrow core), §3.4 rule, §0 supersede table (orchestrator decision 6) |
| Opus 6 | `MainThread<T>` dropped on an IO thread | merged with GPT 5 (the registry removes the cell) |
| Opus 7 | Windows has no working extension fallback | merged with GPT 8; its missing items (Chromium-branded external CRX acceptance, helper load, `developerPrivate` over a page session) fixed in §5.10 items 1, 3, 5; its second route is outcome B |
| Opus 8 | ESB pref set too late; silently removes a user feature | fixed in §5.3 (helper installed as an External CRX; ESB untouched), §3.3, §5.10 item 3 |
| Opus 9 | existing extensions have no CRX; storage and XPI/MV2 dropped | merged with GPT 16 (§5.9 includes `ext_storage_local` import and the XPI/MV2 state) |
| Opus 10 | `permissions.request` deleted from core unverified | merged with GPT 15 |
| Opus 11 | clear-on-exit after every browser closed; no start-up clear | merged with GPT 10 |
| Opus 12 | W4.0 and Q9 shape the contract frozen in W0.2; UA §4.4 contradictions | fixed in §1.5 (extension part frozen by WX.1 after WX.0), §8.2 (WX.0 in wave 0), Q9 (`set_frozen` in the contract regardless of the owner call), §0 (UA paragraphs superseded: status bubble, Memory Saver, `NativeParent`, `sync_extensions`, hooks) |
| Opus 13 | `--use-fake-ui-for-media-stream` defeats the permission checks | fixed in §3.3 (dropped; `--auto-select-desktop-capture-source` added) |
| Opus 14 | `vendor-gpui` has no checksum | merged with GPT 17 |
| Opus 15 | libcef-linked binaries outside Cargo; helper not built | merged with GPT 22; the library path fixed in §1.1 (`xtask` sets `LD_LIBRARY_PATH`) and §7.1 (pure tests create no browser) |
| Opus 16 | hidden browsers tied to the last-used window | fixed in §1.3 (process-wide hidden host owned by `Desktop`) and §3.7 |
| Opus 17 | `-p vsesvit-sandbox --features gpui` rejected | merged with GPT 21 (confirmed by the probe) |
| Opus 18 | Windows parity gaps (media, PiP, split view, native messaging, default browser, startup error, file filters) | merged with GPT 14; native messaging fixed in §5.8, the default-browser row, `fatal_message`, file filters and the window frame in §4.2.1 and §1.3 |
| Opus 19 | Flatpak `fallback-x11`, helper not built, spikes copied, wrong cite | fixed in §6.4 (`--socket=x11`, helper built), §2.2 (spikes skipped, cite corrected, generator test), W6.3 |
| Opus 20 | downloads outlive their browser; private context dropped too late | fixed in §4.2 downloads row (keep-alive in the hidden host; private downloads cancelled first), W1.5, W3.3 |
| Opus 21 | one check registry cannot serve both flavours; `passwords_purged`; `--phase` | fixed in §4.3 row 14 (shared list plus per-flavour extras), row 1 (`--phase`), §7.2 (`passwords_purged` dropped). `shortcuts_sync` stays adopted: it exists as a WinUI check (`winui/selftest/shortcut_checks.rs`), which is enough to port it |
| Opus 22 | shutdown details contradict the inventory | fixed in §3.6 (Ctrl+Q only; steps 2 and 4 skipped for a removed profile; AppImage relaunches `$APPIMAGE`; Windows Restart uses `install(…, ["--profile-dir", root])`) |
| Opus 23 | picker has no branch in the boot order | fixed in §3.1 step 4 and W1.4 |
| Opus 24 | `fake-platform` swaps the real surface under `--all-features` | fixed in §1.4 (aliases by OS only; `Box<dyn SurfaceOps>`; `cfg(test)` plus additive `test-support`; `check-deps` rule; CI runs the foundation checks on the `--all-features` binary) (orchestrator decision 8) |
| Opus 25 | helper links core through `vsesvit-cef` | fixed in §1.1 (`vsesvit-cef-child`, `cef` only; `check-deps` rule; `readelf -d` check through `xtask helper-check`) |
| Opus 26 | `cef::args::Args` panics on non-UTF-8 argv | fixed in §1.1 (`OwnedArgs` from `args_os`) |
| Opus 27 | Alloy invisibility to `tabs` APIs not shown by EA | fixed in §3.7 (claim corrected; management browsers short-lived) and §5.10 item 5 |
| Opus 28 | AppImage silently unsandboxed | fixed in §3.5, §6.4 and Q29 (owner call; never silent) |

#### 10.1.3 Where the reviews disagree

- **Shutdown after the cap** (GPT 2 vs Opus): the Opus variant, by orchestrator decision 3. Drained is reached only by a zero count; a timeout leads to Stuck: no `cef::shutdown`, core flushed, exit 70.
- **Chrome-created windows** (GPT 11 vs Opus): both. GPT is right that a synchronous native step is needed, because Chromium shows and focuses the window before any event arrives, and that holds for close-and-reopen too. Opus is right about the default: embedding with Chrome's UI cropped was seen once on X11, depends on scale and layout, and leaves a live tab strip and omnibox. So the hook neutralizes, and the shell closes and reopens.
- **The release half of the GTK problem** (GPT 18 vs Opus 1): Opus, by orchestrator decision 4. CI alone was not the problem; a nightly built from `main` would have shipped the skeleton.
- **The mixed optional dependency** (GPT's closing note vs Opus's probe): the probe settles it, so §6.1 drops the draft's renaming fallback and keeps one confirmation step in W0.1 with the real crates.
- **The self-test guard** (GPT 1 vs Opus 3): both are adopted; Opus's examples and XTEST points are the larger hole.

#### 10.1.4 Opus appendix A (inventory sweep)

| # | Item (short) | Disposition |
|---|---|---|
| A.1 | extension load errors and "Not running" | fixed in §5.4 (`Details`, `LoadFailed`) and §5.7 (buttons gated on `List`) |
| A.2 | mid-session extension sync | fixed in §5.9 |
| A.3 | extensions in private windows; `chrome.cookies.set` vs Block | fixed in §5.10 item 10 |
| A.4 | Chromium-native `chrome.bookmarks` etc. | fixed as Q30 |
| A.5 | popup links and `window.open` | fixed in §5.7 |
| A.6 | extension shortcut clashes | fixed in §5.7 |
| A.7 | "Load Unpacked" and ".xpi" buttons; `--load-extension` meaning | fixed in §5.2 step 5 |
| A.8 | welcome "Add extensions" shows Installed at once | fixed in §5.9 (restart wording) and Q6 |
| A.9 | per-extension notifications switch | merged with GPT 15 |
| A.10 | `ExtensionUpdater` races `extension_update` | fixed in §3.3 (self-test switch, checked in W4.4) |
| A.11 | checks comparing shell tab ids with extension ids | fixed in §7.2 |
| A.12 | store shim safeguards | fixed in §5.7 |
| A.13 | engine schemes typed in the omnibox | fixed in §4.2 and §4.3 row 7 |
| A.14 | permission types without a policy; screen-sharing stop | fixed in §4.2 permissions row and W3.1 |
| A.15 | certificate details, crashed page, auth, beforeunload | fixed: crashed page in §4.2 and W2.5; auth and beforeunload in §1.5, W2.6 and §7.2. Certificate details need no change: UA §4.2's `SslInfo` already carries the chain and status for `connection_info` (W3.1) |
| A.16 | `view-source:` as seen by `on_before_browse`/`on_address_change` | dismissed as a design item: it is an observable behaviour, not a decision, and W2.5 settles it while building view source, under its `page_commands` check; inv/tab-engine §28 item 8 already lists it for the implementer |
| A.17 | favicons from `/favicon.ico` only | fixed in W3.2's done-when |
| A.18 | bookmark drag zone/destination moving to core | fixed in §4.3 row 10 (stays in the shell) |
| A.19 | Windows window frame | fixed in §1.3 (`platform/windows/frame.rs`), §4.2.1, W1.2 |
| A.20 | forwarded-launch activation under Wayland | fixed in §7.4 (hand test) |
| A.21 | audio indicator depends on the helper | fixed in Q11 (document-start script) |
| A.22 | Linux Downloads folder has no owner | fixed in §1.3 and §4.3 row 15 (`core::os`, xdg-user-dirs, C.3) |
| A.23 | packaging metadata, `.gitattributes`, release job split, `scripts/wsl.sh` | fixed in §6.4 "Metadata", §2.2, §6.3. `scripts/wsl.sh` dismissed: a developer convenience on no shipped or tested path; it gains `CEF_PATH` when someone first uses it with CEF |
| A.24 | §4.4 deletion list details; W7.1's grep | fixed in §4.4 and W7.1 |
| A.25 | cite errors | fixed in §4.3 rows 3 and 12, §6.5 (`installation.rs:120`, the manifest test) |

**Counts.** GPT: 22 fixed, 0 dismissed, 0 merged. Opus: 16 fixed, 0 dismissed, 12 merged (1, 2, 3, 6, 7, 9, 10, 11, 14, 15, 17, 18; each merged row's extras are fixed in place). Partial dismissals inside fixed rows: GPT 2 (no transition on elapsed time at all), GPT 11 (the async embedding descriptor), Opus 21 (dropping `shortcuts_sync`). Appendix A: 23 fixed (A.23 with `scripts/wsl.sh` dismissed), 1 merged (A.9), 1 dismissed (A.16).

### 10.2 Round 2 dispositions

Round 2: `.audit/scratch/design/review-gpt-r2.md` (GPT-6 Astra, 2026-10-24, at `0677237`): an audit of every §10.1 row, fourteen new findings R2-1 to R2-14 (one blocker), and a wave-0 verdict with five prerequisites. Rows the audit marked resolved are not repeated, except those it resolved with a note (§10.2.3). The decisions the review accepted are not reopened: the 15 s Stuck cap, the Windows opt-in scope, close-and-reopen's loss of the returned tab id, `shortcuts_sync`, and the A.16 and `scripts/wsl.sh` dismissals.

#### 10.2.1 New findings

| # | Finding (short) | Disposition |
|---|---|---|
| R2-1 | blocker: test-policy exceptions (WX.0's harness, Q22's user-desktop fallback, the guard before CEF's sub-process start) | fixed in §7.3 (`require()` guards browser-process entries only, after the sub-process return; the `smoke` prologue and its `syn` check; WinUI's `--self-test` and `--ui-smoke` entries guarded; the old spike scripts hardened by W0.5), Q22 (the Windows Sandbox route, identified by the token file and the `WDAGUtilityAccount` account, or the gate stays open; never a scripted run on the user's desktop), §5.10 "Harness" (WX.0 hardens its own copies, since it started before W0.5), W0.5, §6.2. The review's option of a CI runner's own desktop was not taken: a guard mode that trusts "this is CI" is an override by another name |
| R2-2 | §8's ownership and dependency proof incomplete | fixed in W0.5 (a real WinUI branch of `xtask selftest` on the private desktop; report validated against the expected 47 names; zero checks fail; `raw_window` through `xtask probe`), W0.1 (no stub step in CI; unbuilt commands exit 4), §6.2 (`probe` takes `vsesvit-testenv`; `check-deps` scoped to member crates with a dev-dependency allow list), §6.3, §8.1 (one owner at a time; handoffs only along dependencies), W1.1 (`testenv/linux.rs`), W1.8 (`ci.yml`), W6.5 (`install.rs`), §1.3 (`x11_probe.rs` replaced), §8.9 ("lands after" for `ime_compose` and `pack-helper`), §8.11 (the check is now a script; its run is recorded) |
| R2-3 | auth loses the challenger; stale admission; menu dismissal; WrongKind | fixed in §1.5 (`AuthChallenger` apart from `requester`; scope by browser and document generation; admission refuses closed, navigated and shutting-down scopes and runs the cancel action; `WrongKind` leaves the entry; `MenuDismissed`; the per-kind answer and cancel table; reordered-delivery, dismissal, duplicate and wrong-kind tests), §7.2 (`http_auth` through a proxy), W0.2, W2.6 |
| R2-4 | staged shutdown not expressible; restart confused with installing an update | fixed in §1.5 (`begin_shutdown` → `UserBrowsersClosed`, `close_internal` → `AllBrowsersClosed`), §3.6 (ClosingUser and ClosingInternal; late creations closed at once and counted; late requests refused; `ExitIntent` with `Relaunch` needing no update artifact, beside the two update intents; `shutdown_late_creation`), §5.2 step 4, §0, W0.2, W1.4, W4.4. The 15 s cap is unchanged |
| R2-5 | foreign close-and-reopen loses URL readiness and context | fixed in §5.4 (`ForeignContext`, `ForeignWindowReady`, `ForeignWindowRefused`), §5.7 (neutralize failure closes the browser; the reopen waits for the first committed URL, with `about:blank` and timeout rules from `spikes/ext-api/src/shell.rs`; reopened in the same context; an unknown context is never reopened; the lost opener and window type recorded), §1.4 (`neutralize` returns a result), §1.5, §5.10 item 7, Q18 |
| R2-6 | download parking has no platform handoff or separate tab lifetime | fixed in §4.2 "Download keep-alive" (`park_decision`, then `Surface::park_page`, then `detach_tab` and `TabEvent::Detached`; the old host lives until the reparent returns; a page-initiated close is measured in W3.3 and shown as interrupted if CEF stops reporting), §1.4, §1.5, §8.1 (dispatch stub), W1.1, W1.2, W1.5, W1.7, W3.3 |
| R2-7 | migration needs network and UI before either exists | fixed in §3.1 (step 7 is thread-free and metadata only; new step 10a), §5.9 (restore jobs after the first window; offline and error states retried at the next start; activation by `Relaunch`), W1.4, W4.5 |
| R2-8 | desired disabled and withheld state not enforced before first execution | fixed in §5.5 (publish rule for ids not yet in Chromium; ids in Chromium keep their JSON and follow runtime state; approval only by the user's answer to core's prompt; pre-start `Preferences` state, checked in W4.2; start-up reconciliation; the private context waits for it), §3.1 step 7a, §5.2 step 2 (the withheld version's archive kept), W4.1, W4.2, W4.5 (sentinel extension) |
| R2-9 | "nothing deleted" and once-only storage import not guaranteed | fixed in §5.9 "What is preserved" (every record keeps a valid directory; rollback to the pre-migration version is not offered; the round-1 promise withdrawn) and "Storage" (both GTK areas seeded into Chromium's stores before the extension's first run; temp-and-rename as the marker; never overwrites; a page-session fallback whose limit needs the owner's acceptance; WinUI profiles start empty), §5.6, §4.4, §2.1, W4.5 |
| R2-10 | WX.0's outcome gate does not cover the lifecycle | fixed in §5.10 (a per-OS capability matrix with fundamental, mechanism and omission classes; A and B need item 2's lifecycle; B tested as a whole, the helper under ESB included; C; a new D for a Linux failure; items 7 and 9 extended), §5.3, W4.2, Q5, the WX.0 row. These match the additions already given to the running WX.0 agent |
| R2-11 | the page-focus exception treats every system focus event as a click | fixed in §1.5 (`PageFocus` with the three facts and a pure `allows()`; no `FOCUS_SOURCE_SYSTEM` exception; a click hands the page the keyboard through `Desktop::pointer_presses`; focus that bypasses `on_set_focus` is taken back; the programmatic-focus case), W1.2 (a posted press reaches the hook), W1.6, §7.2 |
| R2-12 | the shared CLI and the unpacked UI contradict their backend | fixed in §4.3 row 1 (per-shell flag table; GPUI accepts `--load-extension`), §3.3, §5.2 step 5 (no hot load exists; "Load unpacked…" relaunches with the folder until quit; disabled with ESB on; never written into core), W0.3, W4.4 |
| R2-13 | coordinate type, engine dependency, folder picker, owner cite | fixed in §1.5 (`PagePoint` in the engine), §1.4 (the screen `PhysPoint` stays in the platform), §1.1 and §2.1 (`serde_json`), §1.3 (`OpenFolder` through GPUI's `prompt_for_paths`), §4.2.1 (owner W2.6), W2.6 |
| R2-14 | URL matching is not tab identity | fixed in §7.2 (a token per created tab mapped with the probe extension's tab id and `incognito` flag; no match or two matches fail; a duplicate-URL case in each affected check), W4.4 |

#### 10.2.2 Rows the audit did not mark resolved

| Row | Audit | Disposition |
|---|---|---|
| GPT 1 | partially resolved | fixed with R2-1 |
| GPT 2 | partially resolved | fixed with R2-4 |
| GPT 4 | partially resolved | fixed with R2-3 |
| GPT 8 | partially resolved | fixed with R2-10 |
| GPT 9 | partially resolved | fixed with R2-11 |
| GPT 11 | partially resolved | fixed with R2-5 |
| GPT 15 | partially resolved | fixed in §5.10 item 9 (appear, click, close, block; `permissions.request`) and W4.3 (buttons, `onPermissionLevelChanged`, routing to Settings: each tested on the outcome's backend or named unsupported in `parity.md`) |
| GPT 16 | partially resolved | fixed with R2-7 and R2-9 |
| GPT 19 | partially resolved | fixed with R2-2 |
| GPT 20 | partially resolved | fixed with R2-2 (no zero-check stub satisfies a gate; "lands after" edges) |
| Opus 2 | partially resolved | fixed with R2-2 |
| Opus 3 | partially resolved | fixed with R2-1 |
| Opus 7 | partially resolved | fixed with R2-10 |
| Opus 9 | partially resolved | fixed with R2-7 and R2-9 |
| Opus 20 | partially resolved | fixed with R2-6 |
| Contested: Chrome-created windows | partially resolved | fixed with R2-5; the close-and-reopen ruling and its identity loss stand |
| Contested: self-test guard | partially resolved | fixed with R2-1 |
| A.5 | partially resolved | fixed in §5.7 (the popup's own navigations: same-extension URLs stay, every other URL opens as a tab) and W4.4's `popup` check |
| A.6 | partially resolved | fixed in §5.7 (existing bindings reconciled at each start and on every keymap change; a conflicting extension binding is cleared and reported) and W4.4 |
| A.7 | partially resolved | fixed with R2-12 |
| A.9 | partially resolved | fixed as GPT 15 |
| A.11 | partially resolved | fixed with R2-14 |
| A.12 | partially resolved | fixed in §1.5 "Page messages" (CEF process messages, origin from the browser-side frame, allow list checked on both sides), §5.7, W1.3, W4.3 |
| A.18 | partially resolved | fixed in W3.2 (a two-window drop test through `FakeDesktop::toplevel_at`) and §7.4 (the real-pointer hand test) |
| A.22 | partially resolved | fixed in §4.3 row 15 (`user.xdg.origin.url` and `user.xdg.referrer.url` written for normal-browsing downloads only, in `core::os`, C.3) and W3.3 |

#### 10.2.3 Notes on resolved rows

The audit marked these resolved and attached a remaining point; each is handled where shown. GPT 6, A.24 → R2-9. GPT 7, A.2 → R2-8. GPT 10, Opus 22, A.8 → R2-4. A.15 → R2-3. Opus 4 → R2-2 (`testenv/linux.rs` to W1.1). Opus 8, Opus 12 → R2-10. Opus 16 → R2-6. Opus 18 → R2-13. A.10 → W4.4 now shows no competing Chromium update check ran, not only that the switch was passed. A.16 → W2.5's `page_commands` records both the URL `on_before_browse` saw and the URL the omnibox shows.

**Wave-0 prerequisites** (the verdict's five): test-policy exceptions closed (§7.3, Q22, §5.10 "Harness"); wave-0 gates executable (W0.5, W0.1, §6.2, §8.1, §8.11); the API W0.2 freezes completed (§1.5, §3.6); W0.3's CLI input decided (§4.3 row 1, §5.2 step 5); WX.0 a complete decision experiment (§5.10).

**Counts.** New findings: 14 fixed, 0 dismissed. Rows not marked resolved: 25, all fixed (19 through an R2 finding, 6 in their own sections: GPT 15, A.5, A.6, A.12, A.18, A.22; A.9 with GPT 15). Notes on resolved rows: 16, all handled. No dismissals in this round; one suggested route was not taken, with its reason in R2-1's row.
