#!/bin/bash
# Q2a: CDP over --remote-debugging-pipe (fds 3/4, in process): Extensions.* and targets.
source /tmp/vsesvit-scratch/spike-ext-load/drive.sh
X=$E/exts; A=fkahekhmpoaldhodgancfgmmjbghngpp; B=fbnpklkbdgiepoopnmaihkhjikacmjci; C=ohhbacgnpjplioghnknahphiamnhjpnj
A1=$X/$A/1.0; A2=$X/$A/2.0; BD=$X/$B/1.0; CD=$X/$C/1.0
R=$E/root-cdp
show() { grep -E "cdp> |cdp event|report: \{|console id=[0-9]+ vsesvit|main: remote" $L | sed -E 's/"t":[0-9]+,?//' | cut -c1-900; }
CTL_WAIT=1.2
echo "######## run 1: --load-extension=A1 + CDP pipe"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1 SPIKE_URLS=$SITE/index.html; sleep 2
ctl cdp Browser.getVersion
ctl targets
ctl cdp Extensions.getExtensions
ctl cdp Extensions.loadUnpacked "{\"path\":\"$BD\"}"
sleep 1
ctl cdp Extensions.getExtensions
ctl cdp Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\"}"
ctl cdp Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"sync\"}"
ctl cdp Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"session\"}"
ctl cdp Extensions.setStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\",\"values\":{\"cdpSet\":42}}"
ctl sweval $A "chrome.storage.local.get().then(JSON.stringify)"
ctl sweval $A "JSON.stringify({id: chrome.runtime.id, v: chrome.runtime.getManifest().version, incog: chrome.extension.inIncognitoContext})"
ctl targets
ctl reload
sleep 1.5
echo "--- update A in place: loadUnpacked A2 (A is a command-line extension at 1.0)"
ctl cdp Extensions.loadUnpacked "{\"path\":\"$A2\"}"
sleep 2
ctl sweval $A "chrome.storage.local.get().then(JSON.stringify)"
ctl cdp Extensions.getExtensions
echo "--- uninstall via CDP: B (CDP-loaded) and A (command line, now reloaded by CDP)"
ctl cdp Extensions.setStorageItems "{\"id\":\"$B\",\"storageArea\":\"local\",\"values\":{\"keepMe\":\"B-data\"}}"
ctl cdp Extensions.uninstall "{\"id\":\"$B\"}"
sleep 1
ctl cdp Extensions.getStorageItems "{\"id\":\"$B\",\"storageArea\":\"local\"}"
ctl cdp Extensions.loadUnpacked "{\"path\":\"$CD\",\"enableInIncognito\":true}"
sleep 1
ctl cdp Extensions.getExtensions
show
quit; cp $L $E/q2-cdp-run1.log
echo "  LES after run1: $(ls "$R/Default/Local Extension Settings" | tr '\n' ' ')"
echo "  prefs after run1: $(jq -c '[.extensions.settings | to_entries[] | select(.value.location!=5) | {id:.key[0:6], loc:.value.location, path:(.value.path|sub(".*/exts/";"")), flags:.value.creation_flags, incog:.value.incognito}]' $R/Default/Preferences)"
echo "######## run 2: restart, same flags (A1 on the command line), CDP pipe"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1 SPIKE_URLS=$SITE/index.html; sleep 2
ctl cdp Extensions.getExtensions
ctl targets
ctl cdp Extensions.loadUnpacked "{\"path\":\"$CD\"}"
sleep 1.5
show
quit; cp $L $E/q2-cdp-run2.log
echo "  prefs after run2: $(jq -c '[.extensions.settings | to_entries[] | select(.value.location!=5) | {id:.key[0:6], loc:.value.location, path:(.value.path|sub(".*/exts/";"")), flags:.value.creation_flags, incog:.value.incognito}]' $R/Default/Preferences)"
