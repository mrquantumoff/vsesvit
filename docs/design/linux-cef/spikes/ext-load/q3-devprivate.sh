#!/bin/bash
# Q3/Q4: chrome://extensions in a Chrome-style child; per-extension settings via its developerPrivate API.
source /tmp/vsesvit-scratch/spike-ext-load/drive.sh
X=$E/exts; A=fkahekhmpoaldhodgancfgmmjbghngpp; B=fbnpklkbdgiepoopnmaihkhjikacmjci; G=gnnilbihpkjoolbjmkaifmcmbblajkdp
A1=$X/$A/1.0; BD=$X/$B/1.0; GD=$X/$G/1.0
R=$E/root-q3; OUT=$E/shots
CTL_WAIT=1.2
INFO="chrome.developerPrivate.getExtensionsInfo({includeDisabled:true, includeTerminated:true}).then(r => console.log('devp info ' + JSON.stringify(r.filter(e => e.location !== 'COMPONENT').map(e => ({id:e.id.slice(0,6), location:e.location, state:e.state, incognito:e.incognitoAccess, mayModify:e.userMayModify, hostAccess:e.permissions?.runtimeHostPermissions?.hostAccess, hosts:e.permissions?.runtimeHostPermissions?.hosts?.map(h=>[h.host,h.granted]), commands:e.commands?.map(c=>[c.name,c.keybinding,c.isActive,c.scope])})))), e => console.log('devp info err ' + e))"
call() { ctl js "chrome.developerPrivate.$1.then(r => console.log('devp $2 ok ' + JSON.stringify(r ?? null)), e => console.log('devp $2 err ' + e.message))"; }
dump() { ctl switch 1; ctl js "$INFO"; ctl switch 0; ctl reload; sleep 1.5
  ctl "js console.log('page: repA=' + document.documentElement.dataset.repA + ' repB=' + document.documentElement.dataset.repB)"
  ctl sweval $A "chrome.commands.getAll().then(c => JSON.stringify(c.map(x => [x.name, x.shortcut])))"
  ctl sweval $B "chrome.permissions.getAll().then(JSON.stringify)"; ctl switch 1; }
echo "######## run 1"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1,$BD,$GD SPIKE_URLS=$SITE/index.html; sleep 1
ctl new chrome://extensions; sleep 2.5
import -display :92 -window root $OUT/chrome-extensions.png
ctl js "$INFO"
call "updateExtensionConfiguration({extensionId: '$A', incognitoAccess: true})" incog
call "updateExtensionCommand({extensionId: '$A', commandName: 'rep-cmd', keybinding: 'Alt+Shift+U'})" cmdA
call "updateExtensionCommand({extensionId: '$B', commandName: 'rep-cmd', keybinding: 'Ctrl+Shift+9'})" cmdB
call "updateExtensionConfiguration({extensionId: '$B', hostAccess: 'ON_CLICK'})" hostB
sleep 1; import -display :92 -window root $OUT/chrome-extensions-after.png
dump
grep -E "devp |page: repA|cdp> sw" $L | sed -E 's/^.*console id=[0-9]+ //; s/^\[ *[0-9.]+\] //' | cut -c1-1200
quit; cp $L $E/q3-run1.log
echo "  prefs: $(jq -c '[.extensions.settings | to_entries[] | select(.value.location==8) | {id:.key[0:6], incognito:.value.incognito, withholding:.value.withholding_permissions, runtime_granted:.value.runtime_granted_permissions, commands:.value.commands}]' $R/Default/Preferences)"
echo "  extensions.commands: $(jq -c '.extensions.commands' $R/Default/Preferences)"
echo "######## run 2: restart, same --load-extension"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1,$BD,$GD SPIKE_URLS=$SITE/index.html; sleep 1
ctl new chrome://extensions; sleep 2.5
dump
grep -E "devp |page: repA|cdp> sw" $L | sed -E 's/^.*console id=[0-9]+ //; s/^\[ *[0-9.]+\] //' | cut -c1-1200
quit; cp $L $E/q3-run2.log
