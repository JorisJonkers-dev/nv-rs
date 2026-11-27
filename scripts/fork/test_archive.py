#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Compile the test binaries of a job group once, hand them to the jobs that run them (docs/fork/CI.md, "Compile once").

  build DIR -- <cargo test arguments>   `cargo test --no-run` for the arguments, and every executable it built (test
                                        binaries and binaries, for CARGO_BIN_EXE_*) copied under DIR at its path relative
                                        to the workspace, with DIR/archive.json naming them. Upload DIR as an artifact.
  extract DIR                           copies the binaries of DIR back to the same relative paths of the workspace.
  find DIR CRATE TEST                   prints "<executable>\\t<the package's directory>" of an integration test, for
                                        run-component.sh. Exits 1 when the archive has no such test.

The binaries carry the absolute path of the checkout (env!("CARGO_MANIFEST_DIR")), and every job of a run checks out to the
same path on the same runner image, so they run where they were built."""
import json
import os
import shutil
import subprocess
import sys


def rel(path, root):
    return os.path.relpath(os.path.realpath(path), os.path.realpath(root)).replace(os.sep, "/")


def package_name(package_id):
    """`path+file:///r/crates/engine#0.1.0` (the name is the folder's) or `path+file:///r/crates/x#engine@0.1.0`."""
    base, _, frag = package_id.partition("#")
    return frag.split("@")[0] if "@" in frag else base.rstrip("/").split("/")[-1]


def build(out, cargo_args, root="."):
    cmd = ["cargo", "test", "--locked", "--no-run", "--message-format=json-render-diagnostics", *cargo_args]
    print("::group::" + " ".join(cmd[:4] + cargo_args), flush=True)
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
    binaries = []
    for line in proc.stdout:
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") != "compiler-artifact" or not msg.get("executable"):
            continue
        target = msg["target"]
        crate = package_name(msg["package_id"])
        binaries.append({
            "crate": crate, "name": target["name"], "kind": target["kind"][0],
            "executable": rel(msg["executable"], root),
            "cwd": rel(os.path.dirname(msg["manifest_path"]), root)})
    status = proc.wait()
    print("::endgroup::", flush=True)
    if status:
        print("::error title=compile failed::" + " ".join(cmd), flush=True)
        return status
    outside = [b["executable"] for b in binaries if b["executable"].startswith("../")]
    if outside:
        print("::error title=archive::the target directory is outside the workspace: " + outside[0], flush=True)
        return 1
    for b in binaries:
        dest = os.path.join(out, b["executable"])
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        shutil.copy2(os.path.join(root, b["executable"]), dest)
    with open(os.path.join(out, "archive.json"), "w", encoding="utf-8") as fh:
        json.dump({"binaries": binaries}, fh, indent=1)
    size = sum(os.path.getsize(os.path.join(out, b["executable"])) for b in binaries)
    print(f"archived {len(binaries)} executables, {size / 1e6:.0f} MB")
    return 0


def load(d):
    with open(os.path.join(d, "archive.json"), encoding="utf-8") as fh:
        return json.load(fh)["binaries"]


def extract(d, root="."):
    for b in load(d):
        dest = os.path.join(root, b["executable"])
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        shutil.copy2(os.path.join(d, b["executable"]), dest)
        os.chmod(dest, os.stat(dest).st_mode | 0o111)
    return 0


def find(d, crate, test):
    for b in load(d):
        if b["crate"] == crate and b["name"] == test and b["kind"] == "test":
            print(b["executable"] + "\t" + b["cwd"])
            return 0
    return 1


def main(argv):
    if len(argv) >= 3 and argv[1] == "build" and "--" in argv:
        i = argv.index("--")
        return build(argv[2], argv[i + 1:])
    if len(argv) == 3 and argv[1] == "extract":
        return extract(argv[2])
    if len(argv) == 5 and argv[1] == "find":
        return find(argv[2], argv[3], argv[4])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
