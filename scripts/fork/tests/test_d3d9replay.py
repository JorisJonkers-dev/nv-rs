"""Tests of scripts/fork/bench/d3d9replay.py: stream names and the bench lock around a run (a fake binary)."""
import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "bench"))

import lock  # noqa: E402
import d3d9replay as replay  # noqa: E402


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
                rc = replay.run_locked(["run", "x"], exe=self.fake(d, 7), other=lambda: 0, out=said.append)
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
                rc = replay.run_locked(["run", "x"], wait_timeout=0.2, exe=self.fake(d, 0), other=lambda: 0, out=said.append)
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
                replay.run_locked(["run"], exe=self.fake(d, 0), other=lambda: 2, out=said.append)
                self.assertTrue(any(s.startswith("NOISY: 2") for s in said))
            finally:
                del os.environ["NV_BENCH_LOCK"]


if __name__ == "__main__":
    unittest.main()
