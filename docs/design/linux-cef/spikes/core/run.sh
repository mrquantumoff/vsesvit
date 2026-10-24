#!/bin/bash
# Run the spike on the private display :1 started by start-display.sh. No display comes from the
# environment (design section 7.3): SPIKE_DISPLAY is refused.
# Usage: ./run.sh [extra env assignments...]   e.g. ./run.sh SPIKE_KEYS=none
set -e
if [ -n "${SPIKE_DISPLAY+set}" ]; then
  echo "run.sh: refusing SPIKE_DISPLAY=$SPIKE_DISPLAY; the spike runs only on start-display.sh's private :1" >&2
  exit 1
fi
ENV_DIR=/tmp/vsesvit-scratch/core-env
BIN=${BIN:-/tmp/vsesvit-scratch/target-core/debug/vsesvit-spike}
export DISPLAY=:1
unset WAYLAND_DISPLAY GDK_BACKEND QT_QPA_PLATFORM
[ -f $ENV_DIR/dbus.env ] && export $(cat $ENV_DIR/dbus.env)
export SPIKE_CTL=${SPIKE_CTL:-$ENV_DIR/ctl}
mkdir -p $ENV_DIR/downloads $ENV_DIR/home/Downloads
# Keep Chrome defaults (Downloads, caches) out of the owner's real home.
export HOME=$ENV_DIR/home XDG_DOWNLOAD_DIR=$ENV_DIR/home/Downloads
for kv in "$@"; do export "$kv"; done
exec "$BIN" > $ENV_DIR/spike.log 2>&1
