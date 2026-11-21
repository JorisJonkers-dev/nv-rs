#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The layout gate of an upstream sync (nv-rs#272): a change to a layout constant of `engine-core/src/mem.rs` (where the
guest heap, the platform area, the extenders' code and the helper area lie) must pass the window test before it merges.

  layout_gate.py changed --base REF --head REF     the layout constants whose definition differs, one a line (none: nothing)
  layout_gate.py check   --base REF --head REF     the same, as a GitHub summary; exit 0 either way, prints `changed=true|false`
                                                   to $GITHUB_OUTPUT when set, so a job can run the window test only then

The window test is `cargo test -p engine-core --lib -- mem::heap::tests::`: the heap's tests at lowered windows
(`NV_GUEST_WINDOW`, docs/fork/ADDRESS-SPACE.md) state the ranges' sizes, so a moved constant fails them. Stdlib only.
"""
import argparse
import os
import re
import subprocess
import sys

FILE = "crates/engine-core/src/mem.rs"
CONSTANTS = (
    "HEAP_BASE", "HEAP_LOW_END", "HEAP_HIGH_BASE", "HEAP_END", "PLATFORM_AREA", "EXTENDER_CODE", "BUILTIN_DATA",
    "HELPER_AREA", "HELPER_SLOT", "HEAP_LARGEST_BLOCK",
)


def definition(text, name):
    """The text of `pub const NAME ...;` (to the line that ends in `;`, comments and blanks left out), or None."""
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if re.match(rf"\s*pub\s+const\s+{name}\b", line):
            out = []
            for l in lines[i:]:
                l = re.sub(r"//.*", "", l).rstrip()
                if l.strip():
                    out.append(l.strip())
                if l.endswith(";"):
                    break
            return " ".join(out)
    return None


def changed_constants(before, after):
    """The names in CONSTANTS whose definition differs between two texts of mem.rs (added, removed or edited). Pure."""
    return [n for n in CONSTANTS if definition(before, n) != definition(after, n)]


def show(ref):
    done = subprocess.run(["git", "show", f"{ref}:{FILE}"], capture_output=True, text=True, check=False)
    return done.stdout if done.returncode == 0 else ""


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=["changed", "check"])
    ap.add_argument("--base", required=True)
    ap.add_argument("--head", required=True)
    a = ap.parse_args(argv)
    names = changed_constants(show(a.base), show(a.head))
    if a.cmd == "changed":
        print("\n".join(names))
        return 0
    text = ("The layout constants of mem.rs changed: " + ", ".join(f"`{n}`" for n in names) + ". The window test must pass."
            if names else "No layout constant of mem.rs changed.")
    print(text)
    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with open(out, "a", encoding="utf-8") as fh:
            fh.write(f"changed={'true' if names else 'false'}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
