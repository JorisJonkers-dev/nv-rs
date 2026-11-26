"""Tests of coverage_report.py (fork-coverage.yml).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v        (no network, no cargo)
"""
import argparse
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import coverage_report as cr  # noqa: E402

LCOV = """SF:/home/runner/work/nv-rs/nv-rs/crates/esm/src/a.rs
DA:1,1
DA:2,0
DA:3,4
end_of_record
SF:/home/runner/work/nv-rs/nv-rs/crates/bsa/src/b.rs
DA:1,0
end_of_record
"""


class ParseTests(unittest.TestCase):
    def write(self, text):
        d = tempfile.mkdtemp()
        p = Path(d) / "lcov.info"
        p.write_text(text)
        return str(p)

    def test_paths_are_relative_to_the_repository(self):
        files = cr.parse_lcov(self.write(LCOV))
        self.assertEqual(sorted(files), ["crates/bsa/src/b.rs", "crates/esm/src/a.rs"])

    def test_totals(self):
        files = cr.parse_lcov(self.write(LCOV))
        self.assertEqual(cr.totals(files), (2, 4))

    def test_missing_file_is_none(self):
        self.assertIsNone(cr.parse_lcov("/nonexistent/lcov.info"))

    def test_repeated_line_keeps_the_best_count(self):
        files = cr.parse_lcov(self.write("SF:/x/crates/a.rs\nDA:1,0\nend_of_record\nSF:/x/crates/a.rs\nDA:1,3\nend_of_record\n"))
        self.assertEqual(files["crates/a.rs"][1], 3)


class GateTests(unittest.TestCase):
    MEASURED = {"crates/fork-log/src/a.rs": {1: 1, 2: 0, 3: 2, 4: 1}, "crates/esm/src/b.rs": {1: 0}}
    RANGES = {"crates/fork-log/src/a.rs": {1, 2, 3, 9}, "crates/esm/src/b.rs": {1}}

    def test_only_fork_owned_crates_count(self):
        total, miss, value, per = cr.gate_eval(self.MEASURED, self.RANGES, {"fork-log"}, 70.0)
        self.assertEqual((total, miss), (3, 1))
        self.assertAlmostEqual(value, 66.666, places=2)
        self.assertEqual(list(per), ["fork-log"])

    def test_nothing_to_gate(self):
        self.assertIsNone(cr.gate_eval(self.MEASURED, {"crates/esm/src/b.rs": {1}}, {"fork-log"}, 70.0)[2])

    def test_enforced_only_with_switch_and_nightly_baseline(self):
        result = cr.gate_eval(self.MEASURED, self.RANGES, {"fork-log"}, 70.0)
        nightly, own = {"the last nightly run, run 1"}, {"the base commit abc, measured in this run"}
        cfg = {"enforce": True, "min_changed_line_coverage": 70.0}
        self.assertEqual(cr.render_gate(cfg, ["fork-log"], nightly, result)[1:], ("below", True))
        self.assertEqual(cr.render_gate(cfg, ["fork-log"], own, result)[1:], ("below", False))
        off = dict(cfg, enforce=False)
        self.assertFalse(cr.render_gate(off, ["fork-log"], nightly, result)[2])

    def test_a_baseline_at_the_merge_base_enforces_and_the_one_without_a_nightly_does_not(self):
        result = cr.gate_eval(self.MEASURED, self.RANGES, {"fork-log"}, 70.0)
        cfg = {"enforce": True, "min_changed_line_coverage": 70.0}
        exact = {"the last nightly run, run 1, on the merge base abc"}
        measured = {"the merge base abc, measured in this run (the last nightly, run 1, was on def)"}
        self.assertTrue(cr.render_gate(cfg, ["fork-log"], exact, result)[2])
        self.assertTrue(cr.render_gate(cfg, ["fork-log"], measured, result)[2])

    def test_gate_fails_when_the_plan_artifact_is_missing(self):
        d = tempfile.mkdtemp()
        args = argparse.Namespace(dir=d, plan=str(Path(d) / "cov-plan" / "plan.json"),
                                  base="HEAD", head="HEAD", out=None)
        with patch.object(cr, "changed_ranges", return_value={}):
            with self.assertRaises(SystemExit) as caught:
                cr.gate(args)
        self.assertNotEqual(caught.exception.code, 0)

    def test_gate_fails_when_the_plan_artifact_is_unreadable(self):
        d = Path(tempfile.mkdtemp())
        (d / "cov-plan").mkdir()
        (d / "cov-plan" / "plan.json").write_text("not json")
        args = argparse.Namespace(dir=str(d), plan=str(d / "cov-plan" / "plan.json"),
                                  base="HEAD", head="HEAD", out=None)
        with patch.object(cr, "changed_ranges", return_value={}):
            with self.assertRaises(SystemExit) as caught:
                cr.gate(args)
        self.assertNotEqual(caught.exception.code, 0)

    def test_fork_owned_crates_are_derived(self):
        d = tempfile.mkdtemp()
        (Path(d) / ".fork").mkdir()
        (Path(d) / ".fork/overlay-paths").write_text("crates/fork-log/\ncrates/x/src/f.rs\n.fork/\n")
        (Path(d) / ".fork/components.toml").write_text('[[component]]\nname="w"\npurpose="A layer. Fork-owned"\ncrates=["fork-window"]\n'
                                                        '[[component]]\nname="c"\npurpose="Core"\ncrates=["engine-core"]\n')
        self.assertEqual(cr.fork_owned_crates(d), ["fork-log", "fork-window"])


