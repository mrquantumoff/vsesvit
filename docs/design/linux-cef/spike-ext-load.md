# spike-ext-load: installing and managing user extensions in CEF 154

Date: 2026-10-23. Spike code: `spikes/ext-load` (README there says how to run it).
Versions: `cef` 154.5.0+154.0.34 (CEF 154.0.34, Chromium 154.0.8037.98), `gpui-pre` 0.3.9, rustc 1.99.1, Ubuntu 26.04 under WSL2, Xvfb 21.1.22 (no window manager), lavapipe.
The earlier run (2026-10-16) used Fedora 44 with the same crates. Its findings are marked "earlier run".
Source references are Chromium tag `154.0.8037.98`, fetched from chromium.googlesource.com.

Labels: **VERIFIED** = run, command and output given. **NOT VERIFIED** = run, and the thing does not work. **INCONCLUSIVE** = run, but the setup cannot settle it. **INFERRED** = my conclusion, not run.

---

## 0. Summary

**Recommended install path: External Extensions JSON pointing at a CRX that core has verified.**
The file is `<root_cache_path>/External Extensions/<id>.json` with `{"external_crx": "<path>", "external_version": "<v>"}`.
Chromium installs it silently on Linux as a packed extension (location `kExternalPref`, 2).
It persists, it survives the incognito toggle and every other reload, and Chromium's own updater updates it.
The shell manages it through CDP over `--remote-debugging-pipe` and a small helper extension.
Uninstall is `chrome.management.uninstallSelf({showConfirmDialog: false})` evaluated inside the extension through CDP. No dialog appears.

`--load-extension` is the wrong path for user installs. Any reload turns its extension into a "Load unpacked" install, which Chromium disables unless developer mode is on.
CDP `Extensions.loadUnpacked` installs last one session, and a reload makes them permanent unpacked installs with the same problem.

| # | Item | Result |
|---|---|---|
| 1 | `--load-extension` installs are location 8 (command line) | VERIFIED (earlier run; again here, §3) |
| 2 | Their `chrome.storage` survives runs without the extension, also past the 30 s GC delay | VERIFIED (earlier run; `q1-restart.sh` again here, §2) |
| 3 | Enhanced Safe Browsing on blocks `--load-extension`; off again, the extensions load with their storage | VERIFIED (`q1-esb.sh`, §2) |
| 4 | External Extensions JSON installs happen without a prompt | VERIFIED (earlier run; again here, §6) |
| 5 | `management.uninstall` of another extension always shows Chrome's "Remove …?" dialog, even with `showConfirmDialog: false` | VERIFIED (earlier run; screenshot here, §7) |
| 6 | Incognito toggle on a command-line extension leaves it disabled with `DISABLE_UNSUPPORTED_DEVELOPER_EXTENSION` across restarts: root cause | VERIFIED (§3): the toggle reloads the extension; the reload goes through `UnpackedInstaller::Load` and makes it location 4 (unpacked); unpacked needs developer mode |
| 7 | Same for `chrome.runtime.reload()` from a command-line extension | VERIFIED (§3) |
| 8 | Same toggle on an External JSON (packed) extension | VERIFIED: stays enabled, location 2, works in an off-the-record context (§3) |
| 9 | Same toggle on a CDP-loaded extension | VERIFIED: disabled the same way, and it loses its CDP flag, so it stays installed after restart (§3) |
| 10 | Developer mode on before the toggle | VERIFIED: stays enabled, but is now a persistent unpacked install that loads without the flag (§3) |
| 11 | CDP over `--remote-debugging-pipe`: `Browser.getVersion`, `Target.*`, `Extensions.getExtensions`, `loadUnpacked`, `uninstall` | VERIFIED (§4). `uninstall` refuses anything but location 4 |
| 12 | CDP storage methods (get/set/remove/clear) | VERIFIED from the extension's own worker or page session, and from a page session where its content script runs. NOT VERIFIED from the browser session: "No associated browser context." (§5) |
| 13 | CDP storage after the worker stopped | VERIFIED through a hidden `chrome-extension://<id>/manifest.json` tab (§5) |
| 14 | `CefBrowserHost::ExecuteDevToolsMethod` (page session) | VERIFIED for storage of an extension injected into that page; `Extensions.loadUnpacked`/`getExtensions` "Method not available.", `Target.attachToBrowserTarget` "Not allowed" (§5) |
| 15 | Chromium's updater with a manifest `update_url` | VERIFIED: checks 3.4 s after start, downloads the CRX, applies it when the worker goes idle (about 32 s later) or at the next start; `runtime.requestUpdateCheck()` works (§6) |
| 16 | External JSON with `external_update_url` only | VERIFIED: downloads and installs from the update URL (location 6) (§6) |
| 17 | External JSON `external_version` bump | VERIFIED: picked up at the next start, applied when the worker idles or at the start after that; a JSON change while running is not picked up (§6) |
| 18 | Uninstall without a dialog | VERIFIED: `uninstallSelf` through CDP, no new X window, storage deleted, works for packed and command-line installs, also for a disabled extension after re-enabling it (§7) |
| 19 | Uninstall by deleting the JSON file | VERIFIED: removed at the next start with its storage and files (§7) |
| 20 | Command-line extensions see `onInstalled` reason `install` on every start, never `update` | VERIFIED (`q1-restart.sh`, §2) |
| 21 | Shortcut and site-access changes through `developerPrivate` persist without a reload | VERIFIED (`q3-devprivate.sh`, §8.1) |
| 22 | Install while the browser runs, without a restart, through a stock persistent path | NOT VERIFIED: the JSON is read only at start (§6.2) |

