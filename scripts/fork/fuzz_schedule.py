#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The scheduled family fuzz runs: one family a night, a fixed length, in the verifier's container.

    python3 -I scripts/fork/fuzz_schedule.py list
    python3 -I scripts/fork/fuzz_schedule.py next [--state FILE]
    python3 -I scripts/fork/fuzz_schedule.py run [FAMILY] [--seconds 600] [--state FILE] [--workdir DIR] [--repo OWNER/NAME]

A family can be fuzzed when its suite defines `pub fn replay_bytes(&[u8])` (see `fuzz_gate.py`). `next` names the one
whose last run is the oldest (a family never run comes first, in name order). `run` writes the family's fuzz crate
(`fuzz_gate.py scaffold`) under the repository's `target/fuzz-families/FAMILY`, then runs `cargo +NIGHTLY fuzz run` for
`--seconds` in the verifier's container (`verifier.py`: capped memory, no network, the pinned nightly and cargo-fuzz).
A crash is minimised (`cargo fuzz tmin`) and the report holds it as a byte-literal test, the text `fuzz_gate.py to-test`
would append to the suite, so the finding is a permanent case and the corpus is never committed.

The result is one JSON report on stdout and in the state file, and the exit status says it: 0 clean, 1 a finding, 3 the
run itself failed (the image, the build or the container: nothing was learned about the family). With `--repo` a finding
opens an issue with the test in its body, and a failed run opens an issue of its own, so a scheduled run that cannot run
is reported and not silent (an open issue with the same title is not opened twice). Without `--repo` nothing is sent.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any, TextIO

FORK = Path(__file__).resolve().parent
REPO = FORK.parent.parent
sys.path.insert(0, str(FORK))
import family_lab
import fuzz_gate as _fuzz_gate
import verifier as _verifier

fuzz_gate: Any = _fuzz_gate  # the fork scripts are untyped; what they return is checked by their own tests
verifier: Any = _verifier

SECONDS = 600
BUILD_ALLOWANCE = 3600
MAX_CRASHES = 1
# A dev build with no sanitizer: the release build with AddressSanitizer of the first family needed more than 20 GB (killed
# at its cap), this one 6.5 GB and a minute. The code under test is safe Rust, so memory errors are not what it looks for;
# the dev profile keeps overflow checks and debug assertions, which a differential run wants.
BUILD_FLAGS = "--dev --sanitizer none"
REPLAY = re.compile(r"^pub fn replay_bytes\(", re.MULTILINE)
Report = dict[str, Any]
Runner = Callable[[list[str]], tuple[int, str]]
Container = Callable[..., Report]


def run(argv: list[str]) -> tuple[int, str]:
    """(exit status, combined output); a missing program is status 127."""
    try:
        done = subprocess.run(argv, text=True, errors="replace", stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              check=False)
    except OSError as exc:
        return 127, f"{argv[0]}: {exc}"
    return done.returncode, done.stdout


def families(root: Path) -> list[str]:
    """The families whose suite defines `replay_bytes`, in name order."""
    found: list[str] = []
    for suite in sorted((root / "crates" / "engine" / "tests").glob("typed_*.rs")):
        if REPLAY.search(suite.read_text(encoding="utf-8", errors="replace")):
            found.append(suite.stem.removeprefix("typed_"))
    return found


def load_state(path: Path) -> dict[str, Any]:
    """The state file's families; an absent or unreadable file is an empty state."""
    try:
        data: Any = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}  # pyright: ignore[reportUnknownVariableType]


def save_state(path: Path, state: dict[str, Any]) -> None:
    """Write the state file whole or not at all."""
    path.parent.mkdir(parents=True, exist_ok=True)
    handle, name = tempfile.mkstemp(dir=path.parent, prefix=".fuzz-state-")
    with os.fdopen(handle, "w", encoding="utf-8") as out:
        out.write(json.dumps(state, indent=1, sort_keys=True) + "\n")
    os.replace(name, path)


