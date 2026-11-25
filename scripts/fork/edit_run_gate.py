# SPDX-License-Identifier: GPL-3.0-only
"""Pipeline Complete of a description edit (pull_request `edited`, fork-ci.yml).

An edit run checks only the title and the description; its builds and code checks skip. So that an edit cannot turn
a red pull request green, its Pipeline Complete takes the code's result from the newest run of the same workflow on
the same head commit whose "Repository checks" job ran (a run of the commits, not another edit run):
- that run's Pipeline Complete passed: exit 0;
- it failed, was cancelled or has not finished: exit 1 (when it finishes later, its own Pipeline Complete is the newer
  check on the commit and counts);
- no such run: exit 1.

Environment: GH_TOKEN, REPO (owner/name), HEAD_SHA, RUN_ID (this run, left out).
"""
import json
import os
import subprocess
import sys

WORKFLOW = "fork-ci.yml"
CODE_JOB = "Repository checks"
GATE_JOB = "Pipeline Complete"


def api(path):
    out = subprocess.run(["gh", "api", path], capture_output=True, text=True, check=True).stdout
    return json.loads(out)


def verdict(runs):
    """runs: newest first, each {"id", "jobs": [{"name", "status", "conclusion"}]}. Returns (passed, message)."""
    for run in runs:
        jobs = {j["name"]: j for j in run["jobs"]}
        code = jobs.get(CODE_JOB)
        if code is None or code.get("conclusion") == "skipped":
            continue  # another edit run
        gate = jobs.get(GATE_JOB)
        if gate is None or gate.get("status") != "completed":
            return False, f"run {run['id']} of the commits has not finished; its own {GATE_JOB} decides"
        if gate.get("conclusion") == "success":
            return True, f"run {run['id']} of the commits passed"
        return False, f"run {run['id']} of the commits: {GATE_JOB} {gate.get('conclusion')}"
    return False, "no run of the commits on this head commit"


def main():
    repo, sha, me = os.environ["REPO"], os.environ["HEAD_SHA"], int(os.environ["RUN_ID"])
    listed = api(f"repos/{repo}/actions/workflows/{WORKFLOW}/runs?head_sha={sha}&event=pull_request&per_page=50")
    runs = sorted((r for r in listed["workflow_runs"] if r["id"] != me), key=lambda r: r["run_started_at"], reverse=True)
    full = []
    for r in runs:
        jobs = api(f"repos/{repo}/actions/runs/{r['id']}/jobs?per_page=100")["jobs"]
        full.append({"id": r["id"], "jobs": jobs})
        if any(j["name"] == CODE_JOB and j.get("conclusion") != "skipped" for j in jobs):
            break  # the newest run of the commits is all verdict() needs
    passed, message = verdict(full)
    print(message if passed else f"::error title=Pipeline::{message}")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
