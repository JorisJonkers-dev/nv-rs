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
    python3 -I scripts/fork/porting_loop.py weekly [--days 7] [--out FILE] [--since DATE]
"""
import argparse
import concurrent.futures
import datetime
import fnmatch
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import threading
import time
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
CLAIM_KEY = "porting-"  # remote claim key prefix, followed by the family slug
ATTEMPTS = 3  # total worker attempts per family; override with NV_PORT_ATTEMPTS
FAILURE_LINES = 40  # error lines carried into the next attempt's prompt
START = "<!-- shakedown:start -->"
END = "<!-- shakedown:end -->"
PASS = re.compile(r"test result: ok\. (\d+) passed; (\d+) failed")
FAIL = re.compile(r"test result: FAILED\. (\d+) passed; (\d+) failed")
UNSAFE = re.compile(r"[^A-Za-z0-9_.-]+")
TEST_FAILED = re.compile(r"^\s*test\s+(.+?)\s+\.\.\.\s+FAILED\b", re.MULTILINE)
ERROR_LINE = re.compile(r"error\[|panicked at|\bFAILED\b", re.IGNORECASE)

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


def claim_config(env=None):
    """Remote claim settings, or None when no claim command and repository are set.

    NV_PORT_CLAIM_CMD may name the command; when it is absent `claim` is looked
    up on PATH. NV_PORT_CLAIM_REPO is an owner/name slug and NV_PORT_CLAIM_ISSUE
    the issue the claim belongs to. NV_PORT_CLAIM_LEASE optionally sets the
    lease passed with --lease-seconds. Without a repository (or a command) the
    local claim files are the only claims.
    """
    child = dict(os.environ if env is None else env)
    repo = child.get("NV_PORT_CLAIM_REPO", "").strip()
    if not repo:
        return None
    command = child.get("NV_PORT_CLAIM_CMD", "").strip()
    argv = shlex.split(command, posix=os.name != "nt") if command else []
    if not argv:
        found = shutil.which("claim", path=child.get("PATH"))
        argv = [found] if found else []
    if not argv:
        return None
    issue = child.get("NV_PORT_CLAIM_ISSUE", "").strip()
    if not issue.isdigit():
        raise ValueError("NV_PORT_CLAIM_ISSUE must be an issue number when remote claims are used")
    lease = child.get("NV_PORT_CLAIM_LEASE", "").strip()
    return {"argv": argv, "repo": repo, "issue": issue, "lease": lease}


class RemoteClaim:
    """A claim held by the claim command, with its local file as a backup."""

    def __init__(self, config, key, local=None):
        self.config, self.key, self.local = config, key, local


def claim_command(config, verb, key, root, env=None, runner=None):
    """One acquire/renew/release/list call to the configured claim command."""
    runner = runner or call
    argv = list(config["argv"]) + [verb, config["repo"], key]
    if verb in ("acquire", "release"):
        argv += ["--issue", config["issue"]]
    if verb == "acquire" and config.get("lease"):
        argv += ["--lease-seconds", config["lease"]]
    try:
        return runner(argv, cwd=root, env=env)
    except OSError as exc:
        # A claim service that cannot be started is a claim that was not taken.
        return 127, str(exc)


def claim(root, family, owner, now=None, max_age=None, directory=None, env=None,
          runner=None):
    """Take a family exclusively, remotely when configured and locally as well.

    The remote claim is the cross-device lock: a failed acquire skips the family
    just like a held local claim. The local file keeps the old single-clone
    behaviour and is the whole claim when no claim command is configured. Its
    signature and reclaim rules are unchanged.
    """
    child = dict(os.environ if env is None else env)
    config = claim_config(child)
    if config is None:
        return local_claim(root, family, owner, now=now, max_age=max_age,
                           directory=directory)
    key = CLAIM_KEY + slug(family)
    code, _ = claim_command(config, "acquire", key, root, env=child, runner=runner)
    if code:
        return None
    local = local_claim(root, family, owner, now=now, max_age=max_age,
                        directory=directory)
    if local is None:
        claim_command(config, "release", key, root, env=child, runner=runner)
        return None
    return RemoteClaim(config, key, local)


def renew_claim(held, root, env, runner=None):
    """Renew the remote lease before a worker attempt; local-only claims need it not."""
    if not isinstance(held, RemoteClaim):
        return True
    code, output = claim_command(held.config, "renew", held.key, root, env=env,
                                 runner=runner)
    if code:
        print(f"warning: claim renewal failed: {output.strip()}", file=sys.stderr)
    return code == 0


def release_claim(held, root, env, runner=None):
    """Release whatever `claim` took, at the end of the batch."""
    if isinstance(held, RemoteClaim):
        code, output = claim_command(held.config, "release", held.key, root,
                                     env=env, runner=runner)
        if code:
            print(f"warning: claim release failed: {output.strip()}", file=sys.stderr)
        held = held.local
    if held is not None:
        Path(held).unlink(missing_ok=True)


def local_claim(root, family, owner, now=None, max_age=None, directory=None):
    """Take a local family claim, reclaiming one older than the configured age.

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


