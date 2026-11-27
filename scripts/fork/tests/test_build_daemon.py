# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the build daemon (scripts/fork/build_daemon.py), with synthetic cargo stubs.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
No cargo, no network outside loopback, no files outside temporary directories.
"""
import io
import os
import shlex
import subprocess
import sys
import tempfile
import threading
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


def stub(path, program, log):
    """Write a Python stub, invoked through the interpreter in the command prefix.

    The log path is baked into the stub: the daemon forwards only the variables
    cargo needs, so a test-only NV_* variable cannot reach the build.
    """
    path.write_text(program.replace("@LOG@", repr(str(log))), encoding="utf-8")
    return path


CARGO_ORDER = """
import os, sys, time
LOG = @LOG@
with open(LOG, "a", encoding="utf-8") as log:
    log.write("start " + sys.argv[-1] + "\\n"); log.flush()
    time.sleep(0.2)
    log.write("end " + sys.argv[-1] + "\\n")
"""

CARGO_SLEEP = """
import os, sys, time
LOG = @LOG@
with open(LOG, "a", encoding="utf-8") as log:
    log.write("start " + sys.argv[-1] + " " + str(time.time()) + "\\n"); log.flush()
    time.sleep(0.3)
    log.write("end " + sys.argv[-1] + " " + str(time.time()) + "\\n")
"""

CARGO_LIMITS = """
import os, sys
LOG = @LOG@
with open(LOG, "a", encoding="utf-8") as log:
    log.write("jobs=" + os.environ.get("CARGO_BUILD_JOBS", "") + "\\n")
    log.write("target=" + os.environ.get("CARGO_TARGET_DIR", "") + "\\n")
    log.write("wrapper=" + os.environ.get("RUSTC_WRAPPER", "") + "\\n")
    log.write("argv=" + " ".join(sys.argv[1:]) + "\\n")
"""

CARGO_CACHE = """
import os, sys
LOG = @LOG@
os.makedirs(os.environ["CARGO_TARGET_DIR"], exist_ok=True)
marker = os.path.join(os.environ["CARGO_TARGET_DIR"], "typed_fade.marker")
with open(LOG, "a", encoding="utf-8") as log:
    log.write("wrapper=" + os.environ.get("RUSTC_WRAPPER", "") + "\\n")
    if os.path.exists(marker):
        log.write("cache hit: typed_fade\\n")
    else:
        open(marker, "w", encoding="utf-8").close()
        log.write("compiled: typed_fade\\n")
"""

WRAPPER_STUB = """
import os, subprocess, sys
LOG = @LOG@
with open(LOG, "a", encoding="utf-8") as log:
    log.write(os.path.basename(sys.argv[0]) + " " + sys.argv[1] + "\\n")