def next_family(candidates: list[str], state: dict[str, Any]) -> str | None:
    """The family whose last run is the oldest; one never run comes first."""
    if not candidates:
        return None
    return min(candidates, key=lambda family: (float(state.get(family, {}).get("last", 0)), family))


def crash_files(crate: Path, family: str) -> list[Path]:
    """The crash inputs libFuzzer left for the family, oldest name first."""
    return sorted(path for path in (crate / "artifacts" / family).glob("*") if path.is_file()
                  and not path.name.startswith("minimized-"))


def container_command(family: str, seconds: int, crate: Path) -> list[str]:
    """The command run in the container: the fuzz run, then the minimisation of the first crash input it left.

    One container does both, because its build directory is thrown away with it and a second container would build
    everything again. The run's own exit status is the command's.
    """
    nightly = f"+{fuzz_gate.FUZZ_NIGHTLY}"
    script = (f"cd {crate} && timeout {seconds + BUILD_ALLOWANCE} cargo {nightly} fuzz run {BUILD_FLAGS} --fuzz-dir {crate} {family} -- "
              f"-max_total_time={seconds} -rss_limit_mb=4096; status=$?; "
              f"f=$(ls artifacts/{family}/crash-* 2>/dev/null | head -n 1); "
              f"[ -n \"$f\" ] && timeout 900 cargo {nightly} fuzz tmin {BUILD_FLAGS} --fuzz-dir {crate} {family} \"$f\"; "
              "exit $status")
    return ["bash", "-c", script]


def finding(family: str, data: bytes) -> Report:
    """A crash input as the byte-literal test that keeps it: its name, its bytes and the text of the test."""
    name = f"regression_{hashlib.sha256(data).hexdigest()[:12]}"
    test = (f"#[test]\nfn {name}() {{\n    let bytes: &[u8] = {fuzz_gate.byte_literal(data)};\n"
            "    replay_bytes(bytes);\n}\n")
    return {"test": name, "bytes": len(data), "hex": data.hex(), "test_text": test, "family": family}


def fuzz(root: Path, family: str, seconds: int, workdir: Path, container: Container, now: Callable[[], float],
         memory: str = "12g", lab: Any = family_lab) -> Report:
    """One scheduled run of one family; the report (always a dict, whatever went wrong)."""
    started = now()
    report: Report = {"family": family, "seconds": seconds, "started": started, "status": "failed", "findings": []}
    crate = root / "target" / "fuzz-families" / family
    shutil.rmtree(crate, ignore_errors=True)
    seed = workdir / "empty-seed"
    seed.mkdir(parents=True, exist_ok=True)
    try:
        fuzz_gate.scaffold(root, family, crate, lab)
        record = container(root, seed, container_command(family, seconds, crate), memory=memory, workdir=workdir)
    except (ValueError, OSError, verifier.VerifierError) as exc:
        report["detail"] = str(exc)[:300]
        return finish(report, now, crate)
    report["container"] = {k: record.get(k) for k in ("image", "run_seconds", "exit", "peak_bytes", "sccache_hits")}
    crashes = crash_files(crate, family)
    if crashes:
        report["status"] = "finding"
        for crash in crashes[:MAX_CRASHES]:
            report["findings"].append(finding(family, minimised(crash)))
    elif record.get("exit") == 0 and record.get("container_left") is False:
        report["status"] = "clean"
    else:
        report["detail"] = str(record.get("output_tail", ""))[-300:] or f"exit status {record.get('exit')}"
    return finish(report, now, crate)


def minimised(crash: Path) -> bytes:
    """The crash input after `cargo fuzz tmin` (`minimized-from-X` for `crash-X`); the input itself when it wrote nothing."""
    smaller = crash.parent / f"minimized-from-{crash.name.removeprefix('crash-')}"
    return (smaller if smaller.is_file() else crash).read_bytes()


