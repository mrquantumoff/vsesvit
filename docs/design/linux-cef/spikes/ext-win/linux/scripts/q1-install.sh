#!/bin/bash
# Item 1 (Linux): the per-profile External Extensions route, two profiles open at once with
# conflicting sets. A: H, C 1.0, D 1.0, S.  B: H, C 2.0, E, S. Silent? Per profile? Prompt?
cd "$(dirname "${BASH_SOURCE[0]}")"; source env.sh; source lib.sh
L=${1:-q1}
rm -rf $XW/runs/$L
xjson A $L H 1.0; xjson A $L C 1.0; xjson A $L D 1.0; xjson A $L S 1.0
xjson B $L H 1.0; xjson B $L C 2.0; xjson B $L E 1.0; xjson B $L S 1.0
./run.sh A $L; ./run.sh B $L
sleep 6
for P in A B; do say "$P bridge: $(xbridge $P)"; done
sleep 1
for P in A B; do say "$P list: $(xcall $P '{op:"list"}')"; done
for P in A B; do say "$P toplevel maps:"; grep -E 'rootwatch: MapNotify' $(logf $P $L) | grep -v 'Vsesvit spike' | head; done
for P in A B; do say "$P page marks: $(c $P 'ev active JSON.stringify(document.documentElement.dataset)')"; done
for P in A B; do say "$P reports:"; reports $P $L | cut -c1-200 | head -20; done
[ "$2" = keep ] && exit 0
for P in A B; do ./stop.sh $P $L; done
for P in A B; do say "$P prefs after quit:"; prefs $P $L; done
