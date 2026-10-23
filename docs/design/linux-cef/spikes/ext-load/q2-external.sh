#!/bin/bash
# Q2d: "External Extensions/<id>.json" in root_cache_path pointing at a CRX.
source "$(dirname "$0")/drive.sh"
GD=$X/$G/1.0
R=$E/root-ext; OUT=$E/shots; mkdir -p "$R/External Extensions" $OUT
show() { grep -E "report: \{\"ext\":\"(C|agent)\"" $L | grep -E '"event":"(onInstalled|sw-start|management|content)' | sed -E 's/"t":[0-9]+,?//; s/"permissions":\[[^]]*\],?//g; s/"hostPermissions":\[[^]]*\],?//g; s/"offlineEnabled":false,//g' | cut -c1-520; }
prefs() { echo "  prefs: $(jq -c '[.extensions.settings | to_entries[] | select(.value.location!=5 and .value.location!=8) | {id:.key[0:6], loc:.value.location, path:.value.path, ver:.value.manifest.version, dr:.value.disable_reasons, flags:.value.creation_flags}]' $R/Default/Preferences)"; echo "  external_uninstalls: $(jq -c '.extensions.external_uninstalls' $R/Default/Preferences)"; echo "  LES: $(ls "$R/Default/Local Extension Settings" 2>/dev/null | tr '\n' ' ')  Extensions/: $(cd $R/Default/Extensions 2>/dev/null && find . -maxdepth 2 | tr '\n' ' ')"; }
printf '{"external_crx": "%s", "external_version": "1.0"}' $E/crx/repC-1.0.crx > "$R/External Extensions/$C.json"
echo "######## run 1: JSON for C 1.0 present before start"
start SPIKE_ROOT=$R SPIKE_LOAD_EXT=$GD SPIKE_URLS=$SITE/index.html; sleep 5
shot $OUT/external-run1.png
show; quit; cp $L $E/q2-ext-run1.log; prefs
echo "######## run 2: plain restart"
start SPIKE_ROOT=$R SPIKE_LOAD_EXT=$GD SPIKE_URLS=$SITE/index.html; sleep 3
echo "--- now point the JSON at C 2.0 while running; wait 20 s"
printf '{"external_crx": "%s", "external_version": "2.0"}' $E/crx/repC-2.0.crx > "$R/External Extensions/$C.json"
sleep 20
show; quit; cp $L $E/q2-ext-run2.log; prefs
echo "######## run 3: restart after the JSON changed to 2.0"
start SPIKE_ROOT=$R SPIKE_LOAD_EXT=$GD SPIKE_URLS=$SITE/index.html; sleep 5
show; quit; cp $L $E/q2-ext-run3.log; prefs
echo "######## run 4: JSON removed"
mv "$R/External Extensions/$C.json" "$R/C.json.removed"
start SPIKE_ROOT=$R SPIKE_LOAD_EXT=$GD SPIKE_URLS=$SITE/index.html; sleep 5
show; quit; cp $L $E/q2-ext-run4.log; prefs
echo "######## run 5: JSON back (2.0)"
mv "$R/C.json.removed" "$R/External Extensions/$C.json"
start SPIKE_ROOT=$R SPIKE_LOAD_EXT=$GD SPIKE_URLS=$SITE/index.html; sleep 5
show; quit; cp $L $E/q2-ext-run5.log; prefs
