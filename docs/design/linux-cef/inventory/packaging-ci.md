# Inventory: packaging, CI, release and updater integration (for the CEF + GPUI Linux shell)

Scope: `packaging/` (linux, flatpak, appimage, icons, `updater.json`), `xtask/` (especially
`xtask/src/linux`), `.github/workflows/release.yml` (and, for completeness, `sync-server.yml`),
`scripts/` (`wsl.sh`, `release-plan.sh`, `set-version.sh`, plus `sync-e2e.sh` and
`screenshot-window.ps1`), `README.md`, `docs/design/packaging.md`, `docs/PLAN.md`,
`.gitattributes`, the workspace `Cargo.toml`, and the shell code these files depend on (the binary
`crates/vsesvit`, `crates/vsesvit-update`, and the GTK shell's `cli.rs`, `updates/`, `profile.rs`,
`engine.rs`, and the default-browser part of `dialogs/welcome.rs`).

Checkout read: `/home/dy/git/vsesvit-cef` (branch `linux-cef-gpui`, HEAD `27896e0`, clean).

Conventions:

- **Verified** means I read the code, the doc, or the binary. **(inference)** marks anything I did
  not verify.
- Classification of engine- or toolkit-touching pieces:
  - **(KEEP)**: product behavior the new shell reproduces.
  - **(WEBKIT-ONLY)**: a WebKitGTK workaround or limitation that goes away with Chromium.
  - **(NEEDS-CEF)**: needs a CEF equivalent; the likely CEF API is named.
  - **(GTK-ONLY)**: an extra tag used in this file for GTK, libadwaita or GLib pieces (widgets,
    icon themes, GSettings, GIO modules, GApplication). They go away, or GPUI or plain Rust has to
    replace them. It is not an engine question.
- Paths are repo-relative unless absolute.

Research artifacts (third-party, read only, nothing built or run): the `download-cef` 3.0.0 crate
source in `/tmp/vsesvit-research/download-cef/`, the `gpui` 0.2.2 crate in
`/tmp/vsesvit-research/gpui-crate/`, the CEF build index in `/tmp/vsesvit-research/cef-index/`,
and the file listing plus ELF headers of the CEF 154 minimal distribution in
`/tmp/vsesvit-research/cef-minimal-listing/` and `/tmp/vsesvit-research/cef-minimal-elf/`.

---

## 0. Facts about the new stack that packaging depends on (verified unless marked)

### 0.1 CEF as pulled by `cef` 154.5.0+154.0.34

- The `cef` crate (`~/.cargo/registry/src/index.crates.io-*/cef-154.5.0+154.0.34`) is edition
  2021 and dual-licensed Apache-2.0 OR MIT. Its default features are `sandbox`, `build-util` and
  `resources`; the optional ones are `accelerated_osr` (pulls in `wgpu` 30, `ash` 0.38 and `libc`
  on Linux) and `linux-x11`. It also has a bin target, `bundle-cef-app`, which is not built when
  building `-p vsesvit`.
- `cef-dll-sys` 154.5.0+154.0.34 has `links = "cef_dll_wrapper"`. Its build-dependencies are
  `anyhow`, `cc`, `cmake`, `download-cef` 3.0 and `serde_json`, and its bindings come
  pre-generated (`src/bindings/x86_64_unknown_linux_gnu.rs`), so the build needs no libclang.
- The CEF version is the crate version's build metadata, `154.0.34`
  (`download_cef::default_version`). In the CDN index that matches
  `154.0.34+g14c5a08+chromium-154.0.8037.98` (stable channel, built 2026-10-05). The minimal
  archive is `cef_binary_154.0.34+g14c5a08+chromium-154.0.8037.98_linux64_minimal.tar.bz2`,
  326,489,962 bytes.
- **Where the build gets CEF** (`cef-dll-sys/build.rs`):
  - When the env var `FLATPAK` is set to any value, CEF comes from `/usr/lib`. The archive
    version is checked against `archive.json` there.
  - Otherwise, when `CEF_PATH` is set:
    - If `$CEF_PATH/154.0.34` exists, CEF comes from `$CEF_PATH/154.0.34/cef_linux_x86_64`,
      which is downloaded there if missing.
    - If `$CEF_PATH` itself holds a valid `archive.json`, that directory is used as is.
    - Otherwise CEF is downloaded into `$CEF_PATH/154.0.34/`.
  - Otherwise CEF is downloaded into the crate's `OUT_DIR`, again for every profile and after
    every `cargo clean`.
  - The download fetches `https://cef-builds.spotifycdn.com/index.json`, then the **minimal**
    archive (`CEF_DOWNLOAD_URL` overrides the CDN). It checks the SHA-1, extracts the archive,
    and flattens `Release/` and `Resources/` into one `cef_linux_x86_64/` directory together
    with `include/`, `cmake/`, `libcef_dll/`, `CMakeLists.txt` and `CREDITS.html`. Then it
    writes `archive.json` (`{"type":"minimal","name":...,"sha1":...}`).
  - `check_archive_json` accepts an archive whose version is less than or equal to `154.0.34`.
- **On Linux the build runs no cmake and no C++.** The `linux` arm of `build.rs` only copies the
  runtime files and emits `cargo::rustc-link-lib=dylib=cef`. `cmake::Config::build()` is called
  only in the `windows` and `macos` arms. The `cmake` binary the user is installing is therefore
  not needed by `cef-dll-sys` on Linux. `gpui` 0.2.2's `Cargo.lock` has no `cmake` crate either.
- **Runtime files are copied next to the binary at build time.** `copy_cef_runtime_files` copies
  every regular file of the CEF directory, plus `locales/`, into the target profile directory
  (`target/release/` or `target/debug/`). That also brings in `archive.json`, `CMakeLists.txt`
  and `CREDITS.html`. The directory is exported to dependents' build scripts as
  `DEP_CEF_DLL_WRAPPER_CEF_DIR` (`cargo::metadata=CEF_DIR=`), and at run time as
  `cef_dll_sys::get_cef_dir()`.
- **No RUNPATH on the executable.** `grep -rn "rpath\|ORIGIN"` over `cef` and `cef-dll-sys`
  finds nothing. The `vsesvit` executable gets `libcef.so` as `DT_NEEDED`, so it needs a RUNPATH
  of `$ORIGIN` (or an `LD_LIBRARY_PATH` wrapper) before it can start at all, even for
  `--version`. That is an inference from how ld.so works. `libcef.so` itself has
  `RUNPATH [$ORIGIN]` (verified).
- **Files of the minimal distribution** (verified from the archive listing):

  | File | Size (bytes) | Notes |
  |---|---|---|
  | `Release/libcef.so` | 1,455,021,248 | shipped **not stripped**, "with debug_info"; `strip --strip-unneeded` gives 272,219,288 bytes, about 83 MB after `zstd -19` (measured) |
  | `Release/chrome-sandbox` | 31,680 | the setuid sandbox helper; needs only `libc` and `libpthread` |
  | `Release/libvk_swiftshader.so` | 14,907,128 | SwANGLE (software Vulkan for ANGLE) |
  | `Release/libvulkan.so.1` | 1,533,120 | Vulkan loader shipped with CEF |
  | `Release/vk_swiftshader_icd.json` | 107 | `{"ICD":{"library_path":"./libvk_swiftshader.so","api_version":"1.0.5"}}` |
  | `Release/v8_context_snapshot.bin` | 757,576 | required |
  | `Resources/icudtl.dat` | 10,819,840 | required |
  | `Resources/resources.pak` | 23,500,543 | |
  | `Resources/chrome_100_percent.pak` | 582,158 | |
  | `Resources/chrome_200_percent.pak` | 944,863 | |
  | `Resources/locales/*.pak` | 221 files | `<lang>.pak` plus `_FEMININE`, `_MASCULINE` and `_NEUTER` variants for 55 locales |
  | `CREDITS.html` | 8,679,939 | third-party licences; the CEF README points to it or `about:credits` |
  | `LICENSE.txt`, `README.txt` | | not copied by `download-cef` (they are deleted with the extracted directory) |

  There is no `libEGL.so`, `libGLESv2.so` or `snapshot_blob.bin` in 154.
- **The CEF README** (verified): `libcef.so`, `icudtl.dat` and `v8_context_snapshot.bin` are
  required. Locales are chosen from `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` and `LANG` and
  "only configured locales need to be distributed", falling back to `en-US`. `Release/` files go
  "in the same directory as the executable" and `Resources/` files "in the same directory as
  libcef.so". `CefSettings.resources_dir_path` and `locales_dir_path` override this, and
  `pack_loading_disabled` turns pack loading off.
- **`libcef.so` DT_NEEDED** (verified with `readelf -d`): `libdl.so.2`, `libpthread.so.0`,
  `libglib-2.0.so.0`, `libgobject-2.0.so.0`, `libnspr4.so`, `libnss3.so`, `libnssutil3.so`,
  `libsmime3.so`, `libgio-2.0.so.0`, `libatk-1.0.so.0`, `libatk-bridge-2.0.so.0`,
  `libdbus-1.so.3`, `libcups.so.2`, `libX11.so.6`, `libXcomposite.so.1`, `libXdamage.so.1`,
  `libXext.so.6`, `libXfixes.so.3`, `libXrandr.so.2`, `libgbm.so.1`, `libexpat.so.1`,
  `libxcb.so.1`, `libxkbcommon.so.0`, `libcairo.so.2`, `libpango-1.0.so.0`, `libudev.so.1`,
  `libasound.so.2`, `libm.so.6`, `libatspi.so.0`, `libgcc_s.so.1`, `libc.so.6`,
  `ld-linux-x86-64.so.2`.
- **Symbol versions** (verified with `readelf -V`): at most `GLIBC_2.25`, `NSS_3.30`,
  `LIBUDEV_183`, `ALSA_0.9.0` and `GCC_4.2.0`. CEF itself runs on old distributions, so the
  `vsesvit` binary's own glibc floor (set by the CI image) becomes the real minimum.
- **Library names `libcef.so` loads at run time** (verified as strings in the binary; dlopen is
  inferred): `libgtk-3.so.0`, `libgtk-4.so.1`, `libgdk-3.so.0` (Chromium's optional GTK UI layer
  for native dialogs and theming), `libpulse.so.0`, `libpipewire-0.3.so.0`, `libva.so`,
  `libva-drm.so`, `libEGL.so.1`, `libGL.so.1`, `libGLESv2.so.2`, `libvulkan.so.1` plus vendor
  ICD names, `libgssapi_krb5.so.2`, `libspeechd.so.2` and `libpci.so.3`.
