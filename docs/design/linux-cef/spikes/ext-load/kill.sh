#!/bin/bash
# Kill every process of this spike (browser + CEF helpers + its private dbus), never others.
for p in $(pgrep -f 'vsesvit-scratch/target-extload/'); do kill -9 $p 2>/dev/null; done
sleep 0.5
pgrep -fa 'vsesvit-scratch/target-extload/' || echo "no extload processes"