def chunk_size(env):
    """Functions per chunk (NV_PORT_CHUNK, default 5; 0 keeps a family whole)."""
    raw = str(env.get("NV_PORT_CHUNK", "5")).strip()
    try:
        size = int(raw)
    except ValueError as exc:
        raise ValueError("NV_PORT_CHUNK must be a whole number of functions") from exc
    if size < 0:
        raise ValueError("NV_PORT_CHUNK must not be negative")
    return size


def chunk_limit(env):
    """Chunks of one family taken per batch (NV_PORT_CHUNKS, 0 = all of them)."""
    try:
        return max(0, int(str(env.get("NV_PORT_CHUNKS", "0")).strip()))
    except ValueError as exc:
        raise ValueError("NV_PORT_CHUNKS must be a whole number") from exc


def chunk_family(rows, size, limit=0):
    """A family's rows in chunks of at most `size`, in queue order; siblings stay adjacent."""
    if not size or len(rows) <= size:
        return [list(rows)]
    chunks = [list(rows[i:i + size]) for i in range(0, len(rows), size)]
    return chunks[:limit] if limit else chunks


def restore(worktree, commit, env, runner):
    """Drop everything a failed chunk left in the worktree."""
    runner(["git", "reset", "--hard", commit], cwd=worktree, env=env)
    runner(["git", "clean", "-fd"], cwd=worktree, env=env)


def allowed_attempts(env):
    """Total worker attempts a family gets (NV_PORT_ATTEMPTS, default 3)."""
    raw = str(env.get("NV_PORT_ATTEMPTS", str(ATTEMPTS))).strip()
    try:
        count = int(raw)
    except ValueError as exc:
        raise ValueError("NV_PORT_ATTEMPTS must be a whole number of attempts") from exc
    if count < 1:
        raise ValueError("NV_PORT_ATTEMPTS must be at least one")
    return count


def failing_cases(text, family=None):
    """Names from the lab's `test <name> ... FAILED` lines of this family."""
    return [match.group(1).strip()
            for match in TEST_FAILED.finditer(family_section(text, family))]


def failure_excerpt(texts, family=None, limit=FAILURE_LINES):
    """The first `limit` error lines from the previous attempt's outputs.

    Diff failures are the useful lines: failed case names, `error[...]` and
    `panicked at`. When neither output has such lines, the first lines of the
    raw output are still carried forward rather than losing the evidence.
    """
    error_lines = []
    raw = []
    for text in texts:
        section = family_section(text, family)
        for line in section.splitlines():
            raw.append(line.rstrip())
            if ERROR_LINE.search(line):
                error_lines.append(line.rstrip())
    return "\n".join((error_lines or [line for line in raw if line.strip()])[:limit])


def write_failure(path, attempt, detail, cases, excerpt):
    """The failure file handed to the worker on the next attempt."""
    lines = [f"Attempt {attempt} failed: {detail}",
             "Failing cases: " + (", ".join(cases) if cases else "none reported")]
    if excerpt:
        lines += ["", excerpt]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


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


def model_slots(env):
    """Families run at once (NV_PORT_MODEL_SLOTS, default 20)."""
    try:
        return max(1, int(env.get("NV_PORT_MODEL_SLOTS", "20")))
    except ValueError:
        return 20


WAITED = re.compile(r"^build daemon: waited (\d+(?:\.\d+)?)s in the queue$", re.MULTILINE)


def build_wait(output):
    """Seconds the build daemon's queue held the cargo jobs behind `output` (a lab run's text).

    The daemon prints one `build daemon: waited Ns in the queue` line per job it starts. Output
    without such a line (an older daemon, or no cargo job) counts as no wait.
    """
    return sum(float(seconds) for seconds in WAITED.findall(output or ""))


