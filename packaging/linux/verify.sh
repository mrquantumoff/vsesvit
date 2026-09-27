#!/usr/bin/env bash
# Installs each Linux artifact from target/dist in a clean container of its distribution and
# checks what docs/design/packaging.md promises: the binary runs, the marker names the format,
# the desktop entry validates, and the GStreamer libav and VP9 decoders resolve. Fedora's
# ffmpeg-free has no H.264, so its libav check uses AAC.
# Usage: packaging/linux/verify.sh [deb|rpm|pacman|appimage|flatpak]... (default: all present)
set -euo pipefail
dist="${CARGO_TARGET_DIR:-$(cd "$(dirname "$0")/../.." && pwd)/target}/dist"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$(dirname "$0")/../../Cargo.toml")"
formats=("$@")
[ ${#formats[@]} -gt 0 ] || formats=(deb rpm pacman appimage flatpak)

# Runs `script` in `image` with the artifacts under /dist.
container() {
  local image=$1 script=$2
  shift 2
  docker run --rm -v "$dist:/dist:ro" "$@" "$image" bash -ec "$script"
}

# Shell text run inside the container after installing; $1 is the libav element to look for.
checks() {
  echo "
echo '--- vsesvit --version'; vsesvit --version
echo '--- marker'; cat /usr/lib/vsesvit/package-format; readlink -f /usr/bin/vsesvit
echo '--- desktop-file-validate'; desktop-file-validate /usr/share/applications/dev.mrquantumoff.vsesvit.desktop && echo ok
echo '--- gst-inspect'; gst-inspect-1.0 $1 | grep Long-name; gst-inspect-1.0 vp9dec | grep Long-name
"
}

for format in "${formats[@]}"; do
  echo "===== $format"
  case $format in
    deb)
      container ubuntu:26.04 "export DEBIAN_FRONTEND=noninteractive; apt-get update -qq >/dev/null
        apt-get install -y -qq /dist/Vsesvit_${version}_amd64.deb desktop-file-utils gstreamer1.0-tools >/dev/null
        dpkg -s vsesvit | grep -E '^(Depends|Recommends)'; $(checks avdec_h264)" ;;
    rpm)
      container fedora:latest "dnf install -y -q /dist/Vsesvit-${version}-1.x86_64.rpm desktop-file-utils gstreamer1-plugins-base-tools >/dev/null
        rpm -q --requires --recommends vsesvit; $(checks avdec_aac)" ;;
    pacman)
      container archlinux:latest "pacman -Sy --noconfirm --needed -q desktop-file-utils >/dev/null 2>&1
        pacman -U --noconfirm /dist/vsesvit-${version}-1-x86_64.pkg.tar.zst >/dev/null
        pacman -Qi vsesvit | grep -E '^(Depends On|Optional Deps)'; $(checks avdec_h264)" ;;
    appimage)
      # A container has no FUSE, so the runtime extracts the image. The self-test
      # (docs/design/self-test.md) runs when the shell has it; until then the proof is a page
      # served by a local HTTP server and the WebKit helper processes running out of the image.
      container ubuntu:26.04 "export DEBIAN_FRONTEND=noninteractive; apt-get update -qq >/dev/null
        apt-get install -y -qq xvfb python3 libgl1 libegl1 libgles2 libasound2t64 fontconfig fonts-dejavu-core shared-mime-info procps >/dev/null
        ! dpkg -l libwebkitgtk-6.0-4 libgtk-4-1 2>/dev/null | grep -q '^ii' || { echo 'container must not have WebKitGTK'; exit 1; }
        cp /dist/Vsesvit_${version}_amd64.AppImage /tmp/app && chmod +x /tmp/app
        export APPIMAGE_EXTRACT_AND_RUN=1 HOME=/tmp/home; mkdir -p \$HOME /tmp/site
        echo '<title>appimage proof</title><p>hello' > /tmp/site/index.html
        echo '--- --version'; /tmp/app --version
        echo '--- self-test'
        xvfb-run -a -s '-screen 0 1280x800x24' /tmp/app --self-test /tmp/out && cat /tmp/out/report.json || true
        echo '--- page load'
        python3 -m http.server 8000 --bind 127.0.0.1 --directory /tmp/site > /tmp/http.log 2>&1 &
        xvfb-run -a -s '-screen 0 1280x800x24' /tmp/app http://127.0.0.1:8000/index.html > /tmp/app.log 2>&1 &
        sleep 12
        pgrep -af 'WebKit(Web|Network|GPU)Process' | cut -d' ' -f1-2 || echo 'no WebKit helper processes'
        grep -m1 'GET /index.html' /tmp/http.log || echo 'no page request seen'
        echo '--- app stderr'; grep -v '^$' /tmp/app.log | head -20" ;;
    flatpak)
      flatpak install --user -y --noninteractive --reinstall "$dist/Vsesvit_${version}_x86_64.flatpak" >/dev/null
      echo "--- flatpak run --version"; flatpak run dev.mrquantumoff.vsesvit --version
      echo "--- marker"; flatpak run --command=cat dev.mrquantumoff.vsesvit /app/lib/vsesvit/package-format
      echo "--- gst-inspect"; flatpak run --command=gst-inspect-1.0 dev.mrquantumoff.vsesvit avdec_h264 | grep Long-name
      flatpak run --command=gst-inspect-1.0 dev.mrquantumoff.vsesvit vp9dec | grep Long-name ;;
  esac
done