class MergeTests(unittest.TestCase):
    def test_merge_reads_the_directory_it_is_given(self):
        d = Path(tempfile.mkdtemp())
        (d / "cov-formats").mkdir()
        (d / "cov-formats" / "lcov.info").write_text(LCOV)
        (d / "cov-formats" / "baseline.txt").write_text("nightly 123\n")
        plan_doc = {"include": [{"component": "formats"}, {"component": "core"}]}
        rows, merged_pr, merged_base, missing, baselines = cr.merge_dir(str(d), plan_doc, {"crates/esm/src/a.rs": {2}})
        self.assertEqual([r[0] for r in rows], ["formats"])
        self.assertEqual(rows[0][3], 1)
        self.assertEqual(sorted(merged_pr), ["crates/bsa/src/b.rs", "crates/esm/src/a.rs"])
        self.assertEqual(merged_base, {})
        self.assertEqual(missing, ["core"])
        self.assertEqual(baselines, {"nightly 123"})

class FormatTests(unittest.TestCase):
    def test_spans(self):
        self.assertEqual(cr.spans({1, 2, 3, 7, 9, 10}), "1-3,7,9-10")

    def test_delta_without_a_base(self):
        self.assertEqual(cr.fmt_delta(50.0, None), "n/a")
        self.assertEqual(cr.fmt_delta(50.5, 50.0), "+0.50 pp")

    def test_percent_of_nothing(self):
        self.assertIsNone(cr.pct(0, 0))


class PlanTests(unittest.TestCase):
    def test_absent_plan_means_nothing_to_measure(self):
        d = tempfile.mkdtemp()
        doc = cr.load_plan(str(Path(d) / "nope" / "plan.json"), d)
        self.assertEqual(doc["include"], [])
        self.assertIn("nothing was measured", doc["reason"])

    def test_plan_is_found_beside_the_directory_root(self):
        d = tempfile.mkdtemp()
        (Path(d) / "plan.json").write_text('{"include": [{"component": "formats"}], "reason": "x"}')
        self.assertEqual(cr.load_plan(str(Path(d) / "missing.json"), d)["include"][0]["component"], "formats")

    def test_unreadable_plan_is_empty(self):
        d = tempfile.mkdtemp()
        (Path(d) / "plan.json").write_text("not json")
        self.assertEqual(cr.load_plan(str(Path(d) / "plan.json"), d)["include"], [])

    def test_a_required_plan_that_is_missing_or_broken_raises(self):
        d = Path(tempfile.mkdtemp())
        with self.assertRaises(FileNotFoundError):
            cr.load_plan(str(d / "cov-plan" / "plan.json"), str(d), required=True)
        (d / "plan.json").write_text("not json")
        with self.assertRaises(FileNotFoundError):
            cr.load_plan(str(d / "cov-plan" / "plan.json"), str(d), required=True)

class PickRunTests(unittest.TestCase):
    RUNS = [
        {"databaseId": 5, "headSha": "ccc", "event": "schedule", "conclusion": "success"},
        {"databaseId": 4, "headSha": "bbb", "event": "pull_request", "conclusion": "success"},
        {"databaseId": 3, "headSha": "bbb", "event": "schedule", "conclusion": "failure"},
        {"databaseId": 2, "headSha": "bbb", "event": "workflow_dispatch", "conclusion": "success"},
        {"databaseId": 1, "headSha": "aaa", "event": "schedule", "conclusion": "success"},
    ]

    def test_the_run_on_the_merge_base_is_found(self):
        self.assertEqual(cr.pick_run(self.RUNS, "aaa"), 1)
        self.assertEqual(cr.pick_run(self.RUNS, "ccc"), 5)

    def test_pull_request_and_failed_runs_do_not_count(self):
        self.assertEqual(cr.pick_run(self.RUNS, "bbb"), 2)
        self.assertIsNone(cr.pick_run(self.RUNS[1:3], "bbb"))

    def test_no_run_on_that_commit_is_none(self):
        self.assertIsNone(cr.pick_run(self.RUNS, "zzz"))
        self.assertIsNone(cr.pick_run([], "aaa"))



if __name__ == "__main__":
    unittest.main()
