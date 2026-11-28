#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Cross-machine claims for porting tasks, kept on the task's GitHub issue.

    python3 -I scripts/fork/factory_claims.py acquire REPO KEY --issue N [--lease-seconds S] [--machine ID]
    python3 -I scripts/fork/factory_claims.py renew   REPO KEY --issue N [--lease-seconds S] [--machine ID]
    python3 -I scripts/fork/factory_claims.py release REPO KEY --issue N [--machine ID]
    python3 -I scripts/fork/factory_claims.py list    REPO KEY --issue N

The verbs and arguments are the ones `porting_loop.py` hands to NV_PORT_CLAIM_CMD, so this script can be that command;
the issue comes from `--issue` or, as `porting_loop.py` leaves it out of renew and list, from NV_PORT_CLAIM_ISSUE.

A claimed issue carries the label `factory:claimed` and is assigned to the claiming account: that pair is the lock
people see and other tooling reads. The lock itself is decided by the lease comments, because every machine uses the
same account and the assignee cannot tell machines apart. A lease comment holds a hidden JSON marker (key, machine,
expiry). The valid claims for a key are its unexpired lease comments; the earliest one by comment id wins, and the
others back off by deleting their own comment. A lease past its expiry is stale: the next machine deletes it and
claims. The lease is advisory in the sense that nothing stops a machine that ignores it; renewing moves the expiry.

