#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The porting benchmark: a frozen set of already-ported functions, re-ported blind, scored on hidden cases.

    python3 -I scripts/fork/port_benchmark.py select [--count 25] [--out SET.json]
    python3 -I scripts/fork/port_benchmark.py check SET.json
    python3 -I scripts/fork/port_benchmark.py hide SET.json ADDRESS
    python3 -I scripts/fork/port_benchmark.py record SET.json RESULTS.json --label LABEL --prompt FILE --n-impl N --out LOG.jsonl
    python3 -I scripts/fork/port_benchmark.py compare LOG.jsonl

The set is 20 to 30 functions that are already typed, chosen by a fixed rule (the lowest hashes of their addresses),
and frozen with a hash over its entries: a changed set is a different benchmark, and `check` says so. A blind attempt
sees only the address, the original-side harness and the rules: `hide` lists the files that hold the accepted code and
its tests (the typed source with the `// Typed from ADDRESS` marker, and the suites that name the address), which a
blind workspace must not contain. Scoring uses hidden differential cases, kept outside the repository.

`record` turns one run's results (one entry per address: passed the hidden cases or not, retries, seconds, cost) into one
line of a versioned, append-only log: correct rate with a 95% Wilson interval, functions per hour, dollars per accepted
function and retries per success (retries and cost of failed ports are charged to the successes), with the label (the model and route, as the operator names them), the hash of the
prompt and the number of implementers. `compare` reads the log and judges a run against the one before with the same
label: a change is real only when the intervals do not overlap; one function's difference is noise.
"""
import argparse
import hashlib
import json
import math
import re
import sys
from pathlib import Path
from typing import Any, cast

REPO = Path(__file__).resolve().parent.parent.parent
TYPED = Path("crates") / "engine-typed" / "src"
SUITES = Path("crates") / "engine" / "tests"
MARKER = re.compile(r"^\s*// Typed from ([0-9a-fA-F]{8})\b", re.MULTILINE)
LITERAL = re.compile(r"0x([0-9a-fA-F_]{6,11})\b")
MIN_SET, MAX_SET = 20, 30
Z = 1.96

Entry = dict[str, Any]


def set_hash(entries: list[Any]) -> str:
    """The hash of the entries, in a canonical form: the benchmark's identity."""
    return hashlib.sha256(json.dumps(entries, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def normal(literal: str) -> str:
    return literal.lower().removeprefix("0x").replace("_", "").zfill(8)


def ported(root: Path) -> list[Entry]:
    """Every typed function that a suite names by its address: {address, source, suites}."""
    suites: dict[str, list[str]] = {}
    for suite in sorted((root / SUITES).glob("typed_*.rs")):
        for literal in set(LITERAL.findall(suite.read_text(encoding="utf-8", errors="replace"))):
            suites.setdefault(normal(literal), []).append(suite.relative_to(root).as_posix())
    found: list[Entry] = []
    for source in sorted((root / TYPED).rglob("*.rs")):
        for address in sorted(set(MARKER.findall(source.read_text(encoding="utf-8", errors="replace")))):
            if address.lower() in suites:
                found.append({"address": address.lower(), "source": source.relative_to(root).as_posix(),
                              "suites": suites[address.lower()]})
    return found


def select(root: Path, count: int) -> dict[str, Any]:
    """The frozen set: the `count` ported functions with the lowest hashes of their addresses."""
    if not MIN_SET <= count <= MAX_SET:
        raise ValueError(f"a benchmark has {MIN_SET} to {MAX_SET} functions, not {count}")
    pool = ported(root)
    if len(pool) < count:
        raise ValueError(f"only {len(pool)} ported functions have a suite")
    chosen = sorted(pool, key=lambda e: hashlib.sha256(e["address"].encode()).hexdigest())[:count]
    entries = sorted(chosen, key=lambda e: e["address"])
    return {"version": 1, "entries": entries, "hash": set_hash(entries)}


def check(frozen: dict[str, Any]) -> list[str]:
    """What is wrong with a frozen set: its size, duplicate addresses, a hash that no longer matches."""
    raw: Any = frozen.get("entries")
    if not isinstance(raw, list) or not MIN_SET <= len(cast(list[Any], raw)) <= MAX_SET:
        return [f"a benchmark has {MIN_SET} to {MAX_SET} functions"]
    entries = cast(list[Any], raw)
    problems: list[str] = []
    addresses = [str(cast(dict[str, Any], e).get("address")) for e in entries if isinstance(e, dict)]
    if len(set(addresses)) != len(entries):
        problems.append("an address is listed twice or an entry is not an object")
    if frozen.get("hash") != set_hash(entries):
        problems.append("the hash does not match the entries: this is another benchmark")
    return problems


def hide(frozen: dict[str, Any], address: str) -> list[str]:
    """The files a blind workspace for `address` must not contain: the accepted code and the tests that name it."""
    for entry in frozen["entries"]:
        if entry["address"] == normal(address):
            return [entry["source"], *entry["suites"]]
    raise ValueError(f"{address} is not in the set")


def wilson(passed: int, total: int) -> tuple[float, float]:
    """The 95% Wilson score interval of a proportion."""
    if total == 0:
        return 0.0, 1.0
    p = passed / total
    denom = 1 + Z * Z / total
    centre = (p + Z * Z / (2 * total)) / denom
    half = Z * math.sqrt(p * (1 - p) / total + Z * Z / (4 * total * total)) / denom
    return max(0.0, centre - half), min(1.0, centre + half)


def summarise(frozen: dict[str, Any], results: list[Entry]) -> dict[str, Any]:
    """The measures of one run over the frozen set; a function with no result counts as not correct."""
    by_address = {str(r["address"]): r for r in results}
    unknown = sorted(set(by_address) - {e["address"] for e in frozen["entries"]})
    if unknown:
        raise ValueError(f"results for functions outside the set: {', '.join(unknown)}")
    total = len(frozen["entries"])
    done = [by_address[e["address"]] for e in frozen["entries"] if e["address"] in by_address]
    passed = [r for r in done if r.get("passed") is True]
    seconds = sum(float(r.get("seconds", 0)) for r in done)
    low, high = wilson(len(passed), total)
    return {
        "functions": total, "attempted": len(done), "correct": len(passed), "correct_rate": len(passed) / total,
        "correct_low": low, "correct_high": high,
        "functions_per_hour": len(passed) / (seconds / 3600) if seconds > 0 else 0.0,
        "cost_per_function": sum(float(r.get("cost", 0)) for r in done) / len(passed) if passed else None,
        "retries_per_success": sum(int(r.get("retries", 0)) for r in done) / len(passed) if passed else None,
    }


def record(frozen: dict[str, Any], results: list[Entry], label: str, prompt: str, n_impl: int) -> dict[str, Any]:
    """One line of the log: the measures plus what the run was (label, prompt hash, implementers, set hash)."""
    problems = check(frozen)
    if problems:
        raise ValueError("; ".join(problems))
    return {"set": frozen["hash"], "label": label, "prompt": hashlib.sha256(prompt.encode()).hexdigest()[:16],
            "n_impl": n_impl, **summarise(frozen, results)}


def compare(lines: list[Entry]) -> list[str]:
    """Judge each run against the one before it with the same label and set: better, worse or noise."""
    verdicts: list[str] = []
    last: dict[tuple[str, str], Entry] = {}
    for run in lines:
        before = last.get((run["label"], run["set"]))
        last[(run["label"], run["set"])] = run
        if before is None:
            continue
        if run["correct_low"] > before["correct_high"]:
            word = "better"
        elif run["correct_high"] < before["correct_low"]:
            word = "worse"
        else:
            word = "noise"
        verdicts.append(f"{run['label']}: {before['correct']}/{before['functions']} -> "
                        f"{run['correct']}/{run['functions']}: {word}")
    return verdicts


def main(argv: list[str] | None = None, root: Path = REPO, out: Any = sys.stdout) -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    pick = sub.add_parser("select")
    pick.add_argument("--count", type=int, default=25)
    pick.add_argument("--out")
    sub.add_parser("check").add_argument("set")
    gone = sub.add_parser("hide")
    gone.add_argument("set")
    gone.add_argument("address")
    rec = sub.add_parser("record")
    rec.add_argument("set")
    rec.add_argument("results")
    rec.add_argument("--label", required=True)
    rec.add_argument("--prompt", required=True)
    rec.add_argument("--n-impl", type=int, required=True)
    rec.add_argument("--out", required=True)
    sub.add_parser("compare").add_argument("log")
    args = parser.parse_args(argv)
    try:
        if args.command == "select":
            text = json.dumps(select(root, args.count), indent=1, sort_keys=True) + "\n"
            if args.out:
                Path(args.out).write_text(text, encoding="utf-8")
            print(text, file=out, end="")
            return 0
        if args.command == "compare":
            lines = [json.loads(x) for x in Path(args.log).read_text(encoding="utf-8").splitlines() if x.strip()]
            print("\n".join(compare(lines)), file=out)
            return 0
        frozen = json.loads(Path(args.set).read_text(encoding="utf-8"))
        if args.command == "check":
            problems = check(frozen)
            print("\n".join(problems) or f"ok: {len(frozen['entries'])} functions, hash {frozen['hash'][:12]}", file=out)
            return 1 if problems else 0
        if args.command == "hide":
            print("\n".join(hide(frozen, args.address)), file=out)
            return 0
        results = json.loads(Path(args.results).read_text(encoding="utf-8"))
        line = record(frozen, results, args.label, Path(args.prompt).read_text(encoding="utf-8"), args.n_impl)
        with Path(args.out).open("a", encoding="utf-8") as log:
            log.write(json.dumps(line, sort_keys=True) + "\n")
        print(json.dumps(line, sort_keys=True), file=out)
    except (ValueError, OSError, KeyError) as exc:
        print(f"port_benchmark: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
