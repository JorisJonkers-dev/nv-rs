# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the build daemon (scripts/fork/build_daemon.py), with synthetic cargo stubs.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
No cargo, no network outside loopback, no files outside temporary directories.
"""
import os
import shlex
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

FORK = Path(__file__).resolve().parents[1]
ENTRY = FORK / "build_daemon.py"
sys.path.insert(0, str(FORK))

import build_daemon as daemon  # noqa: E402


def prefix(program):
    """A command prefix as an environment value, quoted for the host shell."""
    if os.name == "nt":
        return subprocess.list2cmdline(program)
    return shlex.join(program)


def stub(path, program):
    """Write a Python stub, invoked through the interpreter in the command prefix."""
    path.write_text(program, encoding="utf-8")
    return path


CARGO_ORDER = """
import os, sys, time
with open(os.environ["NV_TEST_LOG"], "a", encoding="utf-8") as log:
    log.write("start " + sys.argv[-1] + "\\n"); log.flush()
    time.sleep(0.2)
    log.write("end " + sys.argv[-1] + "\\n")
"""

CARGO_LIMITS = """
import os, sys
with open(os.environ["NV_TEST_LOG"], "a", encoding="utf-8") as log:
    log.write("jobs=" + os.environ.get("CARGO_BUILD_JOBS", "") + "\\n")
    log.write("target=" + os.environ.get("CARGO_TARGET_DIR", "") + "\\n")
    log.write("wrapper=" + os.environ.get("RUSTC_WRAPPER", "") + "\\n")
    log.write("argv=" + " ".join(sys.argv[1:]) + "\\n")
"""

CARGO_CACHE = """
import os, sys
os.makedirs(os.environ["CARGO_TARGET_DIR"], exist_ok=True)
marker = os.path.join(os.environ["CARGO_TARGET_DIR"], "typed_fade.marker")
with open(os.environ["NV_TEST_LOG"], "a", encoding="utf-8") as log:
    log.write("wrapper=" + os.environ.get("RUSTC_WRAPPER", "") + "\\n")
    if os.path.exists(marker):
        log.write("cache hit: typed_fade\\n")
    else:
        open(marker, "w", encoding="utf-8").close()
        log.write("compiled: typed_fade\\n")
"""

WRAPPER_STUB = """
import os, subprocess, sys
with open(os.environ["NV_TEST_LOG"], "a", encoding="utf-8") as log:
    log.write(os.path.basename(sys.argv[0]) + " " + sys.argv[1] + "\\n")
raise SystemExit(subprocess.call(sys.argv[2:]))
"""


class BuildDaemon(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.log = root / "log.txt"
        self.cargo = stub(root / "cargo", CARGO_ORDER)
        self.capped = stub(root / "capped", WRAPPER_STUB)
        self.flock = stub(root / "flock", WRAPPER_STUB)
        self.env = os.environ.copy()
        self.env.update({
            "NV_BUILD_DAEMON_DIR": str(root / "daemon"),
            "NV_BUILD_CARGO": prefix([sys.executable, str(self.cargo)]),
            "NV_BUILD_CAPPED": prefix([sys.executable, str(self.capped)]),
            "NV_BUILD_FLOCK": prefix([sys.executable, str(self.flock)]),
            "NV_BUILD_LOCK": str(root / "cargo.lock"),
            "NV_BUILD_TARGET_DIR": str(root / "target"),
            "NV_BUILD_WRAPPER": "sccache-stub",
            "NV_TEST_LOG": str(self.log),
        })
        self.state = daemon.ensure_daemon(self.env)
        self.addCleanup(self._stop)

    def _stop(self):
        try:
            daemon.request(self.state, {"kind": "stop", "token": self.state["token"]})
        except OSError:
            pass

    def lines(self):
        return self.log.read_text(encoding="utf-8").splitlines() if self.log.exists() else []

    def test_two_workers_requesting_builds_are_served_in_order(self):
        procs = []
        for label in ("A", "B"):
            env = dict(self.env, NV_BUILD_LABEL=label)
            procs.append(subprocess.Popen(
                [sys.executable, str(ENTRY), "run", "--", "check", "--", label],
                env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            ))
            if label == "A":
                time.sleep(0.25)
        for proc in procs:
            out, err = proc.communicate(timeout=30)
            self.assertEqual(proc.returncode, 0, err)
        self.assertEqual(
            [line for line in self.lines() if line.startswith(("start ", "end "))],
            ["start A", "end A", "start B", "end B"],
        )
        status = daemon.request(self.state, {"kind": "status",
                                             "token": self.state["token"]})["status"]
        self.assertEqual(status["max_active"], 1)
        self.assertEqual(status["served"][-2:], ["A", "B"])

    def test_the_daemon_enforces_the_job_and_memory_limits(self):
        self.cargo.write_text(CARGO_LIMITS, encoding="utf-8")
        code = daemon.submit(["check", "--verbose"], cwd=str(Path(self.tmp.name)),
                             label="limits", env=self.env)
        self.assertEqual(code, 0)
        lines = self.lines()
        self.assertIn("jobs=2", lines)
        self.assertIn("target=" + self.env["NV_BUILD_TARGET_DIR"], lines)
        self.assertIn("wrapper=sccache-stub", lines)
        self.assertIn("capped 10G", lines)
        self.assertIn("flock " + self.env["NV_BUILD_LOCK"], lines)
        self.assertIn("argv=check --verbose", lines)

    def test_a_warm_rebuild_of_a_family_target_hits_the_cache(self):
        self.cargo.write_text("#!/usr/bin/env python3\n" + CARGO_CACHE, encoding="utf-8")
        self.cargo.chmod(0o755)
        argv = ["test", "--test", "typed_fade"]
        self.assertEqual(daemon.submit(argv, cwd=str(Path(self.tmp.name)), env=self.env), 0)
        self.assertEqual(daemon.submit(argv, cwd=str(Path(self.tmp.name)), env=self.env), 0)
        lines = self.lines()
        self.assertEqual(lines.count("compiled: typed_fade"), 1)
        self.assertEqual(lines.count("cache hit: typed_fade"), 1)
        self.assertEqual(lines.count("wrapper=sccache-stub"), 2)

    def test_sccache_is_the_default_and_limits_come_from_configuration(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            config = daemon.Config({})
        self.assertEqual(config.jobs, "2")
        self.assertEqual(config.mem, "10G")
        self.assertEqual(config.wrapper, "sccache")
        other = daemon.Config({"NV_BUILD_JOBS": "3", "NV_BUILD_MEM": "8G",
                               "NV_BUILD_WRAPPER": "other-wrapper"})
        self.assertEqual(other.jobs, "3")
        self.assertEqual(other.mem, "8G")
        self.assertEqual(other.wrapper, "other-wrapper")


if __name__ == "__main__":
    unittest.main()
