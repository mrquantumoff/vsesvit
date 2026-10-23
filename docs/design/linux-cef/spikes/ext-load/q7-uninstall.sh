#!/bin/bash
# Q7: uninstall without Chrome's "Remove ...?" dialog.
#   run 1  chrome.management.uninstallSelf({showConfirmDialog:false}) evaluated (CDP) inside the
#          target extension: external CRX C, command-line A; then the agent's management.uninstall(B)
#          for comparison (the dialog the earlier run saw)
#   run 2  restart with the same JSON and flags: does C come back, does A?
#   run 3  fresh profile: C from External JSON, then the JSON file is deleted before a restart
source "$(dirname "$0")/drive.sh"
CTL_WAIT=1.2
A1=$X/$A/1.0; BD=$X/$B/1.0; GD=$X/$G/1.0
mkdir -p $E/shots
wins() { DISPLAY=$DPY xwininfo -root -tree | grep -cE '^ +0x'; }
self_uninstall() { ctl sweval $1 "chrome.management.uninstallSelf({showConfirmDialog: false}).then(() => 'resolved', e => 'rejected ' + e.message)"; sleep 2; }
show() { grep -E "cdp> |vsesvit-agent|\"ext\":\"agent\"" $L | sed -E 's/^\[ *[0-9.]+\] //; s/"t":[0-9]+,?//' | cut -c1-500; }
les() { echo "  LES: $(ls "$R/Default/Local Extension Settings" 2>/dev/null | tr '\n' ' ')"; }
pr() { echo "  prefs: $(jq -c --arg a $A --arg b $B --arg c $C '[.extensions.settings | to_entries[] | select(.key==$a or .key==$b or .key==$c) | {id:.key[0:6], loc:.value.location, state:.value.state, dr:.value.disable_reasons}]' $R/Default/Preferences)  external_uninstalls: $(jq -c '.extensions.external_uninstalls' $R/Default/Preferences)"; les; }
json() { mkdir -p "$R/External Extensions"; printf '{"external_crx": "%s", "external_version": "1.0"}' $E/crx/repC-1.0.crx > "$R/External Extensions/$C.json"; }

R=$E/root-uninst1; rm -rf $R; json
echo "######## run 1"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1,$BD,$GD SPIKE_URLS=$SITE/index.html; sleep 3
les; echo "  X windows: $(wins)"
self_uninstall $C; echo "  after C uninstallSelf: X windows $(wins)"; shot $E/shots/uninstallself-C.png; les
self_uninstall $A; echo "  after A uninstallSelf: X windows $(wins)"; shot $E/shots/uninstallself-A.png; les
ctl cdp Extensions.getExtensions
ctl new chrome-extension://$G/agent.html; sleep 1.5
ctl extpageeval $G "agent.run('uninstall', '$B', {showConfirmDialog: false})" &
sleep 3; echo "  after agent management.uninstall(B): X windows $(wins)"; shot $E/shots/agent-uninstall-B.png
show; quit; cp $L $E/q7-run1.log; pr

echo "######## run 2: restart, same External JSON and --load-extension"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1,$BD,$GD SPIKE_URLS=$SITE/index.html; sleep 3
ctl cdp Extensions.getExtensions
ctl "js console.log('page: repA=' + document.documentElement.dataset.repA + ' repB=' + document.documentElement.dataset.repB + ' repC=' + document.documentElement.dataset.repC)"
grep -E "cdp> |page: " $L | sed -E 's/^.*console id=[0-9]+ //; s/^\[ *[0-9.]+\] //' | cut -c1-400
quit; pr

R=$E/root-uninst3; rm -rf $R; json
echo "######## run 3: C from External JSON, then the JSON is deleted"
start SPIKE_ROOT=$R SPIKE_URLS=$SITE/index.html; sleep 3; quit; pr
rm "$R/External Extensions/$C.json"
start SPIKE_ROOT=$R SPIKE_URLS=$SITE/index.html; sleep 4
ctl "js console.log('page: repC=' + document.documentElement.dataset.repC)"
grep -E "page: " $L | sed -E 's/^.*console id=[0-9]+ //'
quit; pr
echo "  Extensions/ dir: $(ls $R/Default/Extensions 2>/dev/null | tr '\n' ' ')"

R=$E/root-uninst4; rm -rf $R; json
echo "######## run 4: C disabled by the agent, worker idle: enable, then uninstallSelf from a hidden extension page"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$GD SPIKE_URLS=$SITE/index.html; sleep 3
ctl new chrome-extension://$G/agent.html; sleep 1.5
ctl extpageeval $G "agent.run('setEnabled', '$C', false)"; sleep 1
ctl pageeval chrome-extension://$C/ "typeof chrome"
ctl extpageeval $G "agent.run('setEnabled', '$C', true)"; sleep 40
ctl targets
ctl new chrome-extension://$C/manifest.json; sleep 2
ctl pageeval chrome-extension://$C/ "JSON.stringify([typeof chrome.management, typeof chrome.management?.uninstallSelf])"
echo "  X windows before: $(wins)"
ctl pageeval chrome-extension://$C/ "chrome.management.uninstallSelf({showConfirmDialog: false}).then(() => 'resolved', e => 'rejected ' + e.message)"; sleep 2
echo "  X windows after: $(wins)"; shot $E/shots/uninstallself-hidden-page.png
show; quit; pr
