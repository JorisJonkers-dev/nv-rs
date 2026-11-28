#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The sandboxed verifier: one throwaway container per run, a copy-on-write copy of a seed target, nothing left behind.

    python3 -I scripts/fork/verifier.py image
    python3 -I scripts/fork/verifier.py run --seed DIR [--memory 12g] [--cache DIR] [--out FILE] -- COMMAND...

`image` builds (when missing) the pinned image `nv-rs-verifier:<hash>`: the Rust image of the repository's toolchain
channel plus cargo-nextest, cargo-llvm-cov and sccache from pinned release archives with their SHA-256. The hash covers
the Dockerfile text, Cargo.lock and rust-toolchain.toml, so the image is rebuilt when any of them changes. (cargo-fuzz
needs the pinned nightly and belongs to the scheduled fuzz runs.)

`run` copies the seed target with `cp --reflink=always` (copy-on-write on btrfs: it costs no disk until a file is
written), starts one container with `--rm`, `--memory` and `--memory-swap` equal, no network, the repository mounted
at its own absolute path (so sccache and cargo fingerprints match), a fixed sccache directory, and runs COMMAND in it.
Afterwards the copy is removed and the container is made sure to be gone. One JSON record on stdout: the image, the
seconds of the copy and of the run, the command's exit status, the container's peak memory, sccache's hit counts, and
the disk used before and after (shared with whatever else runs on the machine) and the bytes the verifier's own
directory holds afterwards (the sccache directory: the copy is gone). The seed is only ever read.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import time
import uuid
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
TOOLS = (  # name, archive URL, SHA-256, member path inside the archive
    ("cargo-nextest", ("https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.152/"
                       "cargo-nextest-0.9.152-x86_64-unknown-linux-musl.tar.gz"),
     "521c5f790ed07ee69d0a88403d9d86a8c8ab27977f12fc73f22aa4e7e14a0f71", "cargo-nextest"),
    ("cargo-llvm-cov", ("https://github.com/taiki-e/cargo-llvm-cov/releases/download/v0.9.1/"
                        "cargo-llvm-cov-x86_64-unknown-linux-musl.tar.gz"),
     "3fca950394a3c49457657c158b1619cec8dfd2647ae5b48746734c0ab969a522", "cargo-llvm-cov"),
    ("sccache", ("https://github.com/mozilla/sccache/releases/download/v0.20.1/"
                 "sccache-v0.20.1-x86_64-unknown-linux-musl.tar.gz"),
     "c9f6c5a3ae84d9da0956b28c357a365827bda33f3383647fc0b67484c74a5ea9",
     "sccache-v0.20.1-x86_64-unknown-linux-musl/sccache"),
)
INNER = r"""
"$@"
rc=$?
echo "VERIFIER_EXIT=$rc"
echo "VERIFIER_PEAK=$(cat /sys/fs/cgroup/memory.peak /sys/fs/cgroup/memory/memory.max_usage_in_bytes 2>/dev/null | head -n 1)"
sccache --show-stats 2>/dev/null | sed -n 's/^\(Cache hits\|Cache misses\)  *\([0-9][0-9]*\)$/VERIFIER_SCCACHE \1=\2/p'
exit $rc
"""
STAT = re.compile(r"^VERIFIER_(EXIT)=([0-9]+)$|^VERIFIER_(PEAK)=([0-9]*)$|^VERIFIER_SCCACHE (Cache hits|Cache misses)=([0-9]+)$",
                  re.MULTILINE)


class VerifierError(Exception):
    """A failure that ends the run."""


