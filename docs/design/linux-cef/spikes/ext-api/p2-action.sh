#!/bin/bash
# P2: what can open or invoke the action without a toolbar. Tab 1 (localhost) is outside spy's
# host permission; a fresh tab load has no activeTab grant.
#   1 _execute_action (Alt+Shift+U, XTEST) alone: popup? activeTab grant?
#   2 chrome.action.openPopup() after chrome.windows.update({focused: true})
#   3 CDP Extensions.triggerAction on the "tab" target (Target.getTargets filter type=tab)
source "$(dirname "$0")/drive.sh"
CTL_WAIT=0.8
mkdir -p $E/shots
DISPLAY=$DPY xwininfo -root >/dev/null 2>&1 || { (setsid $HERE/start-display.sh &); sleep 2; }
pgrep -f "harness/server.py 8793" >/dev/null || { (cd $HERE && setsid python3 -I harness/server.py 8793 > $E/server.out 2>&1 &); sleep 0.5; }
URLS="http://127.0.0.1:8793/index.html http://localhost:8793/page2.html" WAIT=6 $HERE/restart.sh
SPY=$(jq -r .spy.id $E/ids.json)
sw() { cdp eval sw:spy "$1" | jq -c '.result // .error'; }
LH=$(sw "chrome.tabs.query({url: 'http://localhost/*'}).then(t => t[0].id)")
echo "spy=$SPY localhost tab=$LH"

echo "=== _execute_action alone on the localhost tab"
ctl switch 1; ctl page; sleep 0.5
mark; $X key alt+shift+u; sleep 2
since | grep -E "rootwatch: (Create|Map)|on_after_created|chrome_command|on_before_popup" | cut -c1-200
echo "  popup targets: $(cdp targets | jq -c '[.[] | select(.url | test("popup"))]')"
echo "  spy scripting into the tab afterwards: $(sw "tryInject($LH)")"
shot $E/shots/execute-action-alone.png

echo "=== openPopup after windows.update({focused: true})"
W=$(sw "chrome.tabs.get($LH).then(t => t.windowId)")
mark
echo "  update: $(sw "chrome.windows.update($W, {focused: true}).then(w => JSON.stringify({id: w.id, focused: w.focused}), e => 'rejected ' + e.message)")"
echo "  openPopup({windowId}): $(sw "chrome.action.openPopup({windowId: $W}).then(() => 'resolved', e => 'rejected ' + e.message)")"
echo "  openPopup(): $(sw "chrome.action.openPopup().then(() => 'resolved', e => 'rejected ' + e.message)")"
sleep 2; since | grep -E "rootwatch|on_after_created" | cut -c1-200 | head
echo "  popup targets: $(cdp targets | jq -c '[.[] | select(.url | test("popup"))]')"
shot $E/shots/openpopup-focused.png

echo "=== CDP Extensions.triggerAction"
cdp browser Target.getTargets '{"filter":[{"type":"tab"}]}' | jq -c '[.targetInfos[] | {type, url, id: .targetId[0:8]}]'
TID=$(cdp browser Target.getTargets '{"filter":[{"type":"tab"}]}' | jq -r '.targetInfos[] | select(.url | test("localhost")) | .targetId')
echo "  tab target: $TID"
mark
cdp browser Extensions.triggerAction "{\"id\":\"$SPY\",\"targetId\":\"$TID\"}" | jq -c .
sleep 3
echo "  browser alive: $(pgrep -fc "^$TARGET/debug/vsesvit-spike$")"
since | grep -vE "MESA|lavapipe|^\s*$" | tail -12 | cut -c1-220
grep -E "Received signal|SIGSEGV|crash|exit_code" $L | tail -3
ctl quit; sleep 3
for p in $(pgrep -f '^node harness/cdp[.]mjs daemon'); do kill $p; done
$HERE/kill.sh
