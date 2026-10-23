#!/bin/bash
# Kill every process of this spike (browser + CEF helpers + its private dbus), never others.
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
for p in $(pgrep -f "^$TARGET/"); do kill -9 $p 2>/dev/null; done
sleep 0.5
pgrep -fa "^$TARGET/" || echo "no extload processes"
