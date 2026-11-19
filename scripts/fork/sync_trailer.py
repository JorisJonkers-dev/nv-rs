"""Fork-only. A pull request that replays upstream commits (they carry `Upstream-Commit:` trailers) must end its
description with the newest of those trailers, so the replay point survives a squash merge: this repository's squash
message is the pull request's description, and a squash that drops the trailer leaves verify-sync, the next replay and
the release build reading an older replay point (nv-rs#376). A rebase merge keeps the trailers anyway.

Run:  python3 -I scripts/fork/sync_trailer.py BASE HEAD BODY_FILE
Exits 0 when the pull request replays nothing or its description ends with the right trailer, 1 otherwise.
"""
import re
import subprocess
import sys

TRAILER = re.compile(r"^Upstream-Commit: ([0-9a-f]{40})$", re.M)


def newest_replayed(base: str, head: str) -> str | None:
    """The newest upstream commit the pull request's own commits were replayed from, or None."""
    text = subprocess.run(
        ["git", "log", "-1", "--format=%B", r"--grep=^Upstream-Commit: [0-9a-f]\{40\}$", f"{base}..{head}"],
        capture_output=True, text=True, check=True,
    ).stdout
    found = TRAILER.findall(text)
    return found[-1] if found else None


def body_trailer(body: str) -> str | None:
    """The trailer on the description's last non-empty line, or None."""
    lines = [l.strip() for l in body.replace("\r\n", "\n").splitlines() if l.strip()]
    if not lines:
        return None
    m = TRAILER.match(lines[-1])
    return m.group(1) if m else None


def verdict(newest: str | None, body: str) -> str | None:
    """None when the description is right; otherwise the error to print."""
    if newest is None:
        return None
    got = body_trailer(body)
    if got == newest:
        return None
    if got is None:
        return (f"this pull request replays upstream commits: end its description with the line "
                f"'Upstream-Commit: {newest}', so a squash merge (its message is the description) keeps the replay point")
    return (f"the description ends with Upstream-Commit: {got}, but the newest replayed commit is {newest}: "
            f"end it with 'Upstream-Commit: {newest}'")


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    base, head, body_file = argv[1:]
    with open(body_file, encoding="utf-8") as f:
        body = f.read()
    newest = newest_replayed(base, head)
    error = verdict(newest, body)
    if error:
        print(f"::error title=Upstream-Commit trailer::{error}")
        return 1
    print("no replayed commits" if newest is None else f"the description ends with Upstream-Commit: {newest}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
