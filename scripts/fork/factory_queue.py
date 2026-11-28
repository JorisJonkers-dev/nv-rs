#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The factory's queue of attempts: implementers submit, one verifier takes the next and sends failures back.

    python3 -I scripts/fork/factory_queue.py submit FAMILY WORKTREE [--queue DIR]
    python3 -I scripts/fork/factory_queue.py verify [--queue DIR] [--hidden DIR] [--base REF]
    python3 -I scripts/fork/factory_queue.py list [--queue DIR]
    python3 -I scripts/fork/factory_queue.py prompt FAMILY [--feedback FILE]

An implementer writes code in its own worktree and checks it with `port_check.py --static` only: it never runs
cargo, because compiling belongs to the verifier. It submits the worktree. `verify` takes the oldest queued attempt,
runs the fast gate (`port_check.py`, from the attempt's own checkout, whose checker files the gate itself keeps
unchanged), then, when the gate passes, the hidden differential cases and the planted-bug control. A failure goes
back to the same attempt as one line per rule, and the implementer resubmits; after MAX_RETRIES failed retries the
attempt is rejected. A gate that gives no readable verdict is the verifier's fault, not the implementer's: the
attempt is queued again without using a retry, up to MAX_INFRA times.

The queue is a directory of one JSON file per attempt, written atomically, so it survives a reboot. A claim is a
file made with O_EXCL; a claim older than the lease (a killed verifier) is taken over.

The planted-bug control copies the attempt's typed sources, mutates one operator in the copy and runs the family's
differential cases on it: they must fail. If they still pass, the cases do not see that operator and the attempt is
not accepted. `prompt` prints what an implementer is given: the rules, a frozen source-only excerpt of the Diff
harness API (with its hash), the smallest typed suite as a worked example and the accepted `typed_call!` forms.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
import uuid
from collections.abc import Callable
from pathlib import Path
from typing import Any, cast

FORK = Path(__file__).resolve().parent
REPO = FORK.parent.parent
MAX_RETRIES = 3
MAX_INFRA = 3
LEASE_SECONDS = 3600
FEEDBACK_LINES = 20
FEEDBACK_WIDTH = 240
STATES = ("queued", "verifying", "retry", "accepted", "rejected")
FAMILY_NAME = re.compile(r"^[A-Za-z0-9_]+$")
DIFF_API = Path("crates") / "engine-core" / "src" / "typed" / "diff.rs"
MACRO_DOC = Path("crates") / "engine-core" / "src" / "typed" / "mod.rs"
API_LINE = re.compile(r"^\s*pub (?:fn|struct|enum)\b.*")
STRING = re.compile(r'"(?:[^"\\]|\\.)*"')
MUTATIONS = (("wrapping_add(", "wrapping_sub("), ("wrapping_mul(", "wrapping_add("), (" == ", " != "),
             (" < ", " >= "), (" > ", " <= "), (" + ", " - "), (" & ", " | "), (" << ", " >> "))

Runner = Callable[..., tuple[int, str]]
Verdict = dict[str, Any]


