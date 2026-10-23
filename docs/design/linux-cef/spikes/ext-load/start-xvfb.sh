#!/bin/bash
# Private X server for spike-ext-load ($SPIKE_DISPLAY, default :92). Nothing touches the owner's desktop.
# stop: kill $(cat $E/xvfb.pid)
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
DISPLAY=$DPY xwininfo -root >/dev/null 2>&1 && { echo "display $DPY already up"; exit 0; }
setsid Xvfb $DPY -screen 0 1600x1000x24 -nolisten tcp > $E/xvfb.log 2>&1 < /dev/null &
echo $! > $E/xvfb.pid
sleep 1; DISPLAY=$DPY xwininfo -root | grep -E "Width|Height|Depth"
