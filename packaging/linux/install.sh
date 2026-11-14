#!/bin/sh
# Installs an unpacked release tree (the folder this script is in) under a prefix.
#   ./install.sh [--prefix DIR]     default: ~/.local
# Puts nv-viewer and nv-rs-launch in DIR/bin, the desktop entry in DIR/share/applications (with the launcher's full
# path) and the icon under DIR/share/icons. Nothing outside DIR is touched and no game file is read.
set -eu
prefix="${HOME:?}/.local"
if [ "${1:-}" = "--prefix" ]; then
    [ "$#" -eq 2 ] || { echo "usage: install.sh [--prefix DIR]" >&2; exit 2; }
    prefix="$2"
elif [ "$#" -ne 0 ]; then
    echo "usage: install.sh [--prefix DIR]" >&2
    exit 2
fi
here="$(dirname "$(readlink -f "$0")")"
mkdir -p "$prefix/bin" "$prefix/share/applications" "$prefix/share/icons/hicolor/256x256/apps" "$prefix/share/doc/nv-rs"
install -m 0755 "$here/bin/nv-viewer" "$prefix/bin/nv-viewer"
install -m 0755 "$here/bin/nv-rs-launch" "$prefix/bin/nv-rs-launch"
sed "s|^Exec=nv-rs-launch|Exec=$prefix/bin/nv-rs-launch|" "$here/share/applications/nv-rs.desktop" >"$prefix/share/applications/nv-rs.desktop"
install -m 0644 "$here/share/icons/hicolor/256x256/apps/nv-rs.png" "$prefix/share/icons/hicolor/256x256/apps/nv-rs.png"
install -m 0644 "$here/share/doc/nv-rs/README.txt" "$prefix/share/doc/nv-rs/README.txt"
echo "Installed under $prefix. Next:"
echo "  $prefix/bin/nv-rs-launch --set-data <your Fallout New Vegas folder>"
echo "  $prefix/bin/nv-rs-launch --check"
case ":$PATH:" in
*":$prefix/bin:"*) ;;
*) echo "($prefix/bin is not on your PATH)" ;;
esac
