#!/bin/bash
# Private display for spike-ext-api: Xvfb $SPIKE_DISPLAY (default :93, no window manager) inside a
# private D-Bus session whose environment carries only that DISPLAY (no WAYLAND_DISPLAY, no owner
# display), so services it activates cannot reach the owner's desktop. Writes $E/dbus.env.
# Run it in the background; it lives as long as Xvfb does.
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
rm -f $E/dbus.env $E/xvfb.log
mkdir -p $E/home $E/run; chmod 700 $E/run
XVFB_DIR=$(dirname "$(command -v Xvfb)")
exec env -i PATH=$XVFB_DIR:/usr/bin:/bin HOME=$E/home DISPLAY=$DPY XDG_RUNTIME_DIR=$E/run LANG=C.UTF-8 \
  dbus-run-session -- bash -c '
  echo "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" > '"$E"'/dbus.env
  exec Xvfb '"$DPY"' -screen 0 1600x1000x24 -nolisten tcp
' > $E/xvfb.log 2>&1
