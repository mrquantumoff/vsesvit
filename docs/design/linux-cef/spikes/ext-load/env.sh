# Sourced by every ext-load script. Paths come from the environment, never from this machine:
#   VSESVIT_SCRATCH  scratch root (default ~/vsesvit-scratch); env dir, target dir and CEF live under it
#   VSESVIT_REPO     the vsesvit checkout (default: the git top level of this script), for tests/fixtures
#   CARGO_TARGET_DIR build output (default $VSESVIT_SCRATCH/target-extload)
# The private X display is always :92, started by start-xvfb.sh. No display comes from the
# environment (design section 7.3): SPIKE_DISPLAY is refused.
if [ -n "${SPIKE_DISPLAY+set}" ]; then
  echo "env.sh: refusing SPIKE_DISPLAY=$SPIKE_DISPLAY; ext-load runs only on its private :92" >&2
  exit 1
fi
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SCRATCH=${VSESVIT_SCRATCH:-$HOME/vsesvit-scratch}
REPO=${VSESVIT_REPO:-$(git -C "$HERE" rev-parse --show-toplevel 2>/dev/null)}
E=$SCRATCH/extload-env
TARGET=${CARGO_TARGET_DIR:-$SCRATCH/target-extload}
BIN=${BIN:-$TARGET/debug/vsesvit-spike}
DPY=:92
SITE=http://127.0.0.1:8792
# GPUI renders through lavapipe (software Vulkan); the ICD file name differs between distros.
LVP=${VK_ICD_FILENAMES:-$(ls /usr/share/vulkan/icd.d/lvp_icd*.json 2>/dev/null | head -1)}
export VSESVIT_SCRATCH=$SCRATCH VSESVIT_REPO=$REPO
mkdir -p "$E"
# Extension ids derive from the throwaway keys fixtures.sh generates; it writes them here.
if [ -f "$E/ids.env" ]; then source "$E/ids.env"; fi
