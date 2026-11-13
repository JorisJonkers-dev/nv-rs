#!/usr/bin/env bash
# Fork-only. Audits the GitHub repository settings that cannot live in a file and applies the ones that are
# missing. Needs admin rights to apply; the audit alone needs only read access.
#
#   scripts/fork/setup-repo.sh --audit [owner/repo]     report what is present and what is missing; changes nothing
#   scripts/fork/setup-repo.sh [owner/repo]             apply what is missing (DRY_RUN=1 prints the calls instead)
#
# Covers: merge methods and auto-merge, secret scanning and push protection, Dependabot alerts, private
# vulnerability reporting, CodeQL, the workflow token, which actions may run (GitHub's, the organisation's and a
# short list, pinned by SHA), approval for outside contributors' workflow runs, the fork-sync environment, labels,
# the branch and tag rulesets, and disabling upstream's ci.yml. It also lists who can write to the repository.
set -euo pipefail

mode=apply
if [ "${1:-}" = "--audit" ]; then mode=audit; shift; fi
repo=${1:-JorisJonkers-dev/nv-rs}
here=$(dirname "$(readlink -f "$0")")
dry=${DRY_RUN:-0}
missing=0

say() { printf '  %-58s %s\n' "$1" "$2"; }
call() { # call METHOD PATH [json]: a failure is reported, not fatal, so one unavailable feature does not stop the rest
  local method=$1 path=$2 body=${3:-}
  if [ "$dry" = 1 ]; then printf '     + gh api -X %s %s %s\n' "$method" "$path" "$body"; return 0; fi
  if [ -n "$body" ]; then
    printf '%s' "$body" | gh api -X "$method" "$path" --input - >/dev/null || echo "     !! $method $path failed (feature unavailable, or no admin rights)" >&2
  else
    gh api -X "$method" "$path" >/dev/null || echo "     !! $method $path failed" >&2
  fi
}
# ensure LABEL TEST_COMMAND APPLY_FUNCTION: runs the test; when it fails, counts it and (unless auditing) applies.
ensure() {
  local label=$1 test=$2 apply=$3
  if eval "$test" >/dev/null 2>&1; then say "$label" "ok"; return; fi
  missing=$((missing + 1))
  if [ "$mode" = audit ]; then say "$label" "MISSING"; else say "$label" "applying"; "$apply"; fi
}
get() { gh api "$@" 2>/dev/null; }

repo_json=$(get "repos/$repo") || { echo "cannot read $repo (gh auth?)" >&2; exit 1; }
rj() { jq -e "$1" <<<"$repo_json"; }

echo "Repository settings of $repo ($mode)"
a_merge() { call PATCH "repos/$repo" '{"allow_squash_merge":true,"allow_rebase_merge":true,"allow_merge_commit":false,"allow_auto_merge":true,"delete_branch_on_merge":true,"squash_merge_commit_title":"PR_TITLE","squash_merge_commit_message":"PR_BODY","has_wiki":false,"has_projects":false}'; }
ensure "squash and rebase only, auto-merge, branch deleted after merge" \
  "rj '.allow_squash_merge and .allow_rebase_merge and (.allow_merge_commit|not) and .allow_auto_merge and .delete_branch_on_merge'" a_merge
a_scan() { call PATCH "repos/$repo" '{"security_and_analysis":{"secret_scanning":{"status":"enabled"},"secret_scanning_push_protection":{"status":"enabled"}}}'; }
ensure "secret scanning and push protection" \
  "rj '.security_and_analysis.secret_scanning.status==\"enabled\" and .security_and_analysis.secret_scanning_push_protection.status==\"enabled\"'" a_scan
a_alerts() { call PUT "repos/$repo/vulnerability-alerts"; }
ensure "Dependabot alerts (Renovate makes the update pull requests)" "gh api repos/$repo/vulnerability-alerts" a_alerts
a_pvr() { call PUT "repos/$repo/private-vulnerability-reporting"; }
ensure "private vulnerability reporting" "get repos/$repo/private-vulnerability-reporting | jq -e .enabled" a_pvr
a_codeql() { call PATCH "repos/$repo/code-scanning/default-setup" '{"state":"configured","query_suite":"default"}'; }
ensure "CodeQL default setup" "get repos/$repo/code-scanning/default-setup | jq -e '.state==\"configured\"'" a_codeql

