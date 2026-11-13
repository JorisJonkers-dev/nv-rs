#!/usr/bin/env bash
# One-time: give the fork repository its final branch layout.
#   main  = a pure copy of upstream's main, kept current by GitHub's own sync (no ruleset: nothing but the sync writes it)
#   fork  = our line (the default branch): upstream's commits replayed with Conventional Commit titles, plus our own
# Needs admin rights on the repository. Idempotent. DRY_RUN=1 prints what it would do.
#
# What it does, in order:
#   1. creates `fork` from main (they are identical today: the fork point), if it does not exist
#   2. makes `fork` the default branch
#   3. retargets open pull requests that point at main to fork (the baseline pull request)
#   4. removes the old ruleset "Main" (it would block GitHub's sync of main) and applies the rulesets in .github/rulesets
#   5. restricts the fork-sync environment to the fork branch
#   6. brings main up to upstream's tip with GitHub's own sync
set -euo pipefail

repo=${1:-JorisJonkers-dev/nv-rs}
here=$(dirname "$(readlink -f "$0")")
dry=${DRY_RUN:-0}
run() { if [ "$dry" = 1 ]; then printf '   + %s\n' "$*"; else "$@"; fi; }
say() { printf '\n== %s\n' "$*"; }

say "1/6 branch fork"
if gh api "repos/$repo/git/ref/heads/fork" --silent 2>/dev/null; then
  echo "   exists"
else
  sha=$(gh api "repos/$repo/git/ref/heads/main" --jq .object.sha)
  run gh api -X POST "repos/$repo/git/refs" -f ref=refs/heads/fork -f sha="$sha" --silent
  echo "   created at ${sha:0:8}"
fi

say "2/6 default branch"
run gh repo edit "$repo" --default-branch fork

say "3/6 pull requests that target main"
for n in $(gh pr list -R "$repo" --base main --state open --json number --jq '.[].number'); do
  run gh pr edit "$n" -R "$repo" --base fork
  echo "   #$n now targets fork"
done

say "4/6 rulesets"
old=$(gh api "repos/$repo/rulesets" --jq '.[] | select(.name=="Main") | .id' 2>/dev/null || true)
if [ -n "$old" ]; then run gh api -X DELETE "repos/$repo/rulesets/$old" --silent; echo "   removed the old ruleset 'Main' ($old)"; fi
if [ "$dry" = 1 ]; then echo "   + $here/apply-ruleset.sh $repo"; else bash "$here/apply-ruleset.sh" "$repo"; fi

say "5/6 environment fork-sync: only the fork branch"
if gh api "repos/$repo/environments/fork-sync" --silent 2>/dev/null; then
  for id in $(gh api "repos/$repo/environments/fork-sync/deployment-branch-policies" --jq '.branch_policies[] | select(.name!="fork") | .id'); do
    run gh api -X DELETE "repos/$repo/environments/fork-sync/deployment-branch-policies/$id" --silent
  done
  if ! gh api "repos/$repo/environments/fork-sync/deployment-branch-policies" --jq '.branch_policies[].name' | grep -qx fork; then
    run gh api -X POST "repos/$repo/environments/fork-sync/deployment-branch-policies" -f name=fork -f type=branch --silent
  fi
  echo "   done"
else
  echo "   the environment does not exist yet: scripts/fork/setup-repo.sh creates it with the right branch"
fi

say "6/6 bring main up to upstream (GitHub's own sync, fast-forward only)"
run gh repo sync "$repo" --branch main || echo "   !! the sync was refused: see docs/fork/KEEPING-IN-SYNC.md (a fast-forward is impossible only if main has commits of its own)"

echo
echo "Done. Check: scripts/fork/setup-repo.sh --audit"
echo "Next: when the baseline pull request is green, merge it (it now targets fork)."
