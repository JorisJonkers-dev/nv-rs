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


class AllJobs(unittest.TestCase):
    def test_jobs_past_the_first_page_are_read(self):
        # 105 jobs, "Pipeline Complete" last: one page would not see it
        jobs = [{"name": f"Build / Component {i}"} for i in range(104)] + [{"name": "Pipeline Complete"}]
        pages = []

        def get_page(rid, page, n):
            pages.append(page)
            return {"total_count": len(jobs), "jobs": jobs[(page - 1) * n:page * n]}

        got = g.all_jobs(get_page, 7)
        self.assertEqual(len(got), 105)
        self.assertEqual(got[-1]["name"], "Pipeline Complete")
        self.assertEqual(pages, [1, 2, 3])

    def test_one_short_page_is_one_request(self):
        pages = []

        def get_page(rid, page, n):
            pages.append(page)
            return {"total_count": 2, "jobs": [{"name": "a"}, {"name": "b"}]}

        self.assertEqual(len(g.all_jobs(get_page, 7)), 2)
        self.assertEqual(pages, [1])


class Api(unittest.TestCase):
    class Done:
        def __init__(self, code, out="", err=""):
            self.returncode, self.stdout, self.stderr = code, out, err

    def test_a_failed_call_is_tried_again(self):
        answers = [self.Done(1, err="gh: Server Error (HTTP 502)"), self.Done(0, out='{"jobs": []}')]
        waits = []
        got = g.api("repos/x/actions/runs/1/jobs", run=lambda *a, **k: answers.pop(0), wait=waits.append)
        self.assertEqual(got, {"jobs": []})
        self.assertEqual(waits, [5])

    def test_three_failed_calls_fail_the_gate(self):
        calls = []

        def run(*a, **k):
            calls.append(a)
            return self.Done(1, err="gh: Server Error (HTTP 502)")

        with self.assertRaises(RuntimeError):
            g.api("repos/x/actions/runs/1/jobs", run=run, wait=lambda s: None)
        self.assertEqual(len(calls), 3)


class OfThisPullRequest(unittest.TestCase):
    def test_a_run_naming_another_pull_request_is_left_out(self):
        r = {"pull_requests": [{"number": 5}], "head_branch": "b", "head_repository": {"full_name": "o/r"}}
        self.assertFalse(g.of_this_pull_request(r, 6, "b", "o/r"))
        self.assertTrue(g.of_this_pull_request(r, 5, "b", "o/r"))

    def test_a_run_from_a_fork_matches_by_head_branch_and_repository(self):
        r = {"pull_requests": [], "head_branch": "b", "head_repository": {"full_name": "someone/r"}}
        self.assertTrue(g.of_this_pull_request(r, 6, "b", "someone/r"))
        self.assertFalse(g.of_this_pull_request(r, 6, "b", "other/r"))
        self.assertFalse(g.of_this_pull_request(r, 6, "c", "someone/r"))


if __name__ == "__main__":
    unittest.main()