echo "Actions"
a_token() { call PUT "repos/$repo/actions/permissions/workflow" '{"default_workflow_permissions":"read","can_approve_pull_request_reviews":false}'; }
ensure "workflow token read-only by default" "get repos/$repo/actions/permissions/workflow | jq -e '.default_workflow_permissions==\"read\"'" a_token
a_allowed() {
  call PUT "repos/$repo/actions/permissions" '{"enabled":true,"allowed_actions":"selected","sha_pinning_required":true}'
  call PUT "repos/$repo/actions/permissions/selected-actions" '{"github_owned_allowed":true,"verified_allowed":false,"patterns_allowed":["JorisJonkers-dev/*","jdx/mise-action@*","Swatinem/rust-cache@*","googleapis/release-please-action@*"]}'
}
ensure "only GitHub's, the organisation's and four named actions, all pinned by SHA" \
  "get repos/$repo/actions/permissions | jq -e '.allowed_actions==\"selected\" and .sha_pinning_required==true'" a_allowed
a_approval() { call PUT "repos/$repo/actions/permissions/fork-pr-contributor-approval" '{"approval_policy":"all_external_contributors"}'; }
ensure "outside contributors' workflow runs need approval" \
  "get repos/$repo/actions/permissions/fork-pr-contributor-approval | jq -e '.approval_policy==\"all_external_contributors\"'" a_approval
a_env() {
  call PUT "repos/$repo/environments/fork-sync" '{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}'
  call POST "repos/$repo/environments/fork-sync/deployment-branch-policies" '{"name":"main","type":"branch"}'
}
ensure "environment fork-sync (secrets readable only by runs on main)" \
  "get repos/$repo/environments/fork-sync | jq -e '.deployment_branch_policy.custom_branch_policies==true'" a_env
a_ci() { if [ "$dry" = 1 ]; then echo "     + gh workflow disable ci.yml -R $repo"; else gh workflow disable ci.yml -R "$repo" >/dev/null 2>&1 || echo "     (ci.yml not found)"; fi; }
ensure "upstream's ci.yml disabled (fork-build-*.yml replaces it)" \
  "get repos/$repo/actions/workflows/ci.yml | jq -e '.state!=\"active\"'" a_ci

echo "Labels and rulesets"
a_labels() {
  local l name color desc
  for l in "allow-repository-hygiene-exception|BFD4F2|Bypasses the repository hygiene guard (sync pull requests)" \
           "ci-parity-review|FBCA04|Upstream changed ci.yml: compare it with fork-build-*.yml" \
           "dependencies|0366D6|Dependency updates" "type: chore|EDEDED|Chore" "component: dependencies|EDEDED|Dependencies"; do
    IFS='|' read -r name color desc <<<"$l"
    if [ "$dry" = 1 ]; then echo "     + gh label create '$name'"; else gh label create "$name" -R "$repo" --color "$color" --description "$desc" --force >/dev/null; fi
  done
}
ensure "labels (hygiene exception, ci-parity-review, Renovate's)" \
  "get repos/$repo/labels --paginate --jq '.[].name' | grep -qx allow-repository-hygiene-exception && get repos/$repo/labels --paginate --jq '.[].name' | grep -qx ci-parity-review" a_labels
a_rulesets() { if [ "$dry" = 1 ]; then echo "     + $here/apply-ruleset.sh $repo"; else bash "$here/apply-ruleset.sh" "$repo"; fi; }
ensure "ruleset Main: Pipeline Complete, pull requests only, linear history, no force push" \
  "get repos/$repo/rulesets | jq -e 'map(select(.name==\"Main\" and .enforcement==\"active\"))|length==1'" a_rulesets
ensure "ruleset Release tags: v* cannot be moved or deleted" \
  "get repos/$repo/rulesets | jq -e 'map(select(.name==\"Release tags\" and .enforcement==\"active\"))|length==1'" a_rulesets

echo "Who can change workflows (anyone with write access can read the repository's secrets)"
get "repos/$repo/collaborators?affiliation=all" --jq '.[] | select(.permissions.push or .permissions.admin) | "  \(.login): \(if .permissions.admin then "admin" else "write" end)"' || echo "  (cannot list: needs push access)"
n=$(get "repos/$repo/collaborators?affiliation=all" --jq '[.[] | select(.permissions.push or .permissions.admin)] | length' || echo 0)
[ "${n:-0}" -le 1 ] && echo "  ok: only you" || echo "  !! more than one person can write; each of them can read the fork-sync secrets on main"

echo
if [ "$mode" = audit ]; then
  echo "$missing item(s) missing. Run without --audit to apply them."
else
  echo "Applied what was missing ($missing). Run with --audit to confirm."
fi
cat <<NOTE

Still by hand:
  * The API key as an ENVIRONMENT secret, typed at the prompt so it stays out of your shell history:
      gh secret set ANTHROPIC_API_KEY --env fork-sync -R $repo
    from a dedicated Anthropic workspace with a low monthly spend limit. Without it a fixed rule writes the titles.
  * The fork's own GitHub App, with only the permissions it needs: scripts/fork/create-app.py (docs/fork/APP.md).
  * Renovate (the organisation's app) onboards itself once renovate.json is on the default branch.
NOTE
