# Helpers for the item scripts (sourced after env.sh).
# root PROFILE LABEL: the profile's CEF root_cache_path.
root() { echo $XW/runs/$2/$1/root; }
# xjson PROFILE LABEL NAME VERSION: archive NAME-VERSION.crx in the profile and point an
# External Extensions JSON at it (the design's per-profile install route, §5.2).
xjson() {
  local P=$1 L=$2 N=$3 V=$4 R=$XW/runs/$2/$1
  local id=${!N}
  mkdir -p $R/archive "$R/root/External Extensions"
  cp $FX/crx/$N-$V.crx $R/archive/$N-$V.crx
  printf '{"external_crx": "%s", "external_version": "%s"}\n' "$R/archive/$N-$V.crx" "$V" > "$R/root/External Extensions/$id.json"
}
xjson_rm() { rm -f "$XW/runs/$2/$1/root/External Extensions/${!3}.json"; }
# bridge PROFILE: open the helper's bridge page in a hidden Alloy browser (idempotent).
xbridge() { c $1 "@15 hidden bridge alloy chrome-extension://$H/bridge.html"; }
# call PROFILE JSON: vsesvitBridge.call(JSON) on the bridge page.
xcall() { c $1 "@20 ev bridge vsesvitBridge.call($2)"; }
logf() { echo $XW/runs/$2/$1/spike.log; }
reports() { cat $XW/runs/$2/$1/reports.jsonl 2>/dev/null; }
# waitlog PROFILE LABEL REGEX SECONDS
waitlog() { local f=$(logf $1 $2); for i in $(seq $(( $4 * 10 ))); do grep -qE "$3" $f && return 0; sleep 0.1; done; return 1; }
# prefs PROFILE LABEL PYEXPR: evaluate against the profile's Preferences JSON (as p).
prefs() { python3 -I $XW/src/tools/prefs.py "$XW/runs/$2/$1/root/Default" $FX/ids.json; }
say() { echo "## $*"; }
