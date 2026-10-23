#!/bin/bash
# Q6: does Chromium's ExtensionUpdater update a packed extension in CEF? D's manifest carries
# update_url http://127.0.0.1:8792/updates/D.xml; the report server logs every GET under /updates/.
#   run 1-2  D 1.0 from External JSON (external_crx); D.xml offers 1.0, then 2.0 + requestUpdateCheck
#   run 3    fresh profile, D.xml offers 2.0 from the start; no poke (startup check only)
#   run 4    fresh profile, External JSON with external_update_url only (no CRX on disk)
source "$(dirname "$0")/drive.sh"
CTL_WAIT=1.2
offer() { cat > $E/site/updates/D.xml <<EOF
<?xml version='1.0' encoding='UTF-8'?>
<gupdate xmlns='http://www.google.com/update2/response' protocol='2.0'>
  <app appid='$D'><updatecheck codebase='http://127.0.0.1:8792/updates/repD-$1.crx' version='$1' /></app>
</gupdate>
EOF
}
show() { grep -E 'report: GET|"ext":"D".*"event":"(onInstalled|sw-start|requestUpdateCheck)|cdp> sweval' $L | sed -E 's/^\[ *([0-9.]+)\] /\1 /; s/"t":[0-9]+,?//; s/"(local|sync|session)Before":\{[^}]*\},?//g; s/"id":"[a-p]{32}",?//' | cut -c1-400; }
pr() { echo "  prefs D: $(jq -c --arg d $D '.extensions.settings[$d] | if . then {loc:.location, ver:.manifest.version, path, dr:.disable_reasons, from_webstore, update_url:.manifest.update_url} else null end' $R/Default/Preferences)"; }
json() { mkdir -p "$R/External Extensions"; echo "$1" > "$R/External Extensions/$D.json"; }

R=$E/root-upd1; rm -rf $R
json "{\"external_crx\": \"$E/crx/repD-1.0.crx\", \"external_version\": \"1.0\"}"; offer 1.0
echo "######## run 1: D 1.0 installed from External JSON; D.xml offers 1.0"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_URLS=$SITE/index.html; sleep 15
echo "--- D.xml now offers 2.0; D calls chrome.runtime.requestUpdateCheck()"
offer 2.0
ctl sweval $D "self.checkUpdate()"; sleep 10
show; quit; cp $L $E/q6-run1.log; pr
echo "######## run 2: restart (JSON still says external_version 1.0)"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_URLS=$SITE/index.html; sleep 8
show; quit; cp $L $E/q6-run2.log; pr

R=$E/root-upd3; rm -rf $R
json "{\"external_crx\": \"$E/crx/repD-1.0.crx\", \"external_version\": \"1.0\"}"; offer 2.0
echo "######## run 3: fresh profile, D.xml offers 2.0 from the start; wait 75 s, no poke"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_URLS=$SITE/index.html; sleep 75
show; quit; cp $L $E/q6-run3.log; pr

R=$E/root-upd4; rm -rf $R
json "{\"external_update_url\": \"http://127.0.0.1:8792/updates/D.xml\"}"; offer 1.0
echo "######## run 4: fresh profile, External JSON with external_update_url only; wait 30 s"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_URLS=$SITE/index.html; sleep 30
show; quit; cp $L $E/q6-run4.log; pr
ls $R/Default/Extensions/$D 2>/dev/null
