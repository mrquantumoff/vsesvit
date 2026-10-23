# Sourced by every ext-api script. Paths come from the environment, never from this machine:
#   VSESVIT_SCRATCH  scratch root (default ~/vsesvit-scratch); env dir and target dir live under it
#   VSESVIT_REPO     the vsesvit checkout (default: the git top level of this script), for tests/fixtures
#   CARGO_TARGET_DIR build output (default $VSESVIT_SCRATCH/target-extapi)
#   SPIKE_DISPLAY    private X display (default :93, started by start-display.sh)
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SCRATCH=${VSESVIT_SCRATCH:-$HOME/vsesvit-scratch}
REPO=${VSESVIT_REPO:-$(git -C "$HERE" rev-parse --show-toplevel 2>/dev/null)}
E=$SCRATCH/extapi-env
TARGET=${CARGO_TARGET_DIR:-$SCRATCH/target-extapi}
BIN=${BIN:-$TARGET/debug/vsesvit-spike}
DPY=${SPIKE_DISPLAY:-:93}
# Fixture extensions run from a copy: Chromium writes DNR indexes (_metadata/) into unpacked dirs.
FX=$E/fixtures-ext
SX=$HERE/exts
SITE_DIR=$REPO/tests/fixtures/site
LVP=${VK_ICD_FILENAMES:-$(ls /usr/share/vulkan/icd.d/lvp_icd*.json 2>/dev/null | head -1)}
export VSESVIT_SCRATCH=$SCRATCH VSESVIT_REPO=$REPO SPIKE_ENV=$E
mkdir -p "$E"
