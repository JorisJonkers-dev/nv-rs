"""Tests of scripts/fork/bench (the playthrough and benchmark runner's parsing, report and compare code).

Synthetic fixtures only: no game, no build, no GPU. Run: python3 -I -m unittest discover -s scripts/fork/tests -v
"""
import io
import json
import os
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent / "bench"
sys.path.insert(0, str(BENCH))

import nvbench  # noqa: E402
import parse  # noqa: E402
import lock  # noqa: E402
import report  # noqa: E402
import suite  # noqa: E402
import vinput  # noqa: E402
import scenarios  # noqa: E402

HEADER = "frame\tus\tm_fdelta\thk_count\thk_time\tpx\tpy\tpz\tscreen\tqpc\tpoll_qpc\tpresent\tshown\tshown_qpc\tinput_to_present_us"


def frame_log(n=300, dt_us=7000, slow_every=100, slow_us=40000, walk=True):
    lines = [HEADER]
    us = 1_000_000
    for i in range(n):
        step = slow_us if slow_every and i and i % slow_every == 0 else dt_us
        us += step
        screen = "menu" if i < 50 else "play"
        x = 100.0 + (i - 50) * 2.0 if walk and i >= 50 else 0.0
        lines.append(f"{i}\t{us}\t{step / 1e6}\t1\t0.0\t{x}\t200.0\t10.0\t{screen}\t0\t0\t0\t0\t0\t")
    lines.append("# input to present over the last 1000 measured frames: p50 1.0 ms, p95 2.0 ms")
    return "\n".join(lines) + "\n"


VIEWER_LOG = """# nv-rs bench boot
static initializer skipped: 00ed417e stopped: open function 00fdf128: not translated yet (FalloutNV.exe 1.4.0.525, docs/LEDGER.md)
NV_D3D9 wgpu: present 601: 2356 draws, 12 skipped, 5 pipelines, 10 bind groups, 0 wgpu errors, 16.6 MB kept for read-backs (1115 levels)
NV_D3D9 wgpu: pipeline work since the last report: at most 0.7 ms in one present (present 5), 3 presents over 20 ms, 0 pipelines made on draws over 1 ms
Game's movement: main 600 frames; running; 0 untranslated addresses reached.
NV_MEMLOG t=5.000 n=1 frame=600 heap_used=100 heap_peak=120 rss=1000
NV_D3D9 wgpu: present 1201: 4756 draws, 12 skipped, 5 pipelines, 10 bind groups, 2 wgpu errors, 16.6 MB kept
Game's movement: main 1201 frames; running; 3 untranslated addresses reached.
Game's main stopped in frame 1300: function 0044fb20 not translated yet (in 0044fb20)
thread 'main' panicked at foo.rs:1:1
# exit status: 3
"""


class StatsTests(unittest.TestCase):
    def test_percentiles_and_lows(self):
        s = parse.frame_stats([10.0] * 98 + [50.0, 100.0])
        self.assertEqual(s["frames"], 100)
        self.assertEqual(s["max_ms"], 100.0)
        self.assertEqual(s["p50_ms"], 10.0)
        self.assertEqual(s["p99_ms"], 50.0)
        self.assertAlmostEqual(s["low1_fps"], 10.0)  # the slowest 1 frame of 100 is 100 ms
        self.assertEqual(parse.frame_stats([]), {"frames": 0})

    def test_frame_log_rows_and_timeline(self):
        rows = parse.parse_frame_log(frame_log(300, 10000, slow_every=0))
        self.assertEqual(len(rows), 300)
        self.assertIsNone(rows[0]["dt_ms"])
        self.assertAlmostEqual(rows[1]["dt_ms"], 10.0)
        fs = parse.frame_summary(rows)
        self.assertAlmostEqual(fs["all"]["fps"], 100.0)
        self.assertEqual(fs["by_screen"]["play"]["frames"], 250)
        self.assertEqual(fs["timeline"][0]["fps"], 100)

    def test_truncated_line_is_skipped(self):
        text = frame_log(10) + "11\t99"
        self.assertEqual(len(parse.parse_frame_log(text)), 10)

    def test_screens_and_movement(self):
        rows = parse.parse_frame_log(frame_log())
        self.assertEqual([s for _, _, s in parse.screen_transitions(rows)], ["menu", "play"])
        mv = parse.distance_moved(rows)
        self.assertAlmostEqual(mv["path"], 2.0 * 249, delta=1)
        self.assertAlmostEqual(mv["net"], 498.0, delta=1)

    def test_teleport_is_not_walking(self):
        rows = parse.parse_frame_log(frame_log(60, walk=False))
        for i, r in enumerate(rows):
            r["pos"] = [100000.0 * (i % 2) + 1, 1.0, 1.0]
            r["screen"] = "play"
        self.assertEqual(parse.distance_moved(rows)["path"], 0.0)