def run(argv, cwd=".", input=None):
    """(exit status, combined output); a missing program is status 127."""
    try:
        done = subprocess.run([str(a) for a in argv], cwd=str(cwd), input=input, text=True, errors="replace",
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    except OSError as exc:
        return 127, f"{argv[0]}: {exc}"
    return done.returncode, done.stdout


def channel(root):
    """The toolchain channel the repository pins."""
    text = (Path(root) / "rust-toolchain.toml").read_text(encoding="utf-8")
    found = re.search(r'^channel\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not found:
        raise VerifierError("rust-toolchain.toml names no channel")
    return found.group(1)


def dockerfile(chan):
    """The image's Dockerfile text: the toolchain's own Rust image and the pinned tools."""
    lines = [f"FROM rust:{chan}-slim", ("RUN apt-get update && apt-get install -y --no-install-recommends curl "
              "&& rm -rf /var/lib/apt/lists/*"),
             "RUN rustup component add rustfmt clippy llvm-tools-preview"]
    for name, url, digest, member in TOOLS:
        lines.append(f"RUN curl -fsSL -o /tmp/{name}.tgz {url} && echo '{digest}  /tmp/{name}.tgz' | sha256sum -c - "
                     f"&& tar -xzf /tmp/{name}.tgz -C /tmp && install -m 0755 /tmp/{member} /usr/local/bin/{name} "
                     f"&& rm -rf /tmp/{name}.tgz /tmp/{name} /tmp/sccache-v*")
    lines.append("ENV CARGO_INCREMENTAL=0 SCCACHE_DIR=/sccache")
    return "\n".join(lines) + "\n"


def image_tag(root):
    """`nv-rs-verifier:<hash>`: changes with the Dockerfile, Cargo.lock and rust-toolchain.toml."""
    digest = hashlib.sha256()
    for part in (dockerfile(channel(root)).encode(), (Path(root) / "Cargo.lock").read_bytes(),
                 (Path(root) / "rust-toolchain.toml").read_bytes()):
        digest.update(hashlib.sha256(part).digest())
    return f"nv-rs-verifier:{digest.hexdigest()[:12]}"


def ensure_image(root, runner=run):
    """Build the image unless its tag exists; returns (tag, whether it was built)."""
    tag = image_tag(root)
    if runner(["docker", "image", "inspect", tag])[0] == 0:
        return tag, False
    code, output = runner(["docker", "build", "-t", tag, "-"], input=dockerfile(channel(root)))
    if code:
        raise VerifierError(f"image build failed: {output.strip()[-400:]}")
    return tag, True


def used_bytes(path):
    """Bytes in use on the filesystem holding `path`."""
    usage = shutil.disk_usage(path)
    return usage.used


def tree_bytes(path):
    """Bytes of the files under `path` (what the verifier's own directory holds, apart from the host's noise)."""
    total = 0
    for folder, _, names in os.walk(path):
        for name in names:
            try:
                info = os.lstat(os.path.join(folder, name))
            except OSError:
                continue  # gone since the walk listed it
            if stat.S_ISREG(info.st_mode):
                total += info.st_size
    return total


def clone_seed(seed, dest, runner=run):
    """A copy-on-write copy of the seed target; the seed is only read."""
    seed, dest = Path(seed), Path(dest)
    if not seed.is_dir():
        raise VerifierError(f"the seed {seed} is not a directory")
    if dest.exists():
        raise VerifierError(f"{dest} exists")
    dest.parent.mkdir(parents=True, exist_ok=True)
    code, output = runner(["cp", "-a", "--reflink=always", str(seed), str(dest)])
    if code:
        shutil.rmtree(dest, ignore_errors=True)
        raise VerifierError(f"copy-on-write copy failed (is the seed on btrfs?): {output.strip()[-300:]}")


def docker_run_args(root, tag, name, target, cache, memory, command):
    """The `docker run` argv: capped, offline, repository at its own path, one fixed sccache directory."""
    root = str(Path(root).resolve())
    config = Path.home() / ".cargo" / "config.toml"  # names the registry source the host's cache directories belong to
    source = ["-v", f"{config}:/usr/local/cargo/config.toml:ro"] if config.is_file() else []
    return ["docker", "run", "--rm", "--name", name, "--memory", memory, "--memory-swap", memory, "--network", "none",
            "-v", f"{root}:{root}", "-v", f"{target}:/target", "-v", f"{cache}:/sccache",
            "-v", f"{Path.home() / '.cargo' / 'registry'}:/usr/local/cargo/registry:ro", *source,
            "-e", "CARGO_TARGET_DIR=/target", "-e", "RUSTC_WRAPPER=sccache", "-e", "CARGO_NET_OFFLINE=true",
            "-w", root, tag, "bash", "-c", INNER, "verifier", *command]


def parse_stats(output):
    """exit status, peak memory and sccache hit counts from the container's marker lines."""
    stats: dict[str, int | None] = {"exit": None, "peak_bytes": None, "sccache_hits": None, "sccache_misses": None}
    for exit_kind, exit_number, peak_kind, peak_number, label, count in STAT.findall(output):
        if exit_kind:
            stats["exit"] = int(exit_number)
        elif peak_kind:
            stats["peak_bytes"] = int(peak_number) if peak_number else None
        else:
            stats["sccache_hits" if label == "Cache hits" else "sccache_misses"] = int(count)
    return stats


def verify(root, seed, command, memory="12g", cache=None, runner=run, workdir=None):
    """One verifier run; returns the record. Whatever it made is removed, also when it fails."""
    root = Path(root)
    work = (Path(workdir) if workdir else Path.home() / ".cache" / "nv-workers" / "verifier").resolve()
    work.mkdir(parents=True, exist_ok=True)
    cache = Path(cache).resolve() if cache else work / "sccache"
    cache.mkdir(parents=True, exist_ok=True)
    tag, built = ensure_image(root, runner)
    name = f"nv-rs-verifier-{uuid.uuid4().hex[:8]}"
    target = work / f"target-{name}"
    before = used_bytes(work)
    record = {"image": tag, "image_built": built, "container": name}
    started = time.time()
    clone_seed(seed, target, runner)  # a refused or failed copy removes only what it made itself
    record["clone_seconds"] = round(time.time() - started, 2)
    try:
        started = time.time()
        code, output = runner(docker_run_args(root, tag, name, target, cache, memory, command))
        record["run_seconds"] = round(time.time() - started, 2)
        record.update(parse_stats(output))
        record["docker_exit"] = code
        record["output_tail"] = output.strip()[-600:]
    finally:
        runner(["docker", "rm", "-f", name])
        shutil.rmtree(target, ignore_errors=True)
    left = runner(["docker", "ps", "-a", "--filter", f"name={name}", "--format", "{{.Names}}"])[1].split()
    record["container_left"] = bool(left)
    record["disk_used_before"], record["disk_used_after"] = before, used_bytes(work)
    record["disk_left_by_run"] = record["disk_used_after"] - before
    record["workdir_bytes_after"] = tree_bytes(work)
    return record


def main(argv=None, root=REPO, runner=run, out=sys.stdout):
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("image")
    one = sub.add_parser("run")
    one.add_argument("--seed", required=True)
    one.add_argument("--memory", default="12g")
    one.add_argument("--cache")
    one.add_argument("--workdir")
    one.add_argument("--out")
    one.add_argument("argv", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    try:
        if args.command == "image":
            tag, built = ensure_image(root, runner)
            print(json.dumps({"image": tag, "built": built}), file=out)
            return 0
        command = args.argv[1:] if args.argv[:1] == ["--"] else args.argv
        if not command:
            raise VerifierError("a command is required after --")
        record = verify(root, args.seed, command, args.memory, args.cache, runner, args.workdir)
    except (VerifierError, OSError) as exc:
        print(f"verifier: {exc}", file=sys.stderr)
        return 2
    text = json.dumps(record, indent=2, sort_keys=True)
    if args.out:
        Path(args.out).write_text(text + "\n", encoding="utf-8")
    print(text, file=out)
    return 0 if record.get("exit") == 0 and not record["container_left"] else 1


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
