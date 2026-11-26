#!/usr/bin/env python3
"""Fork-only. The private porting run: check the checker, then require every gated test to execute.

Public CI runs the same test commands without --require: tests that need the backend or the player's
game files print `skipped:` and the job stays green. The private run sets NV_ORACLE_REQUIRE=1 (the
gate then fails instead of skipping) and feeds all output to gate_counts.py --require, so a single
skip fails the run. Integrity is checked first, so a changed judge, threshold, control or ledger
generator cannot pass unnoticed (nv-rs#916, stories 24, 29, 30 and 58).

Run: python3 -I scripts/fork/private_run.py [--require]
"""
import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path

FORK = Path(__file__).resolve().parent
sys.path.insert(0, str(FORK))

import gate_counts  # noqa: E402
import private_integrity  # noqa: E402

REPO = FORK.parent.parent

# The same commands are used by public CI and by the private run; only --require differs. Each one
# must use `--nocapture` so the gate's executed/skipped lines reach gate_counts.py.
COMMANDS = (
    ("engine-core", ("cargo", "test", "-p", "engine-core", "--lib", "--", "--nocapture")),
    ("engine-havok", ("cargo", "test", "-p", "engine-havok", "--lib", "--", "--nocapture")),
    ("host", ("cargo", "test", "-p", "host", "--lib", "--", "--nocapture")),
    ("reference peer", ("cargo", "test", "-p", "engine", "--test", "diff_reference_peer", "--", "--nocapture")),
)


def run(commands, root, require, env=None) -> int:
    """Run each (label, argv) pair and count gate lines across all of them."""
    child_env = os.environ.copy() if env is None else dict(env)
    child_env.pop("NV_ORACLE_REQUIRE", None)
    if require:
        child_env["NV_ORACLE_REQUIRE"] = "1"
    lines, failed = [], False
    for label, argv in commands:
        result = subprocess.run(
            argv, cwd=root, env=child_env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, errors="replace", check=False,
        )
        lines.extend(result.stdout.splitlines())
        if result.returncode:
            print(f"failed: {label} exited {result.returncode}", file=sys.stderr)
            failed = True
    if require and any(
        line.strip().startswith("skipped:") and not gate_counts.LINE.match(line.strip()) for line in lines
    ):
        # A backend or toolchain skip printed outside the shared gate must fail too.
        print("failed: a test skipped outside the data gate", file=sys.stderr)
        failed = True
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", suffix=".log", delete=False) as log:
        log.write("\n".join(lines) + "\n")
        name = log.name
    try:
        gate_args = ["--require"] if require else []
        if gate_counts.main([name, *gate_args]):
            failed = True
    finally:
        Path(name).unlink()
    return int(failed)


def main(argv=None, commands=COMMANDS) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", type=Path, default=REPO, help=argparse.SUPPRESS)
    parser.add_argument("--require", action="store_true", help="fail on an unset private-input variable or any skip")
    args = parser.parse_args(argv)
    ok, why = private_integrity.check(args.root)
    if not ok:
        print(f"failed: {why}", file=sys.stderr)
        return 1
    print(why)
    return run(commands, args.root, args.require)


if __name__ == "__main__":
    sys.exit(main())