class LogTests(unittest.TestCase):
    def test_log_counters(self):
        log = parse.parse_log(VIEWER_LOG)
        self.assertEqual(log["presents"], 1201)
        self.assertEqual(log["wgpu_errors"], 2)
        self.assertEqual(log["skipped_draws"], 12)
        self.assertEqual(log["slow_presents_over_20ms"], 3)
        self.assertEqual(log["untranslated_reported"], 3)
        self.assertEqual(log["main_frames"], 1201)
        self.assertEqual(len(log["stop_lines"]), 1)  # the skipped static initializer is not a stop
        self.assertEqual(log["panics"], 1)
        self.assertEqual(log["exit_status"], 3)
        self.assertEqual(log["memlog"][0]["heap_used"], 100.0)

    def test_stop_line_paths_are_scrubbed(self):
        log = parse.parse_log("Game's main stopped in frame 3: no file /home/user/Games/x/Data (in 0040)\n")
        self.assertNotIn("/home", log["stop_lines"][0])

    def test_watchdog_line(self):
        log = parse.parse_log("# GPU memory watchdog: VRAM 20000 MiB of 24000 (limit 70%): stopped the game\n")
        self.assertIn("VRAM", log["watchdog"])

    def test_missing_lists(self):
        self.assertEqual(parse.count_missing("address\thits\tstage\n00401000\t5\tx\n00402000\t2\ty\n"), (2, 7))
        self.assertEqual(parse.count_stops("entry\twhy\n"), 0)


class SampleTests(unittest.TestCase):
    def test_growth_slope(self):
        text = "t\trss_mib\tcpu_pct\tvram_mib\tgtt_mib\n" + "".join(f"{t}.0\t{1000 + t}\t50\t2000\t100\n" for t in range(0, 241, 2))
        s = parse.sample_summary(parse.parse_samples(text))
        self.assertAlmostEqual(s["rss_growth_mib_per_min"], 60.0, places=1)
        self.assertAlmostEqual(s["vram_growth_mib_per_min"], 0.0, places=3)
        self.assertEqual(s["rss_peak_mib"], 1240)


def make_summary(**changes):
    rows = parse.parse_frame_log(frame_log(1200, 7000, slow_every=0))
    log = parse.parse_log(VIEWER_LOG.replace("2 wgpu errors", "0 wgpu errors").replace("Game's main stopped", "Game has ended"))
    samples = parse.parse_samples("t\trss_mib\tcpu_pct\tvram_mib\tgtt_mib\n" + "".join(f"{t}.0\t500\t40\t1500\t60\n" for t in range(10)))
    m = parse.summarize(rows, log, samples, (0, 0), 0)
    m["panics"] = 0
    m["exit_status"] = 143
    m.update(changes)
    checks = nvbench.verdict_checks(m, scenarios.build("boot", {"fps": 144.0}))
    return {"name": "x-1", "scenario": "boot", "settings": {"mode": "capped"}, "run": {"ended_by": "scenario end"}, "metrics": m,
            "verdict": {"ok": all(c["ok"] for c in checks), "checks": checks}, "series_t": [r["t"] for r in samples],
            "series": {"rss": [500.0] * 10, "vram": [1500.0] * 10, "gtt": [60.0] * 10}}


class ReportTests(unittest.TestCase):
    def test_markdown_and_html(self):
        s = make_summary()
        md = report.markdown(s)
        self.assertIn("Verdict: **PASS**", md)
        self.assertIn("## Frame times", md)
        page = report.page(s)
        self.assertIn("<svg", page)
        self.assertNotIn("<script", page)

    def test_failing_run(self):
        s = make_summary(stops=1, stop_lines=["stopped"])
        self.assertFalse(s["verdict"]["ok"])
        self.assertIn("FAIL", report.markdown(s))

    def test_sparkline(self):
        self.assertEqual(len(report.sparkline(list(range(200)), 40)), 40)
        self.assertEqual(report.sparkline([]), "")

    def test_compare_flags_regressions_only(self):
        a = make_summary()
        same, ok = report.compare(a, json.loads(json.dumps(a)))
        self.assertTrue(ok)
        b = json.loads(json.dumps(a))
        b["metrics"]["frame_time"]["fps"] *= 0.8
        b["metrics"]["frame_time"]["p99_ms"] += 5
        rows, ok = report.compare(a, b)
        verdicts = {r[0]: r[5] for r in rows}
        self.assertFalse(ok)
        self.assertEqual(verdicts["fps"], "REGRESSION")
        self.assertEqual(verdicts["p99"], "REGRESSION")
        self.assertEqual(verdicts["rss"], "ok")
        rows, ok = report.compare(a, b, {"fps": 50.0, "p99": 1000.0})
        self.assertTrue(ok)

    def test_compare_counters_regress_on_any_increase(self):
        a = make_summary()
        b = json.loads(json.dumps(a))
        b["metrics"]["wgpu_errors"] = 1
        rows, ok = report.compare(a, b)
        self.assertFalse(ok)
        self.assertEqual({r[0]: r[5] for r in rows}["wgpu_errors"], "REGRESSION")
        self.assertIn("REGRESSION", report.compare_markdown(a, b, rows, ok))

    def test_small_change_under_noise_floor_is_ok(self):
        a = make_summary()
        b = json.loads(json.dumps(a))
        b["metrics"]["memory"]["rss_peak_mib"] += 10
        rows, ok = report.compare(a, b)
        self.assertTrue(ok)


