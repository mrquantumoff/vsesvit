# source me: helpers to drive the spike on :93
export DISPLAY=:93
unset WAYLAND_DISPLAY
X=/tmp/vsesvit-scratch/target-extapi/debug/xctl
L=/tmp/vsesvit-scratch/extapi-env/spike.log
CTL=/tmp/vsesvit-scratch/extapi-env/ctl
cdp() { node /tmp/vsesvit-scratch/spike-ext-api/harness/cdp.mjs "$@"; }
mark() { N=$(wc -l < $L); }
since() { tail -n +$((N+1)) $L | grep -v MESA; }
ctl() { timeout 3 bash -c "echo \"\$1\" > $CTL" _ "$*" || echo "ctl: no reader"; sleep ${CTL_WAIT:-0.4}; }
