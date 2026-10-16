#!/bin/bash
# Run the spike on the private display started by start-display.sh (or $SPIKE_DISPLAY).
# Usage: ./run.sh [extra env assignments...]   e.g. ./run.sh SPIKE_KEYS=none
set -e
ENV_DIR=/tmp/vsesvit-scratch/core-env
BIN=${BIN:-/tmp/vsesvit-scratch/target-core/debug/vsesvit-spike}
export DISPLAY=${SPIKE_DISPLAY:-:1}
unset WAYLAND_DISPLAY GDK_BACKEND QT_QPA_PLATFORM
[ -f $ENV_DIR/dbus.env ] && export $(cat $ENV_DIR/dbus.env)
export SPIKE_CTL=${SPIKE_CTL:-$ENV_DIR/ctl}
mkdir -p $ENV_DIR/downloads $ENV_DIR/home/Downloads
# Keep Chrome defaults (Downloads, caches) out of the owner's real home.
export HOME=$ENV_DIR/home XDG_DOWNLOAD_DIR=$ENV_DIR/home/Downloads
for kv in "$@"; do export "$kv"; done
exec "$BIN" > $ENV_DIR/spike.log 2>&1
