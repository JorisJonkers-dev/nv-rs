#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The worker and reviewer commands of the porting loop (NV_PORT_WORKER, NV_PORT_REVIEW).

`work` hands one family to a cheap writing model through the model gateway and applies what
it wrote to the family worktree (NV_PORT_WORKTREE) as one commit. `review` asks a model of
another family, read-only, to judge that commit and exits 0 only on an ACCEPT verdict.

Only public material goes to a model: the rulebook, the family's queue rows (addresses and
names) and the repository, which is public. No game data, exe bytes or decompiled output is
ever put in a prompt; the translations a worker reads are the repository's own Rust.

Configuration (environment, nothing machine-specific in this file):
    NV_PORT_WORKER_CLIENT / NV_PORT_WORKER_MODEL   writing model (default pi-free, free/deepseek-v4.1-flash)
    NV_PORT_REVIEW_CLIENT / NV_PORT_REVIEW_MODEL   reviewing model, another family than the worker's
    NV_PORT_GATEWAY                                the gateway command (default model-gateway)
    NV_PORT_SLOT_DIR                               directory of the write-run slot locks
    NV_PORT_SLOTS                                  concurrent writing runs (default 2)
    NV_PORT_TIME_LIMIT                             seconds per model run (default 1500)
    NV_PORT_METRICS                                JSON lines file the runs are logged to

Usage:
    python3 -I scripts/fork/porting_agent.py work
    python3 -I scripts/fork/porting_agent.py review
"""
import json
import os
import re
import subprocess
import zlib
import sys
import time
from pathlib import Path

try:
    import fcntl
except ImportError:  # Windows: the slot lock is not taken
    fcntl = None

FORK = Path(__file__).resolve().parent
REPO = FORK.parent.parent
RULEBOOK = Path("docs") / "fork" / "PORTING-RULEBOOK.md"
VERDICT = re.compile(r"^\s*VERDICT:\s*(ACCEPT|REJECT)\b", re.MULTILINE | re.IGNORECASE)
DEFAULT_WORKER = ("pi-free", "free/deepseek-v4.1-flash")
DEFAULT_REVIEWER = ("pi-free", "free/glm-5.3-flash")
MAX_DIFF = 60000

WORK_PROMPT = """You are porting functions of one family into typed Rust in the nv-rs repository
(your working directory). Edit only files inside the working directory. Do not commit.

Family: {family}
Batch: {batch}

The functions to type are these rows (rank, address, state, done, family, name); the address is
the function's exe address, the name its Xbox-prototype name:

{rows}

How a function is typed:
- Each function already has a translation in an engine part crate (`crates/engine-*/src`), marked
  `// Translated from <address>`. Find it with grep. It is the reference: the typed version must
  behave exactly as it does, including float rounding and every side effect.
- Read an existing typed family in `crates/engine-typed/src/` (the family `{template}` is a good
  model) and follow its layout, its `// Typed from <address>` marker above each function, its
  registration of the function at the exe address in the family's `funcs`, and its `typed_call!`
  use for calls by address.
- Add a Diff case for each typed function to the family's suite `crates/engine/tests/typed_{family}.rs`
  (create the suite from an existing one if the family has none). A case runs the typed version and
  the translation on the same inputs and compares them.
- Never edit the checker: crates/engine-core/src/typed/diff.rs, crates/engine-core/src/typed/mod.rs,
  crates/engine-core/src/x87.rs, crates/engine/tests/diff_reference_peer.rs or anything under
  scripts/ledger. Never weaken, delete or skip a test. Never run cargo; builds are run for you.
- New source files start with `// SPDX-License-Identifier: GPL-3.0-only`.
- If a function cannot be typed faithfully, leave it out and say why in your last message instead
  of guessing.

The rulebook every batch follows:

{rulebook}

End your last message with one line per function: `TYPED <address>` or `SKIPPED <address>: <reason>`.
"""

REVIEW_PROMPT = """You review one porting commit in the nv-rs repository (your working directory,
checked out at the commit). Another model typed the functions of family `{family}`.

Queue rows the commit was meant to type:

{rows}

Check, reading the code: every row has a typed function with a `// Typed from <address>` marker, it
matches the translation (`// Translated from <address>`) in behaviour including float rounding and
side effects, it is registered at the exe address, each has a Diff case in
`crates/engine/tests/typed_{family}.rs` that exercises it with real inputs, and no checker file,
test or unrelated code was changed or weakened. Reject if any function is a stub, a copy that cannot
differ from the translation, or a case that asserts nothing.

