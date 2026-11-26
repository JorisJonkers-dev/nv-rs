#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Drive one batch of the porting pipeline from the ledger's work queue.

The loop takes no hand-written work list. It reads the queue command of the ledger
(scripts/ledger), gives each family its own git worktree, runs a configured worker
there, runs that family's Diff cases through the build daemon's family lab, asks a
configured reviewer, and prepares one pull request per batch. A shakedown batch is
thrown away unmerged and records what it showed in the rulebook. A weekly report
counts delivered verified functions per day from the queue and the delivery log.

Workers and reviewers are commands from NV_PORT_WORKER and NV_PORT_REVIEW: no
service is built into this tool. Both must be set, so a batch cannot pass without
an independent review. The commands receive the family, its queue rows and their
worktree in NV_PORT_* variables. The checker files are read-only to workers: the
integrity hash (scripts/fork/private_integrity.py) is checked before the batch and
again in each worktree before any of its work is accepted.

Usage:
    python3 -I scripts/fork/porting_loop.py batch [--count N] [--name NAME]
    python3 -I scripts/fork/porting_loop.py shakedown [--count 100]
    python3 -I scripts/fork/porting_loop.py weekly [--days 7] [--out FILE]
"""
import argparse
import datetime
import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

FORK = Path(__file__).resolve().parent
REPO = FORK.parent.parent
LEDGER = FORK.parent / "ledger" / "Cargo.toml"
RULEBOOK = REPO / "docs" / "fork" / "PORTING-RULEBOOK.md"
START = "<!-- shakedown:start -->"
END = "<!-- shakedown:end -->"
PASS = re.compile(r"test result: ok\. (\d+) passed; (\d+) failed")
FAIL = re.compile(r"test result: FAILED\. (\d+) passed; (\d+) failed")
UNSAFE = re.compile(r"[^A-Za-z0-9_.-]+")

sys.path.insert(0, str(FORK))
import private_integrity  # noqa: E402


def call(argv, cwd, env=None, log=None):
    """Run one command, return (exit status, combined output), and keep its log."""
    result = subprocess.run(
        [str(a) for a in argv], cwd=str(cwd), env=env,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, errors="replace", check=False,
    )
    if log is not None:
        log = Path(log)
        log.parent.mkdir(parents=True, exist_ok=True)
        log.write_text(result.stdout, encoding="utf-8")
    return result.returncode, result.stdout


def slug(family):
    """A family name as one safe file and branch component."""
    return UNSAFE.sub("_", family).strip("_") or "family"


def paths(root, name, family=None):
    base = Path(root) / "target" / "porting-loop"
    if family is None:
        return base / name
    return base / "worktrees" / name / slug(family)


def claim(root, family, owner):
    """Take a family exclusively; a second claimant gets None, on every system."""
    directory = Path(root) / "target" / "porting-loop" / "claims"
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / (slug(family) + ".claim")
    try:
        handle = path.open("x", encoding="utf-8")
    except FileExistsError:
        return None
    with handle:
        handle.write(owner + "\n")
    return path


def release(root, family):
    path = Path(root) / "target" / "porting-loop" / "claims" / (slug(family) + ".claim")
    path.unlink(missing_ok=True)


def parse_queue(text):
    """The ledger queue's tab-separated rows, without its comment header."""
    rows = []
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) != 6:
            raise ValueError(f"bad ledger queue row: {line!r}")
        rank, address, state, done, family, name = fields
        rows.append({
            "rank": int(rank), "address": address, "state": state,
            "done": done, "family": family, "name": name,
        })
    return rows


def ledger_command(root, args):
    """The ledger runs through the build daemon's client, like every other build."""
    return [
        sys.executable, str(FORK / "build_daemon.py"), "run", "--",
        "run", "--quiet", "--manifest-path", str(Path(root) / LEDGER.relative_to(REPO)),
        "--", *args,
    ]


def queue(root, env, runner):
    """The only source of work: the ledger's queue command."""
    code, text = runner(ledger_command(root, ["queue"]), cwd=root, env=env)
    if code:
        raise RuntimeError(f"ledger queue exited {code}")
    return parse_queue(text)


def diff_counts(text):
    """Pass and failure counts from a family lab run."""
    passed = failed = 0
    for pattern in (PASS, FAIL):
        for match in pattern.finditer(text):
            passed += int(match.group(1))
            failed += int(match.group(2))
    return {"passed": passed, "failed": failed}


