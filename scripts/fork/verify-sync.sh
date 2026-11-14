#!/usr/bin/env bash
# Fork-only. Proves the fork has not diverged from upstream: at the commit upstream was last replayed
# to, the fork's tree outside the overlay must equal upstream's tree at that commit, apart from the
# files in .fork/allowed-divergence (the README notice). Prints how far behind the fork is.
set -euo pipefail
# The upstream copy is the fork repository's own `main` (kept by GitHub's sync); run this on the fork branch.
mirror=${MIRROR_REF:-origin/main}
git fetch --quiet --no-tags origin main
last=$(git log -1 --format=%B --grep='^Upstream-Commit: [0-9a-f]\{40\}$' HEAD | sed -n 's/^Upstream-Commit: \([0-9a-f]\{40\}\)$/\1/p' | tail -n 1)
[[ -n $last ]] || last=$(git merge-base HEAD "$mirror")
mapfile -t skip < <(sed -e 's/#.*//' -e 's/[[:space:]]*$//' -e '/^[[:space:]]*$/d' .fork/overlay-paths .fork/allowed-divergence)
spec=(); for p in "${skip[@]}"; do spec+=(":(exclude)$p"); done
# HEAD can hold fork-only commits after the last replayed one, so compare the file lists and contents
# of everything upstream has: the fork's version of each upstream file must equal upstream's.
changed=$(git diff --name-only --diff-filter=DM "$last" HEAD -- . "${spec[@]}" || true)
echo "behind upstream by $(git rev-list --count "$last..$mirror") commits (replayed to ${last:0:8})"
if [[ -n $changed ]]; then
  echo "DIVERGED from upstream at ${last:0:8}, outside the overlay:" >&2
  echo "$changed" >&2
  exit 1
fi
echo "no divergence outside the overlay and .fork/allowed-divergence"