class LoadingFrameTests(unittest.TestCase):
    def test_loading_frames_are_not_in_the_headline(self):
        lines = [HEADER]
        us = 0
        for i in range(400):
            us += 1000 if i < 200 else 7000  # 1 ms spinning frames while loading, then 7 ms play frames
            lines.append(f"{i}\t{us}\t0\t0\t0\t1\t1\t1\t{'loading' if i < 200 else 'play'}\t0\t0\t0\t0\t0\t")
        fs = parse.frame_summary(parse.parse_frame_log("\n".join(lines)))
        self.assertAlmostEqual(fs["all"]["avg_ms"], 7.0)
        self.assertEqual(fs["by_screen"]["loading"]["frames"], 199)
        self.assertAlmostEqual(fs["loading_s"], 0.199, places=3)


class ScenarioTests(unittest.TestCase):
    OPTS = {"mode": "capped", "windowed": False, "preset": "fixes", "threads": None, "resolution": "1920x1080", "sound": None, "fps": 100.0, "legacy": True}

    def test_script_converts_seconds_to_frames(self):
        s = scenarios.Script(fps=100.0)
        s.wait(1.0).press("enter", 0.2, 0.5).hold(("w", "lshift"), 2.0)
        self.assertIn((100, 0x1C, 20), s.keys)
        self.assertIn((150, 0x11, 200), s.keys)

    def test_type_uses_shift_for_capitals(self):
        s = scenarios.Script(fps=100.0)
        s.type("Ab")
        codes = [k[1] for k in s.keys]
        self.assertIn(scenarios.DIK["lshift"], codes)
        self.assertEqual(codes.count(scenarios.DIK["a"]), 1)

    def test_play_clock_items_get_the_p_prefix_and_the_env_keeps_both_clocks(self):
        plan = scenarios.build("chargen", self.OPTS)
        keys = plan.env(self.OPTS)["NV_MOVE_KEYS"].split(",")
        self.assertTrue(any(k.startswith("p") for k in keys))
        self.assertTrue(any(not k.startswith("p") for k in keys))
        self.assertEqual(scenarios.on_play_clock([(5, 1, 2)]), [("p5", 1, 2)])

    def test_env_for_modes(self):
        plan = scenarios.build("boot", self.OPTS)
        capped = plan.env(self.OPTS)
        self.assertEqual(capped["NV_FRAME_LIMIT"], "144")
        self.assertEqual(capped["NV_D3D9_OFFSCREEN"], "1")
        fast = plan.env(dict(self.OPTS, mode="uncapped"))
        self.assertEqual(fast["NV_FRAME_LIMIT"], "0")
        self.assertEqual(fast["NV_PRESENT"], "immediate")
        win = plan.env(dict(self.OPTS, windowed=True))
        self.assertNotIn("NV_D3D9_OFFSCREEN", win)

    def test_newgame_is_capped_only_and_scripts_the_opening(self):
        plan = scenarios.build("newgame", self.OPTS)
        self.assertTrue(plan.capped_only)
        env = plan.env(self.OPTS)
        self.assertIn("2900:1C:30", env["NV_MOVE_KEYS"])
        self.assertGreater(plan.end_frame, scenarios.OPENING_END)

    def test_every_scenario_builds_and_env_keys_parse(self):
        for name in scenarios.SCENARIOS:
            plan = scenarios.build(name, self.OPTS)
            env = plan.env(self.OPTS)
            for item in env.get("NV_MOVE_KEYS", "").split(","):
                if item:
                    f, k, h = item.split(":")
                    int(f.lstrip("p")), int(k, 16), int(h)


