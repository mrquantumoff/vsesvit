#!/bin/bash
# Q2c: a Vsesvit-owned helper extension with "management" enabling/disabling/uninstalling others.
source /tmp/vsesvit-scratch/spike-ext-load/drive.sh
X=$E/exts; A=fkahekhmpoaldhodgancfgmmjbghngpp; B=fbnpklkbdgiepoopnmaihkhjikacmjci; G=gnnilbihpkjoolbjmkaifmcmbblajkdp
A1=$X/$A/1.0; BD=$X/$B/1.0; GD=$X/$G/1.0
R=$E/root-agent
OUT=$E/shots; mkdir -p $OUT
CTL_WAIT=1.2
show() { grep -E "cdp> |report: \{\"ext\":\"agent\"|console id=[0-9]+ vsesvit-agent|on_after_created|\"event\":\"(sw-start|onInstalled)\"" $L | sed -E 's/"t":[0-9]+,?//; s/"permissions":\[[^]]*\],?//g; s/"hostPermissions":\[[^]]*\],?//g' | cut -c1-600; }
echo "######## run 1"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1,$BD,$GD SPIKE_URLS=$SITE/index.html; sleep 1.5
ctl new chrome-extension://$G/agent.html; sleep 1.5
ctl js "agent.run('get', '$A')"
ctl js "agent.run('setEnabled', '$A', false)"
ctl switch 0; ctl reload; sleep 1.5
ctl "js console.log('after-disable repA=' + document.documentElement.dataset.repA + ' repB=' + document.documentElement.dataset.repB)"
ctl switch 1
ctl js "agent.run('setEnabled', '$A', true)"
ctl js "agent.run('setEnabled', '$A', false)"
echo "--- uninstall B without a user gesture (execute_java_script), then with one (CDP userGesture)"
ctl js "agent.run('uninstall', '$B', {showConfirmDialog: false})"
ctl extpageeval $G "agent.run('uninstall', '$B', {showConfirmDialog: false})"
sleep 1.5; import -display :92 -window root $OUT/agent-uninstall-other.png
ctl js "agent.run('getAll')"
show
grep -E "after-disable" $L | sed 's/^.*console id=[0-9]* /  page: /'
quit; cp $L $E/q2-agent-run1.log
echo "  prefs after run1: $(jq -c '[.extensions.settings | to_entries[] | select(.value.location!=5) | {id:.key[0:6], loc:.value.location, dr:.value.disable_reasons, state:.value.state}]' $R/Default/Preferences)"
echo "  LES: $(ls "$R/Default/Local Extension Settings" | tr '\n' ' ')"
echo "######## run 2: restart with the same --load-extension"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1,$BD,$GD SPIKE_URLS=$SITE/index.html; sleep 2.5
ctl "js console.log('run2 repA=' + document.documentElement.dataset.repA + ' repB=' + document.documentElement.dataset.repB)"
show; grep -E "run2 repA" $L | sed 's/^.*console id=[0-9]* /  page: /'
quit; cp $L $E/q2-agent-run2.log
echo "  prefs after run2: $(jq -c '[.extensions.settings | to_entries[] | select(.value.location!=5) | {id:.key[0:6], loc:.value.location, dr:.value.disable_reasons}]' $R/Default/Preferences)"
