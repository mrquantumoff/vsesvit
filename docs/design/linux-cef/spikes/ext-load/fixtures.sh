#!/bin/bash
# Lay out every test extension under $E and write $E/ids.env. Idempotent: keys are made once
# (throwaway RSA-2048, never committed), extension directories and CRX files are rebuilt each run.
#   A, B   reporter, --load-extension directories ($E/exts/<id>/<version>)
#   C      reporter packed as CRX 1.0 / 2.0 for External Extensions JSON (external_crx)
#   D      reporter with update_url http://127.0.0.1:8792/updates/D.xml, CRX 1.0 / 2.0 (updater test)
#   G      Vsesvit Agent (chrome.management helper)
#   P      tests/fixtures/extensions/probe with the committed test-only probe key
set -e
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
[ -n "$REPO" ] || { echo "set VSESVIT_REPO to the vsesvit checkout"; exit 1; }
mkdir -p $E/keys $E/exts $E/crx $E/site/updates
for k in A B C D agent; do
  [ -f $E/keys/$k.pem ] || openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out $E/keys/$k.pem 2>/dev/null
done
sed -n '/-----BEGIN/,$p' $REPO/tests/fixtures/keys/test-only-probe-key.pem > $E/keys/probe.pem
cp -r $HERE/site/. $E/site/
mk() { python3 -I $HERE/mkext.py "$@" | cut -d' ' -f1; }
A=$(mk reporter A 1.0 A); mk reporter A 2.0 A >/dev/null
B=$(mk reporter B 1.0 B)
C=$(mk reporter C 1.0 C); mk reporter C 2.0 C >/dev/null
UPD='{"update_url": "http://127.0.0.1:8792/updates/D.xml"}'
D=$(mk reporter D 1.0 D "$UPD"); mk reporter D 2.0 D "$UPD" >/dev/null
G=$(mk agent 1.0)
P=$(mk fixture probe)
MKCRX_TARGET=$SCRATCH/target-mkcrx
cargo build -q --manifest-path $REPO/docs/design/linux-cef/spikes/ext-load/tools/mkcrx/Cargo.toml --target-dir $MKCRX_TARGET
for x in C:$C D:$D; do
  n=${x%%:*}; id=${x#*:}
  for v in 1.0 2.0; do $MKCRX_TARGET/debug/mkcrx $E/keys/$n.pem $E/exts/$id/$v $E/crx/rep$n-$v.crx >/dev/null; done
done
cp $E/crx/repD-*.crx $E/site/updates/
cat > $E/ids.env <<EOF
A=$A
B=$B
C=$C
D=$D
G=$G
P=$P
X=$E/exts
EOF
cat $E/ids.env
