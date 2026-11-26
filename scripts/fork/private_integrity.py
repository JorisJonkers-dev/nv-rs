#!/usr/bin/env python3
"""Fork-only. One hash over the porting check a private run must not accept a change to.

The four groups are the judge (crates/engine-core/src/typed/diff.rs), the thresholds it uses, the
planted-bug and byte-matching controls, and the ledger generator. A change to any one of them changes
the hash, so the stored hash has to be updated in the same pull request (nv-rs#916, story 24). The
private run calls this before any test; a worker cannot pass by weakening its own check.

Run: python3 -I scripts/fork/private_integrity.py [--check|--print]
"""
import argparse
import hashlib
import sys
from pathlib import Path

FORK = Path(__file__).resolve().parent
REPO = FORK.parent.parent
STORED = "private-integrity.sha256"

# The groups are named so that moving a file between them also changes the hash. Keep this list
# explicit: a new file in one of these groups must be added here, and the hash updated with it.
GROUPS = {
    "judge": (
        "crates/engine-core/src/typed/diff.rs",
    ),
    "thresholds": (
        "crates/engine-core/src/typed/mod.rs",
        "crates/engine-core/src/x87.rs",
    ),
    "controls": (
        "crates/engine/tests/diff_reference_peer.rs",
    ),
    "ledger generator": (
        "scripts/ledger/Cargo.toml",
        "scripts/ledger/Cargo.lock",
        "scripts/ledger/function_evidence.tsv",
        "scripts/ledger/fallout_crates.tsv",
        "scripts/ledger/src/main.rs",
        "scripts/ledger/src/units.rs",
        "scripts/ledger/src/typed_marker.rs",
    ),
}


def digest(root: Path) -> str:
    """One SHA-256 over the protected files, in a fixed order, independent of line endings."""
    h = hashlib.sha256()
    for group, paths in GROUPS.items():
        h.update(group.encode("utf-8") + b"\0")
        for rel in paths:
            data = (root / rel).read_bytes().replace(b"\r\n", b"\n")
            h.update(rel.encode("utf-8") + b"\0" + str(len(data)).encode("ascii") + b"\0" + data)
    return h.hexdigest()


def check(root: Path) -> tuple[bool, str]:
    """Compare the hash with the stored one. Never prints a path into a build log."""
    stored_file = root / ".fork" / STORED
    if not stored_file.is_file():
        return False, f"missing {stored_file.name}: the checker hash was never stored"
    stored = stored_file.read_text(encoding="utf-8").strip()
    try:
        actual = digest(root)
    except FileNotFoundError as exc:
        rel = Path(exc.filename).relative_to(root).as_posix()
        group = next((g for g, paths in GROUPS.items() if rel in paths), "protected files")
        return False, f"checker hash cannot be computed: missing {rel} from {group}"
    if stored != actual:
        return False, (
            f"checker hash changed: stored {stored}, found {actual}. A change to the judge, its "
            f"thresholds, its controls or the ledger generator must update .fork/{STORED} in the "
            "same change (python3 -I scripts/fork/private_integrity.py --print)."
        )
    return True, f"checker integrity: {actual}"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", type=Path, default=REPO, help=argparse.SUPPRESS)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="fail when the stored hash is stale (default)")
    mode.add_argument("--print", action="store_true", help="print the hash to store in .fork/" + STORED)
    args = parser.parse_args(argv)
    if args.print:
        print(digest(args.root))
        return 0
    ok, why = check(args.root)
    print(why)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
