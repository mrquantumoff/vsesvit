#!/bin/bash
# Run the spike on the private Xvfb with a sanitized environment: scratch HOME and XDG_RUNTIME_DIR,
# a private D-Bus session (dbus-run-session), GPUI on lavapipe. Extra args are ENV=VALUE pairs.
# LOG=<file> chooses the log (default $E/spike.log).
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
LOG=${LOG:-$E/spike.log}
mkdir -p $E/home/Downloads $E/xdg $E/downloads; chmod 700 $E/xdg
env -i PATH=/usr/bin:/bin HOME=$E/home USER=$USER LANG=C.UTF-8 DISPLAY=$DPY XDG_RUNTIME_DIR=$E/xdg \
  VK_ICD_FILENAMES=$LVP SPIKE_ENV=$E SPIKE_CTL=$E/ctl SPIKE_SITE=$E/site \
  SPIKE_REPORTS=$E/reports.jsonl RUST_BACKTRACE=1 "$@" \
  dbus-run-session -- $BIN > $LOG 2>&1
echo "exit_code=$?" >> $LOG
