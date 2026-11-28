#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Two independent reviews of a candidate, from two different families, one after the other.

    python3 -I scripts/fork/review_gate.py --pool POOL.json --writer-family FAMILY --diff DIFF \\
        --context TEXT-FILE [--state STATE.json] --out VERDICT.json

An ordinary review and an adversarial one (a red team: evidence for every finding) judge one diff, the second only
after the first. They come from two different families, neither the family that wrote the change, and the adversarial
reviewer is never the one used for the previous adversarial review (the state file remembers it across candidates).
A reviewer that is rate limited, or answers without a readable verdict, is put on a cooldown and replaced by another
of an allowed family; when no allowed reviewer is left the verdict is `absent`, which means: requeue with a backoff.
`fail` goes back to the implementer. The result is the file `port_check.py --full --review` reads:
`{"verdict": "pass|fail|absent", "findings": [{file, line, issue, fix}], "reviewers": [{kind, family}]}`.

The pool is a JSON list of `{"client", "model", "family"}` entries of the free cost class, kept outside the repository
(a machine's own configuration): this file names no model. Each review is one `model-gateway run` capped at 3G,
read-only, with escalation off, and asks for a short answer ending in the JSON verdict, so an output cap cannot cut it.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any, cast

COOLDOWN_SECONDS = 900
CAP = "3G"
KINDS = ("ordinary", "red")
Runner = Callable[..., tuple[int, str]]
Reviewer = dict[str, str]

ORDINARY = ("Review the following change. Give improvement hints, then finish with one JSON object on the last line: "
            '{"verdict":"pass|fail","findings":[{"file":"","line":0,"issue":"","fix":""}]}. '
            "Use fail only for a real defect. Answer in at most 8 lines, no narration.")
RED = ("You are a red team reviewer. Try to break this change: find inputs, races, error paths and misreadings that "
       "make it wrong. Give evidence (file, line, a concrete input) for every finding; no evidence, no finding. "
       "Finish with one JSON object on the last line: "
       '{"verdict":"pass|fail","findings":[{"file":"","line":0,"issue":"","fix":""}]}. '
       "Answer in at most 8 lines, no narration.")


def run(argv: list[str], cwd: str | Path = ".") -> tuple[int, str]:
    """(exit status, standard output); a missing program is status 127."""
    try:
        done = subprocess.run([str(a) for a in argv], cwd=str(cwd), text=True, errors="replace", capture_output=True, check=False)
    except OSError as exc:
        return 127, f"{argv[0]}: {exc}"
    return done.returncode, done.stdout


def load_state(path: Path | None) -> dict[str, Any]:
    """The state file: {"last_red": "client/model", "cooldown": {"client/model": until}}; empty when unreadable."""
    try:
        data: Any = json.loads(Path(path).read_text(encoding="utf-8")) if path else {}
    except (OSError, ValueError):
        return {"last_red": None, "cooldown": {}}
    if not isinstance(data, dict):
        return {"last_red": None, "cooldown": {}}
    state = cast(dict[str, Any], data)
    cooldown = state.get("cooldown")
    numbers = {str(k): float(v) for k, v in cast(dict[Any, Any], cooldown).items()
               if isinstance(v, (int, float)) and not isinstance(v, bool)} if isinstance(cooldown, dict) else {}
    return {"last_red": state.get("last_red"), "cooldown": numbers}


def save_state(path: Path | None, state: dict[str, Any]) -> None:
    if path:
        temp = Path(f"{path}.{os.getpid()}.tmp")
        temp.write_text(json.dumps(state, sort_keys=True), encoding="utf-8")
        os.replace(temp, path)


def key(reviewer: Reviewer) -> str:
    return f"{reviewer['client']}/{reviewer['model']}"


def candidates(pool: list[Reviewer], kind: str, writer_family: str, used_families: list[str], state: dict[str, Any],
               now: float) -> list[Reviewer]:
    """The reviewers that may take this review, in pool order: another family than the writer's and than the review
    before; not on a cooldown; for the adversarial review, not the one that did the last adversarial review."""
    cooldown = cast(dict[str, float], state.get("cooldown", {}))
    allowed: list[Reviewer] = []
    for reviewer in pool:
        if reviewer["family"] == writer_family or reviewer["family"] in used_families:
            continue
        if cooldown.get(key(reviewer), 0.0) > now:
            continue
        if kind == "red" and state.get("last_red") == key(reviewer):
            continue
        allowed.append(reviewer)
    return allowed


def read_verdict(text: str) -> dict[str, Any] | None:
    """The last JSON object of the answer that is a verdict; None when there is none."""
    found: dict[str, Any] | None = None
    for match in re.finditer(r"\{", text):
        try:
            data: Any = json.JSONDecoder().raw_decode(text[match.start():])[0]
        except ValueError:
            continue
        verdict = cast(dict[str, Any], data)  # an object, because the text it came from starts with `{`
        if verdict.get("verdict") in ("pass", "fail") and isinstance(verdict.get("findings", []), list):
            found = verdict
    return found


def request(kind: str, reviewer: Reviewer, diff: str, context: str, workflow: str) -> dict[str, Any]:
    """The gateway request of one read-only review."""
    prompt = (RED if kind == "red" else ORDINARY) + f"\n\nContext: {context}\n\n```diff\n{diff}\n```\n"
    return {"workflow": workflow, "client": reviewer["client"], "model": reviewer["model"], "mode": "read-only",
            "prompt": prompt, "escalate": False}


def ask(kind: str, reviewer: Reviewer, diff: str, context: str, runner: Runner, workflow: str) -> tuple[str, dict[str, Any] | None]:
    """('ok'|'limited'|'absent', verdict): one review through the gateway."""
    with tempfile.TemporaryDirectory() as folder:
        path = Path(folder) / "request.json"
        path.write_text(json.dumps(request(kind, reviewer, diff, context, workflow)), encoding="utf-8")
        code, output = runner(["capped", CAP, "model-gateway", "run", "--input", str(path)])
    try:
        data: Any = json.loads(output)
    except ValueError:
        return "absent", None
    if not isinstance(data, dict):
        return "absent", None
    answer = cast(dict[str, Any], data)
    text = json.dumps(answer)
    if answer.get("status") != "ok" and re.search(r"429|rate.?limit|is limited|quota", text, re.IGNORECASE):
        return "limited", None
    if code or answer.get("status") != "ok":
        return "absent", None
    verdict = read_verdict(str(answer.get("summary") or ""))
    return ("ok", verdict) if verdict else ("absent", None)


def review_candidate(pool: list[Reviewer], writer_family: str, diff: str, context: str, state: dict[str, Any],
                     runner: Runner = run, now: float | None = None, workflow: str = "review") -> dict[str, Any]:
    """Both reviews, one after the other; the combined verdict (absent when a review could not be had)."""
    stamp = time.time() if now is None else now
    state.setdefault("last_red", None)
    cooldown = cast(dict[str, float], state.setdefault("cooldown", {}))
    findings: list[dict[str, Any]] = []
    reviewers: list[dict[str, str]] = []
    verdicts: list[str] = []
    for kind in KINDS:
        got: dict[str, Any] | None = None
        options = candidates(pool, kind, writer_family, [r["family"] for r in reviewers], state, stamp)
        if kind == "ordinary":  # leave the adversarial review a reviewer of another family, when one can be left
            leaves = [r for r in options if candidates(pool, "red", writer_family, [r["family"]], state, stamp)]
            options = leaves or options
        for reviewer in options:
            status, verdict = ask(kind, reviewer, diff, context, runner, f"{workflow}-{kind}")
            if status != "ok":
                cooldown[key(reviewer)] = stamp + COOLDOWN_SECONDS
                continue
            got = verdict
            reviewers.append({"kind": kind, "family": reviewer["family"]})
            if kind == "red":
                state["last_red"] = key(reviewer)
            break
        if got is None:
            return {"verdict": "absent", "findings": [], "reviewers": reviewers}
        verdicts.append(str(got["verdict"]))
        findings += [cast(dict[str, Any], f) for f in got.get("findings", []) if isinstance(f, dict)]
    return {"verdict": "pass" if all(v == "pass" for v in verdicts) else "fail", "findings": findings,
            "reviewers": reviewers}


def main(argv: list[str] | None = None, runner: Runner = run, out: Any = sys.stdout) -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    parser.add_argument("--pool", required=True)
    parser.add_argument("--writer-family", required=True)
    parser.add_argument("--diff", required=True)
    parser.add_argument("--context", required=True)
    parser.add_argument("--state")
    parser.add_argument("--out")
    args = parser.parse_args(argv)
    try:
        raw: Any = json.loads(Path(args.pool).read_text(encoding="utf-8"))
        if not isinstance(raw, list) or not all(isinstance(r, dict) and {"client", "model", "family"} <= r.keys()
                                                for r in cast(list[Any], raw)):
            raise ValueError("the pool is a list of {client, model, family}")
        pool = cast(list[Reviewer], raw)
        diff = Path(args.diff).read_text(encoding="utf-8")
        context = Path(args.context).read_text(encoding="utf-8").strip()
        state_path = Path(args.state) if args.state else None
        state = load_state(state_path)
        result = review_candidate(pool, args.writer_family, diff, context, state, runner)
        save_state(state_path, state)
        text = json.dumps(result, indent=2, sort_keys=True)
        if args.out:
            Path(args.out).write_text(text + "\n", encoding="utf-8")
    except (OSError, ValueError) as exc:
        print(f"review_gate: {exc}", file=sys.stderr)
        return 2
    print(text, file=out)
    return {"pass": 0, "fail": 1, "absent": 3}[result["verdict"]]


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
