#!/usr/bin/env bash
# Builds the sync server and runs crates/vsesvit-sync/tests/e2e.rs against it: real profiles sign
# in through a mock OpenID Connect provider and sync through the real server on SQLite.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"
# Cargo names the binary it built, wherever CARGO_TARGET_DIR or a config's build.target-dir put it.
bin="$(cargo build --manifest-path server/Cargo.toml --message-format=json-render-diagnostics \
  | sed -nE 's/.*"executable":"([^"]*vsesvit-sync-server(\.exe)?)".*/\1/p' | sed 's/\\\\/\\/g')"
[[ -n "$bin" ]] || { echo "cargo built no vsesvit-sync-server" >&2; exit 1; }
VSESVIT_SYNC_SERVER_BIN="$bin" cargo test -p vsesvit-sync "$@"
