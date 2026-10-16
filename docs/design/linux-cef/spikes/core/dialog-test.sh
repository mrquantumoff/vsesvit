#!/bin/bash
# Trigger page UI that Chrome normally shows its own UI for; capture what appears.
# Usage: dialog-test.sh <SPIKE_DIALOGS mode: default|shell>
MODE=${1:-default}
source /tmp/vsesvit-scratch/spike/drive.sh
/tmp/vsesvit-scratch/spike/kill.sh >/dev/null
cd /tmp/vsesvit-scratch/spike && (./run.sh SPIKE_DIALOGS=$MODE SPIKE_URLS=http://127.0.0.1:8765/page1.html &)
sleep 3
OUT=/tmp/vsesvit-scratch/core-env/dialogs-$MODE; mkdir -p $OUT; rm -f $OUT/*
run() { # name x y
  mark; $K click $2 $3; sleep 1.8
  echo "=== $1"
  since | grep -E 'on_jsdialog|on_file_dialog|download|permission|js dialog|file dialog|fullscreen|page: (alert|confirm|prompt|file|geo|notification|fs|fullscreen)' | sed 's/^/   /'
  $X tree | grep -E '^0x' | grep VIEWABLE | grep -v '1x1+\|Vsesvit\|1600x1000' | sed 's/^/   extra toplevel: /'
  $X shotscreen $OUT/$1.png >/dev/null
  $K key esc; sleep 0.5; ctl closeext; ctl unfullscreen; sleep 0.5
}
run alert 805 285
run confirm 230 321
run prompt 363 321
run file 492 321
run geolocation 805 321
run download 210 354
run notification 363 357
run fullscreen 593 357
run select 805 357
ls $OUT/downloads 2>/dev/null; ls /tmp/vsesvit-scratch/core-env/downloads
