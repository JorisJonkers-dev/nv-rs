#!/usr/bin/env bash
# Fork-only. Replays every upstream commit that the fork branch does not have yet onto a branch
# of the fork, one commit each, with the SAME change and a Conventional Commit message.
#
#   * the diff of each fork commit equals the diff of the upstream commit (git cherry-pick);
#   * author and author date are upstream's; the committer is whoever runs this;
#   * the message is rewritten: a Conventional Commit title (scripts/fork/rewrite_title.py),
#     the original body without coding-agent credits, #123 turned into slaterain/nv-rs#123 so it does
#     not link to this repository, and the trailer `Upstream-Commit: <sha>`, which is how the next run
#     knows where this one stopped;
#   * the fork branch is never rewritten: it only grows. Merge the branch by REBASE (not squash, which
#     would put the many messages into one).
#
# Usage: scripts/fork/replay-upstream.sh [--push]     (branch: sync/upstream)
# Stops on the first commit that does not apply (upstream touched a line the fork changed).
set -euo pipefail

# `main` of the fork repository is a pure copy of upstream's main, kept current by GitHub's own sync
# (`gh repo sync <repo> -b main`, or the Upstream Sync workflow). This script reads it and never talks to
# upstream itself. The fork's own line is the branch `fork`.
fork_remote=${FORK_REMOTE:-origin}
mirror_branch=${MIRROR_BRANCH:-main}
base_branch=${BASE_BRANCH:-fork}
branch=${SYNC_BRANCH:-sync/upstream}
upstream_slug=slaterain/nv-rs
# Credits that are not people. Their trailers and "generated with" lines are dropped.
not_people_re=${NOT_PEOPLE_RE:-'(claude|anthropic|openai|codex|copilot|gemini|\[bot\]|github-actions|dependabot|renovate)'}
here=$(dirname "$(readlink -f "$0")")