---

## 1. How the tests were run

- Private Xvfb `:92` (`start-xvfb.sh`). No window manager. The Windows desktop and WSLg's `:0` were never used.
- `run.sh` starts the browser under `env -i` with a scratch `HOME`, a private `dbus-run-session` and lavapipe.
- WSL has no Xvfb package installed and no root. Xvfb, `x11-utils` and ImageMagick were unpacked from their `.deb` files into the scratch root. WSLg mounts `/tmp/.X11-unix` read-only, so Xvfb listens on the abstract socket only. X clients connect fine.
- The private D-Bus session activated `xdg-desktop-portal-gtk`. It opened its window on `:92` (seen in `xwininfo -root -children`), not on the desktop.
- `fixtures.sh` rebuilds every test extension. The earlier run's sources and keys were not in the repo, so the Reporter and Agent extensions were rewritten (`exts-src/`). The keys are new throwaway RSA-2048 keys, so the ids differ from the earlier run: A `chnbjc…`, B `hlfakc…`, C `jpkgpd…`, D `pchbjp…`, Agent `einjei…`, probe `eonajg…`.
  - A and B: Reporter directories for `--load-extension` or `loadUnpacked`.
  - C: Reporter CRX 1.0 and 2.0 for External JSON.
  - D: Reporter CRX 1.0 and 2.0 with `update_url: http://127.0.0.1:8792/updates/D.xml`.
- The Reporter posts `sw-start` (with the storage it found), `onInstalled`, `content` and `requestUpdateCheck` to the spike's report server. Its content script marks the page `data-rep-<name>="<version>:<visits>"`.
- CDP runs in process over fds 3/4 (`src/cdp.rs`, `SPIKE_CDP_PIPE=1`). `ctl cdp` uses the browser session. `swcdp`/`pagecdp` attach to a worker or page target first.
- Every script sources `env.sh`. Paths come from `VSESVIT_SCRATCH`, `VSESVIT_REPO`, `CARGO_TARGET_DIR` and `SPIKE_DISPLAY`.