- **D-Bus names in `libcef.so`** (verified as strings): `org.freedesktop.portal.FileChooser`,
  `org.freedesktop.portal.Print`, `org.freedesktop.portal.ScreenCast`,
  `org.freedesktop.portal.Notification`, `org.freedesktop.portal.Settings`,
  `org.freedesktop.Notifications`, `org.freedesktop.secrets`, and `org.kde.kwalletd`,
  `org.kde.kwalletd5` and `org.kde.kwalletd6` (Chromium's OSCrypt key storage, inferred).
- **Media codecs** (verified from the symbol table of the unstripped `libcef.so`): the only ffmpeg
  decoders and parsers linked are `ff_flac_decoder`, `ff_flac_parser`, `ff_mp3_decoder`,
  `ff_vorbis_decoder`, `ff_vorbis_parser`, `ff_opus_parser` and `ff_vp9_parser`. There is **no
  `ff_h264_decoder`, `ff_aac_decoder` or HEVC decoder**: the Spotify-CDN builds have no
  proprietary codecs. VP8, VP9, AV1 and Opus go through libvpx, dav1d and libopus inside Chromium
  (inference). The string `WidevineCdm` appears 61 times (whether Widevine can be used is an open
  question).
- **Sandbox strings** (verified): `chrome-sandbox`, `CHROME_DEVEL_SANDBOX`, `--no-sandbox` and
  `/proc/self/ns/user`.
- **`cef_settings_t` fields that matter for packaging** (verified in the generated bindings and
  their doc comments):
  - `no_sandbox`.
  - `browser_subprocess_path`: when empty on Linux, "the main process executable will be used".
  - `cache_path` and `root_cache_path`: when both are empty the data goes to
    `~/.config/cef_user_data`, which is "not recommended in production".
  - `log_file`: **when empty, a `debug.log` is written in the main executable directory on
    Linux**. That directory is `/usr/lib/vsesvit/` or the read-only AppImage mount, so the shell
    has to set `log_file` or `log_severity`.
  - `resources_dir_path`, `locales_dir_path`, `locale` (ignored on Linux).
  - `chrome_policy_id`: "On Linux, this is an absolute directory path like
    `/etc/opt/chrome/policies`. Only supported with Chrome style."
  - `chrome_app_icon_id` (Windows only), `windowless_rendering_enabled`,
    `multi_threaded_message_loop`, `external_message_pump`, `command_line_args_disabled`,
    `user_agent_product`, `accept_language_list`, `disable_signal_handlers`,
    `use_views_default_popup`.
- **Single instance** (verified in the doc of
  `cef_browser_process_handler_t::on_already_running_app_relaunch`): "only a single app instance
  is allowed to run for a given `CefSettings.root_cache_path`". A relaunch hands its command line
  to the running process through `OnAlreadyRunningAppRelaunch`, and "client apps should therefore
  check the `cef_initialize()` return value for early exit". This is the CEF counterpart of
  today's GApplication-per-profile single instance (§7.3).
  `on_before_child_process_launch` exists for editing child command lines.

### 0.2 GPUI 0.2.2 (crates.io)

- `gpui` 0.2.2 was published on 2025-10-22. It is edition 2024 and Apache-2.0. Its default
  features are `font-kit`, `wayland`, `x11` and `windows-manifest`.
- On Linux:
  - Rendering is `blade-graphics` 0.7 (Vulkan through `ash` 0.38, which loads the Vulkan loader
    with `libloading`).
  - Wayland is `wayland-backend` 0.3 with features `client_system` and `dlopen` (so
    `libwayland-client.so.0` is dlopened), plus `wayland-client`, `wayland-cursor` 0.31,
    `wayland-protocols` and `wayland-protocols-plasma`.
  - X11 is `x11rb` 0.13 (`allow-unsafe-code`, `xkb`), `zed-xim` and `x11-clipboard`.
  - Keyboard is `xkbcommon` 0.8 with features `wayland` and `x11`.
  - Fonts are `zed-font-kit` 0.14.1 with `source-fontconfig-dlopen`, plus `freetype-sys` 0.20.1
    (`cc` and `pkg-config`) and `cosmic-text`.
  - Portals go through `ashpd` 0.11, secrets through `oo7` 0.5, and links open with `open` 5.2.
- Backend choice (`src/platform.rs::guess_compositor`): `ZED_HEADLESS` set gives headless. A
  non-empty `WAYLAND_DISPLAY` gives Wayland, else a non-empty `DISPLAY` gives X11, else
  headless. Under `xvfb-run` (no `WAYLAND_DISPLAY`) GPUI therefore uses X11.
- `WindowOptions.app_id` becomes `xdg_toplevel.set_app_id` on Wayland and both halves of
  `WM_CLASS` on X11 (`platform/linux/{wayland,x11}/window.rs::set_app_id`). The Wayland
  activation token is tagged with the app_id as well.
- Renderer env knobs: `ZED_DEVICE_ID`, `ZED_PATH_SAMPLE_COUNT` (a workaround for an amdgpu/radv
  bug), `ZED_FONTS_GAMMA` and `ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST`.
- Runtime needs (inference from the crate design): a Vulkan loader (`libvulkan.so.1`) and a
  Vulkan ICD. That is Mesa (`mesa-vulkan-drivers` or lavapipe for software) or the NVIDIA
  driver; blade has no GL fallback. Also needed: `libxkbcommon.so.0` and
  `libxkbcommon-x11.so.0` (linked by `xkbcommon` with `x11`), `libxcb.so.1`,
  `libwayland-client.so.0` (dlopened), fontconfig (dlopened) and freetype.
- Build needs (inference): `pkg-config`, `libxkbcommon-dev`, `libxkbcommon-x11-dev`,
  `libxcb1-dev` (or `libx11-xcb-dev`), `libwayland-dev`, `libfontconfig-dev`, and optionally
  `libfreetype-dev`. Zed's own Linux build script installs a superset of these.

---

## 1. Product identity constants

**Where:** `xtask/src/ctx.rs`, `crates/vsesvit-gtk/src/lib.rs::APP_ID`, the desktop entry, the
metainfo, the Flatpak manifest and `docs/design/packaging.md` §1.

| Constant | Value | Used by |
|---|---|---|
| `APP_ID` | `dev.mrquantumoff.vsesvit` | desktop file name, `Icon=`, `StartupWMClass=`, AppStream id, Flatpak `app-id`, hicolor icon name, AppImage root icon, GApplication id of the Default profile |
| `PRODUCT` | `Vsesvit` | artifact names, release title `Vsesvit <version>`, `.desktop` `Name=` |
| `BINARY` | `vsesvit` | package name, `/usr/bin/vsesvit`, `/usr/lib/vsesvit/vsesvit`, pacman artifact prefix |
| `SUMMARY` | `A web browser with Chrome Web Store extensions` | deb `Description:`, rpm `Summary:`, pacman `pkgdesc`, NSIS description |
| `HOMEPAGE` | `https://github.com/mrquantumoff/vsesvit` | deb/rpm/pacman url, NSIS |
| `MAINTAINER` | `Demir Yerli <demiryerli@gmail.com>` | deb `Maintainer:`, pacman `packager`, NSIS publisher (name part only) |
| `LICENSE` | `env!("CARGO_PKG_LICENSE")` = `MIT` | rpm `License:`, pacman `license`; tested against the metainfo `<project_license>` |

What the user sees: the launcher entry "Vsesvit" ("Web Browser") with the logo, windows grouped
under that entry, and "Vsesvit" as the application name (`glib::set_application_name("Vsesvit")`
in `app.rs::setup`).

- **(KEEP)** The window must carry the app id `dev.mrquantumoff.vsesvit` so the compositor
  matches it to the desktop file (`StartupWMClass=dev.mrquantumoff.vsesvit`). In GPUI that is
  `WindowOptions { app_id: Some(...) }` (§0.2).
- **(GTK-ONLY)** Today's GApplication id differs per profile:
  `crates/vsesvit-gtk/src/profile.rs::app_id_for_profile` uses the plain id for the standard
  `Default` profile and `dev.mrquantumoff.vsesvit.Profile<first 16 hex of SHA-256(path)>` for
  any other. It is the session-bus name that makes one process per profile. On Wayland GTK4 also
  uses the GApplication id as the toplevel app id (inference, from GTK4's
  `gtkapplication-wayland.c`, not checked here), so windows of non-default profiles may not match
  the desktop file today. The new shell's single-instance mechanism is CEF's `root_cache_path`
  singleton (§0.1), and the window app id is set separately.
- `gtk::Window::set_default_icon_name(APP_ID)` (`app.rs:234`, `profiles.rs:324`) takes the window
  icon from the hicolor theme **(GTK-ONLY)**. On Wayland the compositor takes the icon from the
  desktop file named by the app_id. Whether GPUI 0.2.2 can set `_NET_WM_ICON` on X11 is
  unverified (inference: no API seen).

---

## 2. Desktop entry: `packaging/linux/dev.mrquantumoff.vsesvit.desktop`

Content (verified, LF line endings enforced):

```
[Desktop Entry]
Type=Application
Name=Vsesvit
GenericName=Web Browser
Comment=A web browser with Chrome Web Store extensions
Exec=vsesvit %U
Icon=dev.mrquantumoff.vsesvit
Terminal=false
Categories=Network;WebBrowser;
MimeType=application/pdf;application/rdf+xml;application/rss+xml;application/x-mimearchive;application/xhtml+xml;application/xml;image/avif;image/gif;image/jpeg;image/png;image/svg+xml;image/webp;text/html;text/xml;x-scheme-handler/http;x-scheme-handler/https;
Keywords=browser;web;internet;
StartupNotify=true
StartupWMClass=dev.mrquantumoff.vsesvit
```

1. **User-visible:** the app appears in launchers; it can be chosen as the handler for http/https
   links, HTML, PDF, images and feeds (registered "without making it the default", commit
   `2911080`); `%U` passes URLs/files as arguments; startup notification on.
2. **Core/update APIs:** none. The binary parses `%U` targets (`vsesvit_gtk::cli::parse` →
   `Command::Browse { targets }`).
3. **State:** installed to `/usr/share/applications/` (deb/rpm/pacman), `/app/share/applications/`
   (Flatpak), and the AppImage root plus `usr/share/applications/`.
4. **Classification:** (KEEP) entirely. `application/pdf` stays valid if CEF's PDF viewer is
   present in the build (CEF ships Chromium's PDF viewer by default; inference, not checked in
   `resources.pak`). `StartupNotify=true` works on Wayland only if the shell consumes
   `XDG_ACTIVATION_TOKEN`/`DESKTOP_STARTUP_ID` (GTK did this for free) **(GTK-ONLY → GPUI)**.
5. **Interactions:**
   - `stage.rs::validate` runs `desktop-file-validate` when available; `verify.sh` validates the
     installed copy in each distro container.
   - The Welcome dialog's Default Browser page (`crates/vsesvit-gtk/src/dialogs/welcome.rs`
     `DefaultBrowser`, ll. 415-460, 528-540) looks up `dev.mrquantumoff.vsesvit.desktop` with
     `gio::AppInfo::default_for_type("x-scheme-handler/https")` and
     `gio::AppInfo::all_for_type`, and sets it with
     `app.set_as_default_for_type(kind)` for `x-scheme-handler/http`,
     `x-scheme-handler/https` and `text/html` **(GTK-ONLY: GIO)**. User-visible wording:
     - "Vsesvit is your default browser."
     - "Links from other apps open in {name}."
     - "No default browser is set."
     - In a Flatpak (`/.flatpak-info` exists): "Flatpak apps cannot change the default browser
       themselves. Choose Vsesvit as the web browser under Default Apps in your system settings."
     - With no desktop entry installed: "This copy of Vsesvit has no desktop entry installed, so
       the system cannot open links with it."

     The new shell needs a non-GIO way to read and write `mimeapps.list` defaults (e.g.
     `xdg-settings`/`xdg-mime`; inference). The self-test `welcome` check records this status.
   - The Windows installer registers the same associations (`installer.nsi` `URLAssociations`);
     unchanged.

---

## 3. AppStream metainfo: `packaging/linux/dev.mrquantumoff.vsesvit.metainfo.xml`

1. **User-visible:** software centers show name "Vsesvit", summary "Browse the web with Chrome Web
   Store extensions", developer "Demir Yerli" (`id="dev.mrquantumoff"`), homepage and bug tracker
   URLs, `<content_rating type="oars-1.1"/>`, a single `<release version="@VERSION@"
   date="@DATE@"/>`, the `vsesvit` binary and the same 16 media types as the desktop entry.
   Description: "Vsesvit is a web browser built on WebKitGTK. It installs extensions from the
   Chrome Web Store and keeps bookmarks, history, open tabs and settings in a sync-ready profile."
2. **APIs:** none.
3. **State:** template; `@VERSION@`/`@DATE@` substituted by `xtask/src/linux/stage.rs::metainfo`
   (UTC date `[year]-[month]-[day]`) and by a `sed` in the Flatpak manifest; installed to
   `usr/share/metainfo/`.
4. **Classification:** the description's "built on WebKitGTK" must change (docs); everything else
   (KEEP). `<project_license>MIT</project_license>` is asserted equal to the workspace licence by
   `xtask/src/ctx.rs` test `package_license_matches_workspace` (also asserts no
   `LicenseRef-proprietary`). Bundling CEF adds BSD-3-Clause plus Chromium's third-party licences
   (`CREDITS.html`); whether `project_license` or package licence fields should list them is an
   open question (inference: distributions expect the bundled licences to be shipped).
5. **Interactions:** validated by `appstreamcli validate --no-net` in `stage.rs` when present.

---

## 4. Icons: `packaging/icons/` and `xtask icons`

- Files: `dev.mrquantumoff.vsesvit.svg` (2160×2160, the app icon; byte-identical to
  `WhiteBG.svg`, sha256 `c90bbf6d…`), `DarkBG.svg`, `NoBG.svg`, `WhiteBG.svg` (logo variants kept
  as sources, commit `d85c333`), `dev.mrquantumoff.vsesvit.ico` (committed, rendered).
- `xtask/src/icons.rs`: `svg()`, `png(size)` (resvg 0.48 render scaled to the square), `ico()`
  (sizes 16, 24, 32, 48, 64, 256), `write_all(out)` (writes `<APP_ID>.svg`, `<APP_ID>.ico`,
  `hicolor/`), `write_hicolor(dir)` (`<n>x<n>/apps/<APP_ID>.png` for 16, 24, 32, 48, 64, 128,
  256, 512, plus `scalable/apps/<APP_ID>.svg`). Test
  `the_hicolor_tree_holds_every_size_and_nothing_else`.
- Uses: `stage.rs` installs hicolor into `usr/share/icons/hicolor`; the AppImage root gets
  `<APP_ID>.png` (256 px) and `.DirIcon` → it; the Flatpak manifest runs
  `cargo --offline run --release -p xtask -- icons icons` and copies `icons/hicolor`;
  `crates/vsesvit/build.rs` embeds the `.ico` into `vsesvit.exe` (Windows only); README shows
  `DarkBG.svg`/`WhiteBG.svg` via `<picture>`.
- **Classification:** (KEEP) all. The AppImage additionally bundles the Adwaita icon theme and
  `hicolor/index.theme` because GTK widgets use `*-symbolic` icon names **(GTK-ONLY)**; GPUI
  draws its own embedded SVG assets, so the theme bundling goes (the hicolor app icon stays for
  the desktop entry).

---

## 5. Installation layout and the `package-format` marker

**Contract** (`docs/design/packaging.md` "The installation marker",
`crates/vsesvit-update/src/installation.rs`, `xtask/src/linux/stage.rs`):

```
usr/lib/vsesvit/vsesvit          real executable, stripped (strip --strip-unneeded), mode 0755
usr/lib/vsesvit/package-format   one line: deb | rpm | pacman | appimage | flatpak  (nsis on Windows)
usr/bin/vsesvit -> ../lib/vsesvit/vsesvit
usr/share/applications/dev.mrquantumoff.vsesvit.desktop   (0644)
usr/share/metainfo/dev.mrquantumoff.vsesvit.metainfo.xml
usr/share/icons/hicolor/{16..512}x{..}/apps/dev.mrquantumoff.vsesvit.png, scalable/apps/….svg
```

Flatpak: `/app/lib/vsesvit/vsesvit`, `/app/lib/vsesvit/package-format` (`flatpak`),
`/app/bin/vsesvit -> ../lib/vsesvit/vsesvit`.

- `vsesvit_update::Installation::detect()` canonicalizes `std::env::current_exe()` (so the
  `/usr/bin` symlink resolves), reads `package-format` beside it, and returns `Unpackaged`, `Nsis`,
  `Deb`, `Rpm`, `Pacman`, `AppImage { image: $APPIMAGE }` (a marker `appimage` without a non-empty
  `$APPIMAGE` is `Unpackaged`), or `Flatpak`; an unknown marker logs a warning and is
  `Unpackaged`. `variant()` is the protocol's `{{bundle_type}}`; `self_updates()` is false for
  Unpackaged and Flatpak; `format()` maps to `vsesvit_update::Format`.
- **(KEEP)** The marker and the `lib/vsesvit` + `bin` symlink layout. **(NEEDS-CEF)** every CEF
  runtime file must sit in `usr/lib/vsesvit/` next to the executable (or the shell sets
  `resources_dir_path`/`locales_dir_path` and `browser_subprocess_path`): `libcef.so` (stripped),
  `chrome-sandbox`, `libvk_swiftshader.so`, `libvulkan.so.1`, `vk_swiftshader_icd.json`,
  `v8_context_snapshot.bin`, `icudtl.dat`, `resources.pak`, `chrome_100_percent.pak`,
  `chrome_200_percent.pak`, `locales/*.pak`. `CREDITS.html` should ship somewhere readable
  (inference: `/usr/share/doc/vsesvit/` or beside the binary). None of these collide with
  `package-format`. The executable needs `RUNPATH=$ORIGIN` (§0.1); `$ORIGIN` of the main
  program resolves through the `/usr/bin` symlink because glibc derives it from `/proc/self/exe`
  (inference).
- **(NEEDS-CEF)** CEF child processes (zygote, renderer, GPU, utility) re-execute the same binary
  (`browser_subprocess_path` empty → main executable). `stage.rs` must therefore never install a
  different `vsesvit` wrapper as the real executable, except where a sandbox wrapper is
  intended (Flatpak zypak, §9).

---

## 6. Binary entry point and command line

**Where:** `crates/vsesvit/src/main.rs` (calls `vsesvit_gtk::run()` on Linux,
`vsesvit_winui::run()` on Windows, else prints "vsesvit supports Windows and Linux only"),
`crates/vsesvit/Cargo.toml` (`[target.'cfg(target_os = "linux")'.dependencies] vsesvit-gtk`),
`crates/vsesvit-gtk/src/lib.rs::run`, `crates/vsesvit-gtk/src/cli.rs`.

Usage text (verbatim, printed by `--help` and after errors):

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

Parsing rules (`cli::parse`, verified with its tests):

- Non-option arguments (anything not starting with `-` or exactly `-`) are targets; `--` ends
  options.
- `--profile-dir DIR` / `--profile-dir=DIR`; empty value → `MissingValue`; repeated → `Repeated`.
- `--self-test OUT_DIR` / `=OUT_DIR`; `--network` only with `--self-test`
  (`NetworkWithoutSelfTest`).
- `--self-test`, `--check-for-updates`, `--update` are standalone: each may appear once and not
  with targets, `--profile-dir` or each other (`Exclusive`).
- `-h`/`--help` and `--version` win immediately, even after other arguments.
- Unknown options → `UnknownOption`. **Any parse error prints `vsesvit: <error>\n\n<USAGE>` to
  stderr and exits 2.**
- `--version` prints `vsesvit <CARGO_PKG_VERSION>` and exits 0 (verify.sh relies on this).
- The process logs through `glib::GlibLogger` at Debug level **(GTK-ONLY)**.

