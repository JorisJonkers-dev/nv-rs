#!/usr/bin/env bash
# The whole Anthropic side in one flow: admin login, the dedicated workspace, workload identity federation for
# this repository, the end-to-end check, removal of the old API key and a first sync. Idempotent: rerun it.
#
#   scripts/fork/setup-anthropic.sh            phase 1: login, workspace, federation, GitHub variables
#   scripts/fork/setup-anthropic.sh --verify   phase 2 (after the baseline pull request is merged to fork):
#                                              run the check workflow, remove the old key, optional first sync
#
# Phase 2 is separate because the check and the sync run in the `fork-sync` environment, which only runs on
# the fork branch can use, and the workflows exist on it only once the baseline pull request is merged.
# Needs: ant (logged in as an organisation admin during phase 1), gh (admin on the repository), python3, curl, jq.
# Walkthrough and background: docs/fork/WIF.md.
set -euo pipefail

repo=${REPO:-JorisJonkers-dev/nv-rs}
env_name=${ENV_NAME:-fork-sync}
ws_name=${WORKSPACE_NAME:-nv-rs-fork}
api=${ANTHROPIC_API_BASE:-https://api.anthropic.com}
here=$(dirname "$(readlink -f "$0")")
mode=${1:-setup}

say() { printf '\n== %s\n' "$*"; }
die() { printf 'setup-anthropic: %s\n' "$*" >&2; exit 1; }
for c in ant gh python3 curl jq; do command -v "$c" >/dev/null || die "missing command: $c"; done
gh auth status >/dev/null 2>&1 || die "gh is not logged in (gh auth login)"

on_fork() { gh api "repos/$repo/contents/.github/workflows/$1?ref=fork" --silent 2>/dev/null; }

if [ "$mode" = "--verify" ]; then
  on_fork fork-wif-check.yml || die "the check workflow is not on the fork branch yet: merge the baseline pull request first, then rerun with --verify"
  say "1/3 Run 'Claude Federation Check' on the fork branch"
  gh workflow run fork-wif-check.yml -R "$repo" --ref fork
  sleep 8
  id=$(gh run list -R "$repo" --workflow fork-wif-check.yml --limit 1 --json databaseId --jq '.[0].databaseId')
  gh run watch "$id" -R "$repo" --exit-status || die "the federation check failed. Read the log: gh run view $id -R $repo --log-failed . The reason is also on the Console's Workload identity page, tab Authentication history."
  say "2/3 Remove the old API key (it would override federation)"
  for scope in "--env $env_name" ""; do
    # shellcheck disable=SC2086
    if gh secret list $scope -R "$repo" 2>/dev/null | grep -q '^ANTHROPIC_API_KEY'; then
      read -r -p "Delete the secret ANTHROPIC_API_KEY ${scope:+($scope)}? [y/N] " a
      # shellcheck disable=SC2086
      [ "$a" = y ] && gh secret delete ANTHROPIC_API_KEY $scope -R "$repo" && echo "deleted"
    fi
  done
  echo "Now delete the key itself in the Console: https://platform.claude.com/settings/keys"
  read -r -p "Press Enter when done: " _
  say "3/3 First sync (optional)"
  read -r -p "Run 'Upstream Sync' now? [y/N] " a
  if [ "$a" = y ]; then
    on_fork fork-sync.yml || die "fork-sync.yml is not on the fork branch"
    gh workflow run fork-sync.yml -R "$repo" --ref fork && echo "started: gh run list -R $repo --workflow fork-sync.yml"
  fi
  echo; echo "Done. Federation works and no API key is stored."
  exit 0
fi

say "1/5 Log in to Anthropic as an organisation admin (a browser opens)"
ant --profile admin auth login --admin
token=$(ant --profile admin auth print-credentials --access-token)
[ -n "$token" ] || die "no access token from ant"
export ANTHROPIC_AUTH_TOKEN=$token
unset ANTHROPIC_API_KEY
trap 'ant --profile admin auth logout >/dev/null 2>&1 || true; unset ANTHROPIC_AUTH_TOKEN' EXIT

call() { curl -sS --fail-with-body -H "authorization: Bearer $token" -H "anthropic-version: 2026-10-21" -H "content-type: application/json" "$@"; }

say "2/5 The dedicated workspace '$ws_name'"
ws=$(call "$api/v1/organizations/workspaces?limit=100" 2>/dev/null | jq -r --arg n "$ws_name" '.data[]? | select(.name==$n and (.archived_at==null)) | .id' | head -n 1 || true)
if [ -z "$ws" ]; then
  ws=$(call -X POST "$api/v1/organizations/workspaces" -d "$(jq -n --arg n "$ws_name" '{name:$n}')" 2>/dev/null | jq -r '.id // empty' || true)
  [ -n "$ws" ] && echo "created $ws"
else
  echo "found $ws"
fi
if [ -z "$ws" ]; then
  echo "The API would not list or create workspaces with this login. Create it in the Console instead:"
  echo "  https://platform.claude.com/settings/workspaces  (name: $ws_name)"
  read -r -p "Paste its id (wrkspc_...): " ws
fi
case $ws in wrkspc_*) ;; *) die "'$ws' is not a workspace id" ;; esac

say "3/5 The monthly spend limit (the Console is the only place I could verify for this)"
echo "  Set it on the workspace's Limits tab: https://platform.claude.com/settings/workspaces"
read -r -p "Press Enter once the monthly limit (for example \$5) is set: " _

say "4/5 Workload identity federation and the GitHub variables"
gh api "repos/$repo/environments/$env_name" --silent 2>/dev/null || {
  gh api -X PUT "repos/$repo/environments/$env_name" --silent
  echo "created the '$env_name' environment (scripts/fork/setup-repo.sh restricts it to the fork branch)"
}
python3 -I "$here/setup-anthropic-wif.py" --workspace "$ws" --repo "$repo" --env "$env_name"

say "5/5 Admin login removed (on exit). Next"
if on_fork fork-wif-check.yml; then
  echo "The workflows are on the fork branch: run  scripts/fork/setup-anthropic.sh --verify"
else
  echo "Merge the baseline pull request to the fork branch, then run  scripts/fork/setup-anthropic.sh --verify"
fi
