#!/bin/bash
# Press Chrome accelerators while the page has focus; report Chrome-side effects.
# Usage: accel-test.sh <SPIKE_KEYS mode>
MODE=${1:-none}
source /tmp/vsesvit-scratch/spike/drive.sh
/tmp/vsesvit-scratch/spike/kill.sh >/dev/null
cd /tmp/vsesvit-scratch/spike && (./run.sh SPIKE_KEYS=$MODE SPIKE_URLS=http://127.0.0.1:8765/page1.html &)
sleep 3
OUT=/tmp/vsesvit-scratch/core-env/accel-$MODE
mkdir -p $OUT; rm -f $OUT/*
toplevels() { $X tree | grep -E '^0x' | grep VIEWABLE | grep -v '1x1+' ; }
for key in ctrl+t ctrl+n ctrl+w ctrl+shift+t f11 ctrl+p ctrl+f f12; do
  $K click 760 132; sleep 0.3; $K click 360 405; sleep 0.4   # activate our window, focus the page
  before=$(toplevels | wc -l)
  mark
  $K key $key; sleep 1.5
  after=$(toplevels | wc -l)
  echo "=== $key: viewable toplevels $before -> $after"
  since | grep -E 'chrome_command|action|on_after_created|on_before_close|fullscreen|create_browser|tab [0-9]+ <-|activated|keydown' | grep -v 'key=Control\|key=Shift' | sed 's/^/   /'
  toplevels | sed 's/^/   top: /'
  ctl "shot $OUT/$key.png"
  # close anything Chrome opened on its own (new toplevels other than ours)
  for w in $(toplevels | grep -v 'Vsesvit spike' | awk '{print $1}'); do $X shot $OUT/$key-extra-$w.png $w >/dev/null 2>&1; done
  $K key esc; sleep 0.3; ctl closeext; ctl unfullscreen; sleep 0.8
  [ "$key" = f11 ] && sleep 1
  [ "$key" = ctrl+w ] && { ctl "new http://127.0.0.1:8765/page1.html"; sleep 1.5; }
done
