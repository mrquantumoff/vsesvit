# vsesvit-gpui: implementation design

Resume notes: all sections written 2026-10-23. Next: consistency pass, then review (ST "Next" step 2).

## 0. Scope and sources

Date: 2026-10-23. Status: design, nothing built. Branch `linux-cef-gpui`.

This document is the implementation design for `crates/vsesvit-gpui`, the GPUI + CEF browser shell.
- On Linux it replaces `crates/vsesvit-gtk` and `crates/vsesvit-webext`. The whole app runs on X11 (XWayland under Wayland).
- On Windows it is a build-time alternative to `crates/vsesvit-winui`, chosen by the `gpui` cargo feature on `crates/vsesvit` (§6). WinUI stays the default until the GPUI shell reaches parity.

It builds on `ui-architecture.md` and does not repeat it. That document stays the reference for the view tree, entities, the feature map (its §2), theme (§3), the engine contract (§4) and the frozen interfaces (§5.2). This document settles what the spikes changed, adds Windows, and replaces its work breakdown (§5.3, §5.4) with the waves in §8.

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

**What the spikes changed in UA.** Every row is a decision this document takes; the rest of UA stands.

| UA said | Now | Why |
|---|---|---|
| Overlays default to GPUI `PopUp` windows; XShape cut-out is the fallback (UA §1.7) | Overlays are in-window GPUI elements plus a hole in the content host. PopUp windows only for surfaces that must leave the window | SC §9 recommends SHAPE for in-window surfaces. SW §9 shows in-window overlays paint over the page for free on Windows and need only an input hole. One mechanism serves both OSes |
| Activation from `_NET_ACTIVE_WINDOW`; GPUI's `is_window_active()` is wrong (UA §1.6) | GPUI's own activation, on both OSes | The X11 focus patch keeps the window active when focus enters the page (SC §7.2). Windows GPUI never loses activation to a child (SW §7) |
| Focus patch "only if the spikes demand it" (UA §1.10) | Carried: `gpui-x11-focus.patch` on Linux, `gpui-windows-keys.patch` on Windows | SC §7, SW §4 |
| `vsesvit-cef` owns an x11rb connection and does geometry, map/unmap and shape (UA §4.4) | The shell's `platform` module owns every native window call. The CEF glue only creates browsers into a parent handle and calls CEF | One seam per OS instead of two; SW §13 |
| Chromium's native page context menu, shell items added (UA §0.8) | GPUI-drawn menu built from `CefMenuModel` in `run_context_menu` | Verified path that keeps extension items and their `activeTab` grant (EA §5); the native menu in a toolbar-less child is unverified |
| Extensions loaded with `sync_extensions` (UA §4.2) | External Extensions install, helper extension, `uninstallSelf`, shell-hosted Alloy popups (§5) | EL §9, EA §7 |
| `NativeParent::X11(u32)` | `NativeHandle`, an XID or an HWND | Windows |

## 1. Crate and module layout, platform seam

### 1.1 Crates

"One crate for both platforms" (ST) means one shell crate, not one crate per OS. The CEF glue and the engine contract stay separate crates, as UA §1.1 and SW §13.1 both propose. The crate boundaries enforce two rules that would otherwise be review rules: the CEF glue never names a GPUI type (G7: `wrap_*` macros expand to unqualified names such as `App` and `Window`), and the shell never names a CEF type, so its tests run without libcef.

```
crates/vsesvit (bin "vsesvit")      wires engine + shell; picks the shell by OS and, on Windows, by feature (§6)
 ├─► vsesvit-cef                    CEF glue; implements the contract. Deps: cef, vsesvit-engine, vsesvit-core (types only)
 └─► vsesvit-gpui                   the shell. Deps: vsesvit-engine, vsesvit-core, vsesvit-sync, vsesvit-update, gpui, gpui-component
        └─► vsesvit-engine          contract: types, Engine trait, events, hooks, FakeEngine. Deps: vsesvit-core, url, futures-channel
crates/vsesvit-helper (bin "vsesvit-helper")   Linux CEF sub-process image: api_hash + execute_process(ChildApp). Deps (Linux only): vsesvit-cef
crates/vsesvit-sandbox (cdylib "vsesvit_app")  Windows sandboxed entry: exports RunWinMain. Deps (Windows only): vsesvit-cef, vsesvit-gpui
```

| Crate | Exists today | Compiles on | Notes |
|---|---|---|---|
| `vsesvit-engine` | no | both | UA §4.2 with the changes in §1.5. `xtask check-deps` rejects a `gpui` or `cef` dependency (UA U00 asked for a CI grep) |
| `vsesvit-cef` | no | both | SC §13.1 `src/cef/`, SW §13.1 `vsesvit-cef`. Never `use gpui` |
| `vsesvit-gpui` | no | both | UA §5.1 tree with the `platform` changes of §1.3. Never names `cef` |
| `vsesvit-helper` | no | both; real `main` on Linux only, `fn main() {}` elsewhere | SC §3.1: the helper keeps GPUI, X11 and Vulkan code out of renderers (2.5 MB `.text` against 45 MB). Windows needs none (below) |
| `vsesvit-sandbox` | no | both; empty off Windows | SW §3.2: Windows' sandbox runs only through CEF's `bootstrap.exe`, which loads `<exe>.dll` and calls `RunWinMain`. The lib is named `vsesvit_app`, not `vsesvit`, so its PDB does not collide with the `vsesvit` bin's (cargo #6313). Packaging copies it to `vsesvit.dll` next to the renamed bootstrap (§6.4) |

**Process images per build.**

| Build | Browser process | CEF sub-processes | Sandbox |
|---|---|---|---|
| Linux, any | `vsesvit` (crates/vsesvit) | `vsesvit-helper` beside it (`browser_subprocess_path`) | namespace + seccomp-BPF (SC §3.3) |
| Windows GPUI, release package | `vsesvit.exe` = CEF `bootstrap.exe` renamed, loading `vsesvit.dll` (= `vsesvit_app.dll`) | the same image | `bootstrap.exe` + `RunWinMain` (SW §3.2) |
| Windows GPUI, `cargo run` | `vsesvit.exe` (crates/vsesvit) | the same exe (empty `browser_subprocess_path`) | off (`no_sandbox = 1`); dev only (SW §3.1) |
| Windows WinUI | `vsesvit.exe` (crates/vsesvit) | WebView2's own | WebView2's own |

Same-exe mode on Windows dev builds avoids a second Windows binary that release packages would never ship. Linux keeps the helper because release builds use it too.

Every entry point runs the same two calls in the same order. `vsesvit_cef::run_subprocess(&sandbox)` comes first: it calls `api_hash` (G1) and `execute_process`, and returns `Some(code)` in a sub-process. `vsesvit_gpui::run(&factory)` follows in the browser process. On Linux with the helper, `execute_process` returns -1 at once; it stays so that a same-exe fallback and stray `--type=` relaunches behave (SC §3.1).

### 1.2 `vsesvit-cef` module tree

