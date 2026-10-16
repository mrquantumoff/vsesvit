# Behavior inventory: shell core (startup, CLI, profiles, browser controller, session, keymap)

What the current Linux shell does in these files, so a GPUI + CEF shell can reproduce every
user-visible behavior without reading them:

- `crates/vsesvit-gtk/src/lib.rs`, `app.rs`, `cli.rs`, `browser.rs`, `profile.rs`, `profiles.rs`,
  `session.rs`, `keymap.rs`, `motion.rs`
- `crates/vsesvit/src/main.rs`, `crates/vsesvit/build.rs`, `crates/vsesvit/Cargo.toml`,
  `crates/vsesvit-gtk/Cargo.toml`

Read from the `linux-cef-gpui` worktree at commit `27896e0`. I also read the core modules these
files call (`vsesvit-core/src/{lib.rs,profiles.rs,session.rs,private.rs,prefs.rs,onboarding.rs,
shortcuts.rs,memory_saver.rs,https_only.rs,tab_search.rs,sync.rs}`), and the neighbours they
lean on (`extensions.rs`, `engine.rs`, `updates/{mod,headless}.rs`, `closed_tabs.rs`,
`location.rs`, parts of `window/{mod,menu,actions,groups}.rs`, `tab.rs`, `self_test.rs`,
`dialogs/{mod,settings}.rs`), plus `docs/PLAN.md` and `docs/design/self-test.md`. Those
neighbours belong to other inventories. They appear here only where these files depend on them.

Conventions:

- Everything below was checked against the code unless it is marked **(inferred)**.
- Tags on behavior that touches the engine or the toolkit:
  - **(KEEP)**: product behavior the rewrite must reproduce.
  - **(WEBKIT-ONLY)**: a WebKitGTK workaround or limitation that goes away with Chromium.
  - **(NEEDS-CEF)**: needs a CEF equivalent. Where I name a CEF API, I confirmed it exists in
    `cef-dll-sys-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs`. C++ names are
    `CefXxx::Method`, which is `cef_xxx_t::method` in the bindings.
  - **(GTK-ONLY)**: GTK or libadwaita mechanics that go away with GPUI.
  - **(NEEDS-GPUI)**: the GPUI shell must provide an equivalent.
- "Core" means `vsesvit_core`. `Profile` is `vsesvit_core::Profile`.

---

## 1. The binary crate `vsesvit`

### `crates/vsesvit/src/main.rs`
- `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]`: Windows
  release builds have no console. This has no effect on Linux.
- `fn main() -> ExitCode` returns `vsesvit_gtk::run()` on Linux and `vsesvit_winui::run()` on
  Windows. Any other OS prints `vsesvit supports Windows and Linux only` to stderr and exits
  with `FAILURE`.
- The rewrite swaps `vsesvit_gtk::run()` for the new shell's entry point, which must also return
  `std::process::ExitCode`.
