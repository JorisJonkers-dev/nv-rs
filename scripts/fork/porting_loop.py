#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Drive one batch of the porting pipeline from the ledger's work queue.

The loop takes no hand-written work list. It reads the queue command of the ledger
(scripts/ledger), gives each family its own git worktree, runs a configured worker
there, runs that family's Diff cases through the build daemon's family lab, asks a
configured reviewer, and prepares one pull request per batch. A shakedown batch is
thrown away unmerged and commits what it showed to the rulebook. A weekly report
counts delivered verified functions per day from the queue and the committed
delivery log.

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
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import uuid
from pathlib import Path

# REPO is resolved from this script's location: a copy of the script outside the
# checkout would read the wrong queue, rulebook and delivery log. Run it in place.
FORK = Path(__file__).resolve().parent
REPO = FORK.parent.parent
LEDGER = FORK.parent / "ledger" / "Cargo.toml"
DELIVERIES = Path("docs") / "fork" / "porting-batches" / "delivered.jsonl"
BASE = "fork"  # every worktree starts at the protected branch, never the caller's HEAD
CLAIM_AGE = 3600  # seconds; override with NV_PORT_CLAIM_AGE
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
    """A family name as one safe file and branch component.

    A name the substitution changed gets a short hash, so `a/b` and `a_b` cannot
    share a claim file, a worktree or a branch.
    """
    safe = UNSAFE.sub("_", family).strip("_") or "family"
    if safe != family:
        safe += "-" + hashlib.sha1(family.encode("utf-8")).hexdigest()[:6]
    return safe


def paths(root, name, family=None):
    base = Path(root) / "target" / "porting-loop"
    if family is None:
        return base / name
    return base / "worktrees" / name / slug(family)


def claim_directory(root, directory=None):
    """One claim directory for every worktree of a clone.

    Exclusive file creation is the lock on Windows, Linux and macOS. Keeping the
    files in the common git directory makes worktrees of the same clone agree;
    a checkout with no git metadata (a test tree) falls back to its target dir.
    Separate clones of one repository share nothing: point every worker at one
    directory with NV_PORT_CLAIM_DIR (a path on a shared disk) to exclude them too.
    """
    if directory is not None:
        return Path(directory)
    git = Path(root) / ".git"
    common = None
    if git.is_dir():
        common = git
    elif git.is_file():
        try:
            line = git.read_text(encoding="utf-8").strip()
        except OSError:
            line = ""
        found = re.match(r"gitdir:\s*(.+)", line)
        if found:
            gitdir = Path(found.group(1).strip())
            if not gitdir.is_absolute():
                gitdir = (Path(root) / gitdir).resolve()
            marker = gitdir / "commondir"
            if marker.is_file():
                try:
                    common = (gitdir / marker.read_text(encoding="utf-8").strip()).resolve()
                except OSError:
                    common = None
            else:
                common = gitdir
    if common is None:
        return Path(root) / "target" / "porting-loop" / "claims"
    return common / "porting-claims"


def as_utc(value):
    if isinstance(value, datetime.date) and not isinstance(value, datetime.datetime):
        value = datetime.datetime.combine(value, datetime.time())
    return value if value.tzinfo else value.replace(tzinfo=datetime.timezone.utc)


def claim(root, family, owner, now=None, max_age=None, directory=None):
    """Take a family exclusively; reclaim a claim older than the configured age.

    Reclaiming runs under a second exclusive file, so two batches that both saw
    the same expired claim cannot each delete the other's fresh one.
    """
    directory = claim_directory(root, directory)
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / (slug(family) + ".claim")
    now = as_utc(now or datetime.datetime.now(datetime.timezone.utc))
    age = datetime.timedelta(seconds=CLAIM_AGE if max_age is None else max_age)
    for _ in range(2):
        try:
            handle = path.open("x", encoding="utf-8")
        except FileExistsError:
            taken = None
            try:
                fields = path.read_text(encoding="utf-8").strip().split("\t")
                if len(fields) >= 2:
                    taken = as_utc(datetime.datetime.fromisoformat(fields[1]))
            except (OSError, ValueError):
                taken = None
            if taken is not None and now - taken <= age:
                return None
            guard = path.with_name(path.name + ".reclaim")
            try:
                guard.open("x", encoding="utf-8").close()
            except OSError:
                return None  # another batch is reclaiming it, or the guard is held
            try:
                fields = path.read_text(encoding="utf-8").strip().split("\t")
                current = as_utc(datetime.datetime.fromisoformat(fields[1])) \
                    if len(fields) >= 2 else None
            except (OSError, ValueError):
                current = None
            try:
                if current is not None and now - current <= age:
                    return None  # it was renewed while this batch decided
                try:
                    path.unlink(missing_ok=True)
                except OSError:
                    # A Windows process still holding the file keeps the claim.
                    return None
            finally:
                guard.unlink(missing_ok=True)
            continue
        with handle:
            handle.write(f"{owner}\t{now.isoformat()}\t{os.getpid()}\n")
        return path
    return None


