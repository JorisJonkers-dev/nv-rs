#!/usr/bin/env python3
"""Fork-only. Fails a Linux component job whose shader compiler tests skipped for want of libvkd3d-shader.

The tests of crates/d3dx-shader and engine-platform's d3dxshader print "executed: <test> (<version>)" when they compile
with libvkd3d-shader and "skipped: <test> (libvkd3d-shader not available)" when they cannot load it (and fail instead
when NV_REQUIRE_VKD3D=1, which the flake's ci shell sets). Reads the log of `cargo test` run with RUST_TEST_NOCAPTURE=1,
prints the counts and the library version as Markdown, exits 1 when any test skipped or none executed.

Usage: vkd3d_gate.py LOG
"""
import re
import sys

ANSI = re.compile(r"\x1b\[[0-9;]*[a-zA-Z]")
EXECUTED = re.compile(r"executed: \S+ \((.+)\)\s*$")
SKIPPED = re.compile(r"skipped: \S+ \(libvkd3d-shader not available\)")


def main(path):
    executed, skipped, versions = 0, 0, set()
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = ANSI.sub("", line)
            m = EXECUTED.search(line)
            if m:
                executed += 1
                versions.add(m.group(1))
            elif SKIPPED.search(line):
                skipped += 1
    print("### Shader compiler tests (libvkd3d-shader)\n")
    print(f"Executed with the library: {executed} ({', '.join(sorted(versions)) or 'no version'}); skipped: {skipped}.")
    if skipped:
        print(f"::error title=vkd3d tests::{skipped} test(s) skipped for want of libvkd3d-shader")
        return 1
    if executed == 0:
        print("::error title=vkd3d tests::no test compiled with libvkd3d-shader")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
