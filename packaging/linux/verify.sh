#!/usr/bin/env bash
# Installs each Linux artifact from target/dist in a clean container of its distribution and
# checks what docs/design/packaging.md promises: the binary runs, the marker names the format,
# the desktop entry validates, and the GStreamer libav and VP9 decoders resolve. Fedora's
# ffmpeg-free has no H.264, so its libav check uses AAC.
# Usage: packaging/linux/verify.sh [deb|rpm|pacman|appimage|flatpak]... (default: all present)
set -euo pipefail
dist="${CARGO_TARGET_DIR:-$(cd "$(dirname "$0")/../.." && pwd)/target}/dist"
version="$(tr -d '\r' < "$(dirname "$0")/../../Cargo.toml" | sed -n 's/^version = "\(.*\)"$/\1/p')"
formats=("$@")
[ ${#formats[@]} -gt 0 ] || formats=(deb rpm pacman appimage flatpak)

# Runs `script` in `image` with the artifacts under /dist; any failing command fails the format.
container() {
  local image=$1 script=$2
  shift 2
  docker run --rm -v "$dist:/dist:ro" "$@" "$image" bash -o pipefail -ec "$script"
}

# Shell text run inside the container after installing; $1 is the libav element to look for.
checks() {
  echo "
echo '--- vsesvit --version'; vsesvit --version
echo '--- marker'; cat /usr/lib/vsesvit/package-format; readlink -f /usr/bin/vsesvit
echo '--- desktop-file-validate'; desktop-file-validate /usr/share/applications/dev.mrquantumoff.vsesvit.desktop; echo ok
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
      # (docs/design/self-test.md) must pass; then a page served by a local HTTP server must
      # reach a normal run, with the WebKit helper processes running out of the image. Both runs
      # get relative paths from /tmp, which AppRun must resolve there and not inside the image.
      # The browser runs as an ordinary user with bubblewrap installed, and the container lets
      # bwrap make its namespaces and mount /proc, so the web processes must run in the sandbox:
      # under bwrap and without WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS. Where the host still
      # refuses user namespaces, AppRun must say it turned the sandbox off instead.
      container ubuntu:26.04 "export DEBIAN_FRONTEND=noninteractive; apt-get update -qq >/dev/null
        apt-get install -y -qq xvfb python3 libgl1 libegl1 libgles2 libasound2t64 fontconfig fonts-dejavu-core shared-mime-info procps bubblewrap >/dev/null
        ! { dpkg -l libwebkitgtk-6.0-4 libgtk-4-1 2>/dev/null || true; } | grep -q '^ii' || { echo 'container must not have WebKitGTK'; exit 1; }
        useradd -m -d /tmp/home user
        cp /dist/Vsesvit_${version}_amd64.AppImage /tmp/app && chmod +x /tmp/app
        mkdir -p /tmp/site
        echo '<title>appimage proof</title><p>hello' > /tmp/site/index.html
        cd /tmp
        as_user() { runuser -u user -- env HOME=/tmp/home APPIMAGE_EXTRACT_AND_RUN=1 \"\$@\"; }
        echo '--- --version'; as_user /tmp/app --version
        echo '--- self-test'
        as_user xvfb-run -a -s '-screen 0 1280x800x24' /tmp/app --self-test out || { echo 'self-test failed'; cat /tmp/out/report.json; exit 1; }
        cat /tmp/out/report.json
        echo '--- page load'
        python3 -m http.server 8000 --bind 127.0.0.1 --directory /tmp/site > /tmp/http.log 2>&1 &
        as_user xvfb-run -a -s '-screen 0 1280x800x24' /tmp/app --profile-dir=profile http://127.0.0.1:8000/index.html > /tmp/app.log 2>&1 &
        sleep 12
        echo '--- app stderr'; awk 'NF && n++ < 20' /tmp/app.log
        pgrep -af 'WebKit(Web|Network|GPU)Process' | cut -d' ' -f1-2 || { echo 'no WebKit helper processes'; exit 1; }
        grep -m1 'GET /index.html' /tmp/http.log || { echo 'no page request seen'; exit 1; }
        test -d /tmp/profile || { echo 'the profile is not in /tmp'; exit 1; }
        echo '--- sandbox'
        web=\$(ps -eo pid=,comm= | awk '\$2 ~ /^WebKitWebProc/ { print \$1 }')
        test -n \"\$web\" || { echo 'no web process'; exit 1; }
        if runuser -u user -- bwrap --ro-bind / / --proc /proc --dev /dev --unshare-all true; then
          for pid in \$web; do
            parent=\$(cat /proc/\$(ps -o ppid= -p \$pid | tr -d ' ')/comm)
            echo \"web process \$pid runs under \$parent\"
            test \"\$parent\" = bwrap || { echo 'the web process is not sandboxed'; exit 1; }
            environ=\$(tr '\0' '\n' < /proc/\$pid/environ)
            ! grep -q '^WEBKIT_DISABLE_SANDBOX' <<< \"\$environ\" || { echo 'the sandbox is turned off'; exit 1; }
          done
        else
          echo 'bwrap cannot make namespaces in this container; checking the fallback'
          grep -m1 'web pages run without the sandbox' /tmp/app.log || { echo 'AppRun did not say the sandbox is off'; exit 1; }
        fi" --security-opt seccomp=unconfined --security-opt apparmor=unconfined --security-opt systempaths=unconfined --cap-add SYS_PTRACE ;;
    flatpak)
      flatpak install --user -y --noninteractive --reinstall "$dist/Vsesvit_${version}_x86_64.flatpak" >/dev/null
      echo "--- flatpak run --version"; flatpak run dev.mrquantumoff.vsesvit --version
      echo "--- marker"; flatpak run --command=cat dev.mrquantumoff.vsesvit /app/lib/vsesvit/package-format
      echo "--- gst-inspect"; flatpak run --command=gst-inspect-1.0 dev.mrquantumoff.vsesvit avdec_h264 | grep Long-name
      flatpak run --command=gst-inspect-1.0 dev.mrquantumoff.vsesvit vp9dec | grep Long-name ;;
  esac
done