Build: `CEF_PATH=$VSESVIT_SCRATCH/cef cargo build`. The cef crate downloads CEF 154.0.34 into `CEF_PATH` when it is missing. WSL has no `-dev` packages, so the link names `libxkbcommon-x11.so` and `libxcb-xkb.so` were symlinked into a scratch directory passed as `RUSTFLAGS="-L native=..."`. `tools/mkcrx` now finds vsesvit-core by a relative path and is built from the checkout.

---

## 2. Command-line installs across restarts (VERIFIED)

`q1-restart.sh` (A, B, probe P on the command line; A bumped, B dropped and re-added, A downgraded):

```
=== phase r3-bumpA: --load-extension=A/2.0,B/1.0,P/1.0.0
{"ext":"A","version":"2.0","event":"onInstalled","reason":"install","previousVersion":null}
{"ext":"A","version":"2.0","event":"sw-start","localBefore":{"firstVersion":"1.0","ran-1.0":true,"ran-2.0":true,"starts":6,"visits":6}}
=== phase r4-dropB: --load-extension=A/2.0,P/1.0.0
=== phase r5-addB: --load-extension=A/2.0,B/1.0,P/1.0.0
{"ext":"B","version":"1.0","event":"onInstalled","reason":"install","previousVersion":null}
{"ext":"B","version":"1.0","event":"sw-start","localBefore":{"firstVersion":"1.0","ran-1.0":true,"starts":6,"visits":6}}
Local Extension Settings after every phase: chnbjc… eonajg… hlfakc…
prefs A path follows the flag: …/1.0, …/2.0, …/2.0, …/2.0, …/1.0
```

B's storage stays while B is left out. A's prefs record follows whatever version the flag names.
A command-line extension gets `onInstalled` with reason `install` on every start, also after a version bump (never `update`, `previousVersion` null). Extensions that open a welcome page on install would do it at every start.

`q1-esb.sh`:

```
run1 reports: 6        pref safebrowsing.enhanced = false -> setpref true -> Ok(())
run2 (ESB on) reports: 0      repA=undefined
run3 (ESB off again) reports: 6
  {"ext":"B","localBefore":{"firstVersion":"1.0","ran-1.0":true,"starts":1,"visits":1}}
  {"ext":"A","localBefore":{"firstVersion":"1.0","ran-1.0":true,"starts":1,"visits":1}}
```

With Enhanced Safe Browsing on, no command-line extension loads. The user can turn that pref on.

`q2-agent.sh`: a disable through the Agent's `management.setEnabled(A, false)` is kept across restarts even for a command-line extension (`dr:[1]`, `DISABLE_USER_ACTION`). `management.uninstall` without a user gesture fails with "chrome.management.uninstall requires a user gesture."

---

## 3. Why the incognito toggle disables a command-line extension (VERIFIED)

### 3.1 Source

1. `developerPrivate.updateExtensionConfiguration({incognitoAccess})` calls `util::SetIsIncognitoEnabled`. That calls `ReloadExtensionIfEnabled` (`chrome/browser/extensions/extension_util.cc:190-255, 83-93`).
2. The reload ends in `ChromeExtensionRegistrarDelegate::DoLoadExtensionForReload` (`chrome_extension_registrar_delegate.cc:264-294`). It re-loads from the prefs record only if the record has a stored manifest. Command-line and unpacked records have none. Otherwise it runs `UnpackedInstaller::Load(path)`.
3. `UnpackedInstaller::Load` installs as `ManifestLocation::kUnpacked` (`extensions/browser/unpacked_installer.cc:409`). Only `LoadFromCommandLine` uses `kCommandLine` (line 171).
4. `ExtensionManagement::IsAllowedByUnpackedDeveloperModePolicy` allows `kUnpacked` only with developer mode on, unless the extension has `INSTALLED_VIA_CDP` (`extension_management.cc:375-395`). Otherwise `StandardManagementPolicyProvider` returns `DISABLE_UNSUPPORTED_DEVELOPER_EXTENSION` (`standard_management_policy_provider.cc:229-234`).
5. The prefs record is now a persistent unpacked install with that disable reason. The next start loads it from prefs, disabled.

