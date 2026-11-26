#!/usr/bin/env python3
"""Coverage of a pull request, per engine component, as one sticky comment (fork-coverage.yml, docs/fork/COVERAGE.md).

  coverage_report.py plan --event EVENT [--base SHA --head SHA]
      JSON for the workflow matrix: the Linux components to measure. Uses scripts/fork/components.py (the same
      affected selection as the build), keeps the ones cargo-llvm-cov can measure on a plain runner.
  coverage_report.py gate --dir DIR --plan plan.json --base SHA --head SHA [--out gate.md]
      Changed-line coverage of the fork-owned crates against the minimum of .fork/coverage-gate.toml. Exit 1 only
      when `enforce = true`, a nightly baseline exists and the coverage is below the minimum; otherwise it informs.
  coverage_report.py report --dir DIR --plan plan.json --base SHA --head SHA --out comment.md [--json summary.json]
      Reads DIR/cov-<component>/{lcov.info,base.info,baseline.txt} and renders the comment: totals for the pull
      request against the base, a table per component, the changed files and the changed lines no test ran.

  coverage_report.py pick-run --runs runs.json --merge-base SHA
      The id of the nightly (or manual) run that measured exactly the commit SHA, from the JSON of
      `gh run list --json databaseId,headSha,event,conclusion` (newest first); nothing when there is none.

Stdlib only; Python 3.11 or newer.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
MARKER = "<!-- fork-coverage -->"
# Not measured: these need the Nix shell (GPU, shader compiler), take hours, or are not made of workspace crates.
SKIP = {"rendering", "platform", "integration", "launcher-and-ledger", "release-files", "repo-checks"}
# A change to one of these runs the cheapest component, so a pull request that only changes the coverage tooling
# still shows a comment with numbers.
SMOKE_FILES = (".github/workflows/fork-coverage.yml", "scripts/fork/coverage_report.py")
SMOKE_COMPONENT = "formats"
MAX_FILES = 30


def sh(*cmd):
    done = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if done.returncode:
        sys.exit(f"{' '.join(cmd)} failed:\n{done.stderr[-1500:]}")
    return done.stdout


def changed_files(base, head):
    return [f for f in sh("git", "diff", "--name-only", "--diff-filter=ACMR", f"{base}...{head}").splitlines() if f]


def changed_ranges(base, head):
    """{file: set of added or changed line numbers} from `git diff -U0` (the new side of every hunk)."""
    out, current = {}, None
    for line in sh("git", "diff", "-U0", "--diff-filter=ACMR", f"{base}...{head}", "--", "*.rs").splitlines():
        if line.startswith("+++ "):
            current = line[6:] if line.startswith("+++ b/") else None
            continue
        m = re.match(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", line)
        if m and current:
            start, count = int(m.group(1)), int(m.group(2) or "1")
            out.setdefault(current, set()).update(range(start, start + count))
    return out


def plan(args):
    cmd = [sys.executable, "-I", os.path.join(HERE, "components.py"), "matrix", "--event", args.event]
    files = []
    if args.event == "pull_request":
        cmd += ["--base", args.base, "--head", args.head]
        files = changed_files(args.base, args.head)
    matrix = json.loads(sh(*cmd))
    include, seen = [], set()

    def add(entry):
        script = entry["script"].splitlines()[0] if entry["script"] else ""
        if entry["component"] in seen or not script.startswith("cargo test"):
            return
        seen.add(entry["component"])
        rest = script[len("cargo test"):].replace(" --locked", "").strip()
        include.append({"component": entry["component"], "name": f"Coverage: {entry['component']}", "args": rest,
                        "cache": entry["cache"]})

    for entry in matrix["include"]:
        if entry["os"] == "ubuntu-latest" and not entry["shard"] and entry["component"] not in SKIP:
            add(entry)
    reason = matrix.get("reason", "")
    if not include and any(f in SMOKE_FILES for f in files):
        full = json.loads(sh(sys.executable, "-I", os.path.join(HERE, "components.py"), "matrix", "--event", "push"))
        for entry in full["include"]:
            if entry["component"] == SMOKE_COMPONENT and entry["os"] == "ubuntu-latest":
                add(entry)
        reason = f"the coverage tooling changed: {SMOKE_COMPONENT} as a smoke run"
    print(json.dumps({"include": include, "reason": reason}))


def parse_lcov(path):
    """{file: {line: hits}} with every path made relative to the repository (from its `crates/` directory on)."""
    files, current = {}, None
    try:
        fh = open(path, encoding="utf-8", errors="replace")
    except OSError:
        return None
    with fh:
        for line in fh:
            line = line.strip()
            if line.startswith("SF:"):
                name = line[3:]
                at = name.find("/crates/")
                current = files.setdefault(name[at + 1:] if at >= 0 else name, {})
            elif line.startswith("DA:") and current is not None:
                num, _, rest = line[3:].partition(",")
                hits = int(rest.split(",")[0])
                n = int(num)
                current[n] = max(current.get(n, 0), hits)
    return files


def totals(files):
    found = sum(len(v) for v in files.values())
    hit = sum(1 for v in files.values() for h in v.values() if h > 0)
    return hit, found


def pct(hit, found):
    return 100.0 * hit / found if found else None


def fmt_pct(value):
    return "n/a" if value is None else f"{value:.2f}%"


def fmt_delta(new, old):
    if new is None or old is None:
        return "n/a"
    d = new - old
    return f"{d:+.2f} pp"


def spans(lines):
    out, run = [], None
    for n in sorted(lines):
        if run and n == run[1] + 1:
            run[1] = n
        else:
            run = [n, n]
            out.append(run)
    return ",".join(str(a) if a == b else f"{a}-{b}" for a, b in out)


def load_plan(path, directory, required=False):
    """The plan artifact, wherever the download put it; an absent or unreadable plan means nothing to measure.

    The gate passes `required=True`: a plan that cannot be read is an error there, not an empty plan that would
    let the gate pass silently (docs/fork/COVERAGE.md).
    """
    for candidate in (path, os.path.join(directory, "plan.json")):
        try:
            with open(candidate, encoding="utf-8") as fh:
                doc = json.load(fh)
            doc.setdefault("include", [])
            return doc
        except (OSError, ValueError, AttributeError):
            continue
    if required:
        raise FileNotFoundError(f"no readable plan artifact at {path} or {os.path.join(directory, 'plan.json')}")
    return {"include": [], "reason": "no plan was recorded for this run, so nothing was measured"}


def fork_owned_crates(root="."):
    """Crates only the fork has: directories named in .fork/overlay-paths (crates/<name>/), and the crates of a
    component whose purpose says "Fork-owned" (.fork/components.toml), plus `extra_crates` of coverage-gate.toml."""
    crates = set()
    try:
        with open(os.path.join(root, ".fork/overlay-paths"), encoding="utf-8") as fh:
            for line in fh:
                m = re.match(r"^crates/([^/\s#]+)/\s*(#.*)?$", line.strip())
                if m:
                    crates.add(m.group(1))
    except OSError:
        pass
    try:
        with open(os.path.join(root, ".fork/components.toml"), "rb") as fh:
            for comp in tomllib.load(fh).get("component", []):
                if "fork-owned" in comp.get("purpose", "").lower():
                    crates.update(comp.get("crates", []))
    except OSError:
        pass
    crates.update(load_gate_config(root).get("extra_crates", []))
    return sorted(crates)


def load_gate_config(root="."):
    cfg = {"enforce": False, "min_changed_line_coverage": 70.0, "extra_crates": []}
    try:
        with open(os.path.join(root, ".fork/coverage-gate.toml"), "rb") as fh:
            cfg.update(tomllib.load(fh))
    except (OSError, tomllib.TOMLDecodeError):
        pass
    return cfg


def gate_eval(measured, ranges, crates, minimum):
    """(executable changed lines, uncovered ones, percent or None, per-crate rows) over the fork-owned crates."""
    per = {}
    for f, nums in ranges.items():
        m = re.match(r"^crates/([^/]+)/", f)
        lines = measured.get(f)
        if not m or m.group(1) not in crates or lines is None:
            continue
        ex = [n for n in nums if n in lines]
        row = per.setdefault(m.group(1), [0, 0])
        row[0] += len(ex)
        row[1] += sum(1 for n in ex if lines[n] == 0)
    total = sum(r[0] for r in per.values())
    miss = sum(r[1] for r in per.values())
    value = 100.0 * (total - miss) / total if total else None
    return total, miss, value, per


def has_nightly_baseline(baselines):
    """A baseline made at the merge base: a nightly run on that very commit, or the commit measured in the run
    (when the last nightly was on another commit); not the one measured because no nightly run exists at all."""
    return any(b.startswith(("the last nightly", "the merge base")) for b in baselines)


def pick_run(runs, merge_base):
    """The newest successful run that is not a pull request's and measured exactly `merge_base`, else None."""
    for run in runs:
        if run.get("event") == "pull_request" or run.get("conclusion", "success") != "success":
            continue
        if run.get("headSha") == merge_base:
            return run.get("databaseId")
    return None


