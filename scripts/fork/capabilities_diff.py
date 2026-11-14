#!/usr/bin/env python3
"""Fork-only. Which commands a regenerated .fork/capabilities.json adds or removes.

  capabilities_diff.py OLD NEW      markdown for the sync pull request; prints nothing when the commands did not change
"""
import json
import sys


def commands(doc):
    return {e["id"]: (e.get("name", e["id"]), list(e["commands"]["implemented_names"]), e["commands"].get("total")) for e in doc["extenders"]}


def diff(old, new):
    o, n = commands(old), commands(new)
    out = []
    for ident in sorted(set(o) | set(n)):
        name = n.get(ident, o.get(ident))[0]
        before, after = o.get(ident, (name, [], None)), n.get(ident, (name, [], None))
        # A command at two opcodes is named twice: compare as counted names.
        b, a = {}, {}
        for x in before[1]:
            b[x] = b.get(x, 0) + 1
        for x in after[1]:
            a[x] = a.get(x, 0) + 1
        added = sorted(k for k in a if a[k] > b.get(k, 0))
        removed = sorted(k for k in b if b[k] > a.get(k, 0))
        if not added and not removed and before[2] == after[2]:
            continue
        out.append(f"- {name}: {len(before[1])} to {len(after[1])} implemented" + (f", total {before[2]} to {after[2]}" if before[2] != after[2] else ""))
        if added:
            out.append(f"  - added ({len(added)}): " + ", ".join(f"`{x}`" for x in added))
        if removed:
            out.append(f"  - removed ({len(removed)}): " + ", ".join(f"`{x}`" for x in removed))
    return "\n".join(out)


def main(argv):
    with open(argv[1], encoding="utf-8") as fh:
        old = json.load(fh)
    with open(argv[2], encoding="utf-8") as fh:
        new = json.load(fh)
    text = diff(old, new)
    if text:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
