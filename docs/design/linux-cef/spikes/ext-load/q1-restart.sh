#!/bin/bash
# Q1: --load-extension across restarts: storage, onInstalled, version bump, drop/re-add.
source "$(dirname "$0")/drive.sh"
A1=$X/$A/1.0; A2=$X/$A/2.0
B=$X/$B/1.0; P=$X/$P/1.0.0
ROOT=${ROOT:-$E/root1}
phase() { # phase NAME DIRS
  echo "=== phase $1: --load-extension=${2//$X\//}"
  start SPIKE_ROOT=$ROOT SPIKE_LOAD_EXT=$2 SPIKE_URLS=$SITE/index.html; sleep 4
  ctl "js console.log(\"probe=\" + document.documentElement.dataset.vsesvitProbe + \" visits=\" + document.documentElement.dataset.vsesvitVisits)"; sleep 0.5
  reports | grep -E '"event":"(onInstalled|sw-start|onStartup)"' | sed -E 's/^[0-9.]+ //; s/  \(origin.*$//' | jq -c '{ext,version,event,reason,previousVersion,localBefore,syncBefore,sessionBefore}'
  grep -E 'console id=1 probe=' $L | sed 's/^.*console id=1 /  page: /'
  quit; cp $L $E/q1-$1.log
  echo "  Local Extension Settings: $(ls $ROOT/Default/Local\ Extension\ Settings | tr '\n' ' ')"
  echo "  prefs: $(jq -c '[.extensions.settings | to_entries[] | select(.value.location==8) | {id:.key[0:6], path:(.value.path|sub(".*/exts/";"")), dr:.value.disable_reasons}]' $ROOT/Default/Preferences)"
}
phase r2-same "$A1,$B,$P"
phase r3-bumpA "$A2,$B,$P"
phase r4-dropB "$A2,$P"
phase r5-addB "$A2,$B,$P"
phase r6-downgradeA "$A1,$B,$P"
