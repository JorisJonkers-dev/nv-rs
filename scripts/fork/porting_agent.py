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
    NV_PORT_FAILURE_FILE                           previous attempt's failure, added to the work prompt

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
except ImportError:  # POSIX only
    fcntl = None

try:
    import msvcrt
except ImportError:  # Windows only
    msvcrt = None

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

{previous}
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


LIMITED = re.compile(r"429|rate.?limit|is limited|back in (\d+)s|quota", re.IGNORECASE)
BACK_IN = re.compile(r"back in (\d+)s")


def cooldown_path(env):
    return Path(env.get("NV_PORT_COOLDOWN") or Path(env.get(
        "NV_PORT_SLOT_DIR", str(Path.home() / ".cache" / "nv-workers" / "slots"))) / "cooldown.json")


def cooling(env):
    """Models a provider said it limits, with the time they come back (seconds since epoch)."""
    try:
        data = json.loads(cooldown_path(env).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    now = time.time()
    return {k: v for k, v in data.items() if isinstance(v, (int, float)) and v > now}


def mark_limited(env, model, text):
    found = BACK_IN.search(text)
    until = time.time() + (int(found.group(1)) if found else 900)
    state = cooling(env)
    state[model] = until
    path = cooldown_path(env)
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(state), encoding="utf-8")
    except OSError as exc:
        print(f"cooldown not written: {exc}", file=sys.stderr)


def is_foreign_only(result):
    """A failed run whose only fault is the gateway noticing another run's worktree, with work staged."""
    return (result.get("status") == "failed" and result.get("reason") == "foreign-worktree-change"
            and bool(result.get("worktree")) and bool(result.get("staged_files")))


def is_limited(result):
    return result.get("status") != "ok" and bool(LIMITED.search(json.dumps(result)))


def candidates(env, prefix, default):
    """(client, model) pairs to try, the family's pick first, models in cooldown last.

    An entry of `<prefix>_MODELS` is `model` or `client:model` (the client defaults to
    `<prefix>_CLIENT`).
    """
    client = env.get(f"{prefix}_CLIENT", default[0])
    pairs = []
    for item in [m.strip() for m in env.get(f"{prefix}_MODELS", "").split(",") if m.strip()]:
        head, sep, tail = item.partition(":")
        pairs.append((head, tail) if sep and "/" not in head else (client, item))
    if f"{prefix}_MODEL" in env or not pairs:
        return [(client, env.get(f"{prefix}_MODEL", default[1]))]
    start = zlib.crc32(env.get("NV_PORT_FAMILY", "").encode()) % len(pairs)
    ordered = pairs[start:] + pairs[:start]
    hot = cooling(env)
    return [p for p in ordered if p[1] not in hot] + [p for p in ordered if p[1] in hot]


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
                    taken = self._try_lock(handle)
                except Exception:
                    handle.close()
                    raise
                if not taken:
                    handle.close()  # held by another run: try the next slot
                    continue
                self.handle = handle
                return self
            time.sleep(self.POLL)
        return self

    POLL = 2

    def _try_lock(self, handle):
        """Take an advisory lock; False means this slot is held by another run."""
        try:
            if fcntl is not None:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            elif msvcrt is not None:
                msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                raise RuntimeError(
                    "no file locking on this platform: the slot limit cannot be enforced")
        except OSError:
            return False
        return True

    def _unlock(self, handle):
        """Release a Windows byte lock; closing the file releases an flock on POSIX."""
        if fcntl is None and msvcrt is not None:
            handle.seek(0)
            msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)

    def __exit__(self, *_):
        if self.handle is not None:
            try:
                self._unlock(self.handle)
            finally:
                self.handle.close()
                self.handle = None


def provider_of(model):
    """The part of a model name before the first slash; a bare name is its own provider."""
    return (model or "unknown").split("/", 1)[0] or "unknown"


def provider_slot(env, model):
    """A lock bounding concurrent runs of one provider (NV_PORT_PROVIDER_LIMIT, default 4)."""
    try:
        limit = int(env.get("NV_PORT_PROVIDER_LIMIT", "4"))
    except ValueError:
        limit = 4
    base = env.get("NV_PORT_SLOT_DIR", str(Path.home() / ".cache" / "nv-workers" / "slots"))
    return Slot(Path(base) / f"provider-{provider_of(model)}", max(1, limit))


ALLOWED_PATHS = ("crates/engine-typed/", "crates/engine/tests/typed_*.rs")


