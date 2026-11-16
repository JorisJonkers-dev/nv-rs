"""Tests of coverage_report.py (fork-coverage.yml).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v        (no network, no cargo)
"""
import sys
import tempfile
import unittest
from pathlib import Path

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


if __name__ == "__main__":
    unittest.main()
