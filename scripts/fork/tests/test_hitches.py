"""Tests of scripts/fork/hitches.py and runs.py on synthetic logs and summaries (no game, no build)."""
import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import hitches  # noqa: E402
import runs  # noqa: E402


def hitch(frame, ms, cause, trigger, end_ms, samples=None):
    h = {"frame_idx": frame, "wall_ms": ms, "cause": cause, "trigger": trigger,
         "frame": {"end_ms": end_ms, "cell": "001658b7", "screen": "play"}}
    if samples:
        h["samples"] = samples
    return json.dumps({"ts": "T", "level": "warn", "target": "hitch", "kind": "hitch", "hitch": h, "frame": frame})


LOG = "\n".join([
    '{"ts":"T","level":"info","target":"host","message":"not a hitch"}',
    "garbage that is not json",
    hitch(100, 80.0, "file_io", "cell.change 20 ms before", 12_000,
          {"n": 20, "in_guest": 2, "calls": [{"call": "ReadFile", "n": 18, "arg": "file ext:nif hash:00ab"}],
           "hot": [{"addr": "0040abcd", "n": 18}]}),
    hitch(500, 150.0, "loading", "loading.begin 5 ms before", 15_000),
    hitch(900, 40.0, "file_io", "cell.change 300 ms before", 31_000),
    json.dumps({"ts": "T", "kind": "perf.slow_window", "slow_window": {"from_ms": 20000, "to_ms": 30000, "p99_ms": 31.0, "fps": 52.0, "cause_mix": {"game_logic": 12}}}),
    json.dumps({"ts": "T", "kind": "hitch_summary", "summary": {"frames": 5000, "per_minute": 3.5, "over_ms": {"16.7": 30}}}),
])


class HitchesTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = Path(self.dir.name) / "run.jsonl"
        self.path.write_text(LOG)

    def tearDown(self):
        self.dir.cleanup()

    def test_load_keeps_only_the_recorders_records(self):
        h, w, s = hitches.load(self.path)
        self.assertEqual((len(h), len(w)), (3, 1))
        self.assertEqual(s["frames"], 5000)

    def test_rank_orders_by_time_and_groups_triggers_by_kind(self):
        h, _, _ = hitches.load(self.path)
        r = hitches.rank(h)
        self.assertEqual(r["cause"][0][0], "loading")
        file_io = [row for row in r["cause"] if row[0] == "file_io"][0]
        self.assertEqual((file_io[1], file_io[2], file_io[3]), (2, 120.0, 80.0))
        trig = {row[0]: row[1] for row in r["trigger"]}
        self.assertEqual(trig["cell.change"], 2)

    def test_the_report_names_the_sampled_function_and_the_timeline(self):
        h, w, s = hitches.load(self.path)
        text = hitches.report(h, w, s, names={0x0040ABCD: "Actor::Update"})
        self.assertIn("3 hitches", text)
        self.assertIn("ReadFile (file ext:nif hash:00ab) x18", text)
        self.assertIn("Actor::Update x18", text)
        self.assertIn("Slow windows", text)
        self.assertIn("Timeline", text)

    def test_cli_prints_and_json_mode_is_json(self):
        out = io.StringIO()
        with redirect_stdout(out):
            self.assertEqual(hitches.main([str(self.path), "--json"]), 0)
        self.assertEqual(json.loads(out.getvalue())["hitches"], 3)
        self.assertEqual(hitches.main([str(self.path) + ".missing"]), 2)


def summary(unix, pid, fps, p99, per_min, flags=(), ended="exit"):
    return {"schema": "nv-rs-run-summary/1", "id": f"{unix}-{pid}", "started_unix": unix, "ended": ended,
            "play_s": 300, "frame_ms": {"fps": fps, "p99": p99}, "hitches": {"count": 3, "per_min": per_min},
            "flags": list(flags), "verdict": "v", "log": "x.jsonl"}


class RunsTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        d = Path(self.dir.name)
        for s in (summary(1000, 1, 140.0, 9.0, 0.5),
                  summary(2000, 2, 120.0, 22.0, 4.0, ["p99_over_20ms", "hitches_per_min_high"]),
                  summary(3000, 3, 130.0, 12.0, 1.0)):
            (d / f"run-summary-{s['started_unix']}-{s['id'].split('-')[1]}.json").write_text(json.dumps(s))
        (d / "run-summary-bad.json").write_text("{")
        (d / "nv-rs-1-1.jsonl").write_text("")

    def tearDown(self):
        self.dir.cleanup()

    def test_runs_are_in_time_order_with_a_trend_against_the_one_before(self):
        table = runs.rows(runs.load_runs(self.dir.name))
        self.assertEqual([t["id"] for t in table], ["1000-1", "2000-2", "3000-3"])
        self.assertIsNone(table[0]["trend"])
        self.assertEqual(table[1]["trend"]["fps"], -20.0)
        self.assertEqual(table[2]["trend"]["p99_ms"], -10.0)

    def test_flagged_and_last_filter(self):
        loaded = runs.load_runs(self.dir.name)
        self.assertEqual([t["id"] for t in runs.rows(loaded, flagged=True)], ["2000-2"])
        self.assertEqual([t["id"] for t in runs.rows(loaded, last=1)], ["3000-3"])

    def test_a_running_summary_of_a_dead_process_is_an_unclean_exit(self):
        d = Path(self.dir.name)
        s = summary(4000, 999999999, 100.0, 10.0, 0.0, ended="running")
        (d / "run-summary-4000-999999999.json").write_text(json.dumps(s))
        r = runs.load_runs(self.dir.name)[-1]
        if sys.platform.startswith("linux"):
            self.assertIn("unclean_exit", r["flags"])

    def test_show_and_the_cli(self):
        out = io.StringIO()
        with redirect_stdout(out):
            self.assertEqual(runs.main(["--dir", self.dir.name, "--json", "--flagged"]), 0)
        self.assertEqual(json.loads(out.getvalue())[0]["flags"][0], "p99_over_20ms")
        out = io.StringIO()
        with redirect_stdout(out):
            self.assertEqual(runs.main(["--dir", self.dir.name, "show", "2000"]), 0)
        self.assertIn("run 2000-2", out.getvalue())
        with redirect_stdout(io.StringIO()):
            self.assertEqual(runs.main(["--dir", self.dir.name, "show", "nope"]), 2)
        self.assertIn("p99_over_20ms", runs.render(runs.rows(runs.load_runs(self.dir.name))))


if __name__ == "__main__":
    unittest.main()
