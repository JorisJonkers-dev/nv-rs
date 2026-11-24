#!/usr/bin/env python3
"""What a pull request changes in upstream-owned code, as one sticky comment (fork-divergence.yml, docs/fork/DIVERGENCE.md).

  divergence_report.py report --base SHA --head SHA --out comment.md [--json summary.json]

Classifies every changed file as fork-owned (.fork/overlay-paths, the workflows, the fork-only crates of
.fork/components.toml) or upstream-owned, counts the changed lines of the upstream-owned ones, measures the total
divergence (upstream-owned files that differ from upstream at the commit it was last replayed to, in files and lines)
at the base and the head and a week before the base, compares it with .fork/divergence-budget.toml and checks that
a record (one file per record in docs/fork/divergence.d/, or the older table of docs/fork/DIVERGENCE.md) gives the
reason and the evidence of each upstream-owned file the pull request starts to change. It warns only: the exit status is 0 whatever it finds.

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
sys.path.insert(0, HERE)
import coverage_report  # noqa: E402  (fork_owned_crates: one definition of "fork-owned crate")

MARKER = "<!-- fork-divergence -->"
DEFAULT_BUDGET = {"pr_changed_lines": 300, "weekly_growth_percent": 5.0, "window_days": 7}
# The workflows are the fork's own (upstream's ci.yml is not used here).
FORK_PREFIXES = (".github/workflows/",)
FIELDS = ("Upstream approach", "Fork approach", "Decision", "Reason", "Evidence")
MAX_ROWS = 40


def git(*cmd, check=True):
    done = subprocess.run(("git",) + cmd, capture_output=True, text=True, check=False)
    if check and done.returncode:
        raise RuntimeError(f"git {' '.join(cmd)} failed: {done.stderr[-500:]}")
    return done.stdout if done.returncode == 0 else ""


def read_patterns(text):
    out = []
    for line in text.splitlines():
        line = re.sub(r"#.*", "", line).strip()
        if line:
            out.append(line)
    return out


def classify(path, overlay, fork_crates):
    """'fork' for a fork-owned file, else 'upstream'."""
    if path.startswith(FORK_PREFIXES):
        return "fork"
    for pat in overlay:
        if (pat.endswith("/") and path.startswith(pat)) or path == pat:
            return "fork"
    m = re.match(r"^crates/([^/]+)/", path)
    if m and m.group(1) in fork_crates:
        return "fork"
    return "upstream"


def parse_numstat(text):
    """{path: changed lines}; a binary file counts as one line, a rename as its new path."""
    out = {}
    for line in text.splitlines():
        parts = line.split("\t")
        if len(parts) != 3:
            continue
        a, d, path = parts
        path = re.sub(r"\{.* => (.*)\}", r"\1", path).split(" => ")[-1]
        out[path] = 1 if a == "-" else int(a) + int(d)
    return out


def divergence(numstat, overlay, fork_crates):
    """(files, lines) of the upstream-owned entries of a numstat."""
    own = {p: n for p, n in numstat.items() if classify(p, overlay, fork_crates) == "upstream"}
    return len(own), sum(own.values())


def parse_records(text):
    """Divergence records: a `### Area` heading, then `- Key: value` lines (Files, Upstream approach, Fork approach,
    Decision, Reason, Evidence). Returns [{area, files(text), fields{name: value}}]."""
    records, cur = [], None
    for line in text.splitlines():
        if line.startswith("### "):
            cur = {"area": line[4:].strip(), "files": "", "fields": {}}
            records.append(cur)
        elif line.startswith("## "):
            cur = None
        elif cur is not None:
            m = re.match(r"^\s*[-*]\s*\**(Files|Upstream approach|Fork approach|Decision|Reason|Evidence)\**:\**\s*(.*)$", line)
            if m:
                key, val = m.group(1), m.group(2).strip()
                if key == "Files":
                    cur["files"] += " " + val
                else:
                    cur["fields"][key] = val
    return records


def mentions(text, path):
    return path in text or (path.startswith("crates/") and path[len("crates/"):] in text)


def record_status(path, doc):
    """'complete', 'incomplete: <missing>', 'table row only' (the older table, which has no evidence) or 'missing'."""
    for rec in parse_records(doc):
        if mentions(rec["files"], path):
            missing = [f for f in FIELDS if not rec["fields"].get(f)]
            if not rec["area"] or missing:
                return "incomplete: " + ", ".join(missing or ["Area"])
            return "complete"
    for line in doc.splitlines():
        if line.startswith("|") and mentions(line, path):
            return "table row only"
    return "missing"


def last_upstream_commit(ref):
    text = git("log", "-1", "--format=%B", r"--grep=^Upstream-Commit: [0-9a-f]\{40\}$", ref, check=False)
    found = re.findall(r"^Upstream-Commit: ([0-9a-f]{40})$", text, re.M)
    if found:
        return found[-1]
    out = git("merge-base", ref, "origin/main", check=False).strip()
    return out or None


def show(ref, path):
    return git("show", f"{ref}:{path}", check=False)


def load_budget(text):
    budget = dict(DEFAULT_BUDGET)
    try:
        budget.update(tomllib.loads(text))
    except tomllib.TOMLDecodeError:
        pass
    return budget


def analyse(base, head, root="."):
    os.chdir(root)
    overlay = read_patterns(show(head, ".fork/overlay-paths"))
    allowed = read_patterns(show(head, ".fork/allowed-divergence"))
    # One file per entry (nv-rs#438): every file of the folder at `head` adds its paths.
    for name in sorted(git("ls-tree", "--name-only", head, ".fork/allowed-divergence.d/", check=False).split()):
        if not os.path.basename(name).startswith("."):
            allowed += read_patterns(show(head, name))
    fork_crates = set(coverage_report.fork_owned_crates("."))
    budget = load_budget(show(head, ".fork/divergence-budget.toml"))
    doc = show(head, "docs/fork/DIVERGENCE.md")
    # One file per record, so pull requests do not all edit DIVERGENCE.md: each `### Area` file of the folder at `head`.
    for name in sorted(git("ls-tree", "--name-only", head, "docs/fork/divergence.d/", check=False).split()):
        if name.endswith(".md"):
            doc += "\n" + show(head, name)
    last = last_upstream_commit(base)
    pr = parse_numstat(git("diff", "--numstat", f"{base}...{head}"))
    touched = {p: n for p, n in pr.items() if classify(p, overlay, fork_crates) == "upstream"}
    res = {"budget": budget, "last": last, "pr_files": len(pr), "touched": touched,
           "pr_upstream_lines": sum(touched.values()), "fork_files": len(pr) - len(touched), "files": []}
    base_diff = {}
    if last:
        base_diff = parse_numstat(git("diff", "--numstat", last, base, check=False))
        res["base_div"] = divergence(base_diff, overlay, fork_crates)
        res["head_div"] = divergence(parse_numstat(git("diff", "--numstat", last, head, check=False)), overlay, fork_crates)
        week = git("rev-list", "-1", f"--before={budget['window_days']} days ago", base, check=False).strip()
        if week:
            # The upstream commit of that day: the replay point moves, and the old tree lacks what was replayed since.
            week_last = last_upstream_commit(week)
            if week_last:
                res["week_div"] = divergence(parse_numstat(git("diff", "--numstat", week_last, week, check=False)), overlay, fork_crates)
    for path in sorted(touched):
        new = bool(last) and path not in base_diff
        res["files"].append({"path": path, "lines": touched[path], "listed": path in allowed, "new": new,
                             "record": record_status(path, doc) if new or path not in allowed else "listed"})
    return res


def pct_growth(new, old):
    return None if not old else 100.0 * (new - old) / old


def render(res):
    b, files = res["budget"], res["files"]
    warnings = []
    if res["pr_upstream_lines"] > b["pr_changed_lines"]:
        warnings.append(f"This pull request changes {res['pr_upstream_lines']} lines of upstream-owned code "
                        f"(budget {b['pr_changed_lines']}).")
    growth = pct_growth(res["head_div"][1], res["week_div"][1]) if "week_div" in res and "head_div" in res else None
    if growth is not None and growth > b["weekly_growth_percent"]:
        warnings.append(f"Total divergence grew {growth:.1f}% in {b['window_days']} days (budget {b['weekly_growth_percent']}%).")
    bad = [f for f in files if f["new"] and f["record"] != "complete"]
    for f in bad:
        warnings.append(f"New upstream-owned file `{f['path']}`: divergence record {f['record']}.")
    out = [MARKER, "## Divergence from upstream", "",
           "Warns only, never blocks a merge. Upstream-owned means outside `.fork/overlay-paths`, the fork-only crates and "
           "the workflows (docs/fork/DIVERGENCE.md, \"Process\").", ""]
    out += [f"- **Warning:** {w}" for w in warnings] or ["No warning: within the budget, and every new upstream-owned file is recorded."]
    out += ["", "| | This pull request | Total at the base | Total at the head |", "| --- | ---: | ---: | ---: |",
            f"| Fork-owned files changed | {res['fork_files']} | | |",
            f"| Upstream-owned files changed | {len(files)} | {res['base_div'][0] if 'base_div' in res else 'n/a'} | "
            f"{res['head_div'][0] if 'head_div' in res else 'n/a'} |",
            f"| Upstream-owned lines changed | **{res['pr_upstream_lines']}** (budget {b['pr_changed_lines']}) | "
            f"{res['base_div'][1] if 'base_div' in res else 'n/a'} | {res['head_div'][1] if 'head_div' in res else 'n/a'} |"]
    if growth is not None:
        out.append(f"| Growth in {b['window_days']} days | | | {growth:+.1f}% ({res['week_div'][1]} lines then) |")
    if not res["last"]:
        out += ["", "The commit upstream was last replayed to was not found: totals are not available."]
    if files:
        out += ["", "| Upstream-owned file | Lines | In allowed-divergence | New | Divergence record |",
                "| --- | ---: | --- | --- | --- |"]
        for f in sorted(files, key=lambda f: -f["lines"])[:MAX_ROWS]:
            out.append(f"| `{f['path']}` | {f['lines']} | {'yes' if f['listed'] else 'no'} | {'yes' if f['new'] else 'no'} | {f['record']} |")
        if len(files) > MAX_ROWS:
            out.append(f"| and {len(files) - MAX_ROWS} more | | | | |")
    out += ["", "A record is a file of docs/fork/divergence.d/ with one `### Area` section and the lines Files, Upstream approach, Fork approach, "
                "Decision, Reason and Evidence. Budget: `.fork/divergence-budget.toml`."]
    return "\n".join(out) + "\n", warnings


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("report")
    r.add_argument("--base", required=True)
    r.add_argument("--head", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--json")
    a = ap.parse_args()
    try:
        res = analyse(a.base, a.head)
        text, warnings = render(res)
    except Exception as exc:  # warn-only: a failure of the report is itself reported, never raised
        text, warnings, res = f"{MARKER}\n## Divergence from upstream\n\nThe report could not be made: {exc}\n", [str(exc)], {}
    with open(a.out, "w", encoding="utf-8") as fh:
        fh.write(text)
    if a.json:
        with open(a.json, "w", encoding="utf-8") as fh:
            json.dump({"warnings": warnings, "pr_upstream_lines": res.get("pr_upstream_lines"),
                       "base_div": res.get("base_div"), "head_div": res.get("head_div")}, fh)
    sys.stdout.write(text)


if __name__ == "__main__":
    main()