- **(NEEDS-CEF)** CEF runs subprocesses (renderer, GPU, utility) from the same executable unless
  `CefSettings.browser_subprocess_path` names a helper. So `main` (or the first lines of the
  shell's `run`) must call `cef_execute_process` before anything else and return its exit code
  for subprocesses. Today nothing in `main` handles subprocess arguments: the CLI parser would
  reject CEF's `--type=renderer` as `unknown option` and exit 2 (see §3). **(inferred)** from the
  CEF process model.

### `crates/vsesvit/build.rs`
- It prints `cargo::rerun-if-changed=../../packaging/icons/dev.mrquantumoff.vsesvit.ico` on every
  target.
- On Windows only, `winresource::WindowsResource` embeds the icon with `ProductName` and
  `FileDescription` set to "Vsesvit". It does nothing for Linux.

### `crates/vsesvit/Cargo.toml`
- Package `vsesvit`, described as "The Vsesvit web browser", with workspace version, edition,
  rust-version and licence. It has one `[[bin]] name = "vsesvit"`.
- Target-specific dependencies: `vsesvit-gtk` on `cfg(target_os = "linux")`, `vsesvit-winui` on
  `cfg(windows)`. Windows has `winresource = "0.1.31"` as a build dependency.
- The rewrite changes the Linux dependency line.

### `crates/vsesvit-gtk/Cargo.toml`
- Its description reads "Linux shell of the Vsesvit browser: GTK4 + libadwaita + WebKitGTK 6.0".
- Features: `default = ["self-test"]`, `self-test = ["vsesvit-core/testkit"]`. The self-test is
  a default feature so that it runs against the same optimized binary that ships. A packager who
  does not want the test key material builds with `--no-default-features`. **(KEEP)** this
  feature contract.
- Dependencies, all Linux-only:
  - `vsesvit-core`, `vsesvit-webext`, `vsesvit-update`, `vsesvit-sync`
  - `gtk` (gtk4 0.11, feature `v4_22`), `adw` (libadwaita 0.9, `v1_9`), `webkit` (webkit6 0.6,
    `v2_52`), `javascriptcore` (0.6)
  - `serde`, `serde_json`, `log`, `url`, `semver = "1"`, `futures-channel 0.3`,
    `futures-util 0.3` (without default features)
  - `glib 0.22` with feature `log`, which routes the `log` crate into `g_log`
  - `gio 0.22` with feature `v2_80`, for `ApplicationCommandLine::printerr_literal`
- Dev dependencies: `vsesvit-core` with `testkit` (fixture servers), and `rusqlite 0.40`
  (`bundled`), which holds the profile database during an uninstall test.
- **(GTK-ONLY / WEBKIT-ONLY)** Every gtk, adw, webkit, javascriptcore, glib and gio dependency
  goes. `vsesvit-webext` probably goes too. `vsesvit-core`, `vsesvit-update` and `vsesvit-sync`
  stay. The workspace `Cargo.toml` declares `gtk`, `adw`, `webkit`, `javascriptcore`,
  `vsesvit-gtk` and `vsesvit-webext` as workspace dependencies, and those lines need editing.
  `.github/workflows/release.yml:130` installs `libgtk-4-dev libadwaita-1-dev
  libwebkitgtk-6.0-dev`.

---

## 2. Process startup: `vsesvit_gtk::run()` (lib.rs)

The order is exact:

1. **Logging.** `log::set_logger(&LOGGER)`, where
   `LOGGER = glib::GlibLogger::new(GlibLoggerFormat::Plain, GlibLoggerDomain::CrateTarget)`.
   Then `log::set_max_level(Debug)`. Every `log::` call becomes a GLib log message whose domain
   is the record's target (the crate or module path). **(inferred, GLib defaults)** Warnings and
   errors reach stderr or the journal. Info and debug show only with `G_MESSAGES_DEBUG` naming
   the domain or `all`. See §19.
2. **Parse arguments.** `std::env::args_os()` is collected. `cli::parse(args.skip(1))` runs on the
   arguments after the program name.
   - On error it prints `vsesvit: {error}\n\n{USAGE}` to stderr and exits with code 2.
3. **Dispatch** on `cli::Command`:
   - `Help`: prints `USAGE` to stdout and exits 0.
   - `Version`: prints `vsesvit {CARGO_PKG_VERSION}` to stdout (for example `vsesvit 0.1.0`) and
     exits 0.
   - `CheckForUpdates`: `updates::headless::check()`. See §4.
   - `Update`: `updates::headless::update()`. See §4.
   - `SelfTest { out_dir, network }`: `self_test::run(&out_dir, network)` when the `self-test`
     feature is on. Without it, prints `vsesvit: this build was made without the `self-test`
     feature` and exits with `FAILURE`.
   - `Browse { profile_dir, targets }`: `app::run(profile_dir, !targets.is_empty(), &args)`.
     Note that `app::run` receives the whole argv (`args`), which includes the program name and
     `--profile-dir`.

The constant `APP_ID = "dev.mrquantumoff.vsesvit"` is public. It is the D-Bus and GApplication id
of the Default profile, the desktop file name
(`packaging/linux/dev.mrquantumoff.vsesvit.desktop`), the icon name, `StartupWMClass` and the
Flatpak app id. **(KEEP)** as the Wayland `app_id` / X11 `WM_CLASS`, so the desktop file matches
the windows. **(inferred)** GTK sets the toplevel's app id from the GApplication id, so windows of
a non-default profile carry `dev.mrquantumoff.vsesvit.Profile<hash>` today, and the desktop entry
does not match them.

---

## 3. The command line (cli.rs)

### Grammar and usage text **(KEEP)**
`USAGE`, verbatim:
```
Usage: vsesvit [URL...] [--profile-dir DIR]
       vsesvit --self-test OUT_DIR [--network]
       vsesvit --check-for-updates
       vsesvit --update

Options:
  --profile-dir DIR     keep browsing data in DIR instead of the default profile
  --self-test OUT_DIR   run the end-to-end self-test on a fresh profile and write
                        report.json and window.png to OUT_DIR
  --network             with --self-test: also install from the Chrome Web Store
  --check-for-updates   print the newest available version as JSON, without opening a window
  --update              download and install the newest version, then print the outcome as JSON
  -h, --help            show this help
  --version             show the version
```

`enum Command`:
- `Browse { targets: Vec<OsString>, profile_dir: Option<PathBuf> }`
- `SelfTest { out_dir, network }`
- `CheckForUpdates`, `Update`, `Help`, `Version`

### Parsing rules
- An argument that is not valid UTF-8, does not start with `-`, or is exactly `-` is a **target**.
  Targets and options can come in any order.
- `--` ends option parsing. Everything after it is a target, including `--weird-file-name`.
- `-h` or `--help` returns `Help` at once. `--version` returns `Version` at once. Both win over
  anything else, so `vsesvit https://x --help` is `Help`.
- `--profile-dir DIR` and `--profile-dir=DIR` are both accepted.
  - The value must be non-empty, so `--profile-dir=` fails with `MissingValue`.
  - Repeating the option is an error.
- `--self-test OUT_DIR` and `--self-test=OUT_DIR` are standalone. `--network` may come before or
  after it.
- `--check-for-updates` and `--update` are standalone and take no `=value`.
  `--update=now` is `unknown option --update=now`.
- Standalone commands (`--self-test`, `--check-for-updates`, `--update`):
  - The same one twice is `Repeated`.
  - Two different ones is `Exclusive(first)`.
  - Any of them together with targets or `--profile-dir` is `Exclusive(name)`.
- `--network` without `--self-test` is an error, even next to `--check-for-updates`.
  `--network` twice is `Repeated`.
- Any other `-x` or `--x` is `UnknownOption(text)`.

Error messages (the `Display` for `CliError`):

| Error | Message |
|---|---|
| `MissingValue` | `{opt} needs a value` |
| `Repeated` | `{opt} was given more than once` |
| `UnknownOption` | `unknown option {opt}` |
| `Exclusive` | `{opt} cannot be combined with URLs or other options` |
| `NetworkWithoutSelfTest` | `--network needs --self-test` |

The unit tests in `cli.rs` pin all of this down. They are a ready acceptance list:
`no_arguments_browses_without_targets`, `urls_and_profile_dir_in_any_order`,
`self_test_takes_an_output_dir_and_a_network_flag`, `self_test_is_exclusive_and_network_needs_it`,
`update_commands_run_alone`, `option_values_are_required_and_unique`,
`unknown_options_are_rejected_but_dash_dash_ends_options`, `help_and_version`.

`pub(crate) const PROFILE_DIR = "--profile-dir"` is reused when building relaunch and launch
command lines (§6, §9).

**(NEEDS-CEF)** Chromium switches (`--type=...`, `--enable-features=...`, and others) would hit
`unknown option`. The new entry point must route subprocess invocations to
`cef_execute_process` before calling `cli::parse`. If the browser process should accept Chromium
switches for debugging, the parser needs a decision about them. **(inferred)**

---

## 4. Headless updater commands (`--check-for-updates`, `--update`)

`updates/headless.rs` is shown here because `run()` dispatches to it.

- `check()` calls `vsesvit_update::cli::print(vsesvit_update::cli::check(&Installation::detect(),
  &current_version(), UpdateChannel::of_build().name()))`.
- `update()` calls `cli::print(cli::update(&Installation::detect(), &current_version(),
  UpdateChannel::of_build().name(), &download_dir()))`.
- Output is one JSON line on stdout, shaped
  `{"installation": ..., "current": ..., "available": {...}|null, "installed": {...}|null}`, or
  `{"error": "..."}` with exit 1. Download progress goes to stderr. `vsesvit_update::cli`'s module
  doc is the contract.
- Neither command opens a profile or a display. Both follow the channel the build was released
  on (`UpdateChannel::of_build()`), never the `updates.channel` preference, and never read
  `updates.automatic`.
- `current_version()` is `semver::Version::parse(env!("CARGO_PKG_VERSION"))`.
- `download_dir()` is `glib::user_cache_dir()/vsesvit/updates`, which is
  `~/.cache/vsesvit/updates`.
- `Installation::detect()` reads the marker file `package-format` next to the canonicalized
  executable (`deb`, `rpm`, `pacman`, `appimage`, `flatpak`; `nsis` on Windows) plus `$APPIMAGE`.
- **(KEEP)** all of it. It does not touch the engine and needs no display. The only GLib call is
  `user_cache_dir`, which becomes `$XDG_CACHE_HOME` or `~/.cache` (`directories`/`dirs`).

---

## 5. Profile resolution (profile.rs)

### `Start::resolve(profile_dir: Option<PathBuf>, has_targets: bool) -> io::Result<Start>`
1. `profiles = ProfilesDir::standard()`, the parent of `Profile::default_root`, which is
   `directories::ProjectDirs::from("", "", "Vsesvit").data_local_dir()/profiles`, or
   `~/.local/share/vsesvit/profiles` on Linux.
2. If `--profile-dir` was given, that directory is used as is (no picker).
3. Otherwise:
   - `profiles.sweep()` deletes the data of removed profiles that no process holds.
   - Then `profiles.startup(has_targets)`:
     - `Startup::Open(id)` gives `dir = profiles.root(&id)`.
     - `Startup::Picker` returns `Start::Picker(profiles)` (§12).
   - Core decides (`Registry::startup(has_urls, any_running)`). The picker shows only if all of
     these hold:
     - there is more than one profile
     - `show_picker` is on (the default)
     - the launch has no URLs
     - no listed profile is running (`ProfilesDir::is_running`, which try-locks each `LOCK`)
   - Otherwise the profile opened is:
     - `last_used`, if it is still listed
     - else `Default`, if listed
     - else the first listed profile
     - else `Default`
4. `fs::create_dir_all(&dir)`, then `root = fs::canonicalize(&dir)`. The directory is created if
   missing, and canonicalized so that two spellings of one directory share one instance.
5. `home = profiles.locate(&root).map(|id| Home { dir: profiles, id })`. A directory directly
   inside the standard profiles directory is a listed profile with a `Home`. Any other
   `--profile-dir` has no `Home`: it has no profile button, no profile menu, and never appears in
   `profiles.json`.
6. `app_id = app_id_for_profile(&root, &profiles.root(Default))`:
   - `APP_ID` if `canonicalize(default) == root`, however the path was spelled
   - else `format!("{APP_ID}.Profile{}", &sha256_hex(root bytes)[..16])`, computed with
     `glib::compute_checksum_for_data(Sha256, ...)` over the canonical path's raw bytes
     (`OsStrExt::as_bytes`)

On error the caller (`app::run`) prints `vsesvit: cannot use the profile directory: {e}` and
exits with `FAILURE`.

### `ProfileLocation { root, app_id, home }`
`relaunch_args()` returns `["--profile-dir", root]`. Restart-after-update uses it (§9).

### `profile::launch(widget, root) -> Result<(), glib::Error>`: start another profile's process
- `program()` is the AppImage file (`Installation::AppImage { image }`, which is `$APPIMAGE`)
  because the mount goes away with this process. Otherwise it is `std::env::current_exe()`.
- It builds the command line `"<program>" --profile-dir "<root>"`, each part shell-quoted, and
  `gio::AppInfo::create_from_commandline(cmd, Some("Vsesvit"), SUPPORTS_STARTUP_NOTIFICATION)`.
  It launches that with the widget's display `AppLaunchContext`. On Wayland this passes an XDG
  activation token, so the process that shows a window may take focus. On X11 it is a startup
  notification id.
- If the profile already runs, the new process finds it locked and forwards its empty command
  line (§7). The running profile then raises its window.
- **(NEEDS-GPUI)** Spawning with an activation token needs `xdg-activation-v1`: request a token
  from the compositor (with the triggering input serial) and pass it as `XDG_ACTIVATION_TOKEN`,
  or `DESKTOP_STARTUP_ID` on X11. **(inferred)** GPUI may not expose this. The fallback is a plain
  spawn, which loses focus-stealing permission.
- Errors show a toast `Could not open the profile: {e}` (in `Browser::open_profile` and the
  picker).

### `profile::notify(root)`
- Spawns `program --profile-dir root` with `std::process::Command`, with no activation token on
  purpose. This only makes the running process read the profile list again, for example after
  its profile was removed elsewhere.
- On failure it logs
  `telling the profile at {root} to look at the profile list: {e}`.

### `profile::downloads_dir()`
`glib::user_special_dir(Downloads)`, else `~/Downloads`. It is the default for the
`downloads.directory` pref (`None` = the platform's Downloads folder, which only the shell knows).
**(NEEDS-GPUI)** Use an `xdg-user-dirs` lookup instead (`dirs::download_dir()`).

### `type Core = Rc<RefCell<Profile>>`
The open profile is borrowed for exactly one call and never across a call into the toolkit, so a
borrow is never re-entered. **(KEEP)** this rule. Core never calls back into the shell.

---

## 6. Profile opening and locking (app.rs `run` and core `Profile::open`)

`app::run(profile_dir, has_targets, args)`:
1. `Start::resolve`. A picker result returns `profiles::run_picker(dir)`. This process then never
   opens a profile.
2. `Profile::open(&location.root, OpenOptions::default())`:
   - `Ok(profile)`: kept as `Some`.
   - `Err(OpenError::Locked)`: becomes `None`. Another process owns the profile. The application
     still runs so that GApplication can forward this command line to it (§7).
   - Any other error prints `vsesvit: cannot open the profile at {root}: {e}` and exits with
     `FAILURE`. Messages from core:
     - `TooNew`: `profile schema {found} is newer than this build supports ({supported})`
     - `Db` and `Io` errors pass through unchanged.
3. If the profile has a `Home`, opened, and the list marks it removed
   (`home.dir.load().is_removed(&home.id)`), it was removed while another process ran it:
   - drop the profile, which releases `LOCK`
   - `home.dir.sweep()` deletes its data now
   - print `vsesvit: the profile at {root} was removed` and exit with `FAILURE`

Core's locking contract (`Profile::open`, verified in `vsesvit-core/src/lib.rs`) **(KEEP)**:
- It creates `root`, opens `root/LOCK`, and takes the exclusive OS lock (`File::try_lock`). It
  retries every 10 ms for `profiles::LOCK_PATIENCE = 100 ms`, then returns `OpenError::Locked`
  (message: `the profile is open in another Vsesvit process`). The OS releases the lock when the
  process dies, so there is never a stale lock.
- It creates `extensions/`, `staging/` and `engine/`, opens and migrates `vsesvit.db`, loads
  bookmarks, wipes staging, prunes favicons.
- `ProfilePaths`:
  - `engine_data = <root>/engine` is the WebKit `NetworkSession` data directory today.
  - `engine_cache` is `~/.cache/vsesvit/<name>/` for a profile in the default location, else
    `<root>/cache`.
  - **(WEBKIT-ONLY)** in meaning. **(NEEDS-CEF)** CEF needs `CefSettings.root_cache_path`. Every
    `CefRequestContextSettings.cache_path` must equal it or be its child, and CEF takes a process
    singleton lock on `root_cache_path` (§7). `ProfilesDir::delete` (removing a profile) deletes
    `engine_cache` when it lies outside the root, so a new cache location must be added to that
    deletion.
- `Profile::is_new()` is true only on the open that created the profile. It drives the welcome
  (§13).
- `OpenOptions::default().chrome_version = extensions::DEFAULT_CHROME_VERSION` is sent to the Web
  Store as `prodversion`. The Linux shell keeps the default. **(NEEDS-CEF)** It should become the
  real CEF Chromium version (154.x), as Windows passes WebView2's.

---

## 7. Single instance and forwarding command lines between processes

### Today: one GApplication per profile on the session bus **(GTK-ONLY mechanism, KEEP behavior)**
- `adw::Application::builder().application_id(&location.app_id)
  .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)`. There is one bus name per profile: the
  Default profile has `APP_ID`, others have the hashed id.
- The profile is opened before the application registers on the bus.
  - **Primary instance** (owns the bus name). `startup` runs `startup(app, slot, profile, home)`
    only if the profile was opened (`pending.take()`). `command-line` then runs in the primary
    for its own argv and for every later invocation's argv.
  - **Remote instance** (the name is already owned). GApplication sends argv, cwd and environ to
    the primary's `command-line` handler and exits with the status the primary returns.
    `startup` never runs here.
  - **Locked but unreachable.** The profile is locked (`None`) but nobody owns the bus name, for
    example when the lock holder is the self-test, which uses another app id, or a process that
    lost its bus connection. This process becomes primary with no browser, and `command-line`
    prints `vsesvit: the profile is in use by another Vsesvit process that could not be
    reached` through `printerr_literal` and returns `FAILURE`.
- `activate` (raised by D-Bus activation, not by a normal command line) calls
  `browser.present()`.
- `app.run_with_args_os(args)` runs the loop. `status.into()` is the process exit code.

### `open_from_command_line(browser, command_line)`, in the primary
1. `browser.close_if_removed()`: if this profile was removed from the list, quit without saving
   and return `SUCCESS`. This is how `profile::notify` closes a removed profile's process.
2. Re-parse the forwarded argv with `cli::parse(arguments().skip(1))`:
   - `Browse { targets }` gives targets.
   - Any other command gives none.
   - A parse error prints `vsesvit: {e}` to the remote's stderr and returns `FAILURE`.
   - `--profile-dir` in a forwarded argv is ignored here, because it only chose the instance.
3. `cwd = command_line.cwd()`, else `/`. Each target goes through
   `location::resolve_cli_target(target, &cwd, Path::is_file)`:
   - an existing file relative to the *invoking* process's cwd (or absolute) becomes a `file://`
     URL
   - else `vsesvit_core::search::classify_url(text)`: a URL with a navigable scheme as is
     (`http`, `https`, `file`, `about`, `data`, `view-source`, `vsesvit`), host-like text with a
     scheme added (`example.com/path` gives `https://example.com/path`, `localhost:8080` gives
     `http://localhost:8080/`, `[::1]:8080` gives `http://[::1]:8080/`)
   - anything else (`rust lifetimes`, `mailto:...`, `./missing.html`) resolves to nothing and
     prints `vsesvit: cannot open {target}` to the remote's stderr. The other targets still open.
   - The command line never searches.
4. `hold = app.hold()` keeps the app alive while nothing is shown. Then
   `browser.when_blockers_applied(...)` waits until tracking protection's and the cookie rules'
   blockers are on the tabs and the data of sites set to clear has been deleted, so no page of
   the restored session or the command line loads before them (`Trackers::when_applied`, then
   `Cookies::when_applied`). Invocations arriving meanwhile queue behind the first. Then:
   - **no normal window open** (a fresh start, or only private windows): run
     `browser.open_startup_windows(&urls)`. The browser starts afresh, in normal windows, as
     Chrome does (§14.3).
   - **normal windows open, no URLs**: `browser.present()` raises the most recently focused
     normal window.
   - **normal windows open, with URLs**: `browser.open_window(Normal, &urls)` opens a **new
     normal window** with one tab per URL, the first selected. This differs from Chrome, which
     adds tabs to the last window. It is current behavior.
   - Then drop the hold. Return `SUCCESS` immediately, before the windows open.
5. Pages from outside a window (another invocation, an extension, sync) never land in a private
   window (`Browser::normal_window`).

### What the rewrite must reproduce **(KEEP)**
- One process per profile. A second launch on the same profile, including another spelling of
  its path, hands over its targets and cwd and exits. The exit status reflects the primary's
  answer.
- Targets resolve against the invoker's cwd. Only the targets matter in a forwarded command line.
- Different profiles run as separate processes at the same time.
- A launch with an empty forwarded command line raises the running profile's window. That is how
  the profile menu and the picker switch to a running profile.
- A launch that finds the profile locked and cannot reach the owner fails with the "could not be
  reached" message.

### CEF equivalent **(NEEDS-CEF)**
- CEF 120+ takes a process singleton lock on `CefSettings.root_cache_path`. The doc comment on
  `_cef_settings_t::root_cache_path` says so, and that "Multiple application instances writing to
  the same root_cache_path ... a process singleton lock ... is used". On relaunch with the same
  `root_cache_path`, `cef_initialize` forwards the new launch's arguments to the running process
  and returns false for an early exit.
- The running process gets
  `CefBrowserProcessHandler::OnAlreadyRunningAppRelaunch(command_line, current_directory)` on the
  UI thread. Returning true means handled. The default, returning false, opens a new
  Chrome-styled window, which is wrong for Vsesvit.
- That maps onto today's design if `root_cache_path` is per profile (for example
  `<profile>/cef` or the existing `engine/`). It gives argv and cwd, as GApplication does.
- Caveats **(inferred)**:
  - Core's `LOCK` (taken in `Profile::open`) and CEF's singleton are two locks. Their order must
    be designed so that the second process forwards rather than failing on `Locked`.
  - `cef_execute_process` and `cef_initialize` need the profile's `root_cache_path` before core
    opens the profile, so profile resolution (§5) has to run first.
  - CEF's forwarding hands over the full argv. Whether the exit status can carry the primary's
    answer (`FAILURE` for "cannot open") is unknown. GApplication carries it today.
  - The picker (§12) and the headless commands must not initialize CEF.
- The "locked but unreachable" case would need its own handling.

---

## 8. Application setup and app-level actions (app.rs)

### `app::setup(app, slot)`
Used by startup and by the self-test.
- `glib::set_application_name("Vsesvit")` **(KEEP)** as the app's display name.
- `gtk::Window::set_default_icon_name(APP_ID)` **(KEEP)**: windows use the
  `dev.mrquantumoff.vsesvit` icon.
- `load_css()` installs `CSS + profiles::avatar_css()` on the default display at
  `STYLE_PROVIDER_PRIORITY_APPLICATION`, plus `window::install_group_style(display)` for the tab
  group colours. **(GTK-ONLY)** mechanism. The visual values are listed in Appendix A as
  **(KEEP)** design reference.
- `install_actions(app, slot)` registers the actions below. Debug builds add the debug actions.

### `app::startup(app, slot, profile, home)`
`setup`, then `Browser::new(app, profile, home)`, then `browser.start()`, then
`slot.replace(Some(browser))`.

`type Slot = Rc<RefCell<Option<Browser>>>`. The browser lives here only between startup and the
end of `run`, so nothing the application's closures hold keeps it alive past shutdown.

### App actions **(KEEP behavior; GTK-ONLY as `app.*` GActions)**

| Action | Parameter | Does | Bound to |
|---|---|---|---|
| `app.new-window` | none | `browser.open_window(Normal, &[])`: a new normal window with a new tab page | Ctrl+N (keymap); main menu "New _Window" |
| `app.new-private-window` | none | `browser.open_window(Private, &[])` | Ctrl+Shift+N; main menu "New _Private Window" |
| `app.about` | none | `dialogs::about::present(active window)` | main menu "_About Vsesvit" |
| `app.welcome` | none | `dialogs::welcome::present(active BrowserWindow)` | main menu "_Welcome" |
| `app.shortcuts` | none | `dialogs::shortcuts::present(active window)`, the keyboard shortcuts help | Ctrl+? (`ShowShortcuts`); main menu "_Keyboard Shortcuts" |
| `app.extension-notification` (`vsesvit_webext::notifications::ACTION`) | `(sss)` = (extension id, notification id, activation) | `Activation::parse(activation)`: `"settings"` opens `dialogs::extensions::present_extension(active window, ext)`; `"default"` (click) and `"<n>"` (button index) call `browser.runtime().notification_activated(ext, id, activation)` | GNotification buttons and default action |
| `app.quit` | none | `browser.save_session_now()` while the windows still exist, then `app.quit()` | Ctrl+Q only. There is no Quit menu item. |
| `app.update` | none | Registered by `updates::Updates::new`, not `install_actions`. Steps the update lifecycle (`Event::Activate`: install or restart) | update banner button; Settings > General button |

- **(WEBKIT-ONLY / NEEDS-CEF)** `app.extension-notification` belongs to the home-grown runtime's
  `chrome.notifications` over GNotification. With Chromium's extension system, notifications are
  Chromium's. **(inferred)** Chrome on Linux uses libnotify/portal via its own code, and how that
  works in CEF needs research.
- Debug builds only (`#[cfg(debug_assertions)]`), driven over D-Bus `org.gtk.Actions`. These are
  developer tools, not user features. **(GTK-ONLY)** Keep equivalents only if the scripts that
  use them survive. No script in the repo calls them.
  - `app.debug-screenshot(s path)`: captures the active window with the in-app renderer
    (`screenshot::save_png`), times out after 5 s, logs `debug-screenshot: wrote {path}` or a
    warning.
  - `app.debug-apply-sync(s path)`: reads a JSON array
    `[{"kind": "...", "id": "...", "body": {...}}]`. Kinds: `bookmarks`, `history_pages`,
    `history_deletions`, `sessions`, `extensions`, `ext_storage_sync`, `prefs`,
    `search_engines`, `site_permissions`. Each becomes a `vsesvit_core::sync::WireRecord { kind,
    id, body }`. It calls `core.sync().apply(records)`, then `browser.sync_applied(&report.changed)`,
    and logs `merged {}, unchanged {}, rejected {}: {:?}`.
  - `app.debug-set-tabs-position(s left|right|top)`: `browser.set_tabs_position(...)`.
  - `app.debug-close-dialog()`: closes the active window's dialog, or the active window when it
    is a Bookmarks, History or Downloads window. Otherwise it warns `debug-close-dialog: no
    dialog is open`.

### `install_actions` interaction
The keymap (§16) sets the accelerators for `app.new-window`, `app.new-private-window`,
`app.quit` and `app.shortcuts` application-wide, together with the `win.*` ones.

---

## 9. Shutdown, quit, restart, profile release (app.rs, browser.rs)

- **Quit** (`app.quit`, Ctrl+Q): `save_session_now()` first, because the windows are gone by the
  time `shutdown` runs. Then `app.quit()`.
- **The last window closes**: GApplication ends when its last window closes and nothing holds
  it. Session saving on window close is in §15.
- **`shutdown` signal**: `browser.shutdown()`:
  1. `save_session_now()`, a no-op if no normal window is left or it already shut down
  2. `shut_down = true`. Nothing writes the session after this, so tearing windows down for a
     restart cannot overwrite it.
  3. If the profile was removed, return.
  4. Otherwise `cookies().clear_at_exit()` deletes the data of sites set to "Clear on exit" and
     "Block", **(KEEP)**.
  5. `sync.final_sync()`: one sync round of what changed, bounded by `FINAL_SYNC_WAIT`.
- **After `app.run` returns** (back in `app::run`):
  1. If the browser was removed (`Browser::was_removed`), `release_profile`, then
     `home.dir.sweep()` deletes this profile's data now that the lock is free, and the process
     exits with the run status.
  2. If `browser.restart_program()` is `Some(program)` (the user chose to restart into an
     installed update), `release_profile`, then
     `std::process::Command::new(program).args(["--profile-dir", root]).spawn()`. It logs
     `restarted into {program}` or warns `could not start {program}: {e}`. The restart reopens
     the same profile only: no URLs, and the session restores according to the `startup` pref.
     The spawn happens after `run`, so the new process becomes primary instead of forwarding to
     this one.
- **`release_profile(app, slot)`**:
  1. Take the browser out of the slot and drop it.
  2. For every app window, force-close a visible `adw::Dialog`, then destroy the window.
  3. Iterate the main context for up to 2 s.
  4. If the `Profile` `Rc` is still alive, warn `the profile is still open; the restarted browser
     may find it locked`.
  - Purpose **(KEEP)**: the profile's `LOCK` must be released before the next process starts.
    Test: `a_restart_frees_the_profile_before_the_new_process_starts`, which runs in a child
    process because the webext runtime allows one per process.
- **(NEEDS-CEF)** With CEF, release also means closing every browser (`try_close_browser` /
  `CloseBrowser(force)`), waiting for `CefLifeSpanHandler::OnBeforeClose`, then `cef_shutdown`
  so that CEF's singleton and cache locks drop before the relaunch. **(inferred)**

---

## 10. Popover helper and scripted mode (lib.rs)

- `thread_local SCRIPTED: Cell<bool>` is set by the self-test. Effects:
  - popovers open without autohide (below)
  - no welcome dialog on the first run (`Browser::new`)
  - no scheduled extension update checks (`Browser::start`)
  - `permissions.rs` also turns it off temporarily
- `popup(popover, anchor)` opens a menu or bubble from `anchor`. The anchor holds it until it
  closes or until the anchor leaves the window (a rebuilt bar, a closed window), whichever comes
  first. The popover is unparented on the next idle after `closed`, and the anchor's `unrealize`
  handler is disconnected then. With `SCRIPTED`, `autohide` is off, because a Wayland popup grab
  needs a real input event.
  - **(KEEP)** behavior: a menu closes and leaves nothing behind when its anchor goes away.
  - **(GTK-ONLY)** mechanism. **(NEEDS-GPUI)** popover and menu anchoring, plus a scripted mode
    in which popovers stay open for capture.
  - Test: `a_popup_leaves_nothing_on_its_anchor_once_it_closes`.

---

## 11. The browser controller (browser.rs)

`Browser(Rc<Inner>)` is the one controller per process. Windows and tabs call it for everything
that touches data or policy.

### 11.1 State (`Inner`)
- `app: adw::Application`, `core: Core`, `engine: Engine` (the WebKit `NetworkSession` and
  `Settings`), `runtime: vsesvit_webext::Runtime`, `trackers: Trackers`, `cookies: Cookies`,
  `downloads: Rc<Downloads>`.
- `https_reach: Cell<Reach>`, `Public` by default. The self-test sets `Everywhere` through
  `set_https_reach`, which exists only with the self-test feature.
- `closed_tabs` and `private_closed_tabs`: two `ClosedTabs<ClosedTab>` stacks, capacity
  `CLOSED_TABS_KEPT = 25` each.
- `private: Option<PrivateEngine { session, downloads handler }>`, which lasts as long as the
  private session.
- `extension_errors: HashMap<ExtensionId, String>`, `updating_extensions: Cell<bool>`, and
  `extension_views` (weak refresh callbacks).
- `next_tab_id` and `next_window_id` (`u32`, from 1). These are the ids `chrome.tabs` and
  `chrome.windows` see (`vsesvit_webext::TabId`, `WindowId`).
- `last_use: Cell<u64>`: the "most recently used" clock for tab search (§11.6).
- `session_save: Option<SourceId>`, `shut_down: Cell<bool>`.
- `favicon_fetch: Cell<FetchState { Idle, Running, RunningStale }>`.
- Observer lists, all weak, each refreshed when its data changes: `bookmark_views`,
  `history_views`, `pref_views` (closures returning false once their widgets are gone),
  `profile_views`.
- `windowed: Vec<(Windowed, WeakRef<adw::Window>)>`: one Bookmarks window, one History window,
  and one Downloads window per `Browsing`, each a separate top-level window while open.
- `updates: Option<Updates>` (`None` when this copy does not update itself), `sync: Syncer`.
- `welcome: Cell<bool>`, `home: Option<Home>`, `profiles: RefCell<Registry>` (the profile list as
  last read), `removed: Cell<bool>`.

### 11.2 Construction: `Browser::new(app, profile, home)`, in order
1. `profiles = home.dir.opened(&home.id)`. This lists the profile if it is unlisted ("Person N",
   next colour) and marks it last used. On error it warns `the profile list: {e}` and uses
   `home.dir.load()`. With no home it uses `Registry::default()`.
2. `core = Rc::new(RefCell::new(profile))`.
3. `Engine::new(&mut core)`. **(WEBKIT-ONLY)**:
   - adds the AppImage to the WebKit sandbox
   - registers the `view-source` scheme
   - creates `NetworkSession::new(engine_data, engine_cache)` with TLS errors set to fail and
     favicons on
   - `Settings`: developer extras on, fullscreen on, back/forward swipe gestures on
   - `apply_prefs`
   **(NEEDS-CEF)** `cef_initialize` with a per-profile `root_cache_path`, a
   `CefRequestContext` with `cache_path`, scheme registration in
   `CefApp::OnRegisterCustomSchemes`, and preferences (§11.11).
4. `Trackers::new(core)`, `Cookies::new(core, session)`,
   `Downloads::new(app, core, session, profile::downloads_dir())`. **(NEEDS-CEF)** Their engine
   halves (resource blocking, cookie policy, downloads) are other inventories.
5. Read `prefs.get(keys::UPDATES_AUTOMATIC)` and `keys::UPDATES_CHANNEL`.
6. `welcome = !SCRIPTED && onboarding::should_show(profile)`, where `should_show` is
   `profile.is_new() && !prefs.get(ONBOARDING_DONE)`.
7. `Rc::new_cyclic`:
   - `host = Rc<dyn TabHost>` over a weak `Inner` (§11.13)
   - `Runtime::new(core, session, host)`
   - `Syncer::new(weak, core)`
   - `Updates::new(app, automatic, channel)`, which registers `app.update`
8. `app.connect_window_removed`: for a `BrowserWindow`, `runtime.windows_changed()`.
9. `runtime.connect_actions_changed`: every window `refresh_extension_actions()`.
10. `app.connect_window_removed`: if the removed window was private and no other private window
    remains, `end_private_session()` (§11.5).
11. `permissions::seed_notifications(&browser)` hands WebKit the stored notification allow and
    block origins (`web_context().initialize_notification_permissions`). **(WEBKIT-ONLY)**
    **(NEEDS-CEF)** Notification permission comes from
    `CefPermissionHandler::OnShowPermissionPrompt` and/or Chromium content settings
    (`CefRequestContext::SetContentSetting` / `SetWebsiteSetting`).

### 11.3 Start: `Browser::start()`, in order
1. `apply_theme()` (§11.11).
2. `apply_keymap()` (§16).
3. `trackers().apply()`, `cookies().apply()`, `cookies().clear_at_start()`. Data of Block and
   Clear-on-exit sites is deleted again at start in case the last exit was unclean.
   **(KEEP)** behavior.
4. For every installed extension with `enabled`, `load_into_runtime(ext)`. Failures are logged as
   `extension {id}: {e}` and kept in `extension_errors` for the Extensions dialog.
   `extensions().list()` failing warns `cannot list extensions: {e}`.
5. `reconcile_extensions()`: core's `extensions().reconcile()` returns installs (store extensions
   another device asked for) and removals. Removals unload. Each install runs the core pipeline
   in the background and logs `installed {id} from another device's list` or `reconcile install
   from {source:?} failed: {e}`. The keymap is reapplied.
