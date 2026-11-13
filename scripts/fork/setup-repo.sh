#!/usr/bin/env bash
# Fork-only. One-time, idempotent setup of the GitHub repository settings that cannot live in a file:
# merge methods and auto-merge, the branch ruleset, secret scanning with push protection, Dependabot
# alerts, private vulnerability reporting, CodeQL, workflow token permissions, labels, and disabling
# upstream's own ci.yml (fork-build.yml replaces it). Needs admin rights on the repository.
#
# Usage: scripts/fork/setup-repo.sh [owner/repo]        DRY_RUN=1 prints the calls without making them
# Run it after the pull request that adds this script is merged (the ruleset requires Pipeline Complete,
# which must have run once), or before, if you merge that pull request yourself.
set -euo pipefail

repo=${1:-JorisJonkers-dev/nv-rs}
here=$(dirname "$(readlink -f "$0")")
dry=${DRY_RUN:-0}

step() { printf '\n== %s\n' "$*"; }
api() { # api METHOD PATH [json body]  (a failure is reported, not fatal, so one unsupported feature does not stop the rest)
  local method=$1 path=$2 body=${3:-}
  if [ "$dry" = 1 ]; then printf '+ gh api -X %s %s %s\n' "$method" "$path" "$body"; return 0; fi
  if [ -n "$body" ]; then
    printf '%s' "$body" | gh api -X "$method" "$path" --input - >/dev/null || echo "   !! $method $path failed (feature unavailable for this plan, or no admin rights)" >&2
  else
    gh api -X "$method" "$path" >/dev/null || echo "   !! $method $path failed" >&2
  fi
}

step "Merge methods: squash for people, rebase for the upstream replay, auto-merge on, branches deleted after merge"
api PATCH "repos/$repo" '{"allow_squash_merge":true,"allow_rebase_merge":true,"allow_merge_commit":false,"allow_auto_merge":true,"delete_branch_on_merge":true,"squash_merge_commit_title":"PR_TITLE","squash_merge_commit_message":"PR_BODY","has_wiki":false,"has_projects":false}'

step "Secret scanning and push protection"
api PATCH "repos/$repo" '{"security_and_analysis":{"secret_scanning":{"status":"enabled"},"secret_scanning_push_protection":{"status":"enabled"}}}'

step "Dependabot alerts (alerts only: Renovate makes the update pull requests)"
api PUT "repos/$repo/vulnerability-alerts"

step "Private vulnerability reporting (SECURITY.md points to it)"
api PUT "repos/$repo/private-vulnerability-reporting"

step "CodeQL default setup (languages detected by GitHub)"
api PATCH "repos/$repo/code-scanning/default-setup" '{"state":"configured","query_suite":"default"}'

step "Workflow token: read-only by default (each workflow asks for what it needs)"
api PUT "repos/$repo/actions/permissions/workflow" '{"default_workflow_permissions":"read","can_approve_pull_request_reviews":false}'

step "Labels"
for l in "allow-repository-hygiene-exception|BFD4F2|Bypasses the repository hygiene guard (sync pull requests)" \
         "ci-parity-review|FBCA04|Upstream changed ci.yml: compare it with fork-build.yml" \
         "dependencies|0366D6|Dependency updates" "type: chore|EDEDED|Chore" "component: dependencies|EDEDED|Dependencies"; do
  IFS='|' read -r name color desc <<<"$l"
  if [ "$dry" = 1 ]; then echo "+ gh label create '$name' -R $repo"; else gh label create "$name" -R "$repo" --color "$color" --description "$desc" --force >/dev/null; fi
done

step "Branch ruleset 'Main' (Pipeline Complete required, pull requests only, squash or rebase, linear history, no force push)"
if [ "$dry" = 1 ]; then echo "+ $here/apply-ruleset.sh $repo"; else bash "$here/apply-ruleset.sh" "$repo"; fi

step "Disable upstream's ci.yml (fork-build.yml runs the same commands as parallel jobs)"
if [ "$dry" = 1 ]; then echo "+ gh workflow disable ci.yml -R $repo"; else gh workflow disable ci.yml -R "$repo" 2>&1 || echo "   (already disabled or not found)"; fi

if [ "$dry" != 1 ]; then
  step "Result"
  gh api "repos/$repo" --jq '"merge: squash=\(.allow_squash_merge) rebase=\(.allow_rebase_merge) merge-commit=\(.allow_merge_commit) auto=\(.allow_auto_merge) delete-branch=\(.delete_branch_on_merge)", "secret scanning: \(.security_and_analysis.secret_scanning.status), push protection: \(.security_and_analysis.secret_scanning_push_protection.status)"'
  gh api "repos/$repo/rulesets" --jq '.[] | "ruleset: \(.name) (\(.enforcement))"'
fi
cat <<NOTE

Still by hand:
  * repository secret ANTHROPIC_API_KEY (the sync workflow's title rewriting; without it a fixed rule is used);
    RELEASE_APP_ID and RELEASE_APP_PRIVATE_KEY are already organisation secrets.
  * Renovate (the org's app, installed on all repositories) onboards itself once renovate.json is on the
    default branch, so merge the baseline pull request first.
NOTE