class NestedAndWorldTests(unittest.TestCase):
    OPTS = ScenarioTests.OPTS

    def test_sound_is_the_games_clock_into_the_null_sink_in_both_paths(self):
        plan = scenarios.build("boot", self.OPTS)
        for windowed in (False, True):
            env = plan.env(dict(self.OPTS, windowed=windowed))
            self.assertEqual((env["NV_SOUND"], env["NV_SOUND_BACKEND"]), ("out", "null"))
            self.assertNotIn("NV_D3D9_HIDDEN", env)  # a hidden window mutes the device and Doc's lines never end

    def test_sway_config_and_resolution(self):
        import nested
        self.assertEqual(nested.parse_resolution("2560x1440"), (2560, 1440))
        conf = nested.sway_config(2560, 1440)
        self.assertIn("output * mode 2560x1440@60Hz", conf)
        self.assertIn("xwayland disable", conf)
        with self.assertRaises(ValueError):
            nested.parse_resolution("wide")

    def test_world_scenarios_teleport_by_form_id_and_scale_by_laps(self):
        for name in ("goodsprings", "novac", "strip", "combat", "soak"):
            plan = scenarios.build(name, self.OPTS)
            self.assertTrue(plan.capped_only and plan.self_scaled, name)
            self.assertGreater(plan.end_frame, scenarios.LOADING_FRAMES)
        self.assertEqual(scenarios.laps({"scale": 0.1}), 1)
        self.assertEqual(scenarios.laps({}), 3)
        self.assertEqual(scenarios.laps({"scale": 2}), 6)
        long, short = (scenarios.build("goodsprings", dict(self.OPTS, scale=s)) for s in (1.0, 0.34))
        self.assertGreater(long.end_frame, short.end_frame)

    def test_teleport_types_the_marker_id_and_dismisses_boxes(self):
        sc = scenarios.Script(fps=100.0)
        scenarios.teleport(sc, "novac", boxes=2)
        keys = [k for _, k, _ in sc.keys]
        self.assertEqual(keys.count(scenarios.DIK["enter"]), 3)  # the command and the two boxes
        self.assertIn(scenarios.DIK["d"], keys)  # "8D0A3" is typed
        self.assertNotIn(scenarios.DIK["grave"], keys)  # a console key after the boxes would reopen it

    def test_runner_borrows_nothing_when_the_system_has_the_libraries(self):
        import runner
        self.assertIsInstance(runner.borrowed_libs({}), dict)


