#!/bin/bash
# Kill every spike process (browser + CEF helpers), never this script itself.
for p in $(pgrep -f '^/tmp/vsesvit-scratch/target-core/[a-z]+/vsesvit-spike'); do kill -9 $p 2>/dev/null; done
sleep 0.3
pgrep -fa '^/tmp/vsesvit-scratch/target-core/' || echo "no spike processes"