6. Unless `SCRIPTED`, `schedule_extension_updates()`: a loop that waits
   `core.extensions().next_update_check()` (fallback `UPDATE_INTERVAL`), then runs
   `update_extensions()` (prepare, run on a worker, commit, reload the updated ones) and logs
   `extension update check: {summary}`.
7. `preload_favicons()` (§11.8).
8. Memory Saver: `glib::timeout_add_local(memory_saver::SWEEP_EVERY = 60 s)` calls
   `sleep_idle_tabs(Instant::now())` (§11.12).

**(WEBKIT-ONLY / NEEDS-CEF)** Steps 4 to 6 drive `vsesvit-webext` with core's install pipeline.
With Chromium's extension system, core's `extensions()` desired set (synced), installed set
(local) and update checks must be bridged to whatever loads extensions into CEF. **(inferred
risk)** CEF 154 has no `CefRequestContext::LoadExtension`: the bindings contain no
`load_extension` or `get_extension`, so the Alloy extension API is gone. The extension inventory
covers this.

### 11.4 Windows
- `windows()`: every `BrowserWindow` of the app, **most recently focused first**
  (`gtk::Application::windows()` order). The session and command-line placement depend on this
  order. **(NEEDS-GPUI)** Track focus order yourself.
- `windows_of(browsing)` filters that list by kind.
- `normal_window()`: the most recently focused normal window, else
  `open_window(Normal, &[])`, which adds a blank tab. Used for anything from outside a window:
  another invocation, an extension, sync.
- `find_tab(TabId) -> Option<(BrowserWindow, Tab)>`.
- `open_window(browsing, targets)`: `BrowserWindow::with_browsing`. If targets is empty,
  `new_tab()` opens the new tab page. Otherwise `open_tabs(targets)`: one tab per target at the
  end, the first selected. Then `present()`.
- `present()`: `normal_window().present()`.
- `windowed(kind)`: the open Bookmarks, History or Downloads(browsing) window, if visible.
  `add_windowed(kind, window)` registers it. Each is a separate top-level window that closes with
  the browser window that opened it (`dialogs::present_window`).
- What `BrowserWindow::with_browsing` does on creation (window/mod.rs, other inventory):
  - default size 1280×820
  - CSS class `private` for private windows
  - title `"Vsesvit"`, passed through the profile and private suffixes (§13.6)
  - window actions, tab menu, groups
  - `apply_prefs()`, then refresh the bookmarks bar, extension actions and profile button
  - shows the downloads button if a download started this session in that kind of window
  - `updates.window_opened(window)`

