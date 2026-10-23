#!/bin/bash
# P1: the action popup without Chrome's toolbar. Tab 0 is 127.0.0.1 (inside spy's host permission),
# tab 1 is localhost (outside it), so scripting into tab 1 works only with an activeTab grant.
#   1 popup page in a shell-made child browser, Alloy and Chrome style (auto-resized, top-right)
#   2 chrome.action.openPopup() from the worker
#   3 keyboard commands through XTEST: spy-cmd (Alt+Shift+Y), _execute_action (Alt+Shift+U)
#   4 a context-menu item run through the shell's menu, then the popup again
#   5 CDP Extensions.triggerAction on the tab target (last: it may take the browser down)
source "$(dirname "$0")/drive.sh"
CTL_WAIT=0.8
mkdir -p $E/shots
DISPLAY=$DPY xwininfo -root >/dev/null 2>&1 || { (setsid $HERE/start-display.sh &); sleep 2; }
pgrep -f "harness/server.py 8793" >/dev/null || { (cd $HERE && setsid python3 -I harness/server.py 8793 > $E/server.out 2>&1 &); sleep 0.5; }
URLS="http://127.0.0.1:8793/index.html http://localhost:8793/page2.html" WAIT=6 $HERE/restart.sh SPIKE_MENU=shell
SPY=$(jq -r .spy.id $E/ids.json)
sw() { cdp eval sw:spy "$1" | jq -c '.result // .error'; }
pg() { cdp eval "page:$1" "$2" | jq -c '.result // .error'; }
inject="chrome.scripting.executeScript({target: {tabId: TAB}, func: () => location.href}).then(r => 'ok ' + r[0].result, e => 'err ' + e.message)"
LH=$(sw "chrome.tabs.query({url: 'http://localhost/*'}).then(t => t[0].id)")
echo "spy=$SPY localhost tab=$LH"
echo "--- tab model seen by spy"; sw "report()"
echo "--- baseline: spy scripting into the localhost tab"; sw "tryInject($LH)"
ctl switch 1; ctl page

popup() { # popup alloy|chrome
  echo "=== popup page in a shell child browser, $1 style"
  mark
  ctl popuphost $1 chrome-extension://$SPY/popup.html; sleep 3
  since | grep -E "gpui: popup|auto_resize|on_after_created" | cut -c1-260
  echo "  popup sees: $(pg popup.html "document.getElementById('out').textContent" | jq -r . | jq -c '{activeCurrent, current, getCurrentTab, viewsPopup, viewsTab}')"
  echo "  popup scripting into localhost tab: $(pg popup.html "${inject/TAB/$LH}")"
  shot $E/shots/popup-$1$2.png
  pg popup.html "window.close(), 'closed'" >/dev/null; sleep 1
  since | grep -E "popup-host browser id=.* closed" | cut -c1-200
  ctl popupclose
}
popup alloy; popup chrome

echo "=== chrome.action.openPopup() from the worker"
mark
echo "  no args: $(sw "chrome.action.openPopup().then(() => 'resolved', e => 'rejected ' + e.message)")"
echo "  last-focused window: $(sw "chrome.windows.getLastFocused().then(w => chrome.action.openPopup({windowId: w.id}).then(() => 'resolved ' + w.id + ' ' + w.type, e => 'rejected ' + e.message))")"
sleep 2; since | grep -E "rootwatch: (Create|Map)|on_after_created" | cut -c1-200
cdp targets | jq -c '[.[] | select(.url | test("popup"))]'
shot $E/shots/openpopup.png

echo "=== keyboard commands into the focused localhost page (XTEST)"
ctl switch 1; ctl page; sleep 0.5
$X key alt+shift+y; sleep 1.5
echo "  spy-cmd: $(sw "JSON.stringify(cmds)")"
mark; $X key alt+shift+u; sleep 2
since | grep -E "rootwatch: (Create|Map)|on_after_created|on_pre_key|chrome_command" | cut -c1-200
echo "  popup targets after _execute_action: $(cdp targets | jq -c '[.[] | select(.url | test("popup"))]')"
shot $E/shots/execute-action.png

echo "=== context-menu item through the shell's menu"
read WXY <<< "$($X find 'Vsesvit spike' | head -1 | awk '{print $2}')"
OX=$(echo $WXY | cut -d+ -f2); OY=$(echo $WXY | cut -d+ -f3)
mark; $X click $((OX + 500)) $((OY + 400)) 3; sleep 1.5
since | grep -E "run_context_menu|Spy" | cut -c1-160 | head -8
ctl menupick "Spy child page"; sleep 1.5
echo "  clicks: $(sw "JSON.stringify(clicks)")"
popup chrome -after-menu

echo "=== CDP Extensions.triggerAction on the localhost tab target"
TID=$(cdp browser Target.getTargets | jq -r '.targetInfos[] | select(.type == "tab" and (.url | test("localhost"))) | .targetId')
mark
cdp browser Extensions.triggerAction "{\"id\":\"$SPY\",\"targetId\":\"$TID\"}"
sleep 3
echo "  browser alive: $(pgrep -fc "^$TARGET/debug/vsesvit-spike$")"
since | grep -vE "MESA|lavapipe" | tail -15 | cut -c1-200
ctl quit; sleep 3
for p in $(pgrep -f '^node harness/cdp[.]mjs daemon'); do kill $p; done
$HERE/kill.sh
