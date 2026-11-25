# SPDX-License-Identifier: GPL-3.0-only
"""Tests of edit_run_gate.py (Pipeline Complete of a description edit), on synthetic run lists.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v        (no network, no cargo)
"""
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import edit_run_gate as g  # noqa: E402


def run(rid, code, gate_status="completed", gate="success"):
    jobs = [{"name": "Repository checks", "status": "completed", "conclusion": code}]
    if gate_status is not None:
        jobs.append({"name": "Pipeline Complete", "status": gate_status, "conclusion": gate if gate_status == "completed" else None})
    return {"id": rid, "jobs": jobs}


class Verdict(unittest.TestCase):
    def test_the_commits_run_that_passed_passes_the_edit(self):
        self.assertEqual(g.verdict([run(1, "success")])[0], True)

    def test_a_red_commits_run_keeps_the_edit_red(self):
        passed, message = g.verdict([run(1, "failure", gate="failure")])
        self.assertFalse(passed)
        self.assertIn("run 1", message)

    def test_earlier_edit_runs_are_passed_over_for_the_commits_run(self):
        # newest first: an edit run that passed, then the red run of the commits
        self.assertFalse(g.verdict([run(3, "skipped"), run(2, "skipped"), run(1, "failure", gate="failure")])[0])
        self.assertTrue(g.verdict([run(3, "skipped", gate="failure"), run(1, "success")])[0])

    def test_a_commits_run_still_going_fails_the_edit(self):
        passed, message = g.verdict([run(1, "success", gate_status="in_progress")])
        self.assertFalse(passed)
        self.assertIn("has not finished", message)

    def test_a_commits_run_whose_gate_has_not_started_fails_the_edit(self):
        self.assertFalse(g.verdict([run(1, None, gate_status=None)])[0])

    def test_a_cancelled_commits_run_fails_the_edit(self):
        self.assertFalse(g.verdict([run(1, "success", gate="cancelled")])[0])

    def test_no_commits_run_fails_the_edit(self):
        self.assertFalse(g.verdict([])[0])
        self.assertFalse(g.verdict([run(2, "skipped")])[0])


if __name__ == "__main__":
    unittest.main()
