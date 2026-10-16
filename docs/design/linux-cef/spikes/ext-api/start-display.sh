#!/bin/bash
# Private display for spike-ext-api: Xvfb :93 (no window manager) inside a private D-Bus
# session whose environment carries only DISPLAY=:93 (no WAYLAND_DISPLAY, no owner display),
# so services it activates cannot reach the owner's desktop. Writes extapi-env/dbus.env.
OUT=/tmp/vsesvit-scratch/extapi-env
rm -f $OUT/dbus.env $OUT/xvfb.log
exec env -i PATH=/usr/bin:/bin HOME=$OUT/home DISPLAY=:93 XDG_RUNTIME_DIR=$OUT/run LANG=C.UTF-8 \
  dbus-run-session -- bash -c '
  echo "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" > '"$OUT"'/dbus.env
  exec Xvfb :93 -screen 0 1600x1000x24 -nolisten tcp
' > $OUT/xvfb.log 2>&1
