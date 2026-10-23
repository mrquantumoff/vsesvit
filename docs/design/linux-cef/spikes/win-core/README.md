# win-core: GPUI + Chrome-style CEF on Windows (throwaway spike)

The findings are in `docs/design/linux-cef/spike-win.md`. This folder holds the code that produced them. It is not production code.

## What is here

- `src/entry.rs` runs CEF and GPUI in order: `api_hash`, `execute_process`, `initialize`, the GPUI app, then `cef::shutdown`. `src/main.rs` (plain exe) and the `RunWinMain` export in `src/lib.rs` (sandbox, loaded by CEF's `bootstrap.exe`) both call it.
- `src/cefglue/` holds the CEF app, the client and its handlers. It sends events to GPUI over a channel and never imports `gpui`.
- `src/pump.rs` is the external message pump on GPUI's thread. `SPIKE_PUMP=mt` switches to CEF's multi-threaded loop instead.
- `src/shell.rs` is the GPUI window: tab strip, toolbar, omnibox, content area, overlays, focus model, and the control-channel commands.
- `src/win.rs` covers the Win32 side: content host, geometry, focus guard, regions, `PrintWindow`, z-order, hit tests, and messages posted to our own windows.
- `src/wgc.rs` is a Windows.Graphics.Capture experiment. It does not work on the private desktop.
- `src/bin/helper.rs` is the CEF sub-process exe. `src/bin/serve.rs` serves `testpages/`. `src/bin/deskrun.rs` starts a program on a private desktop.
- `vendor/gpui-pre-windows` is `gpui-pre-windows` 0.3.9 plus `vendor/gpui-windows-keys.patch`. Setting `GPUI_SPIKE_UNPATCHED=1` turns the patch off at run time.
- `scripts/` holds `lib.ps1` (helpers), `selftest.ps1`, the `explore-*.ps1` experiments, `bundle-sandbox.ps1`, `run.ps1` and `kill.ps1`.

## Build

Requirements: MSVC Rust, Visual Studio with C++, and the CMake and Ninja that ship with Visual Studio.

```powershell
. C:\Users\mrqua\git\vsesvit\.audit\scratch\win-spike\env.ps1   # CMAKE, PATH (Ninja), CEF_PATH, CARGO_TARGET_DIR
cd docs\design\linux-cef\spikes\win-core
cargo build
.\scripts\bundle-sandbox.ps1        # optional: sandboxed layout in .audit\scratch\win-spike\bundle
```

When `CEF_PATH` is empty, `cef-dll-sys` downloads the minimal CEF 154.0.34 build into it. It then copies the runtime files next to the exe. Stop the page server before you rebuild, because cargo cannot replace a running `serve.exe`.

## Run and drive

The scripts start the spike on a private desktop named `vsesvit-spike` through `deskrun.exe`. That desktop is never shown, so the spike cannot steal focus, cover the screen, or send input to the user's desktop.

```powershell
. .\scripts\lib.ps1
Start-PageServer
$p = Start-Spike -Label try1 -Vars @{ SPIKE_PUMP = 'ext' }   # log: .audit\scratch\win-spike\runs\try1\spike.log
ctl state; ctl "keypost l ctrl to=page"; ctl "shot $script:Run\s.png"; ctl quit
Stop-Spike
```

Every control command is listed in spike-win.md §1.2. These environment variables change the setup:

| Variable | Values |
|---|---|
| `SPIKE_PUMP` | `ext`, `mt` |
| `SPIKE_KEYS` | `none`, `cmd`, `key`, `both` |
| `SPIKE_DIALOGS` | `default`, `shell` |
| `SPIKE_POPUPS` | `child`, `default`, `tab` |
| `SPIKE_HOST` | `direct` |
| `SPIKE_HOLE` | `0` |
| `SPIKE_SAME_EXE` | `1` |
| `SPIKE_STRICT` | `1` |
| `SPIKE_CEF_SCALE` | a scale factor |
| `SPIKE_DISABLE_FEATURES` | Chrome feature names |
| `SPIKE_ACTIVATE` | `0` |
| `SPIKE_WATCHDOG` | `0` |
| `SPIKE_RESIZE_INLINE` | `1` |
| `SPIKE_MT_DRAIN` | `0` |

## Test

```powershell
.\scripts\selftest.ps1      # prints PASS/FAIL lines and "== N passed, M failed"
.\scripts\kill.ps1          # kills anything left under .audit\scratch\win-spike
```
