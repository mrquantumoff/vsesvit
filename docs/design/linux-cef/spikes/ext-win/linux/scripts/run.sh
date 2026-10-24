#!/bin/bash
# run.sh PROFILE LABEL [ENV=VALUE ...]: start the spike for profile A or B in the background,
# under the private Xvfb and D-Bus session. The CEF root persists per LABEL/PROFILE, so a
# second run with the same label is a restart of the same profile.
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
P=$1; L=$2; shift 2
CTL=$(port_ctl $P); REP=$(port_rep $P)
[ -n "$CTL" ] || { echo "profile must be A or B"; exit 2; }
R=$XW/runs/$L/$P
mkdir -p $R/home $R/xdg; chmod 700 $R/xdg
[ -d $R/site ] || cp -r $FX/site $R/site
echo "=== start $(date +%T) $*" >> $R/spike.log
source $XW/dbus.env
env -i PATH=/usr/bin:/bin HOME=$R/home USER=$USER LANG=C.UTF-8 DISPLAY=$DPY XDG_RUNTIME_DIR=$R/xdg \
  DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS VK_ICD_FILENAMES=$LVP SPIKE_XTOKEN=$(cat $XW/xtoken) \
  SPIKE_LOG=$R/spike.log SPIKE_ROOT=$R/root SPIKE_CACHE_NAME=Default SPIKE_CTL_PORT=$CTL SPIKE_REPORT_PORT=$REP \
  SPIKE_SITE=$R/site SPIKE_REPORTS=$R/reports.jsonl SPIKE_URLS=${SPIKE_URLS:-http://site.vsesvit.test/index.html} \
  SPIKE_SCALE=1 RUST_BACKTRACE=1 "$@" setsid $BIN > $R/stderr.txt 2>&1 < /dev/null &
echo $! > $R/pid
for i in $(seq 150); do
  grep -q "GUARD:" $R/spike.log && { grep GUARD $R/spike.log; exit 3; }
  c $P ping >/dev/null 2>&1 && break
  sleep 0.1
done
echo "profile $P pid $(cat $R/pid) ctl $CTL log $R/spike.log"