def pick_run_cmd(args):
    with open(args.runs, encoding="utf-8") as fh:
        found = pick_run(json.load(fh), args.merge_base)
    if found is not None:
        print(found)


def render_gate(cfg, crates, baselines, result):
    total, miss, value, per = result
    minimum = cfg["min_changed_line_coverage"]
    baseline = has_nightly_baseline(baselines)
    enforcing = bool(cfg["enforce"]) and baseline
    out = ["### Fork-owned crates: changed-line coverage", ""]
    if value is None:
        out.append("No executable changed line in a measured fork-owned crate: nothing to gate.")
        status = "none"
    else:
        ok = value >= minimum
        status = "pass" if ok else "below"
        out.append(f"{value:.1f}% of {total} executable changed lines covered ({miss} uncovered), minimum {minimum:.0f}%: "
                   f"**{'ok' if ok else 'below the minimum'}**.")
        out += ["", "| Crate | Changed lines | Uncovered |", "| --- | ---: | ---: |"]
        out += [f"| {c} | {r[0]} | {r[1]} |" for c, r in sorted(per.items())]
    mode = ("enforced" if enforcing else
            "informational: `enforce = false` in .fork/coverage-gate.toml" if not cfg["enforce"] else
            "informational: no nightly baseline artifact yet")
    out += ["", f"Mode: {mode}. Fork-owned crates: {', '.join(crates) or 'none'}. Upstream-owned code is report-only."]
    return "\n".join(out) + "\n", status, enforcing