The same applies to every reload: `chrome.runtime.reload()`, "Allow access to file URLs", the reload button in `chrome://extensions`.

### 3.2 Measurements (`q4-reload.sh`)

Run 1: A on the command line, C from External JSON, B through CDP `loadUnpacked`. Incognito toggled on all three from `chrome://extensions`:

```
devp info: ["jpkgpd","UNKNOWN","ENABLED","incog=true", unsupportedDeveloperExtension:false]
           ["chnbjc","UNPACKED","DISABLED","incog=true", unsupportedDeveloperExtension:true]
           ["hlfakc","UNPACKED","DISABLED", ... unsupportedDeveloperExtension:true]
page: repA=undefined repB=undefined repC=1.0:2
otr page: repC=1.0:3:incognito
prefs: chnbjc loc 4 dr [16777216] flags 38 manifest:false
       hlfakc loc 4 dr [16777216] flags 38 manifest:false
       jpkgpd loc 2 dr null      flags 1  manifest:true
```

`16777216` is `1 << 24`, `DISABLE_UNSUPPORTED_DEVELOPER_EXTENSION`. B lost `INSTALLED_VIA_CDP` in the reload (flags 38 = `REQUIRE_MODERN_MANIFEST_VERSION | ALLOW_FILE_ACCESS | FOLLOW_SYMLINKS_ANYWHERE`). The packed C stayed enabled and ran in the off-the-record context.

Run 2, restart with the same `--load-extension`: A starts `UNPACKED DISABLED`. `chrome.management.setEnabled(A, true)` from `chrome://extensions` re-enables it and the record goes back to location 8. B stays installed and disabled although nothing loads it.

Run 3, restart without `--load-extension`: A is gone ("Failed to find extension"). B is still there, disabled. With `extensions.ui.developer_mode` set to `true`, `setEnabled(B, true)` works and B runs (`repB=1.0:1`).

Run 4, fresh profile, developer mode on first, then the toggle: A stays `ENABLED` and runs in off the record (`repA=1.0:3:incognito`). Its record is location 4. Run 4b, restart without the flag: A still loads (`repA=1.0:5`). The toggle turned a session install into a permanent one.

Run 5, fresh profile, A calls `chrome.runtime.reload()`: A is `UNPACKED DISABLED` with `unsupportedDeveloperExtension: true`, prefs `loc 4 dr [16777216]`.

### 3.3 What this means for the install path

- Command-line and CDP installs break on any reload. Turning developer mode on hides the problem but makes them "Load unpacked" installs. The user then sees developer-mode UI, and every extension gets the unpacked semantics.
- A packed install (External JSON) reloads from its stored manifest and keeps location 2. That is the only stock path where enable, disable, incognito and reload behave as in Chrome.

---

## 4. CDP `Extensions.*` over `--remote-debugging-pipe` (VERIFIED)

`q2-cdp.sh`, `q5-cdp-storage.sh`. The pipe session is a browser-target session, so the extension-management methods are allowed.

| Method | Result |
|---|---|
| `Browser.getVersion` | `Chrome/154.0.8037.98`, protocol 1.3 |
| `Target.getTargets`, `attachToTarget` (flatten), `Runtime.evaluate` in a worker | work |
| `Extensions.getExtensions` | lists command-line and unpacked extensions only (not External JSON ones) |
| `Extensions.loadUnpacked {path}` | works; `onInstalled install`. Over a command-line A 1.0 with A 2.0: `onInstalled update`, storage kept |
| `Extensions.loadUnpacked {path, enableInIncognito: true}` | works |
| CDP install at the next start | gone from `getExtensions`; its `Local Extension Settings` stayed (`localBefore {"firstVersion":"1.0","ran-1.0":true,"starts":1}` when loaded again) |
| `Extensions.uninstall` on a CDP-loaded extension | `OK {}`; no dialog (X window count 4 before and after); storage deleted |
| `Extensions.uninstall` on command-line A or External C | `Uninstall failed. Reason: extension is not an unpacked extension.` |
| `Extensions.triggerAction` | crashes the browser (see spike-ext-api.md §6) |

