#!/bin/bash
# Private X server for spike-ext-load (display :92). Nothing touches the owner's desktop.
DISPLAY=:92 xdpyinfo >/dev/null 2>&1 && { echo "display :92 already up"; exit 0; }
setsid Xvfb :92 -screen 0 1600x1000x24 -nolisten tcp > /tmp/vsesvit-scratch/extload-env/xvfb.log 2>&1 < /dev/null &
echo $! > /tmp/vsesvit-scratch/extload-env/xvfb.pid
sleep 1; DISPLAY=:92 xdpyinfo | head -3
