#!/bin/bash
# Private X server for spike-ext-load (:92). Nothing touches the owner's desktop.
# A server already on :92 is refused, never reused: this script cannot tell it is its own (design
# section 7.3). stop: kill $(cat $E/xvfb.pid)
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
n=${DPY#:}
if [ -e "/tmp/.X$n-lock" ] || [ -e "/tmp/.X11-unix/X$n" ] || DISPLAY=$DPY xwininfo -root >/dev/null 2>&1; then
  echo "start-xvfb.sh: refusing: a server already holds $DPY; stop it first" >&2
  exit 1
fi
setsid Xvfb $DPY -screen 0 1600x1000x24 -nolisten tcp > $E/xvfb.log 2>&1 < /dev/null &
echo $! > $E/xvfb.pid
sleep 1; DISPLAY=$DPY xwininfo -root | grep -E "Width|Height|Depth"
