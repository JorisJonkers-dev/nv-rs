#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The test-impact map: which source files each CI job's tests execute (docs/fork/CI.md, "Test impact").

  test_impact.py collect --job JOB.json --out DIR [--root DIR]
      Runs one job of `components.py impact-plan` under cargo-llvm-cov, one test binary at a time, and writes
      DIR/<job>.json: the source files each binary executed (a line with a hit count above zero). Used by
      .github/workflows/fork-test-impact.yml on the nightly schedule.
  test_impact.py merge --dir DIR --commit SHA --out test-impact.json
      Merges the per-job files into the map the plan step reads: {"version", "commit", "generated", "cargo_lock_sha256",
      "components_toml_sha256", "known": [every tracked .rs file at the commit], "jobs": {key: {"files": [...],
      "binaries": {binary: [...]}}}, "skipped": {key: why}}. A job with a binary that failed or produced no coverage is
      left out ("skipped"): the plan selects it by crate, as without a map.
  test_impact.py replay --map FILE [--prs N] [--prs-json FILE] [--slots 16] [--timings FILE]
      Replays the last N merged pull requests (their changed files) through the selection by crate and the selection by
      the map and prints the jobs, the peak of concurrent jobs and the modelled wall time of each.

Stdlib only; Python 3.11 or newer.
"""
import argparse
import hashlib
import heapq
import json
import os
import re
import shlex
import subprocess
import sys
from datetime import datetime, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
VERSION = 1


def slug(key):
    return re.sub(r"[^A-Za-z0-9]+", "-", key).strip("-").lower()


def relative(name, root):
    """A path of an lcov file relative to the repository, or None when it is not in it."""
    name = name.replace("\\", "/")
    base = root.replace("\\", "/").rstrip("/") + "/"
    if name.lower().startswith(base.lower()):
        return name[len(base):]
    for marker in ("/crates/", "/vendor/"):
        at = name.find(marker)
        if at >= 0:
            return name[at + 1:]
    return None


def executed_files(lcov, root):
    """The repository files of an lcov text with at least one line hit."""
    out, current, hit = set(), None, False
    for line in lcov.splitlines():
        line = line.strip()
        if line.startswith("SF:"):
            current, hit = relative(line[3:], root), False
        elif line.startswith("DA:") and current and not hit:
            try:
                if int(line[3:].split(",")[1]) > 0:
                    hit = True
                    if current.endswith(".rs"):
                        out.add(current)
            except (IndexError, ValueError):
                pass
    return out


def binaries(cmd):
    """[(label, cargo arguments)]: the test binaries a `cargo test` command runs, one run each, so that the coverage of one
    binary is not mixed with another's. Doctests are not measured (they need the nightly compiler); a command that is only
    doctests gives nothing."""
    toks = shlex.split(cmd)
    if toks[:2] != ["cargo", "test"] or "--doc" in toks:
        return []
    rest, tests, lib, i = [], [], False, 2
    while i < len(toks):
        t = toks[i]
        if t == "--test" and i + 1 < len(toks):
            tests.append(toks[i + 1])
            i += 2
            continue
        if t == "--config" and i + 1 < len(toks):
            i += 2
            continue
        if t == "--lib":
            lib = True
        else:
            rest.append(t)
        i += 1
    if not tests and not lib:
        return [(" ".join(rest), rest)]
    base = " ".join(x for x in rest if x not in ("--locked",))
    out = [(f"{base} --lib", rest + ["--lib"])] if lib else []
    out += [(f"{base} --test {t}", rest + ["--test", t]) for t in tests]
    return out


def run(cmd, **kw):
    print("+ " + " ".join(cmd), flush=True)
    return subprocess.run(cmd, check=False, **kw)


def linux_skips(path=".fork/linux-skip.txt"):
    """`-- --skip NAME ...` for the tests that only pass on Windows (run-component.sh skips the same on Linux)."""
    try:
        with open(path, encoding="utf-8") as fh:
            names = [ln.split("#")[0].strip() for ln in fh]
    except OSError:
        return []
    names = [n for n in names if n]
    return ["--"] + [x for n in names for x in ("--skip", n)] if names else []


def collect(args):
    with open(args.job, encoding="utf-8") as fh:
        job = json.load(fh)
    root = os.path.abspath(args.root)
    os.makedirs(args.out, exist_ok=True)
    result = {"key": job["key"], "os": job["os"], "binaries": {}}
    lcov = os.path.join(args.out, "tmp.lcov")
    for cmd in job["runs"]:
        for label, cargo_args in binaries(cmd):
            # Profile data of an earlier binary is gone, so the report is this binary's alone.
            run(["cargo", "llvm-cov", "clean", "--profraw-only"])
            skips = linux_skips() if sys.platform.startswith("linux") else []
            status = run(["cargo", "llvm-cov", "--no-report", "--no-fail-fast"] + cargo_args + skips).returncode
            rep = run(["cargo", "llvm-cov", "report", "--lcov", "--output-path", lcov])
            files = None
            if rep.returncode == 0 and os.path.isfile(lcov):
                with open(lcov, encoding="utf-8", errors="replace") as fh:
                    files = sorted(executed_files(fh.read(), root))
                os.remove(lcov)
            ok = files is not None and status == 0
            result["binaries"][label] = {"status": "ok" if ok else "tests failed" if files else "no coverage", "files": files or []}
            print(f"{label}: {len(files or [])} files, {result['binaries'][label]['status']}", flush=True)
    with open(os.path.join(args.out, slug(job["key"]) + ".json"), "w", encoding="utf-8") as fh:
        json.dump(result, fh, indent=1)
    # A binary that failed is in the file as such; the merge leaves the job out of the map. Not an error of this step.
    return 0


def sha256(path):
    try:
        with open(path, "rb") as fh:
            return hashlib.sha256(fh.read()).hexdigest()
    except OSError:
        return None


def merge(args):
    jobs, skipped = {}, {}
    for name in sorted(os.listdir(args.dir)):
        if not name.endswith(".json"):
            continue
        with open(os.path.join(args.dir, name), encoding="utf-8") as fh:
            got = json.load(fh)
        bad = [f"{b}: {v['status']}" for b, v in got["binaries"].items() if v["status"] != "ok" or not v["files"]]
        if bad or not got["binaries"]:
            skipped[got["key"]] = "; ".join(bad) or "no test binary"
            continue
        jobs[got["key"]] = {"os": got["os"], "files": sorted({f for v in got["binaries"].values() for f in v["files"]}),
                            "binaries": {b: v["files"] for b, v in got["binaries"].items()}}
    known = subprocess.run(["git", "ls-files", "*.rs"], capture_output=True, text=True, check=True).stdout.split("\n")
    out = {"version": VERSION, "commit": args.commit, "generated": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
           "cargo_lock_sha256": sha256("Cargo.lock"), "components_toml_sha256": sha256(".fork/components.toml"),
           "known": sorted(f for f in known if f), "jobs": jobs, "skipped": skipped}
    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump(out, fh, separators=(",", ":"))
    print(f"test-impact map of {args.commit[:10]}: {len(jobs)} jobs, {len(skipped)} left out, {len(out['known'])} known files")
    for k, why in skipped.items():
        print(f"::warning title=test impact::{k} is not in the map: {why}")
    return 0


# ---- replay -------------------------------------------------------------------------------------------------------

def durations(model, timings):
    """{job name of a matrix entry: modelled minutes}: the measured seconds of each test binary of the job (the upstream
    timings of .fork/test-timings.json) plus a compile estimate, in minutes; a job with no measurement takes the median."""
    minutes = {}
    for name, c in model.comps.items():
        for job in (c.get("job") or [None]):
            tests = model.job_tests(c, job)
            secs = sum(timings.get(f"{crate}::{t}", 0) for crate, t in tests)
            minutes[model.job_key(name, job)] = secs / 60
    return minutes


def wall(jobs, slots, overhead):
    """Modelled wall time (minutes): the jobs started longest first on `slots` runners, plus a fixed cost per job (checkout,
    toolchain, cache restore) and the plan job in front."""
    free = [0.0] * max(1, min(slots, len(jobs) or 1))
    heapq.heapify(free)
    end = 0.0
    for d in sorted(jobs, reverse=True):
        start = heapq.heappop(free)
        heapq.heappush(free, start + d + overhead)
        end = max(end, start + d + overhead)
    return end + 1.0 if jobs else 1.0


def replay(args):
    sys.path.insert(0, HERE)
    import components  # noqa: E402
    with open(args.map, encoding="utf-8") as fh:
        impact = json.load(fh)
    with open(".fork/test-timings.json", encoding="utf-8") as fh:
        timings = json.load(fh)["tests"]
    model = components.load()
    model.check()
    minutes = durations(model, timings)
    known = [m for m in minutes.values() if m > 0]
    median = sorted(known)[len(known) // 2] if known else 5.0
    if args.prs_json:
        with open(args.prs_json, encoding="utf-8") as fh:
            prs = json.load(fh)
    else:
        raw = subprocess.run(["gh", "pr", "list", "--repo", args.repo, "--state", "merged", "--base", "fork", "--limit", str(args.prs),
                              "--json", "number,title,files"], capture_output=True, text=True, check=True).stdout
        prs = json.loads(raw)
    rows = []
    for pr in prs:
        files = [f["path"] for f in pr["files"]]
        if not files:
            continue
        kind = re.match(r"^(\w+)", pr["title"]).group(1) if re.match(r"^(\w+)", pr["title"]) else "other"
        everything_code = [f for f in files if not components.DOCS.match(f)]
        row = {"number": pr["number"], "title": pr["title"], "type": kind, "files": len(files)}
        for label, imp in (("old", None), ("new", impact)):
            if not everything_code:
                row[label] = {"jobs": 0, "peak": 0, "minutes": 0.0, "runner_minutes": 0.0}
                continue
            all_os = model.platform_relevant(files)
            # The files are those of the pull request at its merge, read against today's tree: a file the tree no longer has
            # counts as deleted, so only files that still exist are selected by the map.
            plan = model.plan(False, files, all_os, impact=imp)
            durs = []
            for e in plan["include"]:
                comp = e["component"]
                if comp.startswith("group-") or comp in model.comps and " / " not in e["name"]:
                    d = minutes.get(comp, 0.0) if comp in minutes else 0.0
                else:
                    d = 0.0
                if " / " in e["name"]:
                    key = e["name"].removeprefix("Component: ").rsplit(" (", 1)[0]
                    key = re.sub(r" \d+/\d+$", "", key)
                    d = minutes.get(key, 0.0)
                    shards = int(e["shard"].split("/")[1]) if e["shard"] else 1
                    d = d / shards
                durs.append((d if d > 0 else median) + 0)
            row[label] = {"jobs": len(durs), "peak": min(len(durs), args.slots),
                          "minutes": round(wall(durs, args.slots, args.overhead), 1),
                          "runner_minutes": round(sum(d + args.overhead for d in durs), 1)}
        rows.append(row)
    json.dump(rows, sys.stdout, indent=1)
    print()
    return 0


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("collect")
    c.add_argument("--job", required=True)
    c.add_argument("--out", required=True)
    c.add_argument("--root", default=".")
    m = sub.add_parser("merge")
    m.add_argument("--dir", required=True)
    m.add_argument("--commit", required=True)
    m.add_argument("--out", required=True)
    r = sub.add_parser("replay")
    r.add_argument("--map", required=True)
    r.add_argument("--prs", type=int, default=25)
    r.add_argument("--prs-json")
    r.add_argument("--repo", default="JorisJonkers-dev/nv-rs")
    r.add_argument("--slots", type=int, default=16)
    r.add_argument("--overhead", type=float, default=3.0, help="minutes of checkout, toolchain and cache restore per job")
    a = ap.parse_args()
    sys.exit({"collect": collect, "merge": merge, "replay": replay}[a.cmd](a))


if __name__ == "__main__":
    main()