def claim_time(root, family, directory=None):
    """The timestamp an existing claim was taken, if it can be read."""
    path = claim_directory(root, directory) / (slug(family) + ".claim")
    try:
        fields = path.read_text(encoding="utf-8").strip().split("\t")
        if len(fields) >= 2:
            return as_utc(datetime.datetime.fromisoformat(fields[1]))
    except (OSError, ValueError):
        return None
    return None


def release(root, family, directory=None):
    (claim_directory(root, directory) / (slug(family) + ".claim")).unlink(missing_ok=True)


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


def family_section(text, family):
    """The output of the family's own test binary, `typed_<family>`, alone."""
    if not family:
        return text
    lines, keep, found = text.splitlines(), False, False
    out = []
    for line in lines:
        stripped = line.strip()
        if stripped.startswith(("Running ", "Doc-tests ")):
            keep = f"typed_{family}" in stripped and stripped.startswith("Running ")
            found = found or keep
            continue
        if keep:
            out.append(line)
    return "\n".join(out) if found else text


def diff_counts(text, family=None):
    """Pass and failure counts from a family lab run, for that family's binary only."""
    text = family_section(text, family)
    passed = failed = 0
    for pattern in (PASS, FAIL):
        for match in pattern.finditer(text):
            passed += int(match.group(1))
            failed += int(match.group(2))
    return {"passed": passed, "failed": failed}


def failure_classes(root, addresses):
    """The ledger's recorded classes for these functions, counted by kind.

    The column is taken from the evidence file's own header, so a change to the
    ledger's format cannot silently turn failure classes into backend numbers.
    """
    path = Path(root) / "scripts" / "ledger" / "function_evidence.tsv"
    if not path.is_file():
        return {}
    lines = path.read_text(encoding="utf-8").splitlines()
    data = [line.split("\t") for line in lines
            if line.strip() and not line.lstrip().startswith("#")]
    if not data:
        return {}
    try:
        column = data[0].index("failure_class")
    except ValueError:
        print("warning: function_evidence.tsv has no failure_class column; "
              "no failure classes are counted", file=sys.stderr)
        return {}
    counts = {}
    for fields in data[1:]:
        if len(fields) > column and fields[0] in addresses and fields[column]:
            kind = fields[column]
            counts[kind] = counts.get(kind, 0) + 1
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


def claim_age(env):
    try:
        seconds = int(env.get("NV_PORT_CLAIM_AGE", str(CLAIM_AGE)))
    except ValueError as exc:
        raise ValueError("NV_PORT_CLAIM_AGE must be a number of seconds") from exc
    if seconds <= 0:
        raise ValueError("NV_PORT_CLAIM_AGE must be a positive number of seconds")
    return seconds


def pr_body(name, report):
    """One pull request's evidence: totals, counts, classes and skipped families."""
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
    for title, key in (("Rejected families", "rejected"), ("Skipped families", "skipped")):
        if report.get(key):
            lines += ["", f"## {title}", ""]
            lines += [f"- {item}" for item in report[key]]
    if report.get("stale"):
        lines += ["", "- Claims reclaimed after expiry: " + ", ".join(report["stale"])]
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


