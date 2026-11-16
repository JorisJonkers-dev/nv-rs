"""Tests of scripts/fork/bench/d3d9replay.py: stream names and the bench lock around a run (a fake binary)."""
import os
import stat
import sys
import tempfile
import unittest
import unittest.mock
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "bench"))

import lock  # noqa: E402
import guard  # noqa: E402
import d3d9replay as replay  # noqa: E402


def plain(cmd, timeout=None, out=print):
    """The guarded run without the machine: the fake binary as it is (guard.run_guarded is tested in test_guard.py)."""
    import subprocess
    return subprocess.call(cmd, timeout=timeout)


class ResolveTests(unittest.TestCase):
    def test_a_name_is_a_stream_in_the_folder(self):
        with tempfile.TemporaryDirectory() as d:
            s = Path(d)
            (s / "doc-room.nvrs").write_bytes(b"x")
            self.assertEqual(replay.resolve("doc-room", s), s / "doc-room.nvrs")
            self.assertEqual(replay.resolve("doc-room.nvrs", s), s / "doc-room.nvrs")
            self.assertEqual(replay.resolve(str(s / "doc-room.nvrs"), s), s / "doc-room.nvrs")
            self.assertIsNone(replay.resolve("street", s))
            self.assertEqual([p.name for p in replay.listing(s)], ["doc-room.nvrs"])
            self.assertEqual(replay.listing(s / "missing"), [])


class GuardTests(unittest.TestCase):
    def test_a_replay_run_goes_through_the_memory_guard(self):
        with tempfile.TemporaryDirectory() as d:
            os.environ["NV_BENCH_LOCK"] = str(Path(d) / "bench.lock")
            try:
                seen = []
                rc = replay.run_locked(["run", "x"], exe=Path(d) / "fake", other=lambda: 0, out=print,
                                       guarded=lambda cmd, timeout=None, out=print: seen.append(cmd) or 4)
                self.assertEqual(rc, 4)
                self.assertEqual(seen, [[str(Path(d) / "fake"), "run", "x"]])
            finally:
                del os.environ["NV_BENCH_LOCK"]

    def test_the_guarded_run_is_refused_while_halted_or_without_room(self):
        with tempfile.TemporaryDirectory() as d:
            halt = Path(d) / "halt"
            said = []
            with unittest.mock.patch.object(guard, "HALT", halt):
                guard.halt("kernel: GPU reset")
                self.assertEqual(guard.run_guarded(["true"], out=said.append, popen=None), 4)
                guard.clear_halt()
                full = guard.GpuMem(24000.0, 24560.0, 100.0, 16002.0)
                with unittest.mock.patch.object(guard, "read_gpu", return_value=full), \
                        unittest.mock.patch.object(guard, "mem_available", return_value=16 << 30):
                    self.assertEqual(guard.run_guarded(["true"], gpu_dir="/x", out=said.append, popen=None), 4)
            self.assertIn("halted", said[0])
            self.assertIn("VRAM", said[1])


class LockTests(unittest.TestCase):
    def fake(self, d, status):
        exe = Path(d) / "fake"
        exe.write_text(f"#!/bin/sh\necho ran \"$@\" >> '{d}/log'\nexit {status}\n")
        exe.chmod(exe.stat().st_mode | stat.S_IEXEC)
        return exe

    def test_a_run_holds_the_bench_lock_and_gives_the_status(self):
        with tempfile.TemporaryDirectory() as d:
            os.environ["NV_BENCH_LOCK"] = str(Path(d) / "bench.lock")
            try:
                said = []
                rc = replay.run_locked(["run", "x"], exe=self.fake(d, 7), other=lambda: 0, out=said.append, guarded=plain)
                self.assertEqual(rc, 7)
                self.assertEqual((Path(d) / "log").read_text(), "ran run x\n")
                # The lock is free again afterwards.
                again = lock.BenchLock(wait_timeout=0.1, out=print)
                self.assertTrue(again.acquire())
                again.release()
            finally:
                del os.environ["NV_BENCH_LOCK"]

    def test_a_held_lock_makes_the_run_give_up_with_status_3(self):
        with tempfile.TemporaryDirectory() as d:
            os.environ["NV_BENCH_LOCK"] = str(Path(d) / "bench.lock")
            try:
                holder = lock.BenchLock(wait_timeout=1, out=print)
                self.assertTrue(holder.acquire())
                said = []
                rc = replay.run_locked(["run", "x"], wait_timeout=0.2, exe=self.fake(d, 0), other=lambda: 0, out=said.append, guarded=plain)
                holder.release()
                self.assertEqual(rc, 3)
                self.assertFalse((Path(d) / "log").exists())
            finally:
                del os.environ["NV_BENCH_LOCK"]

    def test_other_viewers_are_reported(self):
        with tempfile.TemporaryDirectory() as d:
            os.environ["NV_BENCH_LOCK"] = str(Path(d) / "bench.lock")
            try:
                said = []
                replay.run_locked(["run"], exe=self.fake(d, 0), other=lambda: 2, out=said.append, guarded=plain)
                self.assertTrue(any(s.startswith("NOISY: 2") for s in said))
            finally:
                del os.environ["NV_BENCH_LOCK"]


if __name__ == "__main__":
    unittest.main()
