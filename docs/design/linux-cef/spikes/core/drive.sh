#!/bin/bash
# Helpers for driving the spike on the private display: source this file.
export $(cat /tmp/vsesvit-scratch/core-env/dbus.env)
export WAYLAND_DISPLAY=wayland-vscore DISPLAY=:1
X=/tmp/vsesvit-scratch/target-core/debug/xctl
K=/tmp/vsesvit-scratch/target-core/debug/kwin-input
L=/tmp/vsesvit-scratch/core-env/spike.log
CTL=/tmp/vsesvit-scratch/core-env/ctl
mark() { N=$(wc -l < $L); }
since() { tail -n +$((N+1)) $L | grep -v MESA; }
ctl() { timeout 3 bash -c "echo \"\$1\" > $CTL" _ "$*" || echo "ctl: no reader"; sleep ${CTL_WAIT:-0.4}; }
RUN=/tmp/vsesvit-scratch/spike/run.sh