def allowed_path(name):
    """A worker's change belongs to the typed crate or a typed_* Diff suite; the rest is stray."""
    return name.startswith("crates/engine-typed/") or (
        name.startswith("crates/engine/tests/typed_") and name.endswith(".rs") and "/" not in name[len("crates/engine/tests/"):])


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


def previous_failure(env):
    """The failure file the loop wrote before a retry, as a prompt section.

    The loop bounds the file to failing case names and the first 40 error lines;
    a missing or empty file simply leaves the section out. The text is data from
    the repository's own test output, not decompiled material.
    """
    path = env.get("NV_PORT_FAILURE_FILE")
    if not path:
        return ""
    try:
        text = Path(path).read_text(encoding="utf-8").strip()
    except OSError:
        return ""
    if not text:
        return ""
    return "Your previous attempt failed with this output:\n\n" + text + "\n\n"


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
    base = run(["git", "rev-parse", "HEAD"], worktree, check=True).stdout.strip()
    prompt = WORK_PROMPT.format(
        family=family, batch=batch, rows=read_rows(env), template=template_family(worktree),
        rulebook=(worktree / RULEBOOK).read_text(encoding="utf-8"),
        previous=previous_failure(env))
    result, payload, started = {}, {}, time.time()
    for client, model in candidates(env, "NV_PORT_WORKER", DEFAULT_WORKER):
        payload = {
            "workflow": f"porting-{batch}-{family}"[:80], "client": client, "model": model,
            "mode": "write", "prompt": prompt, "repo": str(worktree), "base": base,
            "check": "python3 -I scripts/fork/private_integrity.py --check",
            "time_limit_s": int(env.get("NV_PORT_TIME_LIMIT", "1500")), "escalate": False,
        }
        started = time.time()
        with Slot(env.get("NV_PORT_SLOT_DIR", str(Path.home() / ".cache" / "nv-workers" / "slots")),
                  int(env.get("NV_PORT_SLOTS", "2"))), provider_slot(env, model):
            result = gateway(env, payload)
        if not is_limited(result):
            break
        mark_limited(env, model, json.dumps(result))
        metrics(env, {"role": "worker", "family": family, "batch": batch, "client": client,
                      "model": model, "outcome": "limited", "status": result.get("status")})
        print(f"{model} is limited; trying the next model", file=sys.stderr)
    record = {"role": "worker", "family": family, "batch": batch, "client": client, "model": model,
              "seconds": round(time.time() - started, 1), "status": result.get("status"),
              "reason": result.get("reason") or result.get("code"),
              "usage": result.get("usage") or result.get("tokens"), "cost": result.get("cost_usd")}
    try:
        Path(env["NV_PORT_QUEUE_FILE"] + ".worker").write_text(f"{payload.get('model', '')}\n", encoding="utf-8")
    except OSError:
        pass
    if is_foreign_only(result):
        # Another run's throwaway worktree appeared in the shared repository during this run; the
        # worker's own worktree is untouched, so its work is taken (the Diff run and review still judge it).
        result = {**result, "status": "ok"}
        record["reason"] = "foreign-worktree-change"
    if result.get("status") != "ok" or not result.get("worktree"):
        metrics(env, {**record, "outcome": "gateway-failed"})
        print(f"worker run failed: {json.dumps(result)[:600]}", file=sys.stderr)
        return 1
    gw = Path(result["worktree"])
    run(["git", "add", "-A"], gw)
    names = run(["git", "diff", "--cached", "--name-only", base], gw).stdout.split()
    stray = [n for n in names if not allowed_path(n)]
    if stray:
        record["stray"] = stray
        print(f"dropping files outside the family's module and test: {stray}", file=sys.stderr)
    patch = run(["git", "diff", "--cached", "--binary", base, "--", *ALLOWED_PATHS], gw)
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
    try:
        wmodel = Path(env["NV_PORT_QUEUE_FILE"] + ".worker").read_text(encoding="utf-8").strip()
    except OSError:
        wmodel = settings(env, "NV_PORT_WORKER", DEFAULT_WORKER)[1]
    options = [c for c in candidates(env, "NV_PORT_REVIEW", DEFAULT_REVIEWER)
               if family_of(c[1]) != family_of(wmodel)]
    if not options:
        print("reviewer must differ from the worker", file=sys.stderr)
        return 2
    diff = run(["git", "diff", "HEAD~1", "HEAD"], worktree).stdout
    if len(diff) > MAX_DIFF:
        diff = diff[:MAX_DIFF] + "\n[diff cut]\n"
    prompt = REVIEW_PROMPT.format(family=family, rows=read_rows(env), diff=diff)
    payload = {"workflow": f"porting-review-{batch}-{family}"[:80], "client": options[0][0], "model": options[0][1],
               "mode": "read-only", "prompt": prompt, "repo": str(worktree),
               "time_limit_s": int(env.get("NV_PORT_TIME_LIMIT", "1500")), "escalate": False}
    started, result = time.time(), {}
    for client, model in options:
        payload.update({"client": client, "model": model})
        started = time.time()
        with provider_slot(env, model):
            result = gateway(env, payload, sandbox=env.get("NV_PORT_REVIEW_SANDBOX", "sbx"))
        if not is_limited(result):
            break
        mark_limited(env, model, json.dumps(result))
        print(f"{model} is limited; trying the next reviewer", file=sys.stderr)
    text = result.get("summary") or result.get("output") or ""
    verdicts = VERDICT.findall(text)
    verdict = verdicts[-1].upper() if verdicts else "NONE"
    metrics(env, {"role": "reviewer", "family": family, "batch": batch, "client": client, "model": model,
                  "seconds": round(time.time() - started, 1), "status": result.get("status"),
                  "verdict": verdict, "usage": result.get("usage") or result.get("tokens")})
    print(text[-3000:])
    return 0 if result.get("status") == "ok" and verdict == "ACCEPT" else 1


