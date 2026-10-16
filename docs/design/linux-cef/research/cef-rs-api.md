# cef-rs (`cef` 154.5.0+154.0.34): API cheat-sheet for a GPUI + CEF Linux browser

Research date: 2026-10-16. This is read-only research for the `linux-cef-gpui` worktree.

**Labels**

- **VERIFIED**: I read the code or doc, or ran the command.
- **INFERRED**: my conclusion from what I read, not yet tested at runtime.

No CEF browser was launched. The snippets below **compile**. I ran `cargo check` both ways:

- with the crate's `dox` feature, which needs no CEF download;
- against the real CEF 154.0.34 distribution, downloaded to `CEF_PATH=/home/dy/.cache/vsesvit-cef`.

I also linked the real build once (`cargo build`) to check the rpath. The scratch crate is at `/tmp/vsesvit-scratch/cef-api` (`src/{main,app,client,misc,renderer,pump}.rs`). Snippets marked "excerpt" are cut down from those files.

**What I read**

- The crate on disk: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cef-154.5.0+154.0.34` and `cef-dll-sys-154.5.0+154.0.34`.
- The cef-rs repo at the commit the crate was published from. The repo is cloned to `/tmp/vsesvit-research/cef-rs-api/repo`.
- CEF branch `8037` at `14c5a08`, the exact commit of the binary. It is cloned to `/tmp/vsesvit-research/cef-rs-api/cef-src/cef-8037`.
- The CEF 154.0.34 Linux minimal distribution.
- Chromium sources at tag `154.0.8037.98`.

Related reports in this folder:

- `cef-extensions.md` and `gpt-extension-architecture.md`: extension architecture.
- `cef-osr.md`: off-screen rendering and the GLib loop.
- `cef-windowing.md`: X11 child windows.
- `prior-art.md`: other Rust/CEF embedders.

This file covers the Rust crate and how it maps onto CEF.

---

## Executive summary

### The crate

1. **The `cef` crate is a complete, low-level binding of the CEF 154 C API.** (VERIFIED)
   - It is generated mechanically.
   - It binds every non-test `include/capi/*.h` header and every `include/capi/views/*.h` header of CEF 154.0.34. `sys/wrapper.h` lists them; I compared that list with the distribution.
   - CEF is 154.0.34+g14c5a08, Chromium is 154.0.8037.98.
   - Ergonomics are C-like:
     - booleans are `i32`;
     - every parameter is `Option<&T>` or `Option<&mut T>`;
     - out-params are `Option<&mut Option<T>>`, `Option<&mut i32>` or `Option<&mut CefString>`.
   - Handlers are written with `wrap_*!` macros.
   - What CEF can do, the crate can do. It adds little safety on top.

2. **"Total extension compatibility" is not something this crate provides or blocks.** (VERIFIED, plus INFERRED for design)
   - CEF 154 has **no extension API at all**. There is no `cef_extension*.h`. The old `CefExtension`/`LoadExtension` API was removed, per cef-rs maintainers in #235 and #138.
   - Extensions come entirely from Chromium's Chrome layer, through the CEF **Chrome runtime style**.
   - What the embedder controls through this crate:
     - which browsers are Chrome style versus Alloy style;
     - one `RequestContext` per profile; each one is a Chromium profile with its own extension service;
     - not overriding Chrome UI that extensions need (permission prompts, downloads, popups);
     - returning a client from `BrowserProcessHandler::default_client()`, so that windows Chrome creates itself (extension popups, DevTools, "open in new window") stay managed.
   - The architecture choice is covered in `cef-extensions.md`, `cef-windowing.md` and `gpt-extension-architecture.md`.

3. **Off-screen rendering is always Alloy style.** (VERIFIED, `cef_types_runtime.h`, `cef_types_linux.h`)
   - OSR is how GPUI can composite page content on native Wayland.
   - The crate's `WindowInfo::set_as_windowless` also sets `RuntimeStyle::ALLOY`.
   - Chrome style windowed browsers can be embedded as **X11 child windows only**, via `WindowInfo::set_as_child(xid, rect)`.
   - Embedding Ozone/Wayland windows is CEF issue #2804, still open.

### Processes and the message loop

4. **Process model.** (VERIFIED, plus INFERRED)
   - On Linux, CEF re-executes `/proc/self/exe` with `--type=...` for zygote, renderer, GPU and utility processes.
   - `main()` must therefore:
     - call `api_hash(sys::CEF_API_VERSION_LAST, 0)`;
     - then call `execute_process(...)`;
     - and only then start GPUI or any thread.
   - A separate small helper executable (`Settings.browser_subprocess_path`) keeps GPUI and its dependencies out of every subprocess. It also keeps any background threads out of the zygote; Chromium's sandbox needs a single thread at init. That is safer, at the cost of putting the renderer-side Rust (V8 bindings, `RenderProcessHandler`) into the helper.

5. **The message loop needs a deliberate choice.** (VERIFIED)
   - With `external_message_pump`, CEF's UI pump on Linux subclasses Chromium's `MessagePumpGlib`. Chromium's fd watches (X11/Wayland/D-Bus) sit on `g_main_context_default()`.
   - So the host must also iterate the default GLib context. GPUI's calloop loop does not. CEF's own Linux external-pump sample does exactly this.
   - `multi_threaded_message_loop` gives CEF its own GLib context on its own thread. But it disables GtkUi: no GTK IME, file chooser, print dialog or theme.
   - Both are workable. Section 2.4 has the trade-offs, and `cef-windowing.md` argues for the external pump.

### Build, layout and size

6. **Build.** (VERIFIED, by reading `sys/build.rs` and by an empty `OUT_DIR` after a real build)
   - On Linux, **`cmake` is not needed**. `libcef_dll_wrapper` is built only on Windows and macOS. On Linux the crate links `libcef.so` directly through the C API.
   - The build script downloads the 326 MB minimal archive.
     - With `CEF_PATH` set, it caches the archive under `$CEF_PATH/154.0.34/cef_linux_x86_64`.
     - It **copies about 1.5 GB of runtime files into `target/<profile>/` on every build-script run**.
     - It emits **no rpath**. You must add `-Wl,-rpath,$ORIGIN` yourself.

7. **Runtime layout.** (VERIFIED in CEF source)
   - CEF on Linux resolves its assets relative to **the directory containing `libcef.so`**: it overrides `DIR_ASSETS` via `dladdr`.
   - That covers the `*.pak` files, `icudtl.dat`, `v8_context_snapshot.bin`, `chrome-sandbox`, `locales/` and SwiftShader.
   - The executable may live elsewhere if its RUNPATH finds `libcef.so`.
   - Shipped `libcef.so` is **1.36 GiB unstripped**: it carries DWARF. Stripped it is **272 MB**; with xz it is about 79 MB.
   - Other required files are about 12 MB. The other `.pak` files are about 25 MB, SwiftShader is about 16 MB, and locales are 49 MB for all 220 locales.

8. **Sandbox.** (VERIFIED in Chromium 154 source)
   - Chromium 154 prefers the **user-namespace sandbox**. It falls back to a setuid `chrome-sandbox` (root, mode 4755) only if user namespaces are unavailable.
   - The crate's `sandbox` feature does nothing on Linux.
   - On this Fedora 44 host, unprivileged user namespaces are available (`max_user_namespaces` = 254127), so no setuid helper is needed.

### Where the crate bites

9. **Main hazards in the crate itself.** (VERIFIED by reading the code, unless marked)
   - **Strings.**
     - `CefString::clone()` is a shallow, non-owning copy.
     - A `CefString` assigned into a struct out-param is written back as an **empty string**. This explains cef-rs #409.
     - `CefString::default()` passed as an out-param becomes NULL, so CEF silently returns 0. `set_preference(..., Some(&mut CefString::default()))` always fails.
     - Cloning a `CefStringList` that CEF lent you loses the pointer.
   - **Macros.**
     - `wrap_*!` expansions use unqualified names (`Client::new`, `ImplClient`, `WrapApp`...). They therefore clash with GPUI's `App`, `Window`, `Task` and others if both are glob-imported. (Clash: INFERRED for the GPUI names.)
     - Fields must be `Clone`, with no lifetimes (#450) and no attributes.
     - Calling `self.clone()` on the inner struct leaks a reference.
   - **No thread safety.** There are no `Send`/`Sync` bounds anywhere, yet CEF calls handlers on the UI, IO and renderer threads.
   - **API version pinning.**
     - The crate pins the API to `CEF_API_VERSION_LAST` = 15400 at run time.
     - The bindings are generated from the experimental headers. So the only experimental member in 154 (`BrowserHost::set_ax_viewport_collapse`, `BrowserSettings.ax_viewport_collapse`) must not be used.

10. **IPC is ordinary CEF.**
    - Data at browser creation: `extra_info` → `RenderProcessHandler::on_browser_created`.
    - At run time: `Frame::send_process_message` with `ProcessMessage` + `ListValue`.
    - JavaScript functions: V8 functions via `wrap_v8_handler!`, plus promises.
    - `cef::wrapper::message_router` is a Rust port of `CefMessageRouter` (`cefQuery`).
    - `Frame::execute_java_script` is fire-and-forget. For results, use the DevTools protocol (`execute_dev_tools_method("Runtime.evaluate")`) or an IPC round trip.

11. **Profiles map to request contexts.**
    - Use one `request_context_create_context` per Vsesvit profile.
    - Its `cache_path` must equal `Settings.root_cache_path` or sit below it. Otherwise CEF logs an error and **silently falls back to in-memory storage**.
    - Incognito is `RequestContextSettings::default()`, meaning an empty `cache_path`.
    - Chrome prefs are set with `set_preference` on the UI thread.
    - Each context has its own cookie manager.

---

## 0. What the crate is (versions, layout, features)

### Versions (VERIFIED)

| Item | Value |
|---|---|
| Crate | `cef` **154.5.0+154.0.34**, published 2026-10-07; still the latest on crates.io on 2026-10-16. The `+154.0.34` suffix is the CEF version. `cef-dll-sys` has the same version. |
| Source commit | Published from tauri-apps/cef-rs `2861f6991fae3ee31f877891382adcbc9b9f0953` (`.cargo_vcs_info.json`). Repo HEAD `163744c` is only two dependency bumps later. |
| CEF | `154.0.34+g14c5a08+chromium-154.0.8037.98`, from `CEF_VERSION` in the sys bindings. CEF git `14c5a089e845…`, branch `8037`. |
| API versions | `CEF_API_VERSION_LAST = 15400`. `CEF_API_VERSION_MIN = 13300`. |
| Newer CEF on CDN | The CDN index (2026-10-16) shows 156.0.x beta. No newer 154 stable than .34. |

### Modules (VERIFIED)

From `src/lib.rs`:

| Module | Contents |
|---|---|
| `bindings` | Re-exported at the crate root: all types, the `Impl*`/`Wrap*` traits, the `wrap_*!` macros, and free functions such as `initialize`, `post_task`, `browser_host_create_browser`. |
| `args` | `Args`: argc/argv as `MainArgs`, plus `CommandLine`. |
| `rc` | `RefGuard`, `RcImpl`, the `Rc` trait. |
| `string` | `CefString` = `CefStringUtf16` on Linux, `CefStringList`, `CefStringMap`, `CefStringMultimap`. |
| `window_info` | `set_as_child`, `set_as_windowless`. |
| `wrapper` | `message_router`, `resource_manager`, `stream_resource_handler`, `zip_archive`, `byte_read_handler`, `browser_info_map`. |
| `osr_texture_import` | Only with `accelerated_osr`. |
| `build_util` | Only with `build-util`. |
| `resources` | `IDC_*`, `IDR_*`, `IDS_*` names as `&CStr`, for `sys::cef_id_for_*_name`. |
| `sys` | The raw `cef-dll-sys`. |
| `sandbox`, `library_loader`, `application_mac` | **macOS only.** |

### Features (VERIFIED)

From `Cargo.toml.orig` and `sys/build.rs`:

| Feature | Effect on Linux |
|---|---|
| `default = ["sandbox", "build-util", "resources"]` | |
| `sandbox` | Forwards to `cef-dll-sys/sandbox`. That only sets the CMake `USE_SANDBOX` define on Windows and macOS. **No effect on Linux.** |
| `build-util` | Pulls in `clap`, `cargo_metadata`, `serde`... and builds the `bundle-cef-app` binary. Not needed at run time. |
| `resources` | Version-safe resource and command name constants. |
| `accelerated_osr` | Pulls `wgpu` 30 + `ash` and adds `osr_texture_import::dmabuf`, a DMA-BUF → Vulkan → wgpu import. Ties you to wgpu 30. |
| `linux-x11` | **Empty.** It only makes `bundle-cef-app` pass `-F linux-x11` to `cargo build` of *your* crate, so your crate must define a feature with that name. In the cefsimple example it enables `x11-dl` to set titles of Alloy-native windows. |
| `dox` | Turns `cef-dll-sys`'s build script into a no-op: no download, no link. Used by docs.rs. **Good for CI `cargo check`/`clippy` without the 326 MB download:** `cef = { version = "=154.5.0", default-features = false, features = ["dox"] }`. Linking or running needs the real build. |

### How the bindings are generated (VERIFIED)

- The `update-bindings` tool runs `bindgen` over `sys/wrapper.h` with **no `CEF_API_VERSION` define**. The headers therefore select the *experimental* API.
- The crate's examples, and `sys/build.rs`'s comment, configure the runtime with `cef_api_hash(CEF_API_VERSION_LAST)` (15400).
- In 154.0.34 the only difference between the two is one experimental pair:
  - the `cef_browser_host_t::set_ax_viewport_collapse` method;
  - the `cef_browser_settings_t::ax_viewport_collapse` field.
- That is the only `CEF_API_ADDED(CEF_EXPERIMENTAL)` in `include/capi/*.h`. The struct field is also guarded in `cef_types.h`.

---

## 1. Implementing CEF handler interfaces in Rust

### 1.1 The `wrap_*!` pattern (VERIFIED)

For every client-implemented CEF class `X`, the bindings generate:

- `pub struct X(RefGuard<_cef_x_t>)`: a ref-counted, `Clone` handle that CEF understands.
- `pub trait ImplX: Clone + Sized + Rc { ...methods with default bodies...; fn get_raw(&self) -> *mut _cef_x_t; }`
- `pub trait WrapX: ImplX { fn wrap_rc(&mut self, object: *mut RcImpl<_cef_x_t, Self>); }`
- `macro_rules! wrap_x` (`#[macro_export]`). It:
  - declares your struct with an extra hidden `cef_object: *mut RcImpl<…>` field;
  - implements `WrapX`, `Clone` and `Rc` for it;
  - implements `ImplX`, overriding only the methods you write;
  - generates `pub fn new(field1, field2, …) -> X`.

The 64 macros available in 154.5 are:

- `wrap_app`, `wrap_browser_process_handler`, `wrap_render_process_handler`, `wrap_client`;
- `wrap_life_span_handler`, `wrap_load_handler`, `wrap_display_handler`, `wrap_request_handler`, `wrap_resource_request_handler`, `wrap_cookie_access_filter`, `wrap_response_filter`, `wrap_resource_handler`, `wrap_scheme_handler_factory`;
- `wrap_download_handler`, `wrap_permission_handler`, `wrap_context_menu_handler`, `wrap_keyboard_handler`, `wrap_jsdialog_handler`, `wrap_dialog_handler`, `wrap_focus_handler`, `wrap_render_handler`, `wrap_accessibility_handler`, `wrap_audio_handler`, `wrap_command_handler`, `wrap_drag_handler`, `wrap_find_handler`, `wrap_frame_handler`, `wrap_print_handler`, `wrap_request_context_handler`;
- `wrap_task`, `wrap_v8_handler`, `wrap_v8_accessor`, `wrap_v8_interceptor`, `wrap_v8_array_buffer_release_callback`;
- `wrap_cookie_visitor`, `wrap_set_cookie_callback`, `wrap_delete_cookies_callback`, `wrap_completion_callback`, `wrap_resolve_callback`;
- `wrap_dev_tools_message_observer`, `wrap_preference_observer`, `wrap_setting_observer`;
- `wrap_string_visitor`, `wrap_domvisitor`, `wrap_navigation_entry_visitor`, `wrap_run_file_dialog_callback`, `wrap_pdf_print_callback`, `wrap_download_image_callback`;
- `wrap_media_observer`, `wrap_media_route_create_callback`, `wrap_media_sink_device_info_callback`;
- `wrap_read_handler`, `wrap_write_handler`, `wrap_menu_model_delegate`, `wrap_resource_bundle_handler`, `wrap_component_update_callback`, `wrap_server_handler`, `wrap_end_tracing_callback`, `wrap_urlrequest_client`;
- views delegates: `wrap_view_delegate`, `wrap_browser_view_delegate`, `wrap_button_delegate`, `wrap_menu_button_delegate`, `wrap_panel_delegate`, `wrap_textfield_delegate`, `wrap_window_delegate`.

```rust
use cef::*;                       // required: the macro body names `Client`, `ImplClient`, `WrapClient` unqualified
use std::sync::{Arc, Mutex};

wrap_client! {
    pub struct VsesvitClient {    // fields: any `Clone` type; no lifetimes, no attributes
        shared: Arc<Shared>,
    }

    impl Client {                 // override only what you need; the signatures must match ImplClient exactly
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(VsesvitLifeSpan::new(self.shared.clone()))
        }
        fn on_process_message_received(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> i32 { /* ... */ 0 }
    }
}

