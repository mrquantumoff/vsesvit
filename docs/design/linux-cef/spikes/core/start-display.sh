#!/bin/bash
# Private headless display for spike-core: kwin_wayland virtual backend + rootless Xwayland,
# inside a private D-Bus session so it never touches the owner's desktop session.
# The D-Bus daemon (and every service it activates: portals, kwalletd, kioworkers) gets
# DISPLAY/WAYLAND_DISPLAY of the *private* display, never the owner's wayland-0 / :0.
set -e
OUT=/tmp/vsesvit-scratch/core-env
XNUM=${XNUM:-1}
rm -f $OUT/kwin.log $OUT/dbus.env
exec env -u DISPLAY -u WAYLAND_DISPLAY -u XDG_CURRENT_DESKTOP -u DESKTOP_SESSION -u KDE_FULL_SESSION -u GDK_BACKEND -u QT_QPA_PLATFORM \
  DISPLAY=:$XNUM WAYLAND_DISPLAY=wayland-vscore KWIN_WAYLAND_NO_PERMISSION_CHECKS=1 \
  dbus-run-session -- bash -c '
  echo "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" > '"$OUT"'/dbus.env
  exec env -u DISPLAY -u WAYLAND_DISPLAY kwin_wayland --virtual --xwayland --socket wayland-vscore --width 1600 --height 1000 --no-lockscreen
' > $OUT/kwin.log 2>&1
