# source me: helpers to drive the spike on its private display
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
export DISPLAY=$DPY
unset WAYLAND_DISPLAY
X=$TARGET/debug/xctl
L=$E/spike.log
CTL=$E/ctl
cdp() { node $HERE/harness/cdp.mjs "$@"; }
mark() { N=$(wc -l < $L); }
since() { tail -n +$((N+1)) $L | grep -v MESA; }
ctl() { timeout 3 bash -c "echo \"\$1\" > $CTL" _ "$*" || echo "ctl: no reader"; sleep ${CTL_WAIT:-0.4}; }
shot() { import -display $DPY -window root "$1"; } # shot FILE.png: the whole private screen
