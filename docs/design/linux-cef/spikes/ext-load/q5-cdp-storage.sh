#!/bin/bash
# Q5: the rest of CDP Extensions.* over --remote-debugging-pipe: remove/clear storage, storage of a
# packed (External JSON) extension, storage after the worker stopped, uninstall per location.
source "$(dirname "$0")/drive.sh"
CTL_WAIT=1.2
A1=$X/$A/1.0; BD=$X/$B/1.0
R=$E/root-cdp3; rm -rf $R; mkdir -p "$R/External Extensions"
printf '{"external_crx": "%s", "external_version": "1.0"}' $E/crx/repC-1.0.crx > "$R/External Extensions/$C.json"
show() { grep -E "cdp> |toplevels" $L | sed -E 's/^\[ *[0-9.]+\] //' | cut -c1-600; }
st() { ctl swcdp $A Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"$1\"}"; }
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1 SPIKE_URLS=$SITE/index.html; sleep 3
echo "--- packed external extension C through its own worker session"
ctl swcdp $C Extensions.getStorageItems "{\"id\":\"$C\",\"storageArea\":\"local\"}"
ctl swcdp $C Extensions.setStorageItems "{\"id\":\"$C\",\"storageArea\":\"local\",\"values\":{\"fromShell\":\"yes\"}}"
echo "--- set / remove / clear / get with keys on A"
ctl swcdp $A Extensions.setStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\",\"values\":{\"k1\":1,\"k2\":{\"deep\":[1,2]},\"k3\":\"three\"}}"
ctl swcdp $A Extensions.removeStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\",\"keys\":[\"k1\"]}"
ctl swcdp $A Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\",\"keys\":[\"k1\",\"k2\",\"k3\"]}"
st session
ctl swcdp $A Extensions.clearStorageItems "{\"id\":\"$A\",\"storageArea\":\"session\"}"
st session
ctl swcdp $A Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"managed\"}"
ctl swcdp $A Extensions.setStorageItems "{\"id\":\"$A\",\"storageArea\":\"managed\",\"values\":{\"x\":1}}"
echo "--- uninstall through CDP: command-line A, external C, CDP-loaded B"
ctl cdp Extensions.uninstall "{\"id\":\"$A\"}"
ctl cdp Extensions.uninstall "{\"id\":\"$C\"}"
ctl cdp Extensions.loadUnpacked "{\"path\":\"$BD\"}"; sleep 1
ctl swcdp $B Extensions.setStorageItems "{\"id\":\"$B\",\"storageArea\":\"local\",\"values\":{\"keepMe\":\"B\"}}"
echo "  toplevels before: $(toplevels | wc -l)"
ctl cdp Extensions.uninstall "{\"id\":\"$B\"}"; sleep 1.5
echo "  toplevels after:  $(toplevels | wc -l)"; toplevels | cut -c1-160
mkdir -p $E/shots; shot $E/shots/cdp-uninstall.png
echo "  LES now: $(ls "$R/Default/Local Extension Settings" | tr '\n' ' ')"
echo "--- after the worker idles out (40 s): is storage still reachable?"
sleep 40
ctl targets
st local
ctl new chrome-extension://$A/manifest.json; sleep 2
ctl targets
ctl pagecdp chrome-extension://$A Extensions.getStorageItems "{\"id\":\"$A\",\"storageArea\":\"local\"}"
ctl pagecdp chrome-extension://$A Extensions.setStorageItems "{\"id\":\"$A\",\"storageArea\":\"sync\",\"values\":{\"fromHiddenPage\":1}}"
ctl pagecdp chrome-extension://$A Extensions.getStorageItems "{\"id\":\"$C\",\"storageArea\":\"local\"}"
show; quit; cp $L $E/q5.log
