#!/bin/bash
# Per-process CPU% over N seconds (default 15) for all spike processes, from /proc/<pid>/stat.
N=${1:-15}
HZ=$(getconf CLK_TCK)
declare -A T0
PIDS=$(pgrep -f '^/tmp/vsesvit-scratch/target-core/')
for p in $PIDS; do read -r -a f < /proc/$p/stat; T0[$p]=$(( f[13] + f[14] )); done
sleep $N
total=0; out=""
for p in $PIDS; do
  [ -r /proc/$p/stat ] || continue
  read -r -a f < /proc/$p/stat; d=$(( f[13] + f[14] - T0[$p] )); total=$(( total + d ))
  seccomp=$(awk '/^Seccomp:/{print $2}' /proc/$p/status)
  args=$(tr '\0' ' ' < /proc/$p/cmdline); t=$(echo "$args" | grep -oE -- '--type=[a-z-]+' | head -1)
  u=$(echo "$args" | grep -oE -- '--utility-sub-type=[a-zA-Z.]+' | head -1)
  rss=$(awk '/^VmRSS:/{print $2}' /proc/$p/status)
  out+=$(printf "%7s ppid=%-7s %6.2f%% seccomp=%s rss=%6sk %s %s" $p ${f[3]} $(echo "scale=4; $d*100/($HZ*$N)" | bc) "$seccomp" "$rss" "${t:-browser}" "${u#--utility-sub-type=}")$'\n'
done
echo -n "$out"
printf "  TOTAL %6.2f%% of one core over %ss\n" $(echo "scale=4; $total*100/($HZ*$N)" | bc) $N
