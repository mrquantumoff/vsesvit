#!/bin/bash
# Q2d: when does an external-CRX update (external_version bump) actually apply?
source "$(dirname "$0")/drive.sh"

R=$E/root-ext2; mkdir -p "$R/External Extensions"
show() { grep -E "report: \{\"ext\":\"C\"" $L | grep -E '"event":"(onInstalled|sw-start)' | sed -E 's/^\[ *([0-9.]+)\] report: /\1 /; s/"t":[0-9]+,?//; s/"commands".*$//' | cut -c1-300; }
prefs() { echo "  prefs: $(jq -c '.extensions.settings["'$C'"] | {loc:.location, path, ver:.manifest.version, delayed:(.delayed_install_info|if . then {path, ver:.manifest.version, reason:.delay_install_reason} else null end)}' $R/Default/Preferences)"; }
printf '{"external_crx": "%s", "external_version": "1.0"}' $E/crx/repC-1.0.crx > "$R/External Extensions/$C.json"
echo "######## run 1 (install 1.0)"; start SPIKE_ROOT=$R SPIKE_URLS=$SITE/index.html; sleep 3; show; quit; prefs
printf '{"external_crx": "%s", "external_version": "2.0"}' $E/crx/repC-2.0.crx > "$R/External Extensions/$C.json"
echo "######## run 2 (JSON says 2.0): 50 s, page tab open"; start SPIKE_ROOT=$R SPIKE_URLS=$SITE/index.html; sleep 50; show; quit; cp $L $E/q2-ext2-run2.log; prefs
echo "######## run 3 (restart)"; start SPIKE_ROOT=$R SPIKE_URLS=$SITE/index.html; sleep 4; show; quit; cp $L $E/q2-ext2-run3.log; prefs
