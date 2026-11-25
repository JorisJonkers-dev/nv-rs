# SPDX-License-Identifier: GPL-3.0-only
"""Pipeline Complete of a description edit (pull_request `edited`, fork-ci.yml).

An edit run checks only the title and the description; its builds and code checks skip. So that an edit cannot turn
a red pull request green, its Pipeline Complete takes the code's result from the newest run of the same workflow on
the same head commit whose "Repository checks" job ran (a run of the commits, not another edit run):
- that run's Pipeline Complete passed: exit 0;
- it failed, was cancelled or has not finished: exit 1 (when it finishes later, its own Pipeline Complete is the newer
  check on the commit and counts);
- no such run: exit 1.

Environment: GH_TOKEN, REPO (owner/name), HEAD_SHA, RUN_ID (this run, left out), PR_NUMBER, HEAD_REF and
HEAD_REPO (runs of other pull requests on the same commit are left out).
"""
import json
import os
import subprocess
import sys
import time

WORKFLOW = "fork-ci.yml"
CODE_JOB = "Repository checks"
GATE_JOB = "Pipeline Complete"


def api(path, tries=3, run=subprocess.run, wait=time.sleep):
    """GET a REST path through gh. A failed call (GitHub answers 502 now and then) is tried again; its error goes to
    stderr, so a red gate shows why."""
    for attempt in range(1, tries + 1):
        got = run(["gh", "api", path], capture_output=True, text=True)
        if got.returncode == 0:
            return json.loads(got.stdout)
        print(f"gh api {path} (try {attempt} of {tries}): {got.stderr.strip()}", file=sys.stderr)
        if attempt < tries:
            wait(5 * attempt)
    raise RuntimeError(f"gh api {path} failed {tries} times")


def all_jobs(get_page, run_id, per_page=50):
    """Every job of the run's latest attempt. A run of the commits has more than 100 jobs (the build alone is about 95)
    and "Pipeline Complete" is created last, so one page would miss it. Pages of 50: GitHub answers a page of 100 jobs
    of such a run with HTTP 502 (seen on run 45686261367, every try)."""
    jobs, page = [], 1
    while True:
        got = get_page(run_id, page, per_page)
        jobs += got["jobs"]
        if len(got["jobs"]) < per_page or len(jobs) >= got.get("total_count", 0):
            return jobs
        page += 1


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


def of_this_pull_request(run, number, head_ref, head_repo):
    """A run on the same head commit can belong to another pull request (the same branch opened against another
    base): its result says nothing about this one. A run from a branch of this repository names its pull requests;
    a run from a fork names none, so the head branch and repository must match."""
    prs = run.get("pull_requests") or []
    if prs:
        return any(p.get("number") == number for p in prs)
    return run.get("head_branch") == head_ref and (run.get("head_repository") or {}).get("full_name") == head_repo


def main():
    repo, sha, me = os.environ["REPO"], os.environ["HEAD_SHA"], int(os.environ["RUN_ID"])
    number, head_ref, head_repo = int(os.environ["PR_NUMBER"]), os.environ["HEAD_REF"], os.environ["HEAD_REPO"]
    listed = api(f"repos/{repo}/actions/workflows/{WORKFLOW}/runs?head_sha={sha}&event=pull_request&per_page=50")
    mine = (r for r in listed["workflow_runs"] if r["id"] != me and of_this_pull_request(r, number, head_ref, head_repo))
    runs = sorted(mine, key=lambda r: r["run_started_at"], reverse=True)
    full = []
    for r in runs:
        jobs = all_jobs(lambda rid, page, n: api(f"repos/{repo}/actions/runs/{rid}/jobs?per_page={n}&page={page}"), r["id"])
        full.append({"id": r["id"], "jobs": jobs})
        if any(j["name"] == CODE_JOB and j.get("conclusion") != "skipped" for j in jobs):
            break  # the newest run of the commits is all verdict() needs
    passed, message = verdict(full)
    print(message if passed else f"::error title=Pipeline::{message}")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
