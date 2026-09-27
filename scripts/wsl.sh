#!/usr/bin/env bash
# Run a cargo command for the Linux build inside WSL, from Windows (Git Bash) or from WSL.
# Sources stay on the Windows checkout; build output goes to the WSL filesystem, which is
# much faster than building on /mnt/c. Each checkout gets its own target dir.
# Usage: scripts/wsl.sh build | test | run -- [args]
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if grep -qi microsoft /proc/version 2>/dev/null; then
  cd "$here"
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/vsesvit-target/$(basename "$here")-$(printf '%s' "$here" | md5sum | cut -c1-8)}"
  exec cargo "$@"
fi
win_path="$(cygpath -w "$here")"
wsl_path="$(MSYS_NO_PATHCONV=1 wsl.exe -d Ubuntu -e wslpath -a "$win_path" | tr -d '\r')"
MSYS_NO_PATHCONV=1 exec wsl.exe -d Ubuntu -e bash -lc "cd '$wsl_path' && scripts/wsl.sh $(printf '%q ' "$@")"
