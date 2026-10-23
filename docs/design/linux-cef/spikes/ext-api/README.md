# spike-ext-api: what extensions see under GPUI + Chrome-style CEF children

The spike-core shell with the repo's fixture extensions and two probes (`exts/spy`, `exts/ports3`)
loaded through `--load-extension`, a CDP harness on the remote-debugging port, and `ctl` commands
for Chrome-created windows, popup hosting and the shell-run context menu. Findings:
`docs/design/linux-cef/spike-ext-api.md`.

## Layout

| Path | What |
|---|---|
| `env.sh` | sourced by every script; all paths come from `VSESVIT_SCRATCH`, `VSESVIT_REPO`, `CARGO_TARGET_DIR`, `SPIKE_DISPLAY` |
| `exts/spy` | records tab/window events, commands, menu clicks; `report()` answers the tab model; `popup.html` reports what a popup sees |
| `exts/ports3` | MV3 port of the ports fixture |
| `harness/server.py` | 127.0.0.1:8793: serves `tests/fixtures/site`, logs hits and `POST /report` |
| `harness/cdp.mjs` | Node 22+ CDP daemon (`node harness/cdp.mjs daemon`) and client (`eval sw:spy EXPR`, `targets`, `browser METHOD JSON`) |
| `ids.py` | extension ids for the loaded dirs (`$E/ids.json`), from the manifest key or the path |
| `src/rootwatch.rs` | logs every toplevel X window Chrome creates, maps or reparents |
| `p1-popup.sh`, `p2-action.sh` | the popup and action experiments of this run |

`tools/kwin-input` is a spike-core copy that this spike never used.

## Run

```sh
export VSESVIT_SCRATCH=~/vsesvit-scratch VSESVIT_REPO=~/git/vsesvit
CEF_PATH=$VSESVIT_SCRATCH/cef CARGO_TARGET_DIR=$VSESVIT_SCRATCH/target-extapi cargo build
./p1-popup.sh          # starts Xvfb :93 with a private D-Bus, the fixture server, the spike and the CDP daemon
./p2-action.sh
./kill.sh; kill $(pgrep -f 'Xvfb :93') $(pgrep -f 'harness/server.py')
```

Useful `ctl` commands: `popuphost alloy|chrome URL` (popup page in a child browser at the top
right, auto-resized), `popupjs JS`, `popupclose`, `menupick LABEL|ID` (with `SPIKE_MENU=shell`),
`adopt close|redirect|reparent`, `adoptsync hide|embed|close`, `showext N`, `devtools`.
