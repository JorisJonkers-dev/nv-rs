#!/usr/bin/env python3
"""Fork-only: turns `cargo test` logs into a Markdown table (one row per crate, or per test binary).

usage: linux_test_summary.py LOGDIR   (every *.log file in it is one crate's cargo test output)
Columns: passed, failed, ignored over every `test result:` line, and `build` when the crate did not compile.
"""
import pathlib
import re
import sys

RESULT = re.compile(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored")
RUNNING = re.compile(r"^\s*Running (.+?) \(")
FAILED = re.compile(r"^test (\S+) - should panic \.\.\. FAILED|^test (\S+) \.\.\. FAILED")


def main() -> None:
    print("| crate | passed | failed | ignored | note |")
    print("| --- | ---: | ---: | ---: | --- |")
    failures = []
    for log in sorted(pathlib.Path(sys.argv[1]).glob("*.log")):
        name = log.stem
        text = log.read_text(errors="replace")
        passed = failed = ignored = 0
        seen = False
        binary = ""
        per_binary = {}
        for line in text.splitlines():
            m = RUNNING.match(line)
            if m:
                binary = m.group(1)
            m = RESULT.search(line)
            if m:
                seen = True
                p, f, i = int(m.group(2)), int(m.group(3)), int(m.group(4))
                passed, failed, ignored = passed + p, failed + f, ignored + i
                row = per_binary.setdefault(binary or "doc-tests", [0, 0, 0])
                row[0] += p
                row[1] += f
                row[2] += i
            m = FAILED.match(line)
            if m:
                failures.append(f"{name}: {m.group(1) or m.group(2)}")
        note = ""
        if "could not compile" in text:
            note = "build failed"
        elif not seen:
            note = "no result (timed out or killed)"
        print(f"| {name} | {passed} | {failed} | {ignored} | {note} |")
        if name == "engine":
            for b, (p, f, i) in per_binary.items():
                print(f"| &nbsp;&nbsp;{b} | {p} | {f} | {i} | |")
    if failures:
        print("\n### Failed tests (first 200)\n```")
        print("\n".join(failures[:200]))
        print("```")


main()
