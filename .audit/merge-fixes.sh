#!/usr/bin/env bash
# 3-way merges the review-fix copies into the checkout: base = the commit the copies were
# taken from, theirs = the checkout's working tree (may hold other sessions' uncommitted
# edits), mine = the fix copy. Each unit contributes only the paths it owns.
set -uo pipefail
WS="$1"; BASE_REV="$2"; OUT="$3"   # OUT: scratch dir that receives merged files before they are applied
repo="$(cd "$(dirname "$0")/.." && pwd)"; cd "$repo"
rm -rf "$OUT"; mkdir -p "$OUT"
owned() { # unit path -> 0 if the unit owns the path
  case "$1" in
    fix-core) [[ "$2" == crates/vsesvit-core/* && "$2" != crates/vsesvit-core/src/extensions/mod.rs && "$2" != crates/vsesvit-core/src/extensions/install.rs && "$2" != crates/vsesvit-core/src/extensions/crx.rs && "$2" != crates/vsesvit-core/src/extensions/manifest.rs && "$2" != crates/vsesvit-core/src/extensions/schema.sql && "$2" != crates/vsesvit-core/src/testkit/* && "$2" != crates/vsesvit-core/tests/extensions_* ]] ;;
    fix-ext) [[ "$2" == crates/vsesvit-core/src/extensions/mod.rs || "$2" == crates/vsesvit-core/src/extensions/install.rs || "$2" == crates/vsesvit-core/src/extensions/crx.rs || "$2" == crates/vsesvit-core/src/extensions/manifest.rs || "$2" == crates/vsesvit-core/src/extensions/schema.sql || "$2" == crates/vsesvit-core/src/testkit/* || "$2" == crates/vsesvit-core/tests/extensions_* ]] ;;
    fix-webext) [[ "$2" == crates/vsesvit-webext/* ]] ;;
    fix-gtk) [[ "$2" == crates/vsesvit-gtk/* && "$2" != crates/vsesvit-gtk/src/updates/* ]] ;;
    fix-winui) [[ "$2" == crates/vsesvit-winui/* && "$2" != crates/vsesvit-winui/src/updates.rs && "$2" != crates/vsesvit-winui/src/bindings.rs ]] ;;
  esac
}
for unit in fix-core fix-ext fix-webext fix-gtk fix-winui; do
  while IFS= read -r f; do
    p="${f#./}"; owned "$unit" "$p" || continue
    mine="$WS/$unit/$p"
    if git cat-file -e "$BASE_REV:$p" 2>/dev/null; then
      # Line endings differ between copies (CRLF working tree, LF rewrites); git stores LF
      # under core.autocrlf, so merge all three sides as LF.
      git show "$BASE_REV:$p" | tr -d '\r' > "$OUT/base.tmp"
      tr -d '\r' < "$mine" > "$OUT/mine.tmp"
      cmp -s "$OUT/base.tmp" "$OUT/mine.tmp" && continue          # the unit did not change it
      mkdir -p "$OUT/$(dirname "$p")"
      if [ -f "$p" ]; then tr -d '\r' < "$p" > "$OUT/$p"; else cp "$OUT/base.tmp" "$OUT/$p"; fi
      git merge-file -L theirs -L base -L mine "$OUT/$p" "$OUT/base.tmp" "$OUT/mine.tmp"; rc=$?
      echo "$unit $p merged conflicts=$rc"
    else
      mkdir -p "$OUT/$(dirname "$p")"; cp "$mine" "$OUT/$p"; echo "$unit $p new"
    fi
  done < <(cd "$WS/$unit" && find ./crates -type f -not -path '*/target/*' | sort)
done
rm -f "$OUT/base.tmp" "$OUT/mine.tmp"
# Then: resolve conflicts by hand, regenerate crates/vsesvit-winui/src/bindings.rs from the merged
# bindings.txt with tools/bindgen, copy $OUT over the checkout, and stage only the units' own
# versions (git hash-object + update-index) so other sessions' edits stay unstaged.