### 11.5 Private session
**(KEEP behavior; NEEDS-CEF engine half)**
- `network_session(browsing)`:
  - `Normal` returns the profile's session.
  - `Private` creates the private engine side **lazily, at the first private tab**:
    1. `Engine::ephemeral_session()` (`NetworkSession::new_ephemeral()`, set up like the
       profile's)
    2. `downloads().watch(&session, Private)` (its own download handler)
    3. `cookies().set_private_session(Some(&session))`
    4. store it
    5. last, `runtime().private_session_started(&session)`
  - **(NEEDS-CEF)** A `CefRequestContext` created with an **empty `cache_path`** is "incognito
    mode", with in-memory storage (verified in the `cache_path` doc comment). It needs its own
    `CefDownloadHandler` wiring and cookie policy. **(inferred)** In Chrome style, whether
    Chromium's extension system treats it as an off-the-record profile for "Allow in Incognito"
    needs research.
- `end_private_session()` runs when the last private window is removed:
  1. `downloads().cancel_private()` cancels private downloads still running, as Chrome does
  2. disconnect the private download handler and `cookies().set_private_session(None)`
  3. drop the session, so cookies, cache and storage go
  4. clear `private_closed_tabs`
  5. `core.end_private_session()` forgets private site choices, zoom and download rows
- `private_session_lasts()` (self-test only): whether the private engine side exists.
- Rules that run through the browser: a private window sees only private tabs (tab search,
  reopen closed tab, closed stack), and nothing a private tab does is written (§11.7, §15).

### 11.6 Tab lifecycle hooks (called by window and tab code)
- `allocate_tab_id()` and `allocate_window_id()` increment from 1.
- `tab_closed(tab, position, pinned, group)`:
  - `permissions::closed(tab)` drops one-time grants and pending prompts
  - `runtime().tab_closed(id, false)`
  - `schedule_session_save()`
  - if `tab.committed_uri()` is set and is not `about:blank`, push a `ClosedTab` onto the stack
    of the tab's `Browsing` (when the stack is full, the oldest is dropped):
    - `uri`
    - `title = tab.display_title()`
    - `favicon = web_view.favicon()`
    - `state = web_view.session_state()` (the WebKit back/forward state)
    - `position`, `pinned`, `group: Option<GroupId>`
    - `used = tick()`
- `tab_discarded(tab, window_closing)`: a tab that goes with its window, or a `window.open` view
  that never showed. `permissions::closed` and `runtime().tab_closed(id, window_closing)`. It is
  **not** pushed onto the closed stack. Closing a window does not make its tabs reopenable one by
  one.
- `tab_attached(tab)` calls `runtime().tab_attached(id)`. `tab_moved(tab)` calls
  `runtime().tab_moved(id)` and schedules a save.
- `tab_activated(tab)`: `tab_used(tab)`, `runtime().tab_activated(id)`, schedule a save.
- `tab_used(tab)`: `tab.set_used(tick())`. Also called when a window becomes active (for its
  selected tab) and when a tab opens. `tick()` increments `last_use`.
- `can_reopen_closed_tab(browsing)`: whether that stack is non-empty. It drives the "Reopen
  Closed Tab" menu item's sensitivity.
- `reopen_closed_tab(window)` pops the most recent tab of the window's kind and runs
  `window.restore_closed(&closed)` (window/mod.rs):
  - the new tab is inserted pinned at `position` clamped to the pinned range, or unpinned
    clamped to the unpinned range
  - it is selected
  - it rejoins `group` if the window still has that group
  - `tab.restore(state, uri)` brings back the WebKit back/forward list, else just loads the URI
  - **(KEEP)** reopened tabs keep their history in the current shell.
  - **(NEEDS-CEF)** CEF can read navigation entries (`GetNavigationEntries`, read-only) but has
    **no API to restore a back/forward list** into a browser. The bindings have no restore or set
    function for navigation entries. Reopened and restored tabs would load the URL alone, as
    Windows does today, unless a workaround is found.

### 11.7 Tab search **(KEEP)**
- `search_tabs(browsing, query) -> Vec<tab_search::Row<TabId, ClosedKey>>` builds:
  - **open**: every tab of that kind across all windows, as `Listed { key: id, title:
    display_title, url: session_uri or "", used }`
  - **closed**: the stack of that kind, as `Listed { key: ClosedKey, title, url: uri, used }`
  - It returns `vsesvit_core::tab_search::rows(query, open, closed)`. Core does the ordering:
    open tabs before closed ones, the most recently used first, and the text ranking (title
    prefix, then title, then URL).
- `tab_icon(browsing, hit)`: an open tab's current favicon, or the favicon a closed tab had.
- `go_to_tab(window, hit)`:
  - `Hit::Open(id)`: select the tab in its own window and present that window.
  - `Hit::Closed(key)`: take it out of the stack and `restore_closed` it into `window`.
  - Nothing happens for a tab that has gone since it was listed.
- `ClosedKey` (closed_tabs.rs) names a closed tab stably while the stack changes
  (`push`, `pop`, `take`, `get`, `iter` oldest first, `clear`).

### 11.8 History, zoom, titles, bookmarks, favicons
- **`navigation_committed(tab, uri, commit)`**, for every committed main-frame navigation:
  - `transition = tab.take_pending_transition().unwrap_or(Transition::Link)`. The window sets
    `Typed` (omnibox) or `Bookmark` (home button, bookmarks) before loading.
  - If `commit != SameDocument`, `show_at_site_zoom(tab, url)`.
  - If `commit != ErrorPage` and the tab is Normal,
    `core.history().record_visit(&url, transition)`. On error it warns `history: {e}`.
    **Same-document navigations (fragments, `pushState`) record visits too.** Private tabs and
    error pages record nothing.
  - `runtime().tab_updated(id)`, then schedule a save.
  - `Commit` is `Document | SameDocument | ErrorPage`, where `ErrorPage` is the shell's own error
    page shown under the URI that failed.
  - **(NEEDS-CEF)** Commit detection: `CefLoadHandler::OnLoadStart` / `OnLoadEnd` /
    `OnLoadError`, `CefDisplayHandler::OnAddressChange`, and `CefRequestHandler::OnBeforeBrowse`
    for the transition and user gesture. Same-document detection needs DevTools
    (`Page.navigatedWithinDocument`) or a comparison of navigation entries. **(inferred)**
- **Site zoom** **(KEEP behavior; WEBKIT-ONLY reason; NEEDS-CEF)**:
  - `show_at_site_zoom(tab, url)`: `level = core.site_zoom(browsing).get(url)`, where 1.0 is
    100% and the stored unit is a factor. If it differs from the view's level, set the view's
    zoom. The reason given is that WebKit keeps a view's zoom from page to page. Errors warn
    `site zoom: {e}`.
  - `zoom_changed(tab)`, when the view's zoom changes for any reason:
    `core.site_zoom(browsing).set(url_of_committed_uri, level)`. Private tabs keep it in
    memory only (core decides).
  - Core is the source of truth, a LOCAL preference that is not synced.
  - **(NEEDS-CEF)** `CefBrowserHost::SetZoomLevel` / `GetZoomLevel` use Chromium's logarithmic
    zoom level (0.0 = 100%, factor = 1.2^level, **(inferred)** Chromium convention). Chromium's
    own per-host zoom map would also apply per-site zoom by itself. Pick one source of truth and
    stop Chromium's map from fighting core's.
- **Titles to history** (`title_changed(tab)`): Normal tab, not an error page, committed URI set,
  non-empty title. Then `core.history().set_title(&url, &title)`, `runtime().tab_updated`, and
  schedule a save.
  - **(WEBKIT-ONLY)** reason: WebKit clears the title before reporting the next commit, so an
    empty title would land on the page being left, and the error page's title ("Problem Loading
    Page" in tests) must not overwrite the failed URL's title.
  - **(KEEP)** rule: never write an empty title, never write an error page's title.
  - **(NEEDS-CEF)** `CefDisplayHandler::OnTitleChange`.
  - Tests: `an_error_page_leaves_the_history_title_of_the_page_that_failed` and
    `leaving_a_page_leaves_its_history_title`.
- **Star state**: `is_bookmarked(uri)` is the in-memory `core.bookmarks().is_bookmarked(&url)`,
  safe on every navigation.
- **Star button and Ctrl+D** (`star_clicked(window)`) **(KEEP)**:
  - Takes the selected tab's committed URL (nothing happens without one) and
    `title = display_title()`.
  - If `core.bookmarks().find_by_url(&url)` has a node, open the **"Edit bookmark"** bubble on the
    first one (`added = false`).
  - Otherwise `add_url(BookmarkId::TOOLBAR, InsertAt::End, &title, &url)`, which adds to the
    **end of the bookmarks bar**. Then `save_favicon(tab)` (the icon arrived before the bookmark
    existed), then `bookmarks_changed()`, then the **"Bookmark added"** bubble.
  - On error, toast `Cannot bookmark the page: {e}`.
  - `window.show_bookmark_bubble(node, added)` (other inventory).
- **Favicons** **(KEEP)**:
  - `favicon_changed(tab)`: `save_favicon`, and if that changed the stored icon,
    `bookmarks_changed()`.
  - `save_favicon(tab)`: Normal tab only, with a committed URI and a favicon.
    `favicons::record(core, uri, texture)` turns the texture into PNG for
    `core.favicons().record(page, png)`. Core stores it only when the page or site is bookmarked
    and returns whether it changed. Private tabs never store icons.
  - **(NEEDS-CEF)** `CefDisplayHandler::OnFaviconURLChange` gives URLs, so download with
    `CefBrowserHost::DownloadImage`.
  - `preload_favicons()` fetches icons of bookmarked sites never visited:
    - If a fetch is running, mark it `RunningStale` and return.
    - Otherwise `core.favicons().missing(FAVICON_BATCH = 32)`. If empty, return.
    - Set `Running`, then `gio::spawn_blocking(FaviconFetch::new(pages).run())`, then
      `core.favicons().commit_fetched(results)`.
    - If that changed anything, `bookmarks_changed()`, which preloads the next batch. Otherwise,
      if stale, preload again.
    - Errors are logged: `favicons to fetch: {e}`, `storing fetched favicons: {e}`, `the favicon
      fetch panicked`.
    - Test: `bookmarked_sites_get_their_icons_without_being_visited`.
  - Visiting more pages of a bookmarked site keeps the bar's buttons; they are not rebuilt
    (`visiting_more_of_a_bookmarked_site_keeps_the_bar_buttons`).
- **`bookmarks_changed()`** runs after any bookmark write. Every window
  `refresh_bookmarks_bar()` and `sync_star()`, every bookmark view refreshes, then
  `preload_favicons()`.

### 11.9 Extension toolbar order **(KEEP behavior; NEEDS-CEF data source)**
- `extension_actions()`: `runtime().actions()` (`Vec<ActionInfo>`), stable-sorted by **install
  order**, meaning the position in `installed_extensions()`. Actions the profile does not list go
  last, in id order. Test: `extension_actions_follow_install_order_not_id_order`.
- `extension_toolbar(available_ids_in_install_order) -> toolbar::Layout`, which is
  `vsesvit_core::extensions::toolbar::layout(available, &prefs.get(&toolbar::TOOLBAR))`. Synced
  pref key: `toolbar.extensions`.
- `pin_extension(id, pinned)` uses `toolbar::set_pinned`. `move_extension(id, to)` uses
  `toolbar::move_pinned` (`to` is the index among pinned actions). Both go through
  `change_toolbar`: compute from the available ids and the saved entries, `prefs.set(TOOLBAR)`
  (warns `extension toolbar: {e}`), then every window `refresh_extension_actions()`.
- `extension_error(id)` and `set_extension_error(id, Option<String>)` hold why an enabled
  extension does not run, for the Extensions dialog.
- `updating_extensions()`, `watch_extensions(refresh)` and `extensions_updated()` serve the
  Extensions dialog.
- Install, enable, uninstall and update are in `extensions.rs` (other inventory). It calls
  `apply_keymap()` after every change to the enabled set, because extension commands come from
  manifests (§16).

### 11.10 Omnibox **(KEEP; engine-free)**
- `omnibox_changed(window, text, allow_inline)`, on every address-bar edit:
  1. `address.set_suggestions(omnibox::suggestions(browser, window, text, allow_inline, None))`.
     These are core's rows (search, typed URL, bookmarks, history) plus matching open tabs, with
     inline completion when allowed.
  2. `core.omnibox().suggest_request(text, address.queries(), private)`. It returns `None`
     (nothing to fetch) for:
     - search suggestions turned off (`address_bar.suggest.search`)
     - a private window
     - a non-plain search (a URL being typed, a file path, a keyword search of another engine)
     - an engine without a suggest URL
     Errors warn `search suggestions: {e}`.
  3. Otherwise `gio::spawn_blocking(request.run())` (which debounces with `suggest::DEBOUNCE` and
     fetches). On return, if the window still exists and `found.is_current()` (no newer query),
     rebuild the rows with `Some(&found)` and `address.refill_suggestions(rows)`.
- `omnibox_activated(window, text)`, on Enter: `core.omnibox().resolve(text)`:
  - `Ok(Some(target))`: `window.navigate_with(target.url(), Transition::Typed)`
  - `Ok(None)`: `window.focus_page()`
  - `Err`: toast `Cannot resolve the address: {e}`

### 11.11 Preferences with a live effect
Each setter writes the pref and applies it at once in every window. All preference I/O goes
through `pref(&Pref<T>)` (core `prefs().get`), `set_pref(&Pref<T>, &T)` (warns `prefs: {e}`) and
`reset_pref(&Pref<T>)`.

| Setter | Pref key (scope) | Live effect |
|---|---|---|
| `set_tabs_position(TabsPosition)` | `tabs.position` (synced; `left` default, `right`, `top`) | every window `apply_layout(position)` re-lays out live |
| `set_bookmarks_bar_visible(bool)` | `bookmarks_bar.visible` (synced, default true) | every window `set_bookmarks_bar_visible` |
| `set_home_button_visible(bool)` | `toolbar.home_button` (synced, default false) | every window `set_home_button_visible` |
| `set_compact_address_bar(bool)` | `address_bar.compact` (synced, default true) | every window `set_compact_address_bar` |
| `set_full_urls(bool)` | `address_bar.full_urls` (synced, default false) | every window `set_full_urls` |
| `set_engine_switch(&Pref<bool>, bool)` | `content.block_popups`, `scrolling.smooth`, `system.hardware_acceleration` (local), `spellcheck.enabled` | `engine.apply_prefs(core)` reaches every view |
| `set_spellcheck_language(lang, on)` | `spellcheck.languages` = `Some(engine::dictionaries().choose(current, lang, on))` | `engine.apply_prefs` |
| `set_tracking_protection(level)` | `privacy.tracking_protection` | `trackers().apply()` |
| `set_third_party_cookies(choice)` | `privacy.third_party_cookies` | `cookies().apply()` |
| `set_theme(Theme)` | `theme` (synced; `system` default, `light`, `dark`) | `apply_theme()` |
| `set_updates_automatic(bool)` | `updates.automatic` (local) | `updates.set_automatic` starts or stops the checks |
| `set_updates_channel(UpdateChannel)` | `updates.channel` (local) | `updates.set_channel` checks the new channel at once, even with automatic updates off |
| `set_show_profile_picker(bool)` | not a pref: `profiles.json` `show_picker` (install-wide) | §13 |

- Getters: `tabs_position()`, `bookmarks_bar_visible()`, `home_button_visible()`,
  `compact_address_bar()`, `full_urls()`, `theme()`, `switch(&Pref<bool>)`, and `homepage()`,
  which is `vsesvit_core::prefs::homepage_url(&pref(HOMEPAGE))`. `None` means the new tab page:
  for `about:home` (the default), `about:blank`, an empty value, or non-URL text. The homepage
  never searches.
- `apply_theme()` **(KEEP behavior; GTK-ONLY mechanism)**: `System` gives
  `adw::ColorScheme::Default` (follow the desktop), `Light` gives `ForceLight`, `Dark` gives
  `ForceDark`, all on `adw::StyleManager::default()`. **(NEEDS-GPUI)** Follow the desktop
  light/dark setting (`org.freedesktop.appearance color-scheme` through the portal) and force it
  when asked. **(NEEDS-CEF)** Web content's `prefers-color-scheme` should follow too.
  **(inferred)** One route is `CefRequestContext::SetChromeColorScheme`, if present; another is
  Chromium's dark-mode prefs. This needs research.
- **(WEBKIT-ONLY / NEEDS-CEF)** `Engine::apply_prefs` maps these prefs onto WebKit:
  - `content.block_popups` becomes `javascript_can_open_windows_automatically = !block`. Clicks
    still open windows.
  - `scrolling.smooth` becomes `enable_smooth_scrolling`.
  - `system.hardware_acceleration` becomes `HardwareAccelerationPolicy::Always` or `Never`.
  - Spell checking runs only when `spellcheck.enabled` is on and the chosen dictionaries are
    non-empty, using Enchant/Hunspell dictionaries from `~/.config/enchant/hunspell`,
    `/usr/share/hunspell`, `/usr/share/myspell`, `/usr/share/myspell/dicts`.
  - Test defaults: popups blocked, smooth scrolling on, acceleration Always.
  - CEF needs:
    - pop-up blocking through `CefLifeSpanHandler::OnBeforePopup`, using its user-gesture
      argument, or the content setting
    - smooth scrolling and GPU as Chromium command-line switches (`--disable-smooth-scrolling`,
      `--disable-gpu`), which need a **restart**
    - spell check through Chromium prefs (`spellcheck.dictionaries`) with
      `CefRequestContext::SetPreference`. Chromium downloads its own Hunspell `.bdic`
      dictionaries, so `engine::dictionaries()` does not carry over. **(inferred)**

### 11.12 Memory Saver sweep **(KEEP policy; NEEDS-CEF mechanism)**
- `sleep_idle_tabs(now)`: `Sweep::new(core, now)` reads `performance.memory_saver` and
  `performance.memory_saver.mode`, which sets the delay: Moderate 6 h, Balanced 4 h (the
  default), Maximum 2 h. It also reads the sites allowed to notify. Then for each window and tab
  where `sweeps(...)` holds, `sleep_unless_unsaved(tab, now)`.
- `sweeps(sweep, window, tab)` builds `memory_saver::TabActivity`:
  - `url`: the committed URI. A tab without one is never swept.
  - `shown`: selected in its window
  - `pinned`
  - `audible = web_view.is_playing_audio()`
  - `capturing = tab.capturing().any()`
  - `related = tab.is_related()` (a `window.opener` link)
  - It returns `tab.sleeps(sweep, &activity)`. The tab owns an `IdleClock`. Anything that keeps
    it awake restarts the clock. Only `http` and `https` pages sleep.
- `sleep_unless_unsaved(tab, now)`:
  1. Run `memory_saver::UNSAVED_INPUT_SCRIPT` in the page
     (`evaluate_javascript_future`).
  2. If `tab.has_edits()` (a per-frame script reported an edit) or
     `memory_saver::has_unsaved_input(&result)`, then `tab.keep_awake(Instant::now())`.
  3. Otherwise, if the browser and window still exist, re-check with a new `Sweep` at the same
     `now`. If the committed URI is unchanged and it still sweeps, `tab.sleep()` and schedule a
     save.
- What sleeping looks like (tab.rs, other inventory): the web process ends
  (`TerminatedByApi`), the tab keeps its title, address and back/forward state, and its icon
  shows at half opacity. Selecting it reloads at the same history entry. The session keeps its
  address, title and restore state.
- **(NEEDS-CEF)**:
  - "Audible" comes from `CefAudioHandler::OnAudioStreamStarted` / `Stopped`.
  - "Capturing" comes from `CefDisplayHandler::OnMediaAccessChange`.
  - JavaScript with a result has no direct CEF API. Use DevTools `Runtime.evaluate` through
    `CefBrowserHost::ExecuteDevToolsMethod` plus `AddDevToolsMessageObserver`, or a
    renderer-side `CefRenderProcessHandler` with a `CefProcessMessage` round trip.
  - "has edits" needs injected script in every frame plus a message back.
  - Sleeping: CEF exposes no tab discard. **(inferred)** Options are closing the browser
    (`CloseBrowser`) and recreating it on wake, which loses back/forward history (no restore API,
    §11.6), or `WasHidden`, which frees little.

### 11.13 `TabHost` for the extension runtime
**(WEBKIT-ONLY implementation; KEEP semantics, which Chromium's own extension system provides)**

`struct Host(Weak<Inner>)` implements `vsesvit_webext::TabHost`, so `chrome.tabs` and
`chrome.windows` see exactly what the user sees:

- `windows()` returns `WindowInfo` per window:
  - `id`
  - `focused = is_active`
  - `browsing`
  - `state`: `Minimized` from the GDK toplevel state, `Fullscreen`, `Maximized` or `Normal`
  - `width` and `height`: the size on screen once mapped, else the default size
  - Every window reports origin (0,0) because Wayland gives no positions (PLAN).
- `tabs()` returns `TabInfo`:
  - `id`, `window_id`, `index` (the position in the window)
  - `url = committed_uri`: **never a URL still loading**. Test:
    `extensions_see_the_committed_url_while_the_next_page_loads`.
  - `title = display_title`, `active` (selected), `browsing`
  - `group_id = tab_groups::extension_group_id(group)`: a positive number folded from the group
    id, or -1
- `create_tab(NewTab { window, url, index, active })`: in the named window, else
  `normal_window()`. `open_tab_at(url, index, Foreground|Background)`.
- `update_tab(id, url, active)`: load the URL, and `active == Some(true)` selects the tab.
- `move_tab(id, window, index)`: within a window, or to another window. A window left without
  tabs closes.
- `remove_tab(id)`: `window.close_tab`.
- `web_view(id)`: the tab's WebKit view.
- `create_window(NewWindow { browsing, tab, urls, width, height, state, focused })`:
  - a new window of that kind
  - move the given tab into it, or open the URLs, or a new tab page when neither is given
  - `set_default_size` with the given or current values
  - set the state (`Normal` un-fullscreens, un-maximizes and presents a minimized window;
    `Minimized`, `Maximized` (un-fullscreens first), `Fullscreen`)
  - present if `focused`, else just show
- `update_window(id, WindowUpdate { state, width, height, focused })` and `remove_window(id)`
  (`window.close()`).
- `cookies_blocked(domain) = vsesvit_core::cookies::site_rules(core).blocks_cookie(domain)`.
  Extensions may not set cookies for a site set to Block. It matches the domain, the domain with
  a leading dot, and subdomains (`www.`).
- `private_session()`: the private `NetworkSession` while it exists.
- `ask_permissions(window, Prompt { extension, name, warnings }, answer)`: shows
  `extension_prompts::request_dialog(&prompt)` over the named window, with heading `“{name}” has
  requested additional permissions.`. The answer is `ALLOW` or anything else, and a missing
  window answers false. Test:
  `an_extensions_permission_request_asks_over_the_window_it_names`.

**(NEEDS-CEF, risk)** With Chromium's extension system, `chrome.tabs` and `chrome.windows` are
answered by Chromium's own browser and tab model. Whether a GPUI-drawn tab strip of CEF browsers
appears to extensions as one window with many tabs (rather than, for example, one window per
browser) is the central compatibility question for the CEF extension research. **(inferred)**

### 11.14 Profiles in the browser **(KEEP)**
See §13.

### 11.15 Sync refresh: `sync_applied(&vsesvit_core::sync::Changed)` **(KEEP)**
Called after the sync engine applies remote records, and by `app.debug-apply-sync`.

| `Changed` field | Refresh |
|---|---|
| `bookmarks` | `bookmarks_changed()` |
| `history` or `sessions` | history views (History window, other devices' tabs) |
| `prefs` non-empty or `search_engines` | `pref_views` (open Settings controls), dropping dead ones |
| `extensions` | `reconcile_extensions()` |
| `prefs` contains `extensions.allowed_in_private` (`extensions::private::ALLOWED_IN_PRIVATE.key`) | `runtime().allowed_in_private_changed()` |
| `ext_storage` entries `(ext, changes)` | `runtime().storage_sync_changed(ext, changes)`, for `storage.onChanged` |
| `site_permissions` | `permissions::enforce(browser)` |
| `site_permissions` or `privacy.tracking_protection` | `trackers().apply()` |
| `site_permissions` or `privacy.third_party_cookies` | `cookies().apply()` |
| `keyboard.shortcuts` | `apply_keymap()` |
| `prefs` non-empty | `apply_theme()`, `engine.apply_prefs()`, every window `apply_prefs()` and `refresh_extension_actions()` |

**(WEBKIT-ONLY / NEEDS-CEF)** The `ext_storage` and `allowed_in_private` rows feed the home-grown
runtime. With Chromium, `storage.sync` lives inside Chromium, as on Windows, where PLAN notes it
does not sync.

### 11.16 Other public helpers
- `core()`, `engine()`, `runtime()`, `trackers()`, `cookies()`, `downloads()`, `sync()`,
  `updates()`, `app()`.
- `when_blockers_applied(f)` (§7).
- `https_upgrade(browsing, url)` is
  `vsesvit_core::https_only::upgrade(core, browsing, url, https_reach)`: the https URL a
  navigation loads instead under HTTPS-only. It is `None` unless `privacy.https_only` is on, the
  URL is a public `http` URL on the default port (or `Reach::Everywhere`), and the site has no
  exception. **(NEEDS-CEF)** Hook it in `CefRequestHandler::OnBeforeBrowse` for main frames.
  Chromium tells subframe navigations apart, unlike WebKit's `decide-policy`, so the main-frame
  guessing in PLAN goes away. **(WEBKIT-ONLY)**.
- `keymap()`, `extension_shortcuts(keymap)`, `edit_keymap(edit)`, `apply_keymap()` (§16).
- `restart_program()` (§9).
- `is_last_normal_window(window)`: the window is Normal and at most one normal window exists
  (§15).

---

## 12. The profile picker (profiles.rs `run_picker`, `picker`) **(KEEP; NEEDS-GPUI)**

- **When**: a launch with no `--profile-dir`, no URL targets, more than one listed profile,
  `show_picker` on, and no listed profile running (§5).
- **Process**: `adw::Application` with `application_id = APP_ID` and **`NON_UNIQUE`**, so the
  desktop shows it as Vsesvit but it never claims the Default profile's bus name or receives
  forwarded command lines. `startup` sets the app name and default icon and loads the CSS.
  `activate` presents `picker(app, &dir)`. `run_with_args::<&str>(&[])` ignores argv. The picker
  process **never opens a profile**. It launches the chosen one and exits when its window closes.
  **(NEEDS-CEF)** The picker needs no web engine, so it must not initialize CEF, or at least must
  not take a profile's `root_cache_path`.
- **Window** (`adw::ApplicationWindow`):
  - title `Vsesvit`, default size 760×520
  - a header bar with no title (`show_title(false)`), so it has window controls only
  - a toast overlay around a vertically scrolling area (no horizontal scrolling)
  - a vertical box, spacing 24, vertically centred, 24 px margins top and bottom, holding:
    1. A heading label `Who's Using Vsesvit?` in style `title-1`.
    2. Cards: a flow box, homogeneous, at most 5 per line, horizontally centred, 12 px row and
       column spacing, no selection.
       - Each card is a flat button (class `profile-card`, 12 px padding) with a tooltip of the
         profile name. Inside, a vertical box with spacing 8: the large avatar (72 px) and the
         name label (ellipsized at the end, at most 14 characters wide).
       - Cards come in list order, which is the order profiles were added.
       - The last card is **Add**: an image `list-add-symbolic` at 48 px inside 72×72, labelled
         `Add`.
    3. A check button `Show on startup`, centred, initially `registry.show_picker()`. Toggling it
       calls `dir.set_show_picker(active)` and warns `the profile list: {e}` on error.
- **Choosing a profile card**: `pick(window, toasts, root)`, which is
  `profile::launch(window, root)`. On success the picker window closes, and the picker exits. On
  failure, toast `Could not open the profile: {e}` and the picker stays.
- **Add**: `ask(window, "Add Profile", "_Add", registry.next_name(), registry.next_color())`
  (§13.2). On accept, `dir.add(name, color)` creates the directory `Profile N` and lists it, then
  `pick` launches it. On error, toast `{e}` (for example `a profile needs a name`).
- Test: `the_picker_lists_every_profile_and_can_add_one` expects labels
  `Who's Using Vsesvit?`, `Person 1`, `P`, `Work`, `W`, `Add`.

---

## 13. Profiles: avatar, add and edit dialog, Manage Profiles, profile button, titles

Core contract: `vsesvit_core::profiles`. The shell never caches the list beyond the last read.
Every change re-reads under the lock and returns a new `Registry`.

### 13.1 Avatar **(KEEP visual; GTK-ONLY widgets)**
- `avatar(name, color, picture: Option<&Path>, large) -> Widget`:
  - **With a picture file** (the sync account's picture, a PNG in the profile directory): a
    circular `adw::Avatar` with the picture, 22 px or 72 px when large. A load failure warns
    `the profile picture: {e}` and falls back to the initial.
  - **Without**: a circle in the profile's colour with the initial in white bold. The initial is
    `profiles::avatar_initial(name)`: the first alphanumeric character upper-cased, or `?`.
  - Sizes: 22×22 normally; `.large` is 72×72 with font-size 2em.
- `entry_avatar(dir, entry, large)` uses `dir.picture(entry)`, the account picture's path if the
  list names it and the file exists.
- Colours, `ProfileColor::ALL` in order (`rgb()`, the libadwaita accents):

  | Colour | Hex | Colour | Hex |
  |---|---|---|---|
  | Blue | `#3584e4` | Red | `#e62d42` |
  | Teal | `#2190a4` | Pink | `#d56199` |
  | Green | `#3a944a` | Purple | `#9141ac` |
  | Yellow | `#c88800` | Slate | `#6f8396` |
  | Orange | `#ed5b00` | | |

  Labels are those names. CSS classes are `profile-<label lowercase>`.

### 13.2 Name and colour dialog: `ask(parent, heading, accept, name, color)` **(KEEP)**
- An `adw::AlertDialog` with heading `heading` and no body. Its extra child is a vertical box,
  spacing 12:
  - An entry: text = suggested or current name, Enter activates the default, max length
    `NAME_MAX = 64`, accessible label `Name`. It takes focus.
  - A flow box of the 9 colours: homogeneous, at most 5 per line, centred, no selection. Each is
    a flat, circular toggle button, all in one radio group, with tooltip = colour label, active =
    current colour. The child is `avatar("", colour, None, false)`.
    **(Observed quirk)** Because the name is empty, each swatch shows a white `?` in the circle.
- Responses: `_Cancel` and `accept`. `accept` has the suggested appearance and is the default
  response; close means cancel. `accept` is disabled while the trimmed name is empty.
- Returns `Some((trimmed name, chosen colour))` or `None`.
- Core validation (`EditError`) as surfaced in toasts:
  - empty: `a profile needs a name`
  - more than 64 characters: `a profile name has at most 64 characters`
  - `the last profile cannot be removed`
  - `that profile is no longer in the list`
- Uses:
  - "Add Profile": `"Add Profile"`, `"_Add"`, `registry.next_name()` ("Person N" with the first
    free N), `registry.next_color()` (the first unused colour, else the least used)
  - "Edit Profile": `"Edit Profile"`, `"_Save"`, current name and colour

### 13.3 Add Profile from a browser window (`profiles::add(window)`, action `win.add-profile`)
`ask(...)`, then `browser.add_profile(window, name, color)`:
1. `home.dir.add(name, color)`. Core creates `Profile N` and lists it. A name or colour other than
   the suggestions sets `set_by_hand`.
2. `set_profiles(registry)`.
3. `open_profile(window, &id)` launches the new profile's process, so it opens in a **window of
   its own, as Chrome does**.
4. An error toasts the message. Without a home: `this profile is not in the profile list`.

### 13.4 Manage Profiles (`profiles::manage(window)`, action `win.manage-profiles`, Settings button) **(KEEP)**
- It first calls `browser.profile_used()`, which re-reads the list.
- An `adw::Dialog` titled `Manage Profiles`, content 520×480, with a header bar. Its content is a
  toast overlay around a preferences page with one group:
  - title `Profiles`
  - description `Each profile has its own bookmarks, history, settings, extensions and sync
    account, and opens in windows of its own.`
  - header suffix: a flat `list-add-symbolic` button with tooltip `Add Profile`, which runs
    `profiles::add(window)`
  - a boxed list, no selection, one row per profile in list order
- Each row (`adw::ActionRow`, title = name without markup, prefix = small avatar):
  - **The current profile**: subtitle `This profile`, no Open button.
  - **Other profiles**: an **Open** button (`window-new-symbolic`, tooltip `Open`) that runs
    `browser.open_profile(window, id)`.
  - **Edit** (`document-edit-symbolic`, `Edit`): `ask(dialog, "Edit Profile", "_Save", name,
    color)`, then `browser.edit_profile(id, name, color)`. Core marks the profile set by hand,
    and a new colour drops the account picture. Errors toast inside the dialog.
  - **Remove** (`user-trash-symbolic`, `Remove`): sensitive only if `registry.can_remove(id)`
    (more than one profile). It asks `confirm(dialog, "Remove {name}?", body, "_Remove")`, with a
    **destructive** accept, Cancel as default and close responses, and body:
    `Its bookmarks, history, settings, extensions and sync sign-in are deleted from this device,
    and its windows close. What it synced stays on the sync server.` Then
    `browser.remove_profile(window, id)`. Errors toast in the dialog.
  - Row buttons are flat and vertically centred (`dialogs::row_button`).
- The list refills whenever the profile list changes, through `browser.watch_profiles`.

### 13.5 Browser-side profile operations (browser.rs) **(KEEP)**
- `home()`, `profiles()` (the last read `Registry`), `watch_profiles(refresh)`.
- `profile_title()`: `registry.title_name(&home.id)`, the profile's name **only when there is more
  than one profile**.
- `profile_used()` runs whenever a window comes forward (becomes active), when the profile button
  opens its menu, and when Manage opens. It calls `home.dir.opened(&home.id)`, which marks this
  profile last used:
  - if the result says this profile is removed, `close_removed_profile()`
  - else `set_profiles(registry)`
  - errors warn `the profile list: {e}`
- `close_if_removed()`: if `home.dir.load().is_removed(&home.id)`, `close_removed_profile()` and
  return true.
- `close_removed_profile()`: `removed = true`, `shut_down = true` (no session save), `app.quit()`.
- `set_profiles(registry)`: if it differs from the stored one, store it, every window runs
  `profiles_changed()` (avatar, menu, title), and the profile views refresh.
- `open_profile(window, id)`: the current profile presents the window. Another profile runs
  `profile::launch(window, home.dir.root(id))`, and an error toasts
  `Could not open the profile: {e}`.
- `add_profile`, `edit_profile(id, name, color)` (`home.dir.edit_profile`, then `set_profiles`).
- `remove_profile(window, id)`:
  - `running = id != home.id && home.dir.is_running(id)`
  - `registry = home.dir.remove(id)`: it leaves the list now, its data is deleted now if no
    process holds it, else at a later sweep
  - **Removing the current profile**: if `registry.startup(true, false)` is `Open(next)`, launch
    `next`, the profile a launch would open next. Then `close_removed_profile()`. The process
    quits without saving, and after `run` its data is swept (§9).
  - **Removing another profile**: if it is running, `profile::notify(root)`. Its process gets an
    empty forwarded command line, sees it is removed, and quits. Then `set_profiles(registry)`.
- `set_show_profile_picker(on)`: `home.dir.set_show_picker(on)`, then `set_profiles`.

### 13.6 Window titles **(KEEP)**
- `BrowserWindow::show_title(title)` (window/mod.rs) sets
  `window_title(profiles::with_profile(title, profile_title()), browsing)`:
  - `with_profile`: `"{page} - {profile}"` when there is more than one profile, else `page`.
  - Private windows append ` (Private)`.
  - Examples: `Page`, `Page - Work`, `Page - Work (Private)`, `Page (Private)`.
  - Before any page the title is `Vsesvit`, with the same suffixes.
  - Windows differs: `Page - Work - Vsesvit (Private)`.

### 13.7 Profile button and menu (window/menu.rs, window/mod.rs; other inventory, summarized) **(KEEP)**
- A flat menu button in the toolbar showing the small avatar, with tooltip = profile name.
  - **Hidden** in private windows, and for a `--profile-dir` outside the list.
- Its menu has two sections:
  1. Every profile in list order, as radio items on `win.open-profile(s id)`. The action's state
     is the current profile id, so the current profile shows checked.
  2. `_Add Profile…` (`win.add-profile`) and `_Manage Profiles…` (`win.manage-profiles`).
- Opening the menu re-reads the list (`profile_used`) and rebuilds the button (`sync_profile`).
- If the profile has no entry yet, the fallback avatar uses the directory id as the name, in
  Slate.
- The picker switch also lives in Settings > General: switch row
  `Show Profile Picker at Startup`, subtitle `Ask which profile to open when Vsesvit starts and
  there is more than one`, plus a `Manage Profiles…` button row (dialogs/settings.rs).

### 13.8 Persistent state (core-owned)
- `profiles/profiles.json` (written atomically through a temporary file and rename) holds:
  - `profiles` (`id` = directory name, `name`, `color` snake_case, optional `picture` PNG file
    name, `set_by_hand`)
  - `last_used`
  - `show_picker` (default true)
  - `removed` (profiles waiting to be deleted)
- Every writer holds the exclusive lock on `profiles/profiles.lock`. Readers take no lock.
- Per profile: `<root>/LOCK`, and the account picture
  `<root>/Account Picture <sha256 prefix>.png`.
- The self-test keeps its list in `OUT_DIR/profiles.json` with profile id `profile`
  (`Home { dir: ProfilesDir::at(out_dir), id: "profile" }`).

---

## 14. Startup windows and welcome (browser.rs `open_startup_windows`) **(KEEP)**

`open_startup_windows(targets: &[Url])` builds the first windows of a run, and also the windows
after an invocation when only private windows were open:

1. `startup = pref(keys::STARTUP)`:
   - `restore_session` (the default): `core.session().restore()`. `Ok(Some(snapshot))` runs
     `session::restore(browser, snapshot)` and counts the windows opened. `Ok(None)` opens 0. An
     error warns `cannot restore the session: {e}` and opens 0.
   - `homepage` and `new_tab` restore nothing.
2. If no window was restored **and** there are no targets: one new normal window.
   - `startup == homepage` with a homepage URL: one tab on the homepage.
   - Otherwise `new_tab()`: the new tab page (core `new_tab::page`), with focus in the address
     bar.
   - Present it.
3. If there are targets: they go into the most recently focused normal window (the restored
   active one) or a new window, **with no blank tab beside them**, through `open_tabs(targets)`
   (the first is selected). Present it.
4. If `welcome` is still set (taken once), present `dialogs::welcome::present` over the first
   normal window. That is the profile's first run (`Profile::is_new()` and not
   `onboarding.done`), and never in scripted runs.

---

## 15. Session persistence (session.rs, browser.rs) **(KEEP)**

### Snapshot: `session::snapshot(browser) -> vsesvit_core::session::SessionSnapshot`
- `windows`: every window **most recently focused first**, each through `window_snapshot`, which
  skips windows with no saveable tab. Private windows yield nothing because their tabs are
  filtered out.
- `active_window: 0`. The active window is first by construction.
- `device_name`: the `device.name` pref (local) if it is not blank, else
  `glib::host_name()`. **(NEEDS-GPUI)** Use `gethostname`.
- `window_snapshot(window)`:
  - Tabs: only `Browsing::Normal` tabs whose `session_uri()` parses as a URL. `session_uri` is
    the committed URI, else the one still loading, as for a restored or background tab whose
    first page has not arrived.
  - `active_tab`: the index of the selected tab among the saved tabs, else 0.
  - Each `TabSnapshot`:
    - `id: tab.session_id()` (a stable UUID, `session::TabId`)
    - `url`
    - `title: display_title()`
    - `pinned`
    - `last_active_ms: tab.last_active_ms()`
    - `group: window.group_of(tab)` (`Option<TabGroup>`: id, title, colour, collapsed)
    - `restore_state: tab.session_state_bytes()` (the serialized WebKit `WebViewSessionState`)
  - `bounds: Some((0, 0, width, height))` from `default_size()`. The origin is always 0,0 on
    Wayland.
  - `maximized`.
  - `display_title()` (tab.rs): the asleep tab's kept title, else the page title, else the
    display form of the committed URI, else `New Tab`.

### Save
- `schedule_session_save()`: arms one 2 s timer (`SESSION_SAVE_DELAY`) unless one is already
  pending or the browser has shut down.
  - **It is a throttle, not a debounce**: later changes do not push the timer back, so a save
    happens about 2 s after the first change of a burst. The doc comment says "about two seconds
    after the last tab change", but the code does this.
  - When the timer fires, `save_session_now()`.
- Triggers include tab closed, moved, activated, navigation committed, title changed, tab put to
  sleep, a page detached (window/mod.rs), and others in window code.
- `save_session_now()`:
  - cancels the pending timer
  - **writes nothing if no normal window exists or the browser has shut down**, so quitting, or
    closing the last normal window while a private one stays, never overwrites the last real
    session with an empty one
  - otherwise `core.session().save(&snapshot)`; errors warn `cannot save the session: {e}`
- Explicit saves:
  - `app.quit` saves before quitting.
  - `Browser::shutdown` saves.
  - **When the last tab of the last normal window closes**, the session is saved while that tab
    still exists, so the next start restores it, as Chrome does (window/mod.rs:493).
  - **When the last normal window closes** with tabs (`before_close`), the session is saved
    first.
  - The self-test's `session` check saves on demand.
- Core's `Session::save`: one transaction writes this device's `device_sessions` record (a new
  stamp and seq only if the snapshot changed; restore blobs are not part of equality) and
  replaces `tab_restore_state` for the listed tabs. That record is also the device's synced
  "open tabs" record (Sessions kind). Other devices see it through `session().other_devices()`
  (14-day window).

### Restore: `session::restore(browser, snapshot) -> usize`
1. Move `snapshot.windows[active_window]` to index 0. Create windows in **reverse** order, so the
   active one is created and presented last: it is on top and first in `windows()`, where
   command-line URLs go and the next save puts it.
2. Per saved window with tabs:
   - `BrowserWindow::new`
   - if bounds has width and height above 0, `set_default_size(w, h)`
   - if `maximized`, `maximize()`
   - for each tab, in order:
     - `open_tab(None, None, Background)`
     - `set_pinned(tab, pinned)`
     - `set_session_id(id)`
     - `mark_active(last_active_ms)`
     - `restore_saved(restore_state, url)`: restore the WebKit back/forward state if this WebKit
       can decode it, else load the URL. A blob that cannot be decoded logs
       `this WebKit cannot read the saved history of {url}; loading the page alone`.
   - select the saved `active_tab`, else the first tab
   - **after** selecting, `window.restore_groups(groups)`. Selecting a tab expands its group, so
     collapsed groups are restored after.
   - `present()`
3. Return how many windows opened.

### Tests that pin it
- `a_tab_still_loading_its_first_page_is_saved`
- `a_restored_tab_keeps_its_back_forward_state_until_it_commits`
- `pinned_tabs_are_restored_pinned_and_saved_again`
- `grouped_tabs_are_restored_in_their_groups_and_saved_again` (collapsed group rows hidden)
- `a_restored_session_keeps_its_active_window_first`

### Classification
- Snapshot and restore rules: **(KEEP)**.
- `restore_state` blobs: **(WEBKIT-ONLY)** format. **(NEEDS-CEF)** CEF can read entries
  (`GetNavigationEntries`) but cannot restore a back/forward list. Restored tabs would load
  `url` alone, as Windows does.
  - Profiles migrated from the GTK shell hold WebKit blobs in `tab_restore_state` that CEF cannot
    use. The new shell must ignore them safely. If it writes its own format into the same opaque
    column, it must be able to tell formats apart. **(inferred)**
- Window geometry: GPUI window size and maximized state; Wayland gives no position.
  **(NEEDS-GPUI)**

---

## 16. Keyboard shortcuts (keymap.rs) **(KEEP behavior; GTK-ONLY accelerator plumbing)**

Core owns the model: `vsesvit_core::shortcuts::{Command, Chord, Key, Mods, Keymap}`, and
`Keymap::from_overrides`, `chords(cmd)`, `assign`, `reset`, `reset_all`, `offer`, `iter`.
Overrides are stored in the synced pref `keyboard.shortcuts` (`Overrides =
BTreeMap<command id, Vec<chord string>>`), read through `prefs().keymap()` and written through
`prefs().set_keymap()`.

### Command to shell binding (`keymap::binding`), with core's defaults

| Command (id) | Linux binding | Default chords (core) |
|---|---|---|
| NewTab (`new-tab`) | `win.new-tab` | Ctrl+T |
| CloseTab | `win.close-tab` | Ctrl+W, Ctrl+F4 |
| ReopenClosedTab | `win.reopen-closed-tab` | Ctrl+Shift+T |
| SearchTabs | `win.search-tabs` | Ctrl+Shift+A |
| NextTab | **BuiltIn** `<Control>Tab` (fixed, not reassignable) | Ctrl+Tab |
| PreviousTab | **BuiltIn** `<Control><Shift>Tab` | Ctrl+Shift+Tab |
| SelectTab1…8, SelectLastTab | **none**: the GTK shell does not bind them | Ctrl+1…8, Ctrl+9 |
| ToggleTabList (`toggle-tab-list`) | `win.toggle-tab-sidebar` | **Ctrl+S**, F9 (Vsesvit's own choice: Chrome's Ctrl+S save moves to Ctrl+Shift+S) |
| NewWindow | `app.new-window` | Ctrl+N |
| NewPrivateWindow | `app.new-private-window` | Ctrl+Shift+N |
| Quit | `app.quit` | Ctrl+Q |
| FocusAddress | `win.focus-location` | Ctrl+L, Alt+D, F6 |
| Back | `win.back` | Alt+Left |
| Forward | `win.forward` | Alt+Right |
| Reload | `win.reload` | Ctrl+R, F5 |
| ReloadBypassCache | `win.reload-bypass-cache` | Ctrl+Shift+R, Shift+F5 |
| SavePage | `win.save-page` | Ctrl+Shift+S |
| Print | `win.print` | Ctrl+P |
| Find | `win.find` | Ctrl+F |
| FindNext | `win.find-next` | Ctrl+G |
| FindPrevious | `win.find-previous` | Ctrl+Shift+G |
| BookmarkPage | `win.bookmark-page` | Ctrl+D |
| CopyCleanLink | `win.copy-clean-link` | Ctrl+Shift+C (Chrome uses it for the element inspector) |
| CopyLink | `win.copy-link` | Ctrl+Alt+Shift+C |
| ViewSource | `win.view-source` | Ctrl+U |
| ZoomIn | `win.zoom-in` | Ctrl+Plus, Ctrl+Equal, Ctrl+KeypadPlus |
| ZoomOut | `win.zoom-out` | Ctrl+Minus, Ctrl+KeypadMinus |
| ZoomReset | `win.zoom-reset` | Ctrl+0, Ctrl+Keypad0 |
| Fullscreen | `win.fullscreen` | F11 |
| ToggleBookmarksBar | `win.show-bookmarks-bar` | Ctrl+Shift+B |
| ShowBookmarks | `win.show-bookmarks` | Ctrl+Shift+O |
| ShowHistory | `win.show-history` | Ctrl+H |
| ShowDownloads | `win.show-downloads` | Ctrl+J |
| ShowSettings | `win.show-settings` | Ctrl+Comma |
| ShowShortcuts | `app.shortcuts` | Ctrl+Question |
| DeveloperTools | `win.developer-tools` | Ctrl+Shift+I, F12 |
| JavaScriptConsole | `win.javascript-console` | Ctrl+Shift+J |

Notes:
- Sections, used in the help and Settings in this order: `Tabs and Windows`, `Navigation`,
  `Page`, `General` (`Section::title`). Command titles come from core (`Command::title`): "New
  tab", "Close tab", and so on.
- `keymap::actions()` yields the reassignable commands (`Binding::Action`) in `Command::ALL`
  order. That is the list Settings > Shortcuts shows. BuiltIns appear only in the shortcuts help
  dialog (`adw::ShortcutsItem`) and can never be reassigned. `Keymap::offer` refuses chords that
  a BuiltIn holds.
- **(GTK-ONLY, inferred)** Tab-number selection: the Linux shell binds no Ctrl+1…9. The window
  removes only `AdwTabView`'s Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PageUp and Ctrl+PageDown shortcuts
  and handles those itself: `<Control>Tab|<Control>KP_Tab|<Control>Page_Down` goes forward and
  `<Control><Shift>Tab|<Control><Shift>ISO_Left_Tab|<Control><Shift>KP_Tab|<Control>Page_Up` goes
  back, skipping tabs hidden in collapsed groups and wrapping around. libadwaita's remaining
  defaults (Alt+1…9, Alt+0, Ctrl+Home/End, Ctrl+Shift+PageUp/PageDown, and so on) are presumably
  still active. The rewrite must decide whether to bind core's Ctrl+1…9 defaults. Windows
  probably does; not checked here.
- **Self-test expectations**:
  - `Ctrl+S` runs exactly `win.toggle-tab-sidebar` and toggles the sidebar
    (`ctrl_s_toggles_sidebar`).
  - In the `shortcuts` check:
    - assigning Ctrl+Shift+Y to History frees Ctrl+H
    - assigning Ctrl+J to History takes it from Downloads, reported as `taken from
      ["Downloads"]`, and leaves `win.show-downloads` with no accelerator
    - Settings rows show the result
    - capturing Ctrl+T for History notes `Also used by New tab. Saving moves it here.`
    - Reset All restores every default (Ctrl+Shift+S on `win.save-page`, Ctrl+S on
      `win.toggle-tab-sidebar`)
  - `app.new-private-window` holds `<Control><Shift>n` (`private_window`).

### Applying
- `keymap::apply(app, keymap, extension_shortcuts)`:
  - for each reassignable action, `app.set_accels_for_action(action, chords as accelerators)`
  - clear every `win.extension-command(...)` accelerator
  - set each extension command that has a chord
- `Browser::edit_keymap(edit)`, which every change goes through:
  1. `keymap()`
  2. `edit(&mut keymap)`
  3. `prefs.set_keymap` (warns `prefs: {e}`)
  4. `keymap::apply`
  5. return the edit's result (for example `Vec<Taken>` for "taken from")
- `Browser::apply_keymap()` runs at start, after any change to the enabled extensions, and after
  sync changes `keyboard.shortcuts`.
- `keymap::suspend(app)` removes every accelerator until the next `apply`, so the shortcut capture
  dialog sees chords that the window would otherwise run.
- Extension commands:
  - `Browser::extension_shortcuts(keymap) =
    keymap.extension_shortcuts(commands::extension_commands(&installed_extensions()))`.
  - Core resolves conflicts: browser shortcuts win, then the user's changes, then suggested keys
    in install order.
  - Each command runs the window action `win.extension-command` with target `(extension id,
    command name)`. The detailed name is
    `win.extension-command(('id', 'name'))`, built by
    `gio::Action::print_detailed_name`.
  - **(WEBKIT-ONLY / NEEDS-CEF)** With Chromium's extension system, `commands` and `_execute_action`
    are Chromium's to run. Whether CEF dispatches extension commands when the shell, not Chrome,
    owns the keyboard is open. **(inferred)**
- Accelerator strings (`accelerator(chord)`):
  - modifiers in the order `<Control>`, `<Alt>`, `<Shift>`, then the GDK keyval name
  - letters lower case (`<Control><Shift>s`)
  - renamed keys: `space`, `Page_Up`, `Page_Down`, `plus`, `minus`, `equal`, `comma`, `period`,
    `slash`, `question`, `semicolon`, `KP_Add`, `KP_Subtract`, `KP_0`…`KP_9`
  - digits, F1–F24, `Tab`, `Insert`, `Delete`, `Home`, `End`, arrows: core's name
  - `accelerators(chords)` joins them with spaces, for `AdwShortcutLabel`
  - **(GTK-ONLY)** **(NEEDS-GPUI)** Map `Chord` to GPUI keystrokes (`ctrl-shift-s`) and register
    them as actions in a keymap context. Shortcuts must work while a CEF browser has focus.
    **(NEEDS-CEF)** `CefKeyboardHandler::OnPreKeyEvent` must hand browser shortcuts back to the
    shell before the page sees them. Note that a shortcut on a bare F-key (F5, F6, F9, F11, F12) is
    allowed.
- **Capture dialog key interpretation** (`keymap::pressed(keyval, state, consumed, base) ->
  Pressed`) **(KEEP rules)**:
  - Without modifiers: `Escape` is Cancel, `BackSpace` is Clear, `Return`, `KP_Enter` or
    `ISO_Enter` is Confirm.
  - A letter keeps Shift (`Ctrl+Shift+S`).
  - A symbol typed with Shift is named by what it typed, with Shift dropped: Ctrl+Shift+/ on a
    US layout is `Ctrl+Question`, Ctrl+Shift+= is `Ctrl+Plus`.
  - A shifted symbol core has no name for falls back to the base key and keeps Shift (Ctrl+! is
    `Ctrl+Shift+1`).
  - `ISO_Left_Tab` is Tab with Shift kept.
  - A key core cannot name is `Unusable` (Ctrl+BackSpace, Ctrl+Print).
  - A nameable key without Ctrl or Alt and not an F-key is `NeedsModifier` (`h`, Shift+H).
  - Otherwise `Chord(Chord::new(mods, key))`.
  - Core supplies the notes shown for NeedsModifier and Unusable.
  - Tests: `a_key_press_becomes_the_chord_it_names` and
    `capture_keys_and_unbindable_presses_are_told_apart`.

---

## 17. Animation timing (motion.rs) **(KEEP values; GTK-ONLY implementation)**
- `TAB_ROW_MS = 180` ms is the time a tab row takes to grow into or shrink out of the tab list.
- `EASING = adw::Easing::EaseOutCubic`.
- `fade(widget, to, duration_ms, done)`: a timed opacity animation from the current opacity to
  `to`, then `done`. The caller keeps the animation alive.
- The animations skip to the end when the widget is not mapped or when the desktop turns
  animations off (`gtk-enable-animations`). **(NEEDS-GPUI)** Honour the desktop's
  reduce-animations setting: the `org.gnome.desktop.interface enable-animations` setting, or
  `gtk-enable-animations` through the settings portal. **(inferred)**

---

## 18. Self-test interactions with these modules

`vsesvit --self-test OUT_DIR [--network]` (`self_test::run`):

1. `create_dir_all(out_dir)`. Failure prints `vsesvit: cannot create {dir}: {e}` and exits with
   `FAILURE`.
2. `OUT_DIR/profile` is deleted, then **`SCRIPTED = true`**.
   `Report::new("linux", expected(network))` covers 62 checks, plus `cws_install` with
   `--network`.
3. `Profile::open(OUT_DIR/profile)` is the `profile_open` check. Then `search_suggestions` is set
   off, the fixture server starts (`FixtureServer::start`), `favicons::allow_local_hosts()` runs,
   and `OUT_DIR/probe.crx` is written from `testkit::probe_crx()`.
4. `OUT_DIR/profiles.json` is deleted. The home is
   `Home { dir: ProfilesDir::at(out_dir), id: "profile" }`, which never touches the user's list.
5. Its own application: `application_id = "dev.mrquantumoff.vsesvit.SelfTest"`, `NON_UNIQUE`, so
   no forwarding.
6. On `startup`:
   - `app::setup` (same name, icon, CSS and actions as a real run)
   - `Browser::new(app, profile, Some(home))`
   - `browser.start()`
   - `browser.open_startup_windows(&[])`, which bypasses the command line and the blockers wait
   - then it holds the app and runs `run_checks`
   - finally it closes every window, drops the hold and quits
7. `finish()` writes `OUT_DIR/report.json`, prints
   `[self-test] PASS|FAIL: {passed}/{n} checks passed; report at {path}`, and exits 0 only if
   every check passed. Each check prints `[self-test] {name}: ok|FAIL ({ms} ms) {detail}` as it
   runs.

Checks that exercise these files directly:
- `session`: `save_session_now`, then `core.session().restore()` has at least one window with
  tabs.
- `private_window`:
  - accelerator on `app.new-private-window`
  - title `Vsesvit fixture 2 (Private)` and CSS class `private`
  - ephemeral session, private closed stack, zoom kept in private only
  - tab search isolated by kind
  - session and history exclude the tab
  - `private_session_lasts()` false after the window closes
  - private download rows dropped
- `profiles`:
  - the menu names `["Person 1"]` and the title equals the page title with one profile
  - after adding Work through core and `set_profiles`: menu `["Person 1","Work"]`, title
    `"{page} - Person 1"`
  - a private window titled `"{page} - Person 1 (Private)"` with no profile menu
  - account details through `browser.sync().take_account_details(AccountDetails{...})`
  - `edit_profile(home.id, "Tester", Teal)` gives title `"{page} - Tester"`
  - a later sign-in as Sam changes neither name nor picture
  - Manage Profiles rows `Tester`, `Work`
  - turning the Settings picker switch off stores `show_picker=false`
  - `remove_profile(window, work)` deletes its directory, and the menu and title go back to one
    profile
  - screenshots `profiles-account.png`, `profiles-manage.png`, `profiles.png`
  - "No process is started": profiles are added through core, not launched.
- `shortcuts` and `ctrl_s_toggles_sidebar` (§16), `tab_search`, `memory_saver`
  (`sleep_idle_tabs(now + 7h)`), `favicon_preload`, `bookmark` and `star_bubble`
  (`star_clicked`), `history_recorded`, `zoom_is_remembered_per_site`, `extension_toolbar`
  (`pin_extension` / `move_extension`), `extension_windows` (TabHost), `extension_commands`,
  `welcome` (resets `onboarding.done` and opens the welcome on purpose), `screenshot`
  (`window.png` from an in-app capture that is not one flat colour).
- **(NEEDS-GPUI / NEEDS-CEF)** The self-test drives widgets directly and captures windows in-app
  without stealing focus. GPUI needs a way to render a window to an image. CEF content is
  composited by CEF itself, either a child window or off-screen rendering
  (`windowless_rendering_enabled`, `CefRenderHandler::OnPaint` / `OnAcceleratedPaint`), so the
  capture must include it. `docs/design/self-test.md` is the cross-platform contract.

Unit tests in these files (the GTK shell's `cargo test`) are listed per section above. They need
a display and share one browser per process because the webext runtime allows only one.

---

## 19. Logging
- The `log` crate goes into GLib through `glib::GlibLogger` (Plain format, domain = the record
  target), with max level Debug (§2).
- **(inferred, from GLib defaults)** Error maps to critical, Warn to warning, Info to info, and
  Debug and Trace to debug. GLib's default writer sends to the journal when stderr is the
  journal, else stderr. Info and debug are hidden unless `G_MESSAGES_DEBUG` is set.
- There is **no log file** on Linux. Windows writes `<profile>/vsesvit.log` and honours
  `VSESVIT_LOG` (vsesvit-winui `logging.rs`). The rewrite may want parity. **(KEEP-or-decide)**
- Private addresses stay out of logs: `vsesvit_core::private::Browsing::loggable(&addr)` prints
  `(a private page)` for private tabs. **(KEEP)**
- Messages in these files:
  - CLI errors and fatal startup errors on stderr, prefixed `vsesvit: `
  - `restarted into …`, `could not start …`, `the profile is still open; the restarted browser
    may find it locked`
  - `the profile list: {e}`, `cannot restore the session: {e}`, `cannot save the session: {e}`
  - `history: {e}`, `history title: {e}`, `site zoom: {e}`, `prefs: {e}`, `extension toolbar:
    {e}`
  - `search suggestions: {e}`, `favicons to fetch: {e}`, `storing fetched favicons: {e}`, `the
    favicon fetch panicked`
  - `cannot list extensions: {e}`, `extension {id}: {e}`
  - `automatic updates are off`, and the updater's disabled reason at info level
- **(NEEDS-CEF)** CEF has its own log (`CefSettings.log_file`, `log_severity`), which should go
  somewhere sensible, for example the profile directory or stderr.

---

## 20. Persistent state these modules read or write

| What | Where | Read or written by |
|---|---|---|
| Profile lock | `<root>/LOCK` (OS exclusive lock) | `Profile::open`; `ProfilesDir::is_running` / `delete` |
| Profile list | `profiles/profiles.json` + `profiles/profiles.lock` | `Start::resolve` (sweep, startup), `Browser::new` (opened), `profile_used`, add, edit, remove, `set_show_picker`, picker |
| Session record | `vsesvit.db` `device_sessions` (synced Sessions kind) | `save_session_now` and `open_startup_windows` (restore) |
| Restore blobs | `vsesvit.db` `tab_restore_state` (local) | the same; the format is WebKit's today |
| `startup` pref (synced) | prefs | `open_startup_windows` |
| `homepage` (synced, default `about:home`) | prefs | `homepage()` |
| `tabs.position`, `bookmarks_bar.visible`, `toolbar.home_button`, `address_bar.compact`, `address_bar.full_urls`, `theme` (synced) | prefs | the setters in §11.11 |
| `content.block_popups`, `scrolling.smooth`, `spellcheck.enabled`, `spellcheck.languages` (synced); `system.hardware_acceleration` (local) | prefs | `set_engine_switch`, `set_spellcheck_language`, `Engine::apply_prefs` |
| `privacy.tracking_protection`, `privacy.third_party_cookies` (synced) | prefs | setters; `sync_applied` |
| `updates.automatic`, `updates.channel` (local) | prefs | `Browser::new`, setters |
| `performance.memory_saver`, `performance.memory_saver.mode` (local) | prefs | `Sweep::new` |
| `device.name` (local) | prefs | session snapshot |
| `onboarding.done` (local) | prefs | the welcome decision |
| `keyboard.shortcuts` (synced) | prefs | `keymap()`, `edit_keymap` |
| `toolbar.extensions` (synced) | prefs | extension toolbar layout, pin and move |
| `extensions.allowed_in_private` (synced) | prefs | `sync_applied` |
| `address_bar.suggest.search` (synced) | prefs | `suggest_request`; set false by the self-test |
| `privacy.https_only` (synced) | prefs | `https_upgrade` |
| `downloads.directory` (local; `None` = XDG Downloads) | prefs | `Downloads::new(..., downloads_dir())` |
| History visits and titles | `vsesvit.db` | `navigation_committed`, `title_changed` |
| Site zoom (local; private in memory) | `vsesvit.db` | `show_at_site_zoom`, `zoom_changed` |
| Bookmarks (in memory + db) | `vsesvit.db` | `star_clicked`, `is_bookmarked` |
| Favicons of bookmarked sites (local) | `vsesvit.db` | `save_favicon`, `preload_favicons` |
| Engine data and cache | `<root>/engine/`, `~/.cache/vsesvit/<name>/` | WebKit (**WEBKIT-ONLY**) |
| Update downloads | `~/.cache/vsesvit/updates` | `Updates`, headless commands |
| Package marker | `<exe dir>/package-format`, `$APPIMAGE` | `Installation::detect` |
| Self-test outputs | `OUT_DIR/{profile/, profiles.json, probe.crx, report.json, window.png, *.png, downloads/, saved-page.mhtml}` | self-test |

---

## 21. Engine- and toolkit-touching pieces, at a glance

| Piece | Tag | CEF / GPUI equivalent (named where known) |
|---|---|---|
| Subprocess dispatch before CLI parsing | NEEDS-CEF | `cef_execute_process` |
| One instance per profile, forwarding argv and cwd | KEEP + NEEDS-CEF | per-profile `root_cache_path` singleton + `CefBrowserProcessHandler::OnAlreadyRunningAppRelaunch`, or a custom socket. Today it is GApplication on D-Bus. |
| Activation token when launching another profile | NEEDS-GPUI | xdg-activation-v1 token in `XDG_ACTIVATION_TOKEN` |
| Engine data dirs per profile | NEEDS-CEF | `CefSettings.root_cache_path`, `CefRequestContextSettings.cache_path`; extend `ProfilesDir::delete` |
| Private ephemeral session | NEEDS-CEF | `CefRequestContext` with empty `cache_path` (incognito) |
| Commit and transition tracking for history | NEEDS-CEF | `CefLoadHandler`, `CefRequestHandler::OnBeforeBrowse`, `CefDisplayHandler::OnAddressChange`; DevTools for same-document navigations |
| Titles | NEEDS-CEF (+ WEBKIT-ONLY empty-title workaround) | `CefDisplayHandler::OnTitleChange` |
| Favicons | NEEDS-CEF | `OnFaviconURLChange` + `CefBrowserHost::DownloadImage` |
| Site zoom on every new document | KEEP + NEEDS-CEF (WEBKIT-ONLY reason) | `SetZoomLevel` (log scale), reconciled with Chromium's per-host zoom |
| Back/forward state for closed and restored tabs | NEEDS-CEF (gap) | `GetNavigationEntries` (read-only); no restore API |
| Memory Saver sleep and wake | NEEDS-CEF (gap) | close and recreate the browser, or `WasHidden`; audio from `CefAudioHandler`; capture from `OnMediaAccessChange`; JS results through DevTools `Runtime.evaluate` (`ExecuteDevToolsMethod`) or renderer IPC |
| Engine prefs (pop-ups, smooth scrolling, GPU, spell check) | NEEDS-CEF | `OnBeforePopup` (user gesture), command-line switches (restart), `CefRequestContext::SetPreference` |
| Notification permission seeding | WEBKIT-ONLY → NEEDS-CEF | `CefPermissionHandler::OnShowPermissionPrompt`, `SetContentSetting` / `SetWebsiteSetting` |
| HTTPS-only upgrade | KEEP + NEEDS-CEF | `OnBeforeBrowse` (main frame known) |
| Tracking protection and cookie blockers before the first load | KEEP + NEEDS-CEF | resource request handler (`GetResourceRequestHandler`), cookie manager; the "wait for compiled blockers" step is WEBKIT-ONLY |
| `vsesvit-webext` runtime + `TabHost` | WEBKIT-ONLY | Chromium's extension system; tab and window model compatibility is the open risk |
| Extension notification app action | WEBKIT-ONLY | Chromium notifications |
| Extension commands as `win.extension-command` | WEBKIT-ONLY → NEEDS-CEF | Chromium `commands`; shell keyboard routing |
| Theme (light, dark, system) | KEEP + NEEDS-GPUI (+ NEEDS-CEF for web content) | portal color-scheme; CEF color scheme preference (needs research) |
| Accelerators | KEEP + NEEDS-GPUI + NEEDS-CEF | GPUI keymap; `CefKeyboardHandler::OnPreKeyEvent` |
| Popover anchoring, scripted no-autohide | KEEP + NEEDS-GPUI | GPUI popovers and menus |
| In-app screenshot | NEEDS-GPUI + NEEDS-CEF | window render to image including CEF content |
| CSS visual specs | KEEP (values) / GTK-ONLY (mechanism) | Appendix A |
| Logging into GLib | GTK-ONLY | any `log` backend; decide on a log file |
| Downloads dir, cache dir, host name lookups through GLib | GTK-ONLY | `dirs` / `directories` / `gethostname` |

---

## 22. How these modules connect to the rest of the shell

- **window/** (`BrowserWindow`): constructed by `Browser::open_window`, `open_startup_windows`,
  `session::restore`, `TabHost::create_window`. Calls back for:
  - tab lifecycle (`tab_closed`, `tab_activated`, `tab_moved`, `tab_attached`, `tab_discarded`)
  - `is_last_normal_window`, `save_session_now`, `schedule_session_save`
  - `profile_used`, `profile_title`, `profiles`, `open_profile`
  - `star_clicked`, `omnibox_changed`, `omnibox_activated`
  - `search_tabs`, `tab_icon`, `go_to_tab`, `reopen_closed_tab`
  - `extension_actions`, `extension_toolbar`, `pin_extension`, `move_extension`
  - and the pref getters
  - The browser calls `apply_layout`, `set_*_visible`, `apply_prefs`, `refresh_bookmarks_bar`,
    `sync_star`, `refresh_extension_actions`, `profiles_changed`, `restore_closed`,
    `restore_groups`, `show_bookmark_bubble`, `toast`, `present`.
- **tab.rs** (`Tab`): `committed_uri`, `session_uri`, `display_title`, `session_state_bytes`,
  `restore_saved`, `restore`, sleep (`sleeps`, `sleep`, `keep_awake`, `has_edits`), `capturing`,
  `is_related`, `take_pending_transition`, `shows_error_page`, `used`/`set_used`,
  `session_id`, `last_active_ms`/`mark_active`. Tabs call `navigation_committed`,
  `title_changed`, `favicon_changed`, `zoom_changed`, `https_upgrade`, `network_session`.
- **engine.rs**, **trackers.rs**, **cookies.rs**, **downloads.rs**, **permissions.rs**: owned by
  the browser and created in `Browser::new` (§11.2).
- **extensions.rs**: `impl Browser` for install, enable, uninstall, update, reconcile and
  load into the runtime. Calls `apply_keymap`.
- **sync.rs** (`Syncer`): created in `Browser::new`, `final_sync` at shutdown,
  `take_account_details` (profile name and picture from the sync account, §13), calls
  `sync_applied`.
- **updates/**: `Updates::new` in `Browser::new`, `window_opened` per window, `restart_program`
  after `run`, `app.update`.
- **dialogs/**: `about`, `welcome`, `shortcuts`, `extensions` (from app actions),
  `extension_prompts` (TabHost permission prompt), `settings` (the picker switch, Manage, the
  pref setters, `watch_prefs`), `bookmarks`, `history`, `downloads` (`windowed` registry,
  `watch_bookmarks`, `watch_history`).
- **location.rs**: `resolve_cli_target` and `homepage_url`.
- **omnibox.rs**: `suggestions(...)`.
- **favicons.rs**: `record(...)`.
- **closed_tabs.rs**: the stack.
- **screenshot.rs**: debug action and self-test.
- **self_test.rs**: §18.

---

## Appendix A: visual values from `app::CSS` and `avatar_css` (KEEP as design reference)

Uses libadwaita named colours (`@view_bg_color`, `@accent_bg_color`, `@window_fg_color`,
`@sidebar_bg_color`, and so on).

- `.link-preview` (the hovered-link URL preview): padding 3px 8px; top-right radius 6px;
  background `@view_bg_color`; colour `@view_fg_color`; 1px ring `alpha(currentColor,0.15)`;
  font 0.9em.
- `.fullscreen-notice`: padding 12px 20px; radius 12px.
- Bookmarks bar:
  - `.bookmarks-bar`: min-height 28px; padding 2px 6px.
  - Its buttons: padding 2px 6px; min-height 24px.
  - Drop indicators: `.drop-before` is an inset 2px accent line on the left, `.drop-after` on the
    right.
  - Bookmark rows in lists use top and bottom 2px accent lines.
  - `.drop-into`: accent at 25% alpha, radius 6px.
  - `.bookmarks-chevron`: padding 2px 8px, bold.
- Bookmark menu popover contents: padding 4px. Rows: padding 4px 8px; min-height 24px; normal
  weight.
- Address bar loading progress: a 2px line inside the entry's bottom edge (margin 0 10px 1px),
  radius 1px, with a gradient from transparent `@window_fg_color` to `@window_fg_color`
  left to right, no border or shadow.
- `.address-zoom` pill: min-height 20px; padding 0 6px; font 0.85em; radius 10px.
- Tab sidebar:
  - background `@sidebar_bg_color`, transparent list, rows with no padding
  - `.tab-row`: padding 4px 6px 4px 10px; min-height 30px
  - `.tab-close`: 22×22, padding 0, opacity 0.6, 1 on hover
  - hidden rows have zero vertical margin
- Tab groups:
  - `.tab-group-header`: margin 4px 6px 0; padding 2px 6px 2px 10px; min-height 24px
  - `.tab-group-chip`: padding 1px 8px; radius 9px; font 0.85em bold
  - an untitled chip is a 12×12 dot (radius 6px)
  - `.tab-group-button`: padding 2px 4px
  - `.tab-group-swatch`: 24×24
  - `.tab-group-action`: normal weight
  - group colours come from `window::install_group_style`
- **Private window header**: background `#2e2e32`, foreground `#ffffff`, backdrop `#222226`,
  border `#ffffff`, shade `rgb(0 0 6 / 36%)`, darker shade `rgb(0 0 12 / 90%)`.
- `.extension-badge`: font 0.65em bold; padding 0 3px; min 10×12; radius 6px; accent background
  and foreground.
- Profiles:
  - `.profile-avatar`: fully round, white, bold, min 22×22
  - `.large`: 72×72, font 2em
  - `.profile-card`: padding 12px
  - one background colour per `ProfileColor` (§13.1)

---

## Checklist

- [ ] `main` returns the new shell's `ExitCode` on Linux; non-Linux, non-Windows targets print "vsesvit supports Windows and Linux only"
- [ ] CEF subprocess invocations are handled (`cef_execute_process`) before CLI parsing
- [ ] `self-test` is a default cargo feature mapped to `vsesvit-core/testkit`; `--no-default-features` removes it
- [ ] The GTK, adw, webkit and webext dependencies and CI apt packages are removed or replaced
- [ ] `APP_ID` `dev.mrquantumoff.vsesvit` is the window app id / WM class, the default icon name and the app name "Vsesvit"
- [ ] CLI: `vsesvit [URL...] [--profile-dir DIR]` with targets and options in any order
- [ ] CLI: `--profile-dir=DIR` form; empty value is an error; repeated is an error
- [ ] CLI: `--` ends options; `-` is a target; non-UTF-8 arguments are targets
- [ ] CLI: `-h`/`--help` prints USAGE (exit 0); `--version` prints `vsesvit X.Y.Z` (exit 0); both win over everything
- [ ] CLI: unknown options give `vsesvit: unknown option X` plus USAGE on stderr, exit 2
- [ ] CLI: standalone commands are exclusive and not repeatable; `--network` needs `--self-test`; error texts match exactly
- [ ] USAGE text reproduced verbatim
- [ ] `--check-for-updates` prints one JSON line through `vsesvit_update::cli::check` (build channel, no profile, no window)
- [ ] `--update` checks, downloads to `~/.cache/vsesvit/updates`, installs, prints JSON (progress on stderr)
- [ ] `--self-test OUT_DIR [--network]` runs on `OUT_DIR/profile`, writes report.json and window.png, exits 0 only if all checks pass
- [ ] Self-test is scripted: popovers stay open, no welcome, no scheduled extension updates, its own app id, no forwarding
- [ ] Self-test profile list lives in `OUT_DIR/profiles.json` (id `profile`); search suggestions off; fixture server; probe.crx
- [ ] Without the self-test feature, `--self-test` prints the "built without" message and fails
- [ ] Profile resolution: `--profile-dir` is created and canonicalized; otherwise sweep, then last used / Default / first / picker
- [ ] Picker condition: more than one profile, show_picker on, no URLs, no listed profile running
- [ ] A `--profile-dir` inside the standard profiles dir is a listed profile (Home); outside it there is no profile button or menu
- [ ] Instance identity is per profile: plain id for Default (any spelling), hashed id otherwise
- [ ] Bad profile dir prints `vsesvit: cannot use the profile directory: {e}` and fails
- [ ] `Profile::open` errors other than Locked print `vsesvit: cannot open the profile at {root}: {e}` and fail
- [ ] A profile found removed at startup is swept and fails with `the profile at … was removed`
- [ ] A second launch on a locked profile forwards argv and cwd to the owner and exits with the owner's status
- [ ] Locked but unreachable owner prints "the profile is in use by another Vsesvit process that could not be reached" and fails
- [ ] Forwarded targets resolve against the invoker's cwd (file first, then address-bar URL classification, never search)
- [ ] Unresolvable targets print `vsesvit: cannot open X` to the invoker's stderr; the rest still open
- [ ] No window opens until tracking-protection and cookie blockers apply and clear-on-start data is deleted
- [ ] Invocation with no normal windows runs the full startup windows; with windows and no URLs raises the last normal window; with URLs opens a new normal window with those tabs
- [ ] An invocation on a removed profile closes it (no save)
- [ ] Pages from outside (invocations, extensions, sync) never open in private windows
- [ ] Startup windows: restore session (default), or homepage, or new tab page; targets go into the restored window or a new one with no blank tab
- [ ] First run of a new profile opens the welcome over the first normal window, once
- [ ] App actions: new window, new private window, about, welcome, shortcuts help, extension notification activation, quit, update
- [ ] Quit (Ctrl+Q only, no menu item) saves the session first
- [ ] Shutdown: final session save, clear-on-exit site data, final sync round; skipped for a removed profile
- [ ] Restart into an update releases the profile lock (and CEF) before spawning `program --profile-dir root`; AppImage relaunches `$APPIMAGE`
- [ ] Removed current profile: launch the next profile, quit without saving, delete data after release
- [ ] Debug-only tools (screenshot, apply-sync, set-tabs-position, close-dialog) reproduced or consciously dropped
- [ ] Popovers and menus close and clean up when their anchor goes away
- [ ] Animations: tab row 180 ms ease-out-cubic, opacity fades, honour the desktop's animations-off setting
- [ ] Logging through `log`; private addresses kept out (`Browsing::loggable`); decide on a log file; CEF log routed
- [ ] Picker window: "Who's Using Vsesvit?", profile cards (72 px avatar, name, tooltip), Add card, "Show on startup" check, 760×520, header without title
- [ ] Picker launches the chosen or new profile and closes; failure toasts "Could not open the profile: …"; picker never opens a profile itself or claims an instance
- [ ] Avatar: account picture in a circle, else coloured circle with initial; 22 px and 72 px; 9 colours with exact hex values
- [ ] Add and Edit Profile dialog: name entry (max 64, focused), 9 colour swatches (radio), Cancel and accept (disabled while blank), suggested name "Person N" and next colour
- [ ] Add Profile (menu, Manage "+", picker) creates `Profile N` and opens it in its own process and window
- [ ] Manage Profiles dialog: rows with avatar, "This profile" subtitle, Open, Edit, Remove (disabled for the last profile), live refresh
- [ ] Remove confirmation: "Remove {name}?", exact body text, destructive "_Remove", Cancel default
- [ ] Removing another running profile notifies its process, which closes; its data is deleted once it quits
- [ ] Profile list re-read and last-used marked whenever a window comes forward or the profile menu opens
- [ ] Profile button: avatar, tooltip name, menu of profiles (current checked) plus Add Profile… and Manage Profiles…; hidden in private windows and outside the list
- [ ] Choosing another profile launches `--profile-dir <root>` with an activation token; a running profile comes forward
- [ ] Settings > General picker switch "Show Profile Picker at Startup" writes profiles.json; Manage Profiles… button
- [ ] Window titles: "Page", "Page - Profile" (more than one profile), " (Private)" suffix; initial "Vsesvit"
- [ ] Sync sign-in names and pictures the profile unless set by hand (through Syncer / core)
- [ ] Window list in most-recently-focused order drives session order and command-line placement
- [ ] New window default size 1280×820; private windows styled dark
- [ ] Private session starts with the first private tab (ephemeral engine context, own downloads handler, cookie policy) and ends with the last private window (cancel downloads, forget closed tabs, zoom, choices)
- [ ] Two closed-tab stacks (normal and private), 25 each; about:blank and never-committed tabs are not kept; tabs closed with their window are not kept
- [ ] Reopen closed tab restores position (pinned range), pinned state, group, selection, and history if possible
- [ ] Tab search over open tabs of the window's kind plus closed ones, ordered and ranked by core; select-and-raise or reopen
- [ ] MRU clock ticks on tab open, select, window activation and close
- [ ] History visit on every committed main-frame navigation (including same-document), transition from the pending one or Link; none for error pages or private tabs
- [ ] History titles never empty and never the error page's; none for private tabs
- [ ] Site zoom applied on each new document and remembered per site (private in memory); CEF zoom-scale conversion
- [ ] Star / Ctrl+D: bookmark into the bar's end with the "Bookmark added" bubble, or "Edit bookmark" bubble if already bookmarked; error toast
- [ ] Favicons stored only for bookmarked pages and sites (never private); bars refresh when an icon changes
- [ ] Background favicon preload for bookmarked sites in batches of 32, re-running when stale
- [ ] Extension toolbar actions in install order; pin and move stored in `toolbar.extensions`; all windows refresh
- [ ] Omnibox: core suggestions plus open tabs on each edit; async search suggestions (none in private, off by pref) dropped if stale
- [ ] Omnibox Enter: core resolve, navigate Typed, or focus the page; error toast
- [ ] Live prefs: tabs position, bookmarks bar, home button, compact address bar, full URLs apply to every window at once
- [ ] Engine prefs: pop-up blocking (gesture popups allowed), smooth scrolling, hardware acceleration, spell check and languages
- [ ] Tracking protection and third-party cookie choices apply at once
- [ ] Theme: system, light or dark applied to chrome (and web content)
- [ ] Updates: automatic switch starts or stops checks; channel change checks at once; banner and status; first check 30 s after the first window, then daily
- [ ] Homepage pref: about:home, blank or non-URL means the new tab page; never searches
- [ ] Sync refresh matrix (bookmarks, history and sessions views, prefs views, extensions reconcile, permissions, trackers, cookies, shortcuts, theme, engine prefs, window prefs)
- [ ] Session save throttled at 2 s after a change; never written with no normal window or after shutdown
- [ ] Session saved when the last tab of the last normal window closes, and before the last normal window closes
- [ ] Snapshot: normal tabs only, committed-or-loading URL, title, pinned, last active, group, restore state; window size, maximized; device name or host name
- [ ] Restore: active window last (on top, first in order), size, maximized, tabs in background, pinned, ids, last active, selection, then groups
- [ ] Old WebKit restore blobs ignored safely; new restore-state format (if any) distinguishable
- [ ] Memory Saver sweep every 60 s with core's policy; unsaved-input script and edit tracking keep tabs awake; re-check before sleeping; sleeping tab keeps title, icon (faded) and URL in session
- [ ] `chrome.tabs`/`chrome.windows` semantics: committed URL only, private windows hidden unless allowed, groupId, window state and size, origin 0,0
- [ ] Extensions may not set cookies on sites set to Block
- [ ] Extension permission requests prompt over the requesting window with Chrome's wording
- [ ] Extensions loaded at start, reconciled with the synced desired set, update-checked on schedule; load errors shown in the Extensions dialog
- [ ] Keymap: every reassignable command bound to its action with core's default chords; BuiltIn Ctrl+Tab and Ctrl+Shift+Tab fixed
- [ ] Ctrl+S toggles the tab sidebar; Save Page is Ctrl+Shift+S; Ctrl+Shift+C copies a clean link
- [ ] Decide Ctrl+1…9 tab selection (core defaults exist; GTK shell left them to libadwaita)
- [ ] Shortcut edits go through one path: store overrides, re-apply to all windows, report "taken from"
- [ ] Shortcuts suspended while the capture dialog listens; capture key rules (Esc cancel, Backspace clear, Enter confirm, Shift rules, NeedsModifier, Unusable)
- [ ] Extension command shortcuts resolved by core and dispatched (or delegated to Chromium)
- [ ] Browser shortcuts work while web content has focus (`OnPreKeyEvent`)
- [ ] In-app window capture (including web content) for the self-test and debugging
- [ ] Visual values from Appendix A carried into the GPUI theme