def metrics_summary(path, batch):
    """Model-side counters of one batch from the worker metrics file (NV_PORT_METRICS).

    Returns seconds spent waiting for a model slot, rate-limit hits per provider (the part of the
    model name before `/`) and the cost in dollars of the batch's runs. Rows of other batches and
    unreadable lines are ignored.
    """
    summary = {"model_wait_seconds": 0.0, "rate_limit_hits": {}, "cost_usd": 0.0}
    try:
        lines = Path(path).read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeError):
        return summary
    for line in lines:
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if not isinstance(row, dict) or row.get("batch") != batch:
            continue
        for key, field in (("model_wait_seconds", "wait_seconds"), ("cost_usd", "cost")):
            value = row.get(field)
            if isinstance(value, (int, float)) and not isinstance(value, bool):
                summary[key] += value
        if row.get("outcome") == "limited":
            provider = str(row.get("model") or "unknown").split("/", 1)[0] or "unknown"
            hits = summary["rate_limit_hits"]
            hits[provider] = hits.get(provider, 0) + 1
    return summary


def rate_lines(rate):
    """Markdown section summarising the measured rate of one batch."""
    wall = rate["wall_seconds"]
    per_hour = rate["delivered"] / (wall / 3600) if wall > 0 else 0.0
    lines = [
        "",
        "## Rate",
        "",
        f"- Model runs: {rate['model_runs']}",
        f"- Lab seconds (build and Diff together): {rate['lab_seconds']:.1f}",
        f"- Wall seconds: {wall:.1f}",
        f"- Verified functions per hour: {per_hour:.2f}",
    ]
    if "attempted" in rate:
        hits = rate.get("rate_limit_hits") or {}
        cost = rate.get("cost_per_accepted")
        lines += [
            f"- Families attempted, accepted: {rate['attempted']}, {rate['accepted']}",
            f"- Retries: {rate['retries']}",
            f"- Waiting for the build daemon (seconds): {rate['build_wait_seconds']:.1f}",
            f"- Waiting for a model (seconds): {rate['model_wait_seconds']:.1f}",
            "- Rate-limit hits: " + (", ".join(f"{k} {v}" for k, v in sorted(hits.items())) or "none"),
            "- Cost per accepted family (USD): "
            + ("n/a" if cost is None else f"{cost:.4f}"),
        ]
    return lines


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
    attempts = report.get("attempts") or []
    grouped = {}
    for attempt in attempts:
        grouped.setdefault(attempt["family"], []).append(attempt)
    lines += ["", "## Attempts", ""]
    if grouped:
        for family, entries in grouped.items():
            lines.append(f"- {family}: {len(entries)} attempts")
            for entry in entries:
                lines.append(
                    f"  - attempt {entry['attempt']}: {entry['failure_class']} "
                    f"({entry['detail']})"
                )
    else:
        # Reports written before attempts were recorded still show a count.
        for family in report["families"]:
            lines.append(f"- {family['family']}: {family.get('attempts', 1)} attempts")
        if not report["families"]:
            lines.append("- none recorded")
    lines += ["", "## Failure classes", ""]
    lines += [f"- {kind}: {count}" for kind, count in sorted(report["classes"].items())]
    if not report["classes"]:
        lines.append("- none recorded")
    if report.get("rate") is not None:
        lines += rate_lines(report["rate"])
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
        attempts = allowed_attempts(env)
        claim_config(env)  # fail early if a remote claim is configured without an issue
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
    excluded = [x.strip() for x in env.get("NV_PORT_EXCLUDE", "").split(",") if x.strip()]
    for row in all_rows:
        if any(fnmatch.fnmatchcase(row["family"], pattern) for pattern in excluded):
            continue
        if row["done"] == "no" and row["state"] != "retired":
            groups.setdefault(row["family"], []).append(row)
    # A batch takes whole families: the queue keeps siblings together on purpose.
    selected, offered = {}, 0
    try:
        most_families = int(env.get("NV_PORT_FAMILIES", "0"))
    except ValueError:
        most_families = 0
    busy = 0
    for family, family_rows in groups.items():
        if offered >= count or (most_families > 0 and len(selected) - busy >= most_families):
            break
        selected[family] = family_rows
        taken = claim_time(root, family, env.get("NV_PORT_CLAIM_DIR", "").strip() or None)
        if taken is not None and (as_utc(datetime.datetime.now(datetime.timezone.utc))
                                  - taken).total_seconds() <= age:
            # Held by another batch right now: it is reported as skipped but takes no share of this one.
            busy += 1
            continue
        offered += len(family_rows)
    rows = [row for family_rows in selected.values() for row in family_rows]

    if not env.get("NV_PORT_METRICS", "").strip():
        # Without a shared file the workers write one per worktree and the counters see nothing.
        own_metrics = paths(root, name) / "metrics.jsonl"
        own_metrics.unlink(missing_ok=True)
        env["NV_PORT_METRICS"] = str(own_metrics)

    token = uuid.uuid4().hex[:8]
    batch_branch = f"porting/{slug(name)}-{token}"
    claim_dir = env.get("NV_PORT_CLAIM_DIR", "").strip() or None
    accepted, created, owned = [], [], []
    skipped, stale, rejected = [], [], []
    attempt_log, attempt_counts = [], {}
    success = False
    report_lock = threading.Lock()
    abort = threading.Event()  # a failure that ends the batch stops the families not yet started
    tally = {"runs": 0, "lab": 0.0, "attempted": 0, "retries": 0, "build_wait": 0.0}
    started_at = time.time()

    def count_run():
        with report_lock:
            tally["runs"] += 1

    def count_lab(seconds):
        with report_lock:
            tally["lab"] += seconds

    def count(key, amount=1):
        with report_lock:
            tally[key] += amount

    def rate_report():
        model_side = metrics_summary(env["NV_PORT_METRICS"], name)
        accepted_count = len(accepted)
        return {
            "model_runs": tally["runs"], "lab_seconds": tally["lab"],
            "wall_seconds": time.time() - started_at,
            "delivered": sum(len(f["delivered"]) for f in accepted),
            "attempted": tally["attempted"], "accepted": accepted_count,
            "retries": tally["retries"], "build_wait_seconds": tally["build_wait"],
            "model_wait_seconds": model_side["model_wait_seconds"],
            "rate_limit_hits": model_side["rate_limit_hits"],
            "cost_usd": model_side["cost_usd"],
            "cost_per_accepted": (model_side["cost_usd"] / accepted_count
                                  if accepted_count else None),
        }

    def make_report(status):
        families = []
        for family in accepted:
            entry = {k: family[k] for k in ("family", "passed", "failed", "review")}
            entry.update({
                "attempts": attempt_counts.get(family["family"], 1),
                "attempt_classes": [a["failure_class"] for a in attempt_log
                                    if a["family"] == family["family"]],
            })
            families.append(entry)
        return {
            "status": status,
            "functions": len(rows),
            "families": families,
            "attempts": [dict(a) for a in attempt_log],
            "attempt_counts": dict(attempt_counts),
            "classes": failure_classes(
                root, {a for f in accepted for a in f["addresses"]}),
            "checker": checker_hash(root),
            "skipped": list(skipped), "stale": list(stale),
            "rejected": list(rejected), "branch": batch_branch,
            "rate": rate_report(),
        }

    def fail(message):
        abort.set()
        with report_lock:
            write_json(root, name, make_report("failed"))
        print(f"failed: {message}", file=sys.stderr)
        return 1

    try:
        def run_family(family, family_rows):
            if abort.is_set():
                return None
            attempt_counts.setdefault(family, 0)
            count("attempted")
            previous = claim_time(root, family, claim_dir)
            held = claim(root, family, name, max_age=age, directory=claim_dir, env=env)
            if held is None:
                skipped.append(family)
                print(f"skip {family}: another worker holds the claim", file=sys.stderr)
                return None
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
            done_rows, done_delivered, done_passed, done_failed, done_lab = [], [], 0, 0, 0
            for chunk_rows in chunk_family(family_rows, chunk_size(env), chunk_limit(env)):
                queue_file.write_text(
                    "# rank\taddress\tstate\tdone\tfamily\tname\n"
                    + "".join("\t".join(str(r[k]) for k in
                             ("rank", "address", "state", "done", "family", "name")) + "\n"
                             for r in chunk_rows),
                    encoding="utf-8",
                )
                child = family_env(env, chunk_rows, name, worktree, queue_file, shakedown)
                code, output = call(["git", "rev-parse", "HEAD"], cwd=worktree, env=env)
                chunk_base = output.strip() if not code else base
                failure_file = paths(root, name) / (slug(family) + ".failure.txt")
                failure_file.unlink(missing_ok=True)
                attempt_counts[family] = 0

                last_attempt = {}

                def record_attempt(number, kind, detail, passed=0, failed=0, lab_exit=None):
                    entry = {
                        "family": family, "attempt": number, "failure_class": kind,
                        "detail": detail, "passed": passed, "failed": failed,
                        "lab_exit": lab_exit,
                    }
                    last_attempt["entry"] = entry
                    attempt_log.append(entry)

                finished = None
                for attempt in range(1, attempts + 1):
                    if attempt > 1:
                        count("retries")
                        # A retry starts from the base ref: a failed worker's edits
                        # and partially typed files must not leak into its successor.
                        code, output = call(["git", "reset", "--hard", chunk_base],
                                            cwd=worktree, env=env)
                        if code:
                            return fail(f"reset of {family}: {output.strip()}")
                        code, output = call(["git", "clean", "-fd"], cwd=worktree, env=env)
                        if code:
                            return fail(f"clean of {family}: {output.strip()}")
                    if not renew_claim(held, root, env):
                        skipped.append(family)
                        print(f"skip {family}: its claim could not be renewed", file=sys.stderr)
                        break
                    attempt_counts[family] = attempt
                    attempt_env = dict(child)
                    attempt_env["NV_PORT_ATTEMPT"] = str(attempt)
                    if attempt > 1:
                        attempt_env["NV_PORT_FAILURE_FILE"] = str(failure_file)
                    else:
                        attempt_env.pop("NV_PORT_FAILURE_FILE", None)
                    count_run()
                    worker_code, worker_output = call(
                        worker, cwd=worktree, env=attempt_env,
                        log=logdir / (slug(family) + ".worker.log"),
                    )
                    if worker_code:
                        detail = f"worker exited {worker_code}"
                        record_attempt(attempt, "worker", detail)
                        if attempt < attempts:
                            write_failure(failure_file, attempt, detail, [],
                                          failure_excerpt([worker_output]))
                            continue
                        rejected.append(f"{family}: {detail}")
                        break
                    # The checker is verified before anything of the worker's is run or reviewed.
                    ok, why = private_integrity.check(worktree)
                    if not ok:
                        return fail(f"{family}: {why}")
                    # family_lab.py sends its cargo arguments to the build daemon itself.
                    lab_started = time.time()
                    attempt_env["NV_BUILD_LABEL"] = f"porting-{slug(name)}-{slug(family)}"
                    lab_code, output = call(
                        [sys.executable, str(worktree / "scripts" / "fork" / "family_lab.py"),
                         "run", family],
                        cwd=worktree, env=attempt_env, log=logdir / (slug(family) + ".diff.log"),
                    )
                    count_lab(time.time() - lab_started)
                    count("build_wait", build_wait(output))
                    counts = diff_counts(output, family)
                    if lab_code or counts["failed"]:
                        kind = "lab" if lab_code else "diff"
                        detail = f"Diff exited {lab_code} with {counts['failed']} failing cases"
                        record_attempt(attempt, kind, detail, counts["passed"],
                                       counts["failed"], lab_code)
                        if attempt < attempts:
                            write_failure(failure_file, attempt, detail,
                                          failing_cases(output, family),
                                          failure_excerpt([worker_output, output], family))
                            continue
                        rejected.append(f"{family}: {detail}")
                        break
                    record_attempt(attempt, "none", "passed", counts["passed"],
                                   counts["failed"], lab_code)
                    finished = (attempt_env, counts, lab_code)
                    break
                if finished is None:
                    restore(worktree, chunk_base, env, call)
                    break
                attempt_env, counts, lab_code = finished
                count_run()
                review_code, _ = call(reviewer, cwd=worktree, env=attempt_env,
                                      log=logdir / (slug(family) + ".review.log"))
                review = "accepted" if review_code == 0 else "rejected"
                if review == "rejected":
                    last_attempt["entry"].update({
                        "failure_class": "review", "detail": f"review exited {review_code}",
                    })
                if review == "accepted":
                    delivered = []
                    try:
                        after = queue(worktree, env, call)
                    except (RuntimeError, ValueError):
                        after = []
                    delivered = [r["address"] for r in after
                                 if r["state"] == "reference-verified"
                                 and r["address"] not in verified
                                 and r["address"] in {x["address"] for x in chunk_rows}]
                    done_rows.extend(chunk_rows)
                    done_delivered.extend(delivered)
                    done_passed += counts["passed"]
                    done_failed += counts["failed"]
                    done_lab = lab_code
                else:
                    rejected.append(f"{family}: review exited {review_code}")
                    restore(worktree, chunk_base, env, call)
                    break
            if done_rows:
                accepted.append({
                    "delivered": done_delivered,
                    "family": family, "branch": branch, "worktree": worktree,
                    "addresses": [r["address"] for r in done_rows],
                    "passed": done_passed, "failed": done_failed,
                    "lab_exit": done_lab, "review": "accepted",
                })
            return None

        order = list(selected)
        with concurrent.futures.ThreadPoolExecutor(max_workers=model_slots(env)) as pool:
            results = list(pool.map(lambda item: run_family(*item), selected.items()))
        failed_code = next((code for code in results if code), None)
        if failed_code:
            return failed_code
        accepted.sort(key=lambda f: order.index(f["family"]))

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
            release_claim(held, root, env)