def run(argv: list[str], cwd: str | Path = ".") -> tuple[int, str]:
    """(exit status, combined output); a missing program is status 127."""
    try:
        done = subprocess.run([str(a) for a in argv], cwd=str(cwd), text=True, errors="replace",
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    except OSError as exc:
        return 127, f"{argv[0]}: {exc}"
    return done.returncode, done.stdout


class Queue:
    """A directory of attempts, one JSON file each."""

    def __init__(self, directory: str | Path) -> None:
        self.directory = Path(directory)
        self.directory.mkdir(parents=True, exist_ok=True)

    def path(self, attempt_id: str) -> Path:
        return self.directory / f"{attempt_id}.json"

    def save(self, attempt: dict[str, Any]) -> None:
        temp = self.directory / f".{attempt['id']}.{uuid.uuid4().hex}.tmp"
        temp.write_text(json.dumps(attempt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        os.replace(temp, self.path(attempt["id"]))

    def load(self, attempt_id: str) -> dict[str, Any]:
        data: dict[str, Any] = json.loads(self.path(attempt_id).read_text(encoding="utf-8"))
        return data

    def all(self) -> list[dict[str, Any]]:
        found: list[dict[str, Any]] = []
        for path in sorted(self.directory.glob("*.json")):
            try:
                found.append(json.loads(path.read_text(encoding="utf-8")))
            except (OSError, ValueError):
                continue  # an unreadable file is not an attempt
        return sorted(found, key=lambda a: (a["submitted"], a["id"]))

    def submit(self, family: str, worktree: str | Path, now: float | None = None) -> dict[str, Any]:
        """A new attempt, or the same one again after feedback (a family with an attempt waiting for a retry)."""
        if not FAMILY_NAME.match(family):
            raise ValueError(f"not a family name: {family!r}")
        stamp = time.time() if now is None else now
        for attempt in self.all():
            if attempt["family"] == family and attempt["state"] == "retry":
                attempt.update(state="queued", worktree=str(worktree), submitted=stamp)
                self.save(attempt)
                return attempt
            if attempt["family"] == family and attempt["state"] in ("queued", "verifying"):
                raise ValueError(f"{family} is already in the queue")
        fresh: dict[str, Any] = {"id": f"{family}-{uuid.uuid4().hex[:8]}", "family": family, "worktree": str(worktree),
                   "state": "queued", "retries": 0, "infra": 0, "feedback": [], "submitted": stamp, "history": []}
        self.save(fresh)
        return fresh

    def claim_next(self, now: float | None = None, lease: float = LEASE_SECONDS) -> dict[str, Any] | None:
        """The oldest queued attempt, now `verifying`; a verifying one whose claim outlived the lease is taken over."""
        stamp = time.time() if now is None else now
        for attempt in self.all():
            stale = attempt["state"] == "verifying" and stamp - attempt.get("claimed", 0) > lease
            if attempt["state"] != "queued" and not stale:
                continue
            claim = self.directory / f"{attempt['id']}.claim"
            if stale:
                claim.unlink(missing_ok=True)
            try:
                os.close(os.open(claim, os.O_CREAT | os.O_EXCL | os.O_WRONLY))
            except FileExistsError:
                continue  # another verifier has it
            attempt.update(state="verifying", claimed=stamp)
            self.save(attempt)
            return attempt
        return None

    def release(self, attempt_id: str) -> None:
        (self.directory / f"{attempt_id}.claim").unlink(missing_ok=True)


def feedback_lines(verdict: Verdict) -> list[str]:
    """One line per failure, `rule file:line message -> fix`, short enough to paste into a prompt."""
    lines: list[str] = []
    for failure in verdict.get("failures", []):
        text = (f"{failure.get('rule', '?')} {failure.get('file', '?')}:{failure.get('line', 1)}: "
                f"{failure.get('message', '')} -> fix: {failure.get('fix', '')}")
        lines.append(" ".join(text.split())[:FEEDBACK_WIDTH])
    return lines[:FEEDBACK_LINES]


def read_verdict(output: str) -> Verdict | None:
    """The gate's JSON verdict from its output; None when there is none or it is not a verdict."""
    start = output.find("{")
    try:
        data: Any = json.JSONDecoder().raw_decode(output[start:])[0] if start >= 0 else None
    except ValueError:
        return None
    if not isinstance(data, dict):
        return None
    verdict = cast(Verdict, data)
    if not isinstance(verdict.get("pass"), bool) or not isinstance(verdict.get("failures"), list):
        return None
    return verdict


def mutate_sources(worktree: Path, scratch: Path, only: list[str] | None = None) -> str | None:
    """Copy one of the attempt's typed sources to `scratch`, mutate one operator in the copy; the mutation, or None.

    `only` limits the candidates to those paths (relative to the worktree): the files the attempt changed.
    """
    for source in sorted((worktree / "crates" / "engine-typed" / "src").rglob("*.rs")):
        if only is not None and source.relative_to(worktree).as_posix() not in only:
            continue
        text = source.read_text(encoding="utf-8", errors="replace")
        offset = 0
        for line in text.split("#[cfg(test)]", 1)[0].splitlines(keepends=True):
            masked = STRING.sub(lambda found: "_" * len(found.group(0)), line)  # same length: positions stay valid
            code = masked.split("//", 1)[0]  # an operator in a comment or a string is no operator
            for old, new in MUTATIONS:
                at = code.find(old)
                if at >= 0:
                    target = scratch / source.relative_to(worktree)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_text(text[:offset + at] + new + text[offset + at + len(old):], encoding="utf-8")
                    return f"{source.relative_to(worktree).as_posix()}: `{old.strip()}` -> `{new.strip()}`"
            offset += len(line)
    return None


def planted_bug_control(worktree: Path, family: str, runner: Runner, base: str = "origin/fork") -> tuple[str, str]:
    """('failed'|'survived'|'skipped', detail): the family's cases must fail with one operator of the attempt changed."""
    scratch = Path(worktree) / "target" / "planted-bug"
    shutil.rmtree(scratch, ignore_errors=True)
    code, listing = runner(["git", "diff", "--name-only", f"{base}...HEAD"], cwd=worktree)
    mutation = mutate_sources(Path(worktree), scratch, None if code else listing.split("\n"))
    if mutation is None:
        return "skipped", "no operator to change in the typed sources"
    originals: dict[Path, str] = {}
    try:
        for copy in scratch.rglob("*.rs"):
            real = Path(worktree) / copy.relative_to(scratch)
            originals[real] = real.read_text(encoding="utf-8")
            real.write_text(copy.read_text(encoding="utf-8"), encoding="utf-8")
        code, output = runner([sys.executable, "scripts/fork/family_lab.py", "run", family], cwd=worktree)
    finally:
        for real, text in originals.items():
            real.write_text(text, encoding="utf-8")
        shutil.rmtree(scratch, ignore_errors=True)
    if code and re.search(r"could not compile|error\[E[0-9]+\]", output):
        return "skipped", f"{mutation} did not build, so it says nothing about the cases"
    return ("failed" if code else "survived"), mutation


def run_hidden(worktree: Path, family: str, hidden: Path | None, runner: Runner) -> tuple[bool, str]:
    """Run the hidden differential cases of the family, kept outside what an implementer can see."""
    source = hidden / f"typed_{family}.rs" if hidden else None
    if source is None or not source.is_file():
        return True, "no hidden cases for this family"
    name = f"{family}_hidden"
    target = Path(worktree) / "crates" / "engine" / "tests" / f"typed_{name}.rs"
    shutil.copyfile(source, target)
    try:
        code, output = runner([sys.executable, "scripts/fork/family_lab.py", "run", name], cwd=worktree)
    finally:
        target.unlink(missing_ok=True)
    return code == 0, (output.strip().splitlines() or [""])[-1][:FEEDBACK_WIDTH]


def verify_next(queue: Queue, runner: Runner = run, hidden: Path | None = None, base: str = "origin/fork",
                control: bool = True, now: float | None = None) -> dict[str, Any] | None:
    """Verify the next attempt; returns its updated record, or None when the queue has nothing to verify."""
    attempt = queue.claim_next(now)
    if attempt is None:
        return None
    worktree = Path(attempt["worktree"])
    try:
        code, output = runner([sys.executable, "scripts/fork/port_check.py", attempt["family"], "--base", base],
                              cwd=worktree)
        verdict = read_verdict(output)
        failures: list[str] = []
        if verdict is None:
            attempt["infra"] += 1
            attempt["state"] = "rejected" if attempt["infra"] >= MAX_INFRA else "queued"
            attempt["history"].append({"event": "no-verdict", "exit": code, "tail": output.strip()[-200:]})
            queue.save(attempt)
            return attempt
        failures = feedback_lines(verdict)
        if not verdict["pass"] and not failures:
            failures = ["gate: the gate failed and named no rule -> fix: run port_check.py and read its output"]
        if verdict["pass"]:
            ok, detail = run_hidden(worktree, attempt["family"], hidden, runner)
            if not ok:
                failures = [f"hidden-cases {attempt['family']}: a hidden differential case fails: {detail}"]
        if verdict["pass"] and not failures and control:
            outcome, detail = planted_bug_control(worktree, attempt["family"], runner, base)
            if outcome == "survived":
                failures = [(f"planted-bug {detail}: the family's cases still pass with this operator changed "
                             "-> fix: add cases that tell the two apart")]
            attempt["history"].append({"event": "control", "outcome": outcome, "detail": detail})
        if not failures:
            attempt["state"] = "accepted"
        else:
            attempt["feedback"] = failures
            attempt["retries"] += 1
            attempt["state"] = "rejected" if attempt["retries"] > MAX_RETRIES else "retry"
        attempt["history"].append({"event": "verdict", "pass": not failures, "failures": len(failures)})
        queue.save(attempt)
        return attempt
    finally:
        queue.release(attempt["id"])


def api_excerpt(root: Path) -> str:
    """The Diff harness's public items, source only, with the hash of the file they came from."""
    text = (Path(root) / DIFF_API).read_text(encoding="utf-8")
    lines = [m.group(0).strip() for line in text.splitlines() if (m := API_LINE.match(line))]
    return f"// diff.rs sha256 {hashlib.sha256(text.encode()).hexdigest()[:16]}\n" + "\n".join(lines)


def worked_example(root: Path, limit: int = 140) -> str:
    """The smallest typed suite, as the pattern to copy."""
    suites = sorted((Path(root) / "crates" / "engine" / "tests").glob("typed_*.rs"), key=lambda p: (p.stat().st_size, p.name))
    if not suites:
        return ""
    return f"// {suites[0].name}\n" + "\n".join(suites[0].read_text(encoding="utf-8").splitlines()[:limit])


def typed_call_forms(root: Path) -> str:
    """The doc comment of the `typed_call!` macro: the accepted forms."""
    lines = (Path(root) / MACRO_DOC).read_text(encoding="utf-8").splitlines()
    at = next((i for i, text in enumerate(lines) if text.startswith("macro_rules! typed_call")), len(lines))
    doc: list[str] = []
    for text in reversed(lines[:at]):
        if not text.startswith("///") and not text.startswith("#["):
            break
        if text.startswith("///"):
            doc.append(text[3:].strip())
    return "\n".join(reversed(doc))


def implementer_prompt(root: Path, family: str, feedback: list[str] | None = None) -> str:
    """What an implementer is given for one family: the rules, the frozen harness excerpt, an example, the forms."""
    parts = [
        f"Port the `{family}` family. Edit only crates/engine-typed/src and its typed_* suite.",
        "Check your work with `python3 -I scripts/fork/port_check.py " + family + " --static` until it passes.",
        "Never run cargo: compiling belongs to the verifier, which sends back one line per failed rule.",
        "## The differential harness (public items of diff.rs, frozen)", api_excerpt(root),
        "## A worked example", worked_example(root),
        "## typed_call! forms", typed_call_forms(root),
    ]
    if feedback:
        parts += ["## The verifier's lines about your last attempt", *feedback]
    return "\n\n".join(parts) + "\n"


def main(argv: list[str] | None = None, root: Path = REPO, runner: Runner = run, out: Any = sys.stdout) -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    default_queue = str(Path.home() / ".cache" / "nv-workers" / "factory-queue")
    parser.add_argument("--queue", default=default_queue)
    sub = parser.add_subparsers(dest="command", required=True)
    shared = argparse.ArgumentParser(add_help=False)  # --queue after the subcommand too
    shared.add_argument("--queue", default=argparse.SUPPRESS)
    one = sub.add_parser("submit", parents=[shared])
    one.add_argument("family")
    one.add_argument("worktree")
    check = sub.add_parser("verify", parents=[shared])
    check.add_argument("--hidden")
    check.add_argument("--base", default="origin/fork")
    check.add_argument("--no-control", action="store_true")
    sub.add_parser("list", parents=[shared])
    ask = sub.add_parser("prompt", parents=[shared])
    ask.add_argument("family")
    ask.add_argument("--feedback")
    args = parser.parse_args(argv)
    try:
        queue = Queue(args.queue)
        if args.command == "submit":
            print(json.dumps(queue.submit(args.family, args.worktree), sort_keys=True), file=out)
        elif args.command == "verify":
            done = verify_next(queue, runner, Path(args.hidden) if args.hidden else None, args.base, not args.no_control)
            print(json.dumps(done, sort_keys=True), file=out)
        elif args.command == "list":
            print(json.dumps([{k: a[k] for k in ("id", "family", "state", "retries")} for a in queue.all()]), file=out)
        else:
            lines = Path(args.feedback).read_text(encoding="utf-8").splitlines() if args.feedback else None
            print(implementer_prompt(root, args.family, lines), file=out, end="")
    except (ValueError, OSError) as exc:
        print(f"factory_queue: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