die() { printf 'replay-upstream: %s\n' "$*" >&2; exit 1; }
# stop <reason> <commit-or-empty> <message>: records why the replay stopped for scripts/fork/sync_report.py (SYNC_STOP_FILE),
# then dies. reason is one of: conflict, partial, history, hook, other.
stop() {
  local reason=$1 commit=$2; shift 2
  if [[ -n ${SYNC_STOP_FILE:-} ]]; then
    {
      printf 'reason=%s\n' "$reason"
      printf 'commit=%s\n' "$commit"
      [[ -z $commit ]] || printf 'subject=%s\n' "$(git log -1 --format=%s "$commit" | tr -d '\r\n')"
    } >"$SYNC_STOP_FILE"
  fi
  die "$*"
}
# Conflict policy (docs/fork/KEEPING-IN-SYNC.md, "Who wins"): upstream is a source of imported commits, not a partner, and
# the fork may diverge on purpose. Where a path is the fork's own, the fork wins and the commit is replayed without its
# change to that path, noted in SYNC_DROPPED_FILE (one "<sha12> <path>" per line, shown in the sync summary):
#   * every path in .fork/overlay-paths (a path upstream also has is a collision; the fork's version stays);
#   * .github/workflows (the fork does not use upstream's CI), unless WORKFLOW_POLICY=apply or ALLOW_WORKFLOW_CHANGES=1.
# A conflict in any other file, including one listed in .fork/allowed-divergence, stops the replay: upstream's change and
# the fork's both matter there, so a person merges them.
workflow_policy=${WORKFLOW_POLICY:-drop}
[[ ${ALLOW_WORKFLOW_CHANGES:-0} != 1 ]] || workflow_policy=apply
overlay=()
[[ ! -f .fork/overlay-paths ]] || mapfile -t overlay < <(sed -e 's/#.*//' -e 's/[[:space:]]*$//' -e '/^[[:space:]]*$/d' .fork/overlay-paths)
fork_owns() { # <path>
  local f=$1 o
  for o in "${overlay[@]}"; do
    [[ $f == "$o" || ( $o == */ && $f == "$o"* ) ]] && return 0
  done
  [[ $workflow_policy != apply && $f == .github/workflows/* ]]
}
drop_owned() { # <commit>: put every fork-owned path the commit touches back to the fork's version
  local c=$1 f
  while IFS= read -r -d '' f; do
    fork_owns "$f" || continue
    if git cat-file -e "HEAD:$f" 2>/dev/null; then git checkout --quiet HEAD -- "$f"; else git rm -q -f --cached --ignore-unmatch -- "$f"; rm -f -- "$f"; fi
    [[ -z ${SYNC_DROPPED_FILE:-} ]] || printf '%s %s\n' "${c:0:12}" "$f" >>"$SYNC_DROPPED_FILE"
    dropped+=("$f")
  done < <(git diff-tree --no-commit-id --name-only -r -z "$c")
}
push=false
[[ ${1:-} == --push ]] && push=true

git rev-parse --git-dir >/dev/null 2>&1 || die "run inside the fork's clone"
[[ -z $(git status --porcelain --untracked-files=no) ]] || die "the work tree is not clean"
git fetch --quiet --no-tags "$fork_remote" "$mirror_branch" "$base_branch"
up=$(git rev-parse "$fork_remote/$mirror_branch")
base=$(git rev-parse "$fork_remote/$base_branch")

# Where the last run stopped: the newest Upstream-Commit trailer on the fork branch. Before the first
# run, the point the fork was cut from upstream (its history is upstream's own).
last=$(git log -1 --format=%B --grep='^Upstream-Commit: [0-9a-f]\{40\}$' "$base" \
  | sed -n 's/^Upstream-Commit: \([0-9a-f]\{40\}\)$/\1/p' | tail -n 1)
[[ -n $last ]] || last=$(git merge-base "$base" "$up") || die "the fork branch shares no history with the upstream copy"
git merge-base --is-ancestor "$last" "$up" || stop history "" "upstream main no longer contains $last: upstream rewrote history, choose the fork point again by hand"
[[ $last != "$up" ]] || { echo "already in sync at ${up:0:8}"; exit 0; }

mapfile -t commits < <(git rev-list --reverse --topo-order --no-merges "$last..$up")
((${#commits[@]})) || { echo "nothing to replay (only merge commits)"; exit 0; }

git switch --quiet -C "$branch" "$base"
msg=$(mktemp)
trap 'rm -f "$msg"' EXIT
for c in "${commits[@]}"; do
  dropped=()
  picked=true
  # A commit that adds `mod x;` to a fork-owned file upstream also has would arrive without that line and without the
  # fork's own version knowing the module: the rest of the commit then does not build. Stop before producing that branch.
  while IFS= read -r -d '' f; do
    fork_owns "$f" || continue
    git cat-file -e "$c^:$f" 2>/dev/null || continue
    git cat-file -e "HEAD:$f" 2>/dev/null || continue
    if git diff "$c^" "$c" -- "$f" | grep -Eq '^\+[[:space:]]*(pub(\([a-z]+\))?[[:space:]]+)?mod[[:space:]]+[A-Za-z0-9_]+[[:space:]]*;'; then
      stop partial "$c" "upstream commit ${c:0:12} adds a module line to $f, which .fork/overlay-paths holds out of the replay, so the commit would be applied in part and not build: remove $f from .fork/overlay-paths (list it in .fork/allowed-divergence), or apply the commit by hand"
    fi
  done < <(git diff-tree --no-commit-id --name-only -r -z "$c")
  git cherry-pick --no-commit "$c" >/dev/null 2>&1 || picked=false
  drop_owned "$c"
  if ! $picked && [[ -z $(git diff --name-only --diff-filter=U) ]]; then picked=true; fi
  if ! $picked; then
    git cherry-pick --abort >/dev/null 2>&1 || git reset --quiet --hard HEAD
    stop conflict "$c" "upstream commit ${c:0:12} does not apply on top of the fork: fix by hand (git cherry-pick $c), keep its message format, then rerun"
  fi
  title=$(python3 -I "$here/rewrite_title.py" "$c")
  link() { sed -E "s#(^|[^A-Za-z0-9/])\\#([0-9]+)#\\1$upstream_slug\\#\\2#g"; }
  {
    printf '%s\n\n' "$title"
    # Drop agent credits, and a Co-authored-by that only repeats the commit's own author.
    git log -1 --format=%b "$c" | grep -Eiv "^(Co-authored-by|Signed-off-by):.*${not_people_re}|^(Generated (with|by)|🤖)" \
      | awk -v n="$(git log -1 --format=%an "$c")" -v e="$(git log -1 --format=%ae "$c")" \
        'tolower($0) ~ /^co-authored-by:/ && (index(tolower($0), tolower(n)) || index(tolower($0), tolower(e))) { next } { print }' | link \
      | grep -Ev '^-{5,}$' | cat -s | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}'
    if ((${#dropped[@]})); then printf '\nNot replayed (paths the fork owns, so the fork version stays): %s\n' "${dropped[*]}"; fi
    printf '\nUpstream-Commit: %s\n' "$c"
  } >"$msg"
  GIT_AUTHOR_NAME=$(git log -1 --format=%an "$c") GIT_AUTHOR_EMAIL=$(git log -1 --format=%ae "$c") \
    GIT_AUTHOR_DATE=$(git log -1 --format=%aI "$c") git commit --quiet --allow-empty --file "$msg" \
    || stop hook "$c" "the commit for ${c:0:12} was refused (a commit-msg hook?); nothing was pushed"
  echo "${c:0:8}  $title"
done
echo "replayed ${#commits[@]} upstream commits onto $branch"

if $push; then
  git push --quiet --force-with-lease "$fork_remote" "$branch"
  echo "pushed $branch. Open the pull request and merge it by rebase:"
  echo "  gh pr create --repo JorisJonkers-dev/nv-rs --base $base_branch --head $branch --label allow-repository-hygiene-exception --title \"chore(sync): replay upstream up to ${up:0:8}\" --body \"Replays ${#commits[@]} upstream commits, identical changes, Conventional Commit messages.\""
else
  echo "review with: git log --oneline $base..$branch ; then rerun with --push"
fi
