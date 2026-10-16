#!/bin/bash
# Run the spike on Xvfb :93 with the fixture extensions loaded.
# Usage: ./run.sh [ENV=VALUE ...]   (EXTS="comma,separated,dirs" overrides the extension list)
ENV_DIR=/tmp/vsesvit-scratch/extapi-env
BIN=${BIN:-/tmp/vsesvit-scratch/target-extapi/debug/vsesvit-spike}
FX=/home/dy/git/vsesvit-cef/tests/fixtures/extensions
SX=/tmp/vsesvit-scratch/spike-ext-api/exts
export DISPLAY=:93
unset WAYLAND_DISPLAY GDK_BACKEND QT_QPA_PLATFORM XDG_CURRENT_DESKTOP DESKTOP_SESSION KDE_FULL_SESSION
export $(cat $ENV_DIR/dbus.env)
export XDG_RUNTIME_DIR=$ENV_DIR/run
export SPIKE_CTL=${SPIKE_CTL:-$ENV_DIR/ctl}
export HOME=$ENV_DIR/home XDG_DOWNLOAD_DIR=$ENV_DIR/home/Downloads
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
export SPIKE_SCALE=1 SPIKE_CDP_PORT=${SPIKE_CDP_PORT:-9293} SPIKE_DIALOGS=${SPIKE_DIALOGS:-shell}
mkdir -p $ENV_DIR/home/Downloads $ENV_DIR/downloads
for kv in "$@"; do export "$kv"; done
EXTS=${EXTS:-$FX/probe,$FX/commands,$FX/cookies,$FX/dnr,$FX/menus,$FX/navigation,$FX/notifications,$FX/permissions,$FX/scripts,$FX/windows,$FX/ports-friend,$SX/spy,$SX/ports3}
export SPIKE_CEF_ARGS="--load-extension=$EXTS --noerrdialogs $SPIKE_EXTRA_ARGS"
export SPIKE_ROOT=${SPIKE_ROOT:-$ENV_DIR/cef-root}
exec "$BIN" > $ENV_DIR/spike.log 2>&1