def merge_dir(directory, plan_doc, ranges):
    rows, merged_pr, merged_base, missing = [], {}, {}, []
    baselines = set()
    for item in plan_doc["include"]:
        comp = item["component"]
        base_dir = os.path.join(directory, f"cov-{comp}")
        pr = parse_lcov(os.path.join(base_dir, "lcov.info"))
        base = parse_lcov(os.path.join(base_dir, "base.info"))
        try:
            baselines.add(open(os.path.join(base_dir, "baseline.txt"), encoding="utf-8").read().strip())
        except OSError:
            pass
        if pr is None:
            missing.append(comp)
            continue
        for src, dst in ((pr, merged_pr), (base or {}, merged_base)):
            for f, lines in src.items():
                d = dst.setdefault(f, {})
                for n, h in lines.items():
                    d[n] = max(d.get(n, 0), h)
        unc = sum(1 for f, lines in pr.items() for n in ranges.get(f, ()) if n in lines and lines[n] == 0)
        rows.append((comp, totals(pr), totals(base) if base is not None else None, unc))
    return rows, merged_pr, merged_base, missing, baselines


def gate(args):
    try:
        plan_doc = load_plan(args.plan, args.dir, required=True)
    except FileNotFoundError as exc:
        sys.exit(f"coverage-gate: {exc}")
    ranges = changed_ranges(args.base, args.head)
    _, merged_pr, _, _, baselines = merge_dir(args.dir, plan_doc, ranges)
    cfg, crates = load_gate_config(), fork_owned_crates()
    text, status, enforcing = render_gate(cfg, crates, baselines, gate_eval(merged_pr, ranges, set(crates), cfg["min_changed_line_coverage"]))
    sys.stdout.write(text)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as fh:
            fh.write(text)
    if enforcing and status == "below":
        sys.exit("coverage-gate: changed-line coverage of the fork-owned crates is below the minimum")


