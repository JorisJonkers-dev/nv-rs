# SPDX-License-Identifier: GPL-3.0-only
"""The bench's memory guard (scripts/fork/bench/guard.py) with fakes for the machine: caps, refusal, the sampler's
stop, session health, the halt file and the suite stop."""
import argparse
import signal
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "bench"))

import guard  # noqa: E402
import nvbench  # noqa: E402

CARD = guard.GpuMem(vram_used=2500.0, vram_total=24560.0, gtt_used=265.0, gtt_total=16002.0)
AVAIL = 20 * guard.GIB


class CapsTests(unittest.TestCase):
    def test_defaults_are_shares_of_what_the_machine_has(self):
        c = guard.default_caps(AVAIL, CARD)
        # Half the available memory for both: the compositor's 2 GiB come out of the viewer's part.
        self.assertEqual((c.host_bytes, c.sway_bytes), (8 * guard.GIB, 2 * guard.GIB))
        self.assertLessEqual(c.host_bytes + c.sway_bytes, AVAIL // 2)
        self.assertEqual((c.vram_mib, c.gtt_mib), (12280, 4000))
        big = guard.default_caps(64 * guard.GIB, CARD)
        self.assertEqual(big.host_bytes + big.sway_bytes, guard.HOST_MAX)

    def test_no_caps_no_run(self):
        with self.assertRaises(guard.Refused):
            guard.default_caps(None, CARD)
        with self.assertRaises(guard.Refused):
            guard.default_caps(AVAIL, None)

    def test_caps_can_only_be_lowered(self):
        c = guard.default_caps(AVAIL, CARD)
        low = guard.lowered(c, host=4 * guard.GIB, vram_mib=6000, gtt_mib=1000)
        self.assertEqual(guard.lowered(c).as_dict(), c.as_dict())
        self.assertEqual((low.host_bytes, low.sway_bytes, low.vram_mib, low.gtt_mib), (4 * guard.GIB, guard.GIB, 6000, 1000))
        for kw in ({"host": 9 * guard.GIB}, {"vram_mib": 20000}, {"gtt_mib": 9000}, {"vram_mib": 0}):
            with self.assertRaises(guard.Refused, msg=kw):
                guard.lowered(c, **kw)
        self.assertEqual(guard.parse_size("12G"), 12 * guard.GIB)
        self.assertEqual(guard.parse_size("512M"), 512 * guard.MIB)
        self.assertIsNone(guard.parse_size(""))

    def test_a_card_without_room_for_the_run_and_the_desktop_is_refused(self):
        c = guard.default_caps(AVAIL, CARD)
        guard.preflight(c, CARD)  # an idle card: fits
        # The card of the night the desktop crashed: another program held almost all of it before the run started.
        full = guard.GpuMem(vram_used=24391.9, vram_total=24560.0, gtt_used=12833.1, gtt_total=16002.0)
        with self.assertRaisesRegex(guard.Refused, "VRAM: 168 MiB free"):
            guard.preflight(c, full)
        gtt_full = guard.GpuMem(vram_used=2500.0, vram_total=24560.0, gtt_used=12000.0, gtt_total=16002.0)
        with self.assertRaisesRegex(guard.Refused, "GTT"):
            guard.preflight(c, gtt_full)

    def test_the_stop_comes_before_the_cap_and_before_the_desktops_margin(self):
        c = guard.default_caps(AVAIL, CARD)
        lv, lg = guard.limits(c, CARD)
        self.assertLess(lv, CARD.vram_used + c.vram_mib)
        self.assertGreater(lv, CARD.vram_used)
        self.assertLess(lg, CARD.gtt_used + c.gtt_mib)
        # With the card half used at start, the desktop's margin is the nearer limit.
        busy = guard.GpuMem(vram_used=10000.0, vram_total=24560.0, gtt_used=265.0, gtt_total=16002.0)
        lv2, _ = guard.limits(c, busy)
        self.assertLess(lv2, busy.vram_total - c.vram_margin_mib)
        self.assertIsNone(guard.over(c, CARD, CARD))
        self.assertIn("VRAM", guard.over(c, CARD, guard.GpuMem(lv + 1, 24560.0, 265.0, 16002.0)))
        self.assertIn("GTT", guard.over(c, CARD, guard.GpuMem(2500.0, 24560.0, lg + 1, 16002.0)))


class ScopeTests(unittest.TestCase):
    def test_the_scope_has_max_high_and_no_swap_and_is_required(self):
        with mock.patch("shutil.which", return_value="/usr/bin/systemd-run"), \
                mock.patch("subprocess.run", return_value=mock.Mock(returncode=0)):
            cmd = guard.scope_prefix(4 * guard.GIB)
        self.assertIn("MemoryMax=4096M", cmd)
        self.assertIn("MemoryHigh=3686M", cmd)
        self.assertIn("MemorySwapMax=0", cmd)
        with mock.patch("shutil.which", return_value=None):
            with self.assertRaises(guard.Refused):
                guard.scope_prefix(guard.GIB)
        with mock.patch("shutil.which", return_value="/usr/bin/systemd-run"), \
                mock.patch("subprocess.run", return_value=mock.Mock(returncode=1)):
            with self.assertRaises(guard.Refused):
                guard.scope_prefix(guard.GIB)

    def test_the_nested_compositor_needs_a_scope(self):
        import nested
        with self.assertRaises(ValueError):
            nested.Nested("/bin/sway", "/tmp", "800x600", "headless")


class HealthTests(unittest.TestCase):
    def test_kernel_trouble_and_a_lost_desktop(self):
        crash = "amdgpu 0000:09:00.0: [drm] *ERROR* Not enough memory for command submission!\n"
        self.assertIn("Not enough memory for command submission", guard.session_trouble(crash, {1}, {1}))
        self.assertIn("GPU reset", guard.session_trouble("amdgpu 0000:09:00.0: GPU reset begin!\n", set(), set()))
        # Lines that are not the GPU driver's do not count.
        self.assertIsNone(guard.session_trouble("usb 1-2: device lost\n", set(), set()))
        self.assertIn("pid 7", guard.session_trouble("", {7, 8}, {8}))
        # The run's own processes ending is not the desktop's loss.
        self.assertIsNone(guard.session_trouble("", {7, 8}, {8}, own={7}))
        self.assertIsNone(guard.session_trouble("", {7}, {7}))

    def test_desktop_pids_are_read_by_process_name(self):
        with tempfile.TemporaryDirectory() as d:
            for pid, name in ((10, "gnome-shell"), (11, ".gnome-shell-wr"), (12, "sway"), (13, "bash")):
                (Path(d) / str(pid)).mkdir()
                (Path(d) / str(pid) / "comm").write_text(name + "\n")
            self.assertEqual(guard.desktop_pids(d), {10, 11})


class GuardLoopTests(unittest.TestCase):
    def run_guard(self, samples, health=lambda: None, log_text=lambda: "", wait=5.0):
        c = guard.default_caps(AVAIL, CARD)
        it = iter(samples)
        stops = []
        with tempfile.TemporaryDirectory() as d:
            halt = Path(d) / "halt"
            g = guard.Guard(c, CARD, read=lambda: next(it, samples[-1]), stop=lambda: stops.append(1), health=health,
                            log_text=log_text, interval=0.001, health_every=0.0, halt_path=halt)
            g.start()
            end = time.monotonic() + wait
            while g.stopped is None and time.monotonic() < end and g.thread.is_alive():
                time.sleep(0.005)
            if g.stopped is None:
                g.finish()
            return g, stops, guard.halted(halt)

    def test_a_climb_is_stopped_before_the_cap_and_halts_later_runs(self):
        lv, _ = guard.limits(guard.default_caps(AVAIL, CARD), CARD)
        climb = [guard.GpuMem(CARD.vram_used + k * 1000, 24560.0, 265.0, 16002.0) for k in range(20)]
        g, stops, halted = self.run_guard(climb)
        self.assertIn("VRAM", g.stopped)
        self.assertEqual(stops, [1])
        self.assertLess(g.peak["vram_mib"], CARD.vram_used + guard.default_caps(AVAIL, CARD).vram_mib)
        self.assertGreaterEqual(g.peak["vram_mib"], lv)
        self.assertIn("VRAM", halted["reason"])

    def test_session_trouble_and_the_engines_refusal_stop_the_run(self):
        g, stops, halted = self.run_guard([CARD], health=lambda: "kernel: GPU reset")
        self.assertIn("session trouble", g.stopped)
        self.assertEqual(g.trouble, "kernel: GPU reset")
        self.assertTrue(halted)
        g, stops, _ = self.run_guard([CARD], log_text=lambda: "NV_D3D9 wgpu: stop: a texture ... " + guard.ENGINE_REFUSAL)
        self.assertIn("GPU budget", g.stopped)

    def test_a_card_that_can_no_longer_be_read_stops_the_run(self):
        g, stops, halted = self.run_guard([CARD, None])
        self.assertIn("can no longer be read", g.stopped)
        self.assertEqual(stops, [1])

    def test_a_quiet_run_is_left_alone(self):
        g, stops, halted = self.run_guard([CARD] * 5, wait=0.3)
        self.assertIsNone(g.stopped)
        self.assertEqual(stops, [])
        self.assertIsNone(halted)

    def test_the_whole_process_group_is_stopped_term_then_kill(self):
        sent = []
        guard.stop_group(4242, grace=0.05, killpg=lambda pg, sig: sent.append((pg, sig)), alive=lambda: True, sleep=lambda s: time.sleep(0.01))
        self.assertEqual(sent, [(4242, signal.SIGTERM), (4242, signal.SIGKILL)])
        sent.clear()
        guard.stop_group(4242, grace=1.0, killpg=lambda pg, sig: sent.append((pg, sig)), alive=lambda: False)
        self.assertEqual(sent, [(4242, signal.SIGTERM)])


class LaunchTests(unittest.TestCase):
    """runner.launch with a fake viewer (a shell script) and a fake card: the caps reach the viewer and the log, and a
    climbing card is stopped by the guard."""

    def launch(self, cards, sleep_s):
        import runner
        import scenarios
        d = Path(self.tmp.name)
        viewer = d / "viewer"
        viewer.write_text(f"#!/bin/sh\necho budget $NV_GPU_BUDGET_MB\nsleep {sleep_s}\n")
        viewer.chmod(0o755)
        opts = {"mode": "capped", "windowed": False, "preset": "fixes", "threads": None, "resolution": "800x600", "sound": None,
                "fps": 100.0, "legacy": True, "viewer": str(viewer), "game": str(d), "vram_limit_pct": 70, "timeout_s": 20,
                "nested": None}
        plan = scenarios.build("boot", opts)
        plan.end_frame, plan.wall_after_first_s = 0, 0
        it = iter(cards)
        run_dir = d / "run"
        run_dir.mkdir()
        with mock.patch.object(guard, "find_gpu", return_value=None), \
                mock.patch.object(guard, "read_gpu", side_effect=lambda _d: next(it, cards[-1])), \
                mock.patch.object(guard, "mem_available", return_value=AVAIL), \
                mock.patch.object(guard, "scope_prefix", return_value=[]), \
                mock.patch.object(guard, "kernel_lines_since", return_value=""), \
                mock.patch.object(guard, "kernel_log_readable", return_value=True), \
                mock.patch.object(guard, "HALT", d / "halt"), \
                mock.patch.object(runner, "find_gpu", return_value="/fake"):
            run = runner.launch(plan, opts, run_dir)
        return run, (run_dir / "viewer.log").read_text()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()

    def tearDown(self):
        self.tmp.cleanup()

    def test_a_quiet_run_logs_its_caps_and_hands_the_budget_to_the_engine(self):
        run, log = self.launch([CARD], 0.5)
        self.assertIsNone(run["guard"])
        self.assertEqual(run["caps"]["vram_mib"], 12280)
        self.assertEqual(run["caps"]["engine_budget_mib"], 12280)
        self.assertIn("# memory caps: host 8192 MiB, compositor 2048 MiB", log)
        self.assertIn("budget 12280", log)
        self.assertFalse((Path(self.tmp.name) / "halt").exists())

    def test_a_climbing_card_is_stopped_and_halts_the_bench(self):
        climb = [CARD, CARD] + [guard.GpuMem(CARD.vram_used + k * 2000, 24560.0, 265.0, 16002.0) for k in range(1, 12)]
        t0 = time.monotonic()
        run, log = self.launch(climb, 30)
        self.assertLess(time.monotonic() - t0, 10)  # the 30 s viewer was stopped
        self.assertEqual(run["ended_by"], "memory guard")
        self.assertIn("VRAM", run["guard"])
        self.assertIn("# memory guard: VRAM", log)
        self.assertLess(run["card_peak_mib"]["vram"], CARD.vram_used + 12280)
        self.assertTrue((Path(self.tmp.name) / "halt").exists())

    def test_a_halted_bench_refuses_launch_itself(self):
        guard.halt("kernel: GPU reset", Path(self.tmp.name) / "halt")
        with self.assertRaisesRegex(guard.Refused, "halted"):
            self.launch([CARD], 0.1)

    def test_no_room_on_the_card_refuses_before_anything_starts(self):
        full = guard.GpuMem(24391.9, 24560.0, 12833.1, 16002.0)
        with self.assertRaises(guard.Refused):
            self.launch([full], 0.1)
        self.assertNotIn("viewer.log", [p.name for p in (Path(self.tmp.name) / "run").iterdir()])


class HaltAndSuiteTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.halt = Path(self.tmp.name) / "bench.halt"
        self.patch = mock.patch.object(guard, "HALT", self.halt)
        self.patch.start()

    def tearDown(self):
        self.patch.stop()
        self.tmp.cleanup()

    def args(self, **kw):
        a = argparse.Namespace(cap=None, legacy=False, mode="unlimited", scenario="boot", nested=None, windowed=False,
                               yes_windowed=False, vram_limit_pct=50, no_lock=True)
        a.__dict__.update(kw)
        return a

    def test_a_halt_refuses_every_run_until_cleared(self):
        guard.halt("kernel: Not enough memory for command submission")
        with mock.patch.object(nvbench, "run_scenario", side_effect=AssertionError("must not run")):
            self.assertEqual(nvbench.cmd_run(self.args()), nvbench.HALTED)
        self.assertEqual(nvbench.cmd_clear_halt(None), 0)
        self.assertIsNone(guard.halted())
        with mock.patch.object(nvbench, "run_scenario", return_value=0) as run:
            self.assertEqual(nvbench.cmd_run(self.args()), 0)
            run.assert_called_once()

    def test_the_vram_percent_can_only_be_lowered(self):
        for pct in (0, 71, 100):
            self.assertEqual(nvbench.cmd_run(self.args(vram_limit_pct=pct)), nvbench.HALTED, pct)

    def test_the_suite_stops_at_the_first_halt_and_starts_no_later_scenario(self):
        ran = []

        def fake_run(b):
            ran.append(b.scenario)
            guard.halt("the memory guard stopped the viewer")  # the first scenario's run is stopped by the guard
            return 1

        a = argparse.Namespace(quick=False, scenarios="doc_house,goodsprings,strip", baseline=None, save_baseline=None,
                               mode="unlimited", cap=None, resolution="800x600", perf=False)
        with mock.patch.object(nvbench, "cmd_run", side_effect=fake_run), \
                mock.patch.object(nvbench.runner, "STATE", Path(self.tmp.name)):
            self.assertEqual(nvbench.cmd_suite(a), nvbench.HALTED)
        self.assertEqual(ran, ["doc_house"])


if __name__ == "__main__":
    unittest.main()
