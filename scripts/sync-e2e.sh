#!/usr/bin/env bash
# Builds the sync server and runs crates/vsesvit-sync/tests/e2e.rs against it: real profiles sign
# in through a mock OpenID Connect provider and sync through the real server on SQLite.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"
cargo build --manifest-path server/Cargo.toml
bin="$here/server/target/debug/vsesvit-sync-server"
[[ -x "$bin" ]] || bin="$bin.exe"
VSESVIT_SYNC_SERVER_BIN="$bin" cargo test -p vsesvit-sync "$@"
