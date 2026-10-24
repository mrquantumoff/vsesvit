#!/bin/bash
# Kill every process of this spike (browsers, CEF helpers) and, with "all", its Xvfb and D-Bus.
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
pkill -9 -f "^$XW/target/" 2>/dev/null
if [ "$1" = all ]; then
  pkill -f "Xvfb $DPY" 2>/dev/null
  pkill -f "notifyd.py" 2>/dev/null
fi
sleep 0.5
pgrep -fa "^$XW/target/|Xvfb $DPY" || echo "nothing left"