def shakedown_section(name, report):
    """Rules written from this batch's evidence, not generic prose."""
    lines = [
        f"### Shakedown {name} ({report['status']})", "",
        f"- Functions offered: {report['functions']}",
        f"- Families accepted: {len(report['families'])}",
        f"- Diff cases passed: {sum(f['passed'] for f in report['families'])}",
        f"- Diff cases failed: {sum(f['failed'] for f in report['families'])}",
        "- Failure classes: " + (", ".join(
            f"{kind} {count}" for kind, count in sorted(report["classes"].items())
        ) or "none recorded"),
    ]
    for kind, count in sorted(report["classes"].items()):
        lines.append(f"- Rule: triage {kind} in later batches; this shakedown showed {count}.")
    failed = sum(f["failed"] for f in report["families"])
    if failed:
        lines.append(
            f"- Rule: a family with failing Diff cases is not accepted until the {failed} "
            "failures seen here are triaged."
        )
    if report.get("rejected"):
        lines.append(
            f"- Rule: a family needs a passing worker, lab and review before it is accepted; "
            f"{len(report['rejected'])} were rejected here."
        )
    lines.append("- Rule: a batch is accepted only with a passing review and an unchanged checker.")
    lines.append("")
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


def delivery_path(root):
    """The delivery log is a repository artefact, not a file under ignored target/."""
    return Path(root) / DELIVERIES


def record_delivery(root, date, name, address, family):
    path = delivery_path(root)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps({
            "date": date, "batch": name, "address": address, "family": family,
        }, sort_keys=True) + "\n")


def deliveries(root):
    path = delivery_path(root)
    if not path.is_file():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def record_verified_deliveries(root, name, date, families):
    """Record the addresses an accepted family turned verified in its own worktree.

    Rows that were already verified when the batch started are not this batch's
    work, so they are never dated by it.
    """
    known = {record["address"] for record in deliveries(root)}
    for family, addresses in families:
        for address in addresses:
            if address not in known:
                record_delivery(root, date.isoformat(), name, address, family)
                known.add(address)