def report(args):
    plan_doc = load_plan(args.plan, args.dir)
    ranges = changed_ranges(args.base, args.head)
    all_changed = changed_files(args.base, args.head)
    rows, merged_pr, merged_base, missing, baselines = merge_dir(args.dir, plan_doc, ranges)

    p_hit, p_found = totals(merged_pr)
    b_hit, b_found = totals(merged_base)
    have_base = bool(merged_base)
    p_pct, b_pct = pct(p_hit, p_found), (pct(b_hit, b_found) if have_base else None)

    # Changed Rust lines: executable ones that a measured component covers or misses.
    changed_rows, unc_total, exec_total, unmeasured = [], 0, 0, []
    for f in sorted(ranges):
        lines = merged_pr.get(f)
        if lines is None:
            unmeasured.append(f)
            continue
        ex = [n for n in sorted(ranges[f]) if n in lines]
        miss = [n for n in ex if lines[n] == 0]
        exec_total += len(ex)
        unc_total += len(miss)
        if ex:
            changed_rows.append((f, len(ex), miss))

    out = [MARKER, "## Test coverage (Linux, line coverage)", ""]
    out.append("Informational: this report never blocks a merge. It measures the engine components the change reaches "
               "(`scripts/fork/components.py`), with `cargo llvm-cov`, on Linux only.")
    out.append("")
    if not rows:
        out += [f"No component was measured ({plan_doc.get('reason') or 'no component of the change can be measured on Linux'})."]
        if missing:
            out += ["", "Measurement failed for: " + ", ".join(missing) + "."]
    else:
        out += ["| | This pull request | Base | Delta |", "| --- | ---: | ---: | ---: |",
                f"| Line coverage | **{fmt_pct(p_pct)}** ({p_hit}/{p_found}) | "
                f"{fmt_pct(b_pct)}{f' ({b_hit}/{b_found})' if have_base else ''} | {fmt_delta(p_pct, b_pct)} |",
                f"| Changed Rust files | {len(ranges)} | | |",
                f"| Changed executable lines | {exec_total} | | |",
                f"| Uncovered changed lines | **{unc_total}** | | |"]
        out += ["", "| Component | Lines | PR | Base | Delta | Uncovered changed lines |",
                "| --- | ---: | ---: | ---: | ---: | ---: |"]
        for comp, (h, fnd), base, unc in rows:
            bp = pct(*base) if base else None
            out.append(f"| {comp} | {fnd} | {fmt_pct(pct(h, fnd))} | {fmt_pct(bp)} | {fmt_delta(pct(h, fnd), bp)} | {unc} |")
        if changed_rows:
            out += ["", f"<details><summary>Changed files ({len(changed_rows)} with executable changed lines)</summary>", "",
                    "| File | Changed lines | Uncovered | Uncovered lines |", "| --- | ---: | ---: | --- |"]
            for f, n, miss in sorted(changed_rows, key=lambda r: -len(r[2]))[:MAX_FILES]:
                out.append(f"| `{f}` | {n} | {len(miss)} | {spans(miss)[:80] or '-'} |")
            if len(changed_rows) > MAX_FILES:
                out.append(f"| and {len(changed_rows) - MAX_FILES} more | | | |")
            out += ["", "</details>"]
        if missing:
            out += ["", "Not measured (the run failed or produced no data): " + ", ".join(missing) + "."]
    others = [f for f in all_changed if f.endswith(".rs") and f in unmeasured]
    if others:
        out += ["", f"{len(others)} changed Rust file(s) are in no measured component (not affected, Windows only, "
                    "or too slow to measure here): not counted."]
    if rows:
        cfg, crates = load_gate_config(), fork_owned_crates()
        out += [""] + render_gate(cfg, crates, baselines, gate_eval(merged_pr, ranges, set(crates), cfg["min_changed_line_coverage"]))[0].splitlines()
    note = "; ".join(sorted(baselines)) or "no baseline"
    out += ["", f"Base: {note}. Artifact: `cov-*` (lcov) of this run. Details: docs/fork/COVERAGE.md."]
    text = "\n".join(out) + "\n"
    with open(args.out, "w", encoding="utf-8") as fh:
        fh.write(text)
    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump({"pr_pct": p_pct, "base_pct": b_pct, "lines": p_found, "covered": p_hit,
                       "uncovered_changed": unc_total, "components": [r[0] for r in rows]}, fh)
    sys.stdout.write(text)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("plan")
    p.add_argument("--event", required=True)
    p.add_argument("--base")
    p.add_argument("--head")
    r = sub.add_parser("report")
    r.add_argument("--dir", required=True)
    r.add_argument("--plan", required=True)
    r.add_argument("--base", required=True)
    r.add_argument("--head", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--json")
    g = sub.add_parser("gate")
    for name in ("dir", "plan", "base", "head"):
        g.add_argument(f"--{name}", required=True)
    g.add_argument("--out")
    k = sub.add_parser("pick-run")
    k.add_argument("--runs", required=True)
    k.add_argument("--merge-base", required=True)
    a = ap.parse_args()
    {"plan": plan, "report": report, "gate": gate, "pick-run": pick_run_cmd}[a.cmd](a)


if __name__ == "__main__":
    main()