let client: Client = VsesvitClient::new(shared);   // `new` returns the CEF wrapper type, not your struct
```

**Rules and limits** (VERIFIED from the macro text, unless marked)

- **Fields.** Every field must implement `Clone`: the generated `Clone` impl clones each field.
  - Put non-`Clone` state (`Mutex`, channels you don't want cloned...) behind an `Arc`.
- **No lifetimes or references.** `struct Foo<'a>` is rejected: "no rules expected `'a`" (cef-rs #450). Generics of the form `<T: Bound>` are accepted.
- **No attributes on the struct.** The pattern starts at `$vis struct $name`. A doc comment placed before the macro call gives the warning "unused doc comment". Attributes *are* allowed on methods (`#[cfg(...)]` is used in `examples/osr`).
- **Unit structs.** `wrap_app! { pub struct SimpleApp; impl App { ... } }` works (the first macro arm).
- **Unqualified names.** The expansion names `X::new`, `ImplX` and `WrapX` unqualified, plus the types in your signatures. Invoke the macros in a module that has `use cef::*` (or explicit imports) and **does not also glob-import `gpui::*`**. GPUI exports `App`, `Window`, `Task`, `Point`, `Size`, `Image`... (the GPUI names: INFERRED). Keep CEF glue in its own module.
- **Class hierarchies.** These need one `impl` block per level, in order. For example `wrap_window_delegate!` needs `impl ViewDelegate {}` `impl PanelDelegate {}` `impl WindowDelegate { ... }`; wrong order or a missing block is a cryptic macro error (cef-rs #297).
- **Tooling.** rustfmt and rust-analyzer completion do not work inside the macros (#297). A maintainer's workaround: temporarily rename the macro call to a `mod` block, run rustfmt, then restore it.
- **`self.clone()` leaks.** Inside a handler method, `self` is `&YourStruct`, the inner value stored in the `RcImpl`. Its generated `Clone` calls `add_ref()` on the CEF object, and **there is no `Drop` impl that releases it**. A clone of the inner struct therefore leaks a reference, which keeps your handler and its fields alive forever. Clone *fields* (`self.shared.clone()`) instead.
- **No downcast.** You cannot get back from a `Client` returned by `browser.host().client()` to your Rust struct. Key your state by `browser.identifier()` (`i32`) instead.
- **`Default` on wrapper types is a trap.** `cef::Client::default()` (and the like) are not usable: they panic when passed to CEF (#225). Always construct through your `wrap_*` type's `new`.

### 1.2 How callback signatures map (VERIFIED, from the generated trampolines)

| C | Rust parameter | Notes |
|---|---|---|
| `cef_browser_t* browser` | `Option<&mut Browser>` | Borrowed for the call. `browser.cloned()` (or `b.clone()`) adds a reference you can keep. |
| `const cef_string_t* s` | `Option<&CefString>` | Points into CEF memory. Convert with `.to_string()` **inside** the callback. |
| `cef_string_t* out` (string out-param) | `Option<&mut CefString>` | A borrowed-mutable view. Write with `out.try_set("text")`. Assigning `*out = CefString::from(..)` is **not written back**. |
| `int* out` | `Option<&mut i32>` | `*out = 1`. |
| `cef_x_t** out` (object out-param) | `Option<&mut Option<X>>` | `*out = Some(obj)` transfers one reference to CEF. Covered by the crate's test `dictionary_value_out_param`; fixed in #224 for V8 `retval`. |
| `cef_struct_t* out` (struct out-param) | `Option<&mut StructType>` through `WrapParamRef` | The struct is converted to Rust on entry and back on drop. **String fields need special care (gotcha G3).** |
| arrays `size_t n, cef_x_t* const* v` | `Option<&[Option<X>]>` | |
| `void* buffer` / `int len` | `*const u8` / `*mut u8` + `i32` | E.g. `RenderHandler::on_paint`, `ResourceHandler::read`. |
| return `int` (bool) | `i32` | `1` = true. |
| return object | `Option<X>` | `None` → NULL. |

### 1.3 Reference counting (VERIFIED, `src/rc.rs` and generated code)

- **Wrapper types own references.** `Browser`, `Frame`, `Client`... are `RefGuard<T>`.
  - `Clone` = `add_ref`, `Drop` = `release`.
  - `RefGuard` is `unsafe impl Send + Sync` for **all** CEF types (gotcha G6).
- **Your handler objects** are allocated as `Box<RcImpl<cef_struct, YourStruct>>` with an atomic count starting at 1. When the count reaches 0, the box (and your fields) is dropped.
  - `RcImpl::new` sets `base.size = size_of::<cef struct>()`. libcef checks this size exactly against its own struct for the configured API version, and aborts with LOG(FATAL) on a mismatch (`libcef_dll/ctocpp/ctocpp_ref_counted.h`).
- **The generated glue follows the CEF C-API rules** (`docs/using_the_capi.md`):
  - Parameters CEF passes to your callback were add-ref'd by CEF. The trampoline wraps them with `RefGuard::from_raw` (rule 4: release when done).
  - Objects you pass to CEF as non-self arguments get `add_ref()` first (rule 2).
  - Objects you return are `mem::forget`-ed into a raw pointer (rule 3).
- **What this means for you:** never call `add_ref`/`release` yourself.
  - `clone()` a handle to keep it beyond the callback.
  - Drop it in `on_before_close` (for browsers) or `on_context_released` (for V8 objects).

### 1.4 Thread affinity (VERIFIED, from the class and method docs in `cef-dll-sys` bindings, which are the CEF 154 headers)

| Interface | Called on |
|---|---|
| `App::on_before_command_line_processing`, `on_register_custom_schemes` | The main thread of **every** process. Schemes must be identical in all processes. |
| `BrowserProcessHandler` | Browser main (UI) thread, except as below. |
| `BrowserProcessHandler::on_schedule_message_pump_work` | **Any thread.** |
| `BrowserProcessHandler::on_before_child_process_launch` | UI thread for render processes; **IO thread for GPU processes**. |
| `RenderProcessHandler` | Render process main thread (`TID_RENDERER`). |
| `LifeSpanHandler`, `DisplayHandler`, `FocusHandler`, `KeyboardHandler`, `JsdialogHandler`, `ContextMenuHandler`, `CommandHandler`, `RenderHandler`, `PermissionHandler`, `DownloadHandler`, `DialogHandler` | Browser process UI thread. |
| `LoadHandler` | Browser UI thread, **or** `TID_RENDERER` if returned from `RenderProcessHandler::load_handler`. |
| `RequestHandler` | "The thread indicated". `on_before_browse`, `on_open_urlfrom_tab`, `on_certificate_error` and `on_render_process_terminated` are UI. `get_resource_request_handler` and `get_auth_credentials` are **IO**. |
| `ResourceRequestHandler`, `CookieAccessFilter`, `ResponseFilter`, `ResourceHandler`, `SchemeHandlerFactory` | **IO thread**. `ResourceHandler` methods are called "in sequence but not from a dedicated thread". |
| `CookieVisitor` | "Always … the UI thread". |
| `DevToolsMessageObserver` | Browser UI thread. |
| `V8Handler` | The thread that owns the V8 context: `TID_RENDERER` or a WebWorker thread. |
| `Task` | The target thread of `post_task`. If posting fails it may be destroyed on the source thread. |
| `Client::*_handler()` getters | Whatever thread needs the handler. libcef calls `client->GetRequestHandler()` from the IO-thread request interceptor (`libcef/browser/net_service/resource_request_handler_wrapper.cc`). So **`Client` and everything it reaches must be `Sync`**. |

Library-side objects (VERIFIED):

- `BrowserHost` methods may be called from any browser-process thread unless noted. Several must be on the UI thread:
  - `try_close_browser`;
  - `execute_dev_tools_method` and `send_dev_tools_message`, which return 0 off the UI thread;
  - `browser_host_create_browser_sync`;
  - `RequestContext::set_preference`.
- `Frame` methods may be called from any thread in the browser process, and only from the main thread in the renderer.
- `ProcessMessage` works in any process and thread.
- V8 handles can only be used on the thread that created them.

### 1.5 Holding Rust state and reaching back to the UI

INFERRED design advice, built on the VERIFIED threading above.

- **Shared state.**
  - Keep one `Arc<Shared>` (Send + Sync: `Mutex`/`RwLock`/atomics/channels).
  - Pass it into each handler's `new(...)`.
  - Key per-tab state by `browser.identifier()`.
  - The crate enforces none of this: a handler with `Rc<RefCell<…>>` compiles and then races when CEF calls it on the IO thread.
- **Build handlers once.** libcef calls `client->GetDisplayHandler()` (and the like) **on every event**: 14 call sites for the display handler alone in `libcef/browser`. Store the handler objects as `Client` fields and return `self.x.clone()`, which is a cheap refcount bump. This is the `CachedClient` excerpt in §1.6 and the pattern in `examples/osr`.
- **Never call GPUI from a CEF callback.** With the external pump, CEF UI callbacks run *inside* `do_message_loop_work()` on the GPUI main thread. That call may itself happen inside a GPUI update, and GPUI's app state is a `RefCell`, so re-entering it would panic. With the multi-threaded loop, callbacks run on CEF's UI thread, where GPUI can't be touched at all. Either way:
  - push a message into a channel;
  - wake GPUI's foreground executor;
  - drain the channel in a GPUI task.
- **Don't hold locks across CEF calls.** CEF may call back synchronously (e.g. `browser_host_create_browser_sync` → `on_after_created`).
- **Calling into CEF from GPUI.**
  - With the multi-threaded loop, wrap UI-thread-only calls in `post_task(ThreadId::UI, …)` (the `post_on` helper in §1.7).
  - Thread-agnostic `BrowserHost` input methods (`send_mouse_*`, `send_key_event`, `was_resized`...) can be called directly.

### 1.6 Handler snippets (excerpts of compiled code, `/tmp/vsesvit-scratch/cef-api/src/{app,client}.rs`)

**App and BrowserProcessHandler**

The App must also be passed to `execute_process` so that subprocesses get the schemes and the render process handler.

```rust
fn scheme_bits(opts: &[SchemeOptions]) -> i32 {
    opts.iter().fold(0u32, |acc, o| acc | o.get_raw()) as i32
}

wrap_app! {
    pub struct VsesvitApp { tx: std::sync::mpsc::Sender<UiMsg> }

    impl App {
        fn on_before_command_line_processing(&self, process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
            let Some(cl) = command_line else { return };
            // Docs: process_type is NULL/empty for the browser process.
            let is_browser = process_type.map(|p| p.to_string().is_empty()).unwrap_or(true);
            if is_browser {
                cl.append_switch_with_value(Some(&CefString::from("ozone-platform")), Some(&CefString::from("x11")));
            }
        }
        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            let Some(r) = registrar else { return };
            r.add_custom_scheme(Some(&CefString::from("vsesvit")),
                scheme_bits(&[SchemeOptions::STANDARD, SchemeOptions::SECURE, SchemeOptions::CORS_ENABLED, SchemeOptions::FETCH_ENABLED]));
        }
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(VsesvitBrowserProcessHandler::new(self.tx.clone()))
        }
        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(crate::renderer::VsesvitRenderProcessHandler::new_default())
        }
    }
}

wrap_browser_process_handler! {
    pub struct VsesvitBrowserProcessHandler { tx: std::sync::mpsc::Sender<UiMsg> }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) { let _ = self.tx.send(UiMsg::ContextReady); }     // UI thread; create browsers/contexts now
        fn on_schedule_message_pump_work(&self, delay_ms: i64) { let _ = self.tx.send(UiMsg::PumpWork(delay_ms)); } // ANY thread
        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            if let Some(cl) = command_line {
                cl.append_switch_with_value(Some(&CefString::from("vsesvit-profile")), Some(&CefString::from("Default")));
            }
        }
        fn on_already_running_app_relaunch(&self, _cl: Option<&mut CommandLine>, _cwd: Option<&CefString>) -> i32 {
            1 // handled: open a window in the running instance (same root_cache_path)
        }
    }
}
```

`BrowserProcessHandler::default_client()` is "currently only used with Chrome style when creating new browser windows via Chrome UI". If it returns `None`, those browsers are unmanaged and "application shutdown will be blocked until the browser window is closed manually" (VERIFIED doc). Implement it whenever Chrome-style windows exist: extension popups, DevTools, "open link in new window".

**LifeSpanHandler**

```rust
wrap_life_span_handler! {
    pub struct VsesvitLifeSpan { shared: Arc<Shared> }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser { self.shared.browsers.lock().unwrap().insert(b.identifier(), b.clone()); }
        }
        fn on_before_popup(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _popup_id: i32,
            target_url: Option<&CefString>, _target_frame_name: Option<&CefString>, _target_disposition: WindowOpenDisposition,
            _user_gesture: i32, _popup_features: Option<&PopupFeatures>, window_info: Option<&mut WindowInfo>,
            client: Option<&mut Option<Client>>, _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>, _no_javascript_access: Option<&mut i32>) -> i32 {
            let _url = target_url.map(|u| u.to_string()).unwrap_or_default();
            if let (Some(wi), Some(c)) = (window_info, client) {
                *wi = WindowInfo::default().set_as_windowless(0);       // keep the popup OSR and managed by us
                *c = Some(VsesvitClient::new(self.shared.clone()));
            }
            0 // 1 = cancel the popup (e.g. open it as a GPUI tab instead)
        }
        fn do_close(&self, _browser: Option<&mut Browser>) -> i32 { 0 }   // Alloy style only (VERIFIED doc)
        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser { self.shared.browsers.lock().unwrap().remove(&b.identifier()); }
        }
    }
}
```

**Display, load, request and resource-request handlers**

```rust
wrap_display_handler! {
    pub struct VsesvitDisplay { shared: Arc<Shared> }
    impl DisplayHandler {
        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            let id = browser.as_ref().map(|b| b.identifier()).unwrap_or(-1);
            self.shared.send(UiMsg::Title { browser_id: id, title: title.map(|t| t.to_string()).unwrap_or_default() });
        }
        fn on_address_change(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if frame.map(|f| f.is_main() != 0).unwrap_or(false) { /* send UiMsg::Address */ }
        }
        fn on_cursor_change(&self, _b: Option<&mut Browser>, _cursor: std::os::raw::c_ulong, _type_: CursorType,
                            _info: Option<&CursorInfo>) -> i32 { 1 } // OSR: map CursorType -> GPUI cursor
    }
}

wrap_load_handler! {
    pub struct VsesvitLoad { shared: Arc<Shared> }
    impl LoadHandler {
        fn on_load_error(&self, _b: Option<&mut Browser>, _f: Option<&mut Frame>, error_code: Errorcode,
                         _text: Option<&CefString>, _url: Option<&CefString>) {
            if sys::cef_errorcode_t::from(error_code) == sys::cef_errorcode_t::ERR_ABORTED { return; }
        }
    }
}

wrap_request_handler! {
    pub struct VsesvitRequest { shared: Arc<Shared> }
    impl RequestHandler {
        fn on_before_browse(&self, _b: Option<&mut Browser>, _f: Option<&mut Frame>, request: Option<&mut Request>,
                            _user_gesture: i32, _is_redirect: i32) -> i32 {
            let _url = request.map(|r| CefString::from(&r.url()).to_string());
            0 // 0 = allow
        }
        // IO thread:
        fn resource_request_handler(&self, _b: Option<&mut Browser>, _f: Option<&mut Frame>, _r: Option<&mut Request>,
            _is_navigation: i32, _is_download: i32, _initiator: Option<&CefString>, _disable_default: Option<&mut i32>)
            -> Option<ResourceRequestHandler> {
            Some(VsesvitResourceRequest::new(self.shared.clone()))
        }
    }
}

wrap_resource_request_handler! {
    pub struct VsesvitResourceRequest { shared: Arc<Shared> }
    impl ResourceRequestHandler {
        fn on_before_resource_load(&self, _b: Option<&mut Browser>, _f: Option<&mut Frame>, request: Option<&mut Request>,
                                   _cb: Option<&mut Callback>) -> ReturnValue {
            if let Some(r) = request { r.set_header_by_name(Some(&CefString::from("DNT")), Some(&CefString::from("1")), 1); }
            ReturnValue::CONTINUE
        }
    }
}
```

**Download, permission, context menu, keyboard, JS dialog, file dialog and focus handlers**

```rust
wrap_download_handler! {
    pub struct VsesvitDownload { shared: Arc<Shared> }
    impl DownloadHandler {
        fn on_before_download(&self, _b: Option<&mut Browser>, _item: Option<&mut DownloadItem>,
                              suggested_name: Option<&CefString>, callback: Option<&mut BeforeDownloadCallback>) -> i32 {
            let name = suggested_name.map(|n| n.to_string()).unwrap_or_default();
            if let Some(cb) = callback { cb.cont(Some(&CefString::from(format!("/home/user/Downloads/{name}").as_str())), 0); }
            1 // 0 = default: cancel (Alloy) / Chrome download UI (Chrome style)  [VERIFIED doc]
        }
    }
}

wrap_permission_handler! {
    pub struct VsesvitPermission { shared: Arc<Shared> }
    impl PermissionHandler {
        fn on_show_permission_prompt(&self, _b: Option<&mut Browser>, prompt_id: u64, _origin: Option<&CefString>,
                                     _perms: u32, callback: Option<&mut PermissionPromptCallback>) -> i32 {
            // Keep a clone, answer later from GPUI: cb.cont(PermissionRequestResult::ACCEPT)
            if let Some(cb) = callback { self.shared.pending_prompts.lock().unwrap().insert(prompt_id, cb.clone()); }
            1 // 0 = default: Chrome style shows its prompt; Alloy style = IGNORE  [VERIFIED doc]
        }
    }
}

wrap_context_menu_handler! {
    pub struct VsesvitContextMenu { shared: Arc<Shared> }
    impl ContextMenuHandler {
        fn on_before_context_menu(&self, _b: Option<&mut Browser>, _f: Option<&mut Frame>,
                                  _p: Option<&mut ContextMenuParams>, model: Option<&mut MenuModel>) {
            if let Some(m) = model {
                // Version-safe command id (raw sys::IDC_* numbers change between CEF builds)
                let id = unsafe { sys::cef_id_for_command_id_name(cef::resources::IDC_VIEW_SOURCE.as_ptr()) };
                if id >= 0 { m.add_item(id, Some(&CefString::from("View source"))); }
            }
        }
        // Return 1 and call callback.cont(command_id, flags) later to show a GPUI menu.
        fn run_context_menu(&self, _b: Option<&mut Browser>, _f: Option<&mut Frame>, params: Option<&mut ContextMenuParams>,
                            model: Option<&mut MenuModel>, callback: Option<&mut RunContextMenuCallback>) -> i32 {
            let (Some(p), Some(m), Some(cb)) = (params, model, callback) else { return 0 };
            let _items: Vec<(i32, String)> = (0..m.count())
                .map(|i| (m.command_id_at(i), CefString::from(&m.label_at(i)).to_string())).collect();
            let _at = (p.xcoord(), p.ycoord());
            cb.cancel(); // (keep cb.clone() and resolve asynchronously instead)
            1
        }
    }
}

wrap_keyboard_handler! {
    pub struct VsesvitKeyboard { shared: Arc<Shared> }
    impl KeyboardHandler {
        fn on_pre_key_event(&self, _b: Option<&mut Browser>, event: Option<&KeyEvent>,
                            _os_event: Option<&mut sys::XEvent>,          // NB: XEvent lives in `sys`; NULL with OSR
                            is_keyboard_shortcut: Option<&mut i32>) -> i32 {
            let Some(e) = event else { return 0 };
            if e.windows_key_code == 'L' as i32 && (e.modifiers & sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0) != 0 {
                if let Some(k) = is_keyboard_shortcut { *k = 1; }
            }
            0
        }
    }
}

wrap_jsdialog_handler! {
    pub struct VsesvitJsDialog { shared: Arc<Shared> }
    impl JsdialogHandler {
        fn on_jsdialog(&self, _b: Option<&mut Browser>, _origin: Option<&CefString>, _t: JsdialogType,
                       _msg: Option<&CefString>, _default: Option<&CefString>, callback: Option<&mut JsdialogCallback>,
                       _suppress: Option<&mut i32>) -> i32 {
            if let Some(cb) = callback { cb.cont(1, None); }   // custom dialog: cont(success, user_input)
            1
        }
    }
}

wrap_dialog_handler! {
    pub struct VsesvitDialog { shared: Arc<Shared> }
    impl DialogHandler {
        fn on_file_dialog(&self, _b: Option<&mut Browser>, _mode: FileDialogMode, _title: Option<&CefString>,
                          _default_path: Option<&CefString>, _filters: Option<&mut CefStringList>,
                          _exts: Option<&mut CefStringList>, _descs: Option<&mut CefStringList>,
                          callback: Option<&mut FileDialogCallback>) -> i32 {
            if let Some(cb) = callback {
                let mut paths = CefStringList::new();
                paths.append("/home/user/file.txt");
                cb.cont(Some(&mut paths));
            }
            1
        }
    }
}

wrap_focus_handler! {
    pub struct VsesvitFocus { shared: Arc<Shared> }
    impl FocusHandler {
        fn on_set_focus(&self, _b: Option<&mut Browser>, _source: FocusSource) -> i32 { 0 } // 0 = allow
    }
}
```

**RenderHandler (OSR)**

`on_paint` arrives on the UI thread as BGRA, `width*height*4` bytes, top-left origin.

```rust
wrap_render_handler! {
    pub struct VsesvitRender { shared: Arc<Shared> }
    impl RenderHandler {
        fn view_rect(&self, _b: Option<&mut Browser>, rect: Option<&mut Rect>) {      // DIP, must be non-empty
            if let Some(r) = rect {
                let (w, h) = *self.shared.view_size.lock().unwrap();
                *r = Rect { x: 0, y: 0, width: w.max(1), height: h.max(1) };
            }
        }
        fn screen_info(&self, _b: Option<&mut Browser>, info: Option<&mut ScreenInfo>) -> i32 {
            let Some(i) = info else { return 0 };
            i.device_scale_factor = *self.shared.scale.lock().unwrap();
            1
        }
        fn on_paint(&self, _b: Option<&mut Browser>, type_: PaintElementType, _dirty: Option<&[Rect]>,
                    buffer: *const u8, width: i32, height: i32) {
            if type_ != PaintElementType::VIEW || buffer.is_null() { return; }
            let bytes = unsafe { std::slice::from_raw_parts(buffer, (width * height * 4) as usize) }.to_vec();
            *self.shared.last_frame.lock().unwrap() = Some((bytes, width, height));
        }
    }
}
```

**Creating browsers and feeding input**

```rust
// OSR (always Alloy style). Requires Settings.windowless_rendering_enabled = 1. UI thread for the _sync variant.
let window_info = WindowInfo::default().set_as_windowless(0 /* parent XID or 0 */);
let settings = BrowserSettings { windowless_frame_rate: 60, background_color: 0xFFFF_FFFF, ..Default::default() };
let mut client = VsesvitClient::new(shared);
let browser = browser_host_create_browser_sync(Some(&window_info), Some(&mut client),
    Some(&CefString::from("https://example.org")), Some(&settings), None, ctx.as_mut() /* Option<&mut RequestContext> */);

// Windowed X11 child (Chrome or Alloy style); asynchronous, callable from any browser-process thread.
let wi = WindowInfo { runtime_style: RuntimeStyle::CHROME, ..Default::default() }
    .set_as_child(parent_xid as _, &Rect { x: 0, y: 40, width: 1200, height: 760 });
browser_host_create_browser(Some(&wi), Some(&mut client), Some(&CefString::from(url)), Some(&BrowserSettings::default()), None, None);

// OSR input
let host = browser.host().unwrap();
let ev = MouseEvent { x: 10, y: 20, modifiers: 0 };
host.send_mouse_move_event(Some(&ev), 0);
host.send_mouse_click_event(Some(&ev), MouseButtonType::LEFT, 0 /*up?*/, 1);
host.send_mouse_wheel_event(Some(&ev), 0, -120);
host.send_key_event(Some(&KeyEvent { type_: KeyEventType::CHAR, windows_key_code: 'a' as i32,
    character: 'a' as u16, unmodified_character: 'a' as u16, ..Default::default() }));
host.set_focus(1); host.was_resized(); host.was_hidden(0); host.notify_screen_info_changed();
```

**Cached-handler client (preferred shape)**

```rust
wrap_client! {
    pub struct CachedClient { display: DisplayHandler, life_span: LifeSpanHandler, render: RenderHandler }
    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> { Some(self.display.clone()) }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(self.life_span.clone()) }
        fn render_handler(&self) -> Option<RenderHandler> { Some(self.render.clone()) }
    }
}
let client = CachedClient::new(VsesvitDisplay::new(s.clone()), VsesvitLifeSpan::new(s.clone()), VsesvitRender::new(s));
```

### 1.7 `Task` / `post_task`: closure helper (compiled, `misc.rs`)

```rust
wrap_task! {
    pub struct FnTask { f: Arc<Mutex<Option<Box<dyn FnOnce() + Send + 'static>>>> }
    impl Task {
        fn execute(&self) { if let Some(f) = self.f.lock().unwrap().take() { f(); } }
    }
}

pub fn post_on<F: FnOnce() + Send + 'static>(thread: ThreadId, f: F) -> bool {
    let mut task = FnTask::new(Arc::new(Mutex::new(Some(Box::new(f)))));
    post_task(thread, Some(&mut task)) != 0          // also: post_delayed_task(thread, task, delay_ms)
}
// ThreadId::{UI, IO, FILE_BACKGROUND, FILE_USER_VISIBLE, FILE_USER_BLOCKING, PROCESS_LAUNCHER, RENDERER}
// currently_on(ThreadId::UI) != 0; task_runner_get_for_thread(..); V8Context::task_runner() for V8 threads
```

### 1.8 `SchemeHandlerFactory` + `ResourceHandler` (compiled, `misc.rs`)

```rust
wrap_scheme_handler_factory! {
    pub struct VsesvitSchemeFactory {}
    impl SchemeHandlerFactory {
        fn create(&self, _b: Option<&mut Browser>, _f: Option<&mut Frame>, _scheme: Option<&CefString>,
                  request: Option<&mut Request>) -> Option<ResourceHandler> {        // IO thread
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            Some(BytesResource::new(Arc::new(format!("<h1>{url}</h1>").into_bytes()), Arc::new(Mutex::new(0))))
        }
    }
}

wrap_resource_handler! {
    pub struct BytesResource { data: Arc<Vec<u8>>, offset: Arc<Mutex<usize>> }
    impl ResourceHandler {
        fn open(&self, _r: Option<&mut Request>, handle_request: Option<&mut i32>, _cb: Option<&mut Callback>) -> i32 {
            if let Some(h) = handle_request { *h = 1; } 1                       // handle now
        }
        fn response_headers(&self, response: Option<&mut Response>, len: Option<&mut i64>, _redirect: Option<&mut CefString>) {
            if let Some(r) = response { r.set_status(200); r.set_mime_type(Some(&CefString::from("text/html"))); }
            if let Some(l) = len { *l = self.data.len() as i64; }
        }
        fn read(&self, out: *mut u8, n: i32, bytes_read: Option<&mut i32>, _cb: Option<&mut ResourceReadCallback>) -> i32 {
            let mut off = self.offset.lock().unwrap();
            let k = (self.data.len() - *off).min(n.max(0) as usize);
            if let Some(b) = bytes_read { *b = k as i32; }
            if k == 0 { return 0; }                                           // 0 + bytes_read 0 = done
            unsafe { std::ptr::copy_nonoverlapping(self.data[*off..].as_ptr(), out, k) };
            *off += k; 1
        }
    }
}
// register (any browser-process thread): global or per request context
register_scheme_handler_factory(Some(&CefString::from("vsesvit")), None, Some(&mut VsesvitSchemeFactory::new()));
ctx.register_scheme_handler_factory(Some(&CefString::from("vsesvit")), None, Some(&mut VsesvitSchemeFactory::new()));
```

A custom scheme must also be registered in `App::on_register_custom_schemes` in **all** processes (VERIFIED doc).

`cef::wrapper::resource_manager` and `stream_resource_handler` (Rust ports of `CefResourceManager` / `CefStreamResourceHandler`) are alternatives (VERIFIED, they exist in `src/wrapper/`).

### 1.9 Cookies (compiled, `misc.rs`)

```rust
wrap_cookie_visitor! {
    pub struct CollectCookies { out: Arc<Mutex<Vec<(String, String, String)>>> }
    impl CookieVisitor {
        fn visit(&self, cookie: Option<&Cookie>, _count: i32, _total: i32, _delete: Option<&mut i32>) -> i32 {
            if let Some(c) = cookie {      // strings point into CEF memory: convert now, never store `c.clone()`
                self.out.lock().unwrap().push((c.domain.to_string(), c.name.to_string(), c.value.to_string()));
            }
            1 // continue
        }
    }
}
let cm = ctx.cookie_manager(None).unwrap();                  // per RequestContext; or cookie_manager_get_global_manager(None)
cm.visit_all_cookies(Some(&mut CollectCookies::new(out)));   // visitor runs on the UI thread
cm.set_cookie(Some(&CefString::from("https://example.org/")), Some(&Cookie {
    name: CefString::from("session"), value: CefString::from("abc"), domain: CefString::from(".example.org"),
    path: CefString::from("/"), secure: 1, httponly: 1, ..Default::default() }), None);
// also: visit_url_cookies, delete_cookies(url, name, cb), flush_store(cb)
```

### 1.10 DevTools protocol (compiled, `misc.rs`)

```rust
wrap_dev_tools_message_observer! {
    pub struct CdpObserver { results: Arc<Mutex<Vec<(i32, bool, String)>>> }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, _b: Option<&mut Browser>, id: i32, success: i32, result: Option<&[u8]>) {
            let json = result.map(|r| String::from_utf8_lossy(r).into_owned()).unwrap_or_default(); // valid only in callback
            self.results.lock().unwrap().push((id, success != 0, json));
        }
        fn on_dev_tools_event(&self, _b: Option<&mut Browser>, _method: Option<&CefString>, _params: Option<&[u8]>) {}
    }
}
// UI thread only:
let reg: Option<Registration> = host.add_dev_tools_message_observer(Some(&mut CdpObserver::new(results)));  // keep `reg` alive!
let mut params = dictionary_value_create().unwrap();
params.set_string(Some(&CefString::from("url")), Some(&CefString::from("https://example.org")));
let id = host.execute_dev_tools_method(0 /* auto id */, Some(&CefString::from("Page.navigate")), Some(&mut params)); // 0 = failed
host.send_dev_tools_message(Some(&br#"{"id":42,"method":"Runtime.evaluate","params":{"expression":"1+1"}}"#[..]));
```

- The observer stays registered until the returned `Registration` is dropped (VERIFIED doc).
- `on_dev_tools_message` (return 1 = handled) sees the raw JSON before it is split into result and event callbacks.
- `host.show_dev_tools(window_info, client, settings, inspect_at)` opens the DevTools frontend.
- `Settings.remote_debugging_port` enables an external CDP endpoint.

---

## 2. Process model, sandbox, settings, switches

### 2.1 Entry point: same executable vs helper (VERIFIED, plus INFERRED where marked)

This is the CEF contract (`cef_execute_process` doc, `docs/general_usage.md` "Entry-Point Function"):

- Every process starts in `main()`.
- `execute_process` returns `-1` immediately in the browser process. In a sub-process it blocks and then returns the exit code.
- `initialize` returns `0` on failure **or on intentional early exit**. One such case is a second launch with the same `root_cache_path`, which hands its arguments to the running instance and returns `CEF_RESULT_CODE_NORMAL_EXIT_PROCESS_NOTIFIED`, readable via `get_exit_code()`. After a 0 return, exit without calling other CEF functions.

```rust
fn main() -> std::process::ExitCode {
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);   // 1. FIRST CEF call in EVERY process (see G1)
    let args = OwnedArgs::from_env();                            //    (see G9; or cef::args::Args::new())
    let mut app = VsesvitApp::new(tx);
    let code = cef::execute_process(Some(args.main_args()), Some(&mut app), std::ptr::null_mut()); // 2. pass the App!
    if code >= 0 { return std::process::ExitCode::from(code as u8); }    // sub-process finished
    // 3. browser process only: now start GPUI, threads, tokio, etc.
    let settings = cef::Settings { /* §2.3 */ ..Default::default() };
    if cef::initialize(Some(args.main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()) != 1 {
        return std::process::ExitCode::from(cef::get_exit_code() as u8);
    }
    /* run the loop (§2.4) ... close all browsers, wait for on_before_close, drop all CEF handles ... */
    cef::shutdown();
    std::process::ExitCode::SUCCESS
}
```

- **The crate's `cefsimple` example passes `None` as the App to `execute_process`.** Its renderer processes therefore have no `RenderProcessHandler` and no custom schemes. Pass your App, as `examples/osr` does. (VERIFIED)
- **Same executable** is supported on Linux: an empty `browser_subprocess_path` uses the main exe (VERIFIED, `cef_settings_t` doc).
- **Separate helper executable**: set `Settings.browser_subprocess_path` to an *absolute* path.
  - The helper's `main` calls `api_hash` + `execute_process(Some(args), Some(&mut app), null)`.
  - Its `app` must provide the same `on_register_custom_schemes` and the `render_process_handler`.
  - The crate's `cefsimple_helper.rs` shows the shape. It is macOS-oriented but the code is generic.
- **Recommendation (INFERRED): use a helper for the GPUI shell.** Reasons:
  1. GPUI, Wayland/X11 and Vulkan libraries and any pre-`main` constructors stay out of zygote, renderer, GPU and utility processes.
  2. Chromium's sandbox requires a single thread at initialization. CEF's `tests/cefclient/cefclient_gtk.cc` says so explicitly and initializes GTK *after* CEF for that reason. Any dependency that spawns a thread before `execute_process` would break the sandbox.
  3. `cargo test` harness binaries can't be re-executed as `--type=renderer` children: libtest owns `main`. With a helper, tests that create browsers only need `browser_subprocess_path`. Integration tests that start CEF must otherwise use `harness = false`.

### 2.2 Sandbox on Linux (VERIFIED, Chromium 154.0.8037.98 and the CEF 154 patch)

- **The `sandbox` cargo feature is a no-op on Linux.** It only switches CMake `USE_SANDBOX` on Windows and macOS (`sys/build.rs`). The Linux sandbox is purely runtime.
- **Selection order:** `content/browser/zygote_host/zygote_host_impl_linux.cc` (`ZygoteHostImpl::Init`):
  1. `--no-sandbox` → no sandbox.
  2. Running as root without `--no-sandbox` → `exit(EXIT_FAILURE)`.
  3. If unprivileged user namespaces work, and `--disable-namespace-sandbox` is not set → **namespace sandbox** (no helper binary needed).
  4. Else, if a `chrome-sandbox` binary is found and `--disable-setuid-sandbox` is not set → **setuid sandbox**. The binary must be root-owned with mode 4755, otherwise LOG(FATAL) "…is owned by root and has mode 4755" (`sandbox/linux/suid/client/setuid_sandbox_host.cc`).
  5. Else LOG(FATAL) "No usable sandbox!…" (mentions Ubuntu 23.10+ AppArmor restrictions).
- **Where `chrome-sandbox` is looked for.** Upstream looks in `DIR_EXE`. **CEF patches that to `DIR_ASSETS`**, i.e. the directory of `libcef.so` (`patch/patches/linux_assets_path_1936.patch`). The `CHROME_DEVEL_SANDBOX` env var is honoured only if `/proc/self/exe` is owned by the current user.
- **This host** (Fedora 44): `/proc/sys/user/max_user_namespaces` = 254127 and there is no AppArmor userns restriction. The namespace sandbox should work with `no_sandbox = 0` and the non-setuid `chrome-sandbox` that the build script copies (INFERRED: not launched).
- **Flatpak:**
  - The crate's build script supports `FLATPAK=1` (CEF at `/usr/lib`).
  - Chromium-based Flatpaks can't use setuid or nested namespaces. They usually rely on a zypak-style shim, which I did not research (INFERRED).
  - The crate has no zypak integration.

### 2.3 `Settings` fields that matter (VERIFIED: Rust field names from the bindings, semantics from the `cef_settings_t` docs in CEF 154 `include/internal/cef_types.h`)

`Settings::default()` sets `size` and zeroes everything else. CefString fields take `CefString::from("...")`. The struct is *cloned* (shallow) when passed to `initialize`, so the original must stay alive during the call.

| Field (Rust) | Meaning / Linux notes |
|---|---|
| `no_sandbox: i32` | 1 disables the sandbox. Same as `--no-sandbox`. |
| `browser_subprocess_path: CefString` | Absolute path to the helper exe. Empty = main exe. |
| `multi_threaded_message_loop: i32` | Windows and Linux only. CEF's UI thread runs on its own thread; do not call `run_message_loop`/`do_message_loop_work`. **Disables GtkUi on Linux** (§2.4). |
| `external_message_pump: i32` | Drives scheduling via `on_schedule_message_pump_work` + `do_message_loop_work`. |
| `windowless_rendering_enabled: i32` | Required for OSR browsers. "May reduce rendering performance" if OSR is unused. |
| `command_line_args_disabled: i32` | Ignore the process command line. `on_before_command_line_processing` still runs. |
| `root_cache_path: CefString` | Install-wide data and the **process-singleton lock**. Default when empty is `~/.config/cef_user_data`, "not recommended in production". All `cache_path`s must equal it or be below it. Also the parent of the `Dictionaries` dir, crash dumps and `DevToolsActivePort`. |
| `cache_path: CefString` | Global context's profile. Empty = incognito global context. |
| `persist_session_cookies: i32` | Needs `cache_path`. |
| `user_agent`, `user_agent_product` | |
| `locale` | **Ignored on Linux.** The locale comes from `LANGUAGE`/`LC_ALL`/`LC_MESSAGES`/`LANG`, or `--lang`. |
| `log_file`, `log_severity: LogSeverity`, `log_items: LogItems` | **Default log file is `debug.log` in the executable's directory.** That is not writable for a system install, so set it, or use `LogSeverity::DISABLE`. Switches: `--log-file`, `--log-severity`, `--enable-logging=stderr`. |
| `javascript_flags` | V8 flags. |
| `resources_dir_path`, `locales_dir_path` | Absolute paths. Empty = next to `libcef.so` (§3.3). |
| `remote_debugging_port` | 1024–65535. Exposes a CDP endpoint. |
| `uncaught_exception_stack_size` | >0 enables `RenderProcessHandler::on_uncaught_exception`. |
| `background_color` | ARGB. Alpha 0 with OSR = transparent painting. |
| `accept_language_list`, `cookieable_schemes_list`, `cookieable_schemes_exclude_defaults` | |
| `chrome_policy_id` | Linux: a directory such as `/etc/opt/chrome/policies`. Enables Chrome enterprise policy (Chrome style). Relevant for extension force-install policies (INFERRED). |
| `disable_signal_handlers` | |
| `use_views_default_popup` | API 14600+. Bare Views window instead of Chrome UI for default popups of native-hosted Chrome-style browsers. |

### 2.4 Message loop choice under GPUI

GPUI on Linux uses calloop and no GLib (`cef-osr.md` §7, VERIFIED there).

**`run_message_loop()`** blocks the main thread. It is unusable with GPUI.

**`external_message_pump = 1`** (CEF UI thread = GPUI main thread):

- How CEF does it (VERIFIED):
  - `libcef/browser/browser_message_loop.cc`: `MessagePumpExternal` subclasses `base::MessagePumpForUI`.
  - On Linux that is `MessagePumpGlib` (`base/message_loop/message_pump_for_ui.h`, `USE_GLIB`; libcef.so has `NEEDED libglib-2.0.so.0`).
  - It overrides `Run`, `ScheduleWork` (→ `OnScheduleMessagePumpWork(0)`) and `ScheduleDelayedWork` (→ `OnScheduleMessagePumpWork(delta)`).
  - `MessagePumpGlib` on the main thread uses `g_main_context_default()`, and `WatchFileDescriptor` attaches GSources there.
  - CEF's own Linux sample, `tests/shared/browser/main_message_loop_external_pump_linux.cc`, drives `g_main_context_iteration(g_main_context_default(), …)`. So does the crate's port, `examples/tests_shared/src/browser/main_message_loop_external_pump/linux.rs`.
- **Consequence (INFERRED from VERIFIED code):** the host must
  - call `do_message_loop_work()` when asked, cancelling an earlier pending timer when a new delay arrives (doc), and capping the wait at about 33 ms like CEF's sample;
  - **also** iterate the default GLib context, non-blocking. Otherwise Chromium's X11/Wayland/D-Bus fd watches are never dispatched.
- Iterating it is safe: with `state_ == nullptr` the work source's `HandleCheck` returns FALSE, so only fd sources dispatch.
- **Do not take GLib's prepare timeout.** `HandlePrepare` returns `0` in that state, so a blocking GLib-driven poll would spin. `prior-art.md` cites tauri#16189 for exactly that.
- `pump.rs` in the scratch crate is a compiled sketch:
  - a `PumpHandle::schedule(delay)` that any thread can call;
  - `Pump::tick()` on the main thread, which runs `g_main_context_iteration(NULL, 0)` until it returns 0, and calls `do_message_loop_work()` when due.
  - It needs `#[link(name = "glib-2.0")]`. Under GPUI, `schedule` must also wake GPUI's foreground loop.
- Re-entrancy: callbacks fired by `do_message_loop_work` may schedule more work. Guard against re-entrant calls, as the CEF sample does (VERIFIED in the sample).

**`multi_threaded_message_loop = 1`:**

- How it works (VERIFIED):
  - CEF starts its own "CefUIThread" (`libcef/browser/ui_thread.cc`).
  - Its `MessagePumpGlib` gets an **owned** GLib context, because it is not on the main thread (`message_pump_glib.cc`).
  - Nothing touches GPUI's thread.
- **Cost (VERIFIED):** CEF calls `ui::SetMultiThreadedMessageLoopX11()`/`…Wayland()`, which skip creating `LinuxUiDelegate`. "We can't use GtkUi in combination with multi-threaded-message-loop because Chromium's GTK implementation doesn't use GDK threads" (`patch/patches/linux_gtk_theme_3610.patch`). Without GtkUi there is:
  - no GTK IME for windowed browsers;
  - no GTK file chooser or print dialog;
  - no GTK theme.
  - `cef-windowing.md` and `cef-osr.md` detail the consequences.
- Every callback runs on CEF's UI thread. UI-thread-only APIs must go through `post_task(ThreadId::UI, …)`. OSR frames must cross threads.

The trade-off (INFERRED):

| Content | Better choice |
|---|---|
| OSR content (GPUI-composited, IME through GPUI's `InputHandler`) | `multi_threaded_message_loop` is simpler and robust. |
| Windowed Chrome-style X11 children | `external_message_pump`, because native IME and dialogs need GtkUi. |

Prototype both. `prior-art.md` and `cef-windowing.md` record the conflicting recommendations from different angles.

### 2.5 Command-line switches (VERIFIED API; VERIFIED switch semantics where cited)

- **Browser process:** in `App::on_before_command_line_processing`, `process_type` is NULL or empty for the browser process. CEF's doc warns that changing switches for non-browser processes "may result in undefined behavior including crashes".
- **Child processes:** in `BrowserProcessHandler::on_before_child_process_launch`, to pass app-specific switches to renderers and the GPU process.
- **API:** `has_switch`, `switch_value` (returns a `CefStringUserfree`; convert with `CefString::from(&v).to_string()`), `append_switch`, `append_switch_with_value`, `append_argument`, `remove_switch`, `switches(&mut CefStringMap)`.

Useful switches:

| Switch | Notes |
|---|---|
| `--ozone-platform=x11` or `wayland` | Native child embedding is X11-only (CEF #2804). |
| `--use-angle=gl-egl` | With `--ozone-platform=x11`, needed for **Linux OSR shared-texture (DMA-BUF) mode**, especially on NVIDIA. CEF's own `tests/shared/browser/client_app_browser.cc` (branch 8037) says the EGL DMA-BUF import is "required for compatibility with drivers like NVIDIA that do not support pixmaps". |
| `--remote-debugging-port=N` | CDP endpoint. |
| `--enable-logging=stderr`, `--v=1` | Logging. |
| `--disable-gpu`, `--disable-gpu-compositing` | CEF's samples add these for software OSR, which disables WebGL. |
| `--load-extension=<dir>` | **Works in CEF**: Chromium only ignores it for `GOOGLE_CHROME_BRANDING` builds, Enhanced Safe Browsing users, or a blocklist policy (`chrome/browser/extensions/extension_service.cc` at 154.0.8037.98). A development tool, not a store-install path. |

---

## 3. Runtime layout, the build script, binary size

### 3.1 What `cef-dll-sys`'s build script does on Linux (VERIFIED, `sys/build.rs` + empirical run)

1. **Choose the CEF directory.** The CEF version comes from the crate's `+154.0.34` suffix.
   - `FLATPAK` set → `/usr/lib`.
   - `NIX_CEF_BINARY` set → Nix install.
   - `CEF_PATH` set and exists:
     - if `$CEF_PATH/154.0.34/` exists → use or download `$CEF_PATH/154.0.34/cef_linux_x86_64`;
     - else use `$CEF_PATH` itself, **if its `archive.json` version is ≤ the expected version**;
     - else download into the versioned subdir.
   - `CEF_PATH` set but missing → download into `$CEF_PATH/154.0.34/`.
   - `CEF_PATH` unset → download into `OUT_DIR`, so every `cargo clean` re-downloads.
2. **Download.** The source is `https://cef-builds.spotifycdn.com/index.json`, overridable with `CEF_DOWNLOAD_URL`. The build script gets the **minimal** archive (326,489,962 bytes for 154.0.34) and extracts `Release/*` + `Resources/*` + `include/`, `libcef_dll/`, `cmake/`, `CMakeLists.txt`, `CREDITS.html` flat into `cef_linux_x86_64/`.
   - The `.tar.bz2` is **left in** `$CEF_PATH/154.0.34/` (seen on disk). `export-cef-dir` deletes it unless `--save-archive`.
   - My run took about 1 minute.
3. **Link.** It emits `cargo::rustc-link-search=native=<cef_dir>`, `cargo::rustc-link-lib=dylib=cef`, `cargo::metadata=CEF_DIR=…` and `CEF_API_VERSION=15400` (seen in the build output).
   - The `links = "cef_dll_wrapper"` metadata reaches only **immediate** dependents as `DEP_CEF_DLL_WRAPPER_CEF_DIR` (Cargo docs). Your build script sees it only if you depend on `cef-dll-sys` directly.
4. **No CMake on Linux.**
   - The `cmake::Config` is created, but `.build()` is only called in the Windows and macOS branches.
   - After a real build, `OUT_DIR` of `cef-dll-sys` was empty.
   - **Installing cmake is unnecessary for Linux builds.** It is needed only to cross-build Windows or macOS, or to use CEF's C++ wrapper.
5. **Copy.** `copy_cef_runtime_files` copies **every top-level file** of the CEF dir plus `locales/` into `target/<profile>/`. That is `libcef.so` (1.36 GiB) and the rest: `target/debug` reached 1.9 GB.
   - It re-runs whenever the build script runs (`rerun-if-changed` on the CEF dir).
   - It now removes destination files before copying, which fixes the read-only `CEF_PATH` bug #467.
6. **No rpath, no `chrome-sandbox` permission handling, no stripping.** The binary only loads if RUNPATH finds `libcef.so` or `LD_LIBRARY_PATH` is set. The README tells you to export `LD_LIBRARY_PATH=$CEF_PATH`.

**Add an rpath in your binary crate's `build.rs`** (VERIFIED: `readelf` on the linked scratch binary shows `RUNPATH [$ORIGIN]` and `NEEDED libcef.so`):

```rust
// build.rs of the shell/helper binaries
fn main() {
    println!("cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");          // libcef.so next to the exe
    // packaged layout alternative: "-Wl,-rpath,$ORIGIN/../lib/vsesvit"
}
```

`libcef.so` itself has `RUNPATH [$ORIGIN]` (VERIFIED, `readelf -d`), so it finds `libvk_swiftshader.so` and `libvulkan.so.1` next to itself.

### 3.2 Tools (VERIFIED)

- **`export-cef-dir`** is published on crates.io, latest 154.5.0+154.0.34. Run `cargo install export-cef-dir` or `cargo run -p export-cef-dir -- --force ~/.local/share/cef` in the cef-rs repo.
  - It exports a flat directory compatible with `CEF_PATH`.
  - Options: `--target`, `--version`, `--mirror-url`, `--archive <local file>`, `--save-archive`, `--nix`.
- **`bundle-cef-app <bin> -o <dir> [--release]`** (Linux) runs `cargo build --bin <bin>`, copies the CEF dir's files + `locales/` and the executable into `<dir>`.
  - It does **not** copy `resources_path`, set an rpath, or handle `chrome-sandbox`.
  - The README says `./target/bundle/cefsimple.exe`; the Linux code copies the file without `.exe`.
  - Treat it as a dev convenience, not packaging.
- **`download-cef`** 3.0 is the library behind both tools: `CefIndex`, `extract_target_archive`, `check_archive_json`.

### 3.3 Where files must live (VERIFIED in CEF 154 source)

CEF overrides `base::DIR_ASSETS` with the directory of `libcef.so` (`libcef/common/resource_util.cc`, `OverrideAssetPath()` via `dladdr`, called from `chrome_main_delegate_cef.cc`). Without the override it would be the executable's directory: on Linux, `FILE_MODULE` = `/proc/self/exe` in `base/base_paths_posix.cc`.

These are resolved relative to `DIR_ASSETS`:

- the pak files (`GetResourcesDir()`);
- `chrome-sandbox` (patched lookup);
- files shared with child processes such as `v8_context_snapshot.bin` (patched `child_process_launcher_helper_linux.cc`);
- EGL/GLES and SwiftShader loading (patched `egl_util.cc`).

`resources_dir_path` and `locales_dir_path` (or `--resources-dir-path` / `--locales-dir-path`) override two of those.

Recommended packaged layout (INFERRED):

```text
/usr/lib/vsesvit/          (or /opt/vsesvit/)
  vsesvit                  # GPUI shell, RUNPATH=$ORIGIN
  vsesvit-cef-helper       # optional subprocess exe, RUNPATH=$ORIGIN
  libcef.so                # stripped
  chrome-sandbox           # root:root 4755 only needed where user namespaces are unavailable
  icudtl.dat  v8_context_snapshot.bin
  resources.pak  chrome_100_percent.pak  chrome_200_percent.pak
  libvk_swiftshader.so  libvulkan.so.1  vk_swiftshader_icd.json
  locales/<lang>.pak       # ship what you need; en-US.pak is the fallback
```

The distribution's README lists what is required (VERIFIED, `README.txt` of the 154.0.34 minimal archive):

- Required: `libcef.so`, `icudtl.dat`, `v8_context_snapshot.bin`.
- Optional: `locales/` and the `.pak` files. Without them, "arbitrary Web components may display incorrectly".
- Optional: SwiftShader (`libvk_swiftshader.so`, `libvulkan.so.1`, `vk_swiftshader_icd.json`), for the software WebGL/canvas fallback.
- 154 ships no `libEGL.so`/`libGLESv2.so`.

### 3.4 Sizes (VERIFIED, measured on 154.0.34 x86_64)

| File | Bytes |
|---|---|
| `libcef.so` as shipped ("with debug_info, not stripped") | 1,455,021,248 |
| `libcef.so` after `strip --strip-unneeded` | **272,219,288** (xz -6: 78,659,676) |
| `resources.pak` / `chrome_100_percent.pak` / `chrome_200_percent.pak` | 23,500,543 / 582,158 / 944,863 |
| `icudtl.dat` / `v8_context_snapshot.bin` | 10,819,840 / 757,576 |
| `libvk_swiftshader.so` / `libvulkan.so.1` | 14,907,128 / 1,533,120 |
| `locales/` (220 `.pak`, including gendered variants such as `af_FEMININE.pak`) | about 49 MB total; `en-US.pak` 578,466 |
| `chrome-sandbox` | 31,680 |
| Scratch Rust exe (debug, unstripped, no GPUI) | 28,109,224 |

INFERRED: expect about 320 MB installed plus your executable(s), and about 100 MB compressed, with a stripped `libcef.so` and English-only locales. Keep the unstripped file, or CEF's separate symbol archives, for crash symbolication.

Runtime library dependencies (VERIFIED, `readelf -d libcef.so`):

- glib-2.0, gobject, gio;
- nspr4, nss3, nssutil3, smime3;
- atk, atk-bridge, atspi, dbus-1;
- cups;
- X11, Xcomposite, Xdamage, Xext, Xfixes, Xrandr, xcb, xkbcommon;
- gbm, expat, cairo, pango;
- udev, asound;
- libc, libm, libgcc_s, libdl, libpthread.

All of them resolve on this Fedora 44 host (`ldconfig -p`). Notably libX11 is needed even for Wayland-only use (see also the CEF forum).

---

## 4. IPC and V8 (VERIFIED API; the snippets are compiled `renderer.rs` + `client.rs`)

### 4.1 Messages

- **Startup data.** Pass a `DictionaryValue` as `extra_info` to `browser_host_create_browser[_sync]`. It arrives in every renderer of that browser via `RenderProcessHandler::on_browser_created(browser, extra_info)` (VERIFIED, `docs/general_usage.md` "Process Startup Messages"). Use it for profile id, internal-page flags and the like.
- **Runtime messages.**
  - `process_message_create(Some(&name))` → `msg.argument_list()` (`ListValue`: `set_string/int/bool/double/binary/dictionary/list`).
  - Send with `frame.send_process_message(ProcessId::RENDERER | ProcessId::BROWSER, Some(&mut msg))`.
  - "Ownership of the message contents will be transferred and the |message| reference will be invalidated."
  - Delivery is not guaranteed if the frame navigates, closes or crashes, so ACK if needed (VERIFIED doc).
- **Where messages arrive.** Renderer → browser lands in `Client::on_process_message_received`. Browser → renderer lands in `RenderProcessHandler::on_process_message_received`.
- **Large payloads.** `shared_process_message_builder_create(name, size)` (`SharedMemoryRegion`). The crate's `message_router` switches to it above `message_size_threshold`.
- **Frame identifiers** are strings in 154: `frame.identifier()` returns `CefStringUserfree`. Browser identifiers are `i32`.

Browser side:

```rust
fn on_process_message_received(&self, _b: Option<&mut Browser>, frame: Option<&mut Frame>, _src: ProcessId,
                               message: Option<&mut ProcessMessage>) -> i32 {
    let Some(msg) = message else { return 0 };
    if CefString::from(&msg.name()).to_string() != "vsesvit.echo" { return 0; }
    let Some(args) = msg.argument_list() else { return 0 };
    let (id, text) = (args.int(0), CefString::from(&args.string(1)).to_string());
    if let (Some(frame), Some(mut reply)) = (frame, process_message_create(Some(&CefString::from("vsesvit.echo.reply")))) {
        if let Some(l) = reply.argument_list() { l.set_int(0, id); l.set_string(1, Some(&CefString::from(text.as_str()))); }
        frame.send_process_message(ProcessId::RENDERER, Some(&mut reply));
    }
    1
}
```

### 4.2 V8 binding with a promise (renderer side, `TID_RENDERER`)

```rust
#[derive(Default)]
pub struct Pending { next_id: i32, promises: std::collections::HashMap<i32, (V8Context, V8Value)> }

wrap_render_process_handler! {
    pub struct VsesvitRenderProcessHandler { pending: Arc<Mutex<Pending>> }
    impl RenderProcessHandler {
        fn on_context_created(&self, _b: Option<&mut Browser>, frame: Option<&mut Frame>, context: Option<&mut V8Context>) {
            let (Some(frame), Some(ctx)) = (frame, context) else { return };
            if frame.is_main() == 0 { return; }                       // and check the URL: see 4.4
            let Some(global) = ctx.global() else { return };
            let mut handler = EchoHandler::new(self.pending.clone());
            let (Some(mut obj), Some(mut func)) = (v8_value_create_object(None, None),
                v8_value_create_function(Some(&CefString::from("echo")), Some(&mut handler))) else { return };
            obj.set_value_bykey(Some(&CefString::from("echo")), Some(&mut func), V8Propertyattribute::default());
            global.set_value_bykey(Some(&CefString::from("vsesvit")), Some(&mut obj), V8Propertyattribute::default());
        }
        fn on_context_released(&self, _b: Option<&mut Browser>, _f: Option<&mut Frame>, context: Option<&mut V8Context>) {
            let Some(ctx) = context else { return };                   // drop V8 handles of this context
            self.pending.lock().unwrap().promises.retain(|_, (c, _)| c.is_same(Some(&mut ctx.clone())) == 0);
        }
        fn on_process_message_received(&self, _b: Option<&mut Browser>, _f: Option<&mut Frame>, _src: ProcessId,
                                       message: Option<&mut ProcessMessage>) -> i32 {
            let Some(msg) = message else { return 0 };
            if CefString::from(&msg.name()).to_string() != "vsesvit.echo.reply" { return 0; }
            let Some(args) = msg.argument_list() else { return 0 };
            let entry = self.pending.lock().unwrap().promises.remove(&args.int(0));
            if let Some((ctx, promise)) = entry {
                if ctx.enter() == 1 {                                   // resolve inside the right context
                    let mut v = v8_value_create_string(Some(&CefString::from(&args.string(1))));
                    promise.resolve_promise(v.as_mut());
                    ctx.exit();
                }
            }
            1
        }
    }
}

wrap_v8_handler! {
    pub struct EchoHandler { pending: Arc<Mutex<Pending>> }
    impl V8Handler {
        fn execute(&self, _name: Option<&CefString>, _object: Option<&mut V8Value>, arguments: Option<&[Option<V8Value>]>,
                   retval: Option<&mut Option<V8Value>>, exception: Option<&mut CefString>) -> i32 {
            let text = arguments.and_then(|a| a.first().cloned().flatten()).filter(|v| v.is_string() != 0)
                .map(|v| CefString::from(&v.string_value()).to_string());
            let Some(text) = text else {
                if let Some(e) = exception { e.try_set("echo(text: string) expects a string"); } // try_set, not `*e =`
                return 1;
            };
            let (Some(ctx), Some(promise)) = (v8_context_get_current_context(), v8_value_create_promise()) else { return 0 };
            let id = { let mut p = self.pending.lock().unwrap(); p.next_id += 1;
                       p.promises.insert(p.next_id, (ctx.clone(), promise.clone())); p.next_id };
            if let (Some(frame), Some(mut msg)) = (ctx.frame(), process_message_create(Some(&CefString::from("vsesvit.echo")))) {
                if let Some(a) = msg.argument_list() { a.set_int(0, id); a.set_string(1, Some(&CefString::from(text.as_str()))); }
                frame.send_process_message(ProcessId::BROWSER, Some(&mut msg));
            }
            if let Some(r) = retval { *r = Some(promise); }              // written back by the trampoline (VERIFIED)
            1
        }
    }
}
```

### 4.3 Other mechanisms

- **`frame.execute_java_script(code, script_url, start_line)`** runs in the frame's main world with no return value (VERIFIED signature and doc). For results, either:
  - post back over IPC from an injected function; or
  - call `execute_dev_tools_method(0, "Runtime.evaluate", params)` and read `on_dev_tools_method_result` (UI thread).
- **`cef::wrapper::message_router`** (VERIFIED) is a full Rust port of `CefMessageRouter`:
  - `window.cefQuery({request, persistent, onSuccess, onFailure})` and `cefQueryCancel`;
  - `BrowserSideRouter` / `RendererSideRouter`, `BrowserSideHandler::on_query_str/on_query_binary` (Send + Sync, called on the UI thread);
  - you must forward `on_before_close`, `on_render_process_terminated`, `on_before_browse`, `on_process_message_received`, `on_context_created` and `on_context_released` to it;
  - 145.x fixed panics and the `persistent` handling (CHANGELOG, #346/#347/#354).
- **`V8Context::eval`** in the renderer gives synchronous results inside renderer code.

### 4.4 Safety note (INFERRED)

`on_context_created` fires for **every** frame: web pages, `chrome-extension://` pages, DevTools, PDF viewer. Anything you inject into the main world is visible to page scripts and can be called by them.

- Only bind `window.vsesvit` for trusted origins (your `vsesvit://` scheme), checking `frame.url()`.
- Validate every IPC message in the browser process against the sending frame's URL. Never trust renderer-provided identity.
- CEF exposes no isolated-world API.

---

## 5. Request contexts, preferences, cookies, DevTools, extensions

### 5.1 One request context per profile (VERIFIED API and docs; mapping INFERRED)

```rust
pub fn profile_context(root_cache_path: &str, profile_dir: &str) -> Option<RequestContext> {
    let settings = RequestContextSettings {
        cache_path: CefString::from(format!("{root_cache_path}/{profile_dir}").as_str()), // = or below root_cache_path
        persist_session_cookies: 1,
        accept_language_list: CefString::from("en-US,en"),
        ..Default::default()
    };
    request_context_create_context(Some(&settings), None /* or Some(&mut handler) */)
}
pub fn incognito_context() -> Option<RequestContext> {
    request_context_create_context(Some(&RequestContextSettings::default()), None) // empty cache_path = in-memory
}
// share storage with another context but use a different handler:
// request_context_cef_create_context_shared(Some(&mut other), Some(&mut handler))
// the global context (Settings.cache_path): request_context_get_global_context()
```

What the docs and source say (VERIFIED, `cef_request_context_settings_t` and `cef_request_context.h`):

- Different request contexts never share a render process.
- Browsers opened via `window.open` or targeted links share the opener's context and render process.
- An empty `cache_path` means "incognito mode" with in-memory caches. Installation-specific data still goes to `root_cache_path`.
- An invalid `cache_path` (not equal to or below `root_cache_path`) is logged — "The cache_path is invalid. Defaulting to in-memory storage." — and the context **silently becomes in-memory** (`libcef/browser/context.cc`, `ValidateCachePath` uses `FilePath::IsParent`).
- `RequestContextHandler::on_request_context_initialized` signals readiness. Request-context prefs registered via `on_register_custom_preferences(PreferencesType::REQUEST_CONTEXT, …)` become available after it.
- The RequestContext API exists only in the Rust bindings (`ImplRequestContext`):
  - `cookie_manager`, `register_scheme_handler_factory`, `clear_scheme_handler_factories`;
  - `clear_certificate_exceptions`, `clear_http_auth_credentials`, `close_all_connections`, `clear_http_cache`, `resolve_host`;
  - `media_router`;
  - `website_setting`, `set_website_setting`, `content_setting`, `set_content_setting` (per-site permissions);
  - `set_chrome_color_scheme`;
  - `add_setting_observer`.

**Mapping to Vsesvit (INFERRED).** `vsesvit-core` puts profiles at `~/.local/share/vsesvit/profiles/<name>/` with `engine_data = <root>/engine` (`crates/vsesvit-core/src/lib.rs`, `ProfilePaths`). Two options:

1. `root_cache_path = ~/.local/share/vsesvit/profiles` with `cache_path = …/<name>/engine` (nested). This needs verification: Chromium's ProfileManager normally expects profile directories directly under the user-data dir.
2. A dedicated `root_cache_path = ~/.local/share/vsesvit/cef` with `cache_path = …/cef/<profile-id>`, plus `engine_data` changed to point there.

Note that `root_cache_path` also holds `Local State`, `SingletonLock`, crash dumps and `Dictionaries/`. It is also the **single-instance lock**: two Vsesvit processes with the same root cannot both run Chromium. The second gets `initialize() == 0` and its arguments are forwarded to `on_already_running_app_relaunch`.

### 5.2 Preferences (VERIFIED API; pref names VERIFIED in Chromium 154 sources)

`RequestContext` implements `ImplPreferenceManager`:

- `has_preference`, `preference`, `all_preferences(include_defaults)`;
- `can_set_preference`, `set_preference(name, value, error)`;
- `add_preference_observer`.

Global prefs go through `preference_manager_get_global()`. `set_preference` **must be called on the browser UI thread** (doc). Passing `value: None` restores the default.

```rust
pub fn set_string_pref(ctx: &RequestContext, name: &str, value: &str) -> Result<(), String> {
    let mut v = value_create().ok_or("value_create failed")?;
    v.set_string(Some(&CefString::from(value)));
    let mut err = CefString::from("");   // NOT CefString::default(): that is passed as NULL and CEF returns 0 (G4)
    if ctx.set_preference(Some(&CefString::from(name)), Some(&mut v), Some(&mut err)) == 1 { Ok(()) } else { Err(err.to_string()) }
}
// read: write_json(Some(&mut ctx.preference(Some(&name))?), JsonWriterOptions::default())
```

Verified pref names in Chromium 154:

| Pref | Source |
|---|---|
| `download.default_directory`, `download.prompt_for_download`, `extensions.ui.developer_mode`, `webkit.webprefs.fonts.*` | `chrome/common/pref_names.h` |
| `intl.accept_languages`, `intl.selected_languages` | `components/language/core/browser/pref_names.h` |
| `browser.enable_spellchecking`, `spellcheck.dictionaries` | `components/spellcheck/browser/pref_names.h` |

Extension state is in `extensions.settings` (`extensions/browser/pref_names.h`, `kExtensions`). Read it, but don't write it.

### 5.3 Cookies

There is one cookie manager per request context: `ctx.cookie_manager(callback)`. If you pass a callback, it fires on the UI thread once storage is initialized. Examples are in §1.9.

`cookie_manager_get_global_manager(None)` is the global context's manager.

### 5.4 DevTools protocol

See §1.10.

- Methods return 0 off the UI thread.
- Keep the `Registration` returned by `add_dev_tools_message_observer` alive for as long as you want callbacks.
- `on_dev_tools_agent_attached` and `…_detached` track agent state.
- CDP is also the practical way to reach Chrome features that have no CEF wrapper (`Page.*`, `Emulation.*`, `Runtime.*`, `Storage.*`, and the `Extensions.*` domain where available). That last point is INFERRED and needs checking against the Chromium 154 protocol and its permission gates.

### 5.5 Extensions: what the crate gives you (VERIFIED, plus pointers)

- **No extension API.**
  - There is no `cef_extension*.h` in CEF 154: grepping `include/` finds nothing.
  - Maintainer statements: cef-rs #235 ("there is no API for extension management at runtime in CEF… `CefExtensionHandler` … was deprecated and removed") and #138.
- **What you can use:**
  - Chrome-style browsers;
  - `--load-extension` (§2.5);
  - Chrome Web Store installs through Chrome UI flows;
  - `chrome_policy_id` with `ExtensionInstallForcelist` policies (INFERRED);
  - CDP;
  - `CommandHandler` to filter Chrome commands and menu items in Chrome style;
  - `default_client()` for Chrome-created windows.
- Compatibility per architecture is in `cef-extensions.md` and `gpt-extension-architecture.md`.

---

## 6. Gotchas (numbered for reference)

**G1. Pin the API version first, in every process.** (VERIFIED)

- Call `api_hash(sys::CEF_API_VERSION_LAST, 0)` before any other CEF call: `cefsimple` `shared::load_cef`, `cefsimple_helper.rs`, `examples/osr`.
- On Linux the crate doesn't build `libcef_dll_wrapper`, which would otherwise do this.
- If you skip it, `cef_api_version()` stays `-1` (`libcef_dll/libcef_dll2.cc`). Version-guarded libcef code then hits `CEF_API_REQUIRE_ADDED` → LOG(FATAL), and struct wrapping aborts with "Cannot wrap struct with invalid base.size … at API version -1". `prior-art.md` cites a real embedder hitting "called with invalid version -1".
- The value can't be changed after the first call ("CEF API version cannot be configured multiple times").

**G2. The experimental member must not be used, and `CEF_PATH` must match the version.**

- The Rust bindings come from the experimental headers, while the runtime is pinned to 15400 (VERIFIED). In 154.0.34 the only experimental items are `BrowserHost::set_ax_viewport_collapse` and `BrowserSettings.ax_viewport_collapse`. Don't use them.
  - The struct field is safe, because CEF checks member presence by `size` (`CEF_MEMBER_EXISTS`).
  - Calling the method is INFERRED to be unsafe at 15400.
- For client-implemented structs, libcef checks `size == sizeof(15400 struct)` and aborts on a mismatch (VERIFIED). No client struct has experimental members in 154, so the sizes match.
  - **Risk on upgrade (INFERRED):** a future CEF that adds an experimental *client-side* callback would make cef-rs structs too large and abort at wrap time, unless cef-rs changes how it generates bindings. Pin `cef = "=154.5.0"` and test before upgrading.
- `CEF_PATH` should hold the exact version. The build script accepts any `archive.json` version ≤ the crate's (VERIFIED). An older `libcef.so` would reject API 15400 at run time ("Request for unsupported CEF API version"). Use the versioned subdir layout `$CEF_PATH/154.0.34/`.

**G3. Strings in struct out-params are dropped.** (VERIFIED mechanism; INFERRED that it explains #409)

- Struct out-params (`Option<&mut WindowInfo>` in `on_before_popup`, `Option<&mut LinuxWindowProperties>`, …) pass through `WrapParamRef`. The C struct is converted to Rust and written back on drop by moving it through `From<Struct> for _cef_struct_t`.
- That conversion writes a `CefString` created with `CefString::from(&str)` (the owning "Clear" variant) as a **zeroed, empty string**, and frees the Rust copy.
- So `props.wayland_app_id = CefString::from("org.vsesvit")` silently sets an empty app id. That is exactly cef-rs #409, which is still open.
- `CefString::from("..").clone()` would be worse: it writes back a dangling pointer.
- Workaround: build a CefString whose buffer CEF will own (compiled, `misc.rs`):

```rust
pub fn cef_owned_string(s: &str) -> CefString {
    let mut raw: sys::cef_string_utf16_t = unsafe { std::mem::zeroed() };
    unsafe { sys::cef_string_utf8_to_utf16(s.as_ptr().cast(), s.len(), &mut raw) };
    CefString::from(raw)   // "Borrowed" variant: Rust won't free it; CEF frees it via raw.dtor
}
// p.wayland_app_id = cef_owned_string("org.vsesvit.Vsesvit");
```

- In-params (`Option<&Settings>`, `Option<&WindowInfo>`, `Option<&Cookie>` passed *to* CEF) work with `CefString::from`. Those are shallow-cloned before conversion, so the original must simply outlive the call.

**G4. NULL out-params are silently ignored.** (VERIFIED)

- `CefString::default()` is the "Borrowed(None)" variant. Passed as `Option<&mut CefString>` it becomes a NULL pointer.
- CEF's translator returns the default value (0) without doing anything when a required by-reference argument is NULL (`tools/make_cpptoc_impl.py`: `DCHECK(arg); if (!arg) return …`; DCHECKs are off in release builds).
- Use `CefString::from("")` (an owned empty buffer, valid per `string_types_impl.cc`) for out-params. The same applies to any required argument you pass as `None` (cf. #212).
- For callback-provided `Option<&mut CefString>` (a V8 exception, `on_tooltip` text, `redirect_url`…), use `.try_set("…")`. Assigning a new `CefString` is not written back: the V8 trampoline has no write-back for `exception`.

**G5. `CefString::clone()` is shallow and non-owning; so are clones of structs.** (VERIFIED, `string.rs`)

- `CefStringData::clone` copies the raw `{str, length, dtor}` into a `Borrowed` variant that never frees.
- A clone of a string CEF lent you (e.g. `Option<&CefString>`, or the `name`/`value` of a visited `Cookie`) dangles after the callback.
- A clone of a Rust-created string dangles when the original drops.
- Convert to `String` (`.to_string()`) to keep text.
- `CefStringList`/`Map` are worse: their raw types are zero-sized opaque structs, so cloning a borrowed list keeps a pointer to a local copy. `into_iter()` then yields nothing, and `Debug` reads a bogus pointer.
- To read a list CEF lends you, take it instead of cloning it: `std::mem::take(list).into_iter().collect::<Vec<String>>()` (compiled, `misc.rs`).
- `CefStringUserfree` returns (e.g. `frame.url()`) are owned. Use `CefString::from(&v).to_string()`.

**G6. Thread safety is not enforced.** (VERIFIED)

- `RefGuard<T>` is `unsafe impl Send + Sync` for every CEF type, so the compiler lets you send a renderer-thread `V8Value` anywhere.
- The `wrap_*` macros place no `Send`/`Sync` bounds on your fields.
- CEF calls `Client` getters, the request handler, `ResourceRequestHandler` and `ResourceHandler` on the IO thread (§1.4). Use `Arc` + `Mutex`/atomics everywhere, and keep V8 objects on their own thread.

**G7. Macro ergonomics** (VERIFIED; see §1.1).

- Unqualified names clash with GPUI's prelude, so keep CEF glue in its own module (clash INFERRED).
- Fields must be `Clone`, with no lifetimes (#450) and no struct attributes or docs.
- Impl blocks must follow the class hierarchy in order (#297). rustfmt and rust-analyzer can't see inside the macros.
- `self.clone()` leaks.
- `KeyboardHandler`'s `os_event` type is `sys::XEvent`; `cef::XEvent` doesn't exist.

**G8. Handler getters run on every event.** (VERIFIED) Cache handler objects in the client (§1.5).

**G9. `cef::args::Args`** (VERIFIED, `args.rs`)

- `Args::new()` iterates `std::env::args()`, which panics on non-UTF-8 arguments (documented std behaviour).
- `#[derive(Clone)]` copies the pointer vectors, which point into the *original* `CString`s and argv buffer. A clone dangles once the original is dropped.
- Keep one instance alive for the process, or use the compiled `OwnedArgs` (`misc.rs`, built on `args_os`).

**G10. The examples cut corners.** (VERIFIED)

- `cefsimple` passes `None` as the App to `execute_process` (§2.1).
- The `examples/osr` loop calls `do_message_loop_work()` at a fixed 17 Hz, ignoring `on_schedule_message_pump_work`.
- The OSR example leaks VRAM on resize on AMD/Vulkan/Wayland (#417, open).
- `osr`'s `OsrRenderHandler` holds `Rc<RefCell<…>>`, which is fine there only because it is UI-thread-only.

**G11. OSR specifics.** (VERIFIED)

- OSR is always Alloy style.
- `cef_types_linux.h` still says `shared_texture_enabled` is "only supported on Windows (D3D11)". That is **stale**: CEF 154 has a Linux native-pixmap (DMA-BUF) path in `libcef/browser/osr/video_consumer_osr.cc`, and cef-rs has `osr_texture_import::dmabuf`.
- It needs `--use-angle=gl-egl` + `--ozone-platform=x11` on NVIDIA (CEF's own sample).
- `on_accelerated_paint` handles are pooled. They must be imported or copied inside the callback and never cached (doc).
- `accelerated_osr` pins wgpu 30. GPUI 0.2.2 renders with blade (`cef-osr.md`), so the crate's importer won't plug into GPUI as is (INFERRED).

**G12. Wayland.** (VERIFIED from issues and CEF source)

- Chrome-style native embedding takes an X11 XID (`cef_window_handle_t` = `unsigned long`). Embedding Ozone/Wayland windows is CEF #2804, still open.
- Views windows and OSR work on Ozone/Wayland. CEF has Wayland code paths in `libcef/browser/views/window_impl.cc`.
- Open bug #4273: frameless Views windows get server-side decorations on KWin (CEF 152).
- CEF's `cefclient_gtk.cc` forces GTK onto XWayland: `gdk_set_allowed_backends("x11")`.

**G13. Message-loop pitfalls** (§2.4).

- With the external pump, iterate the GLib default context, and don't take its 0 ms prepare timeout.
- The multi-threaded loop disables GtkUi.

**G14. Build-script side effects** (VERIFIED, §3.1).

- About 1.5 GB is copied into each `target/<profile>`.
- The 326 MB archive stays in `CEF_PATH`.
- There is no rpath.
- The `dox` feature gives cheap CI checks.
- `cmake` is not needed on Linux.

**G15. Logs and paths** (VERIFIED doc).

- `debug.log` defaults to the executable's directory.
- `locale` is ignored on Linux.
- With no `root_cache_path`, CEF uses `~/.config/cef_user_data` and its process singleton. CEF warns at startup: "Please customize CefSettings.root_cache_path…" (`libcef/common/resource_util.cc`).

**G16. Shutdown order.** (INFERRED from CEF docs and the C-API rules)

1. Close every browser (`host.close_browser(1)`, or `try_close_browser` from a window-close handler) and keep pumping until each `on_before_close` has run.
2. Drop every `RefGuard` you hold: browsers, frames, request contexts, callbacks, `Registration`s, and the `Arc<Shared>` maps that contain them.
3. Then call `shutdown()` on the main thread.

Do not call CEF after `shutdown()`. `do_close` only runs for Alloy-style browsers (VERIFIED doc).

**G17. Default behaviour differs by style** (VERIFIED docs).

| Callback returning 0 | Alloy style | Chrome style |
|---|---|---|
| `on_before_download` | cancels | uses Chrome's download UI |
| `on_show_permission_prompt` | IGNORE | Chrome's prompt |

For extension compatibility in Chrome-style browsers, prefer **not** to override UI that extensions trigger (permission prompts, downloads, popups), or reproduce it faithfully (INFERRED).

---

## Verification log (VERIFIED, run 2026-10-16)

- Scratch crate `/tmp/vsesvit-scratch/cef-api`: about 1,400 lines covering every snippet in this file. It depends on `cef = { version = "=154.5.0", default-features = false, features = ["resources"] }`.
  - `cargo check` (default `dox` feature, no CEF download): **passes**.
  - `CEF_PATH=/home/dy/.cache/vsesvit-cef cargo check --no-default-features --features real`: **passes**. That run downloaded and extracted 154.0.34 to `/home/dy/.cache/vsesvit-cef/154.0.34/cef_linux_x86_64` in about 60 s and left the `.tar.bz2` there.
  - `cargo build` with the rpath `build.rs`: **links**. `readelf` shows `RUNPATH [$ORIGIN]` and `NEEDED libcef.so`. The binary was not executed.
  - Sanity check: deliberately breaking a call inside a `wrap_*!` method body (wrong argument type) produced E0308. The macro bodies are therefore really type-checked.
- `readelf -d` on `libcef.so` gave the NEEDED list and `RUNPATH [$ORIGIN]`. `readelf -S` shows `.debug_*` sections. `strip --strip-unneeded` produced 272,219,288 bytes; that temporary copy has since been deleted.
- Host checks: user-namespace limits as above; all NEEDED libraries resolve; `/usr/bin/cmake` is present but not needed.

---

## Sources

All CEF links point to branch `8037` at commit `14c5a089e8452874cb1a556dc3fde26ed2d87723`, the exact source of the 154.0.34 binary. Chromium links point to tag `154.0.8037.98`.

**Crate and repo** (read on disk and in a clone)

- crates.io: https://crates.io/crates/cef/154.5.0+154.0.34 · https://crates.io/crates/export-cef-dir · docs.rs: https://docs.rs/cef/154.5.0+154.0.34/cef/
- Repo: https://github.com/tauri-apps/cef-rs (crate source commit `2861f6991fae3ee31f877891382adcbc9b9f0953`)
  - `cef/src/rc.rs`, `cef/src/string.rs`, `cef/src/args.rs`, `cef/src/window_info.rs`, `cef/src/lib.rs`, `cef/src/bindings/x86_64_unknown_linux_gnu.rs`, `cef/src/wrapper/message_router.rs`, `cef/src/osr_texture_import/dmabuf.rs`, `cef/src/build_util/linux.rs`, `cef/src/bin/bundle-cef-app/linux.rs`, `cef/Cargo.toml`, `cef/CHANGELOG.md`
  - `sys/build.rs`, `sys/src/lib.rs`, `sys/wrapper.h`
  - `download-cef/src/lib.rs`, `export-cef-dir/src/main.rs`, `update-bindings/src/upgrade.rs`
  - `examples/cefsimple/src/{main.rs,shared/mod.rs,shared/simple_app.rs,shared/simple_handler/{mod.rs,linux.rs},bin/cefsimple_helper.rs}`
  - `examples/osr/src/{main.rs,webrender.rs}`
  - `examples/tests_shared/src/{browser/client_app_browser.rs,browser/main_message_loop_external_pump/{mod.rs,linux.rs},renderer/client_app_renderer.rs,common/client_app.rs}`
  - `.github/workflows/rust.yml`, `README.md`
- Issues:
  - open: #450 https://github.com/tauri-apps/cef-rs/issues/450 · #409 https://github.com/tauri-apps/cef-rs/issues/409 · #297 https://github.com/tauri-apps/cef-rs/issues/297 · #417 https://github.com/tauri-apps/cef-rs/issues/417
  - closed: #467 https://github.com/tauri-apps/cef-rs/issues/467 · #235 https://github.com/tauri-apps/cef-rs/issues/235 · #138 https://github.com/tauri-apps/cef-rs/issues/138 · #225 https://github.com/tauri-apps/cef-rs/issues/225 · #224 https://github.com/tauri-apps/cef-rs/issues/224 · #212 https://github.com/tauri-apps/cef-rs/issues/212 · #106 https://github.com/tauri-apps/cef-rs/issues/106 · #49 https://github.com/tauri-apps/cef-rs/issues/49

**CEF** (https://github.com/chromiumembedded/cef/tree/8037)

- Docs:
  - https://github.com/chromiumembedded/cef/blob/8037/docs/general_usage.md
  - https://github.com/chromiumembedded/cef/blob/8037/docs/api_versioning.md
  - https://github.com/chromiumembedded/cef/blob/8037/docs/using_the_capi.md
  - https://github.com/chromiumembedded/cef/blob/8037/docs/sandbox_setup.md
- Headers:
  - https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types.h (`cef_settings_t`, `cef_request_context_settings_t`, `cef_browser_settings_t`)
  - https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types_linux.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types_runtime.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/cef_browser.h
  - https://github.com/chromiumembedded/cef/blob/8037/include/cef_request_context.h
  - Per-class docs were read via the bindgen doc comments in `cef-dll-sys` and the distributed `include/capi/*.h`.
- libcef source:
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/browser_message_loop.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/ui_thread.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/context.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/common/resource_util.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/common/chrome/chrome_main_delegate_cef.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/common/api_version_util.h
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/common/string_types_impl.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/osr/video_consumer_osr.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/net_service/resource_request_handler_wrapper.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/browser_host_base.cc
- libcef_dll and translator:
  - https://github.com/chromiumembedded/cef/blob/8037/libcef_dll/libcef_dll2.cc
  - https://github.com/chromiumembedded/cef/blob/8037/libcef_dll/ctocpp/ctocpp_ref_counted.h
  - https://github.com/chromiumembedded/cef/blob/8037/tools/make_cpptoc_impl.py
  - https://github.com/chromiumembedded/cef/blob/8037/tools/make_ctocpp_impl.py
- Patches:
  - https://github.com/chromiumembedded/cef/blob/8037/patch/patches/linux_gtk_theme_3610.patch
  - https://github.com/chromiumembedded/cef/blob/8037/patch/patches/linux_assets_path_1936.patch
- Samples:
  - https://github.com/chromiumembedded/cef/blob/8037/tests/shared/browser/main_message_loop_external_pump_linux.cc
  - https://github.com/chromiumembedded/cef/blob/8037/tests/shared/browser/client_app_browser.cc
  - https://github.com/chromiumembedded/cef/blob/8037/tests/cefclient/cefclient_gtk.cc
- Issues:
  - #2804 https://github.com/chromiumembedded/cef/issues/2804 (Ozone/Wayland embedding)
  - #4273 https://github.com/chromiumembedded/cef/issues/4273 (Wayland frameless decorations)
  - forum, Wayland-only builds: https://www.magpcss.org/ceforum/viewtopic.php?t=18901
- Builds:
  - https://cef-builds.spotifycdn.com/index.json (fetched 2026-10-16)
  - The `README.txt` inside `cef_binary_154.0.34+g14c5a08+chromium-154.0.8037.98_linux64_minimal.tar.bz2`

**Chromium 154.0.8037.98** (https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/)

- `sandbox/linux/suid/client/setuid_sandbox_host.cc`, `content/browser/zygote_host/zygote_host_impl_linux.cc`, `docs/linux/suid_sandbox_development.md`
- `base/message_loop/message_pump_glib.cc`, `base/message_loop/message_pump_for_ui.h`, `base/base_paths.cc`, `base/base_paths_posix.cc`
- `chrome/browser/extensions/extension_service.cc` (`--load-extension` gating), `chrome/common/pref_names.h`, `components/language/core/browser/pref_names.h`, `components/spellcheck/browser/pref_names.h`, `extensions/browser/pref_names.h`

**Rust**

- Cargo build scripts (the `links` metadata goes to immediate dependents only; `rustc-link-arg-bins`): https://doc.rust-lang.org/cargo/reference/build-scripts.html
- `std::env::args` panics on non-Unicode: https://doc.rust-lang.org/std/env/fn.args.html

**Related reports in this folder:** `cef-osr.md`, `cef-windowing.md`, `cef-extensions.md`, `gpt-extension-architecture.md`, `prior-art.md`.