Classification and changes:

- (KEEP) the usage, the semantics and exit codes, and `--version` output.
- **(NEEDS-CEF)** CEF re-executes the binary with Chromium switches (`--type=zygote`,
  `--type=renderer`, `--type=gpu-process`, `--type=utility`, plus many others). With today's
  parser these would be `UnknownOption` → exit 2. `cef::execute_process` must run before
  `cli::parse` and return for child processes. Relaunch forwarding through
  `OnAlreadyRunningAppRelaunch` also receives Chromium-normalized command lines.
- **(NEEDS-CEF)** `--version`, `--help`, `--check-for-updates` and `--update` open no window and
  no profile today. With `libcef.so` as `DT_NEEDED` they still need every CEF library loadable
  (nss, atk, cups, X11 libs…). That is acceptable for packages, whose Depends pull them in, but
  it means a headless server running `vsesvit --check-for-updates` needs them too (inference
  from the link model in §0.1).
- `--self-test` exists only with the `self-test` cargo feature, which is a **default** feature of
  `vsesvit-gtk` (`self-test = ["vsesvit-core/testkit"]`). Release packages contain it because
  verify.sh runs it from the AppImage (KEEP). Without the feature the binary prints
  "vsesvit: this build was made without the `self-test` feature" and exits 1.

---

## 7. Self-update in the shell (vsesvit-update integration)

### 7.1 Configuration: `packaging/updater.json`

```json
{
  "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDQ0REQ1Q0VEQzk0RkQ2NjEK…",
  "endpoints": ["https://api.usequadrant.dev/api/any/vsesvit/updates/{{channel}}/{{target}}/{{arch}}/{{current_version}}?variant={{bundle_type}}"],
  "windows": { "installMode": "passive" }
}
```

- Compiled in by `vsesvit_update::config::Config::builtin()` (`include_str!` of the file). The
  build-time env vars `VSESVIT_UPDATER_PUBKEY` and `VSESVIT_UPDATER_ENDPOINTS` (comma-separated)
  override it (`crates/vsesvit-update/build.rs` reruns when they change). Endpoints must be https
  unless every override endpoint is loopback; `dangerousInsecureTransportProtocol` is honoured
  for the file. An empty pubkey disables the updater
  (`DisabledReason::NoPublicKey`).
- Also read by `xtask manifest` (to verify signatures before publishing) and by
  `scripts/release-plan.sh` (to refuse a release without a pubkey).
- (KEEP) unchanged; engine-independent.

### 7.2 Protocol and install (crate `vsesvit-update`, unchanged by the rewrite)

The public API the new shell calls (verified, `crates/vsesvit-update/src/lib.rs`):

- `Installation::detect()`, `Installation::{variant, self_updates, format}`.
- `Config::builtin()`.
- `Updater::new(config, current_version: semver::Version, installation)`; it fails with
  `Error::Disabled(..)` when there is no key or no endpoints.
- `Updater::check(channel: &str) -> Result<Option<Available>, Error>`.
- `Available::{release, into_update}`; `Available::NotInstallable(Release)` for Unpackaged and
  Flatpak.
- `Update::download(dir, progress) -> Result<Downloaded, Error>`.
- `Downloaded::install(&installation, relaunch_args) -> Result<Installed, InstallFailed>`.
  `InstallFailed { error, downloaded }` hands the download back for a retry.
- `Downloaded::install_on_exit` (Windows only).
- `remove_stale_downloads(dir, keep)`.
- `Installed::{ExitNow, Relaunch, NextLaunch}`.
- `cli::{check, update, print}`.
- `TARGET` (`"linux"`) and `ARCH` (`"x86_64"`).

How it behaves (verified; detail in `docs/design/packaging.md` "The update protocol"):

- **Check.** It tries the endpoints in order, after substituting `{{channel}}`,
  `{{current_version}}` (`+` → `%2B`), `{{target}}`, `{{arch}}` and `{{bundle_type}}` (`unknown`
  when unpackaged), in both the literal and the percent-encoded spelling. Each request is a GET
  with `Accept: application/json` and `User-Agent: vsesvit/<version>`.
  - `204` or `404` means no update.
  - Another status or a network error moves on to the next endpoint.
  - A response may use the dynamic or the static (`platforms`) format. A static manifest is
    looked up under the key `<target>-<arch>-<variant>`, then `<target>-<arch>`.
  - An update is offered only when its version is greater than the running one and its channel
    (the first prerelease field, or `stable`) is at least as steady as the one asked for, in the
    order stable < beta < weekly < nightly.
- **Download.** Timeouts: connect 30 s, response 60 s, body 30 min.
  - Downloads are capped at 512 MiB (`TooLarge`).
  - They resume with a `Range` header into a locked `<name>.<pid>-<n>.part` file.
  - Each download is verified twice: minisign with prehashing allowed, the trusted comment's
    `version:` must equal the announced version, and the magic bytes must match.
  - Names: `vsesvit-<version>{-setup.exe|.deb|.rpm|.pkg.tar.zst|.AppImage}`.
- **Install.**
  - deb runs `pkexec apt-get install -y --no-install-recommends <absolute path>`.
  - rpm runs `pkexec rpm -U <file>`.
  - pacman runs `pkexec pacman -U --noconfirm <file>`.
  - pkexec exit code 126 or 127 means "administrator authorization was not given". Success
    deletes the file and returns `Relaunch`.
  - AppImage writes `.<name>.vsesvit-update` next to `$APPIMAGE`, copies its mode, fsyncs and
    renames it over, then fsyncs the directory. It returns `NextLaunch`.
  - NSIS (Windows) uses `ShellExecuteW` with `/P /UPDATE /R /ARGS …` and returns `ExitNow`.
- The magic-byte checks:

  | Format | The file starts with |
  |---|---|
  | deb | `!<arch>\ndebian-binary` |
  | rpm | `ED AB EE DB` |
  | pacman | zstd `28 B5 2F FD` |
  | AppImage | ELF with `AI\x02` at offset 8 |
  | nsis | `MZ` |