def batch(root=REPO, count=20, name=None, shakedown=False, env=None,
          today=None, call=call):
    """Run one batch from the queue; return a process exit status."""
    root = Path(root)
    today = today or datetime.date.today()
    env = dict(os.environ if env is None else env)
    name = name or ("shakedown" if shakedown else today.isoformat())
    base = env.get("NV_PORT_BASE", "").strip() or BASE  # e.g. origin/fork when the local branch is stale
    try:
        worker = configured(env, "NV_PORT_WORKER")
        reviewer = configured(env, "NV_PORT_REVIEW")
        age = claim_age(env)
        if shakedown and not env.get("NV_PORT_CLOSE", "").strip():
            raise ValueError("NV_PORT_CLOSE must name a command to close a shakedown")
        close = configured(env, "NV_PORT_CLOSE") if shakedown else None
    except ValueError as exc:
        print(f"failed: {exc}", file=sys.stderr)
        return 2
    ok, why = private_integrity.check(root)
    if not ok:
        print(f"failed: {why}", file=sys.stderr)
        return 1
    try:
        all_rows = queue(root, env, call)
    except (RuntimeError, ValueError) as exc:
        print(f"failed: {exc}", file=sys.stderr)
        return 1
    verified = {r["address"] for r in all_rows if r["state"] == "reference-verified"}
    groups = {}
    for row in all_rows:
        if row["done"] == "no" and row["state"] != "retired":
            groups.setdefault(row["family"], []).append(row)
    # A batch takes whole families: the queue keeps siblings together on purpose.
    selected, offered = {}, 0
    for family, family_rows in groups.items():
        if offered >= count:
            break
        selected[family] = family_rows
        offered += len(family_rows)
    rows = [row for family_rows in selected.values() for row in family_rows]

    token = uuid.uuid4().hex[:8]
    batch_branch = f"porting/{slug(name)}-{token}"
    claim_dir = env.get("NV_PORT_CLAIM_DIR", "").strip() or None
    accepted, created, owned = [], [], []
    skipped, stale, rejected = [], [], []
    success = False

    def make_report(status):
        return {
            "status": status,
            "functions": len(rows),
            "families": [{k: f[k] for k in ("family", "passed", "failed", "review")}
                         for f in accepted],
            "classes": failure_classes(
                root, {a for f in accepted for a in f["addresses"]}),
            "checker": checker_hash(root),
            "skipped": list(skipped), "stale": list(stale),
            "rejected": list(rejected), "branch": batch_branch,
        }

    def fail(message):
        write_json(root, name, make_report("failed"))
        print(f"failed: {message}", file=sys.stderr)
        return 1

    try:
        for family, family_rows in selected.items():
            previous = claim_time(root, family, claim_dir)
            held = claim(root, family, name, max_age=age, directory=claim_dir)
            if held is None:
                skipped.append(family)
                print(f"skip {family}: another worker holds the claim", file=sys.stderr)
                continue
            owned.append(held)
            now = as_utc(datetime.datetime.now(datetime.timezone.utc))
            if previous is not None and (now - previous).total_seconds() > age:
                stale.append(family)
            worktree = paths(root, name, family)
            branch = f"porting/{slug(name)}-{token}-{slug(family)}"
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
            code, output = call(["git", "worktree", "add", "-b", branch, str(worktree), base],
                                cwd=root, env=env)
            if code:
                return fail(f"worktree add for {family}: {output.strip()}")
            created.append((worktree, branch))
            code, _ = call(worker, cwd=worktree, env=child,
                           log=logdir / (slug(family) + ".worker.log"))
            if code:
                rejected.append(f"{family}: worker exited {code}")
                continue
            # The checker is verified before anything of the worker's is run or reviewed.
            ok, why = private_integrity.check(worktree)
            if not ok:
                return fail(f"{family}: {why}")
            # family_lab.py sends its cargo arguments to the build daemon itself.
            lab_code, output = call(
                [sys.executable, str(worktree / "scripts" / "fork" / "family_lab.py"),
                 "run", family],
                cwd=worktree, env=child, log=logdir / (slug(family) + ".diff.log"),
            )
            counts = diff_counts(output, family)
            if lab_code or counts["failed"]:
                rejected.append(
                    f"{family}: Diff exited {lab_code} with {counts['failed']} failing cases"
                )
                continue
            review_code, _ = call(reviewer, cwd=worktree, env=child,
                                  log=logdir / (slug(family) + ".review.log"))
            review = "accepted" if review_code == 0 else "rejected"
            if review == "accepted":
                delivered = []
                try:
                    after = queue(worktree, env, call)
                except (RuntimeError, ValueError):
                    after = []
                delivered = [r["address"] for r in after
                             if r["state"] == "reference-verified"
                             and r["address"] not in verified
                             and r["address"] in {x["address"] for x in family_rows}]
                accepted.append({
                    "delivered": delivered,
                    "family": family, "branch": branch, "worktree": worktree,
                    "addresses": [r["address"] for r in family_rows],
                    "passed": counts["passed"], "failed": counts["failed"],
                    "lab_exit": lab_code, "review": review,
                })
            else:
                rejected.append(f"{family}: review exited {review_code}")

        report = make_report("closed unmerged" if shakedown else "open")
        write_json(root, name, report)

        if shakedown:
            rule_wt = paths(root, name) / "rulebook"
            rule_branch = f"porting/shakedown/{slug(name)}-{token}"
            code, output = call(["git", "worktree", "add", "-b", rule_branch,
                                 str(rule_wt), base], cwd=root, env=env)
            if code:
                return fail(f"shakedown worktree add: {output.strip()}")
            created.append((rule_wt, rule_branch))
            rulebook = rule_wt / "docs" / "fork" / "PORTING-RULEBOOK.md"
            update_rulebook(rulebook, shakedown_section(name, report))
            call(["git", "add", "--", str(rulebook.relative_to(rule_wt))],
                 cwd=rule_wt, env=env)
            code, output = call(["git", "commit", "-m",
                                 f"docs(fork): rules learned from shakedown {name}"],
                                cwd=rule_wt, env=env)
            if code:
                return fail(f"commit of the shakedown rulebook: {output.strip()}")
            report["rulebook_branch"] = rule_branch
            write_json(root, name, report)
            print(f"rulebook branch: {rule_branch}")
            if env.get("NV_PORT_PR", "").strip():
                code, output = call(["git", "push", "-u", "origin", rule_branch],
                                    cwd=root, env=env)
                if code:
                    return fail(f"push of {rule_branch}: {output.strip()}")
            code, output = call(close + [
                "--batch", name, "--reason", "shakedown closed unmerged; rulebook updated",
                "--branch", rule_branch,
            ], cwd=root, env=env)
            if code:
                return fail(f"shakedown close command exited {code}: {output.strip()}")
        elif accepted:
            batch_wt = paths(root, name) / "branch"
            code, output = call(["git", "worktree", "add", "-b", batch_branch,
                                 str(batch_wt), base], cwd=root, env=env)
            if code:
                return fail(f"batch worktree add: {output.strip()}")
            created.append((batch_wt, batch_branch))
            report_path = batch_wt / "docs" / "fork" / "PORTING-BATCHES" / f"{slug(name)}.md"
            report_path.parent.mkdir(parents=True, exist_ok=True)
            report_path.write_text(report_text(name, report), encoding="utf-8")
            call(["git", "add", "--", str(report_path.relative_to(batch_wt))],
                 cwd=batch_wt, env=env)
            code, output = call(["git", "commit", "-m", f"test(fork): porting batch {name} report"],
                                cwd=batch_wt, env=env)
            if code:
                return fail(f"commit of the batch report: {output.strip()}")
            for family in accepted:
                code, output = call(["git", "merge", "--no-ff", "-m",
                                     f"chore(fork): merge {family['family']} into batch {name}",
                                     family["branch"]], cwd=batch_wt, env=env)
                if code:
                    return fail(f"merge of {family['family']}: {output.strip()}")
            body = paths(root, name) / f"{slug(name)}.pr.md"
            body.parent.mkdir(parents=True, exist_ok=True)
            body.write_text(pr_body(name, report), encoding="utf-8")
            pr = env.get("NV_PORT_PR", "").strip()
            if pr:
                code, output = call(["git", "push", "-u", "origin", batch_branch],
                                    cwd=root, env=env)
                if code:
                    return fail(f"push of {batch_branch}: {output.strip()}")
                code, output = call(shlex.split(pr, posix=os.name != "nt") + [
                    "--title", f"feat(engine): porting batch {name}", "--body", str(body),
                    "--head", batch_branch,
                ], cwd=root, env=env)
                if code:
                    return fail(f"pull request command exited {code}: {output.strip()}")
            else:
                print(f"pull request body: {body}")
            record_verified_deliveries(
                root, name, today,
                [(f["family"], f["delivered"]) for f in accepted],
            )
            delivery = delivery_path(root)
            if delivery.is_file():
                copy = batch_wt / DELIVERIES
                copy.parent.mkdir(parents=True, exist_ok=True)
                copy.write_text(delivery.read_text(encoding="utf-8"), encoding="utf-8")
                call(["git", "add", "--", str(DELIVERIES)], cwd=batch_wt, env=env)
                code, output = call(["git", "commit", "-m",
                                     f"chore(fork): delivery log for batch {name}"],
                                    cwd=batch_wt, env=env)
                if code:
                    return fail(f"commit of the delivery log: {output.strip()}")
        success = True
        return 0
    finally:
        for worktree, _ in created:
            call(["git", "worktree", "remove", "--force", str(worktree)], cwd=root, env=env)
        if success:
            # A failed batch keeps every accepted branch: earlier work is not lost.
            for family in accepted:
                call(["git", "branch", "-D", family["branch"]], cwd=root, env=env)
        for held in owned:
            Path(held).unlink(missing_ok=True)


