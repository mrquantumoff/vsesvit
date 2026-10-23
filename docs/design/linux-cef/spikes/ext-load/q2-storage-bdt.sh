#!/bin/bash
# Q2a storage over CDP sessions; Q2b CefBrowserHost::execute_dev_tools_method (page session).
source "$(dirname "$0")/drive.sh"

A1=$X/$A/1.0; BD=$X/$B/1.0; CD=$X/$C/1.0
R=$E/root-cdp2
CTL_WAIT=1.2
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1,$BD SPIKE_URLS=$SITE/index.html; sleep 2
echo "--- storage through the extension's own service-worker session"
ctl swcdp $A Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\"}"
ctl swcdp $A Extensions.setStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\",\"values\":{\"cdpSet\":42}}"
ctl swcdp $A Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"sync\"}"
ctl swcdp $A Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"session\"}"
ctl swcdp $A Extensions.getStorageItems "{\"id\":\"$B\",\"storageArea\":\"local\"}"
echo "--- storage through a page session where A's content script runs"
ctl pagecdp http://127.0.0.1 Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\"}"
ctl sweval $A "chrome.storage.local.get().then(JSON.stringify)"
echo "--- Q2b: execute_dev_tools_method on the tab's page session"
ctl bdt Browser.getVersion
ctl bdt Target.getTargets
ctl bdt Target.attachToBrowserTarget
ctl bdt Extensions.getExtensions
ctl bdt Extensions.loadUnpacked "{\"path\":\"$CD\"}"
ctl bdt Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\"}"
ctl bdt Extensions.setStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\",\"values\":{\"viaCefDevTools\":1}}"
ctl bdt Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\"}"
ctl bdt Runtime.evaluate "{\"expression\":\"location.href\"}"
sleep 1
grep -E "cdp> |devtools:" $L | sed -E 's/"t":[0-9]+,?//' | cut -c1-700
quit; cp $L $E/q2-storage-bdt.log
