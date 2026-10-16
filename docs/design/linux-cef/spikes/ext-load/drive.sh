# Source me. Helpers to drive the spike through its control FIFO and read its log.
E=/tmp/vsesvit-scratch/extload-env
L=${LOG:-$E/spike.log}
CTL=$E/ctl
RUN=/tmp/vsesvit-scratch/spike-ext-load/run.sh
SITE=http://127.0.0.1:8792
mark() { N=$(wc -l < $L); }
since() { tail -n +$((N+1)) $L | grep -v -E 'MESA|libEGL|lavapipe'; }
ctl() { timeout 3 bash -c "echo \"\$1\" > $CTL" _ "$*" || echo "ctl: no reader"; sleep ${CTL_WAIT:-0.4}; }
waitfor() { # waitfor PATTERN [TIMEOUT_S]
  local t=0; while ! grep -q -E "$1" $L 2>/dev/null; do sleep 0.2; t=$((t+1)); [ $t -ge $(( ${2:-20} * 5 )) ] && { echo "timeout waiting for $1"; return 1; }; done; }
start() { # start ENV=... : start the spike in the background, wait for CEF context
  : > $L; ( $RUN "$@" & ); waitfor 'on_context_initialized' 25 && sleep ${START_WAIT:-2}; }
quit() { CTL_WAIT=0 ctl quit; waitfor 'exit_code=' 20; tail -1 $L; }
reports() { grep -E '^\[ *[0-9.]+\] report:' $L | sed -E 's/^\[ *([0-9.]+)\] report: /\1 /'; }