def probe(env, runner=None, hour=3600):
    """Run one cheap read-only prompt per configured model; park the ones that fail for an hour.

    Returns {model: ok}. A model that answers is taken out of the cooldown file.
    """
    from concurrent.futures import ThreadPoolExecutor
    runner = runner or gateway
    pairs = []
    for prefix, default in (("NV_PORT_WORKER", DEFAULT_WORKER), ("NV_PORT_REVIEW", DEFAULT_REVIEWER)):
        for pair in candidates({**env, "NV_PORT_FAMILY": ""}, prefix, default):
            if pair not in pairs:
                pairs.append(pair)

    def one(pair):
        client, model = pair
        result = runner(env, {"workflow": "porting-probe", "client": client, "model": model,
                              "mode": "read-only", "prompt": "Reply with exactly: OK",
                              "time_limit_s": int(env.get("NV_PORT_PROBE_TIME", "120")),
                              "escalate": False})
        return model, result.get("status") == "done" or result.get("status") == "ok"

    with ThreadPoolExecutor(max_workers=max(1, len(pairs))) as pool:
        outcome = dict(pool.map(one, pairs))
    state = cooling(env)
    for model, ok in outcome.items():
        if ok:
            state.pop(model, None)
        else:
            state[model] = time.time() + hour
    path = cooldown_path(env)
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(state), encoding="utf-8")
    except OSError as exc:
        print(f"cooldown not written: {exc}", file=sys.stderr)
    return outcome


def rates(path):
    """Per-model counts from the metrics file: runs, committed, limited, and review accepts."""
    table = {}
    try:
        lines = Path(path).read_text(encoding="utf-8").splitlines()
    except OSError:
        return table
    for line in lines:
        try:
            row = json.loads(line)
        except ValueError:
            continue
        entry = table.setdefault(row.get("model", "?"), {"runs": 0, "committed": 0, "limited": 0,
                                                         "reviews": 0, "accepted": 0, "seconds": 0.0})
        if row.get("role") == "reviewer":
            entry["reviews"] += 1
            entry["accepted"] += row.get("verdict") == "ACCEPT"
            continue
        entry["runs"] += 1
        entry["committed"] += row.get("outcome") == "committed"
        entry["limited"] += row.get("outcome") == "limited"
        entry["seconds"] += row.get("seconds") or 0
    return table


def main(argv=None, env=None):
    argv = sys.argv[1:] if argv is None else argv
    env = dict(os.environ if env is None else env)
    if argv == ["probe"]:
        for model, ok in sorted(probe(env).items()):
            print(("ok   " if ok else "down ") + model)
        return 0
    if argv == ["rates"]:
        for model, row in sorted(rates(env.get("NV_PORT_METRICS", "metrics.jsonl")).items()):
            print(model, json.dumps(row, sort_keys=True))
        return 0
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