---

## 5. Storage through CDP

From the browser session every storage call fails:

```
Extensions.getStorageItems {"id":A,"storageArea":"local"} => ERR {"code":-32600,"message":"No associated browser context."}
```

The handler wants the target's own `DevToolsAgentHost`. It allows a worker of that extension, or a page or frame whose renderer hosts the extension or its content script (`extensions_handler.cc:60-110`). The browser target has no browser context.

From A's worker session (`swcdp`):

```
getStorageItems local   => {"data":{"firstVersion":"1.0","ran-1.0":true,"starts":1,"visits":1}}
setStorageItems local {cdpSet:42} => {}
getStorageItems sync    => {"data":{"syncStarts":1}}
getStorageItems session => {"data":{"sessionStarts":1}}
setStorageItems local {k1, k2:{deep:[1,2]}, k3}; removeStorageItems [k1]; getStorageItems keys [k1,k2,k3]
                        => {"data":{"k2":{"deep":[1,2]},"k3":"three"}}
clearStorageItems session; getStorageItems session => {"data":{}}
getStorageItems managed => {"data":{}};  setStorageItems managed => "This is a read-only store."
getStorageItems for B from A's worker => "Extension not found."
External C from its own worker: get and set work.
```

From the page session of a tab where A's content script runs (`pagecdp http://127.0.0.1`): A's storage works.

After 40 s idle the worker is gone (`targets` lists only the page) and `swcdp` finds no target. Opening `chrome-extension://<A>/manifest.json` in a tab gives a page target. Through it, get and set work again (`{"data":{"firstVersion":"1.0","k2":…}}`; `set sync {"fromHiddenPage":1}` OK).

Through `CefBrowserHost::ExecuteDevToolsMethod` on the tab (`ctl bdt`, page session): storage get and set for A work, because A's content script runs there. `Target.getTargets` works. `Target.attachToBrowserTarget` gives "Not allowed". `Extensions.getExtensions` and `loadUnpacked` give "Method not available."

Usable for sync and export: yes, per extension. The shell attaches to the extension's worker, or opens a hidden extension page when the worker is stopped. `sync` is plain local storage here (no Chrome Sync), so Vsesvit's sync can read and write it this way. It cannot read storage of a disabled extension (the handler looks only at enabled extensions).

---

## 6. Updates

### 6.1 `update_url` in the manifest (`q6-update.sh`, VERIFIED)

D 1.0 installed from External JSON. The report server logs the update checks:

```
run 1 (D.xml offers 1.0)
  0.472 onInstalled install 1.0
  3.423 GET /updates/D.xml?os=linux&arch=x64&prod=chromiumcrx&prodversion=154.0.8037.98&acceptformat=crx3,puff&x=id%3Dpchbjp…%26v%3D1.0%26installsource%3Dnotfromwebstore%26installedby%3Dexternal…
  (D.xml now offers 2.0; D calls chrome.runtime.requestUpdateCheck())
 17.183 GET /updates/D.xml?…v%3D1.0…
 17.189 GET /updates/repD-2.0.crx
 17.191 requestUpdateCheck => {"status":"update_available","version":"2.0"}
  quit after 10 s: prefs D ver 1.0
run 2 (restart; JSON still external_version 1.0)
  0.398 onInstalled update 2.0, previousVersion 1.0;  prefs D ver 2.0, loc 2
run 3 (fresh profile, D.xml offers 2.0 from the start, no poke)
  3.407 GET /updates/D.xml   3.413 GET /updates/repD-2.0.crx
 35.786 onInstalled update 2.0, previousVersion 1.0
run 4 (fresh profile, JSON {"external_update_url": "http://127.0.0.1:8792/updates/D.xml"})
  0.439 GET /updates/D.xml?…v%3D0.0.0.0…   0.447 GET /updates/repD-1.0.crx
  3.611 onInstalled install 1.0;  prefs D loc 6 (kExternalPrefDownload)
```

