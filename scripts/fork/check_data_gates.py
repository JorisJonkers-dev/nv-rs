#!/usr/bin/env python3
"""Fork-only. Fails when Rust source reads NV_EXE, NV_DATA or NV_ORACLE_EXE in a way that can skip silently.

A test that needs the player's game files either calls the shared gate (scripts/fork/test_gate.rs)

    let Some(exe) = crate::test_gate::require_data("NV_EXE") else { return };

which prints `skipped: <test>` and fails under NV_ORACLE_REQUIRE=1, or it reads the variable in a way that
panics when it is unset (`std::env::var("NV_DATA").expect(...)`, usually under `#[ignore = "needs NV_DATA"]`).
Anything else, such as `let Ok(path) = std::env::var("NV_EXE") else { return }`, `.ok()` or `unwrap_or`, is a
silent skip: a private job cannot tell it from a pass, so the check names it.

Run: python3 -I scripts/fork/check_data_gates.py [root]      (root defaults to the repository; run by `task scripts`)
"""
import os
import re
import sys
from pathlib import Path

VARIABLES = "NV_EXE|NV_DATA|NV_ORACLE_EXE"
READ = re.compile(r'(?:\b(?:var|var_os)|\boption_env!|\benv!)\s*\(\s*"(?:%s)"' % VARIABLES)
# Not libtest tests: command line tools that report "skipped" themselves.
EXEMPT_PREFIXES = ("research/nv-oracle/",)
# The gate reads a variable by the name it is given and so never spells one.
SKIP_DIRS = {".git", "target", "node_modules", ".cargo"}


def loud(tail: str) -> bool:
    """True when the statement after the read panics if the variable is unset."""
    return re.search(r"\.(?:expect|unwrap)\s*\(", tail) is not None


def violations(text: str) -> list[tuple[int, str]]:
    found = []
    # Blank out line comments so documentation that names a variable is not a read.
    code = re.sub(r"//[^\n]*", lambda m: " " * len(m.group()), text)
    for m in READ.finditer(code):
        rest = code[m.end():]
        stop = min((i for i in (rest.find(";"), rest.find("{")) if i != -1), default=len(rest))
        tail = rest[:stop]
        if not loud(tail):
            line = code.count("\n", 0, m.start()) + 1
            found.append((line, text.splitlines()[line - 1].strip()))
    return found


def check(root: Path) -> list[str]:
    problems = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for name in filenames:
            if not name.endswith(".rs"):
                continue
            path = Path(dirpath) / name
            rel = path.relative_to(root).as_posix()
            if rel.startswith(EXEMPT_PREFIXES):
                continue
            for line, source in violations(path.read_text(encoding="utf-8", errors="replace")):
                problems.append(
                    f"{rel}:{line}: reads a game-data variable without a gate (use "
                    f"crate::test_gate::require_data(..), or `.expect(..)` it under #[ignore]): {source}"
                )
    return problems


def main() -> int:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent.parent
    problems = check(root)
    for p in problems:
        print(p, file=sys.stderr)
    if problems:
        print(f"{len(problems)} silent real-data read(s)", file=sys.stderr)
        return 1
    print("every read of NV_EXE, NV_DATA and NV_ORACLE_EXE is gated or loud")
    return 0


if __name__ == "__main__":
    sys.exit(main())
