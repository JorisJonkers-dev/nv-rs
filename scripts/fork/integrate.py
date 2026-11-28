#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The deterministic integrator: stack verified task branches into one batch branch and one pull request.

    python3 -I scripts/fork/integrate.py --name 2026-11-28 --task feat/a:1101 --task feat/b:1102 \\
        [--base origin/fork] [--max 4] [--gate "CMD"] [--push] [--pr] [--assignee LOGIN] [--out FILE]

It is the merge queue until the repository has one, and it is a script, not a model: the same inputs give the same
batch. Run it in a clean worktree of the repository. For each task branch, in the order given (at most --max of them),
it merges the branch onto the batch and runs the gate command on the stacked result. A branch that conflicts, or whose
gate fails, is taken off again, its reason is kept and the batch goes on without it: the culprit goes back to the queue
with its failure, the rest lands. Before each push the batch is rebased onto the base, so the pushed result is what
the base plus the batch is. One batch pull request carries `Closes #N` for every accepted task.

The gate command runs in the worktree after each merge and passes with exit status 0 (for example
`python3 -I scripts/fork/port_check.py FAMILY --static`). With no --gate only the merge itself is judged.
Exit status: 0 when at least one task was accepted (or there was nothing to do), 1 when every task was dropped, 2 on
a usage or git failure.
"""
import argparse
import json
import re
import shlex
import subprocess
import sys
from pathlib import Path

DEFAULT_MAX = 4
BRANCH = re.compile(r"^[A-Za-z0-9._/-]+$")
OUTPUT_TAIL = 600


class IntegrateError(Exception):
    """A git or usage failure that ends the run."""


def run(argv, cwd):
    """(exit status, combined output) of one command; a missing program is status 127."""
    try:
        done = subprocess.run([str(a) for a in argv], cwd=str(cwd), text=True, errors="replace",
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    except OSError as exc:
        return 127, f"{argv[0]}: {exc}"
    return done.returncode, done.stdout


def parse_task(text):
    """`branch:issue` as (branch, issue number or None)."""
    branch, _, issue = text.partition(":")
    if not BRANCH.match(branch) or branch.startswith("-"):
        raise IntegrateError(f"not a branch name: {text!r}")
    if issue and not issue.isdigit():
        raise IntegrateError(f"not an issue number: {text!r}")
    return branch, int(issue) if issue else None


def git(root, runner, *args, check=True):
    code, output = runner(["git", *args], root)
    if check and code:
        raise IntegrateError(f"git {' '.join(args)}: {output.strip()[-OUTPUT_TAIL:]}")
    return code, output


def tail(output):
    return output.strip()[-OUTPUT_TAIL:]


def stack(root, runner, name, base, tasks, gate, limit):
    """Merge the tasks onto a fresh batch branch; returns (accepted, dropped, deferred)."""
    branch = f"batch/{name}"
    git(root, runner, "checkout", "-q", "-B", branch, base)
    accepted, dropped = [], []
    taken, deferred = tasks[:limit], tasks[limit:]
    for task, issue in taken:
        head = git(root, runner, "rev-parse", "HEAD")[1].strip()
        code, output = git(root, runner, "merge", "--no-ff", "-m", f"chore(fork): add {task} to batch {name}", task,
                           check=False)
        if code:
            git(root, runner, "merge", "--abort", check=False)
            git(root, runner, "reset", "-q", "--hard", head)
            dropped.append({"task": task, "issue": issue, "reason": "conflict", "detail": tail(output)})
            continue
        if gate:
            code, output = runner(gate, root)
            if code:
                git(root, runner, "reset", "-q", "--hard", head)
                dropped.append({"task": task, "issue": issue, "reason": "gate", "detail": tail(output)})
                continue
        accepted.append({"task": task, "issue": issue})
    return accepted, dropped, [{"task": t, "issue": i} for t, i in deferred]


def pr_body(name, accepted, dropped):
    """The batch pull request's body: what it lands, and what went back to the queue."""
    summary = f"Batch {name}: {len(accepted)} verified task(s) stacked on the base, each merged and gated in turn."
    lines = ["## What", summary, ""]
    lines += [f"- {a['task']}" for a in accepted] or ["- none"]
    lines += ["", "## Why", ""]
    lines += [f"Closes #{a['issue']}" for a in accepted if a["issue"]] or ["No issue is closed."]
    if dropped:
        lines += ["", "## Back in the queue"]
        lines += [f"- {d['task']}: {d['reason']}" for d in dropped]
    return "\n".join(lines) + "\n"