def failure_classes(root, addresses):
    """The ledger's recorded classes for these functions, counted by kind."""
    path = Path(root) / "scripts" / "ledger" / "function_evidence.tsv"
    counts = {}
    if not path.is_file():
        return counts
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) >= 3 and fields[0] in addresses and fields[2]:
            counts[fields[2]] = counts.get(fields[2], 0) + 1
    return counts


def family_env(env, rows, name, worktree, queue_file, shakedown):
    child = dict(env)
    child.update({
        "NV_PORT_FAMILY": rows[0]["family"],
        "NV_PORT_BATCH": name,
        "NV_PORT_WORKTREE": str(worktree),
        "NV_PORT_QUEUE_FILE": str(queue_file),
        "NV_PORT_ADDRESSES": ",".join(r["address"] for r in rows),
        "NV_PORT_SHAKEDOWN": "1" if shakedown else "0",
    })
    return child


def configured(env, key):
    value = env.get(key, "").strip()
    if not value:
        raise ValueError(f"{key} must name a command; no service is built into the loop")
    return shlex.split(value, posix=os.name != "nt")


def pr_body(name, report):
    """One pull request's evidence: totals, per-family counts and failure classes."""
    totals = {"passed": 0, "failed": 0}
    for family in report["families"]:
        totals["passed"] += family["passed"]
        totals["failed"] += family["failed"]
    lines = [f"# Porting batch {name}", "", "## Pass counts", "",
             f"- Diff cases: {totals['passed']} passed; {totals['failed']} failed"]
    for family in report["families"]:
        lines.append(
            f"- {family['family']}: {family['passed']} passed, {family['failed']} failed "
            f"({family['review']})"
        )
    lines += ["", "## Failure classes", ""]
    lines += [f"- {kind}: {count}" for kind, count in sorted(report["classes"].items())]
    if not report["classes"]:
        lines.append("- none recorded")
    lines += ["", "The checker hash and the review result are recorded in the batch report.", ""]
    return "\n".join(lines)


def report_text(name, report):
    """The batch report kept with the batch branch, without private material."""
    lines = [
        "<!-- SPDX-License-Identifier: GPL-3.0-only -->",
        f"# Porting batch {name}", "",
        f"- Status: {report['status']}",
        f"- Functions offered: {report['functions']}",
        f"- Checker: {report['checker']}",
        "",
        pr_body(name, report),
    ]
    return "\n".join(lines)


def update_rulebook(path, section):
    """Replace only the shakedown section, leaving the standing rules alone."""
    path = Path(path)
    text = path.read_text(encoding="utf-8")
    if text.count(START) != 1 or text.count(END) != 1:
        raise ValueError(f"{path} needs one shakedown section with both markers")
    before, rest = text.split(START, 1)
    _, after = rest.split(END, 1)
    path.write_text(before + START + "\n" + section.strip() + "\n" + END + after, encoding="utf-8")


def checker_hash(root):
    """The digest of the protected files, for the batch evidence."""
    try:
        return private_integrity.digest(root)
    except (OSError, ValueError):
        return "unavailable"


def write_json(root, name, data):
    path = paths(root, name) / (name + ".json")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def batch_record(root, name):
    return json.loads((paths(root, name) / (name + ".json")).read_text(encoding="utf-8"))


def record_delivery(root, date, name, address, family):
    path = Path(root) / "target" / "porting-loop" / "delivered.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps({
            "date": date, "batch": name, "address": address, "family": family,
        }, sort_keys=True) + "\n")


