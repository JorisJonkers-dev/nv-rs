#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The fuzz gate of a porting attempt: saved regression cases, findings as byte-literal tests, the fuzz crate.

    python3 -I scripts/fork/fuzz_gate.py check <family>
    python3 -I scripts/fork/fuzz_gate.py to-test <family> <input-file> [--suite FILE]
    python3 -I scripts/fork/fuzz_gate.py scaffold <family> <out-dir>

`check` is the per-attempt part: it runs the family's saved regression cases (the tests of its differential suite
named `regression_*`) through the family lab, which sends cargo to the build daemon, and fails on a failing case or
on a binary corpus or crash artifact tracked in git. The deterministic comparison sweep is the suite itself
(`Diff` cases and the float sweep), which `port_check.py` already runs. One JSON verdict on stdout.

`to-test` turns one minimised crash input into a byte-literal test appended to the family's suite, named after the
input's hash, so the same input is never added twice. The suite must define `pub fn replay_bytes(&[u8])`, which
decodes the bytes the way the fuzz target does and runs the comparison. An input over 512 bytes is refused: minimise
it first (`cargo fuzz tmin`). Binary corpora are never committed (repo_hygiene).

`scaffold` writes a small cargo-fuzz workspace for one family: it depends on engine-core (feature test-support) and
the suite's replay function, and is built with the pinned nightly (FUZZ_NIGHTLY). It is its own workspace, like
fuzz/, so libfuzzer-sys never reaches the root workspace. Its dependencies are the lab's: the suite's `use engine as _;`
(the whole facade, every part crate) is replaced by the assembly of the parts the family's cases reach, as in
`family_lab.py`, so the fuzz build compiles the few parts of one family and not the engine.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

FORK = Path(__file__).resolve().parent
sys.path.insert(0, str(FORK))
import family_lab

