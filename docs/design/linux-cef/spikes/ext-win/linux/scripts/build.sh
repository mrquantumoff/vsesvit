#!/bin/bash
# Copy the spike from the Windows checkout into WSL's own file system and build the Linux shell.
# build.sh [SRC] [fixtures]: with "fixtures", also rebuild the extensions, CRX files and site.
SRC=${1:-/mnt/c/Users/mrqua/git/vsesvit/docs/design/linux-cef/spikes/ext-win}
SCRATCH=${VSESVIT_SCRATCH:-$HOME/vsesvit-scratch}
XW=$SCRATCH/ext-win
mkdir -p $XW
rsync -a --delete --exclude target "$SRC/" $XW/src/
# Files written on Windows may have CRLF; bash and python need LF.
grep -rlI $'\r' --include='*.sh' --include='*.py' $XW/src | xargs -r sed -i $'s/\r$//'
chmod +x $XW/src/linux/scripts/*.sh
cd $XW/src
CEF_PATH=$SCRATCH/cef CARGO_TARGET_DIR=$XW/target RUSTFLAGS="-L native=$SCRATCH/linklibs" cargo build -p vsesvit-extwin-linux 2>&1 | grep -E "^(error|warning: unused)|^\s+-->|Finished" | head -40
if [ "$2" = fixtures ]; then
  python3 -I $XW/src/fixtures/fixtures.py $XW/src $XW/fx $SCRATCH/target-mkcrx/debug/mkcrx
  mkdir -p /mnt/c/Users/mrqua/git/vsesvit/.audit/scratch/ext-win/fx
  rsync -a --delete --exclude keys $XW/fx/ /mnt/c/Users/mrqua/git/vsesvit/.audit/scratch/ext-win/fx/
fi
