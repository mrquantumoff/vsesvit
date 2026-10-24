# spike-ext-win (WX.0): extensions on Linux and Windows, per profile

Date: 2026-10-24. Spec: `design.md` §5.10. Spike code: `spikes/ext-win` (own `[workspace]`, README there). Scratch and every run log: `.audit/scratch/ext-win` (Windows, `runs/<label>/<profile>/spike.log`, `items-win-*.txt`), `~/vsesvit-scratch/ext-win` (WSL; item outputs copied to `.audit/scratch/ext-win/items-linux-*.txt`).
Versions: `cef` 154.5.0+154.0.34 (Chromium 154.0.8037.98), `gpui-pre` 0.3.9, Windows 11 Pro 10.0.26300 (every Windows run in the sandboxed bootstrap bundle on the private desktop), Ubuntu 26.04 under WSL2 with Xvfb 21.1.22 (private display `:94`).

Labels: **VERIFIED** = run, command and output given. **NOT VERIFIED** = run, and the thing does not work. **INCONCLUSIVE** = run, but the setup cannot settle it. **INFERRED** = my conclusion, not run.

## Resume notes

- Done. All items ran on both OSes with profiles A and B open together (conflicting sets, §0). Open ends: typing into the Windows popup (§6), Windows toasts (§9, not safely testable here), the rest of outcome B's backend (§13).
- `log.tsv` was not touched (outside this spike's write scope); the outcome line for it is in "Gate outcome".

## Items

| # | Item | Linux (External Extensions JSON) | Windows (per-process HKCU override of the registry route) |
|---|---|---|---|
| 1 | Install route per profile; prompt; pre-acknowledge; off-store CRX on Windows | VERIFIED: silent, no prompt, per profile (§1.1) | The per-user folder and `<libcef dir>` JSON: NOT VERIFIED, Chromium does not compile them on Windows. The registry route: VERIFIED per profile only with `RegOverridePredefKey`; installs arrive disabled (unacknowledged external, no window shown); one `management.setEnabled` from a hidden `chrome://extensions` page acknowledges for good, or `ack_external` written through `SetPreference` beforehand. Off-store CRX accepted. Live install without restart (§1.2) |
| 2 | Persistence, enable/disable, `uninstallSelf` via hidden Alloy page, update + restart, permission-raising update, `external_uninstalls` edit | VERIFIED; the permission-raising re-enable needs a user gesture and opens Chrome's dialog (not answered on Linux) (§2) | VERIFIED, same; Chrome's re-enable dialog answered, extension re-enabled (§2) |
| 3 | Helper as External CRX with Enhanced Safe Browsing on | VERIFIED (§3) | VERIFIED (§3) |
| 4 | Page-session bridge | VERIFIED (§4) | VERIFIED after a `do_close` fix (§4, §5.2) |
| 5 | `developerPrivate` over a hidden Chrome-style `chrome://extensions` page session; hidden browsers in `tabs.query`/`windows.getAll`/`tabs.onCreated` | VERIFIED; Alloy hidden browsers invisible to extensions; the Chrome-style page is a `popup` window while it lives (§5) | Same; a toplevel hidden host is activated by Alloy hidden browsers, a hidden `WS_CHILD` host is not (§5.1) |
| 6 | Alloy popup in an `AuxHost` above the content host | VERIFIED (§6) | VERIFIED: drawn above the page, hit-tested, sees the real tab, closes. Needs the `do_close` fix (else `window.close()` quits the shell). Typing into it INCONCLUSIVE (§6) |
| 7 | Chrome-created windows: no flash, no focus; close-and-reopen, delayed URL, `about:blank`, incognito | VERIFIED (§7) | VERIFIED (§7) |
| 8 | Web Store shim before the store's own scripts | VERIFIED with `Page.enable` first (§8) | VERIFIED, same (§8) |
| 9 | Notifications (show, click, close, per-extension block); `permissions.request` dialog, parent, answer | D-Bus notifications VERIFIED; content-setting block NOT VERIFIED, `message_center.disabled_extension_ids` VERIFIED; Chrome's permission dialog, not transient-for our window, takes X focus, answer VERIFIED (§9) | Chromium's own notification popups VERIFIED; block as Linux; Chrome's dialog owned by the GPUI window, takes activation, answer VERIFIED from a tab and from the Alloy popup (§9) |
| 10 | Extensions in private windows; `chrome.cookies.set` against a blocked site | Private: VERIFIED. `cookies.set` stores past the IO filter and a COOKIES content setting; never sent (§10) | Same (§10) |
| 11 | `Extensions.setStorageItems` over a page session | NOT VERIFIED on an Alloy page; VERIFIED on a Chrome-style page, and `chrome.storage.*.set` through `Runtime.evaluate` on the Alloy page (§11) | Same (§11) |

## Gate outcome

**A**, on Linux unconditionally and on Windows under one owner decision (accept the per-process HKCU override, §13). Line for `log.tsv`: `2026-10-24  WX.0  outcome A  Linux: External Extensions JSON; Windows: registry route under a per-process HKCU override + one acknowledgement enable; see spike-ext-win.md`.

- Linux: the §5.2 route works as designed: silent, persistent, per profile, with all of item 2 and the helper under ESB (§1.1, §2, §3).
- Windows: there is no External Extensions folder on Windows (§1.0, W1/W2 in §1.2). Chromium's registry route works per profile once the browser process redirects `HKEY_CURRENT_USER` to a subkey of its own (`RegOverridePredefKey`), with the same item 2 results as Linux, the helper under ESB and live installs. It is silent: new installs arrive disabled as unacknowledged external extensions, Chromium draws nothing for that in a toolbar-less child (an app-menu alert only), and one `management.setEnabled` from the hidden management page acknowledges each for good, without a dialog.
- Item 2 ran on both OSes before this outcome was set (R2-10). Profiles A and B, with conflicting sets, ran side by side in every item; no extension, version, enable state, update or storage crossed profiles.
- The condition (§13): the override redirects every in-process HKCU access of the browser process. Measured: Chromium's own HKCU state then stays inside the profile key, and proxy settings are unaffected (a proxy written only into the override key had no effect). Inferred: native messaging host registrations under the user's real `HKCU\Software\Chromium\NativeMessagingHosts` become invisible unless the shell copies them into the profile key, which fits §5.8's "Link native apps" action. If the owner rejects the override, outcome B applies: the browser-target pipe works on Windows in the sandboxed bundle (`--remote-debugging-io-pipes` with in-process anonymous pipes; `Browser.getVersion` and `Extensions.loadUnpacked` VERIFIED), but the rest of B's backend (reload behaviour, helper and ESB on that route) was not run, so B would need its own run first.

Blocking or accepted under A (R2-10):

| Capability | Result | Kind |
|---|---|---|
| Per-profile install; restart persistence; reload (incognito toggle); enable/disable; uninstall without a dialog; re-install after the `external_uninstalls` edit; store update, no downgrade | VERIFIED both OSes | blocking, met |
| Helper as packed CRX with ESB on; bridge both ways | VERIFIED both OSes | blocking, met |
| Windows: acknowledgement after each new install | VERIFIED, no dialog | blocking, met (one extra engine step) |
| Windows: per-process HKCU override | VERIFIED working; side effects partly measured | owner decision (§13) |
| Permission-raising update | re-enable needs a user gesture; Chrome's own dialog is the prompt; VERIFIED end to end on Windows | accepted change: core's `re_enable_heading` prompt is not shown (§2) |
| Notification block per extension | content setting: no effect; pref `message_center.disabled_extension_ids`: works | mechanism change (§9) |
| Windows notifications | Chromium popups (top right, topmost, no activation), not toasts | accepted (§9) |
| `permissions.request` | Chrome's dialog; answer reaches the extension | accepted: no GPUI dialog (§9) |
| `cookies.set` on a site the user blocked | stored, never sent | accepted with a cleanup rule (§10) |
| Local storage import | `Runtime.evaluate` on the hidden Alloy page | mechanism change (§11) |
| Typing into the Windows popup | not shown | open (§6) |
| Extensions Vsesvit did not install (HKLM registry on Windows, `/usr/share/chromium/extensions` on Linux) | reach every profile | must handle (§12) |

## Proposed decisions (items that failed or needed a changed mechanism)

1. **§5.2, Windows install backend.** `vsesvit-cef/ext/external.rs` gets a Windows form: at browser-process start, before `cef::initialize`, `RegOverridePredefKey(HKEY_CURRENT_USER, HKCU\Software\Vsesvit\Engine\<profile id>)`; installs are subkeys `Software\Google\Chrome\Extensions\<id>` with `path` (the archived CRX) and `version` inside it. After `HelperReady` (and for the helper itself, from the management page) every extension the runtime reports as disabled for an unacknowledged external install is enabled once from the hidden `chrome://extensions` page. Removing a subkey replaces deleting a JSON file. The watcher makes installs live, so on Windows "Restart Vsesvit to finish installing" is not needed (Linux keeps it).
2. **§5.5, permission-raising updates.** The shell does not show core's re-enable prompt. The Extensions window shows the extension as "Needs new permissions" with an "Enable" button; its click calls `SetEnabled` through the management page with a user gesture (`userGesture: true` in `Runtime.evaluate`), and Chrome's own re-enable dialog (owned by the browser window on Windows) is the prompt. `ExtensionCall::SetEnabled` gains a `gesture: bool`.
3. **§3.7 and §1.4, the hidden host on Windows** is a hidden `WS_CHILD` of a browser window, never a toplevel: Alloy hidden browsers activate a toplevel host (also with `WS_EX_NOACTIVATE`) and the GPUI window loses activation. Since §3.7 wants the hidden browsers to outlive any window, `Desktop` re-parents the host (`SetParent`) to another browser window before the window that holds it closes. A message-only parent (`HWND_MESSAGE`) does not help: it was activated the same way (§5.1). Creating a hidden Alloy browser can still move Win32 keyboard focus into it; the shell re-applies its focus after every hidden-browser creation and after `neutralize`. The hidden host's window procedure ignores `WM_CLOSE`.
4. **`LifeSpanHandler::do_close` on Windows** returns true for every Alloy child (extension popups, hidden browsers) and destroys the browser's own HWND; CEF's default posts `WM_CLOSE` to the root window, which closed the whole browser window when a popup called `window.close()`.
5. **§5.7, foreign windows.** Keep `neutralize` synchronous in `on_after_created`; reopen in the foreign browser's own request context (an extension's incognito window lives in Chromium's primary off-the-record profile, which is not the shell's private context); close the foreign browser only after the reopened tab exists (closing first destroyed the off-the-record profile and killed the browser process on Linux). Wait for the first committed URL that is not empty or `about:blank`, up to 2.5 s; then reopen whatever it has (`about:blank` included).
6. **§5.7, Web Store shim.** `Page.enable` before `Page.addScriptToEvaluateOnNewDocument`, on the same page session, before the navigation.
7. **§5.7, notifications.** The per-extension block is the profile pref `message_center.disabled_extension_ids` (list of ids, set through `SetPreference`), not a content setting. `extension_notifications` keeps its check with that backend. On Windows notifications are Chromium's own popups at the top right of the primary monitor; the check accepts that.
8. **§5.7, optional permissions.** Chrome's dialog stays the UI for `permissions.request`; the GPUI optional-permission control is removed and the check is redefined as "a `permissions.request` from a tab or the popup gets Chrome's dialog and its answer" (parity.md).
9. **§5.6/§5.9, storage.** `Extensions.setStorageItems` works only on a Chrome-style page session; the import runs `chrome.storage.local.set(...)` through `Runtime.evaluate` on the hidden Alloy `manifest.json` page instead (invisible to `tabs.query`, unlike a Chrome-style page). `Extensions.getStorageItems` for export likewise, or `chrome.storage.*.get`.
10. **Cookie blocking (§4.2 privacy features).** An extension's `cookies.set` into a blocked site is stored but never sent while the block holds. When the user blocks a site the shell deletes that site's cookies; when they unblock it, it deletes them again before the first request, so nothing an extension planted during the block leaks out.
11. **§5.1, extensions Vsesvit did not install.** The runtime record diff (§5.5) treats any id not in core's records as foreign: on its first sighting the shell calls `Uninstall` for it (which records it in `external_uninstalls`), so registry (HKLM, or HKCU without the override) and `/usr/share/chromium/extensions` entries do not run in Vsesvit profiles.

