#!/usr/bin/env bash
# Store the identifiers of a federation rule that you created in the Claude Console (Workload identity, Connect workload)
# as environment variables of the repository, so the workflows can use them. They are identifiers, not secrets: knowing them
# gets nobody a token (the rule decides who may exchange a token).
#
# Usage: scripts/fork/store-wif-ids.sh                       asks for each value
#        scripts/fork/store-wif-ids.sh --rule fdrl_... --service-account svac_... --org <uuid> --workspace wrkspc_...
# Needs gh (admin on the repository). Idempotent. docs/fork/WIF.md has the Console steps and the values to enter there.
set -euo pipefail

repo=${REPO:-JorisJonkers-dev/nv-rs}
env_name=${ENV_NAME:-fork-sync}
rule=""; sa=""; org=""; ws=""
while [ $# -gt 0 ]; do
  case $1 in
    --rule) rule=$2; shift 2 ;;
    --service-account) sa=$2; shift 2 ;;
    --org) org=$2; shift 2 ;;
    --workspace) ws=$2; shift 2 ;;
    --repo) repo=$2; shift 2 ;;
    --env) env_name=$2; shift 2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
ask() { # ask VAR "prompt" "pattern"
  local -n ref=$1
  while [[ -z $ref || ! $ref =~ $3 ]]; do
    [ -z "$ref" ] || echo "   '$ref' does not look right ($3)" >&2
    read -r -p "$2: " ref
  done
}
ask rule "Federation rule id (starts with fdrl_; the rule's page in the Console)" '^fdrl_[A-Za-z0-9]+$'
ask sa "Service account id (svac_...; Console, Service accounts)" '^svac_[A-Za-z0-9]+$'
ask ws "Workspace id (wrkspc_...; Console, Settings, Workspaces)" '^wrkspc_[A-Za-z0-9]+$'
ask org "Organization id (a UUID; Console, Settings, Organization)" '^[0-9a-fA-F-]{36}$'

gh auth status >/dev/null 2>&1 || { echo "gh is not logged in" >&2; exit 1; }
gh api "repos/$repo/environments/$env_name" --silent 2>/dev/null || {
  gh api -X PUT "repos/$repo/environments/$env_name" --silent
  echo "created the '$env_name' environment (scripts/fork/setup-repo.sh restricts it to the fork branch)"
}
for kv in "ANTHROPIC_FEDERATION_RULE_ID=$rule" "ANTHROPIC_SERVICE_ACCOUNT_ID=$sa" "ANTHROPIC_WORKSPACE_ID=$ws" "ANTHROPIC_ORGANIZATION_ID=$org"; do
  gh variable set "${kv%%=*}" --env "$env_name" -R "$repo" --body "${kv#*=}" >/dev/null
  echo "set ${kv%%=*}"
done
echo "Done. Next: merge the baseline pull request, then run the 'Claude Federation Check' workflow (or scripts/fork/setup-anthropic.sh --verify)."
