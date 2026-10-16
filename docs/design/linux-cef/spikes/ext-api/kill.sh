#!/bin/bash
# Kill every spike-ext-api browser process (browser + CEF helpers); never touches other agents' binaries.
for p in $(pgrep -f '^/tmp/vsesvit-scratch/target-extapi/'); do kill -9 $p 2>/dev/null; done
sleep 0.3
pgrep -fa '^/tmp/vsesvit-scratch/target-extapi/' || echo "no extapi processes"