REPO = FORK.parent.parent
FAMILY_NAME = re.compile(r"^[A-Za-z0-9_]+$")
MAX_INPUT = 512
FUZZ_NIGHTLY = "nightly-2026-11-01"
CORPUS_PATH = re.compile(r"(^|/)(corpus|artifacts)/")
TEST_RESULT = re.compile(r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed", re.MULTILINE)
FAILED_CASE = re.compile(r"^\s*test\s+(\S*regression_\S*)\s+\.\.\.\s+FAILED\b", re.MULTILINE)


def run(argv, cwd):
    """(exit status, combined output) of one command; a missing program is status 127."""
    try:
        done = subprocess.run([str(a) for a in argv], cwd=str(cwd), text=True, errors="replace",
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    except OSError as exc:
        return 127, f"{argv[0]}: {exc}"
    return done.returncode, done.stdout


def finding(rule, file, line, message, fix):
    return {"rule": rule, "file": str(file), "line": line, "message": message, "fix": fix}


def suite_path(root, family):
    if not FAMILY_NAME.match(family):
        raise ValueError(f"not a family name: {family!r}")
    return Path(root) / "crates" / "engine" / "tests" / f"typed_{family}.rs"


def tracked_corpora(root, runner):
    """Findings for corpus or crash-artifact files tracked in git (binary corpora are never committed)."""
    code, output = runner(["git", "ls-files", "--", "fuzz", "crates"], root)
    if code:
        return [finding("git", ".", 1, "git ls-files failed: " + output.strip()[-200:], "run in a git checkout")]
    return [finding("fuzz-corpus", f, 1, "a fuzz corpus or crash artifact is tracked", "delete it; keep the case as a "
                    "byte-literal test (fuzz_gate.py to-test)")
            for f in output.splitlines() if CORPUS_PATH.search(f)]


def check(root, family, runner=run):
    """The per-attempt fuzz gate: the saved regression cases, and no tracked corpus."""
    suite = suite_path(root, family)
    rel = suite.relative_to(root).as_posix()
    failures = tracked_corpora(root, runner)
    code, output = runner([sys.executable, FORK / "family_lab.py", "run", family, "regression_"], root)
    totals = [(int(p), int(f)) for _, p, f in TEST_RESULT.findall(output)]
    passed, failed = sum(p for p, _ in totals), sum(f for _, f in totals)
    failures += [finding("fuzz-regression", rel, 1, f"saved case {name} fails", "fix the typed code; never edit the case")
                 for name in FAILED_CASE.findall(output)]
    if failed and not failures:  # a FAILED summary counts even when the exit status says 0
        failures.append(finding("fuzz-regression", rel, 1, f"{failed} saved case(s) fail", "fix the typed code; never edit the case"))
    if code and not failures:
        last = [x for x in output.strip().splitlines() if x.strip()]
        failures.append(finding("fuzz-regression", rel, 1, last[-1][:300] if last else f"exit status {code}",
                                "read the lab output above the first error"))
    return {"family": family, "regression_cases": passed + failed, "pass": not failures, "failures": failures}


def byte_literal(data):
    """A Rust byte-slice literal, 12 bytes to a line."""
    rows = [", ".join(f"0x{b:02x}" for b in data[i:i + 12]) for i in range(0, len(data), 12)]
    return "&[\n" + "".join(f"        {row},\n" for row in rows) + "    ]"


def require_replay(path):
    """The suite must define `pub fn replay_bytes`: the fuzz target and the tests reach it from outside the module."""
    if not re.search(r"^pub fn replay_bytes\(", Path(path).read_text(encoding="utf-8"), re.MULTILINE):
        raise ValueError(f"{path} defines no `pub fn replay_bytes(&[u8])`")


def to_test(root, family, data, suite=None):
    """Append the byte-literal test of `data` to the family's suite; returns (test name, whether it was new)."""
    if not data:
        raise ValueError("the input is empty")
    if len(data) > MAX_INPUT:
        raise ValueError(f"the input is {len(data)} bytes (limit {MAX_INPUT}): minimise it with cargo fuzz tmin")
    path = Path(suite) if suite else suite_path(root, family)
    require_replay(path)
    name = f"regression_{hashlib.sha256(data).hexdigest()[:12]}"
    text = path.read_text(encoding="utf-8")
    if re.search(rf"^fn {name}\(\)", text, re.MULTILINE):
        return name, False
    test = (f"\n/// A case the fuzz run of the `{family}` family found (minimised); it must keep passing.\n"
            f"#[test]\nfn {name}() {{\n    let bytes: &[u8] = {byte_literal(data)};\n    replay_bytes(bytes);\n}}\n")
    path.write_text(text.rstrip("\n") + "\n" + test, encoding="utf-8")
    return name, True


def scaffold(root, family, out, lab=family_lab):
    """Write the family's cargo-fuzz workspace under `out`; returns the files written.

    `lab` is the family lab: it says which parts the suite's cases reach and rewrites the suite's facade line.
    """
    suite = suite_path(root, family)
    require_replay(suite)
    out = Path(out)
    up = os.path.relpath(Path(root).resolve(), out.resolve()).replace(os.sep, "/") + "/"
    parts = lab.assembly_for(suite)
    text = lab.rewrite_source(suite.read_text(encoding="utf-8"), parts)
    dependencies = "".join(f'[dependencies.{name}]\npath = "{up}crates/{name}"\n{options}\n'
                           for name, options in [("engine-core", 'features = ["test-support"]'), ("engine-typed", ""),
                                                 *[(part, "") for part in parts]])
    files = {
        "Cargo.toml": (
            f'[package]\nname = "nv-rs-fuzz-{family}"\nversion = "0.0.0"\nedition = "2021"\npublish = false\n\n'
            '[package.metadata]\ncargo-fuzz = true\n\n[workspace]\n\n[dependencies]\nlibfuzzer-sys = "0.4"\n\n'
            f'{dependencies}\n[[bin]]\nname = "{family}"\npath = "fuzz_targets/{family}.rs"\ntest = false\ndoc = false\n'),
        f"fuzz_targets/{family}.rs": (
            "// SPDX-License-Identifier: GPL-3.0-only\n#![no_main]\n#![allow(dead_code, unused_imports)]\n"
            f"//! Fuzz target of the `{family}` family: the bytes are replayed through the suite's own comparison.\n"
            "use libfuzzer_sys::fuzz_target;\n\nmod suite;\n\n"
            "fuzz_target!(|data: &[u8]| suite::replay_bytes(data));\n"),
        "fuzz_targets/suite.rs": text,
        "README.md": (f"Fuzz crate of the `{family}` family (generated by scripts/fork/fuzz_gate.py).\n\n"
                      f"    cargo +{FUZZ_NIGHTLY} fuzz run {family} -- -max_total_time=600\n\n"
                      "A finding is minimised (`cargo fuzz tmin`) and added with `fuzz_gate.py to-test`; the corpus and "
                      "the artifacts are never committed.\n"),
    }
    for name in lab.INCLUDE.findall(text):  # a table the suite includes goes beside the copy
        included = suite.parent / name
        if not included.is_file():
            raise ValueError(f"{suite.name} includes {name!r}, which is not beside it")
        files[f"fuzz_targets/{name}"] = included.read_text(encoding="utf-8", errors="replace")
    for rel, content in files.items():
        target = out / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")
    return sorted(files)


def main(argv=None, root=REPO, runner=run, out=sys.stdout, lab=family_lab):
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("check").add_argument("family")
    one = sub.add_parser("to-test")
    one.add_argument("family")
    one.add_argument("input")
    one.add_argument("--suite")
    sc = sub.add_parser("scaffold")
    sc.add_argument("family")
    sc.add_argument("out_dir")
    args = parser.parse_args(argv)
    try:
        if args.command == "check":
            result = check(root, args.family, runner)
            print(json.dumps(result, indent=2, sort_keys=True), file=out)
            return 0 if result["pass"] else 1
        if args.command == "to-test":
            name, new = to_test(root, args.family, Path(args.input).read_bytes(), args.suite)
            print(json.dumps({"test": name, "added": new}), file=out)
            return 0
        print(json.dumps({"written": scaffold(root, args.family, args.out_dir, lab)}), file=out)
        return 0
    except (ValueError, OSError) as exc:
        print(f"fuzz_gate: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
