#!/bin/bash
# Kill every spike-ext-api browser process (browser + CEF helpers); never touches other agents' binaries.
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
for p in $(pgrep -f "^$TARGET/"); do kill -9 $p 2>/dev/null; done
sleep 0.3
pgrep -fa "^$TARGET/" || echo "no extapi processes"