def push_batch(root, runner, name, base, gate):
    """Rebase the batch onto the base, gate it again if that changed it, and push it.

    The branch is the integrator's own, so the push is a lease-checked force.
    """
    remote = base.partition("/")[0]
    git(root, runner, "fetch", "-q", remote)
    before = git(root, runner, "rev-parse", "HEAD^{tree}")[1]
    code, output = git(root, runner, "rebase", "-q", base, check=False)
    if code:
        git(root, runner, "rebase", "--abort", check=False)
        raise IntegrateError(f"the batch does not rebase onto {base}: {tail(output)}")
    if gate and git(root, runner, "rev-parse", "HEAD^{tree}")[1] != before:
        code, output = runner(gate, root)
        if code:
            raise IntegrateError(f"the gate fails on the batch rebased onto {base}: {tail(output)}")
    git(root, runner, "push", "-q", "--force-with-lease", "-u", remote, f"batch/{name}")


def open_pr(root, runner, name, base, body, assignee):
    """Open the batch pull request, or update the one the branch already has; returns gh's answer."""
    branch = f"batch/{name}"
    code, existing = runner(["gh", "pr", "list", "--head", branch, "--json", "number", "--jq", ".[0].number"], root)
    number = existing.strip() if not code else ""
    title = f"feat(engine): porting batch {name}"
    if number:
        argv = ["gh", "pr", "edit", number, "--title", title, "--body", body]
    else:
        argv = ["gh", "pr", "create", "--base", base.partition("/")[2] or base, "--head", branch, "--title", title,
                "--body", body]
        if assignee:
            argv += ["--assignee", assignee]
    code, output = runner(argv, root)
    if code:
        raise IntegrateError(f"gh: {tail(output)}")
    return output.strip()


def main(argv=None, root=".", runner=run, out=sys.stdout):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--name", required=True)
    parser.add_argument("--task", action="append", default=[], help="branch[:issue]; the order is the merge order")
    parser.add_argument("--base", default="origin/fork")
    parser.add_argument("--max", type=int, default=DEFAULT_MAX)
    parser.add_argument("--gate", help="command run after each merge; non-zero drops that task")
    parser.add_argument("--push", action="store_true", help="rebase onto the base and push the batch branch")
    parser.add_argument("--pr", action="store_true", help="with --push: open or update the batch pull request")
    parser.add_argument("--assignee")
    parser.add_argument("--out", help="also write the result to this file")
    args = parser.parse_args(argv)
    try:
        if not BRANCH.match(args.name) or args.max < 1:
            raise IntegrateError("--name must be a plain name and --max at least 1")
        tasks = [parse_task(t) for t in args.task]
        gate = shlex.split(args.gate) if args.gate else None
        accepted, dropped, deferred = stack(root, runner, args.name, args.base, tasks, gate, args.max)
        result = {"batch": f"batch/{args.name}", "accepted": accepted, "dropped": dropped, "deferred": deferred,
                  "pr": None}
        if accepted and args.push:
            push_batch(root, runner, args.name, args.base, gate)
            if args.pr:
                result["pr"] = open_pr(root, runner, args.name, args.base, pr_body(args.name, accepted, dropped),
                                       args.assignee)
    except IntegrateError as exc:
        print(f"integrate: {exc}", file=sys.stderr)
        return 2
    text = json.dumps(result, indent=2, sort_keys=True)
    if args.out:
        Path(args.out).write_text(text + "\n", encoding="utf-8")
    print(text, file=out)
    return 1 if tasks and not accepted else 0


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
