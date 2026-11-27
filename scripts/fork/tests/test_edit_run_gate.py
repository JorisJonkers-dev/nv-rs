# SPDX-License-Identifier: GPL-3.0-only
"""Tests of edit_run_gate.py (Pipeline Complete of a description edit), on synthetic run lists.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v        (no network, no cargo)
"""
import os
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
        self.assertEqual(g.verdict([run(1, "success")])[0], "pass")

    def test_a_red_commits_run_keeps_the_edit_red(self):
        state, message = g.verdict([run(1, "failure", gate="failure")])
        self.assertEqual(state, "fail")
        self.assertIn("run 1", message)

    def test_earlier_edit_runs_are_passed_over_for_the_commits_run(self):
        # newest first: an edit run that passed, then the red run of the commits
        self.assertEqual(g.verdict([run(3, "skipped"), run(2, "skipped"), run(1, "failure", gate="failure")])[0], "fail")
        self.assertEqual(g.verdict([run(3, "skipped", gate="failure"), run(1, "success")])[0], "pass")

    def test_a_commits_run_still_going_makes_the_edit_wait_not_fail(self):
        state, message = g.verdict([run(1, "success", gate_status="in_progress")])
        self.assertEqual(state, "wait")
        self.assertIn("has not finished", message)

    def test_a_commits_run_whose_gate_has_not_started_makes_the_edit_wait(self):
        self.assertEqual(g.verdict([run(1, None, gate_status=None)])[0], "wait")

    def test_a_cancelled_commits_run_fails_the_edit(self):
        self.assertEqual(g.verdict([run(1, "success", gate="cancelled")])[0], "fail")

    def test_no_commits_run_yet_waits(self):
        self.assertEqual(g.verdict([])[0], "wait")
        self.assertEqual(g.verdict([run(2, "skipped")])[0], "wait")


class Main(unittest.TestCase):
    """main() with the GitHub calls replaced: a sequence of looks and of pull request states."""

    def setUp(self):
        self.env = {"REPO": "o/r", "HEAD_SHA": "abc123456", "RUN_ID": "9", "PR_NUMBER": "5", "HEAD_REF": "b",
                    "HEAD_REPO": "o/r", "GATE_POLL": "60", "GATE_LIMIT": "600", "GATE_GRACE": "120"}
        for k, v in self.env.items():
            os.environ[k] = v
        self.addCleanup(lambda: [os.environ.pop(k, None) for k in self.env])
        self.now, self.slept = 0, []
        self.looks, self.heads = [], []
        self._look, self._sup = g.look, g.superseded
        g.look = lambda *a: self.looks.pop(0) if len(self.looks) > 1 else self.looks[0]
        g.superseded = lambda *a: self.heads.pop(0) if self.heads else False
        self.addCleanup(lambda: (setattr(g, "look", self._look), setattr(g, "superseded", self._sup)))

    def go(self):
        def sleep(s):
            self.slept.append(s)
            self.now += s
        return g.main(clock=lambda: self.now, wait=sleep)

    def test_it_waits_for_the_commits_run_and_then_passes_without_ever_failing(self):
        self.looks = [("wait", "run 1 of the commits has not finished")] * 3 + [("pass", "run 1 passed")]
        self.assertEqual(self.go(), 0)
        self.assertEqual(self.slept, [60, 60, 60])

    def test_a_red_commits_run_fails_when_it_finishes(self):
        self.looks = [("wait", "run 1 of the commits has not finished"), ("fail", "run 1: Pipeline Complete failure")]
        self.assertEqual(self.go(), 1)

    def test_a_head_that_moved_on_ends_the_wait_green(self):
        self.looks = [("wait", "run 1 of the commits has not finished")]
        self.heads = [False, True]
        self.assertEqual(self.go(), 0)
        self.assertEqual(self.slept, [60])

    def test_a_run_that_never_ends_fails_at_the_limit(self):
        self.looks = [("wait", "run 1 of the commits has not finished")]
        self.assertEqual(self.go(), 1)
        self.assertEqual(sum(self.slept), 600)

    def test_no_run_at_all_fails_after_the_grace(self):
        self.looks = [("wait", "no run of the commits on this head commit yet")]
        self.assertEqual(self.go(), 1)
        self.assertEqual(sum(self.slept), 120)


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
