#!/bin/bash
# Q1: does Enhanced Safe Browsing (or developer mode) gate --load-extension?
source "$(dirname "$0")/drive.sh"
A1=$X/$A/1.0; B=$X/$B/1.0
R=$E/root-esb
start SPIKE_ROOT=$R SPIKE_LOAD_EXT=$A1,$B SPIKE_URLS=$SITE/index.html; sleep 3
ctl pref extensions.ui.developer_mode; ctl pref safebrowsing.enhanced; ctl pref safebrowsing.enabled
ctl setpref safebrowsing.enhanced true; ctl pref safebrowsing.enhanced
echo "run1 reports: $(grep -c "report: {" $L)"; grep -E "^\[.*\] (pref|setpref)" $L
quit
start SPIKE_ROOT=$R SPIKE_LOAD_EXT=$A1,$B SPIKE_URLS=$SITE/index.html; sleep 4
ctl pref safebrowsing.enhanced
ctl "js console.log(\"repA=\" + document.documentElement.dataset.repA)"
echo "run2 (ESB on) reports: $(grep -c "report: {" $L)"; grep -E "^\[.*\] pref|console id=1 repA" $L
ctl setpref safebrowsing.enhanced false
quit
start SPIKE_ROOT=$R SPIKE_LOAD_EXT=$A1,$B SPIKE_URLS=$SITE/index.html; sleep 4
echo "run3 (ESB off again) reports: $(grep -c "report: {" $L)"; grep -E "report: \{" $L | sed -E "s/^.*report: //; s/  \(origin.*$//" | jq -c "select(.event==\"sw-start\") | {ext,localBefore}"
quit
