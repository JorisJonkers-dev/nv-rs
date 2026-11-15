"""Tests of scripts/fork/bench (the playthrough and benchmark runner's parsing, report and compare code).

Synthetic fixtures only: no game, no build, no GPU. Run: python3 -I -m unittest discover -s scripts/fork/tests -v
"""
import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent / "bench"
sys.path.insert(0, str(BENCH))

import nvbench  # noqa: E402
import parse  # noqa: E402
import report  # noqa: E402
import scenarios  # noqa: E402

HEADER = "frame\tus\tm_fdelta\thk_count\thk_time\tpx\tpy\tpz\tscreen\tqpc\tpoll_qpc\tpresent\tshown\tshown_qpc\tinput_to_present_us"


def frame_log(n=300, dt_us=7000, slow_every=100, slow_us=40000, walk=True):
    lines = [HEADER]
    us = 1_000_000
    for i in range(n):
        us += slow_us if slow_every and i and i % slow_every == 0 else dt_us
        screen = "menu" if i < 50 else "play"
        x = 100.0 + (i - 50) * 2.0 if walk and i >= 50 else 0.0
        lines.append(f"{i}\t{us}\t0.016\t1\t0.0\t{x}\t200.0\t10.0\t{screen}\t0\t0\t0\t0\t0\t")
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
    OPTS = {"mode": "capped", "windowed": False, "preset": "fixes", "threads": None, "resolution": "1920x1080", "sound": None, "fps": 100.0}

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
                    int(f), int(k, 16), int(h)


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
