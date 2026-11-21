#!/usr/bin/env bash
# Fork-only. Proves the fork has not diverged from upstream: at the commit upstream was last replayed
# to, the fork's tree outside the overlay must equal upstream's tree at that commit, apart from the
# files in .fork/allowed-divergence (the README notice), and every file only the fork has must be listed in
# .fork/overlay-paths. Prints how far behind the fork is.
set -euo pipefail
# The upstream copy is the fork repository's own `main` (kept by GitHub's sync); run this on the fork branch.
mirror=${MIRROR_REF:-origin/main}
git fetch --quiet --no-tags origin main
last=$(git log -1 --format=%B --grep='^Upstream-Commit: [0-9a-f]\{40\}$' HEAD | sed -n 's/^Upstream-Commit: \([0-9a-f]\{40\}\)$/\1/p' | tail -n 1)
[[ -n $last ]] || last=$(git merge-base HEAD "$mirror")
# allowed-divergence is the old list (frozen) and one file per entry in .fork/allowed-divergence.d/ (nv-rs#438).
shopt -s nullglob
mapfile -t skip < <(sed -e 's/#.*//' -e 's/[[:space:]]*$//' -e '/^[[:space:]]*$/d' .fork/overlay-paths .fork/allowed-divergence .fork/allowed-divergence.d/*)
shopt -u nullglob
# The fork does not use upstream's workflows (the replay drops their changes), so they are outside the comparison too.
skip+=(.github/workflows/)
spec=(); for p in "${skip[@]}"; do spec+=(":(exclude)$p"); done
# HEAD can hold fork-only commits after the last replayed one, so compare the file lists and contents
# of everything upstream has: the fork's version of each upstream file must equal upstream's.
changed=$(git diff --name-only --diff-filter=DM "$last" HEAD -- . "${spec[@]}" || true)
echo "behind upstream by $(git rev-list --count "$last..$mirror") commits (replayed to ${last:0:8})"
# Every file only the fork has (not in upstream's tree at that commit; workflows aside) must be listed in
# .fork/overlay-paths (the file, or a folder ending in /): the replay treats an unlisted path as upstream's.
mapfile -t overlay < <(sed -e 's/#.*//' -e 's/[[:space:]]*$//' -e '/^[[:space:]]*$/d' .fork/overlay-paths)
unlisted=()
while IFS= read -r f; do
  [[ $f == .github/workflows/* ]] && continue
  listed=false
  for o in "${overlay[@]}"; do
    if [[ $f == "$o" || ( $o == */ && $f == "$o"* ) ]]; then listed=true; break; fi
  done
  $listed || unlisted+=("$f")
done < <(LC_ALL=C comm -23 <(git -c core.quotePath=false ls-tree -r --name-only HEAD | LC_ALL=C sort) \
  <(git -c core.quotePath=false ls-tree -r --name-only "$last" | LC_ALL=C sort))
status=0
if [[ -n $changed ]]; then
  echo "DIVERGED from upstream at ${last:0:8}, outside the overlay:" >&2
  echo "$changed" >&2
  status=1
fi
if ((${#unlisted[@]})); then
  echo "fork-only files missing from .fork/overlay-paths:" >&2
  printf '%s\n' "${unlisted[@]}" >&2
  status=1
fi
((status == 0)) || exit 1
echo "no divergence outside the overlay and .fork/allowed-divergence; every fork-only file is in .fork/overlay-paths"
