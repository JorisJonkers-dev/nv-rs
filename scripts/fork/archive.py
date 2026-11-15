#!/usr/bin/env python3
"""Build the test binaries once, run them anywhere (docs/fork/CI-SPEED.md).

  build: reads the JSON of `cargo test --no-run --message-format=json-render-diagnostics` on stdin and copies every test
         executable to --out with a manifest.json (name, file, package directory relative to the workspace root).
  run:   runs the named binaries of --dir from their package directory (what cargo does), each one's tests sliced by
         --shard i/n (every n-th test of the sorted list, offset by a hash of the binary name so the small binaries do not
         all load shard 1). The output is libtest's, headed by a `Running` line, so libtest_summary.py reads it as before.
Why not cargo-nextest: it runs one process per test, and the differential tests cache the 47,733-entry function table per
thread, so a process per test would pay for the table every time.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time
import zlib


def build(a):
    out = os.path.abspath(a.out)
    os.makedirs(out, exist_ok=True)
    seen = {}
    for line in sys.stdin:
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") != "compiler-artifact" or not m.get("executable") or not m.get("profile", {}).get("test"):
            continue
        t = m["target"]
        if "test" not in t["kind"]:
            continue
        name = t["name"]
        if name in seen:
            sys.exit(f"two test targets are named {name!r}; the archive is keyed by name")
        exe = m["executable"]
        dest = os.path.basename(exe)
        shutil.copy2(exe, os.path.join(out, dest))
        seen[name] = {"name": name, "file": dest, "dir": os.path.relpath(os.path.dirname(m["manifest_path"]), a.root).replace("\\", "/")}
    if not seen:
        sys.exit("no test executable in the cargo output")
    json.dump(sorted(seen.values(), key=lambda e: e["name"]), open(os.path.join(out, "manifest.json"), "w"), indent=1)
    print(f"archived {len(seen)} test binaries: {', '.join(sorted(seen))}")


def timings(a):
    """The slowest units of a `cargo --timings` build as a Markdown table (stdout)."""
    import glob
    import re
    html = open(sorted(glob.glob(os.path.join(a.dir, "cargo-timing*.html")))[-1], encoding="utf-8").read()
    data = json.loads(re.search(r"const UNIT_DATA = (\[.*?\]);\n", html, re.S).group(1))
    data.sort(key=lambda u: -u["duration"])
    print(f"Top {a.top} of {len(data)} compile units by wall time (cargo --timings; units overlap, so the sum exceeds the build)\n")
    print("| Unit | Seconds | Codegen start (s) |\n| --- | ---: | ---: |")
    for u in data[:a.top]:
        mode = u.get("mode", "")
        print(f"| {u['name']} {u['version']}{' ' + mode if mode and mode != 'todo' else ''} | {u['duration']:.1f} | {u.get('rmeta_time') or 0:.1f} |")


def tests_of(exe, cwd):
    r = subprocess.run([exe, "--list", "--format", "terse"], cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"{exe} --list failed:\n{r.stderr}")
    return sorted(l[:-6] for l in r.stdout.splitlines() if l.endswith(": test"))


def pick(names, shard, key):
    i, n = (int(x) for x in shard.split("/"))
    off = zlib.crc32(key.encode()) % n
    return [x for k, x in enumerate(names) if (k + off) % n == i - 1]


def run(a):
    d = os.path.abspath(a.dir)
    manifest = {e["name"]: e for e in json.load(open(os.path.join(d, "manifest.json")))}
    wanted = [t for t in a.tests.split(",") if t]
    missing = [t for t in wanted if t not in manifest]
    if missing:
        sys.exit(f"not in the archive: {', '.join(missing)}")
    root = os.getcwd()
    rc, t0 = 0, time.time()
    for t in wanted:
        e = manifest[t]
        exe = os.path.join(d, e["file"])
        cwd = os.path.join(root, e["dir"])
        env_extra = {"CARGO_MANIFEST_DIR": cwd}
        names = tests_of(exe, cwd)
        mine = pick(names, a.shard, t) if a.shard else names
        print(f"     Running tests/{t}.rs (archive, shard {a.shard or '1/1'}: {len(mine)} of {len(names)} tests)", flush=True)
        if not mine:
            continue
        cmd = [exe, "--exact", *mine] + a.rest
        r = subprocess.run(cmd, cwd=cwd, env={**os.environ, **env_extra})
        rc = rc or r.returncode
    with open(a.timings, "a") as f:
        f.write(f"test {int(time.time() - t0)}\n")
    sys.exit(rc)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("--out", required=True)
    b.add_argument("--root", default=".")
    t = sub.add_parser("timings")
    t.add_argument("--dir", required=True)
    t.add_argument("--top", type=int, default=20)
    r = sub.add_parser("run")
    r.add_argument("--dir", required=True)
    r.add_argument("--tests", required=True, help="comma-separated test binary names")
    r.add_argument("--shard", default="")
    r.add_argument("--timings", default=os.path.join(os.environ.get("RUNNER_TEMP", "/tmp"), "timings.txt"))
    r.add_argument("rest", nargs="*")
    a = ap.parse_args()
    {"build": build, "run": run, "timings": timings}[a.cmd](a)


if __name__ == "__main__":
    main()
