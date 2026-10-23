#!/bin/bash
# Run the spike on the private Xvfb with the fixture extensions loaded.
# Usage: ./run.sh [ENV=VALUE ...]   (EXTS="comma,separated,dirs" overrides the extension list)
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
export DISPLAY=$DPY
unset WAYLAND_DISPLAY GDK_BACKEND QT_QPA_PLATFORM XDG_CURRENT_DESKTOP DESKTOP_SESSION KDE_FULL_SESSION
export $(cat $E/dbus.env)
export XDG_RUNTIME_DIR=$E/run
export SPIKE_CTL=${SPIKE_CTL:-$E/ctl}
export HOME=$E/home XDG_DOWNLOAD_DIR=$E/home/Downloads
export VK_ICD_FILENAMES=$LVP
export SPIKE_SCALE=1 SPIKE_CDP_PORT=${SPIKE_CDP_PORT:-9293} SPIKE_DIALOGS=${SPIKE_DIALOGS:-shell}
mkdir -p $E/home/Downloads $E/downloads
rm -rf $FX; cp -r $REPO/tests/fixtures/extensions $FX
for kv in "$@"; do export "$kv"; done
EXTS=${EXTS:-$FX/probe,$FX/commands,$FX/cookies,$FX/dnr,$FX/menus,$FX/navigation,$FX/notifications,$FX/permissions,$FX/scripts,$FX/windows,$FX/ports-friend,$SX/spy,$SX/ports3}
python3 -I $HERE/ids.py "$EXTS" > $E/ids.json
export SPIKE_CEF_ARGS="--load-extension=$EXTS --noerrdialogs $SPIKE_EXTRA_ARGS"
export SPIKE_ROOT=${SPIKE_ROOT:-$E/cef-root}
exec "$BIN" > $E/spike.log 2>&1
