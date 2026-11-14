#!/bin/sh
# Stages the Linux release tree and writes the tarball.
#   package.sh <nv-viewer binary> <version> <out dir> [release-files dir]
# The tarball is always named nv-rs-linux-x86_64-dev.tar.gz: nvmod's install-nv-rs fetches that name from the latest
# release and looks SHA256SUMS up by it. The version is the name of the folder inside and is in release-manifest.json.
# A tree holds plain files and folders only (nvmod refuses a link or a path that leaves the folder).
set -eu
[ "$#" -ge 3 ] || { echo "usage: package.sh <nv-viewer> <version> <out dir> [release-files dir]" >&2; exit 2; }
viewer="$1"
version="$2"
out="$3"
files="${4:-}"
src="$(dirname "$(readlink -f "$0")")"
name="nv-rs-${version}-linux-x86_64"
asset="nv-rs-linux-x86_64-dev.tar.gz"

mkdir -p "$out"
stage="$out/$name"
rm -rf "$stage"
mkdir -p "$stage/bin" "$stage/share/applications" "$stage/share/icons/hicolor/256x256/apps" "$stage/share/doc/nv-rs"
install -m 0755 "$viewer" "$stage/bin/nv-viewer"
install -m 0755 "$src/nv-rs-launch" "$stage/bin/nv-rs-launch"
install -m 0755 "$src/install.sh" "$stage/install.sh"
install -m 0644 "$src/nv-rs.desktop" "$stage/share/applications/nv-rs.desktop"
install -m 0644 "$src/nv-rs.png" "$stage/share/icons/hicolor/256x256/apps/nv-rs.png"
install -m 0644 "$src/README.txt" "$stage/share/doc/nv-rs/README.txt"
if [ -n "$files" ]; then
    mkdir -p "$stage/share/nv-rs"
    for f in "$files"/*.json; do
        install -m 0644 "$f" "$stage/share/nv-rs/"
    done
fi

tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='2000-01-01 00:00Z' -C "$out" -czf "$out/$asset" "$name"
echo "$out/$asset"