The commit's diff:

{diff}

Give short findings, then end with exactly one line: `VERDICT: ACCEPT` or `VERDICT: REJECT`.
"""


def settings(env, prefix, default):
    """(client, model). `<prefix>_MODELS` is a comma list the family name picks from, to spread load."""
    client = env.get(f"{prefix}_CLIENT", default[0])
    models = [m.strip() for m in env.get(f"{prefix}_MODELS", "").split(",") if m.strip()]
    if models and f"{prefix}_MODEL" not in env:
        return client, models[zlib.crc32(env.get("NV_PORT_FAMILY", "").encode()) % len(models)]
    return client, env.get(f"{prefix}_MODEL", default[1])


def family_of(model):
    return model.split("/")[-1].split("-")[0]


def run(cmd, cwd, env=None, check=False):
    return subprocess.run([str(c) for c in cmd], cwd=str(cwd), env=env, text=True, errors="replace",
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=check)


def metrics(env, record):
    path = Path(env.get("NV_PORT_METRICS") or Path(env.get("NV_PORT_WORKTREE", ".")) / "metrics.jsonl")
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(record, sort_keys=True) + "\n")
    except OSError as exc:
        print(f"metrics not written: {exc}", file=sys.stderr)


class Slot:
    """One of N advisory locks: bounds how many writing runs (and so builds) go at once."""

    def __init__(self, directory, count):
        self.directory, self.count, self.handle = Path(directory), max(1, count), None

    def __enter__(self):
        self.directory.mkdir(parents=True, exist_ok=True)
        while self.handle is None:
            for index in range(self.count):
                handle = (self.directory / f"slot{index}.lock").open("w")
                try:
                    if fcntl is not None:
                        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except OSError:
                    handle.close()
                    continue
                self.handle = handle
                return self
            time.sleep(2)
        return self

    def __exit__(self, *_):
        if self.handle:
            self.handle.close()


def gateway(env, payload, sandbox=None):
    """One gateway run; returns the parsed result (an error object on a refusal)."""
    child = dict(env)
    if sandbox:
        child["AGENT_KIT_SANDBOX"] = sandbox
    done = run([env.get("NV_PORT_GATEWAY", "model-gateway"), "run", "--json", json.dumps(payload)],
               cwd=payload.get("repo") or ".", env=child)
    try:
        return json.loads(done.stdout)
    except json.JSONDecodeError:
        return {"status": "error", "code": "unparsable", "message": (done.stdout + done.stderr)[-400:]}


def read_rows(env):
    return Path(env["NV_PORT_QUEUE_FILE"]).read_text(encoding="utf-8")


def template_family(worktree):
    """A small typed family to point the model at: the suite with the fewest lines."""
    suites = sorted((Path(worktree) / "crates" / "engine" / "tests").glob("typed_*.rs"),
                    key=lambda p: p.stat().st_size)
    return suites[0].stem[len("typed_"):] if suites else "culling"


def clean(env, payload, cwd):
    """Remove the gateway's kept worktree of this run once its diff is taken."""
    run([env.get("NV_PORT_GATEWAY", "model-gateway"), "worktrees", "clean", "--json",
         json.dumps({"workflow": payload["workflow"]})], cwd)


