#!/bin/bash
# Q4: why "Allow in Incognito" disables a --load-extension extension, per install path.
# Toggling incognito (or any reload) re-loads the extension. Chromium reloads an extension whose
# prefs record has no stored manifest through UnpackedInstaller::Load, i.e. as location kUnpacked.
# Runs: 1 cmdline A + external CRX C + CDP-loaded B, toggle all three; 2 restart; 3 restart without
# --load-extension; 4 developer mode on, then toggle; 5 chrome.runtime.reload() from the extension.
source "$(dirname "$0")/drive.sh"
CTL_WAIT=1.2
A1=$X/$A/1.0; BD=$X/$B/1.0
INFO="chrome.developerPrivate.getExtensionsInfo({includeDisabled:true, includeTerminated:true}).then(r => console.log('devp info ' + JSON.stringify(r.filter(e => e.location !== 'COMPONENT').map(e => [e.id.slice(0,6), e.location, e.state, 'incog=' + e.incognitoAccess.isActive, 'dr=' + JSON.stringify(e.disableReasons)]))), e => console.log('devp info err ' + e))"
devp() { ctl js "chrome.developerPrivate.$1.then(r => console.log('devp $2 ok ' + JSON.stringify(r ?? null)), e => console.log('devp $2 err ' + e.message))"; }
mgmt() { ctl js "chrome.management.$1.then(r => console.log('mgmt $2 ok ' + JSON.stringify(r ?? null)), e => console.log('mgmt $2 err ' + e.message))"; }
page() { ctl switch 0; ctl reload; sleep 1.5; ctl "js console.log('page: repA=' + document.documentElement.dataset.repA + ' repB=' + document.documentElement.dataset.repB + ' repC=' + document.documentElement.dataset.repC)"; }
otr() { ctl newotr $SITE/index.html; sleep 2.5; ctl "js console.log('otr page: repA=' + document.documentElement.dataset.repA + ' repB=' + document.documentElement.dataset.repB + ' repC=' + document.documentElement.dataset.repC)"; }
show() { grep -E "devp |mgmt |page: |cdp> " $L | sed -E 's/^.*console id=[0-9]+ //; s/^\[ *[0-9.]+\] //' | cut -c1-700; }
pr() { echo "  prefs: $(jq -c --arg a $A --arg b $B --arg c $C '[.extensions.settings | to_entries[] | select(.key==$a or .key==$b or .key==$c) | {id:.key[0:6], loc:.value.location, dr:.value.disable_reasons, incog:.value.incognito, path:(.value.path|sub(".*/(exts|Extensions)/";"")), flags:.value.creation_flags, manifest:(.value.manifest != null)}]' $R/Default/Preferences)"
      echo "  developer_mode: $(jq -c '.extensions.ui.developer_mode' $R/Default/Preferences)"; }
ext_json() { mkdir -p "$R/External Extensions"; printf '{"external_crx": "%s", "external_version": "1.0"}' $E/crx/repC-1.0.crx > "$R/External Extensions/$C.json"; }

R=$E/root-reload1; rm -rf $R; ext_json
echo "######## run 1: A on the command line, C external CRX, B through CDP; toggle incognito on each"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1 SPIKE_URLS=$SITE/index.html; sleep 3
ctl cdp Extensions.loadUnpacked "{\"path\":\"$BD\"}"; sleep 1
ctl new chrome://extensions; sleep 2.5
ctl js "$INFO"
for id in $A $B $C; do devp "updateExtensionConfiguration({extensionId: '$id', incognitoAccess: true})" incog-${id:0:6}; sleep 1; done
ctl js "$INFO"
page; otr; ctl switch 1
show; quit; cp $L $E/q4-run1.log; pr

echo "######## run 2: restart, same flags (B is not re-loaded through CDP)"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1 SPIKE_URLS=$SITE/index.html; sleep 3
ctl new chrome://extensions; sleep 2.5
ctl js "$INFO"
mgmt "setEnabled('$A', true)" enableA
ctl js "$INFO"
show; quit; cp $L $E/q4-run2.log; pr

echo "######## run 3: restart without --load-extension"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_URLS=$SITE/index.html; sleep 3
ctl new chrome://extensions; sleep 2.5
ctl js "$INFO"
ctl setpref extensions.ui.developer_mode true
mgmt "setEnabled('$A', true)" enableA-devmode
ctl js "$INFO"
page
show; quit; cp $L $E/q4-run3.log; pr

R=$E/root-reload4; rm -rf $R
echo "######## run 4: fresh profile, developer mode on before the toggle"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1 SPIKE_URLS=$SITE/index.html; sleep 3
ctl setpref extensions.ui.developer_mode true
ctl new chrome://extensions; sleep 2.5
devp "updateExtensionConfiguration({extensionId: '$A', incognitoAccess: true})" incog-A
ctl js "$INFO"
page; otr; ctl switch 1
show; quit; cp $L $E/q4-run4.log; pr
echo "--- run 4b: restart without --load-extension"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_URLS=$SITE/index.html; sleep 3
ctl new chrome://extensions; sleep 2.5; ctl js "$INFO"; page
show; quit; pr

R=$E/root-reload5; rm -rf $R
echo "######## run 5: fresh profile, the command-line extension calls chrome.runtime.reload()"
start SPIKE_ROOT=$R SPIKE_CDP_PIPE=1 SPIKE_LOAD_EXT=$A1 SPIKE_URLS=$SITE/index.html; sleep 3
ctl sweval $A "chrome.runtime.reload(), 'reloading'"; sleep 2
ctl new chrome://extensions; sleep 2.5; ctl js "$INFO"
show; quit; cp $L $E/q4-run5.log; pr
