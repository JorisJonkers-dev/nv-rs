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
git merge-base --is-ancestor "$last" "$up" || die "upstream main no longer contains $last: upstream rewrote history, choose the fork point again by hand"
[[ $last != "$up" ]] || { echo "already in sync at ${up:0:8}"; exit 0; }

mapfile -t commits < <(git rev-list --reverse --topo-order --no-merges "$last..$up")
((${#commits[@]})) || { echo "nothing to replay (only merge commits)"; exit 0; }

git switch --quiet -C "$branch" "$base"
msg=$(mktemp)
trap 'rm -f "$msg"' EXIT
for c in "${commits[@]}"; do
  # An upstream commit that adds or changes a workflow would run with this repository's secrets once it is
  # on the fork branch, and the sync's token may not push workflow files anyway. A person reads such a commit first.
  if [[ ${ALLOW_WORKFLOW_CHANGES:-0} != 1 ]] && git diff-tree --no-commit-id --name-only -r "$c" | grep -q '^\.github/workflows/'; then
    die "upstream commit ${c:0:12} changes .github/workflows: read it, then apply it by hand or rerun with ALLOW_WORKFLOW_CHANGES=1"
  fi
  if ! git cherry-pick --no-commit "$c" >/dev/null 2>&1; then
    git cherry-pick --abort >/dev/null 2>&1 || git reset --quiet --hard HEAD
    die "upstream commit ${c:0:12} does not apply on top of the fork: fix by hand (git cherry-pick $c), keep its message format, then rerun"
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
    printf '\nUpstream-Commit: %s\n' "$c"
  } >"$msg"
  GIT_AUTHOR_NAME=$(git log -1 --format=%an "$c") GIT_AUTHOR_EMAIL=$(git log -1 --format=%ae "$c") \
    GIT_AUTHOR_DATE=$(git log -1 --format=%aI "$c") git commit --quiet --allow-empty --file "$msg" \
    || die "the commit for ${c:0:12} was refused (a commit-msg hook?); nothing was pushed"
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
