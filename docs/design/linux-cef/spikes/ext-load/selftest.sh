#!/bin/bash
# End-to-end check of the spike on the private display. Prints PASS/FAIL lines.
source /tmp/vsesvit-scratch/spike/drive.sh
OUT=/tmp/vsesvit-scratch/core-env/selftest; mkdir -p $OUT; rm -f $OUT/*
P=http://127.0.0.1:8765
pass=0; fail=0
check() { if eval "$2"; then echo "PASS $1"; pass=$((pass+1)); else echo "FAIL $1"; fail=$((fail+1)); fi; }
/tmp/vsesvit-scratch/spike/kill.sh >/dev/null
(/tmp/vsesvit-scratch/spike/runwait.sh SPIKE_DIALOGS=shell "SPIKE_URLS=$P/page1.html $P/page2.html https://example.org" &)
sleep 5
check "3 tabs created as children of the host window" '[ $(grep -c "(ours=true)" $L) -eq 3 ]'
check "titles reported" 'grep -q "title id=1 \"Page One\"" $L && grep -q "title id=2 \"Page Two\"" $L && grep -q "Example Domain" $L'
check "favicon reported" 'grep -q "favicon id=1 \[\"$P/icon.png\"\]" $L'
WX=$($X find "Vsesvit spike" | sed -E 's/.*\+([0-9]+)\+([0-9]+) .*/\1 \2/'); set -- $WX; WX0=$1; WY0=$2
ctl "switch 0"; sleep 0.8
mark; $K click $((WX0+200)) $((WY0+291)); sleep 0.4; $K move $((WX0+540)) $((WY0+20)); $K type abc; sleep 0.4
check "page click focuses page; keys reach page while pointer is over the chrome" 'since | grep -q "keydown key=c"'
check "X focus is on the CEF child" '[ "$($X focus)" = "0x400003" ]'
mark; $K key ctrl+l; sleep 0.4; $K move $((WX0+600)) $((WY0+500)); $K type zz; sleep 0.4
check "Ctrl+L in page -> omnibox (on_pre_key_event)" 'since | grep -q "action FocusOmnibox from page"'
check "omnibox keys do not leak to page under the pointer" '! since | grep -q "keydown key=z"'
check "X focus on GPUI focus proxy" '[ "$($X focus)" = "$($X tree e00001 | grep gpui-focus-proxy | awk "{print \$1}")" ]'
ctl "state"; check "GPUI window stays active" 'grep "state:" $L | tail -1 | grep -q "gpui_active=true"'
mark; $K click $((WX0+200)) $((WY0+291)); sleep 0.3; $K key ctrl+t; sleep 1; check "Ctrl+T -> shell new tab, no Chrome window" 'since | grep -q "action NewTab" && [ $($X tree | grep -E "^0x" | grep VIEWABLE | grep -vc "1x1+\|1600x1000\|Vsesvit") -eq 0 ]'
mark; $K key ctrl+w; sleep 0.8; check "Ctrl+W -> shell close tab" 'since | grep -q "action CloseTab" && since | grep -q on_before_close'
ctl "switch 0"; sleep 0.5
mark; $K click $((WX0+70)) $((WY0+171)); sleep 1.2; check "window.open -> child tab with opener" 'since | grep -q "popup=true" && since | grep -q "opener=present"'
ctl "switch 0"; sleep 0.5
mark; $K click $((WX0+645)) $((WY0+171)); sleep 0.6; check "alert -> JsDialogHandler (Chrome style)" 'since | grep -q "page: alert returned"'
mark; $K click $((WX0+332)) $((WY0+207)); sleep 0.6; check "file input -> DialogHandler" 'since | grep -q "page: file -> hostname"'
mark; $K click $((WX0+645)) $((WY0+207)); sleep 0.6; check "geolocation -> PermissionHandler prompt" 'since | grep -q "on_show_permission_prompt"'
ctl "menu shape"; sleep 0.5; ctl "shotraw $OUT/overlay-shape.png"; ctl "menu off"
check "screenshot written" '[ -s $OUT/overlay-shape.png ]'
ctl "resize 1100 700"; sleep 0.6; check "child follows window resize" 'grep "configure child" $L | tail -1 | grep -q "1100x"'
CTL_WAIT=3 ctl quit
check "clean shutdown, exit 0" 'grep -q "exit_code=0" $L && grep -q "cef shutdown" $L'
check "no processes left" '[ $(pgrep -fc "^/tmp/vsesvit-scratch/target-core/debug/vsesvit-spike") -eq 0 ]'
echo "== $pass passed, $fail failed"
