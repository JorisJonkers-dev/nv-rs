#!/usr/bin/env python3
"""Fork-only. Reads libtest output and prints how many gated tests executed and how many skipped.

The gate (scripts/fork/test_gate.rs) prints `executed: <test> (VAR set)` or `skipped: <test> (VAR not set)`;
libtest shows a passing test's output with `-- --nocapture` or `--show-output`. A private job runs

    NV_EXE=... cargo test ... -- --nocapture 2>&1 | python3 -I scripts/fork/gate_counts.py --require

and the script fails (exit 1) when a gated test skipped, or when none executed. Without --require it only
reports. Test names and counts are printed, never paths. Reads stdin or the files named.
"""
import re
import sys

LINE = re.compile(r"^(executed|skipped): (\S+)(?: \((NV_[A-Z_]+) (?:not )?set\))?\s*$")


def count(lines):
    executed, skipped = {}, {}
    for raw in lines:
        m = LINE.match(raw.strip())
        if not m:
            continue
        table = executed if m.group(1) == "executed" else skipped
        table[m.group(2)] = m.group(3) or ""
    return executed, skipped


def main(argv):
    require = "--require" in argv
    files = [a for a in argv if not a.startswith("--")]
    lines = []
    for f in files:
        with open(f, encoding="utf-8", errors="replace") as h:
            lines.extend(h)
    if not files:
        lines = sys.stdin
    executed, skipped = count(lines)
    print(f"gated tests: {len(executed)} executed, {len(skipped)} skipped")
    for name in sorted(skipped):
        print(f"  skipped: {name}")
    if require and (skipped or not executed):
        why = "a gated test skipped" if skipped else "no gated test executed (was --nocapture given?)"
        print(f"failed: {why}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