- **(KEEP)** All of it.
- **(NEEDS-CEF), indirectly.** Installing a deb, rpm or pacman package replaces the CEF files
  under `/usr/lib/vsesvit/` while the old browser is still running. Chromium starts new renderer
  and utility processes later. Today the zygote forks them, but other children are re-executed
  from `/proc/self/exe` or from the new files. A package swapped mid-session can therefore make
  child processes fail until restart (inference; Chrome's own deb handles this with a "relaunch
  to update" notice). The current UX already asks for a restart after a package install
  ("Restart to finish updating"). The AppImage case is safe, since the old mount stays.

### 7.3 Shell lifecycle (`crates/vsesvit-gtk/src/updates/{mod.rs,lifecycle.rs,headless.rs}`)

The GTK shell holds this as code the rewrite must port, because the crate has no lifecycle of its
own.

- **Creation.** `Updates::new(app, automatic, channel)` returns `None`, with one info log, when
  `Installation::detect()` does not self-update, the config is unusable, or the program path is
  unknown. Nothing else about updates then appears in the UI.
  - "Program" is the path to relaunch: `$APPIMAGE` for an AppImage, else `current_exe()`. It is
    resolved at startup, because once a package manager replaces the executable,
    `current_exe()` names the deleted file.
  - The download dir is `glib::user_cache_dir()/vsesvit/updates`, which is
    `${XDG_CACHE_HOME:-~/.cache}/vsesvit/updates`. It is (KEEP) as a path, but GLib goes
    **(GTK-ONLY)**.
  - The current version comes from `CARGO_PKG_VERSION` as semver.
- **Timer.** The first check runs 30 s after the first browser window opens
  (`FIRST_CHECK_DELAY_SECS = 30`), then every 24 h (`CHECK_INTERVAL_SECS`), only while
  `updates.automatic` is true (`crate::sync::Repeating`). Turning automatic off cancels the timer;
  turning it on re-arms it if a window has been seen.
- **Worker.** Each check, download or install runs on a thread named `vsesvit-update`, and its
  events come back to the UI thread through a `futures_channel::mpsc` drained by
  `glib::spawn_future_local` **(GTK-ONLY: GLib main context; GPUI has its own executor)**. One
  worker runs at a time.
- **State machine** (`Lifecycle<D>`; verified with its tests):
  - States: `Idle`, `Checking`, `UpToDate`, `Downloading{version}`, `Rechecking`,
    `Ready{version, downloaded}`, `Installing{version}`, `Installed{version, next}`,
    `Failed(String)`, `Disabled(reason)`.
  - Events: `Check`, `ChannelChanged`, `Activate`, `UpToDate`, `Found`, `Downloaded`,
    `Installed`, `InstallFailed`, `Failed`.
  - Effects: `Check`, `Install`, `Restart`, `Exit`, `Notify`.
  - A `Failed(Disabled)` event means `Disabled`, and the timer stops.
  - `Check` is accepted only in `Idle`, `UpToDate` or `Failed` ("one check at a time", and no
    checks while an update waits).
  - A `ChannelChanged` event while idle, up to date, failed or ready starts a check at once (it
    drops a waiting download). While a check or download runs it goes to `Rechecking`, and
    whatever ends the old work starts the new check.
  - On `Downloaded`: an AppImage (`background_install`) installs at once; the other formats go
    to `Ready` and wait for `Activate`.
  - `Installed(ExitNow)` → `Exit`.
  - `InstallFailed` in the foreground goes back to `Ready` with `Notify(error)`. In the
    background it goes to `Idle`, and the next check finds the verified file again.
  - `Activate` in `Installed{Relaunch|NextLaunch}` → `Restart`.
- **Wording** (`Lifecycle::status`, exact strings):

  | State | Status title | Button |
  |---|---|---|
  | Idle | `Vsesvit {current}` | Check for Updates |
  | Checking / Rechecking | `Checking for updates…` | Check (disabled) |
  | UpToDate | `Vsesvit is up to date` | Check |
  | Downloading | `Downloading Vsesvit {v}…` | Check (disabled) |
  | Ready | `Vsesvit {v} is ready to install` | **Update** |
  | Installing | `Installing Vsesvit {v}…` | Check (disabled) |
  | Installed NextLaunch | `Vsesvit {v} will open next time` | **Restart now** |
  | Installed Relaunch | `Restart to finish updating` | **Restart** |
  | Installed ExitNow | `Vsesvit {v} is installed` | Check |
  | Failed | `Could not check for updates: {message}` | Check |
  | Disabled | `Updates are off: {reason}` | Check |

- **Banner.**
  - Every browser window has an `adw::Banner` with `action_name("app.update")`. It is the first
    top bar of the toolbar view, above the tab bar, the bookmarks bar and the find bar
    (`window/mod.rs` ll. 356, 394, `set_update_banner`).
  - It is revealed in `Ready`, in foreground `Installing`, and in `Installed` other than
    `ExitNow`.
  - Its title is the status title and its button label is the status button.
  - The application action `app.update` sends `Activate`.
  - **(GTK-ONLY → GPUI)**: the bar's place, wording and behavior are (KEEP).
- **Toast.** On `Notify(message)` the message, with its first letter uppercased, shows as a toast
  in the active browser window (else the first window). An example is "Installing the update
  failed: administrator authorization was not given".
- **Settings > General > "Updates" group** (`dialogs/settings.rs` ll. 478-498, 597-637) appears
  only when `browser.updates()` is `Some`. It has three rows:
  1. A status row. Its title is the status title and its subtitle is `Version {CARGO_PKG_VERSION}`.
     Its suffix shows either the "Check for Updates" button (sensitive per `can_check`) or, when
     the banner offers one, the banner's button bound to `app.update`. The row is kept live
     through `Updates::watch_status`.
  2. "Update Channel", with the subtitle "Vsesvit moves to a steadier channel once that channel
     has a version newer than this one." and the choices Stable, Beta, Weekly and Nightly. It
     goes through `Browser::set_updates_channel` → `updates.set_channel`, which checks at once.
  3. "Automatic Updates", with the subtitle "Download new versions of Vsesvit in the
     background". It goes through `Browser::set_updates_automatic`.
- **Restart.**
  - `Effect::Restart` sets a flag and quits the app.
  - After `app.run` returns, `app.rs` calls `release_profile`. It drops the browser, force-closes
    visible dialogs, destroys the windows, iterates the main context for up to 2 s, and warns
    "the profile is still open; the restarted browser may find it locked".
  - It then spawns `program --profile-dir <canonical root>` (`ProfileLocation::relaunch_args`).
  - **(NEEDS-CEF)** CEF's process singleton on `root_cache_path` must be released
    (`cef::shutdown()` after every browser has closed) before the new process starts. Otherwise
    the new process hands its command line to the dying one and exits (§0.1).
- **Headless commands** (`updates/headless.rs`). `--check-for-updates` and `--update` call
  `vsesvit_update::cli::{check, update}` with `Installation::detect()`, the current version and
  `vsesvit_core::prefs::UpdateChannel::of_build().name()`. They do not use the pref, because no
  profile is opened. They print one JSON line:
  `{"installation","current","available":{version,notes,pub_date}|null[,"installed":{version,next:"exit_now"|"relaunch"|"next_launch"}|null]}`.
  On error they print `{"error": "..."}` and exit 1. Progress for `--update` goes to stderr as
  `vsesvit: downloading <v>: N% of T bytes`. (KEEP)

Core API used: `vsesvit_core::prefs::UpdateChannel` (`ALL`, `name()`, `of_version()`,
`of_build()`). Prefs: `vsesvit_core::prefs::keys::UPDATES_AUTOMATIC` (`updates.automatic`,
`Scope::Local`, default `true`) and `keys::UPDATES_CHANNEL` (`updates.channel`, `Scope::Local`,
default `UpdateChannel::of_build`), both in the profile database.

Persistent files:

- `~/.cache/vsesvit/updates/vsesvit-<ver><suffix>` and `….<pid>-<n>.part`.
- `.<AppImage name>.vsesvit-update`, a transient file beside `$APPIMAGE`.
- On Linux the Windows-only `install_on_exit` is not used.

---

## 8. Linux packages: deb, rpm, pacman (`xtask/src/linux/{mod,stage,deps,deb,rpm,pacman,util}.rs`)

### 8.1 Build driver

- `cargo xtask package <deb|rpm|pacman|appimage|flatpak>... [--sign]`
  (`.cargo/config.toml` alias: `xtask = "run --quiet --package xtask --"`).
  - Off Linux it fails with "… from Windows run: bash scripts/wsl.sh xtask package <fmt>".
  - `linux::build_release` runs
    `cargo build --release --locked -p vsesvit --message-format=json-render-diagnostics` and
    takes the first `compiler-artifact` with an `executable` field.
- `stage::stage(ctx, format, binary)` builds `target/package/<variant>/root/usr/…` fresh each
  time (§5). It copies the binary, runs `strip --strip-unneeded` and sets mode 0755, writes
  `package-format` (`<variant>\n`), makes the symlink, copies the desktop file (0644), renders
  the metainfo, writes the hicolor icons, then runs `desktop-file-validate` and
  `appstreamcli validate --no-net` when available. A missing validator is reported, not fatal.
- Artifacts go to `target/dist/`, with names following the Tauri bundler:

  | Format | File | `latest.json` keys |
  |---|---|---|
  | deb | `Vsesvit_<v>_amd64.deb` | `linux-x86_64-deb` |
  | rpm | `Vsesvit-<v>-1.x86_64.rpm` | `linux-x86_64-rpm` |
  | pacman | `vsesvit-<v>-1-x86_64.pkg.tar.zst` | `linux-x86_64-pacman` |
  | AppImage | `Vsesvit_<v>_amd64.AppImage` | `linux-x86_64-appimage`, `linux-x86_64` |
  | Flatpak | `Vsesvit_<v>_x86_64.flatpak` | none (`flatpak_is_never_published` test) |

- `Format::package_version` keeps a prerelease sorting below its release: deb and rpm replace the
  first `-` with `~` (`0.1.1~nightly.20260927.5`), pacman removes it (`0.1.1nightly.…`), and
  AppImage and Flatpak keep the semver. File names always keep the semver.

### 8.2 Dependency table (`deps.rs::RUNTIME`, mirrored in packaging.md "Runtime dependencies")

| deb | rpm | pacman | kind | classification |
|---|---|---|---|---|
| `libgtk-4-1` | `gtk4` | `gtk4` | required | (GTK-ONLY): remove |
| `libadwaita-1-0` | `libadwaita` | `libadwaita` | required | (GTK-ONLY): remove |
| `libwebkitgtk-6.0-4` | `webkitgtk6.0` | `webkitgtk-6.0` | required | (WEBKIT-ONLY): remove |
| `gstreamer1.0-plugins-good` | `gstreamer1-plugins-good` | `gst-plugins-good` | required | (WEBKIT-ONLY): remove; Chromium decodes media itself |
| `gstreamer1.0-plugins-bad` | `gstreamer1-plugins-bad-free` | `gst-plugins-bad` | required | (WEBKIT-ONLY): remove |
| `gstreamer1.0-libav` | `gstreamer1-plugin-libav` | `gst-libav` | required | (WEBKIT-ONLY): remove. **H.264/AAC playback was provided by this; CEF minimal builds have no H.264/AAC decoder (§0.1), and no package dependency can add it back.** |
| `pkexec` | `polkit` | `polkit` | optional (Recommends / optdepends "install updates from inside the browser") | (KEEP) |

`deps::required(field)` and `deps::optional(field)` feed every format, and the AppImage sysroot
seeds from `required(deb)`. `OPTIONAL_REASON = "install updates from inside the browser"`.

**What replaces it** (the sonames are verified in §0.1; the package names are an inference to
check on each distribution):

| Need (soname) | deb (Ubuntu 26.04 / Debian) | rpm (Fedora) | pacman (Arch) |
|---|---|---|---|
| `libnss3.so`, `libnssutil3.so`, `libsmime3.so` (NSS ≥ 3.30) | `libnss3` | `nss` | `nss` |
| `libnspr4.so` | `libnspr4` | `nspr` | `nspr` |
| `libglib-2.0`, `libgobject-2.0`, `libgio-2.0` | `libglib2.0-0t64` (Provides `libglib2.0-0`) | `glib2` | `glib2` |
| `libatk-1.0`, `libatk-bridge-2.0`, `libatspi` | `libatk1.0-0t64`, `libatk-bridge2.0-0t64`, `libatspi2.0-0t64` | `atk`, `at-spi2-atk`, `at-spi2-core` | `at-spi2-core` |
| `libdbus-1.so.3` | `libdbus-1-3` | `dbus-libs` | `dbus` |
| `libcups.so.2` | `libcups2t64` | `cups-libs` | `libcups` |
| `libX11`, `libXcomposite`, `libXdamage`, `libXext`, `libXfixes`, `libXrandr`, `libxcb` | `libx11-6`, `libxcomposite1`, `libxdamage1`, `libxext6`, `libxfixes3`, `libxrandr2`, `libxcb1` | `libX11`, `libXcomposite`, `libXdamage`, `libXext`, `libXfixes`, `libXrandr`, `libxcb` | `libx11`, `libxcomposite`, `libxdamage`, `libxext`, `libxfixes`, `libxrandr`, `libxcb` |
| `libgbm.so.1` | `libgbm1` | `mesa-libgbm` | `mesa` (or a split `libgbm`) |
| `libexpat.so.1` | `libexpat1` | `expat` | `expat` |
| `libxkbcommon.so.0` (+ `-x11` for GPUI) | `libxkbcommon0`, `libxkbcommon-x11-0` | `libxkbcommon`, `libxkbcommon-x11` | `libxkbcommon`, `libxkbcommon-x11` |
| `libcairo.so.2`, `libpango-1.0.so.0` | `libcairo2`, `libpango-1.0-0` | `cairo`, `pango` | `cairo`, `pango` |
| `libudev.so.1` | `libudev1` | `systemd-libs` | `systemd-libs` |
| `libasound.so.2` | `libasound2t64` | `alsa-lib` | `alsa-lib` |
| Vulkan loader for GPUI (CEF ships its own copy) | `libvulkan1` | `vulkan-loader` | `vulkan-icd-loader` |
| Vulkan driver for GPUI | `mesa-vulkan-drivers \| vulkan-icd` | `mesa-vulkan-drivers` (Recommends) | `vulkan-driver` (virtual) |
| Wayland client (dlopened by GPUI) | `libwayland-client0` | `libwayland-client` | `wayland` |
| fontconfig, freetype (GPUI) | `libfontconfig1`, `libfreetype6` | `fontconfig`, `freetype` | `fontconfig`, `freetype2` |
| optional, dlopened by CEF: GTK 3 or 4 for native file and print dialogs | Recommends `libgtk-3-0t64 \| libgtk-4-1` | Recommends `gtk3` | optdepends `gtk3` |
| optional, dlopened by CEF: PulseAudio/PipeWire, VA-API, Kerberos | Recommends `libpulse0`, `libva2`, `libgssapi-krb5-2` | weak deps | optdepends |
| updates | Recommends `pkexec` | Recommends `polkit` | optdepends `polkit` |

### 8.3 deb (`deb.rs`)

The control file is written to `root/DEBIAN/control`:

- `Package: vsesvit`
- `Version: <package_version>`
- `Architecture: amd64`
- `Maintainer:`
- `Installed-Size: <KiB of regular files>`
- `Depends: <required, joined with ", ">`
- `Recommends: <optional>`
- `Section: web`
- `Priority: optional`
- `Homepage:`
- `Description:` the SUMMARY, followed by the 3-line long description "Vsesvit is a web browser
  built on WebKitGTK. It installs extensions from the Chrome Web Store and keeps bookmarks,
  history, open tabs and settings in a sync-ready profile."

It is built with `dpkg-deb --root-owner-group --build <root> <artifact>`. There is no
dpkg-shlibdeps and there are no maintainer scripts.

Changes:

- The description's "built on WebKitGTK" goes.
- The Depends list changes (§8.2).
- **(NEEDS-CEF) sandbox.** Chromium needs unprivileged user namespaces or a setuid-root
  `chrome-sandbox` (mode 4755, owned by root). `--root-owner-group` makes files root-owned and
  dpkg keeps mode bits, so staging `chrome-sandbox` as 4755 is enough (inference). On Ubuntu
  23.10 and later, `kernel.apparmor_restrict_unprivileged_userns=1` keeps the namespace sandbox
  from working for a binary that has no AppArmor profile allowing `userns`. A deb would need
  either the setuid helper or an `/etc/apparmor.d/` profile for `/usr/lib/vsesvit/vsesvit`, the
  way Chrome and Brave do it (inference). Either is a new kind of file in the package (a
  conffile or a postinst `apparmor_parser -r`).
- `libcef.so` must be stripped in `stage.rs`: about 1.45 GB unstripped against 272 MB stripped
  (measured). `Installed-Size` grows to about 330 MB (inference from the file sizes).

### 8.4 rpm (`rpm.rs`)

- The spec has `%global debug_package %{nil}`, `%global _build_id_links none`,
  `%global __os_install_post %{nil}` (no brp-strip and no other post-install scripts),
  `Name: vsesvit`, `Release: 1`, `Summary:`, `License: MIT`, `URL:`, **`AutoReqProv: no`**, one
  `Requires:` per required dependency, one `Recommends:` per optional one, the same
  `%description`, and `%install`, which runs `cp -a '<stage root>/.' %{buildroot}/`.
- `%files`:
  - `/usr/bin/vsesvit`
  - `/usr/lib/vsesvit/`
  - `/usr/share/applications/<APP_ID>.desktop`
  - `/usr/share/metainfo/<APP_ID>.metainfo.xml`
  - `/usr/share/icons/hicolor/*/apps/<APP_ID>.*`
- It is built with `rpmbuild -bb --target x86_64 --define "_rpmformat 4"` (rpm 6 writes v6
  packages by default, which rpm 4 hosts cannot read), plus `--define _topdir`,
  `--define _dbpath`, then copied from `RPMS/x86_64/`.

Changes:

- Because of `AutoReqProv: no`, rpm computes no dependencies for `libcef.so`. Every library in
  §8.2 must be listed explicitly. It also keeps the package from "providing" `libcef.so` or
  `libvulkan.so.1` (good, inference).
- `/usr/lib/vsesvit/` already covers the CEF files.
- A setuid `chrome-sandbox` needs `%attr(4755, root, root) /usr/lib/vsesvit/chrome-sandbox`
  (NEEDS-CEF).
- The description text changes.

### 8.5 pacman (`pacman.rs`)

- `.PKGINFO` has:
  - `# Generated by cargo xtask package pacman`
  - `pkgname`, `pkgbase`, `xdata = pkgtype=pkg`
  - `pkgver = <v>-1`
  - `pkgdesc`, `url`
  - `builddate` (`SOURCE_DATE_EPOCH` or now)
  - `packager`
  - `size`
  - `arch = x86_64`
  - `license`
  - one `depend = …` per required dependency and one `optdepend = <dep>: install updates from
    inside the browser` per optional one
- `.MTREE` is made with
  `fakeroot bsdtar -cnf - --format=mtree --options !all,use-set,type,uid,gid,mode,time,size,md5,sha256,link`
  and gzipped. The archive is made with `fakeroot bsdtar --no-fflags` piped into
  `zstd -c -T0 --ultra -20 -`, with `.PKGINFO` and `.MTREE` first.
- Changes: the dependency list (§8.2), and a setuid mode on `chrome-sandbox`, which fakeroot
  bsdtar keeps (inference). The long description is not used here.

---

## 9. Flatpak (`packaging/flatpak/dev.mrquantumoff.vsesvit.yml`, `xtask/src/linux/flatpak.rs`)

Current manifest (verified):

- `app-id: dev.mrquantumoff.vsesvit`, `runtime: org.gnome.Platform`, `runtime-version: '51'`,
  `sdk: org.gnome.Sdk`, and `sdk-extensions: [org.freedesktop.Sdk.Extension.rust-stable]`.
  `command: vsesvit`.
- `finish-args`: `--share=ipc`, `--share=network`, `--socket=fallback-x11`, `--socket=wayland`,
  `--socket=pulseaudio`, `--device=dri` and `--filesystem=xdg-download`.
- `build-options`: `append-path: /usr/lib/sdk/rust-stable/bin` and
  `env: CARGO_HOME: /run/build/vsesvit/cargo`.
- Module `vsesvit` (buildsystem simple) runs:
  1. `cargo --offline build --release -p vsesvit`
  2. `cargo --offline run --release -p xtask -- icons icons`
  3. `install -Dm755 target/release/vsesvit /app/lib/vsesvit/vsesvit`
  4. `echo flatpak > /app/lib/vsesvit/package-format`
  5. `ln -s ../lib/vsesvit/vsesvit /app/bin/vsesvit`
  6. installs the desktop file
  7. `sed`s the version from `[workspace.package]` and the UTC date into the metainfo
  8. copies `icons/hicolor` to `/app/share/icons/`
- Sources: `type: dir` with `path: ../..` (skipping `.git`, `.audit` and `target`), plus
  `cargo-sources.json`.
- The header comment says that for Flathub the `dir` source becomes a pinned `git` source, and
  that media playback comes from the GNOME 51 runtime's `org.freedesktop.Platform.codecs-extra`
  on `GST_PLUGIN_SYSTEM_PATH`.

`xtask package flatpak`:

1. `write_cargo_sources` regenerates `packaging/flatpak/cargo-sources.json` from `Cargo.lock`.
   It writes one `archive` source (`https://static.crates.io/crates/<n>/<n>-<v>.crate`, the
   lock's sha256, `dest cargo/vendor/<n>-<v>`) and one inline `.cargo-checksum.json` per
   crates.io package, plus `cargo/config.toml` replacing crates-io with `vendored-sources`. It
   only rewrites the file when the content changed, and **errors on any non-crates.io source**
   ("only crates.io sources are vendored"). It is committed; today 319 crates.
2. `flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo`.
3. `flatpak-builder --user --force-clean --disable-rofiles-fuse --install-deps-from=flathub --state-dir=… --repo=… <build> <manifest>`.
4. `flatpak build-bundle <repo> target/dist/Vsesvit_<v>_x86_64.flatpak dev.mrquantumoff.vsesvit`.

The bundle is never signed or put in `latest.json`, and `Installation::Flatpak` never
self-updates.

Classification and changes:

- **(GTK-ONLY / WEBKIT-ONLY)** The GNOME 51 runtime and SDK exist to provide GTK, libadwaita,
  WebKitGTK and GStreamer. The codecs-extra comment goes too.
  - A CEF + GPUI app needs NSS, atk/at-spi, cups, dbus, X11 libs, gbm, cairo/pango, alsa, a
    Vulkan loader, and xkbcommon(-x11). All of these are in `org.freedesktop.Platform` (inference).
  - Chromium-based Flatpaks usually use the freedesktop runtime with the base app
    `org.electronjs.Electron2.BaseApp` (or `org.chromium.Chromium.BaseApp`), which brings
    **zypak**. Zypak redirects Chromium's sandbox to `flatpak-spawn --sandbox`, because
    Flatpak's seccomp filter forbids the nested user namespaces Chromium's own sandbox needs
    (inference). Without zypak the app must run with `--no-sandbox`.
  - With zypak, `command` becomes a wrapper (`zypak-wrapper /app/lib/vsesvit/vsesvit`), and
    `/app/lib/vsesvit/vsesvit` stays the real executable that `Installation::detect` finds the
    marker beside (inference).
- **(NEEDS-CEF) offline CEF.** The Flatpak build has no network, so cef-dll-sys's download fails
  (§0.1). The manifest needs:
  - the 326 MB minimal tarball as an `archive` or `file` source with a sha256 (the CDN index
    gives only SHA-1, so the sha256 must be computed);
  - build commands that lay it out the way `download-cef` does (flatten `Release/` and
    `Resources/`, keep `include/`, `cmake/` and `libcef_dll/`, write `archive.json` with the
    `cef_binary_154.0.34+…_minimal.tar.bz2` name);
  - `CEF_PATH` pointing there.

  Leave `FLATPAK` unset, or set it only if CEF is placed at `/usr/lib`, which a Flatpak app
  cannot do (`FLATPAK` makes cef-dll-sys read `/usr/lib`). The CEF runtime files then have to be
  installed into `/app/lib/vsesvit/`.
- **(NEEDS-CEF) cargo sources.** `gpui` from crates.io is fine. `gpui` from git would make
  `write_cargo_sources` fail, so it would need `git` sources plus `[source."git+…"]` replacement
  entries like flatpak-cargo-generator's.
- **finish-args** (inference):
  - Keep ipc, network, wayland, fallback-x11, pulseaudio, dri and xdg-download.
  - CEF's OSCrypt and core's vault (`crates/vsesvit-core/src/vault.rs`, `secret-service` crate
    on `org.freedesktop.secrets`) need `--talk-name=org.freedesktop.secrets`. Today no talk-name
    is granted, so inside the Flatpak the vault probably falls back to `plain` (inference from
    Flatpak's default D-Bus filtering; not tested).
  - Notifications use the portal (`org.freedesktop.portal.Notification`, present in
    `libcef.so`) or need `--talk-name=org.freedesktop.Notifications`.
  - MPRIS needs `--own-name=org.mpris.MediaPlayer2.*`.
  - Webcams and FIDO keys may need `--device=all`.
- **(KEEP)** The marker, the symlink, the desktop file, the metainfo, the icons, the bundle name,
  "Flatpak updates through Flatpak", and the Welcome dialog's Flatpak default-browser message.

---

## 10. AppImage (`xtask/src/linux/{appimage,sysroot,elf}.rs`, `packaging/linux/appimage/`)

### 10.1 How it is built today (verified)

- The runtime is pinned to `https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64`
  (sha256 `2fca8b44…ec260d`) and cached in `target/package/tools/` through `util::Download`
  (verified download, 256 MiB cap).
- **Sysroot** (`sysroot.rs`):
  - `apt-get update` runs with private `Dir::State::Lists` and `Dir::Cache` under
    `target/package/appimage/apt`, so no root is needed. That means a Debian/Ubuntu build host:
    it does not work on the user's Fedora host without apt (inference).
  - The closure follows `Depends`/`PreDepends` from
    `deps::required(deb) + EXTRA_PACKAGES`, taking the first alternative of each group and
    stopping at `HOST_PROVIDED` package patterns. These include `libc6`, `libgcc-s1`, `mesa-*`,
    `libgl1`, `libegl*`, `libgbm*`, `libdrm*`, `libx11*`, `fontconfig*`, `libfreetype6`,
    `libasound2*`, `libpipewire*`, `systemd*`, `dbus*`, `libnss*`, `at-spi2-*`, `libatspi*`,
    `libatk*`, `cups-*`, `libgtk-3-*` and more.
  - Packages are fetched with `apt-get download` and extracted with `dpkg-deb -x` into
    `target/package/appimage/sysroot`. A `.debs` stamp avoids re-extraction.
- `EXTRA_PACKAGES`: `gstreamer1.0-plugins-base`, `glib-networking`, `librsvg2-common`,
  `gsettings-desktop-schemas`, `adwaita-icon-theme` and `hicolor-icon-theme`.
- **Bundle** (`appimage.rs::Bundle`):
  - The staged tree is copied to `target/package/appimage/AppDir`.
  - The `DT_NEEDED` closure of `usr/lib/vsesvit/vsesvit` is walked with its own ELF reader
    (`elf.rs::needed`, so no `ldd`). Libraries land flat in `usr/lib/x86_64-linux-gnu/` under
    their soname, and the first library directory wins.
  - These are added too:
    - the WebKit helpers `webkitgtk-6.0/{WebKitWebProcess, WebKitNetworkProcess, WebKitGPUProcess, injected-bundle/libwebkitgtkinjectedbundle.so}`;
    - `gst-plugin-scanner` and `gdk-pixbuf-query-loaders`;
    - every GIO module and pixbuf loader;
    - GStreamer plugins. `REQUIRED_GST_PLUGINS` (19 named) must resolve; the others go in only
      when satisfiable, and are otherwise left out with a note.
  - The build fails when a soname is neither in the sysroot nor on the exclude list, and the
    error message names the needing files.
- **Post-steps:**
  - copy the GLib schemas and run `glib-compile-schemas`;
  - copy the Adwaita icons and `hicolor/index.theme`;
  - run `gio-querymodules`;
  - **patch `libwebkitgtk-6.0.so.4`**: `relocate` rewrites each C string `/usr/lib/x86_64-linux-gnu/webkitgtk-6.0[/…]`
    to `./wk/lib/…` at the same length. The test
    `only_the_helper_directory_is_relocated_and_the_sandbox_paths_stay` checks this;
  - make the symlink `usr/wk -> .`;
  - install `AppRun` (0755);
  - copy the desktop file to the root;
  - write `<APP_ID>.png` (256 px) and the `.DirIcon` symlink;
  - run `mksquashfs AppDir AppDir.squashfs -root-owned -noappend -no-xattrs -comp zstd -Xcompression-level 19 -quiet`;
  - concatenate the runtime and the squashfs, and set mode 0755.
- **Exclude list** (`packaging/linux/appimage/excludelist`): pkg2appimage's list (MIT). It covers
  glibc, `libstdc++`, GL/EGL/GLX/GLdispatch/OpenGL/glapi, `libgbm`, `libdrm`, `libxcb`,
  `libX11`, `libX11-xcb`, `libwayland-client`, `libasound`, fontconfig, freetype, `libcom_err`,
  `libexpat`, `libgcc_s`, `libgpg-error`, `libICE`, `libSM`, `libusb`, `libuuid`, `libz`,
  `libjack`, `libpipewire`, `libxcb-dri2/3` and `libgmp`. `libnss3.so` and `libfribidi` are
  commented out, meaning they are bundled.
- The resulting requirement (packaging.md): glibc ≥ 2.43 (Ubuntu 26.04 archive) and the host's
  Mesa.

### 10.2 `AppRun` (verified line by line) and classification

| Step | Purpose | Class |
|---|---|---|
| `here=$(dirname $(readlink -f $0))`; `export APPDIR=$here` | image root | (KEEP) |
| `LD_LIBRARY_PATH=$lib:$here/usr/lib:…` | bundled libs | (KEEP) in some form; CEF needs `usr/lib/vsesvit` (RUNPATH can replace it) |
| `XDG_DATA_DIRS=$here/usr/share:…` | GTK icons, schemas, mime | (GTK-ONLY); possibly kept for shared-mime-info (inference) |
| `GSETTINGS_SCHEMA_DIR`, `GIO_MODULE_DIR` | GLib/GTK | (GTK-ONLY) |
| `GST_PLUGIN_SYSTEM_PATH_1_0`, `GST_PLUGIN_SCANNER_1_0`, `GST_REGISTRY_1_0` | GStreamer for WebKit | (WEBKIT-ONLY) |
| `GDK_PIXBUF_MODULE_FILE` + `gdk-pixbuf-query-loaders` into `~/.cache/vsesvit/appimage/loaders.cache` | pixbuf loaders for GTK | (GTK-ONLY) |
| `if ! /usr/bin/bwrap --ro-bind / / --proc /proc --dev /dev --unshare-all true; then echo 'vsesvit: bubblewrap cannot sandbox here, so web pages run without the sandbox' >&2; export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1` | WebKit's bwrap sandbox fallback | (WEBKIT-ONLY) → **(NEEDS-CEF)**: Chromium's namespace sandbox. The AppImage is mounted `nosuid`, so `chrome-sandbox` cannot be setuid. Where user namespaces are refused (containers, Ubuntu's AppArmor userns restriction), Chromium aborts unless started with `--no-sandbox`. The product behavior to keep is "sandbox on where possible, else off with a stderr warning". `bwrap --unshare-all true` does not reliably predict Chromium's sandbox under the AppArmor restriction (inference). |
| argument rewriting: `--profile-dir`/`--self-test` values made absolute against `$OWD`, existing file arguments `readlink -f`'d, `--` respected | needed only because of the `cd` below | (WEBKIT-ONLY) as a mechanism. The behavior "relative paths given to the AppImage resolve against the user's directory" is (KEEP); without the `cd` it holds by itself |
| `cd "$here/usr"` | WebKit's patched helper path `./wk/...` resolves only from there | (WEBKIT-ONLY) |
| `exec "$here/usr/lib/vsesvit/vsesvit" "$@"` | run | (KEEP) |

Shell-side AppImage code:

- `crates/vsesvit-gtk/src/engine.rs::add_appimage_to_sandbox` adds `$APPDIR`, when the exe is
  inside it, and `~/.cache/vsesvit/appimage` to WebKit's sandbox (`WebContext::add_path_to_sandbox`)
  **(WEBKIT-ONLY)**.
- `profile.rs::program` and `updates/mod.rs` relaunch `$APPIMAGE` instead of `current_exe()`,
  because the mount goes away with the process **(KEEP)**.

### 10.3 What changes for CEF + GPUI

- Drop:
  - the WebKit helper list, `patch_webkit_paths`, `relocate` and its test, and the `usr/wk`
    link;
  - the GStreamer plugin collection and `REQUIRED_GST_PLUGINS`;
  - `gst-plugin-scanner` and `gdk-pixbuf-query-loaders`;
  - the GIO modules, the pixbuf loaders and the schemas (`glib-compile-schemas`,
    `gio-querymodules`);
  - the Adwaita icon theme;
  - `EXTRA_PACKAGES` (`glib-networking`, `librsvg2-common`, `gsettings-desktop-schemas`,
    `adwaita-icon-theme`, `gstreamer1.0-plugins-base`).
- Add the CEF runtime set in `usr/lib/vsesvit/` (§5) and strip `libcef.so`. Squashfs zstd-19 of
  the stripped `libcef.so` alone is about 83 MB (measured, single file), so the AppImage grows
  substantially (inference).
- `DT_NEEDED` walking from `libcef.so` meets sonames that today's sysroot and exclude list do not
  cover. Checked against `excludelist`:
  - **already excluded**: `libX11`, `libxcb`, `libgbm`, `libexpat`, `libasound`, `libm`,
    `libgcc_s`, `libc`, `libdl`, `libpthread`, `ld-linux-x86-64`, `libwayland-client`,
    `libfontconfig`, `libfreetype`, `libEGL`, `libGL`;
  - **not excluded, so bundled from the sysroot or a build failure**: `libglib-2.0`,
    `libgobject-2.0`, `libgio-2.0`, `libnspr4`, `libnss3`, `libnssutil3`, `libsmime3`,
    `libatk-1.0`, `libatk-bridge-2.0`, `libatspi`, `libdbus-1`, `libcups`, `libXcomposite`,
    `libXdamage`, `libXext`, `libXfixes`, `libXrandr`, `libxkbcommon`, `libxkbcommon-x11`,
    `libcairo`, `libpango-1.0`, `libudev`, `libvulkan`.

  `HOST_PROVIDED` in `sysroot.rs` stops the closure at `libnss*`, `dbus*`, `at-spi2-*`,
  `libatspi*` and `libatk*`. Those sonames therefore never enter the sysroot, and the bundler
  fails with "libraries neither in the sysroot nor on the exclude list" until the two lists are
  reconciled. If NSS is bundled, its dlopened modules (`libsoftokn3.so`, `libfreebl3.so`,
  `libnssckbi.so`) are not `DT_NEEDED` and must be added explicitly, as plugins are today
  (inference).
- `libvulkan.so.1` appears twice: CEF ships one next to `libcef.so`, and GPUI's ash dlopens
  `libvulkan.so.1` through the exe's search path. With `RUNPATH=$ORIGIN` GPUI loads CEF's copy
  (inference). The host still has to provide a Vulkan ICD for GPUI.
- The glibc floor becomes that of the build image for the `vsesvit` binary. `libcef.so` needs
  only `GLIBC_2.25` (verified), so "glibc 2.43 or newer" was a WebKit and Ubuntu 26.04
  consequence.

---

## 11. `packaging/linux/verify.sh` (manual end-to-end packaging check)

Behavior (verified):

- Usage: `packaging/linux/verify.sh [deb|rpm|pacman|appimage|flatpak]...`. With no arguments it
  checks every format. It reads `${CARGO_TARGET_DIR:-<repo>/target}/dist` and the version from
  `Cargo.toml` (CRs stripped).
- deb, rpm and pacman run in containers (`docker run --rm -v dist:/dist:ro`): `ubuntu:26.04`,
  `fedora:latest` and `archlinux:latest`. The script installs the package plus
  `desktop-file-utils` and the GStreamer tools, prints the Depends/Recommends, then runs
  `checks()`:
  - `vsesvit --version`
  - `cat /usr/lib/vsesvit/package-format` and `readlink -f /usr/bin/vsesvit`
  - `desktop-file-validate /usr/share/applications/dev.mrquantumoff.vsesvit.desktop`
  - `gst-inspect-1.0 avdec_h264` (Fedora: `avdec_aac`, because ffmpeg-free lacks H.264) and
    `gst-inspect-1.0 vp9dec`
- appimage runs in `ubuntu:26.04` with `seccomp=unconfined`, `apparmor=unconfined`,
  `systempaths=unconfined` and `CAP_SYS_PTRACE`:
  1. It installs `xvfb python3 libgl1 libegl1 libgles2 libasound2t64 fontconfig
     fonts-dejavu-core shared-mime-info procps bubblewrap`.
  2. It asserts that the container has **no** `libwebkitgtk-6.0-4` and no `libgtk-4-1`, to
     prove bundling.
  3. It creates the user `user` (home `/tmp/home`), copies the image to `/tmp/app`, and runs
     everything as that user with `APPIMAGE_EXTRACT_AND_RUN=1` (no FUSE).
  4. `--version`.
  5. `xvfb-run -a -s '-screen 0 1280x800x24' /tmp/app --self-test out` from `/tmp`. A failure
     prints `/tmp/out/report.json`. The relative `out` must resolve to `/tmp/out`.
  6. A local `python3 -m http.server` on `127.0.0.1:8000` serves an index page. The script runs
     `/tmp/app --profile-dir=profile http://127.0.0.1:8000/index.html` for 12 s and requires:
     - `pgrep -af 'WebKit(Web|Network|GPU)Process'` to find processes;
     - the server log to show `GET /index.html`;
     - `/tmp/profile` to exist (a relative profile dir resolved against the user's directory).
  7. Sandbox: if `bwrap --unshare-all true` works for the user, every `WebKitWebProc` must have
     `bwrap` as its parent and no `WEBKIT_DISABLE_SANDBOX*` in its environ. Otherwise the app
     log must contain "web pages run without the sandbox".
- flatpak runs on the host: `flatpak install --user --reinstall <bundle>`,
  `flatpak run dev.mrquantumoff.vsesvit --version`, `cat /app/lib/vsesvit/package-format`, and
  the `gst-inspect-1.0` checks for `avdec_h264` and `vp9dec` inside the app.

Changes:

- (KEEP) `--version`, the marker, the symlink, desktop-file-validate, the self-test from the
  AppImage, the page-load proof, relative-path resolution and "sandboxed where possible, warned
  otherwise".
- (WEBKIT-ONLY): the `gst-inspect` checks, the "no WebKitGTK in the container" assertion, the
  `WebKit(Web|Network|GPU)Process` process names, the `bwrap` parent check and the
  `WEBKIT_DISABLE_SANDBOX` environ check.
- (NEEDS-CEF) equivalents:
  - child processes are `vsesvit --type=zygote|renderer|gpu-process|utility` (all the same
    executable);
  - a sandboxed renderer shows a separate user namespace (`/proc/<pid>/ns/user` differs from
    the browser's) or `Seccomp: 2` in `/proc/<pid>/status` (inference);
  - the fallback message is the new AppRun's;
  - media checks become page-level (e.g. VP9 and Opus playback in the self-test), since no
    system codec packages are involved.
- GPUI under Xvfb needs a software Vulkan ICD in the container (`mesa-vulkan-drivers`, i.e.
  lavapipe) (inference). The container lists need CEF's runtime libraries for the deb, rpm and
  pacman installs (their Depends pull them in) and for the AppImage (whatever is not bundled).

---

## 12. Signing and the update manifest (`xtask/src/sign.rs`, `xtask/src/manifest.rs`)

Engine-independent; **(KEEP)** unchanged. Verified behavior:

- **`cargo xtask sign FILE...`** (and `package … --sign`):
  - The key is read from `TAURI_SIGNING_PRIVATE_KEY` (the key text or a path to it) and
    `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
  - The key is base64 of a minisign secret key box. Encrypted keys (the Tauri CLI default) and
    unencrypted ones are both accepted.
  - It writes `<file>.sig`: base64 of the minisign signature, with the trusted comment
    `timestamp:<unix>\tfile:<name>\tversion:<version>` and the untrusted comment
    `signature from tauri secret key`.
  - A tab or newline in the name or version is refused.
  - The version is the one compiled into xtask (`CARGO_PKG_VERSION`), which is why CI stamps the
    version before building the signer.
- **`cargo xtask signer generate -w FILE [-p PASSWORD] [--force]`** writes the encrypted private
  key (mode 0600, also when the file is rewritten) and `FILE.pub`, then prints the line for
  `packaging/updater.json`.
- **`cargo xtask manifest --base-url URL [--notes FILE] [--pub-date RFC3339] [--allow-unverified]`**:
  - It refuses to run when `updater.json` has no pubkey, unless `--allow-unverified` is given.
  - For every format with a `.sig` in `target/dist`, it requires the artifact to exist, verifies
    it with the pubkey (prehashed allowed), and checks that the trusted `version:` matches.
  - It writes `target/dist/latest.json`, the static format
    `{version, notes, pub_date (UTC, whole seconds, RFC 3339), platforms{key:{signature,url}}}`.
    URLs are `<base>/<percent-encoded file name>`, and the keys come from
    `Format::manifest_keys`.
  - It fails when no platform is present, and Flatpak has none. Tests check the Quadrant
    server's ingest shape.
- `cargo xtask icons OUT_DIR` (§4).

---

## 13. Release workflow: `.github/workflows/release.yml`

Verified structure:

- **Triggers.** A pushed tag `v*`, the crons `0 2 * * *` (nightly) and `0 3 * * 1` (weekly,
  Mondays), and `workflow_dispatch` with the input `channel` (nightly|weekly, default nightly).
- **Workflow-level settings.**
  - `permissions: contents: read`.
  - `concurrency: release-${{ github.ref }}-${{ github.event.schedule || inputs.channel || 'tag' }}`
    with no cancel.
  - env `CARGO_TERM_COLOR=always` and `UPDATE_SERVER=https://api.usequadrant.dev/api/any/vsesvit`.
- **Jobs:**
  1. **plan** (ubuntu-latest; `contents: write`, `actions: read`). It checks out without
     persisted credentials and runs `bash scripts/release-plan.sh | tee -a $GITHUB_OUTPUT`
     (with `shell: bash` so pipefail applies), giving the outputs version, channel, tag,
     publish, prerelease and skip. For non-push runs a Tag step creates `refs/tags/$TAG` on the
     run's SHA through `gh api`. A re-run accepts the same SHA, and any other SHA is an error.
  2. **windows** (windows-latest): NSIS through `choco install nsis` if it is missing, then
     `set-version.sh`, `cargo xtask package nsis`, and an upload of `dist-windows`. No change.
  3. **linux** (ubuntu-latest, **`container: ubuntu:26.04`**):
     - It installs, with apt and `--no-install-recommends`, `build-essential pkg-config curl
       ca-certificates git libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev libglib2.0-bin
       dpkg-dev rpm libarchive-tools zstd fakeroot squashfs-tools desktop-file-utils appstream`.
     - It marks the workspace a git `safe.directory`, then runs the Rust toolchain action
       (stable), rust-cache, `set-version.sh`, `cargo xtask package deb rpm pacman appimage`,
       and uploads `dist-linux`.
     - The comment reads: "GTK 4.22, libadwaita 1.9 and WebKitGTK 2.52 need Ubuntu 26.04; the
       AppImage bundles its libraries from this image's archive."
  4. **flatpak** (ubuntu-latest, no container; skipped when the channel is `nightly`):
     - It installs `flatpak flatpak-builder` with apt (recommends kept for `eu-strip`) and runs
       `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0 || true`, because
       flatpak-builder uses bwrap user namespaces.
     - It then runs the toolchain, rust-cache, `set-version.sh` and
       `cargo xtask package flatpak`, and uploads `dist-flatpak`.
  5. **signer** (ubuntu-latest): the toolchain, **no rust-cache** (the comment explains that a
     cache could carry code from the package builds), `set-version.sh`,
     `cargo build --release --locked -p xtask`, and an upload of `signer`.
  6. **sign** (needs plan, windows, linux, signer). It downloads the artifacts and, with the
     secrets `TAURI_SIGNING_PRIVATE_KEY` and `…_PASSWORD`, runs
     `signer/xtask sign target/dist/*-setup.exe *.deb *.rpm *.pkg.tar.zst *.AppImage`. It uploads
     `dist-signatures` and runs no cargo.
  7. **release** (needs all five; runs when every required job succeeded and flatpak succeeded,
     or was skipped on nightly; `contents: write`):
     1. the toolchain, rust-cache and `set-version.sh`;
     2. download `dist-*` (merged);
     3. release notes from `gh api repos/$REPO/releases/generate-notes`;
     4. `cargo xtask manifest --base-url https://github.com/$REPO/releases/download/$TAG --notes notes.md`;
     5. `gh release create` (only if missing) with the title `Vsesvit $VERSION`,
        `--verify-tag`, and `--prerelease` or `--latest`, then `gh release upload --clobber target/dist/*`;
     6. `curl --fail-with-body -X PUT -H "Authorization: $TOKEN"` with the `latest.json` body to
        `$UPDATE_SERVER/add_updates/$CHANNEL/$VERSION?publish=$PUBLISH` (secret
        `VERSION_UPDATE_TRIGGER_TOKEN`).
- Every action is pinned to a 40-hex commit.

Invariants enforced by `xtask/tests/release_workflow.rs` (KEEP; any new job must satisfy them):

- every job that mentions `TAURI_SIGNING_PRIVATE_KEY` runs no `cargo` and contains `xtask sign `;
- **every** `uses:` in **every** workflow file is pinned to a 40-hex commit (except `./` and
  `docker://`);
- the workflow-level permissions are exactly `contents: read`, only `plan` and `release` have
  `contents: write`, and every `actions/checkout@` step has `persist-credentials: false`.

What must change for CEF + GPUI:

- **linux job apt list.** Remove `libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev
  libglib2.0-bin` (glib-compile-schemas and gio-querymodules are no longer needed). Add the GPUI
  build dependencies: `libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libxcb1-dev`
  (or `libx11-xcb-dev`), `libfontconfig-dev`, optionally `libfreetype-dev` (inference). cmake
  and ninja are not needed for cef-dll-sys on Linux (verified, §0.1). Keep the packaging tools.
- **CEF download.** cef-dll-sys downloads about 326 MB from `cef-builds.spotifycdn.com` during
  `cargo build` unless `CEF_PATH` names a prepared directory. Into `OUT_DIR` the download repeats
  per profile and after cache misses. An `actions/cache` keyed on the CEF version with
  `CEF_PATH`, or a pinned download step, would control this (inference). rust-cache would
  otherwise cache roughly 1.7 GB of extracted CEF inside `target/` (inference; GitHub cache
  limits apply). The runner's disk budget must hold the CEF copy, the `target/release` copy and
  the staging copies for four formats (inference).
- **Container image.** The `ubuntu:26.04` requirement came from GTK and WebKit. With CEF the
  image decides only the binary's glibc floor and the AppImage sysroot's archive (inference).
- **flatpak job.** It still needs bwrap. The manifest now needs the CEF tarball as a source
  (§9). Skipping Flatpak on nightly is (KEEP).
- **signer, sign, release.** Unchanged. They build only `xtask`, which does not depend on CEF or
  GPUI, so no CEF download runs where the key is.
- No job runs `cargo test` or the self-test for the browser today (verified). Adding one needs
  Xvfb, a Vulkan software ICD and CEF's runtime libraries in that job (inference).

---

## 14. Scripts

- **`scripts/release-plan.sh`** (KEEP). It reads the `[workspace.package]` version and the
  `updater.json` pubkey, and refuses to plan without a pubkey ("Generate one with 'cargo xtask
  signer generate -w <file>'…").
  - On a push, the tag must match `^v?X.Y.Z(-[A-Za-z][0-9A-Za-z.]*)?$` and equal the workspace
    version. Its channel is `stable`, or the prerelease's first field, which must be `beta`:
    `nightly` and `weekly` tags are refused ("builds come from the schedule"), and any other
    name gives "no browser follows a '<c>' channel".
  - On a schedule or dispatch, the channel is nightly or weekly. The base is the workspace
    version, or its next patch once `v<workspace>` is released. `day` is the run's
    `created_at` as `YYYYMMDD`, and the version is `<base>-<channel>.<day>.<run_number>`. A
    scheduled run skips when the channel's last release is another tag on the same SHA.
  - `publish` is true for nightly and weekly; `prerelease` is false only for stable.
  - Output: `version= channel= tag= publish= prerelease= skip=`.
- **`scripts/set-version.sh VERSION`** (KEEP) `sed`s `version = "…"` inside `[workspace.package]`,
  greps that it took, and runs `cargo update --workspace --quiet`, so every binary reports the
  version `latest.json` announces ("a mismatch would make installed copies update in a loop").
  With CEF in the graph, `cargo update --workspace` still only touches the lock (no build).
- **`scripts/wsl.sh build|test|run -- [args]`** (KEEP, with caveats). From Git Bash it converts
  the path (`cygpath`, `wslpath`) and re-runs itself in `wsl.exe -d Ubuntu` (distribution name
  hard-coded). Inside WSL it sets
  `CARGO_TARGET_DIR=$HOME/.cache/vsesvit-target/<basename>-<md5(path)[:8]>` and runs `cargo
  "$@"`. With CEF, every target directory re-downloads CEF unless `CEF_PATH` is set. GPUI under
  WSLg needs Vulkan, which WSL provides only through Mesa's `dzn` or lavapipe (inference; a
  risk for "build and run from Windows").
- **`scripts/sync-e2e.sh`** (unaffected): it builds `server/` and runs `cargo test -p vsesvit-sync`
  with `VSESVIT_SYNC_SERVER_BIN`.
- **`scripts/screenshot-window.ps1`**: Windows only, unaffected.

`.github/workflows/sync-server.yml` (unaffected). Its path filters include `crates/vsesvit-core/**`,
`crates/vsesvit-sync/**`, `Cargo.toml` and `Cargo.lock`. It runs `scripts/sync-e2e.sh` and the
server tests on SQLite and Postgres, and builds and publishes `ghcr.io/<owner>/vsesvit-sync-server`.
The server Docker build copies only `crates/vsesvit-sync-proto` and `server/`. A workspace with a
CEF crate does not affect it, because `cargo test -p vsesvit-sync` does not build the Linux
shell. Its actions are pinned, as the xtask test requires.

---

## 15. Workspace `Cargo.toml`, `.cargo/config.toml`, `.gitattributes`

- `[workspace]`: `resolver = "3"`, `members = ["crates/*", "xtask"]`. A new shell crate under
  `crates/` joins automatically; deleting `crates/vsesvit-gtk` and `crates/vsesvit-webext`
  removes them.
- `[workspace.package]`: `version = "0.1.0"` (stamped by `set-version.sh`), `edition = "2024"`,
  `rust-version = "1.98"`, `license = "MIT"`. `gpui` 0.2.2 is edition 2024 and `cef` edition
  2021; both fit.
- `[workspace.dependencies]`:
  - `vsesvit-core`, **`vsesvit-webext` (l. 13)**, **`vsesvit-gtk` (l. 14)**, `vsesvit-winui`,
    `vsesvit-update`, `vsesvit-sync`;
  - `serde`, `serde_json`, `thiserror`, `url`, `log`;
  - **`gtk = gtk4 0.11 [v4_22]` (l. 25)**, **`adw = libadwaita 0.9 [v1_9]` (l. 26)**,
    **`webkit = webkit6 0.6 [v2_52]` (l. 27)**, **`javascriptcore = javascriptcore6 0.6`
    (l. 28)**;
  - `windows-*` 0.100.

  The four GTK and WebKit entries plus the two crate paths go. `cef` and `gpui` entries come in.
  `Cargo.lock` loses `gtk4*`, `libadwaita*`, `webkit6*`, `javascriptcore6*`, `soup3*`, glib and
  others. `packaging/flatpak/cargo-sources.json` is regenerated from it.
- Profiles: `[profile.dev.package.scrypt|argon2|blake2] opt-level = 3`, so key and passphrase
  tests are fast (KEEP).
- `crates/vsesvit/Cargo.toml`: `[target.'cfg(target_os = "linux")'.dependencies] vsesvit-gtk`
  (l. 14) and `[target.'cfg(windows)'.dependencies] vsesvit-winui`; `winresource` build-dep on
  Windows. `crates/vsesvit/src/main.rs:7` calls `vsesvit_gtk::run()`. Both are renamed to the new
  shell.
- `crates/vsesvit-gtk/Cargo.toml`: feature `default = ["self-test"]`,
  `self-test = ["vsesvit-core/testkit"]`. The comment explains it is default because the
  self-test is the acceptance check run on the shipped optimized binary, and `--no-default-features`
  drops the test key material. The Linux deps are `vsesvit-core`, `vsesvit-webext`,
  `vsesvit-update`, `vsesvit-sync`, gtk, adw, webkit, javascriptcore, serde, serde_json, log,
  url, semver, futures-channel, futures-util, and glib 0.22 (`log`) / gio 0.22 (`v2_80`);
  dev-deps add `vsesvit-core[testkit]` and rusqlite 0.40 bundled. (KEEP) the feature design;
  the GTK-family dependencies go.
- `.cargo/config.toml`: `[alias] xtask = "run --quiet --package xtask --"` (KEEP).
- `.gitattributes`: `*.sh text eol=lf` and `packaging/linux/** text eol=lf`, because Linux
  packages are built from a Windows checkout too (`wsl.sh`) and CRLF breaks AppRun's shebang and
  `desktop-file-validate`. The xtask test `the_linux_packaging_files_have_unix_line_endings`
  walks `packaging/linux`. Any new Linux packaging file (an AppArmor profile, maintainer scripts,
  a zypak wrapper) belongs under these patterns (KEEP).
- `.gitignore`: `/target`, `/server/target`, `/server/.env`, `/server/*.db*`, `/.audit/scratch/`,
  `*.crx` (except fixtures), `.updater/`. A local `CEF_PATH` directory inside the repo would need
  an entry (inference).

---

## 16. `README.md`

Content (verified):

- The logo `<picture>` (DarkBG/WhiteBG).
- "A native web browser for Windows 11 and Linux, written in Rust." followed by "Each platform
  gets a shell built with its own toolkit around its own web engine, on a shared Rust core:
  WinUI 3 with WebView2 on Windows, GTK4/libadwaita with WebKitGTK on Linux." (l. 12).
- Features list:
  - Chrome Web Store extensions "on both platforms", plus Edge Add-ons, AMO and local `.crx`/`.xpi`;
  - vertical tabs and tab groups with Chrome's tab menu;
  - private windows;
  - Memory Saver;
  - downloads with a dangerous-file check;
  - bookmark import and export;
  - profiles;
  - self-hosted end-to-end-encrypted sync;
  - privacy (tracker blocking, HTTPS-only, cookie controls; passwords left to password
    managers).
- Install: GitHub Releases, channels stable/beta/weekly/nightly; "Linux needs Ubuntu 26.04 or
  newer, or equivalent GTK 4.22, libadwaita 1.9, and WebKitGTK 2.52." (l. 32).
- Build on Linux: "Requires GTK 4.22, libadwaita 1.9, WebKitGTK 2.52, and GLib 2.80+" (l. 38),
  `sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev`
  (l. 41), and the GStreamer plugins for media (l. 44). Then `cargo run -p vsesvit`, or under
  WSL `bash scripts/wsl.sh run -p vsesvit`.
- Windows build requirements.
- Test: `cargo test --workspace`; `vsesvit --self-test <dir>` writes `report.json` and a
  screenshot, `--network` installs uBlock Origin Lite.
- Layout: `crates/vsesvit-gtk  Linux shell (GTK4 + libadwaita + WebKitGTK)` (l. 74) and
  `crates/vsesvit-webext  WebExtensions runtime for WebKitGTK` (l. 75).
- License MIT.

Changes:

- ll. 12, 32, 38, 41, 44, 74 and 75: the toolkit and engine wording, the Linux requirements
  (CEF libraries and Vulkan instead of GTK, WebKit and GStreamer), the build dependencies
  (GPUI's `-dev` packages, the CEF download or `CEF_PATH`), and the crate layout.
- "Chrome Web Store extensions on both platforms" stays true (KEEP).
- The H.264/AAC media caveat (§0.1) may need mentioning.

---

## 17. `docs/design/packaging.md`

Sections and their status:

| Section | Content (verified) | Change |
|---|---|---|
| intro | 6 formats; all but Flatpak self-update via the Tauri updater protocol | (KEEP) |
| app id (l. 5) | "the GTK application id, the desktop file, the AppStream id, the Flatpak id and the icon name" | replace "GTK application id" with the window app_id / Wayland app_id (GTK-ONLY wording) |
| The update protocol | §7.2 steps 1-7; per-format install table (NSIS `/P /UPDATE /R /ARGS`, `/S /UPDATE` on quit; deb `pkexec apt-get install -y --no-install-recommends <abs path>` and why not `dpkg -i`; rpm; pacman; AppImage rename; Flatpak not handled) | (KEEP) |
| The installation marker | `package-format`, the `lib/vsesvit` + `bin` symlink layout, the `Installation` enum | (KEEP); CEF files join `lib/vsesvit` |
| Configuration | `updater.json` = Tauri `plugins.updater`, the build-time env overrides, empty pubkey turns the updater off | (KEEP) |
| Building packages | the xtask commands, the Tauri-compatible signing env and `.sig` format, the artifact names and keys table | (KEEP) |
| Runtime dependencies (ll. 79-95) | GStreamer as hard deps, the Fedora ffmpeg-free H.264 caveat, the GTK/libadwaita/WebKitGTK/GStreamer/pkexec table, "the AppImage bundles all of these from the Ubuntu 26.04 archive, so it needs glibc 2.43", the GNOME 51 runtime + codecs-extra, the WebKit helper path patch and `./wk`, the bubblewrap sandbox and the AppRun fallback, "distribution packages and Flatpak keep it on" | rewrite: CEF and GPUI dependency table (§8.2), the CEF codec situation, the CEF sandbox (setuid, userns, AppArmor, zypak), the CEF file layout; drop the WebKit path patch (WEBKIT-ONLY) |
| Windows App Runtime paragraph | NSIS checks and installs WinAppRuntime ≥ 2.5.1 | (KEEP) |
| Releasing | trigger/channel/version table, `<next>` rule, channel ordering, the `{{channel}}` placeholder and `UpdateChannel::of_build`, `updates.channel` per installation, prerelease spelling per package manager, re-run safety, plan-job tagging, set-version, the job split and the signing isolation, the secrets | (KEEP); the Linux job description ("in an `ubuntu:26.04` container") follows §13 |
| The Quadrant update server | the GET endpoint, `PUT add_updates/<channel>/<version>?publish=`, quadrant_api ≥ 12.5.0, stable refuses `publish=true` | (KEEP) |

---

## 18. `docs/PLAN.md`: every Linux toolkit, engine and runtime statement

PLAN.md is product-wide. The feature inventories cover the feature behavior in detail. This table
indexes every Linux-specific statement (by line) so none is lost, and classifies it. API names in
the "CEF equivalent" column are likely candidates (inference unless they are in §0).

| Line(s) | Statement | Class | CEF / GPUI note |
|---|---|---|---|
| 5-10 | Platform table: GTK 4.22 + libadwaita 1.9; WebKitGTK 6.0 (2.52); extension runtime `vsesvit-webext`; bindings `gtk4` 0.11, `libadwaita` 0.9, `webkit6` 0.6 | docs | becomes GPUI 0.2.2 / CEF 154.0.34 (Chromium 154.0.8037.98) / Chromium's extension system / `gpui` + `cef` 154.5.0 |
| 20 | Decision "Linux uses WebKitGTK, not Chromium" (CEF can't embed in GTK4 except by OSR; extension runtime built as GNOME Web does) | docs | reversed by the user's directive; the decision needs a new record (`docs/adr/`) |
| 29 | "On Linux, `vsesvit-webext` runs it" (the unpacked, key-injected dir) | (WEBKIT-ONLY) → (NEEDS-CEF) | load the unpacked extension into Chrome-style CEF's extension system (mechanism to be settled by the extensions inventory) |
| 37 | Tracking protection on Linux: core rules → `vsesvit-webext/src/dnr.rs` → WebKit content blocker; "WebKitGTK reports no content-blocker hits, so Linux shows no count" | (WEBKIT-ONLY) limitation | (NEEDS-CEF) per-request decision as on Windows: `CefResourceRequestHandler::OnBeforeResourceLoad` returning `RV_CANCEL`, or a 403 via `GetResourceHandler`; a blocked count becomes possible |
| 39 | HTTPS-only: WebKit's `decide-policy` cannot tell frames apart, so Linux upgrades only surely-main-frame loads; one http request may leak | (WEBKIT-ONLY) | `CefRequestHandler::OnBeforeBrowse` (frame known: `CefFrame::IsMain`) and redirect to https; warning page via a custom scheme or data URL |
| 41 | Secure DNS: "WebKitGTK uses the system resolver, so Linux Settings says the system's network settings control DNS" | (WEBKIT-ONLY) | Chromium has its own DoH (`dns_over_https.*` prefs via `CefRequestContext::SetPreference` / global prefs); Settings wording changes |
| 43 | Cookies: WebKitGTK has one accept policy per network session (no per-site Allow on Linux); blocked sites via content blocker + script; website data manager | (WEBKIT-ONLY) | `CefRequestContext::SetContentSetting(…COOKIES…)`, third-party cookie prefs, `CefCookieManager`, DevTools `Storage.clearDataForOrigin` via `ExecuteDevToolsMethod` |
| 45 | "WebKitGTK has no password store" | (WEBKIT-ONLY) | (NEEDS-CEF) Chrome style has a password manager; keep it off (prefs such as `credentials_enable_service`, inference) |
| 55-56 | Architecture lines for `vsesvit-gtk` and `vsesvit-webext` | docs | rename/delete |
| 77-78 | Vault key in the Secret Service "Vsesvit Safe Storage"; Linux may wait on a keyring prompt | (KEEP) core | unchanged; Flatpak talk-name gap (§9); CEF's OSCrypt uses the same service separately (inference) |
| 81 | `engine/` = "WebKit network session data" | (NEEDS-CEF) | CEF `root_cache_path`/`cache_path` under the profile's `engine/`; core's `ProfilePaths::engine_data`/`engine_cache` (`crates/vsesvit-core/src/lib.rs:147-149`) |
| 104 | Tabs on Linux: `AdwOverlaySplitView`, `AdwTabView`, `AdwTabBar`, per-tab menu | (GTK-ONLY) | GPUI re-implementation |
| 113 | Tab search: `GtkPopover` + `GtkSearchEntry` + `GtkListBox`; "a reopened tab keeps its back/forward history" | (GTK-ONLY) + (NEEDS-CEF) | CEF exposes navigation entries read-only (`GetNavigationEntries`); restoring history into a new browser has no public API (inference), so this may become a Windows-like limitation |
| 122 | Tab groups on Linux use `AdwTabView`/`AdwTabBar` limitations (chips, dots) | (GTK-ONLY) | GPUI |
| 129 | Private windows: `NetworkSession::new_ephemeral()`, default web context kept | (WEBKIT-ONLY) → (NEEDS-CEF) | a `CefRequestContext` with an empty `cache_path` is in-memory ("incognito mode", per the `cache_path` doc in §0.1) |
| 144 | Memory Saver on Linux: `webkit_web_view_terminate_web_process`, faded icon, reload on select | (WEBKIT-ONLY) | (NEEDS-CEF) no tab-discard API in CEF (inference); options are closing and recreating the browser (losing history) or `WasHidden` |
| 154 | Page commands: WebKit print dialog, inspector, custom `view-source` scheme (registered local, gated) | (WEBKIT-ONLY) for view-source | Chromium renders `view-source:` natively; `CefBrowserHost::Print`, `ShowDevTools` |
| 161 | Spell check through Enchant/Hunspell dirs | (WEBKIT-ONLY) | (NEEDS-CEF) Chromium spellcheck (`spellcheck.dictionaries` prefs, `.bdic` dictionaries, `CefBrowserHost::AddWordToDictionary`, `ReplaceMisspelling`) (inference) |
| 168 | Search engine editor is an `AdwAlertDialog` | (GTK-ONLY) | GPUI |
| 191 | "WebKitGTK has no way to pause a download" | (WEBKIT-ONLY) | `CefDownloadItemCallback::Pause/Resume/Cancel`, `CefDownloadHandler::OnBeforeDownload/OnDownloadUpdated` |
| 197 | Profile switch launched "through the display's launch context, so on Wayland it carries an activation token" | (GTK-ONLY) | GPUI/xdg-activation: pass `XDG_ACTIVATION_TOKEN` to the child (inference) |
| 212 | "The GTK shell shows an Adwaita alert" for the install prompt | (GTK-ONLY) | GPUI dialog |
| 216-229 | The whole `vsesvit-webext` subset: `chrome-extension://` scheme, hidden background view, isolated worlds, user-level CSS, chrome.* coverage, windows (Wayland reports origin), webNavigation via frame scripts, cookies via store diffs, menus, commands, notifications via GNotification (portal in Flatpak), DNR → content blockers, permissions with WebKit cross-origin exceptions, unsupported list | (WEBKIT-ONLY) | Chromium's extension system replaces it; notifications for extensions go through Chromium's Linux notification bridge (`org.freedesktop.Notifications`/portal strings are in `libcef.so`); "Wayland does not tell a window where it is" stays a Wayland fact |
| 239-242 | Milestones mention WebKitGTK messaging, GTK/libadwaita UI, the webext runtime | docs | update |
| 244 | Milestone 6: packaging formats list | (KEEP) | |
| 246-254 | Linux build dependencies: GTK 4.22, libadwaita 1.9, WebKitGTK 2.52, GLib 2.80, Ubuntu 26.04; the apt line; `libwebkitgtk-6.0-dev` pulls soup and JSC; SQLite bundled and rustls (no `libsqlite3-dev`/`libssl-dev`); GStreamer plugins | docs | rewrite for CEF + GPUI; "SQLite bundled, TLS via rustls" stays true for core (KEEP) |

Engine- and toolkit-neutral statements elsewhere in PLAN.md (sync, CRDTs, profiles list, shortcut
table, search engines' core rules, suggestions, export, downloads list, dangerous types with the
Linux set `.sh`, `.deb`, `.rpm`, `.desktop`, `.AppImage`, `.jar`) are (KEEP).

---

## 19. Every reference to GTK, libadwaita, WebKitGTK, `vsesvit-gtk` or `vsesvit-webext` outside the two crates

All verified with `git grep`. `Cargo.lock`, `cargo-sources.json` and `.audit/` are left out of
the list but are regenerated or obsolete.

| Place | Reference | Required change for CEF + GPUI |
|---|---|---|
| `Cargo.toml:13-14` | `vsesvit-webext`, `vsesvit-gtk` workspace deps | remove; add the new shell crate |
| `Cargo.toml:25-28` | `gtk`, `adw`, `webkit`, `javascriptcore` | remove; add `cef = "154.5.0"` and `gpui = "0.2.2"` (or git) |
| `crates/vsesvit/Cargo.toml:14`, `crates/vsesvit/src/main.rs:7` | Linux shell dep and `vsesvit_gtk::run()` | point at the new shell |
| `xtask/src/linux/deps.rs:12-17` | GTK/adw/WebKit/GStreamer runtime table | replace (§8.2) |
| `xtask/src/linux/deb.rs:27`, `rpm.rs:37` | "built on WebKitGTK" long description | reword |
| `xtask/src/linux/appimage.rs` (module doc, `EXTRA_PACKAGES`, `WEBKIT_HELPERS`, `REQUIRED_GST_PLUGINS`, GIO/pixbuf/schemas/Adwaita steps, `patch_webkit_paths`, `relocate`, `HELPERS_LINK`, test) | WebKitGTK AppImage machinery | remove and replace with the CEF file set, NSS modules, exclude-list and sysroot reconciliation (§10.3) |
| `xtask/src/linux/sysroot.rs:5,24-26` | host-provided patterns incl. `libatk*`, `at-spi2-*`, `libnss*`, `dbus*`, `libgtk-3-*` | revisit against CEF's needs (§10.3) |
| `packaging/linux/appimage/AppRun` | WebKit helper `cd`, GStreamer/GIO/GSettings/pixbuf env, bwrap probe, `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS` | rewrite (§10.2) |
| `packaging/linux/appimage/excludelist` | comment "Vsesvit bundles [fribidi]: pango needs it"; `libnss3.so` commented | revisit for CEF (NSS, X libs, atk, dbus, cups) |
| `packaging/linux/verify.sh` | gst-inspect checks, "container must not have WebKitGTK", WebKit process names, bwrap parent check | rewrite (§11) |
| `packaging/linux/dev.mrquantumoff.vsesvit.metainfo.xml:13` | "built on WebKitGTK" | reword |
| `packaging/flatpak/dev.mrquantumoff.vsesvit.yml:5-11` | GNOME 51 runtime/SDK, codecs-extra comment | new runtime/base app, zypak, offline CEF source (§9) |
| `.github/workflows/release.yml:121-132` | comment about GTK/WebKit needing 26.04; `libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev libglib2.0-bin` | new apt list and CEF caching (§13) |
| `.github/workflows/release.yml:157` | bubblewrap comment for flatpak-builder | stays (flatpak-builder) |
| `README.md:12,32,38,41,44,74,75` | toolkit/engine wording, requirements, apt line, layout | rewrite (§16) |
| `docs/design/packaging.md:5,81-95` | GTK app id, the GTK/WebKit/GStreamer deps table, the WebKit AppImage paragraph, GNOME runtime | rewrite (§17) |
| `docs/PLAN.md` (lines in §18) | many | update per §18 |
| `docs/design/grounding.md:9,16,19,28,35` | "Chromium/CEF was rejected on Linux because it cannot embed natively in GTK4"; WSL Ubuntu 26.04 with GTK/WebKit versions; WebKitGTK has no extension runtime; `vsesvit-gtk` layout; "WebKitWebView" | update; the CEF rejection statement is now reversed |
| `docs/design/self-test.md:45,65,71,74,81` | Linux rows naming WebKit (memory saver via web process, `download_pause` Windows-only, spellcheck via WebKit's web context, page commands `win.*` GTK actions, WebKit mock capture devices) | self-test inventory owner updates |
| `docs/design/core.md:24,33,52,66,130,331-332,430,504`, `docs/design/arena/candidate-{1,2,3}.md` | historical design text naming WebKitGTK/`webkit6`/GTK | historical; optionally annotate |
| `docs/design/research-linux-extensions.md` | the whole WebKitGTK extension research | historical (superseded) |
| `crates/vsesvit-core/src/lib.rs:5,103,147,149` | doc comments: shells are GTK4 + WebKitGTK, `Rc<RefCell<Profile>>`, `engine_data` = "WebKit `NetworkSession` data dir", `engine_cache` = "WebKit cache dir" | comments; the paths stay, their meaning becomes CEF's cache dirs |
| `crates/vsesvit-core/src/session.rs:9-10` | `tab_restore_state` holds WebKit's session-state blob | (NEEDS-CEF): CEF has no restorable session blob (inference); the session-restore inventory owns this |
| `crates/vsesvit-core/src/{cookies.rs:65,159, trackers.rs:9-10, view_source.rs:2, history.rs:223, certificate.rs:4, extensions/manifest.rs:45, https_only.rs:21, memory_saver.rs:28, profiles.rs:96}` | comments about WebKit behavior or GTK title-casing; `profiles.rs:96` avatar colours are "libadwaita's accent colours" | comments; the colour values stay (KEEP) |
| `crates/vsesvit-core/src/favicons/fetch.rs:38` | a Chrome UA string containing "AppleWebKit" | unrelated (a UA string) |

---

## 20. Interactions with other modules and with the self-test

- **Self-test.**
  - verify.sh runs `vsesvit --self-test out` from the AppImage under Xvfb as a non-root user and
    prints `report.json`. `report.json` and `window.png` (plus per-check PNGs) are the shell's
    acceptance contract (`docs/design/self-test.md`; the report is written with
    `vsesvit_core::testkit::report`).
  - The self-test needs the `self-test` cargo feature in release builds (default feature).
  - Its `welcome` check records the default-browser status, which depends on the installed
    desktop entry (§2).
  - No CI job runs it today.
- **Profiles and single instance.**
  - `profile.rs::launch` starts another profile as `program --profile-dir <root>` through
    `gio::AppInfo::create_from_commandline` with `SUPPORTS_STARTUP_NOTIFICATION` and the
    display's launch context (Wayland activation token) **(GTK-ONLY)**.
  - `profile.rs::notify` spawns `program --profile-dir <root>` without a token, to make a
    running profile re-read `profiles.json`.
  - `program()` is `$APPIMAGE` for an AppImage **(KEEP)**.
  - The GApplication `HANDLES_COMMAND_LINE` forwarding becomes CEF's
    `OnAlreadyRunningAppRelaunch` on the same `root_cache_path` **(NEEDS-CEF)**.
- **Downloads dir.** `profile.rs::downloads_dir` uses `glib::user_special_dir(Downloads)` or
  `~/Downloads` **(GTK-ONLY → XDG user-dirs)**. The Flatpak grants `xdg-download` only.
- **Updates ↔ Settings ↔ windows.** See §7.3. The banner is in every window, the Settings
  group appears only for self-updating installs, and toasts report install failures.
- **Logging.** `GlibLogger` routes `log` into the journal today **(GTK-ONLY)**. PLAN.md's
  private-window rule (`Browsing::loggable`) must keep private addresses out of logs. CEF's own
  log must not land in `/usr/lib/vsesvit/debug.log` (§0.1).
- **Core paths.** `vsesvit_core::ProfilePaths::{engine_data, engine_cache}` are the directories
  CEF's caches should use (`<root>/engine`, and `<cache_dir>/Vsesvit/<name>` or `<root>/cache`).
  `Profile::chrome_version()`/`OpenOptions::chrome_version` (default
  `extensions::DEFAULT_CHROME_VERSION`) is the Chrome version core reports to the stores. With
  CEF it can be CEF's Chromium version, 154.0.8037.98 (inference; owned by the extensions
  inventory).

---

## 21. Risks surfaced by this inventory

1. **No H.264 or AAC** in Spotify-CDN CEF builds (verified from the symbol table). Today's
   packages deliver H.264 through `gstreamer1.0-libav`, so this is a playback regression on many
  sites unless a CEF build with proprietary codecs is used.
2. **The sandbox differs per format.** deb, rpm and pacman need a setuid `chrome-sandbox`, or
   user namespaces plus an AppArmor profile on Ubuntu ≥ 23.10. The AppImage cannot be setuid
   and must fall back to `--no-sandbox` where namespaces are refused. The Flatpak needs zypak
   or `--no-sandbox`.
3. **Default CEF writes `debug.log` next to the executable**, which is read-only in packages.
4. **The executable has no RUNPATH from cef-rs**, and `libcef.so` is a hard `DT_NEEDED`. Even
   `--version` fails without it, and so do `--check-for-updates` and `--update`.
5. **CEF child processes re-execute the binary with Chromium switches** that today's CLI parser
   rejects with exit 2.
6. **The AppImage bundler's sysroot and exclude list will reject CEF's libraries** (NSS,
   atk/at-spi, dbus) until reconciled. Bundled NSS needs its dlopened modules too.
7. **Flatpak builds offline.** The CEF tarball must become a manifest source and be laid out for
   cef-dll-sys. A `gpui` git dependency breaks `xtask`'s cargo-sources generator.
8. **Package size.** The unstripped `libcef.so` is 1.45 GB. Staging must strip it (272 MB);
   install size and CI disk use grow a lot.
9. **Vulkan is a hard requirement for GPUI** (blade). VMs or containers without an ICD fail;
   CI and verify.sh need lavapipe. `libvulkan.so.1` is shipped by CEF and also loaded by GPUI.
10. **Restart after update** must release CEF's `root_cache_path` singleton before spawning, or
    the new process forwards its command line to the dying one and exits.
11. **A deb/rpm/pacman update replaces CEF files under a running browser.** New child processes
    may fail until the user restarts.
12. **Licensing metadata** (MIT only) does not reflect bundled CEF/Chromium; `CREDITS.html` must
    ship.

---

## 22. Checklist

- [ ] Window app id `dev.mrquantumoff.vsesvit` on Wayland (`xdg_toplevel`) and X11 (`WM_CLASS`), matching `StartupWMClass`
- [ ] Application name "Vsesvit"; window icon from hicolor `dev.mrquantumoff.vsesvit` (and X11 icon if GPUI allows)
- [ ] Desktop entry installed unchanged (Name, GenericName, Comment, `Exec=vsesvit %U`, Icon, Categories, 16 MimeTypes, Keywords, StartupNotify, StartupWMClass)
- [ ] Startup notification / xdg-activation token consumed on launch
- [ ] Default-browser status and "make default" for `x-scheme-handler/http`, `x-scheme-handler/https`, `text/html` without GIO; Flatpak and no-desktop-entry messages verbatim
- [ ] AppStream metainfo with `@VERSION@`/`@DATE@` substitution; description no longer says WebKitGTK; validated with appstreamcli
- [ ] Licence metadata decision for bundled CEF/Chromium; `CREDITS.html` shipped
- [ ] Icons: hicolor PNG 16-512 + scalable SVG from `xtask icons`; AppImage `.DirIcon` 256 px; Adwaita theme no longer bundled
- [ ] `usr/lib/vsesvit/vsesvit` + `package-format` + `usr/bin/vsesvit` symlink layout kept
- [ ] CEF runtime set installed beside the executable: `libcef.so` (stripped), `chrome-sandbox`, `libvk_swiftshader.so`, `libvulkan.so.1`, `vk_swiftshader_icd.json`, `v8_context_snapshot.bin`, `icudtl.dat`, `resources.pak`, `chrome_100_percent.pak`, `chrome_200_percent.pak`, `locales/`
- [ ] Executable gets `RUNPATH=$ORIGIN` (or an equivalent) so `libcef.so` resolves through the `/usr/bin` symlink
- [ ] `browser_subprocess_path` empty (same binary) or set; `resources_dir_path`/`locales_dir_path` consistent with the layout
- [ ] CEF `log_file`/`log_severity` set so nothing writes into the install directory
- [ ] `cef::execute_process` runs before CLI parsing; Chromium child switches never reach `cli::parse`
- [ ] CLI usage text, parse rules, exit code 2 on errors, `vsesvit --version` = `vsesvit <version>`
- [ ] `--profile-dir`, `--self-test OUT_DIR [--network]`, `--check-for-updates`, `--update` behave as today
- [ ] `self-test` cargo feature default-on in the new shell crate
- [ ] Single instance per profile via CEF `root_cache_path` + `OnAlreadyRunningAppRelaunch` (replacing GApplication forwarding)
- [ ] `vsesvit_update::Installation::detect()` finds the marker in every format (incl. Flatpak with a zypak wrapper)
- [ ] `Updates` created only for self-updating installs with a valid key; otherwise one info log and no UI
- [ ] First update check 30 s after the first window, then every 24 h, only with `updates.automatic`
- [ ] Update lifecycle state machine ported exactly (states, events, effects, channel-change rechecks, one worker)
- [ ] Update status wording and buttons per §7.3 table
- [ ] Update banner at the top of every window (above tabs, bookmarks and find bars) with the `app.update` action
- [ ] Toast with the capitalized error on a failed foreground install
- [ ] Settings > General > Updates: status row (`Version X` subtitle, Check for Updates / banner button), Update Channel (Stable/Beta/Weekly/Nightly + subtitle), Automatic Updates (+ subtitle)
- [ ] Prefs `updates.automatic` (local, default true) and `updates.channel` (local, default `UpdateChannel::of_build`)
- [ ] Download dir `${XDG_CACHE_HOME:-~/.cache}/vsesvit/updates`; `remove_stale_downloads` before each download
- [ ] AppImage updates install in the background ("will open next time" / "Restart now")
- [ ] deb, rpm and pacman updates wait for "Update", run pkexec, then "Restart to finish updating" / "Restart"
- [ ] Restart: close every window and dialog, shut CEF down (release the singleton and profile lock), spawn `program --profile-dir <root>`; `program` = `$APPIMAGE` for AppImage
- [ ] Headless `--check-for-updates`/`--update` JSON output, `of_build` channel, stderr progress
- [ ] deb: control fields, new Depends/Recommends, description, setuid `chrome-sandbox` or AppArmor userns profile
- [ ] rpm: explicit Requires (AutoReqProv off), `%attr(4755,root,root)` for `chrome-sandbox`, `_rpmformat 4`, description
- [ ] pacman: depends/optdepends, setuid mode preserved, `.PKGINFO`/`.MTREE` as today
- [ ] Package versions with `~` (deb/rpm) and glued prerelease (pacman); artifact names unchanged
- [ ] `libcef.so` stripped during staging
- [ ] AppImage: CEF file set bundled; WebKit helpers, path patch, `usr/wk`, GStreamer, GIO, pixbuf, schemas and Adwaita removed
- [ ] AppImage sysroot/excludelist reconciled for CEF sonames (NSS + dlopened NSS modules, atk/at-spi, dbus, cups, X libs, xkbcommon, cairo/pango, udev, glib)
- [ ] AppRun: no `cd`, no GTK/GStreamer env; relative paths still resolve against the user's directory; sandbox-or-warn with `--no-sandbox` fallback
- [ ] AppImage pinned type2 runtime, mksquashfs zstd-19, root desktop file and icon kept
- [ ] Flatpak: new runtime/base app, zypak (or documented `--no-sandbox`), offline CEF source + `CEF_PATH`, CEF files in `/app/lib/vsesvit`, marker, symlink/wrapper, finish-args reviewed (secrets, notifications)
- [ ] `xtask` cargo-sources generator handles every source in `Cargo.lock` (git if `gpui` comes from git)
- [ ] verify.sh rewritten: `--version`, marker, desktop-file-validate, self-test from the AppImage, page load, relative profile dir, CEF child processes, sandbox check, Vulkan software ICD in containers; gst-inspect checks removed
- [ ] Signing, `signer generate`, `manifest` (latest.json keys, pubkey verification) unchanged
- [ ] release.yml linux job: apt list without GTK/WebKit, plus GPUI dev packages; CEF download cached or pinned; disk budget checked
- [ ] release.yml flatpak job builds the new manifest; nightly still skips Flatpak
- [ ] `xtask/tests/release_workflow.rs` invariants hold (key only in the no-cargo sign job, actions pinned, permissions)
- [ ] `scripts/release-plan.sh`, `scripts/set-version.sh` unchanged and working with the new crate graph
- [ ] `scripts/wsl.sh` still builds and runs the Linux shell (CEF_PATH guidance, Vulkan in WSLg)
- [ ] Workspace `Cargo.toml`: GTK/WebKit deps and `vsesvit-gtk`/`vsesvit-webext` removed; `cef`/`gpui` added; `crates/vsesvit` points to the new shell
- [ ] `.gitattributes` LF rules cover every new Linux packaging file; CRLF test passes
- [ ] README rewritten (toolkit/engine sentence, Linux requirements, build deps, layout, media caveat)
- [ ] `docs/design/packaging.md` rewritten (app id wording, dependency table, sandbox, CEF layout, AppImage/Flatpak paragraphs)
- [ ] `docs/PLAN.md` Linux lines updated per §18, plus a new decision record replacing "Linux uses WebKitGTK, not Chromium"
- [ ] `docs/design/grounding.md` CEF-rejection statement and Linux environment updated
- [ ] H.264/AAC playback decision recorded (CEF build with proprietary codecs or accepted regression)