def deliveries(root):
    path = Path(root) / "target" / "porting-loop" / "delivered.jsonl"
    if not path.is_file():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def batch(root=REPO, count=20, name=None, shakedown=False, env=None,
          today=None, call=call):
    """Run one batch from the queue; return a process exit status."""
    root = Path(root)
    today = today or datetime.date.today()
    env = dict(os.environ if env is None else env)
    name = name or ("shakedown" if shakedown else today.isoformat())
    try:
        worker = configured(env, "NV_PORT_WORKER")
        reviewer = configured(env, "NV_PORT_REVIEW")
    except ValueError as exc:
        print(f"failed: {exc}", file=sys.stderr)
        return 2
    ok, why = private_integrity.check(root)
    if not ok:
        print(f"failed: {why}", file=sys.stderr)
        return 1
    try:
        rows = [r for r in queue(root, env, call)
                if r["done"] == "no" and r["state"] != "retired"][:count]
    except (RuntimeError, ValueError) as exc:
        print(f"failed: {exc}", file=sys.stderr)
        return 1
    groups = {}
    for row in rows:
        groups.setdefault(row["family"], []).append(row)

    accepted, created, owned = [], [], []
    try:
        for family, family_rows in groups.items():
            held = claim(root, family, name)
            if held is None:
                print(f"skip {family}: another worker holds the claim", file=sys.stderr)
                continue
            owned.append(held)
            worktree = paths(root, name, family)
            branch = f"porting/{slug(name)}-{slug(family)}"
            logdir = paths(root, name) / "logs"
            queue_file = paths(root, name) / (slug(family) + ".tsv")
            queue_file.parent.mkdir(parents=True, exist_ok=True)
            queue_file.write_text(
                "# rank\taddress\tstate\tdone\tfamily\tname\n"
                + "".join("\t".join(str(r[k]) for k in
                         ("rank", "address", "state", "done", "family", "name")) + "\n"
                         for r in family_rows),
                encoding="utf-8",
            )
            child = family_env(env, family_rows, name, worktree, queue_file, shakedown)
            code, _ = call(["git", "worktree", "add", "-b", branch, str(worktree), "HEAD"],
                           cwd=root, env=env)
            if code:
                continue
            created.append((worktree, branch))
            code, _ = call(worker, cwd=worktree, env=child,
                           log=logdir / (slug(family) + ".worker.log"))
            if code:
                print(f"failed: worker for {family} exited {code}", file=sys.stderr)
                continue
            # family_lab.py sends its cargo arguments to the build daemon itself.
            code, output = call(
                [sys.executable, str(worktree / "scripts" / "fork" / "family_lab.py"),
                 "run", family],
                cwd=worktree, env=child, log=logdir / (slug(family) + ".diff.log"),
            )
            counts = diff_counts(output)
            code, _ = call(reviewer, cwd=worktree, env=child,
                           log=logdir / (slug(family) + ".review.log"))
            review = "accepted" if code == 0 else "rejected"
            ok, why = private_integrity.check(worktree)
            if not ok:
                print(f"failed: {family}: {why}", file=sys.stderr)
                return 1
            if review == "accepted":
                accepted.append({
                    "family": family, "branch": branch, "worktree": worktree,
                    "addresses": [r["address"] for r in family_rows],
                    **counts, "review": review,
                })

        report = {
            "status": "closed unmerged" if shakedown else "open",
            "functions": len(rows),
            "families": [{k: f[k] for k in ("family", "passed", "failed", "review")} for f in accepted],
            "classes": failure_classes(root, {r["address"] for r in rows}),
            "checker": checker_hash(root),
        }
        write_json(root, name, report)

        if shakedown:
            lines = [
                f"### Shakedown {name} (closed unmerged)", "",
                f"- Functions offered: {report['functions']}",
                f"- Families accepted: {len(accepted)}",
                f"- Diff cases passed: {sum(f['passed'] for f in accepted)}",
                f"- Diff cases failed: {sum(f['failed'] for f in accepted)}",
                "- Failure classes: " + (", ".join(
                    f"{kind} {count}" for kind, count in sorted(report["classes"].items())
                ) or "none recorded"),
                "- Rule: every class seen here is a required triage class in later batches.",
                "- Rule: a batch is accepted only with a passing review and an unchanged checker.",
                "",
            ]
            rulebook = RULEBOOK if root == REPO else root / "docs" / "fork" / "PORTING-RULEBOOK.md"
            update_rulebook(rulebook, "\n".join(lines))
            close = env.get("NV_PORT_CLOSE", "").strip()
            if close:
                code, _ = call(shlex.split(close, posix=os.name != "nt") + [
                    "--batch", name, "--reason", "shakedown closed unmerged; rulebook updated",
                ], cwd=root, env=env)
                if code:
                    print(f"failed: shakedown close command exited {code}", file=sys.stderr)
                    return 1
        elif accepted:
            batch_wt = paths(root, name) / "branch"
            code, _ = call(["git", "worktree", "add", "-b", f"porting/{slug(name)}",
                            str(batch_wt), "HEAD"], cwd=root, env=env)
            if code:
                return 1
            report_path = batch_wt / "docs" / "fork" / "PORTING-BATCHES" / f"{slug(name)}.md"
            report_path.parent.mkdir(parents=True, exist_ok=True)
            report_path.write_text(report_text(name, report), encoding="utf-8")
            call(["git", "add", "--", str(report_path.relative_to(batch_wt))], cwd=batch_wt, env=env)
            code, _ = call(["git", "commit", "-m", f"test(fork): porting batch {name} report"],
                           cwd=batch_wt, env=env)
            if code:
                print(f"failed: commit of the batch report", file=sys.stderr)
                return 1
            for family in accepted:
                code, _ = call(["git", "merge", "--no-ff", "-m",
                                f"chore(fork): merge {family['family']} into batch {name}",
                                family["branch"]], cwd=batch_wt, env=env)
                if code:
                    print(f"failed: merge of {family['family']}", file=sys.stderr)
                    return 1
            body = paths(root, name) / f"{slug(name)}.pr.md"
            body.parent.mkdir(parents=True, exist_ok=True)
            body.write_text(pr_body(name, report), encoding="utf-8")
            pr = env.get("NV_PORT_PR", "").strip()
            if pr:
                code, _ = call(shlex.split(pr, posix=os.name != "nt") + [
                    "--title", f"feat(engine): porting batch {name}", "--body", str(body),
                    "--head", f"porting/{slug(name)}",
                ], cwd=root, env=env)
                if code:
                    print(f"failed: pull request command exited {code}", file=sys.stderr)
                    return 1
            else:
                print(f"pull request body: {body}")
            for family in accepted:
                for address in family["addresses"]:
                    record_delivery(root, today.isoformat(), name, address, family["family"])
        return 0
    finally:
        for worktree, branch in created:
            call(["git", "worktree", "remove", "--force", str(worktree)], cwd=root, env=env)
            call(["git", "branch", "-D", branch], cwd=root, env=env)
        for held in owned:
            Path(held).unlink(missing_ok=True)


