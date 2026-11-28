# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the sandboxed verifier (scripts/fork/verifier.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
No docker and no network: the container runtime is a stand-in; the file copy runs for real (with a plain copy when
the temporary directory is not on a copy-on-write filesystem).
"""
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import verifier

TOOLCHAIN = '[toolchain]\nchannel = "1.100.0"\nprofile = "minimal"\n'
GOOD = ("test result: ok. 2 passed\nVERIFIER_EXIT=0\nVERIFIER_PEAK=5627412480\n"
        "VERIFIER_SCCACHE Cache hits=40\nVERIFIER_SCCACHE Cache misses=2\n")


class Docker:
    """A stand-in for the container runtime and for `cp`: it records calls and answers by command."""

    def __init__(self, image=True, build=0, run_output=GOOD, run_code=0, leaves_container=False, cp_code=0):
        self.calls, self.image, self.build = [], image, build
        self.run_output, self.run_code, self.leaves, self.cp_code = run_output, run_code, leaves_container, cp_code
        self.cloned_to = None

    def __call__(self, argv, cwd=".", input=None):
        argv = [str(a) for a in argv]
        self.calls.append((argv, input))
        if argv[:3] == ["docker", "image", "inspect"]:
            return (0 if self.image else 1), ""
        if argv[:2] == ["docker", "build"]:
            return self.build, "build output\n" if not self.build else "no space\n"
        if argv[0] == "cp":
            if self.cp_code:
                return self.cp_code, "cp: cannot clone\n"
            dest = Path(argv[-1])
            dest.mkdir(parents=True)
            (dest / "marker").write_text("x", encoding="utf-8")
            self.cloned_to = dest
            return 0, ""
        if argv[:2] == ["docker", "run"]:
            return self.run_code, self.run_output
        if argv[:3] == ["docker", "ps", "-a"]:
            return 0, "left\n" if self.leaves else ""
        return 0, ""


class Repo(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name) / "repo"
        self.root.mkdir()
        (self.root / "rust-toolchain.toml").write_text(TOOLCHAIN, encoding="utf-8")
        (self.root / "Cargo.lock").write_text("lock one\n", encoding="utf-8")
        self.seed = Path(self.tmp.name) / "seed"
        self.seed.mkdir()
        self.work = Path(self.tmp.name) / "work"


class Image(Repo):
    def test_the_channel_comes_from_the_toolchain_file(self):
        self.assertEqual(verifier.channel(self.root), "1.100.0")
        (self.root / "rust-toolchain.toml").write_text("[toolchain]\n", encoding="utf-8")
        with self.assertRaises(verifier.VerifierError):
            verifier.channel(self.root)

    def test_the_dockerfile_pins_every_tool_with_its_digest(self):
        text = verifier.dockerfile("1.100.0")
        self.assertTrue(text.startswith("FROM rust:1.100.0-slim\n"))
        for name, url, digest, _ in verifier.TOOLS:
            self.assertIn(url, text)
            self.assertIn(f"{digest}  /tmp/{name}.tgz", text)
            self.assertRegex(digest, r"^[0-9a-f]{64}$")
        self.assertIn("sha256sum -c", text)
        self.assertIn("SCCACHE_DIR=/sccache", text)

    def test_the_tag_changes_with_the_lock_file_and_the_toolchain(self):
        first = verifier.image_tag(self.root)
        self.assertRegex(first, r"^nv-rs-verifier:[0-9a-f]{12}$")
        self.assertEqual(verifier.image_tag(self.root), first)
        (self.root / "Cargo.lock").write_text("lock two\n", encoding="utf-8")
        second = verifier.image_tag(self.root)
        self.assertNotEqual(second, first)
        (self.root / "rust-toolchain.toml").write_text(TOOLCHAIN.replace("1.100.0", "1.101.0"), encoding="utf-8")
        self.assertNotEqual(verifier.image_tag(self.root), second)

    def test_an_existing_image_is_not_built_again(self):
        docker = Docker(image=True)
        tag, built = verifier.ensure_image(self.root, docker)
        self.assertEqual((tag, built), (verifier.image_tag(self.root), False))
        self.assertFalse([c for c, _ in docker.calls if c[:2] == ["docker", "build"]])

    def test_a_missing_image_is_built_from_the_dockerfile_on_stdin(self):
        docker = Docker(image=False)
        tag, built = verifier.ensure_image(self.root, docker)
        self.assertTrue(built)
        argv, text = next(c for c in docker.calls if c[0][:2] == ["docker", "build"])
        self.assertEqual(argv[-3:], ["-t", tag, "-"])
        self.assertEqual(text, verifier.dockerfile("1.100.0"))

    def test_a_failing_build_is_an_error(self):
        with self.assertRaisesRegex(verifier.VerifierError, "no space"):
            verifier.ensure_image(self.root, Docker(image=False, build=1))


class Seed(Repo):
    def test_the_seed_is_copied_copy_on_write_and_never_written(self):
        docker = Docker()
        verifier.clone_seed(self.seed, self.work / "t", docker)
        argv = docker.calls[-1][0]
        self.assertEqual(argv[:3], ["cp", "-a", "--reflink=always"])
        self.assertEqual(argv[3], str(self.seed))

    def test_a_missing_seed_or_an_existing_target_is_refused(self):
        with self.assertRaises(verifier.VerifierError):
            verifier.clone_seed(self.seed / "nope", self.work / "t", Docker())
        (self.work / "t").mkdir(parents=True)
        with self.assertRaises(verifier.VerifierError):
            verifier.clone_seed(self.seed, self.work / "t", Docker())

    def test_a_failed_copy_names_the_filesystem_and_leaves_nothing(self):
        with self.assertRaisesRegex(verifier.VerifierError, "btrfs"):
            verifier.clone_seed(self.seed, self.work / "t", Docker(cp_code=1))
        self.assertFalse((self.work / "t").exists())

    def test_the_real_copy_command_copies_a_tree_where_reflinks_work(self):
        dest = Path(self.tmp.name) / "real"
        (self.seed / "a").write_text("1", encoding="utf-8")
        try:
            verifier.clone_seed(self.seed, dest)
        except verifier.VerifierError as exc:
            self.skipTest(f"no copy-on-write here: {exc}")
        self.assertEqual((dest / "a").read_text(encoding="utf-8"), "1")


class Running(Repo):
    def verify(self, docker, **kw):
        return verifier.verify(self.root, self.seed, ["cargo", "test"], runner=docker, workdir=self.work, **kw)

    def test_the_container_is_capped_offline_and_sees_the_repository_at_its_own_path(self):
        docker = Docker()
        self.verify(docker, memory="12g")
        argv = next(c for c, _ in docker.calls if c[:2] == ["docker", "run"])
        self.assertIn("--rm", argv)
        self.assertEqual(argv[argv.index("--memory") + 1], "12g")
        self.assertEqual(argv[argv.index("--memory-swap") + 1], "12g")
        self.assertEqual(argv[argv.index("--network") + 1], "none")
        root = str(self.root.resolve())
        self.assertIn(f"{root}:{root}", argv)
        self.assertEqual(argv[argv.index("-w") + 1], root)
        self.assertIn(f"{self.work}/sccache:/sccache", argv)
        self.assertTrue(any(a.endswith("/registry:/usr/local/cargo/registry:ro") for a in argv))
        self.assertEqual(argv[-2:], ["cargo", "test"])

    def test_the_record_has_times_peak_memory_hits_and_what_was_left(self):
        record = self.verify(Docker())
        self.assertEqual((record["exit"], record["peak_bytes"]), (0, 5627412480))
        self.assertEqual((record["sccache_hits"], record["sccache_misses"]), (40, 2))
        self.assertFalse(record["container_left"])
        self.assertEqual(set(record) >= {"clone_seconds", "run_seconds", "disk_left_by_run", "image", "container"}, True)
        self.assertEqual(record["disk_left_by_run"], record["disk_used_after"] - record["disk_used_before"])

    def test_the_copy_and_the_container_are_removed_after_a_failing_run_too(self):
        docker = Docker(run_code=1, run_output="VERIFIER_EXIT=1\n")
        record = self.verify(docker)
        self.assertEqual(record["exit"], 1)
        self.assertFalse(docker.cloned_to and docker.cloned_to.exists())
        self.assertTrue(any(c[:3] == ["docker", "rm", "-f"] for c, _ in docker.calls))

    def test_the_copy_is_removed_when_the_runtime_itself_blows_up(self):
        docker = Docker()

        def boom(argv, cwd=".", input=None):
            if argv[0] == "docker" and argv[1] == "run":
                raise RuntimeError("daemon gone")
            return docker(argv, cwd, input)

        with self.assertRaises(RuntimeError):
            verifier.verify(self.root, self.seed, ["x"], runner=boom, workdir=self.work)
        self.assertFalse(docker.cloned_to and docker.cloned_to.exists())

    def test_stats_come_from_the_marker_lines_only(self):
        self.assertEqual(verifier.parse_stats("noise\nVERIFIER_EXIT=3\nVERIFIER_PEAK=\n"),
                         {"exit": 3, "peak_bytes": None, "sccache_hits": None, "sccache_misses": None})
        self.assertEqual(verifier.parse_stats("")["exit"], None)

    def test_a_relative_workdir_and_cache_reach_docker_as_absolute_paths_and_are_created(self):
        docker = Docker()
        with tempfile.TemporaryDirectory() as cwd:
            import os
            old = os.getcwd()
            os.chdir(cwd)
            try:
                verifier.verify(self.root, self.seed, ["x"], runner=docker, workdir="w/deep", cache="c")
            finally:
                os.chdir(old)
            argv = next(c for c, _ in docker.calls if c[:2] == ["docker", "run"])
            mounts = [a for a in argv if a.endswith(":/sccache")]
            self.assertEqual(mounts, [f"{Path(cwd).resolve()}/c:/sccache"])
            self.assertTrue((Path(cwd) / "w" / "deep").is_dir())

    def test_a_refused_copy_never_removes_a_path_the_run_did_not_make(self):
        docker = Docker()
        keep = []

        def refuse(argv, cwd=".", input=None):
            if argv[0] == "cp":
                dest = Path(argv[-1])
                dest.mkdir(parents=True)
                (dest / "precious").write_text("x", encoding="utf-8")
                keep.append(dest)
                return 1, "no"
            return docker(argv, cwd, input)

        with mock.patch.object(verifier.shutil, "rmtree") as rmtree, self.assertRaises(verifier.VerifierError):
            verifier.verify(self.root, self.seed, ["x"], runner=refuse, workdir=self.work)
        for call in rmtree.call_args_list:  # only clone_seed's own cleanup of its failed copy
            self.assertEqual(call.args[0], keep[0])
        self.assertEqual(rmtree.call_count, 1)

    def test_the_workdir_holds_only_the_cache_after_a_run(self):
        record = self.verify(Docker())
        self.assertEqual(record["workdir_bytes_after"], 0)
        (self.work / "sccache" / "f").write_bytes(b"12345")
        link = self.work / "sccache" / "link"
        link.symlink_to(self.work / "sccache" / "f")
        self.assertEqual(verifier.tree_bytes(self.work), 5)
        self.assertEqual([p.name for p in self.work.iterdir()], ["sccache"])

    def test_an_empty_exit_marker_or_unicode_digits_are_not_a_status(self):
        got = verifier.parse_stats("VERIFIER_EXIT=\nVERIFIER_EXIT=\u0663\nVERIFIER_EXIT=7\n")
        self.assertEqual(got["exit"], 7)
        self.assertIsNone(verifier.parse_stats("VERIFIER_EXIT=\n")["exit"])

    def test_a_file_that_vanishes_during_the_walk_is_skipped(self):
        (self.work / "sccache").mkdir(parents=True)
        (self.work / "sccache" / "a").write_bytes(b"123")
        (self.work / "sccache" / "b").write_bytes(b"45")
        real = verifier.os.lstat

        def flaky(path):
            if str(path).endswith("/b"):
                raise FileNotFoundError(path)
            return real(path)

        with mock.patch.object(verifier.os, "lstat", flaky):
            self.assertEqual(verifier.tree_bytes(self.work), 3)

    def test_peak_memory_is_read_from_either_cgroup_layout(self):
        self.assertIn("/sys/fs/cgroup/memory.peak", verifier.INNER)
        self.assertIn("/sys/fs/cgroup/memory/memory.max_usage_in_bytes", verifier.INNER)

    def test_used_bytes_reads_the_filesystem(self):
        self.assertGreater(verifier.used_bytes(self.tmp.name), 0)

    def test_run_gives_127_for_a_missing_program(self):
        self.assertEqual(verifier.run([sys.executable, "-c", "print(1)"])[0], 0)
        self.assertEqual(verifier.run(["cat"], input="x")[1], "x")
        self.assertEqual(verifier.run(["no-such-program-here"])[0], 127)


class Main(Repo):
    def go(self, *args, docker=None):
        out = io.StringIO()
        with mock.patch.object(verifier, "used_bytes", return_value=10):
            code = verifier.main(list(args), root=self.root, runner=docker or Docker(), out=out)
        return code, out.getvalue()

    def test_image_prints_the_tag(self):
        code, out = self.go("image")
        self.assertEqual((code, json.loads(out)["built"]), (0, False))

    def test_run_prints_the_record_and_exits_by_the_command(self):
        target = Path(self.tmp.name) / "rec.json"
        code, out = self.go("run", "--seed", str(self.seed), "--workdir", str(self.work), "--out", str(target),
                            "--", "cargo", "test")
        self.assertEqual((code, json.loads(out)["exit"]), (0, 0))
        self.assertEqual(json.loads(target.read_text(encoding="utf-8"))["image"], json.loads(out)["image"])
        failing = Docker(run_code=1, run_output="VERIFIER_EXIT=1\n")
        self.assertEqual(self.go("run", "--seed", str(self.seed), "--workdir", str(self.work), "--", "x", docker=failing)[0], 1)

    def test_a_leftover_container_is_a_failure_even_when_the_command_passed(self):
        code, _ = self.go("run", "--seed", str(self.seed), "--workdir", str(self.work), "--", "x",
                          docker=Docker(leaves_container=True))
        self.assertEqual(code, 1)

    def test_errors_exit_with_two(self):
        self.assertEqual(self.go("run", "--seed", str(self.seed), "--workdir", str(self.work))[0], 2)
        self.assertEqual(self.go("run", "--seed", str(self.seed / "none"), "--workdir", str(self.work), "--", "x")[0], 2)

    def test_the_script_runs_as_a_command(self):
        import subprocess
        done = subprocess.run([sys.executable, "-I", str(HERE.parent / "verifier.py"), "run", "--seed", "/none"],
                              capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 2)


if __name__ == "__main__":
    unittest.main()
