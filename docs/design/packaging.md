# Packaging and updates

Vsesvit ships as an NSIS installer on Windows and as a deb, an rpm, a pacman package (`.pkg.tar.zst`), an AppImage and a Flatpak on Linux. Every format except Flatpak updates itself through the Tauri updater protocol, so the same update server that serves Tauri apps serves Vsesvit. Flatpak installs update through Flatpak.

The app id is `dev.mrquantumoff.vsesvit` on every surface: the GTK application id, the desktop file, the AppStream id, the Flatpak id and the icon name.

## The update protocol

The client speaks the protocol of `tauri-plugin-updater` 2.12, read from its source.

1. For each endpoint in order, substitute `{{current_version}}`, `{{target}}`, `{{arch}}` and `{{bundle_type}}` (also their percent-encoded forms). `target` is `windows` or `linux`. `arch` is `x86_64`, `aarch64`, `i686`, `armv7` or `riscv64`. `bundle_type` is the installation's variant (`nsis`, `deb`, `rpm`, `pacman`, `appimage`).
2. `GET` it with `Accept: application/json` and `User-Agent: vsesvit/<version>`. `204` or `404` means no update (the Quadrant server answers `404` when it has no release for the target). Other non-success statuses and network errors move on to the next endpoint. A parsed response stops the loop.
3. The body is either the dynamic format `{version, notes?, pub_date?, url, signature}` or the static format `{version, notes?, pub_date?, platforms: {<key>: {url, signature}}}`. `version` may carry a leading `v`, and `name` is accepted as an alias. Unknown fields are ignored, because the Quadrant server returns its whole database row.
4. In the static format, look up `<target>-<arch>-<variant>`, then `<target>-<arch>`.
5. Offer the update when the remote version is greater than the running version (semver) and its channel (the first field of its prerelease, or stable without one) is at least as steady as the one asked for: stable, then beta, weekly and nightly. The signature binds the version but not the channel, so without this an endpoint answering for stable could hand out a nightly build.
6. Download, then verify before anything touches the disk outside the download directory. `signature` is base64 of a minisign signature file, and the public key is base64 of a minisign public key file. Verify with `minisign-verify` (prehashed allowed). Then read `version:` from the trusted comment and require it to equal the announced version. Tauri makes this check opt-in because old Tauri CLIs did not write it. Our signer always writes it, so we require it. Installing reads the download back and verifies it again, since anything running as the user can change it while it waits for the user to click "Update".
7. Check the artifact's magic bytes against the installation's format before installing.

Installing, per format:

