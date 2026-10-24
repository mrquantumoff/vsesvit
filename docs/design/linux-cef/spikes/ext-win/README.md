# ext-win (WX.0): extensions on Linux and Windows, per profile

Throwaway spike code. Findings: `docs/design/linux-cef/spike-ext-win.md`.

## Layout

| Path | What |
|---|---|
| `Cargo.toml` | own workspace: `linux/`, `win/`, vendored GPUI backends with their one patch each (`vendor/`) |
| `common/cefglue/` | CEF glue shared by both shells: client, app, `ext.rs` (browser registry, hidden browsers, DevTools page sessions, bridge console, foreign-window neutralize and reopen, cookie filter, prefs, content settings) |
| `common/ctl.rs`, `common/cdp.rs`, `common/report.rs` | control channel with replies (TCP), CDP pipe client, per-profile report/update/fixture server (`*.vsesvit.test`) |
| `linux/` | the X11 shell (from ext-api and ext-load), `src/platform.rs` (display guard, neutralize, hidden host), `scripts/` |
| `win/` | the Windows shell (from win-core), `src/platform.rs` (desktop guard, hidden host, neutralize, CDP io-pipes, HKCU override, WinEvent log), `scripts/` |
| `exts-src/` | test extensions: `helper` (bridge page), `reporter`, `spy` |
| `fixtures/` | `fixtures.py` builds every extension, CRX and the fixture site; `site/` |
| `tools/items.py` | items 2 to 13, the same checks on both OSes; `prefs.py` (extension state from a profile), `notifyd.py` (a D-Bus notification server for the private session) |
| `tools/mkcrx` | CRX3 packer on vsesvit-core's testkit |

## Run

Linux (WSL; everything under `~/vsesvit-scratch/ext-win`):

```sh
bash linux/scripts/build.sh "$PWD" fixtures   # copy into WSL, build, make fixtures (also copied to the Windows scratch)
~/vsesvit-scratch/ext-win/src/linux/scripts/start-xvfb.sh   # private Xvfb :94 with a token; refuses an existing display
~/vsesvit-scratch/ext-win/src/linux/scripts/guard-test.sh
~/vsesvit-scratch/ext-win/src/linux/scripts/q1-install.sh
cd ~/vsesvit-scratch/ext-win/src/tools && python3 -I items.py linux 2 3 4 5 6 7 8 9 10 11
~/vsesvit-scratch/ext-win/src/linux/scripts/kill.sh all
```

Windows (PowerShell 7; scratch `.audit\scratch\ext-win`, CEF from `.audit\scratch\win-spike\cef`):

```powershell
. win\scripts\lib.ps1; Get-BuildEnv; cargo build -p vsesvit-extwin-win; win\scripts\bundle-sandbox.ps1
win\scripts\guard-test.ps1
win\scripts\q1-routes.ps1; win\scripts\q1-ack.ps1
cd tools; python3 items.py win 2 3 4 5 6 7 8 9 10 11 12 13
win\scripts\kill.ps1
```

The spike refuses to start anywhere but the private desktop `vsesvit-extwin` (Windows) or the token-marked Xvfb `:94` (Linux); there is no override. Input goes only to the spike's own windows (posted messages on Windows, XTEST into the private Xvfb on Linux), CEF host APIs, CDP or JS. The Windows runs write registry keys only under `HKCU\Software\VsesvitSpikeExtWin` (delete it afterwards).
