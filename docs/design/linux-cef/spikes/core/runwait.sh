#!/bin/bash
/tmp/vsesvit-scratch/spike/run.sh "$@"; echo "exit_code=$?" >> /tmp/vsesvit-scratch/core-env/spike.log
