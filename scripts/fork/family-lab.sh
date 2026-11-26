#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Convenience wrapper for POSIX shells. The portable entry point is family_lab.py,
# which works on Windows, Linux and macOS without this script.
# Usage: scripts/fork/family-lab.sh <family> [libtest filter ...]
#        scripts/fork/family-lab.sh --list
set -euo pipefail

FORK="$(cd -- "$(dirname -- "$0")" && pwd)"
exec python3 -I "$FORK/family_lab.py" run "$@"