---

## 0. Harness

- `spikes/ext-win` is one workspace: `linux/` (from ext-api + ext-load), `win/` (from win-core), and `common/` shared by both through `#[path]`: the CEF glue (`common/cefglue/`: client, app, `ext.rs`), the control channel with replies (`common/ctl.rs`), the CDP pipe client (`common/cdp.rs`) and the per-profile report/update/fixture server (`common/report.rs`).
- `ext.rs` keeps a registry of every browser by label (`b<id>` tabs, `f<id>` Chrome-created, `popup`, hidden labels), creates hidden browsers in the platform's hidden host (Alloy or Chrome style), runs `execute_dev_tools_method` on any browser's page session and returns the result to the ctl client (`dt`, `ev`, `evg`), parses the helper's `vsesvit-bridge` console lines only from the browser labelled `bridge`, neutralizes and reopens Chrome-created browsers, and wraps prefs, content settings, the cookie store and a cookie access filter.
- Each profile gets its own report server; Chromium reaches it as `*.vsesvit.test` through `--host-resolver-rules`, so the same extension code reports to whichever profile runs it, and each profile has its own update server (different offered versions per profile).
- Fixtures (`fixtures/fixtures.py`, run in WSL, copied to Windows): H = Vsesvit Helper (bridge page), C = Reporter 1.0/2.0, D = Reporter with `update_url` 1.0/2.0/3.0 (3.0 adds `tabs`+`history`), E = Reporter (profile B only), S = Spy (tabs, notifications, cookies, popup, options, optional permissions). Ids in `.audit/scratch/ext-win/fx/ids.json`.
- Conflicting sets used throughout: A = H, C 1.0, D 1.0, S; B = H, C 2.0, E, S. Both profiles run at the same time.