def weekly(root=REPO, days=7, env=None, today=None, out=None, call=call):
    """Delivered verified functions per day, from the queue and the delivery log."""
    root = Path(root)
    today = today or datetime.date.today()
    env = dict(os.environ if env is None else env)
    rows = queue(root, env, call)
    verified = {r["address"] for r in rows if r["state"] == "reference-verified"}
    first = today - datetime.timedelta(days=days)
    counts = {}
    for record in deliveries(root):
        try:
            date = datetime.date.fromisoformat(record["date"])
        except (KeyError, ValueError):
            continue
        if record.get("address") in verified and first < date <= today:
            counts[date.isoformat()] = counts.get(date.isoformat(), 0) + 1
    lines = [
        "<!-- SPDX-License-Identifier: GPL-3.0-only -->",
        "# Weekly porting rate", "",
        "| date | delivered verified functions |",
        "| --- | --- |",
    ]
    lines += [f"| {date} | {counts[date]} |" for date in sorted(counts)]
    lines += ["", f"- Window: {first.isoformat()} to {today.isoformat()}",
              f"- Total: {sum(counts.values())}", ""]
    text = "\n".join(lines)
    if out is not None:
        out = Path(out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(text, encoding="utf-8")
    print(text)
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", type=Path, default=REPO, help=argparse.SUPPRESS)
    modes = parser.add_subparsers(dest="mode", required=True)
    batch_parser = modes.add_parser("batch", help="run one batch from the ledger queue")
    batch_parser.add_argument("--count", type=int, default=20)
    batch_parser.add_argument("--name")
    shake_parser = modes.add_parser("shakedown", help="throw away the first batch and update the rulebook")
    shake_parser.add_argument("--count", type=int, default=100)
    shake_parser.add_argument("--name", default="shakedown")
    week_parser = modes.add_parser("weekly", help="report delivered verified functions per day")
    week_parser.add_argument("--days", type=int, default=7)
    week_parser.add_argument("--out", type=Path)
    args = parser.parse_args(argv)
    if args.mode == "weekly":
        return weekly(root=args.root, days=args.days, out=args.out)
    return batch(root=args.root, count=args.count, name=args.name,
                 shakedown=args.mode == "shakedown")


if __name__ == "__main__":
    sys.exit(main())
