#!/usr/bin/env bash
# Builds DVRDesk-Native-Linux-<arch>.AppImage from an existing release build
# of dvrdesk-native and guide-history-service (see native-release.yml).
#
# libmpv is NOT bundled, for the same GPL-redistribution reason the Windows
# zip leaves it out (see native-release.yml's header): the app dlopens the
# system's libmpv.so.2, so the AppImage needs the distro's libmpv package
# (libmpv2 on Debian/Ubuntu, mpv-libs on Fedora, mpv on Arch). Nothing else
# is linked beyond libc, so no other libraries need bundling either.
#
# Usage: build-appimage.sh <out-dir> [target-dir]
#   target-dir defaults to native/target/release.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
native="$(cd "$here/../.." && pwd)"
repo="$(cd "$native/.." && pwd)"
out="$(mkdir -p "$1" && cd "$1" && pwd)"
target="${2:-$native/target/release}"
arch="$(uname -m)"

appimagetool="${APPIMAGETOOL:-}"
if [ -z "$appimagetool" ]; then
  appimagetool="$(mktemp -d)/appimagetool"
  curl -fsSL -o "$appimagetool" \
    "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${arch}.AppImage"
  chmod +x "$appimagetool"
fi

appdir="$(mktemp -d)/DVRDesk.AppDir"
mkdir -p "$appdir/usr/bin"
install -m 755 "$target/dvrdesk-native" "$appdir/usr/bin/"
# Next to the app, where deploy.rs looks for it, so Settings can install
# the guide history service on this PC.
install -m 755 "$repo/guide-history-service/target/release/guide-history-service" "$appdir/usr/bin/"
install -m 644 "$here/dvrdesk-native.desktop" "$appdir/"
install -m 644 "$repo/src-tauri/icons/128x128.png" "$appdir/dvrdesk-native.png"
cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
exec "$(dirname "$(readlink -f "$0")")/usr/bin/dvrdesk-native" "$@"
EOF
chmod 755 "$appdir/AppRun"

# Extract-and-run: CI runners have no FUSE to mount appimagetool itself.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$arch" "$appimagetool" --no-appstream \
  "$appdir" "$out/DVRDesk-Native-Linux-${arch}.AppImage"