- `ExtensionUpdater` runs in CEF. It checks soon after start (`ExtensionUpdater::Start` calls `CheckSoon`) and then every 5 hours (`kDefaultUpdateFrequency`).
- Plain `http://` update URLs work.
- A downloaded update waits until the extension is idle. With the worker running it landed about 32 s after the download, which matches the 30 s worker idle timeout. On quit it landed at the next start.
- A lower `external_version` in the JSON does not downgrade an updated extension (run 2).

### 6.2 `external_version` bump (`q2-external-update.sh`, `q2-external.sh`, VERIFIED)

```
q2-external-update run 2 (JSON says 2.0 before the start, page open)
   0.400 sw-start 1.0
  35.695 onInstalled update 2.0, previousVersion 1.0, storage kept
q2-external run 2: JSON changed to 2.0 while running, 20 s: still 1.0
q2-external run 3: restart: still running 1.0 after 5 s; Extensions/<C>/2.0_0 already unpacked
```

The JSON is read at start only. The new version is staged at that start and applied when the worker idles.

### 6.3 What the shell has to do

- For extensions with an `update_url` (all Web Store extensions), nothing: Chromium checks, downloads, verifies the CRX against the installed key and applies the update.
- For extensions core updates itself, write the new CRX path and version into the JSON. It applies at the next start plus the idle delay.
- Core should not run a second updater for Web Store extensions. It can read the installed version from prefs, from `management.getAll` in the helper extension, or from `onInstalled` reports.

---

## 7. Uninstall without Chrome's dialog (`q7-uninstall.sh`, VERIFIED)

`ManagementUninstallFunctionBase::Uninstall` forces the dialog whenever the caller is not the target: `show_confirm_dialog |= !self_uninstall` (`extensions/browser/api/management/management_api.cc:739-741`). WebUI callers have no extension, so `chrome://extensions` gets the dialog too. Only `uninstallSelf` can skip it.

Run 1 (External C, command-line A and B, Agent):

```
LES: chnbjc… hlfakc… jpkgpd…          X windows: 10
sweval C "chrome.management.uninstallSelf({showConfirmDialog: false})"   X windows 10
  agent: onDisabled C, onUninstalled C         LES: chnbjc… hlfakc…
sweval A "…uninstallSelf…"                                              X windows 10
  agent: onDisabled A, onUninstalled A         LES: hlfakc…
agent page: management.uninstall(B, {showConfirmDialog: false}) with a CDP user gesture
                                                                        X windows 12
prefs: external_uninstalls: ["jpkgpd…"]
```

The last step shows Chrome's tab-modal dialog over the child browser (`shots/agent-uninstall-B.png`: "Remove "Vsesvit Reporter B"? "Vsesvit Agent" would like to remove this extension." with Cancel and Remove). The `uninstallSelf` screenshots show no dialog. The worker dies during the call, so the CDP evaluate never resolves.

Run 2, restart with the same JSON and flags: C does not come back (`external_uninstalls` blocks it). A comes back with fresh storage (`repA=1.0:1`), because the flag still names it.

Run 3, fresh profile, C from JSON, then the JSON file deleted before a restart: C is gone, `Local Extension Settings` is empty, `Extensions/` holds only `Temp`. `external_uninstalls` stays `null`, so a new JSON would install it again.

`q2-external.sh` run 4 and 5 show the same and the way back: with the JSON moved away, C and its storage are gone at the next start. With the JSON back (2.0), C installs fresh (`onInstalled install`, empty `localBefore`).

Run 4, C disabled by the Agent, then enabled again (no dialog), worker idle for 40 s. Opening `chrome-extension://<C>/manifest.json` gives a context with `chrome.management.uninstallSelf`. Calling it there: X windows 12 before and after, `onUninstalled C`, storage deleted, `external_uninstalls ["jpkgpd…"]`.

---

