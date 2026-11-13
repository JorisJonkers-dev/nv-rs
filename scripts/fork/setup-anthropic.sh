#!/usr/bin/env bash
# The whole Anthropic side in one flow: admin login, the dedicated workspace, workload identity federation for
# this repository, the end-to-end check, removal of the old API key and a first sync. Idempotent: rerun it.
#
#   scripts/fork/setup-anthropic.sh            phase 1: login, workspace, federation, GitHub variables
#   scripts/fork/setup-anthropic.sh --diagnose log in and show WHICH organisation the login is for and what the Admin API
#                                              answers (read-only probes); changes nothing
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
[ "$mode" = "--diagnose" ] || [ "$mode" = "--verify" ] || [ "$mode" = setup ] || die "unknown option $mode"

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

# Anthropic has stability problems at times: retry what can be transient (network errors, 408, 425, 429, 5xx incl. 529
# "overloaded") up to RETRIES times with a growing pause. Anything else (401, 403, 404, ...) is not transient and is
# returned at once. Output: the response body; exit status 0 for 2xx, 1 otherwise.
RETRIES=${ANTHROPIC_RETRIES:-20}
last_code=000
call() { # call curl-args...
  local attempt=1 out code body wait
  while :; do
    out=$(curl -sS --max-time 60 -w '\n%{http_code}' -H "authorization: Bearer $token" -H "anthropic-version: 2026-10-21" \
      -H "content-type: application/json" "$@" 2>/dev/null) || out=$'\n000'
    code=${out##*$'\n'}; body=${out%$'\n'*}; last_code=$code
    case $code in
      2??) printf '%s' "$body"; return 0 ;;
      000|408|425|429|5??)
        [ "$attempt" -ge "$RETRIES" ] && { printf '%s' "$body"; echo "   gave up after $RETRIES attempts (HTTP $code): Anthropic may be having an incident; rerun later" >&2; return 1; }
        wait=$(( 2 ** (attempt < 5 ? attempt : 5) )); [ "$wait" -gt 30 ] && wait=30
        echo "   HTTP $code, retry $attempt/$((RETRIES - 1)) in ${wait}s" >&2
        sleep "$wait"; attempt=$((attempt + 1)) ;;
      *) printf '%s' "$body"; return 1 ;;
    esac
  done
}
profile_json=~/.config/anthropic/configs/admin.json
org_id=$(jq -r '.organization_id // empty' "$profile_json" 2>/dev/null || true)
login_ws=$(jq -r '.workspace_id // empty' "$profile_json" 2>/dev/null || true)

org_name=$(jq -r '.organization_name // empty' "$profile_json" 2>/dev/null || true)
say "Which organisation is this login for?"
ant --profile admin auth status 2>&1 | sed -n '/^Organization/,$p' | head -12 || true
echo "  organisation id from the login: ${org_id:-unknown}${org_name:+ ($org_name)}"
echo "  If this is not the organisation you meant, stop (Ctrl-C), switch organisation in the Console (the switcher at the top left,"
echo "  https://platform.claude.com) BEFORE logging in again, and choose it on the browser's authorize page."
echo
echo "  Read-only probes of the Admin API with this login:"
for ep in workspaces service_accounts federation_issuers; do
  call "$api/v1/organizations/$ep?limit=1" >/dev/null 2>&1 && r=ok || r="refused"
  printf '    %-20s HTTP %s (%s)\n' "$ep" "$last_code" "$r"
done
if [ "$mode" = "--diagnose" ]; then
  echo
  echo "  200 on all three: the login is an admin of a Console organisation and setup can go on."
  echo "  403 on all three: this login cannot use the Admin API. Either it is the wrong organisation, or you are not an admin/owner"
  echo "  of it, or the organisation is an individual Console account (not supported by the Admin API). Check Settings, Members for"
  echo "  your role, and Settings, Organization for the type. Send me this output."
  exit 0
fi
read -r -p "Continue with this organisation? [y/N] " a
[ "$a" = y ] || die "stopped: log in to the right organisation and rerun"

say "2/5 The dedicated workspace"
ws=${WORKSPACE_ID:-}
if [ -z "$ws" ]; then
  ws=$(call "$api/v1/organizations/workspaces?limit=100" | jq -r --arg n "$ws_name" '.data[]? | select(.name==$n and (.archived_at==null)) | .id' 2>/dev/null | head -n 1 || true)
  [ -n "$ws" ] && echo "found $ws by name '$ws_name'"
fi
if [ -z "$ws" ] && [ "$last_code" != 403 ]; then
  ws=$(call -X POST "$api/v1/organizations/workspaces" -d "$(jq -n --arg n "$ws_name" '{name:$n}')" | jq -r '.id // empty' 2>/dev/null || true)
  [ -n "$ws" ] && echo "created $ws"
fi
if [ -z "$ws" ]; then
  [ "$last_code" = 403 ] && echo "The API answered 403 (not a transient fault, so no retries): this login cannot use the Admin API. See docs/fork/WIF.md, 'If the API says 403'."
  if [ -n "$login_ws" ]; then
    read -r -p "Use the workspace your login is bound to ($login_ws)? [Y/n] " a
    case ${a:-y} in [Yy]*) ws=$login_ws ;; esac
  fi
  if [ -z "$ws" ]; then
    echo "Create or find it in the Console: https://platform.claude.com/settings/workspaces"
    read -r -p "Paste its id (wrkspc_...): " ws
  fi
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
python3 -I "$here/setup-anthropic-wif.py" --workspace "$ws" --repo "$repo" --env "$env_name" ${org_id:+--org-id "$org_id"}

say "5/5 Admin login removed (on exit). Next"
if on_fork fork-wif-check.yml; then
  echo "The workflows are on the fork branch: run  scripts/fork/setup-anthropic.sh --verify"
else
  echo "Merge the baseline pull request to the fork branch, then run  scripts/fork/setup-anthropic.sh --verify"
fi