def weekly(root=REPO, days=7, env=None, today=None, out=None, call=call, since=None):
    """Delivered verified functions per day, from the queue and the committed log.

    With `since`, also compare its rate with the same number of days before it.
    """
    root = Path(root)
    today = today or datetime.date.today()
    if isinstance(since, str):
        try:
            since = datetime.date.fromisoformat(since)
        except ValueError:
            print("failed: --since must be an ISO date (YYYY-MM-DD)", file=sys.stderr)
            return 1
    if since is not None and since > today:
        print("failed: --since must not be after today", file=sys.stderr)
        return 1
    env = dict(os.environ if env is None else env)
    try:
        rows = queue(root, env, call)
    except (RuntimeError, ValueError) as exc:
        print(f"failed: {exc}", file=sys.stderr)
        return 1
    verified = {r["address"] for r in rows if r["state"] == "reference-verified"}
    first = today - datetime.timedelta(days=days - 1)
    counts = {}  # every log date, so a before/after window need not be in the table's
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
        counts[date] = counts.get(date, 0) + 1
    # A function first seen as verified now is credited to today. The count is
    # derived from the queue, so a clean checkout still gets the same table.
    known = {record.get("address") for record in records}
    for address in verified - known:
        counts[today] = counts.get(today, 0) + 1
    visible = {date: n for date, n in counts.items() if first <= date <= today}
    total = sum(visible.values())
    lines = [
        "<!-- SPDX-License-Identifier: GPL-3.0-only -->",
        "# Weekly porting rate", "",
        "| date | delivered verified functions |",
        "| --- | --- |",
    ]
    lines += [f"| {date.isoformat()} | {visible[date]} |" for date in sorted(visible)]
    lines += ["", f"- Window: {first.isoformat()} to {today.isoformat()}",
              f"- Total: {total}",
              f"- Verified functions per hour: "
              f"{total / (days * 24) if days > 0 else 0.0:.2f}",
              f"- Delivery records without a queue row: {dropped}"]
    if since is not None:
        span = (today - since).days + 1

        def total_between(start, end):
            return sum(n for date, n in counts.items() if start <= date <= end)

        before = total_between(since - datetime.timedelta(days=span),
                               since - datetime.timedelta(days=1))
        after = total_between(since, today)
        lines += [
            "", f"## Rate before and after {since.isoformat()}", "",
            f"- Before: {before} verified functions in {span} days "
            f"({before / (span * 24):.2f} per hour)",
            f"- After: {after} verified functions in {span} days "
            f"({after / (span * 24):.2f} per hour)",
        ]
    lines.append("")
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
    week_parser.add_argument("--since", type=datetime.date.fromisoformat, default=None,
                             help="ISO date to compare the delivery rate before and after")
    args = parser.parse_args(argv)
    if args.mode == "weekly":
        return weekly(root=args.root, days=args.days, out=args.out, since=args.since)
    return batch(root=args.root, count=args.count, name=args.name,
                 shakedown=args.mode == "shakedown")


if __name__ == "__main__":
    sys.exit(main())