class LockTests(unittest.TestCase):
    def test_second_holder_waits_then_times_out_and_names_the_first(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "bench.lock"
            said = []
            first = lock.BenchLock(path, out=said.append)
            self.assertTrue(first.acquire())
            second = lock.BenchLock(path, wait_timeout=0.3, out=said.append, poll=0.05)
            self.assertFalse(second.acquire())
            self.assertTrue(any(m.startswith("waiting for bench lock held by pid ") for m in said), said)
            self.assertEqual(second.read_holder()["pid"], os.getpid())
            first.release()
            third = lock.BenchLock(path, wait_timeout=0.3, out=said.append, poll=0.05)
            self.assertTrue(third.acquire())
            third.release()

    def test_waiters_are_served_in_the_order_they_came(self):
        import threading
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "bench.lock"
            first = lock.BenchLock(path, out=lambda m: None)
            self.assertTrue(first.acquire())
            order, locks = [], []

            def waiter(i):
                b = lock.BenchLock(path, wait_timeout=10, out=lambda m: None, poll=0.02)
                locks.append(b)
                if b.acquire():
                    order.append(i)
                    time.sleep(0.05)
                    b.release()

            threads = []
            for i in range(4):
                t = threading.Thread(target=waiter, args=(i,))
                t.start()
                threads.append(t)
                time.sleep(0.15)  # distinct tickets, in order
            first.release()
            for t in threads:
                t.join()
            self.assertEqual(order, [0, 1, 2, 3])
            self.assertEqual(list((Path(d) / "bench.lock.queue").iterdir()), [])

    def test_a_dead_waiter_loses_its_place(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "bench.lock"
            q = Path(d) / "bench.lock.queue"
            q.mkdir()
            (q / f"{1:020d}-999999999").write_text("")  # no such process
            b = lock.BenchLock(path, wait_timeout=2, out=lambda m: None, poll=0.02)
            self.assertTrue(b.acquire())
            b.release()

    def test_describe_and_other_viewers(self):
        self.assertIn("pid 7 (bench run goodsprings), 5 s ago", lock.describe({"pid": 7, "cmd": "bench run goodsprings", "start": 95}, now=100))
        self.assertEqual(lock.describe({}), "pid ?")
        with tempfile.TemporaryDirectory() as d:
            for pid, comm, ppid in ((10, "nv-viewer", 1), (11, "nv-viewer", 5), (12, "python3", 1), (13, "nv-viewer", 1)):
                p = Path(d) / str(pid)
                p.mkdir()
                (p / "comm").write_text(comm + "\n")
                (p / "stat").write_text(f"{pid} ({comm}) S {ppid} 0 0\n")
            self.assertEqual(lock.other_viewers([], d), 3)
            self.assertEqual(lock.other_viewers([10], d), 1 + 1)  # 13 and none of 10; 11 is a child of 5, not of 10
            self.assertEqual(lock.other_viewers([5], d), 2)  # 11 is a child of an own pid

    def test_noisy_runs_are_marked_in_report_and_compare(self):
        a = make_summary()
        b = make_summary()
        b["run"] = dict(b["run"], noisy=True, other_viewers_max=2)
        self.assertIn("NOISY: 2 other nv-viewer", report.markdown(b))
        self.assertNotIn("NOISY", report.markdown(a))
        rows, ok = report.compare(a, b)
        self.assertIn("NOISY: the candidate", report.compare_markdown(a, b, rows, ok))


class VirtualInputTests(unittest.TestCase):
    def test_wire_messages_and_events(self):
        import struct
        m = vinput.message(5, 2, struct.pack("<I", 7))
        self.assertEqual(m, struct.pack("<II", 5, (12 << 16) | 2) + struct.pack("<I", 7))
        self.assertEqual(vinput.string_arg("wl_seat"), struct.pack("<I", 8) + b"wl_seat\0")
        events, rest = vinput.parse_events(m + m[:6])
        self.assertEqual(events, [(5, 2, struct.pack("<I", 7))])
        self.assertEqual(rest, m[:6])

    def test_global_event(self):
        import struct
        name = b"wl_seat\0"
        body = struct.pack("<II", 3, len(name)) + vinput.pad(name) + struct.pack("<I", 7)
        self.assertEqual(vinput.parse_global(body), (3, "wl_seat", 7))

    def test_key_codes_are_evdev(self):
        self.assertEqual((vinput.EVDEV["w"], vinput.EVDEV["ctrl"], vinput.EVDEV["esc"]), (17, 29, 1))
        self.assertEqual(vinput.BTN["left"], 0x110)

    def test_real_steps_let_the_window_input_in_and_scripted_runs_otherwise_shut_it_out(self):
        opts = dict(ScenarioTests.OPTS, windowed=True)
        self.assertEqual(scenarios.build("realwalk", opts).env(opts)["NV_MOVE_REAL_INPUT"], "1")
        self.assertNotIn("NV_MOVE_REAL_INPUT", scenarios.build("goodsprings", opts).env(opts))

    def test_world_scenarios_free_the_player_by_the_quest_stages_that_do_it(self):
        p = scenarios.build("goodsprings", ScenarioTests.OPTS)
        self.assertIn(scenarios.DIK["enter"], [k for _, k, _ in p.keys])
        self.assertEqual((scenarios.QUEST_DONE_STAGE, scenarios.OPENING_QUEST), (200, "104c1c"))


class ClockSanityTests(unittest.TestCase):
    def rows(self, fdelta_scale=1.0, z=None):
        rows = parse.parse_frame_log(frame_log(n=400, slow_every=0))
        for i, r in enumerate(rows):
            r["fdelta"] = (r["dt_ms"] or 7.0) / 1000.0 * fdelta_scale
            if z:
                r["pos"][2] = z(i)
        return rows

    def test_game_time_ratio_and_walking_speed(self):
        ck = parse.game_clock_sanity(self.rows())
        self.assertAlmostEqual(ck["ratio"], 1.0, places=2)
        self.assertAlmostEqual(ck["speed"], 2.0 / 0.007, delta=5)
        slow = parse.game_clock_sanity(self.rows(fdelta_scale=0.5))
        self.assertAlmostEqual(slow["ratio"], 0.5, places=2)

    def test_fall_and_landing(self):
        ck = parse.game_clock_sanity(self.rows(z=lambda i: 1000.0 if i < 100 else max(0.0, 1000.0 - (i - 100) * 50.0)))
        self.assertTrue(ck["settled"])
        self.assertEqual(ck["fall"], 1000.0)
        ck = parse.game_clock_sanity(self.rows(z=lambda i: 1000.0 - i * 5.0))
        self.assertFalse(ck["settled"])  # still falling at the end

    def test_too_little_play_gives_nothing(self):
        self.assertIsNone(parse.game_clock_sanity([])["ratio"])


class SuiteTests(unittest.TestCase):
    def summary(self, name="goodsprings", fps_dt=None, noisy=False):
        s = make_summary()
        s["scenario"], s["name"] = name, f"{name}-20260101T000000"
        s["run"] = dict(s["run"], noisy=noisy)
        s["metrics"]["hitches"] = {"count": 3, "per_min": 1.5}
        s["metrics"]["main_busy_pct"] = 80.0
        return s

    def test_headline_has_the_numbers_first(self):
        h = suite.headline(self.summary())
        for k in ("fps", "p50_ms", "p99_ms", "low1_fps", "low01_fps", "busy_ms", "hitches_per_min", "vram_peak_mib", "verdict", "noisy"):
            self.assertIn(k, h)
        self.assertEqual(h["hitches_per_min"], 1.5)
        self.assertIn("fps average", report.markdown(self.summary()).split("\n")[2])

    def test_capped_runs_are_not_headlines(self):
        s = self.summary()
        s["settings"]["limiters"] = {"NV_FRAME_LIMIT": "144"}
        self.assertEqual(suite.headline(s)["verdict"], "CAPPED")
        self.assertTrue(report.markdown(s).startswith("# Bench run: goodsprings\n\nCAPPED (limiter set: NV_FRAME_LIMIT=144)"))
        s = self.summary()
        s["settings"].update(limiters={}, mode="unlimited")
        s["metrics"]["frame_time_play"]["fps"] = 143.2  # within 2% of 144: something else limits it
        self.assertIn("within 2% of the 144", suite.headline(s)["capped"])
        s["metrics"]["frame_time_play"]["fps"] = 412.0
        self.assertIsNone(suite.headline(s)["capped"])
        self.assertEqual(suite.limiters_in({"NV_FRAME_LIMIT": "0", "NV_FPS_LIMIT": "60"}), {"NV_FPS_LIMIT": "60"})
        s["settings"]["mode"] = "capped"
        self.assertEqual(suite.capped_reason(s["settings"], 90.0), "--cap")

    def test_table_deltas_and_noisy_marker(self):
        a, b = suite.headline(self.summary("newgame")), suite.headline(self.summary("goodsprings", noisy=True))
        prev = {"newgame": dict(a, fps=a["fps"] / 2)}
        base = {"goodsprings": dict(b, fps=b["fps"] * 2)}
        t = suite.table([a, b], prev, base)
        self.assertIn("fps vs previous", t)
        self.assertIn("+100.0%", t)  # newgame doubled against the previous suite
        self.assertIn("-50.0%", t)  # goodsprings halved against the baseline
        self.assertIn("NOISY", t)
        self.assertIsNone(suite.delta_pct(1, 0))

    def test_baselines_and_previous_suite_round_trip(self):
        with tempfile.TemporaryDirectory() as d:
            rows = [suite.headline(self.summary("newgame"))]
            suite.save_baseline(d, "ref", rows)
            self.assertEqual(suite.load_baseline(d, "ref")["newgame"]["scenario"], "newgame")
            self.assertIsNone(suite.load_baseline(d, "nope"))
            for n in ("suite-20260101T000000", "suite-20260102T000000"):
                (Path(d) / n).mkdir()
                (Path(d) / n / "suite.json").write_text(json.dumps({"rows": rows}))
            self.assertIn("newgame", suite.previous_suite(d, "suite-20260103T000000"))
            self.assertIsNone(suite.previous_suite(d, "suite-20260101T000000"))

    def test_perf_busy_and_hitches_parse(self):
        log = parse.parse_log("NV_PERF: thread busy over 10.0 s, 2.3 cores in all: nv-main/1234 84%, render/5 40%\n")
        self.assertEqual(log["busiest_thread_pct"], [84.0])
        rows = parse.parse_frame_log(frame_log(n=400, slow_every=100, slow_us=60000))
        h = parse.hitches(rows)
        self.assertEqual(h["count"], 3)
        self.assertGreater(h["per_min"], 0)


class QuickSuiteTests(unittest.TestCase):
    LOG = """# driver: phase startup start frame=1 t=0.10
# driver: phase startup end frame=6000 t=40.00
# driver: phase goodsprings start frame=6001 t=40.10
# driver: phase goodsprings.measured start frame=6500 t=48.00
# driver: phase goodsprings.walk1 start frame=6600 t=50.00
# driver: phase goodsprings.walk1 end frame=6700 t=53.00
# driver: phase goodsprings.walk2 start frame=6800 t=55.00
# driver: phase goodsprings.walk2 end frame=6900 t=58.00
# driver: phase goodsprings.measured end frame=6900 t=58.00
"""

    def test_phases_parse_with_their_frames_and_seconds(self):
        log = parse.parse_log(self.LOG)
        self.assertEqual(log["phases"][0], ("startup", "start", 1, 0.1))
        self.assertEqual(len(log["phases"]), 9)

    def test_measured_phase_gets_its_own_fps_and_wall(self):
        rows = parse.parse_frame_log(frame_log(n=7000, dt_us=2000, slow_every=0))
        for r in rows:
            r["screen"] = "play"
        ph = {p["name"]: p for p in parse.phase_stats(rows, parse.parse_log(self.LOG)["phases"])}
        self.assertEqual(ph["startup"]["wall_s"], 39.9)
        self.assertAlmostEqual(ph["goodsprings.measured"]["fps"], 500.0, delta=1)

    def test_measured_phase_gets_the_busy_reports_logged_inside_it(self):
        text = self.LOG.replace("# driver: phase goodsprings.walk1 start", "NV_PERF: thread busy over 4.0 s, 2.0 cores in all: game main threa/1 80%, x/2 10%\n# driver: phase goodsprings.walk1 start")
        text = "NV_PERF: thread busy over 4.0 s, 2.0 cores in all: game main threa/1 20%\n" + text
        log = parse.parse_log(text)
        rows = parse.parse_frame_log(frame_log(n=7000, dt_us=2000, slow_every=0))
        for r in rows:
            r["screen"] = "play"
        ph = {p["name"]: p for p in parse.phase_stats(rows, log["phases"], log["phase_busy_pct"])}
        self.assertEqual(ph["goodsprings.measured"]["busy_pct"], 80.0)
        self.assertAlmostEqual(ph["goodsprings.measured"]["busy_ms"], 1.6, delta=0.01)

    def test_heading_change_between_the_two_walks(self):
        rows = parse.parse_frame_log(frame_log(n=7000, dt_us=10000, slow_every=0, walk=False))
        for r in rows:
            r["screen"] = "play"
            f = r["frame"]
            if 6600 <= f <= 6700:
                r["pos"][:2] = [(f - 6600) * 5.0, 0.0]  # along +x
            elif 6800 <= f <= 6900:
                turn = 0 if f < 6830 else 1  # a first stretch, then walking along +y
                r["pos"][:2] = [500.0 + (0 if turn else (f - 6800) * 5.0), (f - 6830) * 5.0 if turn else 0.0]
        deg = parse.heading_changes(rows, parse.parse_log(self.LOG)["phases"])
        self.assertAlmostEqual(deg["goodsprings"], 90.0, delta=1)

    def test_too_long_and_budget_check(self):
        s = make_summary()
        s["settings"]["budget_s"] = 120.0
        s["run"]["wall_s"] = 130.0
        self.assertEqual(suite.headline(s)["verdict"], "TOO_LONG")
        s["run"]["wall_s"] = 100.0
        self.assertNotEqual(suite.headline(s)["verdict"], "TOO_LONG")
        plan = scenarios.build("quick", dict(ScenarioTests.OPTS, legacy=False))
        self.assertEqual(plan.budget_s, 120.0)
        checks = nvbench.verdict_checks(make_summary()["metrics"], plan, 150.0)
        self.assertTrue(any("TOO_LONG" in c["name"] and not c["ok"] for c in checks))

    def test_world_scenarios_are_one_session_and_quick_has_three_places(self):
        import drivers
        self.assertEqual(scenarios.WORLD["quick"][0], ("doc_house", "goodsprings", "strip"))
        for n in ("doc_house", "goodsprings", "strip", "novac", "quick"):
            self.assertIn(n, drivers.DRIVERS)
            self.assertIn(n, scenarios.SCENARIOS)

    def test_phase_rows_expand_a_quick_run_into_its_places(self):
        s = make_summary()
        s["scenario"] = "quick"
        s["metrics"]["phases"] = [{"name": "strip.measured", "wall_s": 18.0, "fps": 321.0, "p99_ms": 4.0, "low1_fps": 200.0}]
        rows = suite.phase_rows(s)
        self.assertEqual((rows[0]["scenario"], rows[0]["fps"]), ("quick:strip", 321.0))


class DriverTests(unittest.TestCase):
    OPTS = dict(ScenarioTests.OPTS, legacy=False, mode="unlimited", windowed=True, scale=0.34)

    def test_default_scenarios_are_drivers_with_no_frame_script(self):
        for name in ("newgame", "goodsprings", "novac", "strip", "combat", "soak", "boot"):
            plan = scenarios.build(name, self.OPTS)
            self.assertTrue(callable(plan.drive), name)
            env = plan.env(self.OPTS)
            self.assertNotIn("NV_MOVE_KEYS", env)
            self.assertFalse(plan.capped_only)

    def test_threads_are_real_unless_asked(self):
        plan = scenarios.build("boot", self.OPTS)
        self.assertEqual(plan.env(self.OPTS)["NV_THREADS"], "real")
        self.assertEqual(plan.env(dict(self.OPTS, threads="stepped"))["NV_THREADS"], "stepped")

    def test_unlimited_mode_is_real_play_with_the_menu_limit(self):
        env = scenarios.build("boot", self.OPTS).env(self.OPTS)
        self.assertEqual((env["NV_FRAME_LIMIT"], env["NV_MENU_FRAME_LIMIT"], env["NV_PRESENT"]), ("0", "120", "mailbox"))
        capped = scenarios.build("boot", dict(self.OPTS, mode="capped", cap=60)).env(dict(self.OPTS, mode="capped", cap=60))
        self.assertEqual((capped["NV_FRAME_LIMIT"], capped["NV_FPS_LIMIT"]), ("60", "60"))
        peak = scenarios.build("boot", dict(self.OPTS, mode="peak")).env(dict(self.OPTS, mode="peak"))
        self.assertEqual((peak["NV_PRESENT"], peak["NV_MENU_FRAME_LIMIT"]), ("immediate", "120"))

    def test_frame_log_tail_parses_whole_lines_only(self):
        import driver
        text = "7\t1\t0.01\t0\t0\t1.5\t2.5\t3.5\tmenu\n8\t1\t0.01\t0\t0\t1.5\t2.5\t3.5\tplay\n9\t1\t0.0"
        self.assertEqual([(r[0], r[1]) for r in driver.parse_tail(text)], [(8, "play")])

    def test_driver_waits_on_the_screen_and_taps_while_it_waits(self):
        import driver

        class Proc:
            def poll(self):
                return None

        class Vi:
            taps = []

            def tap(self, key, hold):
                self.taps.append(key)

        clock = [0.0]
        d = driver.Driver(Vi(), "/nonexistent", Proc(), clock=lambda: clock[0], sleep=lambda s: clock.__setitem__(0, clock[0] + s))
        screens = iter(["loading"] * 30 + ["play"] * 10)
        d.screen = lambda: next(screens, "play")
        self.assertTrue(d.until_not_screen("loading", 60, tap="esc", tap_every=1.0))
        self.assertTrue(Vi.taps and set(Vi.taps) == {"esc"})
        d2 = driver.Driver(Vi(), "/nonexistent", Proc(), clock=lambda: clock[0], sleep=lambda s: clock.__setitem__(0, clock[0] + s))
        d2.screen = lambda: "menu"
        self.assertFalse(d2.until_screen("play", 3))


    def test_settle_play_dismisses_a_box_and_tries_the_console_key_when_it_stays(self):
        import driver

        class Proc:
            def poll(self):
                return None

        class Vi:
            def __init__(self):
                self.taps = []

            def tap(self, key, hold):
                self.taps.append(key)

        clock = [0.0]
        vi = Vi()
        d = driver.Driver(vi, "/nonexistent", Proc(), clock=lambda: clock[0], sleep=lambda s: clock.__setitem__(0, clock[0] + s))
        screens = iter(["menu"] * 130 + ["play"] * 40)  # 13 s of a box that will not go, then play
        d.screen = lambda: next(screens, "play")
        self.assertTrue(d.settle_play(quiet=2.0, timeout=60))
        self.assertEqual(vi.taps[:3], ["enter", "down", "enter"])
        self.assertEqual(vi.taps.count("grave"), 2)  # at 6 s and at 12 s of menu
        vi.taps.clear()
        screens = iter(["menu"] * 130 + ["play"] * 40)
        self.assertTrue(d.settle_play(quiet=2.0, timeout=60, tap="enter", console_after=None))
        self.assertEqual(set(vi.taps), {"enter"})


class CliTests(unittest.TestCase):
    def test_analyze_directory_and_windowed_refusal(self):
        with tempfile.TemporaryDirectory() as d:
            run = Path(d) / "boot-20260101T000000"
            run.mkdir()
            (run / "viewer.log").write_text(VIEWER_LOG.replace("2 wgpu errors", "0 wgpu errors"))
            (run / "frames.tsv").write_text(frame_log())
            summary = nvbench.analyze_dir(run)
            self.assertEqual(summary["scenario"], "boot")
            self.assertFalse(summary["verdict"]["ok"])  # the fixture has a stop and a panic
            nvbench.write_reports(run, summary)
            self.assertTrue((run / "report.html").exists())
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = nvbench.main(["report", str(run)])
            self.assertEqual(rc, 0)
            self.assertIn("Bench run: boot", buf.getvalue())
        self.assertEqual(nvbench.main(["run", "boot", "--windowed"]), 2)


if __name__ == "__main__":
    unittest.main()
