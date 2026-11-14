#!/usr/bin/env python3
"""Fork-only. The tracking issue of a blocked or failed Upstream Sync (docs/fork/KEEPING-IN-SYNC.md, "The tracking issue").

  sync_report.py body  --stop-file F [--dropped-file F] [--detail-file F] [--run-url URL]   print the issue text
  sync_report.py open  (same options)      open or update the one open issue labelled sync-blocked (needs gh, GH_TOKEN)
  sync_report.py close [--run-url URL]     close that issue, if there is one
  sync_report.py summary --dropped-file F  markdown list of the changes the replay left out (the fork owns those paths)

The stop file is written by replay-upstream.sh (reason=, commit=, subject=); without one the failure is "other" (or
"diverged" when --detail-file holds the output of verify-sync.sh). Stdlib only; run inside the fork's clone.
"""
import argparse
import fnmatch
import json
import subprocess
import sys

LABEL = "sync-blocked"
TITLE = "Upstream Sync is blocked"
MARKER = "<!-- sync-blocked -->"

# The stop reasons of docs/fork/KEEPING-IN-SYNC.md, "When the replay stops", plus the two that are not the replay's.
REASONS = {
    "conflict": ("a commit does not apply on top of the fork",
                 "Upstream changed lines that a fork-only change also changed."),
    "history": ("upstream rewrote its history",
                "The upstream copy (main) no longer contains the commit the replay last reached. Stop, read upstream's repository, "
                "then choose the fork point again by hand."),
    "hook": ("a commit-msg hook refused a rewritten message",
             "Read the refusal in the run log; nothing was pushed."),
    "diverged": ("the fork differs from upstream outside the overlay",
                 "scripts/fork/verify-sync.sh found files that differ and are neither fork-owned nor listed in .fork/allowed-divergence."),
    "other": ("the run failed outside the replay",
              "A step before or after the replay failed (token, push, pull request). Read the run log."),
}

NEXT = {
    "allowed": "Merge by hand: the fork's change stays and upstream's change is added around it (`git cherry-pick <sha>`, resolve, commit "
               "with a Conventional title ending in the `Upstream-Commit:` and `Upstream-Author:` trailers, merge by rebase).",
    "overlay": "Nothing to do: the fork owns this path and the replay keeps the fork's version (upstream's change is listed in the sync summary).",
    "other": "Stop and read: neither fork-owned nor a known divergence. Read both changes, decide, and if the fork wants to differ here, "
             "add the file to `.fork/allowed-divergence` with a reason.",
}


def read_list(path):
    """Entries of an .fork list file: comments and blank lines removed."""
    out = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.split("#")[0].strip()
            if line:
                out.append(line)
    return out


def matches(path, entries):
    return any(path == e or (e.endswith("/") and path.startswith(e)) or ("*" in e and fnmatch.fnmatch(path, e)) for e in entries)


def classify(files, allowed, overlay):
    """files -> {"allowed": [...], "overlay": [...], "other": [...]}; allowed-divergence wins over overlay when both match."""
    out = {"allowed": [], "overlay": [], "other": []}
    for f in sorted(files):
        out["allowed" if matches(f, allowed) else "overlay" if matches(f, overlay) else "other"].append(f)
    return out


def git(*args, check=True, cwd=None):
    return subprocess.run(["git", *args], check=check, cwd=cwd, capture_output=True, text=True)


def conflict_files(commit, ours="HEAD", cwd=None):
    """The files git cannot merge when `commit` is replayed onto `ours` (what cherry-pick does), by `git merge-tree`."""
    res = git("merge-tree", "--write-tree", "--name-only", f"--merge-base={commit}^", ours, commit, check=False, cwd=cwd)
    if res.returncode == 0:
        return []
    lines = res.stdout.split("\n")
    names = []
    for line in lines[1:]:
        if not line:
            break
        names.append(line)
    return names


def parse_stop(path):
    info = {}
    if path:
        try:
            with open(path, encoding="utf-8") as fh:
                for line in fh:
                    k, _, v = line.rstrip("\n").partition("=")
                    if k:
                        info[k] = v
        except FileNotFoundError:
            pass
    return info