Built from SC §13.1 and SW §13.1. All code runs on the CEF UI thread (= GPUI's main thread, §3.4) unless marked IO.

```
crates/vsesvit-cef/
  build.rs                      Linux: nothing (rpath is set by the bins). Windows: nothing; cef-dll-sys copies the runtime
  src/
    lib.rs                      run_subprocess(&Sandbox) -> Option<ExitCode>; helper_main(); CefEngineFactory; Sandbox
    app.rs                      BrowserApp (switches, cached BrowserProcessHandler, default_client) and ChildApp
                                (custom schemes, RenderProcessHandler). Same schemes in every process (API §3.4)
    settings.rs                 EngineConfig -> cef::Settings + the switch table of §3.3, per OS
    driver/mod.rs               CefDriver: impl EngineDriver (wakes, do_work, shutdown)
    driver/glib.rs              Linux: GLib default-context service + `glib-fd-watch` thread (SC §4.1)
    engine.rs                   CefEngine: impl vsesvit_engine::Engine; owns the TabId -> Browser map
    shared.rs                   Arc<Shared>: atomics and Mutexes handlers read synchronously; MainThread<T> cell
    events.rs                   Sink: unbounded sender of BrowserEvent (any thread)
    client.rs                   one Client per Browsing, handlers built once and returned by clone (G8)
    handlers/                   life_span display load keyboard command focus jsdialog dialog download
                                permission context_menu find request (UI) resource_request cookie_access (IO)
    contexts.rs                 RequestContext per Browsing; the private context's lifetime
    native.rs                   window_info_child(parent, rect, scale) and popup window_info fix-ups, per OS (cfg)
    idc.rs                      IDC name table (from cef_id_for_command_id_name) and the default-deny map (SC §8.2)
    devtools.rs                 execute_dev_tools_method + one DevToolsMessageObserver; RequestId <-> message id
    schemes.rs                  vsesvit:// handler factory: new tab page, error and warning pages (IO)
    ext/                        extension bridge, External Extensions writer, uninstall machine (§5)
    strings.rs                  G3/G4/G5 helpers: owned out-param strings, non-null error buffers, take() for lists
    teardown.rs                 Running -> Closing -> Drained -> Shutdown (G16)
```

### 1.3 `vsesvit-gpui` module tree

UA §5.1 stands, with these changes:

| UA path | Change |
|---|---|
| `platform/{mod,x11,watch,capture,activation,dirs,portal,default_browser}.rs` | replaced by the tree below |
| `boot/cli.rs` (a port of GTK `cli.rs`) | thin: parsing moves to `vsesvit_core::cli` (§4.3) |
| `state/closed_tabs.rs`, `state/session.rs` plan part | use `vsesvit_core::closed_tabs`, `vsesvit_core::session::plan` (§4.3) |
| `overlay/{popup,cutout}.rs` | `overlay/{inline,window}.rs`: in-window overlay with a host hole (default), PopUp window (leaves the window) |
| `engine_bridge/focus.rs` | calls `platform::Surface::focus` instead of `platform::x11::focus_window` |
| new `self_test/desktop.rs` | refuses to run on a user's display or desktop (§7.3) |

```
crates/vsesvit-gpui/src/platform/
  mod.rs          NativeSurface + Desktop traits, PhysRect/PhysPoint/Hole, `pub type Surface`, `pub type Os`
  x11/            mod.rs (one x11rb connection, error drain), surface.rs (24-bit content host, ConfigureWindow,
                  map/unmap, SHAPE holes, aux hosts), focus.rs (focus proxy lookup, XSetInputFocus, is_within guard),
                  capture.rs (GetImage, ancestor-SHAPE-aware fallback), desktop.rs (transient-for, top-level at point,
                  pointer presses via XI2 raw events, startup id), instance.rs (Unix socket), scale.rs (one scale rule)
  windows/        mod.rs, surface.rs (content host HWND, SetWindowPos/ShowWindow deferred out of GPUI updates,
                  SetWindowRgn holes, aux hosts), focus.rs (SetFocus, refused while inactive), capture.rs
                  (PrintWindow PW_RENDERFULLCONTENT), desktop.rs (owner fix-up for PopUp and dialog windows,
                  WindowFromPoint, WH_MOUSE thread hook for pointer presses, AllowSetForegroundWindow),
                  instance.rs (named pipe), console.rs (AttachConsole for headless commands)
  fake.rs         FakeSurface for TestAppContext (cfg(test) or feature "fake-platform")
  files.rs        open/save dialogs: GPUI prompt_for_paths where it suffices, ashpd portal with filters on Linux
  shell_open.rs   open a file or folder, reveal in the file manager, per OS
  default_browser.rs
```

### 1.4 The platform seam

All native window work is in `vsesvit-gpui::platform`. Each OS has one implementation, chosen by `cfg` in `platform/mod.rs`. The traits exist so both backends must compile against the same signatures; the shell uses the aliases, never generics (one implementation per build, so `BrowserWindow<S>` from SW §13.2 would only add reading load).

```rust
// platform/mod.rs
pub use vsesvit_engine::{NativeHandle, PhysRect};      // physical px, relative to the GPUI window's client area
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub struct PhysPoint { pub x: i32, pub y: i32 }   // screen px
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
}

#[cfg(all(target_os = "linux", not(any(test, feature = "fake-platform"))))] pub type Surface = x11::XSurface;
#[cfg(all(windows, not(any(test, feature = "fake-platform"))))]             pub type Surface = windows::WinSurface;
#[cfg(any(test, feature = "fake-platform"))]                                 pub type Surface = fake::FakeSurface;
```

What differs per OS, and where (SW §13 table, extended):

| Concern | X11 (`platform/x11`) | Windows (`platform/windows`) | Source |
|---|---|---|---|
| Content host | 24-bit `InputOutput` window, root visual, default colormap, border 0, selects no events | plain child HWND, `WS_CHILD \| WS_CLIPCHILDREN \| WS_CLIPSIBLINGS` | SC §5.2, SW §5 |
| GPUI under the page | irrelevant; children are above | transparent: `WindowBackgroundAppearance::Transparent`, no theme background behind the content element | SW §5, §14.2 |
| Geometry, show/hide | `ConfigureWindow`, map/unmap, from the content element's prepaint | `SetWindowPos`, `ShowWindow(SW_HIDE/SW_SHOWNA)`, spawned out of the GPUI update | SC §5.3, SW §5 |
| Overlay holes | SHAPE bounding+input on the host, for every overlay (paint and input) | `SetWindowRgn` on the host, for interactive overlays only; paint is free | SC §9.1, SW §9.1 |
| Focus | `XSetInputFocus` to GPUI's focus proxy (patched GPUI) or the page child; acts only when the X focus is inside our toplevel | `SetFocus`; refused while our toplevel is inactive; never `activate_window` (it calls `SendInput`) | SC §7.3, SW §7, §13.3 |
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

pub struct TabSpec {
    pub tab: TabId, pub kind: TabKind, pub browsing: Browsing,
    pub parent: NativeHandle,                // the window's content host, or an AuxHost for extension popups
    pub bounds: PhysRect, pub scale_factor: f32, // the glue converts to what CEF wants (DIP on Linux, SC §5.3)
    pub visible: bool, pub page: InitialPage, pub opener: Option<TabId>,
}
pub enum TabKind { Page, ExtensionPopup { min: (u32, u32), max: (u32, u32) } }   // ExtensionPopup = Alloy style (EA §7.2)

pub enum TabEvent { Created { native: NativeHandle }, /* … the rest of UA §4.2 … */ }
pub enum PopupDecision { Block, AllowAsTab { tab: TabId, parent: NativeHandle, bounds: PhysRect, scale_factor: f32 } }

pub trait Engine {
    // removed: set_bounds, set_occlusions, reparent  (the shell's platform does these on the page handle)
    fn set_visible(&self, tab: TabId, visible: bool);         // was_hidden only
    fn notify_moved(&self, tab: TabId);                       // notify_move_or_resize_started: closes <select> popups
    fn notify_screen_info_changed(&self, tab: TabId);         // scale change (SC §5.4, SW §5)
    // removed: sync_extensions, loaded_extensions; added: the extension calls of §5.4
    /* … the rest of UA §4.2 … */
}
```

`EngineConfig` gains the paths and switches of §3.2 and §3.3. The Windows sandbox pointer is not part of the contract: `CefEngineFactory::new(Sandbox)` carries it.

### 1.6 Ownership and threads

| Thread | Runs | Owns | Rule |
|---|---|---|---|
| main | GPUI's loop (calloop on Linux, `GetMessageW` on Windows); CEF's UI thread inside `do_message_loop_work`; on Windows every Chromium browser HWND; on Linux Chromium's GLib sources | `vsesvit_core::Profile` (`Rc<RefCell<…>>` in the `Shell` global, UA §1.4), every entity, `Rc<dyn Engine>`, `Surface`s, `Desktop` | core is borrowed for one call, never across an engine or GPUI call (inv/shell-core §5). CEF callbacks never touch `gpui::App`: they run inside `do_work`, outside any GPUI update, and only send events (SC §13.3) |
| GPUI background executor | core's slow `Send` jobs: installs, favicon fetch, suggest, sync rounds, update download/install, imports | the job values | result committed on main through `cx.spawn` (UA §1.3) |
| GPUI timer thread | wakes | — | — |
| `glib-fd-watch` (Linux) | `poll(2)` on GLib's fds, wakes the pump task | eventfd | stays disarmed until main re-arms (SC §4.1) |
| CEF IO and other CEF threads | `resource_request`, `cookie_access`, scheme handlers, `on_schedule_message_pump_work` | `Arc<Shared>`, `Arc<dyn RequestFilter>` snapshots | never `Profile`; only atomics, Mutexes, channels (G6) |
| instance accept thread | blocking accept on the socket or pipe | — | started after `cef::initialize` (UA §1.2 step 7) |
| X11 watcher thread (Linux) | XI2 raw presses on our own connection | — | results through a channel |

Handlers that run on the UI thread but are reached through `Client` getters (which CEF also calls on IO, API §3.5) hold main-thread state in `shared::MainThread<T>`. It is `Send + Sync` by an `unsafe impl`, records the main thread id at creation and panics on access from any other thread. It holds `Rc<dyn ShellHooks>` and the TabId map, and so makes the UI-thread-only contract a runtime check instead of a comment.

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
| `x11rb` | `0.13`, features `shape`, `xinput`, `resource_manager`, `allow-unsafe-code` | vsesvit-gpui (Linux) | plus `xtest` only under vsesvit-gpui's `self-test` feature (`default = ["self-test"]`, `self-test = ["vsesvit-core/testkit", "x11rb/xtest"]`, as GTK's feature today), for key-routing checks on a private display |
| `windows-sys` | `0.61` | vsesvit-gpui, vsesvit-sandbox (Windows) | same major as core and vsesvit-update today. Features: Foundation, UI_WindowsAndMessaging, Graphics_Gdi, UI_HiDpi, UI_Input_KeyboardAndMouse, System_StationsAndDesktops, System_Pipes, System_Console, System_Threading, Storage_FileSystem, UI_Shell, Security |
| `raw-window-handle` | `0.6` | vsesvit-gpui | call `HasWindowHandle::window_handle` explicitly; GPUI also has an inherent `window_handle()` (SC §2) |
| `futures` | `0.3` | vsesvit-cef, vsesvit-gpui | the pump and event channels |
| `ashpd` | the current release | vsesvit-gpui (Linux) | portal file dialogs with filters (UA §1.1); GPUI's own prompt has no filters (GP §2.10) |
| `glib-2.0` | system library, linked by `#[link(name = "glib-2.0")]` in `driver/glib.rs` | vsesvit-cef (Linux) | as SC `pump.rs`; no glib crate |

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

**Keeping the vendored trees honest.** A new xtask command does the vendoring, so nobody edits the trees by hand (principle: build the lever):
- `cargo xtask vendor-gpui` downloads the pinned crate from crates.io, checks its checksum against `Cargo.lock`, unpacks it into `third_party/<crate>/` and applies `third_party/<crate>.patch`.
- `cargo xtask vendor-gpui --check` does the same into a temp dir and fails if the tree differs. It runs in CI (§6.3). A GPUI bump is: change the pin, run `vendor-gpui`, fix the patch if it no longer applies, re-run §7.

**Upstreaming.** Both patches are plain bugs for any embedder of foreign child windows (SC §7.2, SW §14.1). Each gets a Zed PR in wave 1. When one lands in a gpui-pre release, its `[patch]` line and vendored tree are deleted in the same commit as the version bump.

**Flatpak.** The cargo-sources generator skips path dependencies (`xtask/src/linux/flatpak.rs:264`), and the vendored trees are part of the repo checkout the Flatpak build already uses. No git dependency is introduced, which the generator would reject.

## 3. CEF lifecycle per OS

### 3.1 Boot order

UA §1.2 stands. The changes are the first step, the extension step and the Windows details.

| # | Step | Linux | Windows | Source |
|---|---|---|---|---|
| 0 | `run_subprocess(&sandbox)`: `api_hash` first, then `execute_process(args, ChildApp)`; return in a sub-process | in `main`, before anything else | in `RunWinMain` (release) or `main` (dev) | G1, SC §3.1, SW §3.2 |
| 0a | Windows release only: compare the `cef_version_info` that `RunWinMain` receives with our own; refuse to start on a mismatch | — | yes | SW §3.2 |
| 1 | logging (`VSESVIT_LOG`); one backend on both OSes, log file `<profile>/vsesvit.log` as WinUI has | yes | yes | inv/shell-core §19 |
| 2 | parse the CLI with `vsesvit_core::cli` (§4.3); errors exit 2 | yes | yes | UA §1.2 |
| 3 | headless commands (`--help`, `--version`, `--check-for-updates`, `--update`): no profile, no display, no CEF | yes | `AttachConsole(ATTACH_PARENT_PROCESS)` first; the bootstrap image is a GUI-subsystem exe | inv/winui-engine, `winui/platform.rs:148` |
| 4 | profile resolution; `Picker` runs a GPUI-only process with no CEF and no lock | yes | yes | UA §1.2 |
| 5 | `Profile::open(root, OpenOptions { chrome_version: "154.0.8037.98", key_store: System, .. })`; `Locked` forwards over the instance channel and exits with the owner's status | Unix socket | named pipe | UA §1.2, §1.4 |
| 6 | bind the instance listener; no accept thread yet | yes | yes | UA §1.2 step 7 |
| 7 | extensions: `reconcile()` and write the External Extensions files, because Chromium reads them only at start | yes | the Windows route is open (§9, Q5) | EL §6.2, §9.1 |
| 8 | Linux only: compute the scale once; set `GPUI_X11_SCALE_FACTOR` and pass `--force-device-scale-factor` | yes | — (per-monitor DPI) | SC §5.4, SW §5 |
| 9 | `factory.initialize(EngineConfig)` = `cef::initialize` with §3.2 and §3.3; `0` means early exit, with no further CEF call | yes | yes | API §3.1 |
| 10 | GPUI: `gpui_platform::linux(WindowingModes::X11)` / the Windows platform, `.run(app::start)` | yes | yes | UA §1.2 step 9 |
| 11 | after `run` returns: the shutdown sequence of §3.6 | yes | yes | |

**No extra threads before step 9.** The Linux sandbox wants a single thread while CEF initializes (UA §1.2, from cefclient). Steps 1 to 8 must not start one. Two traps: `vault_key()` with the Secret Service store starts an async-io reactor on first use, so nothing may touch the vault before step 9; and the instance accept thread starts in `app::start`. In debug builds, `CefEngineFactory::initialize` counts `/proc/self/task` and logs an error if it is above 1.

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
| `--password-store=<basic or gnome-libsecret>` | Linux | KWallet that does not answer stalls every request; the choice is Q8 | SC §3.2 |
| `--lang=<locale>` | Linux | `locale` is ignored there | G15 |
| `--noerrdialogs` | both | no modal on a failed extension load | EA §0 item 1 |
| `--disable-features=<CEF's list>,<ours>` | both | merge into CEF's own list, never replace it | SC §3.2 |
| `--disable-gpu-process-crash-limit` | both | a GPU process crash loop otherwise disables GPU for the session | P (prior art) |
| `--disable-smooth-scrolling`, `--disable-gpu` | both | the `scrolling.smooth` and `system.hardware_acceleration` prefs; restart-only (UA §1.2 8b) | inv/tab-engine §1.2 |
| `--load-extension=<helper dir>` | both | the helper extension, shipped unpacked (§5.3) | EL §9.2 |
| `--disable-features=CalculateNativeWinOcclusion` | Windows, self-test only | Chromium treats an off-screen window as occluded | SW §14.12 |
| `--use-fake-ui-for-media-stream`, `--use-fake-device-for-media-stream` | both, self-test only | capture checks without devices | inv/self-test §5 |

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
| exactly one caller of `do_message_loop_work`, a `cx.spawn` task outside any `cx.update` | no re-entry; CEF callbacks never re-enter GPUI's `RefCell` | SC §4.1, GP §4.1 |
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
| Fallbacks per package | setuid `chrome-sandbox` 4755 or an AppArmor userns profile (Ubuntu 23.10+); AppImage cannot setuid (nosuid mount); Flatpak needs zypak (§6.4) | none needed |
| Dev builds | same as release | `no_sandbox = 1`, same exe |

`chrome-sandbox` and the CEF assets are found relative to `libcef.so`, not the exe (API §3.1), so they sit beside it.

### 3.6 Shutdown

UA §1.6 "Quit" plus G16 and both spikes' measured sequences (SC §12, SW §12). `vsesvit-cef/teardown.rs` holds the states Running → Closing → Drained → Shutdown; `create_tab` is refused after Running.

| # | Step | Cap | Source |
|---|---|---|---|
| 1 | trigger: menu Quit, the last window's close button (`on_window_should_close` returns false while it has tabs), an update restart, the end of a self-test | | SC §12, SW §12 |
| 2 | save the session now (normal windows only) | | inv/shell-core §9 |
| 3 | hide the windows; `engine.close_all()`: `close_browser(1)` on every tab, extension popup and the hidden bridge browser (§5.4); wait for `AllTabsClosed` | 5 s, then log and go on | UA §1.6 |
| 4 | final sync and cookie `clear_at_exit` (the request contexts are still alive) | 3 s each | inv/extensions-sync-updates C.12 |
| 5 | `cx.quit()`; GPUI's `run` returns on both OSes | | SC §12, SW §12 |
| 6 | `driver.shutdown()`: pump for 200 ms, drop every CEF reference (browsers, frames, contexts, callbacks, observer `Registration`s, the `Shared` maps), `cef::shutdown()` on main | 29 ms Linux, 87–129 ms Windows measured | G16, SC §12, SW §12 |
| 7 | drop the `Profile`; assert it was the last strong reference (frees `LOCK`) | | UA §1.2 step 10 |
| 8 | delete a removed profile's data; spawn the update restart (`--profile-dir <root>`), or on Windows run `install_on_exit` | | inv/extensions-sync-updates D.6 |
| 9 | return the exit code | | |

The restart in step 8 must follow step 6, so that both `LOCK` and Chromium's singleton are free before the new process starts (inv/extensions-sync-updates D.6). GPUI's `on_app_quit` gives only 200 ms (GP §4.1), so none of this runs there.

## 4. vsesvit-core integration

### 4.1 Rules

Core already fits a third shell (inv/core-surface §0; code survey §1):
- **One `Profile` per process, on the main thread.** It is `!Send` and every accessor takes `&mut self`. The shell holds it as `Rc<RefCell<Profile>>` in the `Shell` global (UA §1.4) and borrows it for one call at a time. Main is also CEF's UI thread, so UI-thread CEF handlers may read core through the hooks (UA §4.3), with `try_borrow_mut` failing safe.
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
| Cookies, site data, clear on exit | `cookies::{third_party_blocked, site_rules, to_clear, group_by_site}` | pref `profile.cookie_controls_mode`, COOKIES content setting, `CookieManager` visit/delete, DevTools `Storage.clearDataForOrigin` | WinUI's `dialogs/site_data.rs` plan carries over as is |
| Permissions and capture | `permissions::{decide, answer, prompt, TabGrants, SitePermissions}`, prompt queue (→ core) | `on_show_permission_prompt` (return 1), `on_request_media_access_permission`, `on_dismiss_permission_prompt`, `on_media_access_change`; mirror stored settings with `set_content_setting` | without the handler Windows shows Chrome-native prompt toplevels (SW §10) |
| Downloads | `downloads::*`, download tracker (→ core), `zone_identifier` | `can_download` → 1 (the cef-rs default cancels), `on_before_download` → `cont(path, 0)` always, `on_download_updated`, item callbacks | Mark of the Web written (→ `core::os`); the "Recent download history" bubble still shows (Q2) |
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

### 4.3 What moves into core

**Criterion.** Logic moves into core (or `vsesvit-sync` / `vsesvit-update`) when WinUI has its own copy of it and it is free of toolkit and engine types once the shell passes plain values. The unit that moves it migrates WinUI in the same commit and deletes WinUI's copy (principle: migrate callers, then delete). GTK is not migrated: it is deleted at the switch-over (§8 wave 7), so GPUI ports what only GTK has straight into `vsesvit-gpui`. One exception is new: the privacy snapshot, which nobody has yet.

| # | Logic | Today | Moves to | Wave | Source |
|---|---|---|---|---|---|
| 1 | CLI grammar, `USAGE`, errors, run config (superset: `--load-extension`, `--ui-smoke` stay WinUI-only commands) | `gtk/cli.rs`, `winui/cli.rs` + `config.rs` | `core::cli` | 0 | code survey §3 |
| 2 | startup plan and session snapshot builder | `winui/session.rs` (pure `startup_plan`, `WindowPlan`), `gtk/session.rs` | `core::session::plan` | 0 | code survey §3 |
| 3 | closed-tab stack, cap 25, normal and private | `gtk/closed_tabs.rs` (generic), `winui/browser.rs:115-150` | `core::closed_tabs` | 0 | inv/tab-engine §23 |
| 4 | zoom presets, `step`, `percent` | `gtk/zoom.rs`, `winui/zoom.rs` `LEVELS` | `core::zoom` | 0 | inv/tab-engine §18 |
| 5 | window title format incl. the private suffix | both shells, different formats | `core::profiles::window_title` | 0 | inv/window §3 |
| 6 | in-page key policy per `Command` (`Native`, `Reserved`, `Overridable`) and capture rules over neutral keys | `winui/shortcuts.rs` `InPage`, `gtk/keymap.rs` | `core::shortcuts` | 0 | code survey §3 |
| 7 | omnibox edit model (`Edit`, `Key`, `Outcome`, refill) | `winui/omnibox.rs` (pure), `gtk/address_bar.rs:77` | `core::omnibox_edit` | 2 | inv/omnibox-privacy §1.4 |
| 8 | permission prompt queue (max 8, per-document dismissal, holds, guards) | `gtk/permissions.rs`, `winui/permissions.rs` + `window/permissions.rs` | `core::permissions::queue` | 3 | inv/omnibox-privacy §4.3 |
| 9 | download tracker (reservations, `.unconfirmed` hold, `Transfer`, 250 ms throttle with an injected clock, list limit, engine-state mapping) | `gtk/downloads.rs` (815), `winui/downloads.rs` (664); limits 500 vs 200 | `core::downloads::Tracker` | 3 | inv/bookmarks-downloads §7 |
| 10 | bookmark menu model, bar snapshot and `fitting`, editor `folder_choices` and save, star flow, drag `zone`/`destination`; favicon `fit_png` | both shells; menus already drifted (WinUI lacks Add Page/Folder; labels cut at 40 vs 50) | `core::bookmarks::{menu, bar, editor}`, `core::favicons::fit_png` | 3 | inv/bookmarks-downloads §1–§6 |
| 11 | privacy snapshot for IO threads (tracker level and list, allowed origins, blocked cookie hosts, HTTPS-only pref and exceptions) | WinUI's `Protection` struct, partly | `core::privacy::Snapshot` | 3 | inv/omnibox-privacy §6.2 |
| 12 | sync scheduler (`due`, `settle`, constants 10 s / 2 s / 60 s / 3 s / 120 s) as a sans-IO machine with an injected clock | `gtk/sync.rs` (657), `winui/sync.rs` (802); two state models | `vsesvit_sync::schedule` | 3 | inv/extensions-sync-updates C.2–C.4 |
| 13 | update lifecycle (`step(Event) -> Effect`, banner, status, channel change, 1 h retry, install-on-exit) | `gtk/updates/lifecycle.rs` (pure), `winui/updates.rs:33-316` | `vsesvit_update::lifecycle` | 3 | inv/extensions-sync-updates D.2 |
| 14 | self-test check-name registry and expected list (43 names shared) | `gtk/self_test.rs:83-146`, `winui/report.rs:5-53` | `core::testkit::checks` | 1 | code survey §3 |
| 15 | small Windows OS helpers: Mark of the Web writer, Downloads folder, open/reveal in Explorer, default-browser state | `winui/downloads.rs:529`, `winui/platform.rs` | `core::os` (cfg(windows); Linux halves where they exist) | 3 | inv/winui-engine |

Not moved, on purpose: the extension driver (its engine half changes completely, §5; it can move after wave 4 if it still matches WinUI), Memory Saver orchestration (no common engine operation), toolkit-shaped code (popover anchoring, widgets), and anything holding a CEF or WebView2 object.

Units 8 to 13 touch shared WinUI files (`winui/browser.rs` above all). §8 serializes the units that edit the same WinUI file.

### 4.4 What core stops needing

These lose their last caller when GTK and `vsesvit-webext` go (code survey §1; inv/core-surface §23). WinUI uses none of them (checked by grep; `cookies::block_script` is not on the list because WinUI uses it, and `restore_state` stays a field that WinUI and GPUI write as `None`). They are deleted in the switch-over wave, in the same commit as GTK:
- `ext_storage` and sync `Kind::ExtStorageSync`, unless the `storage.sync` bridge of §5.6 adopts the kind;
- `trackers::TrackerList::dnr_rules`;
- `view_source::source_page` (Chromium renders `view-source:`; `source_url` stays);
- the code that reads `TabSnapshot.restore_state` (old WebKit blobs are ignored);
- `tab_groups::extension_group_id`;
- the `chrome.permissions` request pieces in `extensions::permissions` (`request`, `Request`, `REQUEST_LEAD`, …);
- the WebKit-only manifest normalizations (`dnr_rulesets`, `WebAccessible`, `Background`, content-script run-at/world).

Core's own extension updater (`extensions/update.rs`) stays: WinUI uses it. The GPUI shell does not call it (§5.5).

## 5. Extensions

### 5.1 Who does what

CEF 154 has no extension API (CE §1.1). Chromium runs extensions natively in Chrome-style browsers; the shell feeds it installs and draws the UI Chromium cannot draw in a toolbar-less child (EA §0).

| Concern | Owner | Mechanism | Source |
|---|---|---|---|
| Desired set (synced), verification, install prompt wording | core | `extensions::{ExtensionRecord, reconcile, prepare_install, InstallJob, commit}`, `crx.rs`, `permissions::install_warnings` | EL §8 |
| Install into Chromium | shell (vsesvit-cef `ext/external.rs`) | External Extensions JSON pointing at the CRX core verified; takes effect at the next start | EL §0, §9.1 |
| Running, storage, permissions at run time | Chromium | native | |
| Updates of store extensions | Chromium | its `ExtensionUpdater` (3.4 s after start, then every 5 h) | EL §6.1 |
| Enable, disable, listing, install/uninstall events | helper extension | `chrome.management` | EL §8, §9.5 |
| Incognito, site access, shortcut changes | shell | `developerPrivate` from a hidden `chrome://extensions` page | EL §8.1 |
| Uninstall | shell | `uninstallSelf({showConfirmDialog: false})` inside the extension, then delete JSON and CRX | EL §7, §9.4 |
| Toolbar, puzzle menu, popups, Extensions window | shell (GPUI) | core `toolbar.rs` + manifest; popups are shell-hosted Alloy child browsers | EA §7 |
| Page context-menu items, command shortcuts | Chromium, through the shell | `CefMenuModel` in `run_context_menu`; Chromium's own command handling in the page | EA §5, §7.6–7.7 |

### 5.2 Install

1. Unchanged up to verification: `prepare_install` → `InstallJob::run` on a background task → install prompt (core wording) → `commit`. Sources: Chrome Web Store, Edge Add-ons and local `.crx` files.
2. `commit` keeps the original CRX bytes byte-identical at `<root>/extensions/<id>/<version>_<hash32>.crx` (immutable, as today's directories), and the shell writes `<root_cache_path>/External Extensions/<id>.json`: `{"external_crx": "<absolute path>", "external_version": "<version>"}`. Chromium installs it silently as a packed extension (location 2), which survives the incognito toggle and every reload (EL §3).
3. Chromium reads the folder only at start (EL §6.2). After an install the Extensions window says "Restart Vsesvit to finish installing", with a "Restart now" button that runs the update-restart path (§3.6 step 8) and restores the session. Startup step 7 (§3.1) re-runs `reconcile()` and rewrites every JSON idempotently, so a crash between commit and restart loses nothing (principle: idempotent operations).
4. Sources External JSON cannot take: unpacked folders install for the session only through `--load-extension` (CLI and developer use; EL §2 shows they reload as unpacked and get disabled, so they are never offered as a permanent install). XPI and AMO sources are refused with a message: Chromium cannot keep Gecko ids (inv/webext §5.2).
5. A re-install of an id the user removed must first drop the id from `external_uninstalls` in Chromium's `Preferences` file, before `cef::initialize` (EL §9.6; inferred, checked in W4.0).
6. Windows: whether Chromium on Windows reads the per-user External Extensions folder, and whether it prompts (`prompt_for_external_extensions` defaults on for Windows, CE §3.3), is not verified. Wave 4 starts with a Windows spike that decides the route (Q5).

### 5.3 The helper extension

A small MV3 extension shipped with the browser, in `crates/vsesvit-cef/ext/helper/`. It has a fixed manifest `key`, so its id is fixed.

- **Loaded** with `--load-extension=<dir>` (EL §9.2). It never reloads, developer mode stays off, and its `onInstalled` fires with `install` on every start, which it ignores. Enhanced Safe Browsing blocks command-line extensions (EL §2), so the shell sets `safebrowsing.enhanced = false` at context initialization with a non-null error buffer (G4). If the helper still does not report within 5 s, the shell logs it, shows "Extension management is unavailable" in the Extensions window, and runs without it.
- **Does:** `management.getAll`, `management.setEnabled`, and forwards `management.onInstalled`, `onUninstalled`, `onEnabled`, `onDisabled` (with the disable reason) to the shell. Nothing else. Every other operation is a page-session CDP call (§5.4).
- **Transport, both directions over stock CEF:**
  - shell → helper: a hidden Alloy browser at `chrome-extension://<helper>/bridge.html`; `execute_dev_tools_method("Runtime.evaluate", {expression: "vsesvitBridge.call(<json>)", awaitPromise: true, returnByValue: true})`; the result arrives in `DevToolsMessageObserver::on_dev_tools_method_result` and completes the `RequestId`;
  - helper → shell: the bridge page logs `console.debug("vsesvit-bridge " + json)`; `DisplayHandler::on_console_message` of that browser parses it into `BrowserEvent::Extension`. Console messages from any other browser are never parsed.
- **Why not native messaging or the CDP pipe.** Native messaging needs a host manifest and a separate process per OS. The browser-target pipe is verified on Linux only (EL §4), and on Windows Chromium's pipe setup through CRT fds 3/4 does not map onto a Rust process (inferred). The page session works the same on both OSes; EL §5 verified storage calls through `ExecuteDevToolsMethod` on a page session, and W4.0 verifies the bridge calls. The pipe (`--remote-debugging-pipe`, `spikes/ext-load/src/cdp.rs`) stays the Linux fallback.
- **Hidden pages** (the bridge, `chrome-extension://<id>/manifest.json` for uninstall, `chrome://extensions` for `developerPrivate`) are Alloy browsers that never map. Alloy keeps them out of extensions' `tabs` and `windows` lists (EA §3). They live in the content host of the most recently used window and are recreated lazily if that window closes; they hold no state.

### 5.4 Engine contract additions

```rust
// vsesvit-engine
pub enum ExtensionCall {
    List,                                              // helper: management.getAll
    SetEnabled { id: String, enabled: bool },          // helper: management.setEnabled
    SetIncognito { id: String, allowed: bool },        // hidden chrome://extensions: developerPrivate.updateExtensionConfiguration
    SetCommand { id: String, command: String, chord: Option<Chord> },   // developerPrivate.updateExtensionCommand
    RequestUpdateCheck { id: String },                 // in the extension: runtime.requestUpdateCheck()
    Uninstall { id: String },                          // re-enable if disabled, hidden manifest.json page, uninstallSelf
}
pub enum ExtensionEvent {
    HelperReady, HelperUnavailable,
    Installed { id: String, version: String, reason: InstallReason },
    Uninstalled { id: String }, Enabled { id: String }, Disabled { id: String, reason: String },
}
trait Engine { fn extension_call(&self, call: ExtensionCall) -> RequestId; /* result: RequestResult::Extension(Result<serde_json::Value, String>) */ }
enum BrowserEvent { Extension(ExtensionEvent), /* … */ }
```

`Uninstall` never resolves on its own: the extension's worker dies during the call (EL §7). The engine completes it on `ExtensionEvent::Uninstalled`, with a 20 s timeout, as WinUI guards its engine calls (inv/winui-extensions §1). The shell then deletes the JSON and the CRX and calls `Extensions::uninstall`.

### 5.5 Updates

- Store extensions (every CRX with an `update_url`): Chromium checks, downloads, verifies against the installed key and applies when the worker idles (about 30 s) or at the next start (EL §6.1). The GPUI shell does not run core's updater for them; core's `update.rs` stays for WinUI (§4.4).
- Core-managed CRXs without an `update_url`: the JSON's `external_version` is bumped, applied at the next start (EL §6.2).
- Core's `extension_installs.version` follows `ExtensionEvent::Installed { reason: Update }`, so sync and the Extensions window show the running version.
- The "Update" button calls `RequestUpdateCheck` for every enabled extension and reports what `Installed` events arrive within 30 s.
- Permission-raising updates: Chromium disables such an extension (`onDisabled` with reason `permissions_increase`, inferred). The Extensions window then shows core's re-enable prompt (`re_enable_heading`) and calls `SetEnabled`. Whether that triggers Chrome's own Views dialog is checked in W4.0 (Q19).

### 5.6 Storage

Chromium owns `chrome.storage`. `storage.sync` stays local in the GPUI shell, as on WinUI (inv/winui-extensions §12). A bridge into vsesvit sync is possible later through each extension's page session (`Extensions.getStorageItems`, EL §5) and would reuse sync `Kind::ExtStorageSync`; it is not in these waves (Q20).

### 5.7 Toolbar, popups, commands, menus

- **Buttons** come from the manifest (core `manifest.rs`: `action.default_icon`, `default_title`, `default_popup`) and core's pinned layout (`toolbar.rs`). Dynamic state (badge, `setIcon`, `setPopup`, enable/disable) has no embedder API in any style (CE §1.3), so it is not shown (Q19).
- **Click with a popup**: a `TabKind::ExtensionPopup` browser, Alloy style (so `tabs.query({active, currentWindow})` finds the real tab, EA §3), in an `AuxHost` above the content host, right edge pinned to the button, `set_auto_resize_enabled(25×25, 800×600)` and resized from `on_auto_resize`. It closes on `window.close()` (`on_before_close`), Escape, an outside press and window deactivation (EA §7.1). Keyboard focus moves into it (`FocusTarget::Page`). It gets no `activeTab` grant (EA §3), which is accepted and documented (EA §7.4).
- **Click without a popup**: Chromium offers no way to fire `action.onClicked` here (EA §4); the button opens its menu with a line explaining it (Q19).
- **`_execute_action` shortcut**: intercepted by the shell, opens the shell popup (EA §7.3).
- **Extension commands**: Chromium runs them while the page has focus and grants `activeTab` (EA §5). `on_pre_key_event` never swallows their chords (EA §7.7). With chrome focus there is no API; UA §1.9's fallback (focus the page and replay) stays provisional.
- **Context-menu items** arrive in the `CefMenuModel` (ids from 49000); the GPUI menu shows them and answers with `callback.cont(id)` (EA §5).
- **Windows Chromium opens itself** (`tabs.create`, `windows.create`, `openOptionsPage`): `default_client` + `on_after_created`. On X11 the window is hidden synchronously and embedded as a tab with Chrome's UI cropped off, which keeps the extension's tab identity (EA §0 item 2, earlier run). The fallback on either OS is to read the URL, close the window and open the URL as a tab (UA §4.4). Windows behaviour is part of W4.0 (Q18).
- **Web Store "Add to Chrome"**: WinUI's store shim (`winui/store.rs` + `js/store.js`, inv/winui-extensions §5) is ported, injected with `Page.addScriptToEvaluateOnNewDocument` on the store origins, so store installs go through core's pipeline and prompt. Without it Chromium's own `webstorePrivate` flow and Views dialog would install behind core's back (CE §3.5) (Q21).
- **`chrome://extensions`** is not linked from the UI. Its Remove button goes through Chrome's dialog (EL §9.11).
- **Per-extension private windows**: core's `allowed_in_private` → `SetIncognito` (safe for packed installs, EL §3).

### 5.8 Native messaging

Chromium looks for native messaging hosts in `<root_cache_path>/NativeMessagingHosts` and `/etc/chromium/native-messaging-hosts` on Linux (CE §1.5), and in the registry on Windows. Password-manager desktop integrations need their manifests linked there. Wave 4 adds a Settings note and a "Link native apps" action that copies known host manifests from the Chrome and Chromium locations (Linux first).

## 6. The `gpui` feature on `crates/vsesvit`, CI and packaging

### 6.1 The feature

Name `gpui`; WinUI's feature is `winui`; `default = ["winui"]`. On Linux the GPUI shell is the only shell and is not behind a feature. A runtime switch was rejected: it would ship WebView2 and CEF (about 200 MB more) in one package for no user gain (log.tsv, 2026-10-23).

```toml
# crates/vsesvit/Cargo.toml
[features]
default = ["winui"]
winui = ["dep:vsesvit-winui"]
gpui = ["dep:vsesvit-gpui", "dep:vsesvit-cef"]

[target.'cfg(target_os = "linux")'.dependencies]
vsesvit-gpui = { workspace = true }
vsesvit-cef = { workspace = true }

[target.'cfg(windows)'.dependencies]
vsesvit-winui = { workspace = true, optional = true }
vsesvit-gpui = { workspace = true, optional = true }
vsesvit-cef = { workspace = true, optional = true }

[target.'cfg(windows)'.build-dependencies]
winresource = "0.1.31"
```

The same dependency is plain on Linux and optional on Windows. Cargo accepts a `dep:` feature when at least one declaration of the name is optional; W0.1 confirms it on both OSes. If it does not, the Windows declarations are renamed (`gpui-shell = { package = "vsesvit-gpui", … }`) and only `main.rs` sees the difference.

```rust
// crates/vsesvit/src/main.rs
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
use std::process::ExitCode;

#[cfg(any(target_os = "linux", all(windows, feature = "gpui")))]
fn main() -> ExitCode {
    let sandbox = vsesvit_cef::Sandbox::none();                 // Windows release enters through vsesvit-sandbox instead
    if let Some(code) = vsesvit_cef::run_subprocess(&sandbox) { return code; }
    vsesvit_gpui::run(&vsesvit_cef::CefEngineFactory::new(sandbox))
}

#[cfg(all(windows, not(feature = "gpui"), feature = "winui"))]
fn main() -> ExitCode { vsesvit_winui::run() }

#[cfg(all(windows, not(feature = "gpui"), not(feature = "winui")))]
compile_error!("enable the `winui` or the `gpui` feature");

#[cfg(not(any(target_os = "linux", windows)))]
fn main() -> ExitCode { eprintln!("vsesvit supports Windows and Linux only"); ExitCode::FAILURE }
```

- `gpui` wins when both features are on, so `--all-features` and `--features gpui` without `--no-default-features` still build a working GPUI browser (with WinUI linked in for nothing). The documented command is `cargo build -p vsesvit --no-default-features --features gpui`.
- `crates/vsesvit-sandbox` exports `RunWinMain`, which runs the same two calls with the sandbox pointer it receives: `run_subprocess(&Sandbox::from_raw(info))`, then `vsesvit_gpui::run(…)`.
- `crates/vsesvit/build.rs` keeps the `winresource` icon for both Windows flavours and adds `cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN` on Linux (G14); `crates/vsesvit-helper/build.rs` adds the same rpath.

### 6.2 xtask

| Command | Change |
|---|---|
| `package <formats>` | Linux formats build `-p vsesvit -p vsesvit-helper` and stage the CEF runtime (§6.4). New Windows format `nsisgpui` builds `-p vsesvit-sandbox --no-default-features --features gpui` into `target/gpui` (its own target dir, so the two Windows flavours do not rebuild each other) and stages the bootstrap image. `nsis` stays WinUI and unchanged |
| `vendor-gpui [--check]` | new (§2.2) |
| `check-deps` | new: fails if `vsesvit-engine` depends on `gpui` or `cef`, `vsesvit-gpui` on `cef` or `vsesvit-cef`, or `vsesvit-cef` on `gpui` (`cargo metadata`). The crate rules of §1.1 as a check, not a convention |
| `selftest [--flavour gpui\|winui] [--network]` | new: builds, then runs `--self-test OUT_DIR` on a private display (Linux: headless Weston with Xwayland, else Xvfb) or a private desktop (Windows: the deskrun mechanism of SW §1.1, moved from the spike into `xtask/src/windows/deskrun.rs`), and prints `report.json` (§7.3) |
| `stamp-exe <exe> --icon … --version …` | new, Windows: writes our icon and version resources into the renamed `bootstrap.exe` with `UpdateResourceW` (Q15) |
| `bundle-sandbox` | new, Windows: lays out the sandboxed image (renamed bootstrap, `vsesvit.dll`, CEF runtime) in `target/gpui/bundle`, as SW's `bundle-sandbox.ps1`; used by W1.3's checks and by `package nsisgpui` |

### 6.3 CI

There is no test workflow today; `release.yml` only packages (inv/packaging-ci; code survey). A new `ci.yml` runs on pushes and pull requests:

| Job | Runner | Steps |
|---|---|---|
| `linux` | `ubuntu-latest`, container `ubuntu:26.04` | apt: build-essential, pkg-config, libxkbcommon-dev, libxkbcommon-x11-dev, libxcb1-dev, libwayland-dev, libfontconfig-dev, libglib2.0-dev, libnss3, libatk-bridge2.0-0, libcups2, libgbm1, libasound2, mesa-vulkan-drivers (lavapipe), weston, xwayland; `actions/cache` for `CEF_PATH` keyed `cef-154.0.34-linux`; `xtask vendor-gpui --check`; `xtask check-deps`; `cargo clippy --workspace`; `cargo test --workspace`; `xtask selftest` |
| `windows-winui` | `windows-latest` | `cargo test --workspace` with default features (today's WinUI coverage; CEF is built too because the crates are members, cached the same way) |
| `windows-gpui` | `windows-latest` | CMake and Ninja (for `libcef_dll_wrapper`, SW §2); `CEF_PATH` cache keyed `cef-154.0.34-windows`; `cargo build -p vsesvit --no-default-features --features gpui`; `cargo test -p vsesvit-engine -p vsesvit-cef -p vsesvit-gpui`; `xtask selftest --flavour gpui` on a private desktop |

A CI runner's desktop is not a user's, but the self-test still runs on a private desktop there, so the same command behaves the same on a developer's PC (§7.3).

`release.yml` changes:
- `windows` job: package `nsis` and `nsisgpui`, each staged right after its build.
- `linux` job: the apt list loses `libgtk-4-dev`, `libadwaita-1-dev`, `libwebkitgtk-6.0-dev`, `libglib2.0-bin`, gains the X11/xkbcommon set above; the `CEF_PATH` cache.
- `flatpak` job: the new manifest of §6.4; stays skipped on nightly.
- `sign`: the glob `*-setup.exe` (`release.yml:218`) already matches `…_x64-gpui-setup.exe`.
- `release`: `xtask manifest` iterates `Format::ALL`, which gains `NsisGpui`.
- `xtask/tests/release_workflow.rs` pins every new action by SHA, as it does today.

### 6.4 Packages

The CEF runtime is copied beside the exe by `cef-dll-sys` (SC §2, SW §2) and staged from there. Sizes are measured from the 154.0.34 distributions (inv/packaging-ci §0.1; SW §2).

| Format | Layout | Sandbox | Dependencies and notes |
|---|---|---|---|
| deb, rpm, pacman | `/usr/lib/vsesvit/`: `vsesvit`, `vsesvit-helper`, `libcef.so` (stripped: 272 MB, about 83 MB zstd), `chrome-sandbox`, `*.pak`, `icudtl.dat`, `v8_context_snapshot.bin`, `locales/`, `libvk_swiftshader.so`, `libvulkan.so.1`, `vk_swiftshader_icd.json`, `helper-extension/`, `CREDITS.html`, `package-format`; `/usr/bin/vsesvit` symlink | namespace; `chrome-sandbox` installed 4755 as the fallback (deb `set_mode`, rpm `%attr(4755,root,root)`); an AppArmor profile allowing user namespaces for Ubuntu 23.10+ in the deb | new runtime dependency table (nss, nspr, glib, atk/at-spi, dbus, cups, X11 libs, gbm, expat, xkbcommon-x11, udev, alsa, vulkan loader, fontconfig/freetype; P §8.2); GTK, libadwaita, WebKitGTK and GStreamer go. rpm has `AutoReqProv: no`, so every library is a written `Requires:` |
| AppImage | same tree under `usr/lib/vsesvit/` | namespace only (the mount is `nosuid`); where user namespaces are blocked, start with `--no-sandbox` and say so on stderr | drop the WebKit relocation and GStreamer bundling; reconcile the sysroot's host-provided list with the AppImage excludelist for NSS, atk, dbus and cups (P §10.3) |
| Flatpak | `/app/lib/vsesvit/` | CEF cannot nest its namespace sandbox in Flatpak's; use zypak (Chromium base app) as the launcher | the 326 MB CEF tarball becomes an offline manifest source with a SHA-256 we compute; `CEF_PATH` set; unset the `FLATPAK` env var that makes `cef-dll-sys` look in `/usr/lib` (P §9) |
| NSIS (WinUI, `nsis`) | unchanged | WebView2 | |
| NSIS (GPUI, `nsisgpui`) | `$LOCALAPPDATA\Programs\Vsesvit\`: `vsesvit.exe` (renamed `bootstrap.exe`, stamped), `vsesvit.dll`, `libcef.dll`, `chrome_elf.dll`, `d3dcompiler_47.dll`, `dxcompiler.dll`, `dxil.dll`, `vk_swiftshader.dll`, `vk_swiftshader_icd.json`, `vulkan-1.dll`, `icudtl.dat`, `v8_context_snapshot.bin`, `*.pak`, `locales\` (`File /r`), `helper-extension\`, `vsesvit.ico`, `package-format` = `nsisgpui` | bootstrap | about 423 MB installed; the LZMA installer must stay under the updater's 512 MiB cap (measured in W6); the Windows App Runtime check is compiled out; `CloseApp` waits for every `vsesvit.exe` process, since CEF's children are the same image |

**One Vsesvit per user on Windows.** Both NSIS flavours install into the same directory under the same uninstall key, ProgIDs and `StartMenuInternet` entry. Each installer first deletes the other flavour's payload list (xtask knows both and passes them as defines). Running the other installer switches flavour in place. The profile is shared; engine data is not (§3.2), so site logins do not carry over between flavours.

**Licences.** CEF and Chromium add BSD-3 and Chromium's notices. `CREDITS.html` ships in every package, and the metainfo licence field gains them (P §3; `ctx.rs:148` test updated).

### 6.5 The updater and the flavour

Linux needs nothing new: the GPUI shell replaces GTK under the same `deb`, `rpm`, `pacman` and `appimage` variants, so existing installs update in place to GPUI. Windows separates the flavours:

| Change | Where | Why |
|---|---|---|
| `Installation::NsisGpui`, `Format::NsisGpui`, marker and variant `nsisgpui` (no hyphen) | `vsesvit-update` `installation.rs`, `updater.rs`; `xtask` `ctx.rs` | the variant is the third `-` part of the manifest key, and `manifest.rs:220-223` asserts 2 or 3 parts |
| artifact `Vsesvit_<v>_x64-gpui-setup.exe`, key only `windows-x86_64-nsisgpui` | `xtask` `ctx.rs` | the plain `windows-x86_64` key is WinUI's fallback |
| flavoured variants never fall back to `<target>-<arch>` | `release.rs:94` | otherwise a GPUI install whose release lacks its key silently installs WinUI; signature, version and the `MZ` check all pass (`updater.rs:147`) |
| download-cache names parsed longest suffix first | `updater.rs:417` | `-setup.exe` would otherwise match `…-gpui-setup.exe` as version `1.0.0-gpui` |
| the update check never runs in a CEF sub-process | `vsesvit-gpui` boot (sub-processes return at step 0 anyway) | inv/extensions-sync-updates D.9 |
| restart after `cef::shutdown` | §3.6 | both locks free |

Two issues grow with CEF's size and are Q16: verification reads the whole artifact into memory twice (`updater.rs:446`), and `rpm -U` resolves no new dependencies on the GTK → GPUI update.

## 7. Testing

### 7.1 Layers

| Layer | What | Runs where | Needs |
|---|---|---|---|
| core unit tests | every moved module of §4.3 brings its tests from GTK or WinUI (the 8 `cli.rs` tests, `ClosedTabs`, the startup plan, the omnibox edit model's 8 tests, the 11 `permissions.rs` tests, the download tracker, `lifecycle.rs`, sync `due`/`settle`) | `cargo test`, every OS | nothing |
| `vsesvit-engine` | `FakeEngine` behaviour (UA §4.5) | `cargo test` | nothing |
| `vsesvit-gpui` | views and entities in `gpui::TestAppContext` with `FakeEngine` and `FakeSurface`: UA §5.3 acceptance tests (layout `classify`, close rules, title cases, `TabListModel`, router delivery, `Hooks` never borrows `App`, keymap and routing tables, overlay bookkeeping) | `cargo test`, every OS | no display, no CEF, no X server (`xtask check-deps` keeps it so) |
| `vsesvit-cef` | pure parts: switch table per OS, IDC default-deny map, reserved-key table, transition and disposition mapping, `strings.rs` helpers, teardown state machine | `cargo test` | the CEF runtime (cached in CI) |
| platform probes | `crates/vsesvit-gpui/examples/native_probe.rs`: a GPUI window with a foreign child (an x11rb window or a plain HWND standing in for CEF); geometry follows layout, show/hide, holes route a press to GPUI (hit test), focus both ways, capture shows the child's colour, PopUp owner fix-up, `toplevel_at` | private display or desktop (§7.3) | no CEF |
| self-test | `vsesvit --self-test OUT_DIR [--network]`: the real binary, in process, scripted (§7.2) | private display or desktop | CEF |
| hand tests | real mouse, IME, other display scales, multi-monitor (§7.4) | the user's own desktop, only when the user runs them | the user |

### 7.2 The self-test port

The contract stays `docs/design/self-test.md` and inv/self-test §10: same flags, exit codes, stdout lines, `report.json`, per-check timeouts with "last observed", cleanup guards, scripted mode, fixture server, probe extension. Check names come from `core::testkit::checks` (§4.3 row 14), shared with WinUI, and the guard test that scans the source for `ctx.check("` is ported.

How each harness need is met under CEF:

| Need | GTK / WinUI today | GPUI + CEF |
|---|---|---|
| page eval with a result | `evaluate_javascript_future` / WebView2 | `engine.eval` → DevTools `Runtime.evaluate` through `execute_dev_tools_method` (API §3.7) |
| keys and clicks into the page | WebKit / DevTools input | CEF `send_key_event` / `send_mouse_click_event` for page-level checks. Key-routing regressions need the native path: Linux XTEST into the private X server (works there, EA §1); Windows `PostMessage` to our own HWNDs (the `keypost` of SW §1.2), which is the only route that exercises the GPUI key patch |
| keys into the chrome | GTK actions | `window.dispatch_keystroke` and actions |
| screenshots with popovers | GSK render + popover composite | `Surface::capture` (includes the page, SC §11, SW §11) + PopUp windows composited at their positions |
| extension install | in-process | the probe CRX is committed in step 7 before `cef::initialize`, so it loads at start. The restart path (install while running → restart → loaded) runs as a second phase: at the end of phase 1 the process re-executes itself as `vsesvit --self-test OUT_DIR --phase 2` on the same profile and merges the reports. The reviewer still runs one command |
| capture devices | WebKit mock devices | `--use-fake-device-for-media-stream` (§3.3) |

Checks that change meaning (inv/self-test §5):
- `memory_saver` asserts the frozen state of Q9, not a terminated web process.
- `session` and the reopen checks assert URLs, not back/forward lists (Q7).
- `install_crx`, `cws_install`, `engine_loaded_extension`, `extension_update` follow §5: JSON written, loaded at start, Chromium's version reported.
- `popup`, `extension_toolbar` assert the shell-hosted Alloy popup and manifest-only buttons.
- `extension_windows` asserts EA §2's model (one popup window per tab).
- `spellcheck` uses Chromium's dictionaries; `tracking_protection` can now assert a blocked count.
- Windows-only checks a Chromium shell can adopt (`bookmarks_bar_icons`, `download_pause`, `passwords_purged`, `shortcuts_sync`) join the shared list.

### 7.3 Where the self-test runs, and never runs

The PC this is developed on is the user's. Two spikes showed why tests must not touch its display: Chromium focuses new browsers itself and steals the foreground (SW §1.1), and a private D-Bus session leaked the owner's display into portal dialogs (SC §1).

| | Linux | Windows |
|---|---|---|
| Display | headless Weston with Xwayland (`weston --backend=headless --xwayland`), which also gives a window manager, so activation and focus behave like a desktop; Xvfb as the fallback where Weston is missing (the WSL setup of EL §1). Never `:0` or WSLg | a private desktop created with `CreateDesktopW` and the process started with `STARTUPINFO.lpDesktop` (SW §1.1). Windows can activate, focus and go fullscreen there without reaching the user's screen |
| Environment | `env -i`, a private `dbus-run-session` whose environment carries only the private `DISPLAY` (SC §1), `HOME = OUT_DIR/home`, lavapipe | `OUT_DIR` for every path; `--disable-features=CalculateNativeWinOcclusion` |
| Guard inside the binary | `--self-test` refuses (exit 3) unless `VSESVIT_PRIVATE_DISPLAY=1`, which only `xtask selftest` sets after starting its own server | `--self-test` compares its thread desktop with `OpenInputDesktop` and refuses (exit 3) when they are the same. In both, `VSESVIT_SELFTEST_ON_MY_DESKTOP=1` overrides it for a run the user starts on purpose |
| Input | XTEST only into the private server | posted messages to our own HWNDs only; never `SendInput`. `clippy.toml` `disallowed-methods` bans `gpui::Window::activate_window` (it calls `SendInput` and `SetForegroundWindow`, SW §1.1) |

The guards turn the rule into a check: running the self-test the wrong way fails instead of reaching the user's screen.

WinUI's own self-test is unchanged. Units that migrate WinUI code (§4.3) run it too. Whether WinUI's XAML runs on a private desktop is not known; if it does not, those units hand the 47-check run to the user instead of running it on the user's desktop (Q22).

### 7.4 Hand tests

Only the user runs these, on the user's own desktop, at the end of the waves that make them meaningful: real mouse and wheel input; IME (ibus or fcitx on Linux, Microsoft IME on Windows) in the omnibox and in the page; 100, 150 and 200 % display scale and moving a window between monitors (SW §5 could test only 175 %); the owner's real KWin session (SC §13.4 used only a virtual one); drag and drop between the page and GPUI. Each has a written script in `docs/design/linux-cef/hand-tests.md`, created in wave 2.

### 7.5 Gates every wave passes

- Builds: Linux; Windows default (WinUI, unchanged behaviour); Windows `--no-default-features --features gpui`.
- `cargo test --workspace` on both OSes; `cargo clippy`; `xtask check-deps`; `xtask vendor-gpui --check`.
- The self-test, with every check that exists so far, passing on Linux (private display) and Windows (private desktop).
- From wave 1 on, three standing regression checks in the self-test: 30 open/close cycles leak no processes, fds or handles (SC §6, SW §6); quit leaves no process and exits 0 (SC §12, SW §12); a key posted to the page reaches it, and a key typed in the omnibox with the pointer over the page stays in the omnibox (the two GPUI patches).

## 8. Implementation waves

### 8.1 Rules

- **Every unit is independently landable.** It builds on both OSes, passes §7.5, and is committed on its own. A unit that cannot be finished is not landed half-way; its stubs stay.
- **One owner per file**, as UA §5.1. W0.1 creates every new file of the trees in §1.2 and §1.3 with its frozen stub (UA §5.2 plus §1.4, §1.5 and §5.4 of this document), and declares every new core module of §4.3 as an empty file in `crates/vsesvit-core/src/lib.rs`. Later units fill their own files and never edit another unit's file; an interface change goes through the file's owner.
- **Parallel units** in a wave touch disjoint files, including WinUI files when they migrate WinUI (§4.3). Where two units need the same WinUI file, the table says which goes first.
- **Each unit adds its self-test checks** in its own file under `self_test/checks/`, so the self-test grows with the features and wave 5 only completes it.
- **Each wave ends with a review** (Opus subagent for the wave's diff), then the next wave starts.

### 8.2 Wave 0: skeleton and contracts

| Unit | Files | Depends | Done when |
|---|---|---|---|
| W0.1 skeleton | root `Cargo.toml` (`exclude`, `[patch.crates-io]`, workspace deps, rust-version, dev profile); `third_party/**`; `xtask` `vendor-gpui`, `check-deps`; `crates/vsesvit/{Cargo.toml, build.rs, src/main.rs}`; the five new crates with stub trees; `.github/workflows/ci.yml`; `clippy.toml`; empty new core modules | — | the three builds of §7.5 compile; existing tests pass; `cargo tree -i gpui-pre-linux` and `-i gpui-pre-windows` show the `third_party` path; `vendor-gpui --check` and `check-deps` pass in CI; the mixed optional dependency of §6.1 is confirmed on both OSes; Windows default build behaves exactly as before (WinUI) |
| W0.2 engine contract | `crates/vsesvit-engine/**` | W0.1 | UA §4.2 with §1.5 and §5.4 applied, every item documented; `FakeEngine` (UA §4.5) with tests; reviewed as the contract by the W1.3 owner |
| W0.3 core moves A | core: `cli`, `session/plan`, `closed_tabs`, `zoom` presets, `profiles::window_title`, `shortcuts` in-page policy and capture rules (§4.3 rows 1–6); WinUI: `cli.rs`, `config.rs`, `session.rs`, `browser.rs` (closed tabs), `zoom.rs`, `shortcuts.rs`, the title code | W0.1 | moved tests pass in core; WinUI uses the core versions and its copies are deleted; `cargo test -p vsesvit-winui` passes; WinUI self-test (47) passes, or the user's hand run if Q22 says it cannot run on a private desktop |
| W0.4 theme | `vsesvit-gpui/src/theme/**`, `assets/**`, `examples/theme_gallery.rs` | W0.1 | UA U02 acceptance; the gallery renders light and dark on both OSes (captured on a private display/desktop) |

W0.2, W0.3 and W0.4 run in parallel after W0.1.

### 8.3 Wave 1: foundation (crate, platform seam with both backends, engine core, boot, window and tabs)

| Unit | Files | Depends | Done when |
|---|---|---|---|
| W1.1 platform X11 | `platform/x11/**`, `examples/native_probe/x11.rs` | W0.1 | every `native_probe` check of §7.1 passes on headless Weston and on Xvfb; the focus patch's upstream PR is open |
| W1.2 platform Windows | `platform/windows/**`, `examples/native_probe/windows.rs`, `xtask/src/windows/deskrun.rs` | W0.1 | every `native_probe` check passes on a private desktop; deskrun works from `xtask`; the key patch's upstream PR is open |
| W1.3 CEF engine core | `crates/vsesvit-cef/**` except `ext/` and `schemes.rs`; `crates/vsesvit-helper/**`; `crates/vsesvit-sandbox/**`; `xtask bundle-sandbox` | W0.2; W1.1 and W1.2 for the smoke example, which hosts browsers in `vsesvit-gpui`'s `platform::Surface` through a dev-dependency (`check-deps` checks normal dependencies only) | unit tests of §7.1; on both OSes `crates/vsesvit-cef/examples/smoke.rs` (an ordinary bin, so the same-exe re-exec on Windows works; libtest binaries cannot be re-executed as `--type=renderer`, API §3.1) creates, navigates, switches and closes three tabs and shuts down with exit 0 and no processes left; the Windows sandboxed bundle shows renderers `Lockdown` in `chrome://sandbox`; Linux shows "adequately sandboxed" |
| W1.4 boot | `vsesvit-gpui/src/boot/**`, `app/quit.rs` | W0.3 (core cli), W1.1 and W1.2 (instance transports), W1.3 (factory) | boot order §3.1 and shutdown §3.6; instance tests on both OSes (forward to an in-process listener, exit status and stderr relayed, the unreachable message); headless commands work without a display; no extra thread before `cef::initialize` (debug check) |
| W1.5 state and tab core | `app/{mod,shell}.rs`, `state/**`, `tab/{mod,model,navigation}.rs` | W0.2, W0.3 | the UA U05 and U06 acceptance tests that need no later feature (session throttle, last-tab-in-session, active window first, pinned and grouped restore, `TabListModel` rules, invocation outcomes, `releasing_the_profile_frees_its_lock`) with `FakeEngine` |
| W1.6 bridge | `engine_bridge/**` | W0.2, W0.3 | UA U07 acceptance; routing table built from core's in-page policy |
| W1.7 window and minimal chrome | `window/**` (except `fullscreen.rs`), `tabs_ui/{mod,row,strip,sidebar}.rs`, `toolbar/mod.rs` (back, forward, reload, a plain address field) | W1.1, W1.2, W1.5, W1.6 | UA U08 TestAppContext tests that apply; the window runs on both OSes; Windows content rect transparent |
| W1.8 self-test foundation | `self_test/{mod,report,runner,desktop,capture}.rs`, `self_test/checks/foundation.rs`, `test_support.rs`, `xtask selftest`; core `testkit/checks.rs` + WinUI `report.rs` (§4.3 row 14) | W1.1–W1.7 | checks `profile_open`, `navigate`, `tabs`, `tab_layout`, `session`, `screenshot`, a `shortcuts` subset, and the three standing regressions of §7.5 pass on Linux and Windows; the desktop guards refuse a run on the user's display/desktop; release-build idle CPU recorded in `log.tsv` for both OSes |

W1.1, W1.2, W1.3, W1.5 and W1.6 run in parallel (W1.3's smoke check waits for W1.1 and W1.2). W1.4 and W1.7 follow; W1.8 closes the wave. **End of wave 1:** a browser window with tabs, navigation and correct focus on both OSes.

### 8.4 Wave 2: chrome

| Unit | Files | Depends | Done when |
|---|---|---|---|
| W2.1 overlays and dialogs | `overlay/**`, `dialogs/{mod,host,prompts,library}.rs`, `examples/overlay_demo.rs` | W1 | `overlay_demo` over a foreign child on both OSes: a menu with a submenu whose items take clicks over the page, a passive list driven from an entry, outside-press dismissal; bookkeeping tests (UA U04) |
| W2.2 omnibox | `omnibox/{mod,edit,field,suggestions,progress,zoom_bubble}.rs`; core `omnibox_edit` + WinUI `omnibox.rs` (§4.3 row 7) | W2.1 | UA U11 acceptance; WinUI tests pass on the core model; self-test `omnibox`, `address_completion`, `search_suggestions`, `address_progress` |
| W2.3 tabs UI | `tabs_ui/{tab_menu,tab_search,group_editor,drag}.rs` | W2.1 | UA U09 acceptance; tab drag between windows with `move_page_to`, falling back to reopen (UA S:10); self-test `tab_menu`, `tab_search`, `tab_groups`, `ctrl_s_toggles_sidebar` |
| W2.4 toolbar, menu, find | `toolbar/{main_menu,update_banner}.rs`, `find_bar.rs` | W2.1 | UA U10 acceptance; self-test find and menu checks |
| W2.5 page features | `tab/{content,error_page}.rs`, `window/fullscreen.rs`, `context_menu/**`, `vsesvit-cef/src/schemes.rs`, `vsesvit-cef/src/handlers/context_menu.rs` (handed over from W1.3) | W2.1 | GPUI context menu from `CefMenuModel` with our items; print, DevTools, view source, MHTML save; zoom with core presets; fullscreen and notice; link preview with Chrome's status bubble off; new tab page, error, certificate and HTTPS-only pages under `vsesvit://`; self-test `new_tab_page`, `https_only`, `zoom_indicator`, `page_commands`, `save_page`, `selection_search`; `hand-tests.md` written |

All five run in parallel after W2.1's frozen overlay API (W2.1 lands first or its stubs are used). **End of wave 2:** a browser that is usable for daily browsing on both OSes.

### 8.5 Wave 3: features

| Unit | Files (shell) | Core and WinUI files | Done when |
|---|---|---|---|
| W3.1 privacy | `omnibox/{site_info,permission_prompt}.rs`, `tab/permissions.rs`, `privacy/**` | core `permissions/queue`, `privacy` (rows 8, 11); WinUI `permissions.rs`, `window/permissions.rs`, `trackers.rs` | UA U12 acceptance; self-test `site_permissions`, `tracking_protection`, `cookies`, `connection_info`, `capture_in_use`, `private_window` |
| W3.2 bookmarks | `bookmarks/**`, `favicons.rs`, `dialogs/{bookmarks,history}.rs` | core `bookmarks/{menu,bar,editor}`, `favicons::fit_png` (row 10); WinUI `bookmarks_bar.rs`, `bookmark_editor.rs`, `browser.rs` (star, favicon preload) | UA U13 acceptance; self-test bookmark checks |
| W3.3 downloads | `downloads/**`, `toolbar/downloads_button.rs`, `dialogs/downloads.rs`, `save_page.rs` | core `downloads/tracker`, `os` (rows 9, 15); WinUI `downloads.rs`, `dialogs/downloads.rs`, `platform.rs` | UA U14 acceptance; self-test `download`, `download_safety`, `download_speed`, `download_pause`; no file ever lands in the real Downloads folder (handler always set) |
| W3.4 profiles | `profiles/**`, `toolbar/profile_button.rs` | — | UA U15 acceptance; self-test `profiles` |
| W3.5 settings | `dialogs/settings/**`, `dialogs/{about,shortcuts_help}.rs` | — | UA U17 acceptance; self-test `settings`, `spellcheck`, `search_engines` |
| W3.6 sync and welcome | `sync.rs`, `dialogs/{welcome,passphrase}.rs` | `vsesvit-sync` `schedule` (row 12); WinUI `sync.rs`, `dialogs/sync_prompt.rs` | UA U18 sync acceptance; self-test `sync_passphrase`, `welcome` |
| W3.7 updates | `updates/**` | `vsesvit-update` `lifecycle` (row 13); WinUI `updates.rs` | UA U18 update acceptance; banner and Settings rows on both OSes |
| W3.8 Memory Saver and private sessions | `state/{memory_saver,private_session}.rs`, `tab/sleep.rs` | — | Q9 decided and implemented; self-test `memory_saver` |

All eight run in parallel; their shell, core and WinUI files are disjoint as listed. `winui/browser.rs` is edited only by W3.2 in this wave (W0.3 edited it earlier). **End of wave 3:** feature parity with the GTK shell's checklists except extensions.

### 8.6 Wave 4: extensions

| Unit | Files | Depends | Done when |
|---|---|---|---|
| W4.0 spike `ext-win` and bridge | `docs/design/linux-cef/spikes/ext-win/**`, `spike-ext-win.md` | W3 | each of these is VERIFIED or NOT VERIFIED with evidence, on Windows and (where marked) Linux: the Windows install route and its prompt (Q5); the page-session bridge and `uninstallSelf` through a hidden page, both OSes (§5.3); the Alloy popup `AuxHost` above the content host on Windows; Chrome-created windows on Windows (Q18); editing `external_uninstalls` before start; Chromium's behaviour on a permission-raising update (Q19); the Web Store shim (Q21). §5 of this document is updated from the results |
| W4.1 core extension adaptation | `crates/vsesvit-core/src/extensions/**` | W4.0 | `commit` keeps the CRX and records the JSON path behind an engine mode, so WinUI's path is unchanged; core and WinUI tests pass |
| W4.2 CEF extension glue | `crates/vsesvit-cef/src/ext/**`, `crates/vsesvit-cef/ext/helper/**` | W4.0 | `extension_call` and `ExtensionEvent` (§5.4) work on both OSes against the probe extension |
| W4.3 extension UI | `extensions/**`, `dialogs/{extensions,extension_prompts}.rs`, the store shim | W4.0 (and W4.2 for integration; codes against `FakeEngine` first) | UA U16 acceptance; restart-to-finish flow; popups anchored and dismissed per §5.7 |
| W4.4 extension checks | `self_test/checks/extensions.rs` | W4.1–W4.3 | the extension checks of inv/self-test §5, as redefined in §7.2, pass on both OSes |

### 8.7 Wave 5: self-test completion and parity

| Unit | Done when |
|---|---|
| W5.1 complete the self-test | every check of the shared list (62 + `cws_install` + the adopted Windows checks) runs; the ported guard tests pass; full runs pass on Linux and Windows |
| W5.2 parity audit | `docs/design/linux-cef/parity.md`: every item of every inventory checklist is marked done, changed on purpose (with the reason), or a gap with an issue |
| W5.3 hand tests | the user runs `hand-tests.md` on both OSes; findings become issues |

### 8.8 Wave 6: packaging and CI

Wave 6 touches only `xtask`, `packaging/`, `vsesvit-update` and workflows, so it can start any time after wave 1 and run beside waves 2 to 5. It must finish before wave 7.

| Unit | Files | Done when |
|---|---|---|
| W6.1 Linux native packages | `xtask/src/linux/{stage,deb,rpm,pacman,deps}.rs`, `packaging/linux/**` (AppArmor profile) | deb, rpm and pacman install on a clean container, start, show "adequately sandboxed", and pass the self-test from the installed path; `verify.sh` rewritten with per-format sandbox checks |
| W6.2 AppImage | `xtask/src/linux/appimage*.rs`, `sysroot.rs` | runs on a clean container; sandbox or the stated `--no-sandbox` fallback |
| W6.3 Flatpak | `xtask/src/linux/flatpak.rs`, `packaging/flatpak/**` | offline build with the CEF source; zypak launcher; the self-test passes inside the sandbox |
| W6.4 Windows GPUI installer | `xtask/src/windows/**`, `packaging/windows/**` | `nsisgpui` installs, upgrades, switches flavour in both directions, uninstalls clean; the stamped exe shows our icon and version; installer size recorded and under 512 MiB |
| W6.5 updater | `crates/vsesvit-update/**`, `xtask/src/{ctx,manifest}.rs` | §6.5 changes with tests; Q16 resolved |
| W6.6 workflows | `.github/workflows/**`, `xtask/tests/release_workflow.rs` | a dispatch run of `release.yml` builds every artifact; `ci.yml` jobs of §6.3 green |

### 8.9 Wave 7: switch-over and docs

| Unit | Done when |
|---|---|
| W7.1 delete the old Linux shell | `crates/vsesvit-gtk` and `crates/vsesvit-webext` removed with their workspace lines and the gtk/adw/webkit/javascriptcore dependencies; core's dead code of §4.4 removed in the same commit; a grep for `webkit`, `gtk4`, `libadwaita` in the workspace finds nothing outside docs and history |
| W7.2 profile data migration | Q14 implemented; old WebKit data under `<root>/engine` handled as decided |
| W7.3 docs | `docs/PLAN.md` sections, `README.md` (the lines inv/packaging-ci §16–19 lists), `docs/design/packaging.md`, an ADR in `docs/adr/` recording the GPUI + CEF decision and the Windows flavour, `STATUS.md` |
| W7.4 final review | the full-diff review of ST "Next" step 4 |

### 8.10 Critical path

W0.1 → W0.2 → W1.3 → W1.4 → W1.8 → W2.1 → (wave 3) → W4.0 → W4.2 → W4.4 → W5.1 → W7.1. Wave 6 runs off the path. The riskiest single item is W4.0, because §5's Windows half rests on it; nothing before wave 4 depends on its outcome.

## 9. Open questions and risks

Each row has a proposed resolution or the spike that settles it, and the unit that must have it settled. "Owner call" marks a product, legal or preference decision no experiment can make.

| # | Question or risk | Proposed resolution or spike | Settled by |
|---|---|---|---|
| Q1 | **H.264/AAC.** Stock CEF builds leave them out (ST), so many sites' video fails | Ship stock CEF first; VP9, AV1 and Opus cover most sites. Spike: build CEF 154.0.34 with `proprietary_codecs=true ffmpeg_branding=Chrome` for both OSes (`automate-git.py`), host the archives as private release assets, and point `CEF_PATH` at them; the Flatpak source, the CI cache key and the licence text change with it. Shipping the codecs is an owner call (patent licensing) | owner, before W6 |
| Q2 | **Windows download bubble.** "Recent download history" still shows with our handler (SW §10) | First retry `set_preference("download_bubble.partial_view_enabled", false)` with a non-null error buffer: the spike's `set_preference` returned 0 with an empty error, which is G4's symptom. If that fails, look for the feature or pref that turns the bubble off in Chromium 154's source; last, accept it as a known exception | W3.3 |
| Q3 | **Display scales.** Windows ran only at 175 %; `WM_DPICHANGED` and Linux runtime scale changes untested (SW §5, SC §5.4) | Hand test (§7.4) at 100/150/200 % with SW's `explore-embed.ps1`; handle `WM_DPICHANGED` and Xft.dpi changes by calling `notify_screen_info_changed` and re-applying geometry; on Linux a scale change needs a relaunch of Chromium's side unless `notify_screen_info_changed` suffices (spike it in W2) | W2 hand test |
| Q4 | **Wayland-native.** Chrome-style children need an X11 parent; CEF #2804 is open (G12) | Stay on X11/XWayland. The seam allows a `platform/wayland` later, once CEF can parent to a Wayland surface or OSR becomes viable (`cef-osr.md`: CEF 156 with the fence fix #4314, GPUI texture surfaces). Revisit at each CEF bump | each CEF bump |
| Q5 | **Windows extension install route.** The per-user External Extensions folder is verified on Linux only; on Windows Chromium may read only the registry, and `prompt_for_external_extensions` defaults on (CE §3.3) | W4.0 tests in order: the per-user folder; `HKCU\Software\Chromium\Extensions\<id>` (and Chrome's key); `<libcef dir>/extensions/external_extensions.json`; and whether the prompt appears and can be pre-acknowledged in `Preferences`. If none is silent and persistent, fall back to CDP `Extensions.loadUnpacked` at every start with the reload caveat of EL §3 | W4.0 |
| Q6 | **Install while running.** Chromium reads the folder only at start (EL §6.2) | "Restart to finish" with a session-restoring Restart button (§5.2). Alternatives are a CEF patch or session-only `loadUnpacked`. UX is an owner call | owner, W4.3 |
| Q7 | **No back/forward restore.** CEF can serialize nothing back (inv/tab-engine §13) | Restore URL only, as WinUI does. Reopen Closed Tab and moving tabs between windows (when the reparent fails) lose history. Accept | decided |
| Q8 | **Linux password store.** A KWallet that does not answer stalls every request (SC §3.2) | Use `gnome-libsecret` when core's vault found a working Secret Service at open (it already probes it), else `basic`; never KWallet. With `basic`, Chromium's cookie encryption is a fixed key, as in Chromium without a keyring. Owner call on the default | W1.3 |
| Q9 | **Memory Saver without discard.** CEF has no discard (inv/tab-engine §12.3) | Freeze, not discard: `was_hidden(1)` plus DevTools `Page.setWebLifecycleState({state: "frozen"})` stops CPU and keeps history, but keeps memory. Optional real discard (close the browser, keep the URL) for tabs idle more than a configurable time, which loses history (Q7). Owner call between the two | W3.8 |
| Q10 | **Zoom.** No zoom-changed callback; Chromium's per-host zoom map overlaps core's `site_zoom` (inv/tab-engine §18.3) | Core stays the source of truth: apply core's level on every commit; intercept Ctrl+wheel and the zoom keys in the shell (`on_pre_key_event`, and the wheel through a document-start script reporting `ctrlKey` wheels, or by polling `get_zoom_level` 100 ms after a wheel event); spike in W2.5 which of the two catches every path | W2.5 |
| Q11 | **Audio indicator.** No playing-audio callback (inv/tab-engine §11.1) | Spike in W3.8: the helper extension's `tabs.onUpdated` (`audible`) mapped to shell tabs by URL and window, or a document-start script watching media `play`/`pause`. Without one, no speaker icon (mute still works) | W3.8 |
| Q12 | **IME.** Untested in the page on both OSes and in the omnibox on Windows (SC §7.4, SW §14) | Linux: GtkUi is loaded under the external pump, which is what IME needs (G13); test with ibus in the private session in W2, then the hand test. Windows: hand test | W2, hand test |
| Q13 | **Chrome toplevels on X11.** The status bubble and the fullscreen-exit bubble are separate Chrome windows (SC §10.3) | Status bubble: `chrome_status_bubble` off in `BrowserSettings`, our link preview instead (W2.5). Fullscreen bubble: keep Chromium's and drop our notice if it cannot be turned off (UA S:7) | W2.5 |
| Q14 | **Existing Linux profiles.** `<root>/engine` holds WebKit's data; site logins do not carry over to Chromium | Proposed: import WebKit's cookies once at first GPUI start (its `cookies.sqlite` → `CookieManager::set_cookie`), then leave the WebKit data for one release (downgrade path) and delete it in the next. Without the import, users are signed out of every site once; owner call | owner, W7.2 |
| Q15 | **The renamed bootstrap exe.** Its icon and version resources are CEF's; signing and whether `bootstrap.exe` checks the DLL's signature are unknown (SW §3.2) | `xtask stamp-exe` rewrites the resources (§6.2); a small spike in W6.4 checks that the stamped exe still loads `vsesvit.dll` and runs sandboxed. Authenticode for both files when signing exists (none today, so SmartScreen warns on a 400 MB unsigned installer: an owner call) | W6.4 |
| Q16 | **Updater at CEF size.** Verification reads the whole file into memory, twice (`updater.rs:446`); `rpm -U` resolves no new dependencies on the GTK → GPUI update | Stream the hash for verification if the signature scheme is prehashed, else verify once and keep the bytes; for rpm use `pkexec dnf install -y <file>` (or `zypper`) where present, falling back to `rpm -U` | W6.5 |
| Q17 | **Accessibility.** Two trees per window: GPUI's AccessKit and Chromium's own (AT-SPI on Linux, UIA on Windows) (`cef-windowing.md` risks; GP §2.9) | Check with Orca and Narrator in W5.3 that both are reachable and that focus moves announce; mirroring Chromium's tree into AccessKit is not planned | W5.3 |
| Q18 | **Windows Chromium opens itself** (`tabs.create`, `windows.create`, DevTools) | X11: hide and embed (EA). Windows: W4.0 tries the same with `SetParent` into an AuxHost; fallback close and reopen as a tab. DevTools stays a native window on both | W4.0 |
| Q19 | **Extension action limits.** No badge or dynamic icon, no `action.onClicked`, no `activeTab` from the button (EA §4, CE §1.3); Chromium's handling of a permission-raising update is unverified | Accept and document (EA §7.4); the only fix is a CEF patch exposing the action model, which is out of scope. W4.0 checks what re-enabling after a permission-raising update shows | W4.0 |
| Q20 | **`storage.sync`** no longer syncs between devices for GTK users who had it | Local only, as on WinUI (§5.6). A bridge through each extension's page session can come later; owner call on priority | owner |
| Q21 | **Web Store installs.** Chromium's own `webstorePrivate` flow would bypass core (CE §3.5) | Port WinUI's store shim (§5.7); W4.0 verifies the shim runs before the store's own scripts in a Chrome-style child | W4.0 |
| Q22 | **WinUI's self-test on a private desktop** is unknown; units that migrate WinUI code need it | Try it in W0.3 with `xtask deskrun`. If XAML refuses a non-input desktop, those units ask the user to run the 47 checks; never run them on the user's desktop without asking | W0.3 |
| Q23 | **Windows idle CPU.** 3.8–5.1 % of one core in debug builds, most from window activation (SW §4); release untested | Measure release builds in W1.8; if the active-window cost stays above 2 %, profile GPUI's frame scheduling with the window active and the page idle | W1.8 |
| Q24 | **GPUI churn.** gpui-pre breaks weekly (GP §1.2); our two patches must be re-applied on every bump | Bump deliberately, not continuously; `vendor-gpui` makes the re-apply mechanical; push both upstream PRs in wave 1 | W1.1, W1.2 |
| Q25 | **Drag and drop between the page and GPUI.** GPUI drags are window-local and file-only (GP §2.10); CEF's `StartDragging` has no outlet | Chromium handles drags within the page natively; links dragged from the page onto the tab strip need a spike in W2.3; until then unsupported | W2.3 |
| Q26 | **Sites that refuse embedded browsers** (Google sign-in) (P) | Keep Chrome's own user agent (no "CEF" token); if a site still refuses, a per-site CDP `Network.setUserAgentOverride` as prior art does. Test with the sync sign-in flow in W3.6 | W3.6 |
| Q27 | **Origin: null on extension service-worker fetches** (CEF #4309) breaks some password managers (CE §8) | Not fixable from handlers; track the upstream issue; list it in the release notes | upstream |
| Q28 | **The key patch cannot be carried** (it no longer applies, or upstream refuses it) | Fall back to `multi_threaded_message_loop` on Windows (SW §4): `post_task` for every CEF call, the 200 ms wait before `cef::shutdown`. Linux is unaffected (GtkUi needs the external pump) | if it happens |
