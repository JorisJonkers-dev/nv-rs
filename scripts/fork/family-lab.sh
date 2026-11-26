#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# One typed family per command: generate its small crate, then build and run its Diff cases.
# Usage: scripts/fork/family-lab.sh <family> [libtest filter ...]
#        scripts/fork/family-lab.sh --list
set -euo pipefail

FORK="$(cd -- "$(dirname -- "$0")" && pwd)"
REPO="$(cd -- "$FORK/../.." && pwd)"

if [ "${1:-}" = "--list" ]; then
    exec python3 -I "$FORK/family_lab.py" families
fi
if [ $# -lt 1 ]; then
    echo "usage: $0 <family> [libtest filter ...]" >&2
    exit 2
fi

family="$1"
shift
out="$REPO/target/family-lab/$family"
python3 -I "$FORK/family_lab.py" generate "$family" "$out"

# One cargo process, one lock and one memory cap for every worker on this machine.
if [ $# -gt 0 ] && [ "$1" != "--" ]; then
    set -- -- "$@"
fi
exec flock ~/.cache/nv-workers/cargo.lock ~/.local/bin/capped 10G \
    env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$HOME/.cache/cargo-targets/nv-rs-gw" \
    cargo test --manifest-path "$out/Cargo.toml" --test "typed_$family" "$@"
