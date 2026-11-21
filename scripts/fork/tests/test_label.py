# SPDX-License-Identifier: GPL-3.0-only
"""Run labels (scripts/fork/bench/label.py): the label's form, the renamed program, the scope's arguments and the check
for other viewers that no longer has the comm `nv-viewer`. No game data: the 'program' is a copy of the interpreter."""
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "bench"))

import label  # noqa: E402
import lock  # noqa: E402
import guard  # noqa: E402


class FormTests(unittest.TestCase):
    def test_default_is_bench_scenario_variant(self):
        self.assertEqual(label.bench_label("views", "/x/runs/lod0151/game", env={}), "bench-views-lod0151")
        self.assertEqual(label.bench_label("doc_house", "/x/Fallout New Vegas", preset="vanilla", env={}), "bench-doc-house-x-vanilla")

    def test_callers_label_wins_and_is_checked(self):
        self.assertEqual(label.bench_label("views", "/g", asked="gate14-mojave-ttw", env={}), "gate14-mojave-ttw")
        self.assertEqual(label.bench_label("views", "/g", env={"NV_BENCH_LABEL": "smoke-0201-plain"}), "smoke-0201-plain")
        for bad in ("a b", "../x", "-x", "a/b", "x" * 101, "é"):
            with self.assertRaises(ValueError):
                label.bench_label("views", "/g", asked=bad, env={})

    def test_scope_arguments(self):
        a = label.scope_args("bench-views-x", 7, who="views", cwd="/run/dir")
        self.assertIn("--slice=nv-games.slice", a)
        self.assertIn("--unit=nvgame-bench-views-x-7.scope", a)
        self.assertTrue(any(x.startswith("--description=game run bench-views-x, started by views") for x in a))

    def test_scope_prefix_keeps_extra_out_of_the_probe(self):
        calls = []
        class R:  # noqa
            returncode = 0
        import subprocess
        real, which = subprocess.run, shutil.which
        subprocess.run = lambda cmd, **k: (calls.append(list(cmd)), R())[1]
        shutil.which = lambda n: "/bin/systemd-run"
        try:
            out = guard.scope_prefix(1 << 30, ["--unit=u.scope"])
        finally:
            subprocess.run, shutil.which = real, which
        self.assertNotIn("--unit=u.scope", calls[0])
        self.assertEqual(out[-1], "--unit=u.scope")


class NamedExeTests(unittest.TestCase):
    def setUp(self):
        self.d = Path(tempfile.mkdtemp())
        self.exe = self.d / "viewer"
        shutil.copy(sys.executable if Path(sys.executable).resolve().read_bytes()[:4] == b"\x7fELF" else shutil.which("true"), self.exe)

    def tearDown(self):
        shutil.rmtree(self.d, ignore_errors=True)

    def test_hard_link_next_to_the_program_and_removed(self):
        with label.NamedExe(self.exe, "bench-views-x", fallback_dir=self.d / "cache") as n:
            self.assertEqual(n.how, "link")
            self.assertEqual(Path(n.path), self.d / "bench-views-x")
            self.assertTrue(Path(n.path).samefile(self.exe))
        self.assertFalse((self.d / "bench-views-x").exists())

    def test_a_script_is_left_alone(self):
        s = self.d / "run.sh"
        s.write_text("#!/bin/sh\n")
        with label.NamedExe(s, "x-y", fallback_dir=self.d / "cache") as n:
            self.assertEqual((n.how, n.path), ("none", str(s)))

    def test_leftover_link_of_a_dead_run_is_replaced_but_a_stranger_file_is_not(self):
        os.link(self.exe, self.d / "again")
        with label.NamedExe(self.exe, "again") as n:
            self.assertEqual(n.how, "link")
        (self.d / "other").write_text("mine")
        with label.NamedExe(self.exe, "other") as n:
            self.assertEqual(n.how, "none")
        self.assertEqual((self.d / "other").read_text(), "mine")

    def test_copy_when_the_link_cannot_be_made(self):
        real = os.link
        def no(*a, **k):
            raise OSError("cross-device")
        os.link = no
        try:
            with label.NamedExe(self.exe, "run-a", fallback_dir=self.d / "cache") as n:
                self.assertEqual(n.how, "copy")
                self.assertEqual(Path(n.path).name, "run-a")
                kept = Path(n.path)
                self.assertTrue(kept.exists())
        finally:
            os.link = real
        self.assertFalse(kept.exists())


class ViewerScanTests(unittest.TestCase):
    def test_labelled_viewer_counts_as_another_viewer_but_its_helpers_and_own_do_not(self):
        with tempfile.TemporaryDirectory() as t:
            def proc(pid, comm, ppid, cg):
                p = Path(t) / str(pid)
                p.mkdir()
                (p / "comm").write_text(comm + "\n")
                (p / "stat").write_text(f"{pid} ({comm}) S {ppid} 0 0\n")
                (p / "cgroup").write_text(f"0::/user.slice/nv.slice/nv-games.slice/{cg}\n")
            proc(10, "bench-views-lak", 1, "nvgame-bench-views-lake-9.scope")  # another run's viewer, renamed
            proc(11, "helper", 10, "nvgame-bench-views-lake-9.scope")  # its child in the same scope
            proc(20, "swaybg", 1, "nvsway-bench-views-x-3.scope")  # a compositor's helper
            self.assertEqual(lock.other_viewers([], t), 1)
            self.assertEqual(lock.other_viewers([10], t), 0)


if __name__ == "__main__":
    unittest.main()
