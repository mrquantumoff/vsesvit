#!/bin/bash
# The spike's private X server: Xvfb :94 inside a private D-Bus session, its root window
# marked with a fresh random token. The spike and xctl refuse any display without it.
# Refuses to reuse a display that already answers (it would not be ours).
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
if DISPLAY=$DPY $OPT/xdpyinfo >/dev/null 2>&1; then echo "display $DPY already exists; refusing (stop it first)"; exit 1; fi
mkdir -p $XW/xvfb/home $XW/xvfb/run; chmod 700 $XW/xvfb/run
TOKEN=$(openssl rand -hex 24)
umask 077; printf %s "$TOKEN" > $XW/xtoken; umask 022
setsid env -i PATH=$OPT:/usr/bin:/bin HOME=$XW/xvfb/home DISPLAY=$DPY XDG_RUNTIME_DIR=$XW/xvfb/run LANG=C.UTF-8 \
  dbus-run-session -- bash -c 'echo "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" > '"$XW"'/dbus.env; exec Xvfb '"$DPY"' -screen 0 1600x1000x24 -nolisten tcp -noreset' \
  > $XW/xvfb.log 2>&1 < /dev/null &
for i in $(seq 80); do DISPLAY=$DPY $OPT/xdpyinfo >/dev/null 2>&1 && break; sleep 0.1; done
DISPLAY=$DPY $OPT/xprop -root -f _VSESVIT_SPIKE_TOKEN 8s -set _VSESVIT_SPIKE_TOKEN "$TOKEN" || { echo "xprop failed"; exit 1; }
echo "Xvfb $DPY up, token set, $(cat $XW/dbus.env)"