**Fail-closed guard (VERIFIED).** Both shells check, in the browser process right after `execute_process` returns and before `cef::initialize` or any window:
- Windows (`win/src/platform.rs` `guard`): the thread desktop must be named `vsesvit-extwin` and must not be the input desktop. No override. `win/scripts/guard-test.ps1` (`.audit/scratch/ext-win/guard-win.txt`):
  ```
  other-private-desktop: EXITED code=3 :: GUARD: refusing to start: thread desktop "vsesvit-other" is not "vsesvit-extwin"
  strict-on-right-desktop: EXITED code=3 :: GUARD: refusing to start: desktop "vsesvit-extwin" is (or may be) the input desktop "Default"
  right-desktop: profile A up ... :: main: guard ok (desktop=vsesvit-extwin input=Default)
  ```
  The negative cases ran on other private desktops; nothing ran on the user's desktop.
- Linux (`linux/src/platform.rs` `guard`): `DISPLAY` must be `:94` (checked before connecting, so WSLg's `:0` is never opened), no `WAYLAND_DISPLAY`, and the root window must carry `_VSESVIT_SPIKE_TOKEN` equal to `SPIKE_XTOKEN`. `start-xvfb.sh` refuses a display that already answers, starts Xvfb `-noreset` in a private D-Bus session and sets a fresh 48-hex-digit token. `xctl` (XTEST) runs the same guard. `linux/scripts/guard-test.sh`:
  ```
  wslg-display: exit=3 GUARD: refusing to start: SPIKE_XTOKEN unset
  wslg-wrong-token: exit=3 GUARD: refusing to start: DISPLAY ":0" is not the spike's :94
  private-wrong-token: exit=3 GUARD: refusing to start: display :94 token mismatch
  private-no-token: exit=3 GUARD: refusing to start: SPIKE_XTOKEN unset
  wayland-set: exit=3 GUARD: refusing to start: WAYLAND_DISPLAY is set
  xctl on :0: Error: GUARD: refusing: SPIKE_XTOKEN unset
  xctl on private: 0x1
  ```
  Trap found: Xvfb resets (and drops root properties) when its last client disconnects, so the token vanished until `-noreset` was added.

## 1. Item 1: install routes

### 1.0 What Chromium 154 compiles on Windows (source, tag 154.0.8037.98)

- `chrome/browser/extensions/external_provider_impl.cc:868-905`: under `#if BUILDFLAG(IS_WIN)` the only non-policy external provider is `ExternalRegistryLoader`; the `#else` branch, absent on Windows, holds both file providers (`DIR_EXTERNAL_EXTENSIONS` = `<module dir>/extensions` and the per-user `DIR_USER_EXTERNAL_EXTENSIONS` = `<user data>/External Extensions`). `chrome/common/chrome_paths.cc:414-422` defines `DIR_USER_EXTERNAL_EXTENSIONS` only for ChromeOS, Mac and Linux with Chromium branding.
- `external_registry_loader_win.cc:37`: the key is hard-coded `Software\Google\Chrome\Extensions`, read from HKLM (WOW64 32-bit view) and HKCU, whatever the branding. So every CEF app on the machine and Google Chrome share it, and Chromium watches both keys (`CompleteLoadAndStartWatchingRegistry`, which also *creates* the HKCU key if it is missing). `path` entries (off-store CRX) get `may_be_untrusted`, which only matters to Google-branded builds' install verifier.
- `extensions/common/feature_switch.cc:22-26`: `prompt_for_external_extensions` is on for Windows and Mac and has no command-line switch. `extensions/browser/extension_registrar.cc:545-559`: with it on, a new install from any external location is disabled with `DISABLE_EXTERNAL_EXTENSION` (8192) unless `extensions.settings.<id>.ack_external` is already true. `external_install_manager.cc`: loading the extension once (an enable) counts as acknowledgement; registry installs get a `MENU_ALERT` (an app-menu entry, which a toolbar-less child never shows); after 3 prompts Chromium stops prompting but leaves the extension disabled.
- `chrome_pref_service_factory.cc:220-238`: on Windows tracked prefs are enforced (`GROUP_ENFORCE_DEFAULT`, unless domain-joined), including `extensions.settings` (split) and `extensions.install.initiallist`, so editing them on disk is reset; `extensions.external_uninstalls` is not tracked.
- Other per-profile routes seen in the source, store-only on Windows: `extensions.install.initiallist` (profile pref, installs from the Web Store update URL, observed live) and `CefSettings.chrome_policy_id` (a per-process registry policy key; on a non-managed machine `policy_loader_common.cc:50-140` strips `ExtensionInstallForcelist`/`ExtensionSettings` entries whose update URL is not the Web Store's). Neither installs core's local or archived CRXs, so neither was run.
- `content/browser/devtools/devtools_agent_host_impl.cc:89-135`: Windows supports `--remote-debugging-io-pipes=<read>,<write>` (handle values, adopted with `_open_osfhandle`) for outcome B's pipe.

### 1.1 Linux (VERIFIED)

`linux/scripts/q1-install.sh` (`.audit/scratch/ext-win/q1-linux.txt`): each profile's `<root>/External Extensions/<id>.json` points at a CRX archived inside that profile; A and B start together.

```
## A list: OK [{"name":"Vsesvit Reporter D","version":"1.0","enabled":true,"installType":"sideload"…},{"name":"Vsesvit Spy","version":"1.0","enabled":true…},{"name":"Vsesvit Reporter C","version":"1.0","enabled":true…}]
## B list: OK [{"name":"Vsesvit Reporter E","version":"1.0","enabled":true…},{"name":"Vsesvit Spy"…},{"name":"Vsesvit Reporter C","version":"2.0","enabled":true…}]
## A toplevel maps:          (none: no prompt or other window appeared)
## A page marks: OK "{\"repD\":\"1.0:1\",\"repC\":\"1.0:1\"}"
A reports: D onInstalled install 1.0, C onInstalled install 1.0, both sw-start; helper sw-start; spy sw-start
B reports: E onInstalled install 1.0, C onInstalled install 2.0 ...
```

Each profile runs exactly its own set, C at the version its own JSON names, silently. The lists come from the helper's bridge (item 4).

### 1.2 Windows

`win/scripts/q1-routes.ps1` (`.audit/scratch/ext-win/q1-win.txt`), sandboxed bundle, both profiles at once, listing through a hidden Chrome-style `chrome://extensions` page (`developerPrivate.getExtensionsInfo`), since the helper may not install:

| Route | A | B |
|---|---|---|
| W1 `<root>\External Extensions\<id>.json` | none of ours | none of ours | 
| W2 `<bundle>\extensions\external_extensions.json` (A only) | none of ours | — |
| W3 nothing (what this PC's own registry brings) | `Application Launcher For Drive (by Google) 3.10 DISABLED` | — |
| W4 HKCU redirected per process to `HKCU\Software\VsesvitSpikeExtWin\<label>-<P>` holding `Software\Google\Chrome\Extensions\<id>` {path, version} | `Reporter D 1.0, Spy 1.0, Reporter C 1.0, Helper 1.0`, all `DISABLED` | `Reporter E 1.0, Spy 1.0, Reporter C 2.0, Helper 1.0`, all `DISABLED` |

Prefs after W4: every one of ours `loc=3` (`kExternalRegistry`), `dr=[8192]` (`DISABLE_EXTERNAL_EXTENSION`), from the archived CRX. No window appeared (`wtops`, WinEvent log: only GPUI's, the hidden host, Chrome's hidden helper windows).

- **W1, W2: NOT VERIFIED**, as the source says: Chromium on Windows reads neither file.
- **W3 (VERIFIED, a finding):** every fresh Vsesvit profile on this PC installs the extension the user's HKCU `Software\Google\Chrome\Extensions` names (`loc=6`, downloaded from the Web Store, disabled pending acknowledgement). Any machine-wide (HKLM) or per-user registry extension, written by other software for Google Chrome, reaches every CEF profile. The W4 override removes the HKCU half of this leak; HKLM entries still reach the profile (this PC has none).
- **W4: the per-profile form of the registry route works.** `RegOverridePredefKey(HKEY_CURRENT_USER, HKCU\<subkey>)` in the browser process right after `execute_process` (`win/src/platform.rs` `reg_override`). Chromium then reads, watches and writes "HKCU" inside that subkey only.

`win/scripts/q1-ack.ps1` (`.audit/scratch/ext-win/q1-ack-win.txt`), on W4's profile A:

```
## list before: ["Vsesvit Reporter D 1.0 DISABLED","Vsesvit Spy 1.0 DISABLED","Vsesvit Reporter C 1.0 DISABLED","Vsesvit Helper 1.0 DISABLED"]
## enable H|C|D|S from chrome://extensions (chrome.management.setEnabled): OK "ok" each
## list after enable: [... all ENABLED]                   (no new toplevel window)
## setpref dotted path extensions.settings.<E>.ack_external: ERR Trying to modify an unregistered preference
## setpref extensions.settings (+E ack_external): OK      (the whole dictionary, read and written back through CefRequestContext)
   (registry subkey for E added while the profile runs)
## list after live E (pre-acked): ["Vsesvit Reporter E 1.0 ENABLED", ...]   (6 s later, no restart, no dialog)
## prefs after run: E loc=3 dr=[] ack=True; D, S, C, H dr=None
## list after restart: all five ENABLED
```

- Acknowledgement without a dialog: VERIFIED both ways, (a) one `management.setEnabled(id, true)` from a hidden `chrome://extensions` page after the install (the helper itself starts disabled, so the management page has to do its first enable), and (b) `ack_external` written through `SetPreference("extensions.settings", …)` before the subkey appears; then the install arrives enabled. (b) rewrites Chromium's whole internal extension dictionary and is fragile; (a) is the one to build.
- Off-store CRX on a Chromium-branded Windows build: VERIFIED accepted (our self-signed CRXs installed and ran after acknowledgement; reports from C, D, E, helper and spy arrived).
- Live install without a restart: VERIFIED (the registry watcher re-reads the overridden key). Linux needs a restart (EL §6.2).
- What the override changes besides the extension route: §13.

## 2. Item 2: lifecycle on the route

`tools/items.py <os> 2`; Linux `items-linux-2.txt`, Windows `items-win-2.txt` and `items-win-2b.txt` (the rerun with the child hidden host of §5.1). The two OSes printed the same lines; Windows shown, Linux differences noted.

```
A list: C 1.0 on, D 1.0 on, S 1.0 on            B list: C 2.0 on, E 1.0 on, S 1.0 on
-- restart both: the same lists
-- reload: developerPrivate.updateExtensionConfiguration({extensionId: C, incognitoAccess: true}) -> "ok"; C stays ENABLED, and after a restart too
-- helper setEnabled D false -> bridge event {"event":"disabled","id":D,"reason":"unknown"}; after restart: D 1.0 off; setEnabled true -> {"event":"enabled"}
-- B: hidden Alloy chrome-extension://E/manifest.json, Runtime.evaluate chrome.management.uninstallSelf({showConfirmDialog:false})
   -> evaluate never resolves (TIMEOUT, as EL §7); bridge events disabled E, uninstalled E; no new window
   prefs after quit: external_uninstalls: [E]
-- install entry back (registry subkey / JSON), restart: E stays out
-- external_uninstalls emptied in Preferences before start: E reinstalls fresh (onInstalled install, localBefore {})
-- A's update server offers D 2.0; requestUpdateCheck from a hidden D manifest.json page -> {"status":"update_available","version":"2.0"}
   restart: D 2.0 on (entry still says 1.0); D reports onInstalled update 2.0 previousVersion 1.0. B (no D, its server offers 1.0): unaffected
-- A's server offers D 3.0 (adds tabs, history): restart -> D 3.0 off(permissions_increase); no window appears by itself
   helper setEnabled(D, true): "Re-enabling an extension disabled due to permissions increase requires a user gesture."
   management page, same without gesture: same error
   management page with userGesture: Chrome's re-enable dialog appears; the promise waits for it
```

- `external_uninstalls` is in `Preferences` on Windows too and is not MAC-protected: removing an id there before the start works on both OSes (§1.0).
- The re-enable dialog: Linux, a 448x303 toplevel mapped at the root origin that took X focus (`rootwatch: X focus … -> 0x200017`). Windows, `Chrome_WidgetWin_1` 809x441, owned by the window holding the management page (the hidden host in the first run, the GPUI window with the child host). Answered on Windows (`reenable-win.txt`): the dialog reads "The newest version of "Vsesvit Reporter D" has been disabled because it requires more permissions … Re-enable / Cancel" (`.audit/scratch/ext-win/runs/re/A/reenable-dialog.png`); a posted click on "Re-enable" resolved the call with `ok` and D 3.0 ran. Not answered on Linux.
- Windows only: registry installs are acknowledged once (`ack_external` true in Secure Preferences afterwards); every later start keeps them enabled (`items-win-5-3.txt` prefs, `ack=True`).

## 3. Item 3: the helper under Enhanced Safe Browsing

`items-linux-3-6-8.txt`, `items-win-5-3.txt`:

```
A setpref safebrowsing.enhanced true: OK; restart; pref after restart: OK true
A bridge: OK bridge … load_end status=200; ping: OK {"pong":"nidhmk…"}; list: C 1.0 on, D 1.0 on, S 1.0 on
```

The helper and every other packed external install load with ESB on, on both OSes (ESB only blocks `--load-extension`, EL §2). The helper's service worker did not start by itself after the restart (no event woke it); the bridge page does not need it.

## 4. Item 4: the page-session bridge

`items-linux-4-11.txt`, `items-win-4-6-7.txt`:

```
A ping: OK {"pong":"nidhmkokehfepclimlkbmpccblaklipj"}
20 ping round trips over ctl+page session: 1.7 ms each (Linux), 12.7 ms each (Windows)
bridge events: ['{"event":"test","n":7}', '{"event":"disabled","id":C,"reason":"unknown"}', '{"event":"enabled","id":C}']
a console line "vsesvit-bridge {"forged":1}" from a tab: not parsed as a bridge event (logged as console[b1] / cef-ev: console)
bridge page closed and recreated: OK, ping OK
```

- Shell → helper: `execute_dev_tools_method("Runtime.evaluate", {expression: "vsesvitBridge.call(<json>)", awaitPromise, returnByValue})` on the hidden Alloy `bridge.html`, result in `on_dev_tools_method_result` (`common/cefglue/ext.rs` `exec`). Helper → shell: `console.debug("vsesvit-bridge …")` read in `on_console_message` of that browser only.
- `management.onDisabled` gives `disabledReason: "unknown"` for a user disable and `permissions_increase` for item 2's case.
- The Windows round trip includes the external pump's idle back-off (SW §4); it is a latency, not a cost.
- Windows trap: closing the hidden bridge browser first took the whole hidden host with it (`ERR no browser bridge` in `items-win-4-11.txt`); see §5.2.

## 5. Item 5: `developerPrivate` from a hidden Chrome-style page; what extensions see

`items-linux-11-5.txt`, `items-win-5-3.txt`. Hidden browsers open at the time: `bridge` (Alloy), `maint` (Alloy, `about:blank`), `u` (Alloy, an extension's `manifest.json`), the spy's own control page (Alloy), `mgmt` (Chrome style, `chrome://extensions`).

```
A getExtensionsInfo: D 1.0 ENABLED, Spy 1.0 ENABLED, C 1.0 ENABLED, Helper 1.0 ENABLED
A updateExtensionConfiguration C hostAccess ON_CLICK: OK "ok"      -> C host: ON_CLICK
A updateExtensionConfiguration S incognito: OK "ok"                -> Spy incog: true
A spy tabs.query({}) urls: ['http://site.vsesvit.test/index.html', 'chrome://extensions/']
A spy windows.getAll types: [('popup', ['http://site.vsesvit.test/index.html']), ('popup', ['chrome://extensions/'])]
A spy events (Linux): windows.onCreated, tabs.onCreated chrome://extensions/ ; after closing mgmt: tabs.onRemoved, windows.onRemoved
```

- `developerPrivate` works over the page session of a hidden Chrome-style `chrome://extensions` browser, on both OSes, and so does `chrome.management.*` from that page (used in §1.2 to acknowledge Windows installs). EL §8.1 had used the pipe.
- Alloy hidden browsers are invisible to extensions: not in `tabs.query({})`, not in `windows.getAll`, no `tabs.onCreated`. The Chrome-style management page is a one-tab `popup` window with create and remove events while it lives, as §3.7 expected.

### 5.1 Windows: the hidden host must not be a toplevel (VERIFIED)

`items-win-12.txt`, `items-win-12-message.txt`, `tools/items.py win 12`: the same profile with four kinds of hidden host (`win/src/platform.rs` `create_hidden_host`, `SPIKE_HIDDEN_HOST`):

| Hidden host | After the Chrome-style management page | After the Alloy bridge page |
|---|---|---|
| `WS_POPUP` toplevel (first version) | GPUI active | `winevent: foreground <hidden host>`, GPUI inactive |
| `WS_POPUP` + `WS_EX_NOACTIVATE` | GPUI active | same: hidden host foreground, GPUI inactive |
| message-only (`HWND_MESSAGE`) | GPUI active | active window = hidden host, GPUI inactive |
| hidden `WS_CHILD` of the GPUI window | GPUI active | GPUI active; Win32 focus moved into the hidden child |

An Alloy windowed browser activates the toplevel it lives in when it is created, even with `on_set_focus` refusing focus for hidden browsers. On the user's desktop that would take activation away from the browser window. Only a child of the (visible) browser window avoids it; keyboard focus can still land in the hidden child, so the shell re-applies its own focus afterwards. Linux has no such problem (the X11 hidden host is an unmapped override-redirect window; nothing focused it in any run).

### 5.2 Windows: Alloy children close their root window (VERIFIED)

`runs/i6/A/spike.log` (first run): the popup's `window.close()` →
```
cef: do_close id=4
gpui: close button pressed; closing browsers first      <- WM_CLOSE reached the GPUI window
gpui: quit: closing 4 browsers
```
and the popup browser never closed, so the shell hung in shutdown. CEF's default close of an Alloy windowed browser posts `WM_CLOSE` to its root window: the GPUI window for the popup, the hidden host for hidden browsers (which `DefWindowProc` destroyed with every hidden browser in it, §4). With `do_close` returning true for Alloy children and the shell destroying the browser's own HWND, and the hidden host ignoring `WM_CLOSE` (`common/cefglue/client.rs`, `win/src/platform.rs`), every later run closed cleanly. Linux was not affected.

## 6. Item 6: the extension popup in an AuxHost

`items-win-4-6-7.txt`, `items-linux-3-6-8.txt`. `popup chrome-extension://S/popup.html`: an Alloy browser in a host window that is a child of the browser window, above the content host, right edge at the content's right edge, auto-resize 25x25 to 800x600.

Windows:
```
popupinfo: aux rect 560x317 (320x181 DIP at 175 %) children_top_first=[aux (VsesvitSpikeHost), content host (VsesvitSpikeHost)]
popupprobe: centre colour=#fde68a (the popup's background) hit=…:Chrome_WidgetWin_1 window_from_point=…:Chrome_RenderWidgetHostHWND (the popup's)
popup view: {"size":[320,180],"activeCurrent":[{"url":"http://site.vsesvit.test/index.html"}],"current":{"windowId":-1}}
popupfocus: SetFocus(popup) -> focus=<popup HWND>; popup reports popup-focus hasFocus=true
window.close(): on_before_close client=auxpopup; gpui: popup closed
```
Linux: the same view (`activeCurrent` = the real tab, `windowId: -1`), `x_focus` moved into the popup, `window.close()` closed it.

- Drawing: VERIFIED by colour in a `PrintWindow` capture: the popup shows through GPUI's transparent content area (SW §5). Anything GPUI paints over that spot would hide it, so the popup must stay inside the content rect, or GPUI must leave its rect transparent.
- Input: VERIFIED by hit test (the popup's own render widget wins at its centre). Keyboard focus moves into the popup. A key posted to the popup's top CEF window did not reach the page (`popup-key` never reported); posting to the render widget window was not tried, so typing is INCONCLUSIVE on Windows.

## 7. Item 7: Chrome-created windows

`items-linux-7.txt`, `items-win-7-8-10.txt`. The spy calls each API from its hidden control page; `ext.rs` neutralizes synchronously in `on_after_created` of a `default_client` browser (Linux: unmap and reparent into the hidden host; Windows: `SW_HIDE`, `WS_CHILD`, `SetParent` into the hidden host), waits for the first committed URL that is not `about:blank` (2.5 s cap), opens that URL as a shell tab in the foreign browser's request context, and closes the foreign browser 1.5 s later.

| Case | Both OSes |
|---|---|
| `tabs.create` | `shown_at_callback=false`, neutralize 0.2–1.8 ms, reopened on commit after 22–200 ms |
| `windows.create` | same |
| `runtime.openOptionsPage` | same, URL `chrome-extension://…/options.html` |
| slow URL (server waits 1.5 s) | reopened `why=committed after 1.51 s`: the URL was not known before |
| explicit `about:blank` | reopened `about:blank` `why=timeout 2500 ms` |
| `windows.create({incognito: true})` | foreign context `is_global=0 same_as_shell_private=0 cache_path=""`: Chromium's primary off-the-record profile, not the shell's private context; reopened in that same context |
| `tabs.create` right after the shell's private tab was focused | lands in the shell's private context (`same_as_shell_private=1`, `incognito: true`) |

No toplevel was shown or mapped in any case (WinEvent `show` / `rootwatch: MapNotify` lists empty), and the browser window stayed active (`gpui_active=true` before and after, Windows with the child hidden host; Linux `gpui_active=true`). The baseline without neutralize, same call: Linux `MapNotify … name="Untitled - Chromium"` and X focus to it; Windows `winevent: show … "Untitled - Chromium"`, `gpui_active=false`, active window = Chrome's. Extensions see the cost §5.7 accepted: `tabs.onCreated` for the foreign tab, then `tabs.onRemoved` and `windows.onRemoved`, and a second `onCreated` for the reopened tab.

Trap (Linux, first run): closing the foreign incognito browser at once, before the reopened tab existed in its context, killed the browser process silently; Chromium destroys an off-the-record profile with its last browser. With the 1.5 s close delay it passed on both OSes. Windows trap: after `SetParent`, Win32 focus can stay in the hidden host (`focus=<hidden host>` with the browser window still active); the shell's next focus change fixes it.

## 8. Item 8: the Web Store shim

`items-linux-8-10.txt`, `items-win-7-8-10.txt`. On a Chrome-style tab at `about:blank`: `Page.enable`, then `Page.addScriptToEvaluateOnNewDocument` with the shim, then navigate.

```
store.html fixture: vs-shim ran {"scripts":0,"readyState":"loading",…}; page's first script: sawShim=true
chromewebstore.google.com/detail/<spy id>:   __vsShim {"scripts":0,"readyState":"loading","webstorePrivate":23,"wrapped":true}
                                              beginInstallWithManifest3 after load: "function () { console.log('vs-shim intercepted …'); … }"
chromewebstore.google.com/detail/lmjegml…:  same
```

The shim runs before any of the store's scripts (`document.scripts.length` 0, `readyState` `loading`), sees `chrome.webstorePrivate` (23 members) and replaces `beginInstallWithManifest3`; the store keeps the replacement. Without `Page.enable` first, the script was registered (`{"identifier":"1"}`) but never ran (first Linux run, `items-linux-3-6-8.txt`). Nothing was installed: the shim was only observed.

## 9. Item 9: notifications and `permissions.request`

`items-linux-9.txt`, `items-win-9.txt`.

Notifications (the spy's `chrome.notifications.create` with a button, `requireInteraction`):

| | Linux (private D-Bus, `tools/notifyd.py` as the notification server) | Windows |
|---|---|---|
| show | `Notify` call: app "Chromium", summary, body, actions `["0","B1","default","Activate","settings","Settings"]` | a Chromium popup: `Chrome_WidgetWin_1` 630x266 at the top right of the monitor, `WS_EX_TOPMOST`, no activation, no owner (`.audit/scratch/ext-win/runs/i9/A/notif.png`) |
| click | `ActionInvoked default` → `onClicked n1` | posted click on its body → `onClicked n1` |
| close by the user | `NotificationClosed reason 2` → `onClosed n2 byUser=true` | posted click on its × → `onClosed n2 byUser=true` |
| `notifications.clear` | `CloseNotification` → `onClosed byUser=false` | `onClosed n1 byUser=false` |

Windows toasts were not exercised: the spike disables `NativeNotifications`, and without that Chromium would still not use toasts here, because CEF has no registered toast activator under HKCU (`notification_platform_bridge_win.cc` `SetReadyCallback`; this PC has Chromium's activator only under HKLM). Whether an installed Vsesvit should register one is out of scope.

Per-extension block:
```
content setting NOTIFICATIONS block for chrome-extension://<S>/: set (value 2), permission level still "granted", notify n3 shown
setpref message_center.disabled_extension_ids ["<S>"]: permission level "denied", notify n4 -> "Unknown error.", nothing shown
setpref message_center.disabled_extension_ids []: "granted" again
```
NOT VERIFIED: the content-setting route of §5.7. VERIFIED: the pref Chrome's own notification settings use (`notifier_state_tracker.cc`).

`permissions.request({permissions: ["bookmarks"]})` from the spy's options page in a Chrome-style tab, with a user gesture:
- Linux: a 448x212 toplevel mapped horizontally centred over the browser window, no `WM_TRANSIENT_FOR`, `_NET_WM_WINDOW_TYPE_NORMAL`, takes X focus (`gpui_active=false`); "Vsesvit Spy has requested additional permissions… Read and change your bookmarks", buttons Deny (default) and Allow (`.audit/scratch/ext-win/linux-perm-dialog.png`). Return → `false`; an XTEST click on Allow → `true`, `bookmarks` granted.
- Windows: `Chrome_WidgetWin_1` 809x371 owned by the GPUI window, takes activation, buttons Allow then Deny (`runs/i9/A/perm-dialog.png`). A posted click on Allow → `true`.
- The same from the options page loaded in the Alloy popup host, for an optional host permission: the dialog appears on both OSes (Windows: owned by the GPUI window), the promise waits, Allow → `true`.
- A request covered by an already granted host pattern resolves `true` with no dialog (first Linux run; the fixture's optional host was changed for that reason).

## 10. Item 10: private windows and `cookies.set`

`items-linux-8-10.txt`, `items-win-7-8-10.txt`:

```
allow C, S in incognito (developerPrivate): ok
private tab (shell's off-the-record context): page marks {"repC":"1.0:1:incognito"}; ctx is_global=0 same_as_shell_private=1
spy (normal context, "spanning") sees the private tab: [('…/index.html', False), ('chrome://extensions/', False), ('…/index.html', True)]
cookieblock blocked.vsesvit.test (the shell's IO cookie filter)
extension cookies.set({url: http://blocked.vsesvit.test/, name: ext}) -> {"name":"ext","value":"1"}; cookie store has it
page request: cookiefilter: refuse send ext; document.cookie: SecurityError
plus COOKIES content setting BLOCK for the site: cookies.set again -> stored (ext2); cookies.getAll -> ["ext=1","ext2=2"]
```

Extensions allowed in private windows run there (content scripts, events). `chrome.cookies.set` writes into the store regardless of the IO filter and of a COOKIES content setting; the filter keeps the cookie off the site's requests and `document.cookie` stays blocked, so nothing reaches the site while the block holds. Decision 10 handles the unblock.

## 11. Item 11: extension storage from a page session

`items-linux-11-5.txt`, `items-win-4-11.txt` (identical on both OSes, both profiles):

```
Alloy chrome-extension://C/manifest.json: Extensions.setStorageItems -> ERR {"code":-32600,"message":"Extension not found."}
Alloy, Runtime.evaluate chrome.storage.local.set({viaEval}) -> OK {"viaEval":"A"}
Chrome-style manifest.json: setStorageItems local -> OK {}; getStorageItems -> {"fromShell":"A","n":42,"starts":1}; sync area -> OK
the extension reads {"fromShell":"A","n":42,"viaEval":"A"}; B got its own values
another extension's id from that session -> "Extension not found."
the tab where C's content script runs: getStorageItems -> {"fromShell":"A"}
```

`Extensions.*StorageItems` accepts only a page whose frames hold extension storage access (`extensions_handler.cc` `CanAccessStorage`); an Alloy extension page does not qualify, a Chrome-style one does. The bridge page fails the same way for the helper's own id.

## 12. Things the item list did not ask for

- **Foreign extensions reach every profile.** Windows: the user's `HKCU\Software\Google\Chrome\Extensions` entry installs into every profile without the override (W3); HKLM entries would with it. Linux: `/usr/share/chromium/extensions` (research §8). Decision 11.
- **The registry route installs live** on Windows (the watcher), Linux needs a restart.
- **Chromium's re-enable and permission dialogs** are owned by the browser window that issued the call; the hidden management page therefore needs a hidden host that is a child of a browser window (§5.1), or the dialog's owner is a window the user never sees.
- **Every CEF profile is `<root>\Default`**: `cache_path` `<root>/XW_A` still put the profile in `<root>/Default` (first Linux run). On Windows, without the override, CEF keeps tracked-pref MACs in `HKCU\Software\Chromium\PreferenceMACs\Default`, one key for every Vsesvit profile and any other Chromium-branded CEF app using the name. With the override that key moves into the profile's subkey.

## 13. The HKCU override: what it changes (Windows)

- VERIFIED: Chromium creates and watches `Software\Google\Chrome\Extensions` inside the override subkey, writes its own `Software\Chromium\…` state (BLBeacon, PreferenceMACs, StabilityMetrics) there, and reads no extension entries from the real HKCU.
- VERIFIED: proxy settings are not taken from the override key: with `ProxyEnable=1, ProxyServer=127.0.0.1:9` written only into the override key, pages loaded normally (`items-win-13.txt`; the real per-user setting has no proxy). Chromium's WinHTTP-based proxy lookup reads the real per-user settings.
- INFERRED: Chromium's native messaging host lookup (`HKCU\Software\Chromium\NativeMessagingHosts`) goes through the override and finds nothing unless the shell copies the user's registrations into the profile key. Not run: connecting would have launched the user's Bitwarden or Proton Pass helper.
- INFERRED: any other per-user setting read in process through `HKEY_CURRENT_USER` in the browser process (not in the sandboxed children) reads the empty profile key. GPUI, page loads, fonts and the sandboxed children behaved normally in every run.
- The override must happen in the browser process right after `execute_process` returns and before `cef::initialize`, and lasts for the process's life: the registry watcher re-reads through `HKEY_CURRENT_USER`, so lifting the override later would let the real HKCU entries in (and drop the profile's own).

## What touched this PC

- Every Windows run: the sandboxed bundle on the private desktop `vsesvit-extwin` through `deskrun.exe`; the spike refuses any other desktop. Negative guard tests ran on another private desktop (`vsesvit-other`). Input only as messages posted to the spike's own windows (the spike checks the window's process), CEF host APIs, CDP or JS. Screenshots only of the spike's own windows (`PrintWindow`). No downloads (store pages were only opened; Chromium's updater downloaded "Application Launcher For Drive" from the Web Store into scratch profiles, as any CEF run on this PC does, W3).
- Registry: created and used `HKCU\Software\VsesvitSpikeExtWin` (one subkey per run label and profile: the override targets and their extension entries, one proxy value in `i13-A`); deleted at the end. Runs without the override (the guard test's positive case, q1 W1–W3) wrote the values Chromium always writes under the existing `HKCU\Software\Chromium` (`BLBeacon`, `StabilityMetrics`, `PreferenceMACs\Default` and `\Browser`), as the earlier spikes' runs had; no new keys there (`.audit/scratch/ext-win/reg-before.txt` vs `reg-after.txt`). Read only: `HKCU\Software\Google\Chrome\Extensions`, HKLM's extension keys, Chromium's toast activator CLSID, the per-user proxy flag.
- WSL: Xvfb `:94` in a private D-Bus session (started by the spike, stopped at the end); the WSLg display was never opened (the guard refuses `:0` before connecting).
- Every spike process was stopped (`quit`, then the kill scripts); none left.
