#!/usr/bin/env bash
# Apply every ruleset in .github/rulesets/*.json to a repository: create it if the name is absent,
# update it in place otherwise. Idempotent. Adapted from the organisation template's apply-ruleset.sh,
# which handles one ruleset; this one loops so the release-tag ruleset is applied too.
# The progress App (scripts/fork/setup-progress-bot.py, docs/fork/PROGRESS.md) bypasses "Fork line" to push its
# pictures: on JorisJonkers-dev/nv-rs, when its Actions variable PROGRESS_APP_ID is set, that one bypass actor is
# added to what the file says (the file's own list is empty), so applying the file never drops it.
# Usage: scripts/fork/apply-ruleset.sh <owner/repo>   (default: the current repository)
set -euo pipefail

repo="${1:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"
dir="$(dirname "$0")/../../.github/rulesets"

progress_app="$(gh variable get PROGRESS_APP_ID -R "$repo" 2>/dev/null || true)"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

for file in "$dir"/*.json; do
  name="$(jq -r .name "$file")"
  if [[ $repo == JorisJonkers-dev/nv-rs && $name == "Fork line" && $progress_app =~ ^[0-9]+$ ]]; then
    jq --argjson id "$progress_app" '.bypass_actors += [{"actor_id": $id, "actor_type": "Integration", "bypass_mode": "always"}]' "$file" >"$tmp"
    file="$tmp"
  fi
  id="$(gh api "/repos/$repo/rulesets" --jq ".[] | select(.name==\"$name\") | .id" 2>/dev/null || true)"
  if [[ -n $id ]]; then
    echo "Updating ruleset '$name' ($id) on $repo"
    gh api -X PUT "/repos/$repo/rulesets/$id" --input "$file" >/dev/null
  else
    echo "Creating ruleset '$name' on $repo"
    gh api -X POST "/repos/$repo/rulesets" --input "$file" >/dev/null
  fi
done
