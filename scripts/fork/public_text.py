#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Public-text check for the fork's own docs and scripts (docs/fork/PUBLIC-TEXT.md).

Machine details and personal local tools do not belong in `docs/fork/` or `scripts/fork/`: an exact graphics card or
device id, a path under a real home folder, the maintainer's own wrapper by name, "the owner's machine/session/...".
Role words ("the maintainer's call") are fine. Placeholders (`/home/someone`, `/home/runner`) are fine.

    python3 -I scripts/fork/public_text.py            check the tree, exit 1 on a finding
    python3 -I scripts/fork/public_text.py --selftest synthetic examples must be found / passed
"""
import re
import subprocess
import sys
from pathlib import Path

ROOTS = ("docs/fork", "scripts/fork")
# This file and its test hold the patterns' own examples.
SKIP = {"scripts/fork/public_text.py", "scripts/fork/tests/test_public_text.py"}
SKIP_NAMES = {"Cargo.lock"}
PLACEHOLDER_HOMES = {"someone", "user", "bob", "runner", "you", "name", "me", "example"}

PATTERNS = [
    ("an exact graphics card", re.compile(r"\b(?:RX|RTX|GTX)\s?\d{3,4}\b|\bNAVI\s?\d+\b|\bGeForce\b", re.I)),
    ("a PCI device id", re.compile(r"\b(?:1002|10de|8086):[0-9a-f]{4}\b", re.I)),
    ("the maintainer's own wrapper", re.compile(r"\bnv-rs-run\b(?!-summary)")),
    ("a personal machine or session", re.compile(
        r"\bowner'?s (?:own )?(?:machine|session|display|desktop|card|pc|userdata|game folder|run|windowed)\b", re.I)),
    ("an owner-local tool", re.compile(r"\bowner-local\b", re.I)),
]
HOME = re.compile(r"(?:/home|/Users)/([A-Za-z0-9_.-]+)")


def findings(text):
    """The (line number, what, excerpt) of each machine detail in `text`."""
    out = []
    for n, line in enumerate(text.splitlines(), 1):
        for what, rx in PATTERNS:
            m = rx.search(line)
            if m:
                out.append((n, what, m.group(0)))
        for m in HOME.finditer(line):
            if m.group(1).lower() not in PLACEHOLDER_HOMES:
                out.append((n, "a path under a real home folder", m.group(0)))
    return out


def tracked():
    names = subprocess.run(["git", "ls-files", "-z", *ROOTS], check=True, capture_output=True).stdout.split(b"\0")
    return [Path(n.decode()) for n in names if n]


def check():
    bad = 0
    for path in tracked():
        if str(path) in SKIP or path.name in SKIP_NAMES:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for n, what, excerpt in findings(text):
            print(f"{path}:{n}: {what}: {excerpt!r}")
            bad += 1
    if bad:
        print(f"public_text: {bad} finding(s); see docs/fork/PUBLIC-TEXT.md", file=sys.stderr)
    return 1 if bad else 0


def selftest():
    must_find = [
        "measured on an RX " + "7900 XTX",
        "device 1002:" + "744c",
        "the " + "nv-rs-run wrapper",
        "on the owner's " + "machine",
        "~/.local/bin and /home/" + "alice/Games",
        "GeForce " + "card",
    ]
    must_pass = [
        "a recent AMD card on RADV",
        "/home/someone/.config and /home/runner/work",
        "schema nv-rs-run-summary/1",
        "signing is the owner's call",
    ]
    for s in must_find:
        assert findings(s), f"not found: {s}"
    for s in must_pass:
        assert not findings(s), f"wrongly found: {s}: {findings(s)}"
    print("public_text selftest ok")
    return 0


if __name__ == "__main__":
    sys.exit(selftest() if "--selftest" in sys.argv else check())