def weekly(root=REPO, days=7, env=None, today=None, out=None, call=call):
    """Delivered verified functions per day, from the queue and the committed log."""
    root = Path(root)
    today = today or datetime.date.today()
    env = dict(os.environ if env is None else env)
    try:
        rows = queue(root, env, call)
    except (RuntimeError, ValueError) as exc:
        print(f"failed: {exc}", file=sys.stderr)
        return 1
    verified = {r["address"] for r in rows if r["state"] == "reference-verified"}
    first = today - datetime.timedelta(days=days - 1)
    counts = {}
    records = deliveries(root)
    dropped = 0
    for record in records:
        address = record.get("address")
        if address not in verified:
            dropped += 1
            continue
        try:
            date = datetime.date.fromisoformat(record["date"])
        except (KeyError, ValueError):
            continue
        if first <= date <= today:
            counts[date.isoformat()] = counts.get(date.isoformat(), 0) + 1
    # A function first seen as verified now is credited to today. The count is
    # derived from the queue, so a clean checkout still gets the same table.
    known = {record.get("address") for record in records}
    for address in verified - known:
        key = today.isoformat()
        counts[key] = counts.get(key, 0) + 1
    lines = [
        "<!-- SPDX-License-Identifier: GPL-3.0-only -->",
        "# Weekly porting rate", "",
        "| date | delivered verified functions |",
        "| --- | --- |",
    ]
    lines += [f"| {date} | {counts[date]} |" for date in sorted(counts)]
    lines += ["", f"- Window: {first.isoformat()} to {today.isoformat()}",
              f"- Total: {sum(counts.values())}",
              f"- Delivery records without a queue row: {dropped}", ""]
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