def finish(report: Report, now: Callable[[], float], crate: Path) -> Report:
    """Stamp the report and remove the generated crate (the artifacts live on in the report, as hex)."""
    report["finished"] = now()
    shutil.rmtree(crate, ignore_errors=True)
    return report


def exit_status(report: Report) -> int:
    """0 clean, 1 a finding, 3 the run itself failed."""
    return {"clean": 0, "finding": 1}.get(str(report["status"]), 3)


def issue_text(report: Report) -> tuple[str, str]:
    """(title, body) of the issue a report asks for."""
    family = report["family"]
    if report["status"] == "finding":
        first = report["findings"][0]
        title = f"fuzz: the {family} family has a mismatch ({first['test']})"
        body = (f"The scheduled fuzz run of the `{family}` family ({report['seconds']} s) found an input on which the typed "
                f"code and its translation differ ({first['bytes']} bytes, minimised).\n\nAdd this case to "
                f"`crates/engine/tests/typed_{family}.rs`; it must fail first, and the fix makes it pass:\n\n"
                f"```rust\n{first['test_text']}```\n\nThe input as hex: `{first['hex']}`\n")
    else:
        title = f"fuzz: the scheduled run of the {family} family could not run"
        body = (f"The scheduled fuzz run of the `{family}` family failed before it could tell anything about the "
                f"family.\n\n```\n{report.get('detail', '')}\n```\n")
    return title, body


def open_issue(report: Report, repo: str, runner: Runner = run) -> str:
    """Open the issue of a finding or a failed run unless one with the title is open; what was done."""
    title, body = issue_text(report)
    code, listing = runner(["gh", "issue", "list", "--repo", repo, "--state", "open", "--search", f'"{title}" in:title',
                            "--json", "title", "--jq", ".[].title"])
    if code == 0 and title in listing.splitlines():
        return "exists"
    code, output = runner(["gh", "issue", "create", "--repo", repo, "--title", title, "--body", body])
    return "opened" if code == 0 else f"not opened: {output.strip()[-200:]}"


def main(argv: list[str] | None = None, root: Path = REPO, out: TextIO = sys.stdout, container: Container = verifier.verify,
         runner: Runner = run, now: Callable[[], float] = time.time, lab: Any = family_lab) -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    state_default = str(Path.home() / ".cache" / "nv-workers" / "fuzz" / "state.json")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("list")
    nxt = sub.add_parser("next")
    nxt.add_argument("--state", default=state_default)
    one = sub.add_parser("run")
    one.add_argument("family", nargs="?")
    one.add_argument("--seconds", type=int, default=SECONDS)
    one.add_argument("--state", default=state_default)
    one.add_argument("--workdir")
    one.add_argument("--memory", default="12g")
    one.add_argument("--repo")
    args = parser.parse_args(argv)
    try:
        candidates = families(root)
        if args.command == "list":
            print(json.dumps(candidates), file=out)
            return 0
        state_path = Path(args.state)
        state = load_state(state_path)
        if args.command == "next":
            print(json.dumps(next_family(candidates, state)), file=out)
            return 0
        family = args.family or next_family(candidates, state)
        if family not in candidates:
            raise ValueError(f"{family!r} has no `replay_bytes` in its suite: nothing to fuzz")
        if args.seconds < 1:
            raise ValueError("--seconds is at least 1")
        workdir = Path(args.workdir) if args.workdir else state_path.parent
        report = fuzz(root, family, args.seconds, workdir, container, now, args.memory, lab)
        if args.repo and report["status"] != "clean":
            report["issue"] = open_issue(report, args.repo, runner)
        state[family] = {"last": report["finished"], "status": report["status"],
                         "findings": len(report["findings"])}
        save_state(state_path, state)
        print(json.dumps(report, indent=1, sort_keys=True), file=out)
        return exit_status(report)
    except (ValueError, OSError) as exc:
        print(f"fuzz_schedule: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
