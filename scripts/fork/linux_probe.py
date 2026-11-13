#!/usr/bin/env python3
"""Find out which workspace crates build on Linux.

Runs `cargo check --workspace --all-targets --keep-going`, takes the crates that fail to compile, and adds
every workspace crate that depends on one of them (those cannot build either). Prints a Markdown report
and writes the crates that do build, one per line, to the file named by --out (default .fork/linux-ok.candidate).
Promote that file to .fork/linux-ok.txt to make their Linux tests and clippy a required check.
Stdlib only.
"""
import argparse
import json
import re
import subprocess
import sys


def run(*cmd: str) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, text=True, check=False)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=".fork/linux-ok.candidate")
    ap.add_argument("--target", default="x86_64-unknown-linux-gnu")
    a = ap.parse_args()
    meta = run("cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", a.target)
    if meta.returncode:
        sys.exit(f"cargo metadata failed:\n{meta.stderr[-2000:]}")
    data = json.loads(meta.stdout)
    members = set(data["workspace_members"])
    by_id = {p["id"]: p["name"] for p in data["packages"]}
    deps: dict[str, set[str]] = {}
    for node in data["resolve"]["nodes"]:
        if node["id"] in members:
            deps[by_id[node["id"]]] = {by_id[d] for d in node["dependencies"] if d in members}
    chk = run("cargo", "check", "--locked", "--workspace", "--all-targets", "--keep-going", "--message-format", "short")
    failed = set(re.findall(r"could not compile `([^`]+)`", chk.stderr))
    unbuildable = set(failed)
    grew = True
    while grew:
        grew = False
        for name, ds in deps.items():
            if name not in unbuildable and ds & unbuildable:
                unbuildable.add(name)
                grew = True
    ok = sorted(set(deps) - unbuildable)
    print(f"# Linux probe ({a.target})\n")
    print(f"- builds: {len(ok)} of {len(deps)} workspace crates")
    print(f"- fail to compile: {', '.join(sorted(failed)) or 'none'}")
    print(f"- blocked by those: {', '.join(sorted(unbuildable - failed)) or 'none'}\n")
    print("Builds on Linux:\n\n" + "\n".join(f"- {c}" for c in ok))
    with open(a.out, "w", encoding="utf-8") as fh:
        fh.write("".join(c + "\n" for c in ok))
    if failed:
        print("\n<details><summary>first errors</summary>\n\n```\n" + "\n".join(chk.stderr.splitlines()[:60]) + "\n```\n</details>")


if __name__ == "__main__":
    main()
