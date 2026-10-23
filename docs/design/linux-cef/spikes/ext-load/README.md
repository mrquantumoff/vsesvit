# spike-ext-load: installing and managing extensions in CEF 154

The spike-core shell (GPUI on X11, Chrome-style CEF children) plus what an install/manage test needs:
an in-process CDP client over `--remote-debugging-pipe`, a report server for test extensions, and
`ctl` commands for prefs and CDP. Findings: `docs/design/linux-cef/spike-ext-load.md`.

## Layout

| Path | What |
|---|---|
| `env.sh` | sourced by every script; all paths come from `VSESVIT_SCRATCH`, `VSESVIT_REPO`, `CARGO_TARGET_DIR`, `SPIKE_DISPLAY` |
| `src/cdp.rs` | CDP over fds 3/4 (`SPIKE_CDP_PIPE=1`); `ctl cdp`, `cdps`, `targets`, `sweval`, `swcdp`, `pagecdp`, `pageeval`, `extpageeval` |
| `src/report.rs` | 127.0.0.1:8792: serves `$E/site`, logs `POST /report` bodies and `GET /updates/*` |
| `exts-src/reporter`, `exts-src/agent` | test extensions: Reporter (storage, lifecycle and update reports), Vsesvit Agent (`chrome.management` helper) |
| `site/` | fixture page served by the report server |
| `mkext.py`, `tools/mkcrx` | lay out `<exts>/<id>/<version>` with a manifest `key`; pack CRX3 with vsesvit-core's testkit writer |
| `fixtures.sh` | makes throwaway keys, every extension directory and CRX, and `$E/ids.env` |
| `q*.sh` | the experiments (`q1`–`q3` from the earlier run, `q4`–`q7` from this one) |

`accel-test.sh`, `cpu*.sh`, `dialog-test.sh`, `latency-test.sh`, `runwait.sh`, `selftest.sh`,
`start-display.sh` and `tools/kwin-input` are spike-core copies that this spike never used; they
still point at spike-core's paths.

## Run

```sh
export VSESVIT_SCRATCH=~/vsesvit-scratch VSESVIT_REPO=~/git/vsesvit   # scratch root, checkout
CEF_PATH=$VSESVIT_SCRATCH/cef CARGO_TARGET_DIR=$VSESVIT_SCRATCH/target-extload cargo build
./fixtures.sh          # keys, extensions, CRX files, $VSESVIT_SCRATCH/extload-env/ids.env
./start-xvfb.sh        # private Xvfb :92 (never the desktop display)
./q4-reload.sh         # any q*.sh; each starts and quits the spike itself
./kill.sh              # kill every spike process
```

`run.sh` starts the browser with `env -i`, a scratch `HOME`, a private D-Bus session and lavapipe.
Spike switches beyond spike-core's: `SPIKE_ROOT` (root_cache_path), `SPIKE_LOAD_EXT` (dirs for
`--load-extension`), `SPIKE_CDP_PIPE=1`, `SPIKE_REPORT_PORT`.
