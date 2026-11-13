#!/usr/bin/env bash
# Install the fork's git hooks: Conventional Commit messages, and no direct push to the fork branch or to main (the upstream copy).
set -euo pipefail
root="$(git rev-parse --show-toplevel)"
install -m 0755 "$root/scripts/fork/hooks/commit-msg" "$root/.git/hooks/commit-msg"
install -m 0755 "$root/scripts/fork/hooks/pre-push" "$root/.git/hooks/pre-push"
echo "Installed commit-msg and pre-push into $root/.git/hooks"
