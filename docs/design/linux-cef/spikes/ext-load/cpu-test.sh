#!/bin/bash
# Idle CPU with one page loaded: start, wait, measure 15 s per process + pump rates.
# Usage: cpu-test.sh "ENV=.. ENV=.." [label]
source /tmp/vsesvit-scratch/spike/drive.sh
/tmp/vsesvit-scratch/spike/kill.sh >/dev/null
($RUN $1 SPIKE_URLS=${URL:-https://example.org} &)
sleep 8
ctl stats; a=$(grep 'schedule_requests' $L | tail -1)
/tmp/vsesvit-scratch/spike/cpu.sh 15 > /tmp/vsesvit-scratch/core-env/cpu-$2.txt
ctl stats; b=$(grep 'schedule_requests' $L | tail -1)
echo "### $2 ($1)"
cat /tmp/vsesvit-scratch/core-env/cpu-$2.txt
echo "  pump before: ${a#*pump: }"
echo "  pump after:  ${b#*pump: }"
grep 'latency_avg' $L | tail -1 | sed 's/^/  /'
