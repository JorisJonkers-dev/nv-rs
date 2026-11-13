#!/usr/bin/env python3
"""A job summary from the text `cargo test` prints (libtest): per test binary, the failed tests and the seconds.

  python3 -I scripts/fork/libtest_summary.py --title "Component: core (Windows)" --timings timings.txt test.log >> "$GITHUB_STEP_SUMMARY"

Reads the log (and the timings file scripts/fork/run-component.sh writes: lines "compile N", "test N", "other N"),
writes Markdown to stdout. Never decides the result. The test counts are what the component ran; the sum over
all components must equal what `cargo test --workspace` runs (docs/fork/CI.md). Stdlib only.
"""
import argparse
import re
import sys

RUN = re.compile(r"^\s*(Running|Doc-tests)\s+(.*?)\s*$")
RESULT = re.compile(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored;.*finished in ([\d.]+)s")
FAILED = re.compile(r"^test (\S+) \.\.\. FAILED")
STAMP = re.compile(r"^\d{4}-\d\d-\d\dT[\d:.]+Z ")


def timings(path):
    """{"compile": s, "test": s, "other": s} summed from run-component.sh's file; empty without one."""
    out = {}
    try:
        with open(path, encoding="utf-8") as fh:
            for line in fh:
                kind, _, n = line.partition(" ")
                if n.strip().isdigit():
                    out[kind] = out.get(kind, 0) + int(n)
    except (OSError, TypeError):
        pass
    return out


def minutes(seconds):
    return "-" if seconds is None else f"{seconds / 60:.1f} min"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("log")
    ap.add_argument("--title", default="Tests")
    ap.add_argument("--timings")
    a = ap.parse_args()
    rows, failed, cur = [], [], None
    with open(a.log, encoding="utf-8", errors="replace") as fh:
        lines = fh.read().splitlines()
    for line in lines:
        line = STAMP.sub("", line.rstrip("\n"))
        m = RUN.match(line)
        if m:
            kind, rest = m.groups()
            cur = ("doctests " + rest) if kind == "Doc-tests" else re.sub(r" \(.*\)$", "", rest)
            continue
        m = FAILED.match(line)
        if m:
            failed.append((cur or "?", m.group(1)))
            continue
        m = RESULT.search(line)
        if m and cur:
            rows.append((cur, m.group(1), int(m.group(2)), int(m.group(3)), int(m.group(4)), float(m.group(5))))
            cur = None
    tot = [sum(r[i] for r in rows) for i in (2, 3, 4)]
    secs = sum(r[5] for r in rows)
    phases = timings(a.timings)
    print(f"### {a.title}\n")
    print("| Test binaries | Passed | Failed | Ignored | Compile (measured) | Tests (measured) | Tests (sum of libtest) |\n"
          "| ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    print(f"| {len(rows)} | {tot[0]} | {tot[1]} | {tot[2]} | {minutes(phases.get('compile'))} | {minutes(phases.get('test'))} | {secs / 60:.1f} min |\n")
    if failed:
        print("**Failed**\n")
        for binary, name in failed:
            print(f"- `{name}` in `{binary}`")
        print()
    big = sorted((r for r in rows if r[5] >= 5), key=lambda r: -r[5])[:12]
    if big:
        print("<details><summary>Slowest test binaries</summary>\n\n| Binary | Tests | Time |\n| --- | ---: | ---: |")
        for r in big:
            print(f"| `{r[0]}` | {r[2]} | {r[5]:.0f} s |")
        print("\n</details>")
    return 0


if __name__ == "__main__":
    sys.exit(main())
