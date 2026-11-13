#!/usr/bin/env python3
"""The final summary of the pipeline: every component's result and duration, every job, what failed.

Run it as the last step of `Pipeline Complete` (permissions: actions: read, and GH_TOKEN set). It lists the run's
jobs through the REST API (`gh api`, installed on the runners) and prints Markdown (append it to
$GITHUB_STEP_SUMMARY): a table per component (worst result of its jobs, the longest job's duration, how many
jobs), the job that finished last (the critical path of this run) and the failed steps. Read-only; it never
changes the result. Durations are measured: a job's `started_at` to `completed_at`. Stdlib only.

  python3 -I scripts/fork/pipeline_summary.py --run "$GITHUB_RUN_ID" --repo "$GITHUB_REPOSITORY" --exclude "Pipeline Complete"
  python3 -I scripts/fork/pipeline_summary.py --input jobs.json            # a saved API answer (the tests)
"""
import argparse
import json
import re
import subprocess
import sys
from datetime import datetime, timezone

# "Build / Component: integration / typed animation (Windows)" -> component "integration"; the "Build / " prefix
# is the calling job's name in the pipeline.
COMPONENT = re.compile(r"^(?:.* / )?Component: ([^ (/]+)(?: / (.*?))? \((.*)\)$")
RANK = {"success": 0, "skipped": 0, "cancelled": 1, "timed_out": 2, "failure": 3}
LABEL = {"success": "ok", "failure": "FAILED", "cancelled": "cancelled", "skipped": "skipped", "timed_out": "TIMED OUT",
         "informational failure": "failed (informational)"}


def ts(text):
    if not text or text.startswith("0001"):
        return None
    return datetime.strptime(text, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)


def dur(seconds):
    if seconds is None:
        return "-"
    seconds = int(seconds)
    return f"{seconds // 60}m{seconds % 60:02d}s" if seconds >= 60 else f"{seconds}s"


def rows_of(jobs, exclude=()):
    rows = []
    for j in jobs:
        if j["name"] in exclude:
            continue
        created, started, done = ts(j.get("created_at")), ts(j.get("started_at")), ts(j.get("completed_at"))
        failed_steps = [s["name"] for s in j.get("steps", []) if s.get("conclusion") == "failure"]
        result = j.get("conclusion") or j.get("status") or "unknown"
        # A job with continue-on-error that failed reports success to the workflow: show what really happened.
        if result == "success" and failed_steps:
            result = "informational failure"
        rows.append({"name": j["name"], "result": result, "created": created, "done": done,
                     "queue": (started - created).total_seconds() if started and created else None,
                     "run": (done - started).total_seconds() if done and started else None,
                     "failed_steps": failed_steps})
    return rows


def render(jobs, exclude=()):
    rows = rows_of(jobs, exclude)
    out = []
    comps = {}
    for r in rows:
        m = COMPONENT.match(r["name"])
        if m:
            comps.setdefault(m.group(1), []).append(r)
    if comps:
        out += ["### Components\n", "| Component | Result | Longest job | Jobs |", "| --- | --- | ---: | ---: |"]
        for name, rs in sorted(comps.items(), key=lambda kv: -max(x["run"] or 0 for x in kv[1])):
            required = [x for x in rs if "informational" not in x["name"]] or rs
            worst = max(required, key=lambda x: RANK.get(x["result"], 3))["result"]
            twin_failed = any(x["result"] == "informational failure" or (x["result"] in ("failure", "timed_out") and "informational" in x["name"])
                              for x in rs)
            note = " (its Linux twin failed: informational)" if twin_failed else ""
            longest = max(rs, key=lambda x: x["run"] or 0)
            out.append(f"| {name} | {LABEL.get(worst, worst)}{note} | {dur(longest['run'])} ({longest['name'].split('Component: ')[-1]}) | {len(rs)} |")
        out.append("")
    rows.sort(key=lambda r: (r["done"] is None, -(r["run"] or 0)))
    out += ["### Jobs\n", "| Job | Result | Queued | Ran |", "| --- | --- | ---: | ---: |"]
    for r in rows:
        out.append(f"| {r['name']} | {LABEL.get(r['result'], r['result'])} | {dur(r['queue'])} | {dur(r['run'])} |")
    finished = [r for r in rows if r["done"] and r["run"] is not None]
    if finished:
        last = max(finished, key=lambda r: r["done"])
        first = min(r["created"] for r in rows if r["created"])
        out.append(f"\nLast job to finish: **{last['name']}**, {dur((last['done'] - first).total_seconds())} after the run's first job "
                   f"was created (the critical path of this run; measured).")
        out.append(f"Runner time used by these jobs: {dur(sum(r['run'] for r in finished))}.")
    bad = [r for r in rows if r["result"] in ("failure", "timed_out", "informational failure")]
    if bad:
        out.append("\n**Failed**\n")
        for r in bad:
            out.append(f"- {r['name']}: {', '.join(r['failed_steps']) or r['result']}")
    return "\n".join(out) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run")
    ap.add_argument("--repo")
    ap.add_argument("--input")
    ap.add_argument("--exclude", action="append", default=[])
    a = ap.parse_args()
    if a.input:
        with open(a.input, encoding="utf-8") as fh:
            raw = json.load(fh)
        jobs = raw["jobs"] if isinstance(raw, dict) else raw
    else:
        out = subprocess.run(["gh", "api", "--paginate", "--slurp", f"repos/{a.repo}/actions/runs/{a.run}/jobs?per_page=100"],
                             capture_output=True, text=True, check=False)
        if out.returncode:
            print(f"::warning title=Summary::cannot list the jobs: {out.stderr.strip()[:200]}", file=sys.stderr)
            return 0
        jobs = [j for page in json.loads(out.stdout) for j in page["jobs"]]
    sys.stdout.write(render(jobs, a.exclude))
    return 0


if __name__ == "__main__":
    sys.exit(main())