| Format | How the update is applied | When |
|---|---|---|
| NSIS | Launch the new installer with `/P /UPDATE /R /ARGS <args>` (Tauri's passive mode), then the browser exits. The installer waits for the old process, replaces the files and relaunches. | User clicks "Restart to update", or the browser quits with an update ready (then `/S /UPDATE`, no relaunch). |
| deb | `pkexec apt-get install -y --no-install-recommends <absolute path of file>`. Not `dpkg -i`, which unpacks a package whose new dependencies are missing and leaves it unconfigured, so apt refuses to run until it is repaired. | User clicks "Update", which asks for the admin password. |
| rpm | `pkexec rpm -U <file>` | same |
| pacman | `pkexec pacman -U --noconfirm <file>` | same |
| AppImage | Write the new image next to `$APPIMAGE`, copy its mode, `rename` over the old one. | In the background once verified. It takes effect on the next launch. |
| Flatpak | Not handled by Vsesvit. | Flatpak updates it. |

## The installation marker

Each package puts a one-line file named `package-format` next to the real executable. It holds the variant string (`nsis`, `deb`, `rpm`, `pacman`, `appimage`, `flatpak`). On Linux the real executable lives at `<prefix>/lib/vsesvit/vsesvit` and `<prefix>/bin/vsesvit` is a symlink to it, so `current_exe()` resolves next to the marker. A build without the marker (a `cargo run`) is unpackaged and never updates itself.

```rust
pub enum Installation {
    Unpackaged,
    Nsis,
    Deb,
    Rpm,
    Pacman,
    AppImage { image: PathBuf }, // from $APPIMAGE; a marker saying appimage without $APPIMAGE is Unpackaged
    Flatpak,
}
```

## Configuration

`packaging/updater.json` uses the field names of `plugins.updater` in `tauri.conf.json`, so a Tauri app's block pastes in unchanged:

```json
{ "pubkey": "<base64 minisign public key>", "endpoints": ["https://.../{{target}}/{{arch}}/{{current_version}}?variant={{bundle_type}}"], "windows": { "installMode": "passive" } }
```

It is compiled in. `VSESVIT_UPDATER_PUBKEY` and `VSESVIT_UPDATER_ENDPOINTS` (comma separated) set at build time override it, which is how the end-to-end test builds a copy that trusts a test key and a local server. An empty `pubkey` turns the updater off.

## Building packages

`cargo xtask` builds everything into `target/dist/`:

```
cargo xtask package <nsis|deb|rpm|pacman|appimage|flatpak>... [--sign]
cargo xtask manifest --base-url URL [--notes FILE]   # writes target/dist/latest.json
cargo xtask sign FILE...
```

Signing reads `TAURI_SIGNING_PRIVATE_KEY` (the key text or a path to it) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, the variables the Tauri CLI reads, so a Tauri project's key and CI secrets work as they are. It writes `<file>.sig` with the trusted comment `timestamp:<unix>\tfile:<name>\tversion:<version>`, the format `tauri signer sign` writes.

Artifact names follow the Tauri bundler, so release tooling that expects them keeps working:

| Format | File | `latest.json` keys |
|---|---|---|
| NSIS | `Vsesvit_<v>_x64-setup.exe` | `windows-x86_64-nsis`, `windows-x86_64` |
| deb | `Vsesvit_<v>_amd64.deb` | `linux-x86_64-deb` |
| rpm | `Vsesvit-<v>-1.x86_64.rpm` | `linux-x86_64-rpm` |
| pacman | `vsesvit-<v>-1-x86_64.pkg.tar.zst` | `linux-x86_64-pacman` |
| AppImage | `Vsesvit_<v>_amd64.AppImage` | `linux-x86_64-appimage`, `linux-x86_64` |
| Flatpak | `Vsesvit_<v>_x86_64.flatpak` | none |

## Runtime dependencies

Media playback plugins are hard dependencies, so video works out of the box. Fedora is the exception: its `gstreamer1-plugin-libav` is built on ffmpeg-free and has no H.264 or HEVC decoder, so H.264 there needs RPM Fusion's ffmpeg or `gstreamer1-plugin-openh264`.

| | deb (Debian, Ubuntu) | rpm (Fedora) | pacman (Arch) |
|---|---|---|---|
| GTK 4 | `libgtk-4-1` | `gtk4` | `gtk4` |
| libadwaita | `libadwaita-1-0` | `libadwaita` | `libadwaita` |
| WebKitGTK 6.0 | `libwebkitgtk-6.0-4` | `webkitgtk6.0` | `webkitgtk-6.0` |
| GStreamer good | `gstreamer1.0-plugins-good` | `gstreamer1-plugins-good` | `gst-plugins-good` |
| GStreamer bad | `gstreamer1.0-plugins-bad` | `gstreamer1-plugins-bad-free` | `gst-plugins-bad` |
| GStreamer libav | `gstreamer1.0-libav` | `gstreamer1-plugin-libav` | `gst-libav` |
| pkexec (updates) | recommends `pkexec` | recommends `polkit` | optdepends `polkit` |

The AppImage bundles all of these, from the Ubuntu 26.04 archive, so it needs glibc 2.43 or newer and the host's Mesa. The Flatpak gets them from the GNOME 51 runtime, which declares and installs the `org.freedesktop.Platform.codecs-extra` extension itself.

WebKitGTK finds its helper processes through a compiled-in absolute directory, and distribution builds ignore `WEBKIT_EXEC_PATH`. The AppImage therefore patches that directory in the bundled library to a relative one, `./wk/lib/...` with `usr/wk` linking to `usr`, and starts from `$APPDIR/usr`, as Tauri's AppImages do; the library's other paths, among them `/usr/bin/bwrap` and the host directories the sandbox is built from, stay as they are. WebKit's bubblewrap sandbox stays on: it binds the helper directory at `/wk/...` under its own root, bubblewrap keeps the working directory, and the browser adds `$APPDIR` to the sandbox, so the helpers resolve inside it too. It needs the host's `/usr/bin/bwrap` and the namespaces it creates; where those are missing, as in some containers, `AppRun` turns the sandbox off and says so on stderr. The distribution packages and the Flatpak keep it on as well.

On Windows the installer checks for the Windows App Runtime 2.x (at least 2.5.1, the version `platform.rs` asks for) and installs it when missing. WebView2 ships with Windows 11.

## Releasing

`.github/workflows/release.yml` builds, signs and publishes. `scripts/release-plan.sh` decides what a run is:

| Trigger | Channel | Version | On the update server |
|---|---|---|---|
| push tag `vX.Y.Z` | stable | `X.Y.Z` | waits for a release manager |
| push tag `vX.Y.Z-beta.N` | beta | `X.Y.Z-beta.N` | waits for a release manager |
| daily cron, or "Run workflow" | nightly | `<next>-nightly.<YYYYMMDD>.<run>` | live at once (`publish=true`) |
| Monday cron, or "Run workflow" | weekly | `<next>-weekly.<YYYYMMDD>.<run>` | live at once (`publish=true`) |

`<next>` is the workspace version, or its next patch once `v<workspace>` has been released, because a prerelease sorts below its release. `<YYYYMMDD>` is the day the run was created, so a re-run rebuilds the same tag. Nightly and weekly builds of the same `<next>` compare by channel name, so every weekly sorts above every nightly. The two channels never offer each other's builds, so this only matters to someone who switches channel by hand. A tag must match the workspace version, and its prerelease, if any, must start with `beta`. Scheduled runs skip when the channel's last release is a different tag built from the same commit.

The endpoint in `packaging/updater.json` holds `{{channel}}`, a placeholder Tauri does not have. A build follows the channel it was released on, which is the first field of its version's prerelease (`UpdateChannel::of_build` in vsesvit-core), so a nightly asks for nightlies and a stable build for stable releases. Settings offers the four channels, and the choice is kept per installation in `updates.channel`. The server only offers a version newer than the running one, so moving to a steadier channel takes effect once that channel passes the installed version. deb, rpm and pacman spell a prerelease the way their package manager sorts it below the release (`0.1.1~nightly.20260927.5` for deb and rpm, `0.1.1nightly.20260927.5` for pacman), so installing the stable release over a nightly is an upgrade. The file names keep the semver. Re-running a failed release is safe, because the release job reuses an existing GitHub release and replaces its assets.

For nightly and weekly runs, the plan job tags the run's commit straight away. GitHub refuses `GITHUB_TOKEN` a new tag on a commit whose workflow files differ from main's, so tagging in the release job would fail whenever a push that edits a workflow lands mid-run.

`scripts/set-version.sh` stamps the version into `Cargo.toml` and `Cargo.lock` before every build, so each binary reports exactly the version `latest.json` announces. The Windows job builds the NSIS installer. The Linux job builds deb, rpm, pacman and the AppImage in an `ubuntu:26.04` container. The Flatpak job builds on the runner for stable, beta and weekly releases; nightly releases skip Flatpak. None of these jobs holds the signing key. The sign job signs their updater artifacts with an xtask that the signer job built apart from them, and it runs no cargo, so no crate's build script or proc-macro ever runs where it could read the key. The release job writes `latest.json`, and refuses any artifact whose signature the `packaging/updater.json` key would reject. It then creates the GitHub release, and only then registers the release with the update server, so no client is offered a URL that does not exist yet.

Repository secrets: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` and `VERSION_UPDATE_TRIGGER_TOKEN` (an update server API key with product `updater`, scope `update`). These are the names the Quadrant workflow already uses.

## The Quadrant update server

`GET /api/any/vsesvit/updates/<channel>/<target>/<arch>/<version>?variant=<bundle_type>` returns the newest public row for that product, platform, architecture and variant.

Since quadrant_api 12.5.0, CI publishes a release by uploading `target/dist/latest.json` as the body of `PUT /api/any/vsesvit/add_updates/<channel>/<version>` with the updater API key in `Authorization`. The body's `version` must equal `<version>`. Rows land private and wait for a release manager, unless the request adds `?publish=true`, which nightly and weekly channels use to go live at once. The server refuses `publish=true` on `stable`.
