#!/usr/bin/env python3
"""Fork-only. Counts the d3d9-wgpu GPU tests that ran against those that skipped, from the log of a Linux component job.

The GPU tests (crates/d3d9-wgpu/tests/frame.rs, ops.rs) print "no GPU adapter: ... skipped" and pass when no adapter can
be opened. This reads the log of `cargo test` run with RUST_TEST_NOCAPTURE=1 (so those lines are in it), prints the
executed and skipped counts as a Markdown table, and exits 1 when any test skipped or no GPU test binary ran.

Usage: gpu_gate.py LOG
"""
import re
import sys

GPU_BINARIES = ("frame", "ops")
RUNNING = re.compile(r"^\s*Running (?:unittests )?(?:tests/(\w+)\.rs|(\S+)) ")
RESULT = re.compile(r"^test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored")
SKIP = re.compile(r"no GPU adapter: .*skipped")


def main(path):
    binary, passed, skipped, per_skip = None, {}, 0, {}
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            m = RUNNING.match(line)
            if m:
                binary = m.group(1)
                continue
            if SKIP.search(line):
                skipped += 1
                per_skip[binary] = per_skip.get(binary, 0) + 1
                continue
            m = RESULT.match(line)
            if m and binary in GPU_BINARIES:
                passed[binary] = passed.get(binary, 0) + int(m.group(1))
    total = sum(passed.values())
    print("### GPU tests on the software Vulkan adapter (lavapipe)\n")
    print("| Test binary | Passed | Skipped (no adapter) |\n| --- | ---: | ---: |")
    for b in GPU_BINARIES:
        print(f"| {b} | {passed.get(b, 0)} | {per_skip.get(b, 0)} |")
    print(f"\nExecuted on a device: {max(total - skipped, 0)}; skipped: {skipped}.")
    if total == 0:
        print("::error title=GPU tests::no GPU test binary ran")
        return 1
    if skipped:
        print(f"::error title=GPU tests::{skipped} GPU test(s) skipped for want of an adapter")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
