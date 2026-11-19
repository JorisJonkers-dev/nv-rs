#!/bin/sh
# Stages the macOS release tree and writes the tarball.
#   package.sh <nv-viewer binary> <version> <out dir> [release-files dir]
# The binary is universal (arm64 and x86_64 joined with lipo). The tarball is always named
# nv-rs-macos-universal-dev.tar.gz, with SHA256SUMS-macos beside it in the release; the version is the name of the folder
# inside and is in release-manifest.json. A tree holds plain files and folders only (nvmod and the Toolbox refuse a
# link or a path that leaves the folder). Runs on macOS (bsdtar) and on Linux (GNU tar) alike.
set -eu
[ "$#" -ge 3 ] || { echo "usage: package.sh <nv-viewer> <version> <out dir> [release-files dir]" >&2; exit 2; }
viewer="$1"
version="$2"
out="$3"
files="${4:-}"
src="$(cd "$(dirname "$0")" && pwd)"
name="nv-rs-${version}-macos-universal"
asset="nv-rs-macos-universal-dev.tar.gz"

mkdir -p "$out"
stage="$out/$name"
rm -rf "$stage"
mkdir -p "$stage/bin" "$stage/share/doc/nv-rs"
cp "$viewer" "$stage/bin/nv-viewer"
chmod 0755 "$stage/bin/nv-viewer"
cp "$src/README.txt" "$stage/share/doc/nv-rs/README.txt"
chmod 0644 "$stage/share/doc/nv-rs/README.txt"
if [ -n "$files" ]; then
    mkdir -p "$stage/share/nv-rs"
    for f in "$files"/*.json; do
        cp "$f" "$stage/share/nv-rs/"
        chmod 0644 "$stage/share/nv-rs/$(basename "$f")"
    done
fi

# No AppleDouble (._*) files from macOS's tar.
COPYFILE_DISABLE=1 tar -C "$out" -czf "$out/$asset" "$name"
echo "$out/$asset"