## 8. Recommended install and management path

| Operation | How |
|---|---|
| Install | core verifies the CRX, stores it under the profile, writes `External Extensions/<id>.json` (`external_crx`, `external_version`). Installs at the next start, silently. For an install while running, see the must-do list |
| Update | Chromium's updater for extensions with an `update_url`; JSON version bump for core-managed ones |
| Enable / disable | `chrome.management.setEnabled` in the helper extension, or `developerPrivate` from a `chrome://extensions` page. No dialog |
| Incognito, file access, site access, shortcuts | `developerPrivate.updateExtensionConfiguration` / `updateExtensionCommand` from a `chrome://extensions` page (`q3-devprivate.sh`, §8.1). Safe for packed installs |
| Uninstall | CDP `Runtime.evaluate` of `chrome.management.uninstallSelf({showConfirmDialog: false})` in the extension's worker or a hidden extension page (enable it first if disabled), then delete the JSON and the CRX |
| Storage for sync and export | CDP `Extensions.get/set/remove/clearStorageItems` on the extension's own session |

### 8.1 `developerPrivate` settings (`q3-devprivate.sh`, VERIFIED)

From a `chrome://extensions` tab, all calls return `ok null`:

```
updateExtensionCommand(A, rep-cmd, Alt+Shift+U); updateExtensionCommand(B, rep-cmd, Ctrl+Shift+9)
updateExtensionConfiguration(B, hostAccess: ON_CLICK)  -> B hosts [["http://127.0.0.1/*", false]], content script withheld (repB=undefined)
updateExtensionConfiguration(A, incognitoAccess: true) -> A DISABLED (the §3 reload problem)
prefs extensions.commands: {"linux:Alt+Shift+U":{rep-cmd, A}, "linux:Ctrl+Shift+9":{rep-cmd, B}}
run 2 (restart): B still ON_CLICK with Ctrl+Shift+9; A still DISABLED
```

Shortcut and site-access changes persist and need no reload. One quit in this script did not finish within 20 s and the spike was killed (`timeout waiting for exit_code=`). It did not recur in the other 30-odd starts.

---

## 9. What the real crate must do

1. Install user extensions through `<root_cache_path>/External Extensions/<id>.json` with `external_crx`. Never through `--load-extension` or CDP `loadUnpacked`.
2. Keep `--load-extension` only for Vsesvit's own helper extension, which never reloads, and keep developer mode off. Its `onInstalled` fires with `install` on every start.
3. Start Chromium with `--remote-debugging-pipe` and own fds 3/4 before `cef::initialize` (as `src/cdp.rs`). Never open a TCP debugging port.
4. Uninstall by evaluating `chrome.management.uninstallSelf({showConfirmDialog: false})` inside the extension over CDP. Re-enable a disabled extension first. Open a hidden `chrome-extension://<id>/manifest.json` tab when its worker is not running. Then delete the JSON file and the CRX.
5. Do not expect the uninstall call to resolve. Confirm through `management.onUninstalled` in the helper or the prefs.
6. Remember that `external_uninstalls` blocks a later JSON install of the same id. A user who reinstalls a removed extension needs that entry cleared (prefs edit before start, INFERRED) or a different path.
7. Leave Chromium's updater on. Do not let core fight it for Web Store extensions. Expect an update to land up to about 30 s after download, or at the next start.
8. Read and write extension storage for sync and export through the extension's own CDP session. The browser session cannot do it.
9. Install while running: there is no stock way to make Chromium re-read the JSON without a restart (§6.2). Either restart the browser after an install or accept the next-start install. A CEF patch or `Extensions.loadUnpacked` for the session would be the alternatives, and the second has the reload problem.
10. Watch Enhanced Safe Browsing: it blocks command-line extensions, so the helper extension must not depend on `--load-extension` if the user may turn it on (INFERRED alternative: ship the helper as a packed External JSON install too).
11. Show `chrome://extensions` only as an internal management page. Its "Remove" goes through the dialog.