def work(env):
    worktree = Path(env["NV_PORT_WORKTREE"])
    family, batch = env["NV_PORT_FAMILY"], env["NV_PORT_BATCH"]
    client, model = settings(env, "NV_PORT_WORKER", DEFAULT_WORKER)
    base = run(["git", "rev-parse", "HEAD"], worktree, check=True).stdout.strip()
    prompt = WORK_PROMPT.format(
        family=family, batch=batch, rows=read_rows(env), template=template_family(worktree),
        rulebook=(worktree / RULEBOOK).read_text(encoding="utf-8"))
    payload = {
        "workflow": f"porting-{batch}-{family}"[:80], "client": client, "model": model,
        "mode": "write", "prompt": prompt, "repo": str(worktree), "base": base,
        "check": "python3 -I scripts/fork/private_integrity.py --check",
        "time_limit_s": int(env.get("NV_PORT_TIME_LIMIT", "1500")), "escalate": False,
    }
    started = time.time()
    with Slot(env.get("NV_PORT_SLOT_DIR", str(Path.home() / ".cache" / "nv-workers" / "slots")),
              int(env.get("NV_PORT_SLOTS", "2"))):
        result = gateway(env, payload)
    record = {"role": "worker", "family": family, "batch": batch, "client": client, "model": model,
              "seconds": round(time.time() - started, 1), "status": result.get("status"),
              "reason": result.get("reason") or result.get("code"),
              "usage": result.get("usage") or result.get("tokens"), "cost": result.get("cost_usd")}
    if result.get("status") != "ok" or not result.get("worktree"):
        metrics(env, {**record, "outcome": "gateway-failed"})
        print(f"worker run failed: {json.dumps(result)[:600]}", file=sys.stderr)
        return 1
    gw = Path(result["worktree"])
    patch = run(["git", "diff", "--cached", "--binary", base], gw)
    clean(env, payload, worktree)
    if patch.returncode or not patch.stdout.strip():
        metrics(env, {**record, "outcome": "empty"})
        print("worker produced no change", file=sys.stderr)
        return 1
    applied = subprocess.run(["git", "apply", "--index", "--whitespace=nowarn", "-"], cwd=str(worktree),
                             input=patch.stdout, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if applied.returncode:
        metrics(env, {**record, "outcome": "apply-failed"})
        print(f"patch did not apply: {applied.stderr[-400:]}", file=sys.stderr)
        return 1
    committed = run(["git", "commit", "-m", f"feat(engine-typed): type the {family} family ({batch})"], worktree)
    if committed.returncode:
        metrics(env, {**record, "outcome": "commit-failed"})
        print(committed.stdout + committed.stderr, file=sys.stderr)
        return 1
    metrics(env, {**record, "outcome": "committed", "files": result.get("staged_files")})
    print(result.get("summary") or "worker committed")
    return 0


def review(env):
    worktree = Path(env["NV_PORT_WORKTREE"])
    family, batch = env["NV_PORT_FAMILY"], env["NV_PORT_BATCH"]
    client, model = settings(env, "NV_PORT_REVIEW", DEFAULT_REVIEWER)
    wclient, wmodel = settings(env, "NV_PORT_WORKER", DEFAULT_WORKER)
    if family_of(model) == family_of(wmodel) and env.get("NV_PORT_REVIEW_MODELS"):
        others = [m.strip() for m in env["NV_PORT_REVIEW_MODELS"].split(",")
                  if m.strip() and family_of(m.strip()) != family_of(wmodel)]
        if others:
            model = others[0]
    if (client, model) == (wclient, wmodel):
        print("reviewer must differ from the worker", file=sys.stderr)
        return 2
    diff = run(["git", "diff", "HEAD~1", "HEAD"], worktree).stdout
    if len(diff) > MAX_DIFF:
        diff = diff[:MAX_DIFF] + "\n[diff cut]\n"
    prompt = REVIEW_PROMPT.format(family=family, rows=read_rows(env), diff=diff)
    payload = {"workflow": f"porting-review-{batch}-{family}"[:80], "client": client, "model": model,
               "mode": "read-only", "prompt": prompt, "repo": str(worktree),
               "time_limit_s": int(env.get("NV_PORT_TIME_LIMIT", "1500")), "escalate": False}
    started = time.time()
    result = gateway(env, payload, sandbox=env.get("NV_PORT_REVIEW_SANDBOX", "sbx"))
    text = result.get("summary") or result.get("output") or ""
    verdicts = VERDICT.findall(text)
    verdict = verdicts[-1].upper() if verdicts else "NONE"
    metrics(env, {"role": "reviewer", "family": family, "batch": batch, "client": client, "model": model,
                  "seconds": round(time.time() - started, 1), "status": result.get("status"),
                  "verdict": verdict, "usage": result.get("usage") or result.get("tokens")})
    print(text[-3000:])
    return 0 if result.get("status") == "ok" and verdict == "ACCEPT" else 1


def main(argv=None, env=None):
    argv = sys.argv[1:] if argv is None else argv
    env = dict(os.environ if env is None else env)
    if argv not in (["work"], ["review"]):
        print(__doc__, file=sys.stderr)
        return 2
    missing = [k for k in ("NV_PORT_WORKTREE", "NV_PORT_FAMILY", "NV_PORT_BATCH", "NV_PORT_QUEUE_FILE")
               if not env.get(k)]
    if missing:
        print("missing " + ", ".join(missing), file=sys.stderr)
        return 2
    return work(env) if argv == ["work"] else review(env)


if __name__ == "__main__":
    sys.exit(main())
