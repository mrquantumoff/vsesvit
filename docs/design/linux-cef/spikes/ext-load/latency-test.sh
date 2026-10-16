#!/bin/bash
# Key-press latency: GPUI-loop timestamp ("ctl> mark") -> CEF on_pre_key_event, after idling.
source /tmp/vsesvit-scratch/spike/drive.sh
/tmp/vsesvit-scratch/spike/kill.sh >/dev/null
($RUN $1 SPIKE_URLS=http://127.0.0.1:8765/page2.html &)
sleep 3.5; $K click 600 600; sleep 0.5
for i in 1 2 3 4 5 6; do
  sleep 1.5            # let the pump go idle
  mark; CTL_WAIT=0 ctl mark; $K key a; sleep 0.5
  t0=$(since | grep 'ctl> mark' | head -1 | sed -E 's/^\[ *([0-9.]+)\].*/\1/')
  t1=$(since | grep 'on_pre_key_event' | head -1 | sed -E 's/^\[ *([0-9.]+)\].*/\1/')
  echo "$i: mark=$t0 pre_key=$t1 delta_ms=$(echo "($t1-$t0)*1000" | bc)"
done
