#!/bin/bash
# The fail-closed guard: the spike and xctl must refuse every display but the token-marked Xvfb.
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
R=$XW/runs/guard; rm -rf $R; mkdir -p $R
try() { # name, then env assignments
  local name=$1; shift
  env -i PATH=/usr/bin:/bin HOME=$R LANG=C.UTF-8 SPIKE_LOG=$R/$name.log SPIKE_ROOT=$R/root SPIKE_CTL_PORT=47919 "$@" timeout 20 $BIN > $R/$name.out 2>&1
  echo "$name: exit=$? $(grep -h -m1 -E 'GUARD|guard ok' $R/$name.log $R/$name.out | head -1)"
}
try wslg-display DISPLAY=:0
try wslg-wrong-token DISPLAY=:0 SPIKE_XTOKEN=0123456789abcdef0123456789abcdef
try private-wrong-token DISPLAY=$DPY SPIKE_XTOKEN=0123456789abcdef0123456789abcdef
try private-no-token DISPLAY=$DPY
try wayland-set DISPLAY=$DPY SPIKE_XTOKEN=$(cat $XW/xtoken) WAYLAND_DISPLAY=wayland-0
echo "xctl on :0: $(env -i PATH=/usr/bin:/bin DISPLAY=:0 $XCTL focus 2>&1 | head -1)"
echo "xctl on private: $(env -i PATH=/usr/bin:/bin DISPLAY=$DPY SPIKE_XTOKEN=$(cat $XW/xtoken) $XCTL focus 2>&1 | head -1)"
pgrep -fa "^$BIN" || echo "no spike left"
