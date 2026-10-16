#!/bin/bash
# Run the spike on Xvfb :92 with a sanitized environment: scratch HOME and XDG_RUNTIME_DIR,
# a private D-Bus session (dbus-run-session), GPUI on lavapipe. Extra args are ENV=VALUE pairs.
# LOG=<file> chooses the log (default extload-env/spike.log).
E=/tmp/vsesvit-scratch/extload-env
BIN=${BIN:-/tmp/vsesvit-scratch/target-extload/debug/vsesvit-spike}
LOG=${LOG:-$E/spike.log}
mkdir -p $E/home/Downloads $E/xdg; chmod 700 $E/xdg
env -i PATH=/usr/bin:/bin HOME=$E/home USER=$USER LANG=C.UTF-8 DISPLAY=:92 XDG_RUNTIME_DIR=$E/xdg \
  VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json SPIKE_CTL=$E/ctl SPIKE_SITE=$E/site \
  SPIKE_REPORTS=$E/reports.jsonl RUST_BACKTRACE=1 "$@" \
  dbus-run-session -- $BIN > $LOG 2>&1
echo "exit_code=$?" >> $LOG
