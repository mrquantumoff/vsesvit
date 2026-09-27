#!/usr/bin/env bash
# Copies each wave-1 unit's owned files from its isolated copy back into the repo.
set -euo pipefail
WS="$1"
repo="$(cd "$(dirname "$0")/.." && pwd)"
take() { local unit="$1"; shift; for p in "$@"; do
  src="$WS/$unit/$p"
  if [ -d "$src" ]; then mkdir -p "$repo/$p"; (cd "$src" && tar --exclude=./target -cf - .) | (cd "$repo/$p" && tar -xf -);
  elif [ -f "$src" ]; then mkdir -p "$(dirname "$repo/$p")"; cp "$src" "$repo/$p";
  else echo "missing: $unit/$p" >&2; exit 1; fi; done; }
core=crates/vsesvit-core
take core-data $core/src/{lib.rs,crdt.rs,db.rs,bookmarks.rs,history.rs,session.rs,prefs.rs,search.rs,ext_storage.rs,sync.rs,schema.sql} $core/src/extensions/sync_table.rs $core/tests/convergence.rs
for f in "$WS"/core-data/$core/tests/core_*.rs; do take core-data "$core/tests/$(basename "$f")"; done
take core-ext $core/src/extensions/{mod.rs,install.rs,crx.rs,manifest.rs,schema.sql} $core/src/testkit tests/fixtures/keys
for f in "$WS"/core-ext/$core/tests/extensions_*.rs; do take core-ext "$core/tests/$(basename "$f")"; done
rm -rf "$repo/crates/vsesvit-webext" "$repo/crates/vsesvit-gtk" "$repo/crates/vsesvit-winui"
take webext crates/vsesvit-webext
take gtk crates/vsesvit-gtk
take winui crates/vsesvit-winui
rm -rf "$repo/tests/fixtures/extensions/probe/_metadata"