raise SystemExit(subprocess.call(sys.argv[2:]))
"""


class BuildDaemon(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.log = root / "log.txt"
        self.cargo = stub(root / "cargo", CARGO_ORDER, self.log)
        self.capped = stub(root / "capped", WRAPPER_STUB, self.log)
        self.flock = stub(root / "flock", WRAPPER_STUB, self.log)
        self.env = os.environ.copy()
        self.env.update({
            "NV_BUILD_DAEMON_DIR": str(root / "daemon"),
            "NV_BUILD_CARGO": prefix([sys.executable, str(self.cargo)]),
            "NV_BUILD_CAPPED": prefix([sys.executable, str(self.capped)]),
            "NV_BUILD_FLOCK": prefix([sys.executable, str(self.flock)]),
            "NV_BUILD_LOCK": str(root / "cargo.lock"),
            "NV_BUILD_TARGET_DIR": str(root / "target"),
            "NV_BUILD_WRAPPER": "sccache-stub",
            "NV_BUILD_SLOTS": "1",
        })
        self.state = daemon.ensure_daemon(self.env)
        self.addCleanup(self._stop)

    def restart(self, **env):
        """Stop the daemon and start one with these variables (a different slot count)."""
        self._stop()
        deadline = time.time() + 15
        while daemon.reachable(self.state) and time.time() < deadline:
            time.sleep(0.05)
        self.env.update(env)
        self.state = daemon.ensure_daemon(self.env)

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
        stub(self.cargo, CARGO_LIMITS, self.log)
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
        stub(self.cargo, "#!/usr/bin/env python3\n" + CARGO_CACHE, self.log)
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

    def test_the_lock_parent_directory_is_created(self):
        base = Path(self.tmp.name) / "cache"
        with mock.patch.object(daemon, "default_path",
                               side_effect=lambda env, leaf: str(base / leaf)):
            config = daemon.Config({})
        self.assertTrue(Path(config.lock).parent.is_dir())

    def test_a_missing_memory_cap_is_reported_to_the_client(self):
        with mock.patch.object(daemon.shutil, "which", return_value=None):
            config = daemon.Config({})
        self.assertEqual(config.capped, [])
        self.assertIn("NV_BUILD_CAPPED", " ".join(config.warnings))
        self.assertEqual(config.warnings, config.describe()["warnings"])
        stream = io.StringIO()
        stats = daemon.new_stats()
        proc = mock.Mock(stdout=io.StringIO(""), wait=lambda: 0)
        with mock.patch.object(daemon.subprocess, "Popen", return_value=proc):
            daemon.handle(stream, {"kind": "build", "argv": [],
                                   "cwd": str(self.tmp.name)},
                          config, stats, threading.Event(), 0)
        self.assertIn("warning", stream.getvalue().lower())
        self.assertIn("NV_BUILD_CAPPED", stream.getvalue())

    def test_only_the_variables_cargo_needs_reach_the_build(self):
        passed = {"NV_SECRET": "hunter2", "NV_BUILD_JOBS": "9",
                  "PATH": os.defpath}
        forwarded = daemon.pass_env(passed)
        self.assertIn("PATH", forwarded)
        self.assertNotIn("NV_SECRET", forwarded)
        self.assertNotIn("NV_BUILD_JOBS", forwarded)
        with mock.patch.dict(os.environ, {}, clear=True):
            env = daemon.Config({}).build_env({}, passed)
        self.assertNotIn("NV_SECRET", env)
        self.assertNotIn("NV_BUILD_JOBS", env)
        self.assertEqual(env["CARGO_BUILD_JOBS"], "2")
        self.assertEqual(env["PATH"], os.defpath)

    def test_multiple_slots_run_jobs_in_parallel(self):
        """With 3 slots, three jobs' start/end intervals should overlap."""
        self.restart(NV_BUILD_SLOTS="3")
        env = dict(self.env)
        stub(self.cargo, CARGO_SLEEP, self.log)
        procs = []
        for label in ("J1", "J2", "J3"):
            env_label = dict(env, NV_BUILD_LABEL=label)
            procs.append(subprocess.Popen(
                [sys.executable, str(ENTRY), "run", "--", "check", "--", label],
                env=env_label, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            ))
        for proc in procs:
            out, err = proc.communicate(timeout=30)
            self.assertEqual(proc.returncode, 0, err)
        lines = self.lines()
        starts = [l for l in lines if l.startswith("start ")]
        ends = [l for l in lines if l.startswith("end ")]
        self.assertEqual(len(starts), 3)
        self.assertEqual(len(ends), 3)
        # Parse timestamps: "start J1 123456.789"
        start_times = {}
        end_times = {}
        for s in starts:
            _, label, ts = s.split()
            start_times[label] = float(ts)
        for e in ends:
            _, label, ts = e.split()
            end_times[label] = float(ts)
        # With 3 slots, all three should overlap in time
        # Check that max start < min end (they ran concurrently)
        self.assertLess(max(start_times.values()), min(end_times.values()))

    def test_single_slot_runs_jobs_sequentially(self):
        """With 1 slot, jobs should not overlap."""
        env = dict(self.env)
        stub(self.cargo, CARGO_SLEEP, self.log)
        procs = []
        for label in ("J1", "J2", "J3"):
            env_label = dict(env, NV_BUILD_LABEL=label)
            procs.append(subprocess.Popen(
                [sys.executable, str(ENTRY), "run", "--", "check", "--", label],
                env=env_label, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            ))
        for proc in procs:
            out, err = proc.communicate(timeout=30)
            self.assertEqual(proc.returncode, 0, err)
        lines = self.lines()
        starts = [l for l in lines if l.startswith("start ")]
        ends = [l for l in lines if l.startswith("end ")]
        self.assertEqual(len(starts), 3)
        self.assertEqual(len(ends), 3)
        # Parse timestamps
        start_times = {}
        end_times = {}
        for s in starts:
            _, label, ts = s.split()
            start_times[label] = float(ts)
        for e in ends:
            _, label, ts = e.split()
            end_times[label] = float(ts)
        # With 1 slot, each job ends before the next starts
        # Sort by start time
        labels_by_start = sorted(start_times.keys(), key=lambda k: start_times[k])
        for i in range(len(labels_by_start) - 1):
            self.assertLessEqual(end_times[labels_by_start[i]], start_times[labels_by_start[i+1]])

    def test_status_reports_slots_and_queue(self):
        """Status should include slots info and running jobs per slot."""
        stub(self.cargo, CARGO_LIMITS, self.log)
        self.restart(NV_BUILD_SLOTS="3")
        state = self.state
        status = daemon.request(state, {"kind": "status", "token": state["token"]})["status"]
        self.assertEqual(status["config"]["slots"], 3)
        self.assertEqual(status["queued"], 0)
        self.assertIn("slot_active", status)
        self.assertIn("slot_max_active", status)


if __name__ == "__main__":
    unittest.main()
