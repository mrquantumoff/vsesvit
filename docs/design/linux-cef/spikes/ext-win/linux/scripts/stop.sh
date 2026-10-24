#!/bin/bash
# stop.sh PROFILE LABEL: quit the spike cleanly (ctl quit), wait up to 25 s, then kill what is left.
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
P=$1; L=$2; R=$XW/runs/$L/$P
pid=$(cat $R/pid 2>/dev/null)
c $P quit >/dev/null 2>&1
for i in $(seq 250); do kill -0 $pid 2>/dev/null || break; sleep 0.1; done
if kill -0 $pid 2>/dev/null; then echo "profile $P did not exit; killing"; kill -9 $pid; fi
grep -E "cef shutdown|exiting" $R/spike.log | tail -1
