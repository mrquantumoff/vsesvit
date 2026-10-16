#!/bin/bash
# Fresh profile + spike + CDP daemon. Usage: ./restart.sh [ENV=VALUE ...]
cd /tmp/vsesvit-scratch/spike-ext-api
for p in $(pgrep -f '^node harness/cdp[.]mjs daemon'); do kill $p; done
./kill.sh >/dev/null
E=/tmp/vsesvit-scratch/extapi-env
rm -rf $E/cef-root $E/cdp.log $E/hits.log $E/reports.log
[ -z "$URLS" ] && URLS="http://127.0.0.1:8793/index.html http://127.0.0.1:8793/page2.html http://127.0.0.1:8793/frames.html"
(setsid ./run.sh "SPIKE_URLS=$URLS" "$@" &)
sleep ${WAIT:-3}
(setsid node harness/cdp.mjs daemon > $E/cdpd.out 2>&1 &)
sleep 1.5
grep -c "target+ service_worker" $E/cdp.log