Exit codes: 0 the claim is held (acquire, renew) or the verb is done; 1 another machine holds it, or the claim was lost;
2 the GitHub API failed.
"""
import argparse
import json
import math
import os
import re
import socket
import subprocess
import sys
import time
from collections.abc import Callable
from typing import Any, Protocol, cast

LABEL = "factory:claimed"
LABEL_COLOR = "bfd4f2"
DEFAULT_LEASE = 900
MARKER = re.compile(r"^<!-- factory-claim (\{.*\}) -->$", re.MULTILINE)

Runner = Callable[[list[str]], tuple[int, str]]
Clock = Callable[[], float]


class ApiError(Exception):
    """A GitHub API call that failed for a reason other than the thing being gone already."""


class Api(Protocol):
    def me(self) -> str: ...
    def comments(self, issue: int) -> list[dict[str, Any]]: ...
    def add_comment(self, issue: int, body: str) -> int: ...
    def edit_comment(self, comment: int, body: str) -> bool: ...
    def delete_comment(self, comment: int) -> None: ...
    def ensure_label(self) -> None: ...
    def add_label(self, issue: int) -> None: ...
    def remove_label(self, issue: int) -> None: ...
    def set_assignee(self, issue: int, login: str, on: bool) -> None: ...


def run(argv: list[str]) -> tuple[int, str]:
    done = subprocess.run(argv, capture_output=True, text=True, check=False)
    return done.returncode, done.stdout + done.stderr


class GhApi:
    """The calls the claims need, through `gh api`."""

    def __init__(self, repo: str, runner: Runner = run) -> None:
        self.repo, self.runner = repo, runner

    def call(self, *args: str, gone_ok: bool = False) -> str:
        code, output = self.runner(["gh", "api", *args])
        if code and not (gone_ok and ("HTTP 404" in output or "HTTP 422" in output)):
            raise ApiError(output.strip()[:300])
        return "" if code else output

    def me(self) -> str:
        return self.call("user", "--jq", ".login").strip()

    def comments(self, issue: int) -> list[dict[str, Any]]:
        text = self.call("--paginate", f"repos/{self.repo}/issues/{issue}/comments", "--jq",
                         ".[] | {id, body}")
        return [cast(dict[str, Any], json.loads(line)) for line in text.splitlines() if line.strip()]

    def add_comment(self, issue: int, body: str) -> int:
        return int(self.call("-X", "POST", f"repos/{self.repo}/issues/{issue}/comments", "-f", f"body={body}",
                             "--jq", ".id"))

    def edit_comment(self, comment: int, body: str) -> bool:
        """False when the comment is gone (a stale lease someone else reclaimed)."""
        try:
            self.call("-X", "PATCH", f"repos/{self.repo}/issues/comments/{comment}", "-f", f"body={body}")
        except ApiError as exc:
            if "HTTP 404" in str(exc):
                return False
            raise
        return True

    def delete_comment(self, comment: int) -> None:
        self.call("-X", "DELETE", f"repos/{self.repo}/issues/comments/{comment}", gone_ok=True)

    def ensure_label(self) -> None:
        self.call("-X", "POST", f"repos/{self.repo}/labels", "-f", f"name={LABEL}", "-f", f"color={LABEL_COLOR}",
                  gone_ok=True)  # a label that exists answers 422

    def add_label(self, issue: int) -> None:
        self.call("-X", "POST", f"repos/{self.repo}/issues/{issue}/labels", "-f", f"labels[]={LABEL}")

    def remove_label(self, issue: int) -> None:
        self.call("-X", "DELETE", f"repos/{self.repo}/issues/{issue}/labels/{LABEL}", gone_ok=True)

    def set_assignee(self, issue: int, login: str, on: bool) -> None:
        self.call("-X", "POST" if on else "DELETE", f"repos/{self.repo}/issues/{issue}/assignees", "-f",
                  f"assignees[]={login}", gone_ok=not on)


def marker_of(body: str) -> dict[str, Any] | None:
    """The claim a comment holds, or None when it is not a lease comment."""
    found = MARKER.search(body)
    if not found:
        return None
    try:
        claim = cast(dict[str, Any], json.loads(found.group(1)))  # the pattern only matches an object
    except ValueError:
        return None
    expires = claim.get("expires")
    if not (isinstance(claim.get("key"), str) and isinstance(claim.get("machine"), str)
            and isinstance(expires, (int, float)) and not isinstance(expires, bool) and math.isfinite(expires)):
        return None
    return claim


def body_of(key: str, machine: str, expires: float) -> str:
    stamp = time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime(expires))
    claim = json.dumps({"key": key, "machine": machine, "expires": expires}, sort_keys=True)
    return f"<!-- factory-claim {claim} -->\nClaimed for `{key}` by `{machine}` until {stamp}."


class Claims:
    """Acquire, renew and release a claim of one issue for one machine."""

    def __init__(self, api: Api, machine: str, clock: Clock = time.time, lease: float = DEFAULT_LEASE) -> None:
        self.api, self.machine, self.clock, self.lease = api, machine, clock, lease

    def entries(self, issue: int, key: str) -> list[dict[str, Any]]:
        """The lease comments of `key`, earliest first, each with its id, machine, expiry and whether it is valid."""
        now = self.clock()
        found: list[dict[str, Any]] = []
        for comment in self.api.comments(issue):
            claim = marker_of(str(comment.get("body", "")))
            if claim is not None and claim["key"] == key and isinstance(comment.get("id"), int):
                found.append({"id": int(comment["id"]), "machine": claim["machine"],
                              "expires": float(claim["expires"]), "valid": float(claim["expires"]) > now})
        return sorted(found, key=lambda e: e["id"])

    def winner(self, entries: list[dict[str, Any]]) -> dict[str, Any] | None:
        return next((e for e in entries if e["valid"]), None)

    def mark(self, issue: int) -> None:
        """The visible lock: the label and the assignee, set again on every claim and renewal in case a release cleared them."""
        self.api.ensure_label()
        self.api.add_label(issue)
        self.api.set_assignee(issue, self.api.me(), True)

    def acquire(self, issue: int, key: str) -> bool:
        """True when this machine holds the claim afterwards; False when another machine's earlier claim does."""
        entries = self.entries(issue, key)
        first = self.winner(entries)
        if first is not None:
            if first["machine"] != self.machine:
                return False
            if self.api.edit_comment(first["id"], body_of(key, self.machine, self.clock() + self.lease)):
                self.mark(issue)
                return True
            entries = [e for e in entries if e["id"] != first["id"]]  # the lease vanished under us: claim anew
        for stale in entries:  # leases past their expiry: reclaimed; a valid one of another machine stays
            if not stale["valid"]:
                self.api.delete_comment(stale["id"])
        self.mark(issue)
        mine = self.api.add_comment(issue, body_of(key, self.machine, self.clock() + self.lease))
        first = self.winner(self.entries(issue, key))
        if first is not None and first["id"] == mine:
            return True
        self.api.delete_comment(mine)  # a claim posted earlier by another machine wins
        return False

    def renew(self, issue: int, key: str) -> bool:
        """Move this machine's expiry on; False when the claim was lost (taken over, or its comment is gone)."""
        entries = self.entries(issue, key)
        first = self.winner(entries)
        if first is not None and first["machine"] != self.machine:
            return False
        mine = [e for e in entries if e["machine"] == self.machine]
        if not mine:
            return False
        target = first if first is not None else mine[-1]  # an expired lease nobody took over is renewed
        if not self.api.edit_comment(target["id"], body_of(key, self.machine, self.clock() + self.lease)):
            return False
        self.mark(issue)
        return True

    def release(self, issue: int, key: str) -> None:
        """Delete this machine's lease; when no valid claim of any key is left, free the label and the assignee."""
        for entry in self.entries(issue, key):
            if entry["machine"] == self.machine:
                self.api.delete_comment(entry["id"])
        now = self.clock()
        live = [c for c in self.api.comments(issue)
                if (m := marker_of(str(c.get("body", "")))) is not None and float(m["expires"]) > now]
        if not live:
            self.api.remove_label(issue)
            self.api.set_assignee(issue, self.api.me(), False)


def main(argv: list[str] | None = None, api: Api | None = None, clock: Clock = time.time,
         out: Any = sys.stdout) -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    parser.add_argument("verb", choices=("acquire", "renew", "release", "list"))
    parser.add_argument("repo")
    parser.add_argument("key")
    parser.add_argument("--issue", type=int, default=int(os.environ.get("NV_PORT_CLAIM_ISSUE") or 0))
    parser.add_argument("--lease-seconds", type=float, default=DEFAULT_LEASE)
    parser.add_argument("--machine", default=socket.gethostname())
    args = parser.parse_args(argv)
    if args.issue <= 0:
        parser.error("--issue (or NV_PORT_CLAIM_ISSUE) names the issue")
    claims = Claims(api or GhApi(args.repo), args.machine, clock, args.lease_seconds)
    try:
        if args.verb == "list":
            print(json.dumps(claims.entries(args.issue, args.key), sort_keys=True), file=out)
            return 0
        if args.verb == "release":
            claims.release(args.issue, args.key)
            return 0
        held = claims.acquire(args.issue, args.key) if args.verb == "acquire" else claims.renew(args.issue, args.key)
    except (ApiError, OSError, ValueError) as exc:
        print(f"factory_claims: {exc}", file=sys.stderr)
        return 2
    return 0 if held else 1


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
