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
from typing import Any
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

CARGO_CHATTY = """
import sys, time
if "chatty" in sys.argv:
    for i in range(400):
        print("line", i, "x" * 2000, flush=True)
        time.sleep(0.02)
open(@LOG@, "a", encoding="utf-8").write("ran " + sys.argv[-1] + "\\n")
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
        """Ask the daemon to stop; a daemon that does not answer (a wedged worker) is killed, never waited for."""
        def ask():
            try:
                daemon.request(self.state, {"kind": "stop", "token": self.state["token"]})
            except (OSError, RuntimeError):
                pass

        thread = threading.Thread(target=ask, daemon=True)
        thread.start()
        thread.join(5)
        if thread.is_alive():
            os.kill(self.state["pid"], 15)

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

    def disconnect_mid_build(self, reset):
        """Start a chatty build over a raw connection, read its first output, then drop the connection."""
        import socket
        import struct
        sock = socket.create_connection(("127.0.0.1", self.state["port"]), timeout=30)
        if reset:  # an abortive close: the next write on the daemon's side fails with a reset
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
        stream = sock.makefile("rw", encoding="utf-8", newline="\n")
        daemon.send_frame(stream, {"kind": "build", "token": self.state["token"], "argv": ["chatty"],
                                   "cwd": str(self.tmp.name), "label": "gone", "env": {}})
        seen = None
        while seen is None or seen.get("kind") != "out" or "line" not in seen["data"]:
            seen = daemon.recv_frame(stream)
        stream.close()
        sock.close()

    def submit_with_deadline(self, argv, seconds=20):
        result = []
        thread = threading.Thread(target=lambda: result.append(
            daemon.submit(argv, cwd=str(self.tmp.name), env=self.env)), daemon=True)
        thread.start()
        thread.join(seconds)
        return result[0] if result else None

    def test_a_client_that_disconnects_mid_build_does_not_stop_the_daemon(self):
        stub(self.cargo, CARGO_CHATTY, self.log)
        for reset in (False, True):
            self.disconnect_mid_build(reset)
            self.assertEqual(self.submit_with_deadline(["quick", f"after-{reset}"]), 0)
        self.assertIn("ran after-False", self.log.read_text(encoding="utf-8"))
        self.assertIn("ran after-True", self.log.read_text(encoding="utf-8"))

    def test_the_build_of_a_vanished_client_is_stopped_to_free_the_slot(self):
        stub(self.cargo, CARGO_CHATTY, self.log)
        self.disconnect_mid_build(False)
        started = time.time()
        self.assertEqual(self.submit_with_deadline(["quick", "next"]), 0)
        self.assertLess(time.time() - started, 7)  # the chatty build alone would take 8 s or more

    def test_status_is_answered_while_a_build_runs(self):
        stub(self.cargo, CARGO_CHATTY, self.log)
        self.disconnect_mid_build(False)
        answer = []
        thread = threading.Thread(target=lambda: answer.append(daemon.daemon_status(self.env)), daemon=True)
        thread.start()
        thread.join(3)
        self.assertEqual(len(answer), 1)
        self.assertGreaterEqual(answer[0]["active"], 0)

    def _handled_start(self, request):
        """The job_starts entry the daemon records for one request run against a stub cargo."""
        with mock.patch.object(daemon.shutil, "which", return_value=None):
            config = daemon.Config({})
        stats, stream = daemon.new_stats(), io.StringIO()
        proc = mock.Mock(stdout=io.StringIO(""), wait=lambda: 0)
        with mock.patch.object(daemon.subprocess, "Popen", return_value=proc):
            daemon.handle(stream, dict(request, kind="build", argv=[],
                                              cwd=str(self.tmp.name)),
                          config, stats, threading.Event(), 0)
        self.output = stream.getvalue()
        return stats["job_starts"][-1]

    def test_a_client_that_is_gone_before_its_build_starts_gets_no_build(self):
        with mock.patch.object(daemon.subprocess, "Popen") as popen:
            stats, stream = daemon.new_stats(), mock.Mock()
            stream.write.side_effect = BrokenPipeError
            with mock.patch.object(daemon.shutil, "which", return_value=None):
                config = daemon.Config({})
            daemon.handle(stream, {"kind": "build", "argv": []}, config, stats, threading.Event(), 0)
        popen.assert_not_called()
        self.assertEqual((stats["dropped"], stats["completed"]), (1, 0))

    def test_a_closed_stream_is_not_an_error_for_status_and_stop(self):
        stream = io.StringIO()
        stream.close()
        stop = threading.Event()
        config = mock.Mock(describe=dict)
        daemon.handle(stream, {"kind": "status"}, config, daemon.new_stats(), stop)
        daemon.handle(stream, {"kind": "stop"}, config, daemon.new_stats(), stop)
        self.assertTrue(stop.is_set())

    def test_one_failing_request_does_not_end_the_worker(self):
        """The daemon runs in this process here, so its handler can be made to fail once."""
        runtime = Path(self.tmp.name) / "inproc"
        stub(self.cargo, CARGO_CHATTY, self.log)
        env = dict(self.env, NV_BUILD_DAEMON_DIR=str(runtime))
        real, calls = daemon.handle, []

        def flaky(stream, req, config, stats, stop, slot_index=0):
            calls.append(req.get("kind"))
            if len(calls) == 1:
                raise RuntimeError("boom")
            return real(stream, req, config, stats, stop, slot_index)

        with mock.patch.dict(os.environ, env, clear=True), mock.patch.object(daemon, "handle", flaky), \
                mock.patch.object(sys, "stderr", new=io.StringIO()) as log:
            thread = threading.Thread(target=daemon.serve, daemon=True)
            thread.start()
            state = None
            for _ in range(100):
                state = daemon.read_state(runtime)
                if state and daemon.reachable(state):
                    break
                time.sleep(0.05)
            if state is None:
                self.fail("the daemon did not start")
            with self.assertRaises(RuntimeError):  # the failed request's connection is simply closed
                daemon.request(state, {"kind": "build", "token": state["token"], "argv": ["quick", "one"],
                                       "cwd": str(self.tmp.name), "env": {}})
            answer = daemon.request(state, {"kind": "build", "token": state["token"], "argv": ["quick", "two"],
                                            "cwd": str(self.tmp.name), "env": {}})
            self.assertEqual(answer["exit"], 0)
            with mock.patch.object(sys, "stderr", new=io.StringIO()):
                refused = daemon.request(state, {"kind": "build", "token": "wrong", "argv": ["quick"]})
            self.assertEqual(refused["exit"], 2)
            daemon.request(state, {"kind": "stop", "token": state["token"]})
            thread.join(10)
        self.assertIn("ran two", self.log.read_text(encoding="utf-8"))
        self.assertIn("request failed: RuntimeError('boom')", log.getvalue())

    def test_a_worker_survives_a_log_that_cannot_be_written(self):
        runtime = Path(self.tmp.name) / "inproc2"
        stub(self.cargo, CARGO_CHATTY, self.log)
        env = dict(self.env, NV_BUILD_DAEMON_DIR=str(runtime))
        real, calls = daemon.handle, []

        def flaky(stream, req, config, stats, stop, slot_index=0):
            calls.append(req)
            if len(calls) == 1:
                raise RuntimeError("boom")
            return real(stream, req, config, stats, stop, slot_index)

        class Broken(io.StringIO):
            def write(self, text):
                raise BrokenPipeError

        with mock.patch.dict(os.environ, env, clear=True), mock.patch.object(daemon, "handle", flaky), \
                mock.patch.object(sys, "stderr", new=Broken()):
            thread = threading.Thread(target=daemon.serve, daemon=True)
            thread.start()
            state = None
            for _ in range(100):
                state = daemon.read_state(runtime)
                if state and daemon.reachable(state):
                    break
                time.sleep(0.05)
            if state is None:
                self.fail("the daemon did not start")
            with self.assertRaises(RuntimeError):
                daemon.request(state, {"kind": "build", "token": state["token"], "argv": ["quick", "one"],
                                       "cwd": str(self.tmp.name), "env": {}})
            answer = daemon.request(state, {"kind": "build", "token": state["token"], "argv": ["quick", "two"],
                                            "cwd": str(self.tmp.name), "env": {}})
            self.assertEqual(answer["exit"], 0)
            daemon.request(state, {"kind": "stop", "token": state["token"]})
            thread.join(10)

    def test_a_rejected_token_whose_client_has_gone_is_not_an_error(self):
        self.assertFalse(daemon.send_if_connected(mock.Mock(write=mock.Mock(side_effect=ConnectionResetError)), {}))

    def test_a_build_that_cannot_start_reports_why_to_the_client(self):
        sent = []
        with mock.patch.object(daemon.shutil, "which", return_value=None):
            config = daemon.Config({})
        with mock.patch.object(daemon.subprocess, "Popen", side_effect=FileNotFoundError("no cargo")), \
                mock.patch.object(daemon, "send_frame", lambda stream, message: sent.append(message)):
            daemon.handle(io.StringIO(), {"kind": "build", "argv": [], "cwd": str(self.tmp.name)}, config,
                          daemon.new_stats(), threading.Event(), 0)
        self.assertTrue(any("no cargo" in str(m.get("data", "")) for m in sent))
        self.assertEqual(sent[-1], {"kind": "done", "exit": 2})

    def test_the_command_line_without_a_command_prints_its_help(self):
        with mock.patch.object(sys, "stderr", new=io.StringIO()) as err:
            self.assertEqual(daemon.main(["build_daemon.py"]), 2)
        self.assertIn("build", err.getvalue())

    def test_a_lock_that_was_never_entered_exits_quietly(self):
        lock = daemon.FileLock(Path(self.tmp.name) / "l.lock")
        self.assertFalse(lock.__exit__(None, None, None))
        with lock:
            pass
        self.assertTrue(getattr(lock.handle, "closed", False))

    def test_closing_a_stream_whose_client_has_gone_is_not_an_error(self):
        broken, fine = mock.Mock(), mock.Mock()
        broken.close.side_effect = BrokenPipeError
        daemon.close_quietly(broken, fine)
        fine.close.assert_called_once()
        gone = io.StringIO()
        gone.close()
        daemon.close_quietly(gone)

    def test_a_build_nobody_reads_any_more_is_terminated(self):
        proc = mock.Mock(stdout=iter(["one\n", "two\n", "three\n"]), wait=lambda: -15)
        sent = []

        def send(stream, message):
            sent.append(message)
            if len(sent) >= 2 and message.get("kind") == "out" and message["data"] == "two\n":
                raise BrokenPipeError

        with mock.patch.object(daemon.shutil, "which", return_value=None):
            config = daemon.Config({})
        with mock.patch.object(daemon.subprocess, "Popen", return_value=proc), \
                mock.patch.object(daemon, "send_frame", send):
            daemon.handle(io.StringIO(), {"kind": "build", "argv": [], "cwd": str(self.tmp.name)}, config,
                          daemon.new_stats(), threading.Event(), 0)
        proc.terminate.assert_called_once()
        self.assertNotIn("three\n", [m.get("data") for m in sent])

    def test_a_started_job_records_how_long_it_waited_in_the_queue(self):
        entry = self._handled_start({"label": "w", "queued_at": time.time() - 5})
        self.assertGreaterEqual(entry["waited"], 5)
        self.assertLess(entry["waited"], 60)
        self.assertRegex(self.output, r"build daemon: waited [5-9]\.\d+s in the queue")

    def test_a_job_queued_in_the_future_has_zero_wait(self):
        self.assertEqual(self._handled_start({"queued_at": time.time() + 100})["waited"], 0.0)

    def test_a_job_without_a_queue_time_has_no_wait_figure(self):
        self.assertIsNone(self._handled_start({"label": "old client"})["waited"])
        self.assertNotIn("waited", self.output)
        self.assertIsNone(self._handled_start({"queued_at": True})["waited"])
        self.assertIsNone(self._handled_start({"queued_at": "soon"})["waited"])

    def test_a_client_stamps_the_time_it_queued_the_build(self):
        stub(self.cargo, CARGO_LIMITS, self.log)
        before = time.time()
        self.assertEqual(daemon.submit(["check"], cwd=str(self.tmp.name), env=self.env,
                                       label="stamped"), 0)
        status: dict[str, Any] = daemon.daemon_status(self.env)
        entry = [j for j in status["job_starts"] if j["label"] == "stamped"][-1]
        self.assertIsNotNone(entry["waited"])
        self.assertGreaterEqual(entry["start"], before)


if __name__ == "__main__":
    unittest.main()