def read_text(path):
    try:
        with open(path, encoding="utf-8") as fh:
            return fh.read().strip()
    except (FileNotFoundError, TypeError):
        return ""


def build_body(info, files=None, allowed=(), overlay=(), run_url="", detail="", dropped=""):
    reason = info.get("reason") or ("diverged" if detail else "other")
    what, meaning = REASONS.get(reason, REASONS["other"])
    lines = [MARKER, f"The last run of Upstream Sync stopped: {what}.", "", meaning, ""]
    commit, subject = info.get("commit", ""), info.get("subject", "")
    if commit:
        lines += [f"- Upstream commit: `{commit}`" + (f" ({subject})" if subject else "")]
    lines += [f"- Reason: `{reason}`"]
    if run_url:
        lines += [f"- Run: {run_url}"]
    if reason == "conflict" and files is not None:
        cls = classify(files, allowed, overlay)
        lines += ["", "Files that conflict:", ""]
        if not files:
            lines += ["- none found by `git merge-tree` (the replay may have failed for another reason: read the run log)"]
        titles = {"allowed": "listed in `.fork/allowed-divergence`", "overlay": "listed in `.fork/overlay-paths`", "other": "anything else"}
        for key in ("allowed", "overlay", "other"):
            for f in cls[key]:
                lines += [f"- `{f}`: {titles[key]}. {NEXT[key]}"]
    if detail:
        lines += ["", "Output of `scripts/fork/verify-sync.sh`:", "", "```", detail[:4000], "```"]
    if dropped:
        lines += ["", "Changes left out of the replay so far (paths the fork owns):", "", dropped]
    lines += ["", "This issue is updated by each failed run and closed by the next successful one (docs/fork/KEEPING-IN-SYNC.md)."]
    return "\n".join(lines) + "\n"


def summary(path):
    seen = {}
    for line in read_text(path).splitlines():
        sha, _, f = line.partition(" ")
        seen.setdefault(f, []).append(sha)
    return "\n".join(f"- `{f}` (upstream {', '.join(shas)})" for f, shas in sorted(seen.items()))


def gh(*args, check=True):
    return subprocess.run(["gh", *args], check=check, capture_output=True, text=True)


def open_issue(body):
    gh("label", "create", LABEL, "--color", "B60205", "--description", "Upstream Sync stopped and needs a person", "--force", check=False)
    found = json.loads(gh("issue", "list", "--label", LABEL, "--state", "open", "--json", "number").stdout or "[]")
    if found:
        gh("issue", "edit", str(found[0]["number"]), "--body", body)
        print(f"updated issue #{found[0]['number']}")
    else:
        print(gh("issue", "create", "--label", LABEL, "--title", TITLE, "--body", body).stdout.strip())


def close_issue(run_url):
    found = json.loads(gh("issue", "list", "--label", LABEL, "--state", "open", "--json", "number").stdout or "[]")
    for i in found:
        gh("issue", "close", str(i["number"]), "--comment", "Upstream Sync succeeded" + (f": {run_url}" if run_url else "."))
        print(f"closed issue #{i['number']}")


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["body", "open", "close", "summary"])
    ap.add_argument("--stop-file")
    ap.add_argument("--dropped-file")
    ap.add_argument("--detail-file")
    ap.add_argument("--run-url", default="")
    a = ap.parse_args(argv)
    if a.cmd == "summary":
        print(summary(a.dropped_file))
        return 0
    if a.cmd == "close":
        close_issue(a.run_url)
        return 0
    info = parse_stop(a.stop_file)
    files = None
    if info.get("reason") == "conflict" and info.get("commit"):
        files = conflict_files(info["commit"])
    body = build_body(info, files, read_list(".fork/allowed-divergence"), read_list(".fork/overlay-paths"),
                      a.run_url, read_text(a.detail_file), summary(a.dropped_file) if a.dropped_file else "")
    if a.cmd == "body":
        sys.stdout.write(body)
    else:
        open_issue(body)
    return 0


if __name__ == "__main__":
    sys.exit(main())
