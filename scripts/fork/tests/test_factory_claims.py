# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the cross-machine claims (scripts/fork/factory_claims.py) against a fake GitHub API behind `gh api`."""
import importlib
import io
import json
import os
import sys
import threading
import unittest
from collections.abc import Callable
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import factory_claims as fc

REPO = "o/r"
KEY = "porting-extra_data"
POST_COMMENT = "POST repos/o/r/issues/7/comments"


class FakeGithub:
    """One repository's issues: comments, labels and assignees, answering the `gh api` argv the claims send."""

    def __init__(self) -> None:
        self.lock = threading.RLock()
        self.comments: dict[int, str] = {}
        self.next_id = 100
        self.labels: set[int] = set()
        self.assignees: dict[int, set[str]] = {}
        self.repo_label = False
        self.before: tuple[str, Callable[[], None]] | None = None
        self.fail: str | None = None
        self.log: list[str] = []

    def runner(self, argv: list[str]) -> tuple[int, str]:
        args = argv[2:]
        method = args[args.index("-X") + 1] if "-X" in args else "GET"
        path = next(a for a in args if a.startswith("repos/") or a == "user")
        fields = {a.split("=", 1)[0]: a.split("=", 1)[1] for a in args if "=" in a and not a.startswith("repos/")}
        tag = f"{method} {path}"
        self.log.append(tag)
        if self.before is not None and self.before[0] == tag:
            hook, self.before = self.before[1], None
            hook()
        with self.lock:
            if self.fail and self.fail in tag:
                return 1, "HTTP 500 server error"
            return self.handle(method, path, fields)

    def handle(self, method: str, path: str, fields: dict[str, str]) -> tuple[int, str]:
        if path == "user":
            return 0, "owner\n"
        parts = path.split("/")
        if parts[3] == "labels":
            if self.repo_label:
                return 1, "HTTP 422 already_exists"
            self.repo_label = True
            return 0, "{}"
        if parts[3] == "issues" and parts[4] == "comments":  # comment by id
            comment = int(parts[5])
            if comment not in self.comments:
                return 1, "HTTP 404 Not Found"
            if method == "DELETE":
                del self.comments[comment]
            else:
                self.comments[comment] = fields["body"]
            return 0, ""
        issue = int(parts[4])
        if parts[5] == "comments" and method == "GET":
            return 0, "".join(json.dumps({"id": i, "body": b}) + "\n" for i, b in sorted(self.comments.items()))
        if parts[5] == "comments":
            self.next_id += 1
            self.comments[self.next_id] = fields["body"]
            return 0, f"{self.next_id}\n"
        if parts[5] == "labels":
            if method == "POST":
                self.labels.add(issue)
            elif issue in self.labels:
                self.labels.discard(issue)
            else:
                return 1, "HTTP 404 Not Found"
            return 0, ""
        logins = self.assignees.setdefault(issue, set())
        if method == "POST":
            logins.add(fields["assignees[]"])
        else:
            logins.discard(fields["assignees[]"])
        return 0, ""

    def machine(self, name: str, clock: "Clock", lease: float = 100.0) -> fc.Claims:
        return fc.Claims(fc.GhApi(REPO, self.runner), name, clock, lease)


class Clock:
    def __init__(self, now: float = 1000.0) -> None:
        self.now = now

    def __call__(self) -> float:
        return self.now


class MarkerTests(unittest.TestCase):
    def test_round_trip(self) -> None:
        body = fc.body_of(KEY, "m1", 5.0)
        self.assertEqual(fc.marker_of(body), {"key": KEY, "machine": "m1", "expires": 5.0})
        self.assertIn("1970-01-01 00:00:05 UTC", body)

    def test_a_key_that_looks_like_the_end_of_the_marker(self) -> None:
        body = fc.body_of("a} -->b", "m} -->", 5.0)
        self.assertEqual(fc.marker_of(body), {"key": "a} -->b", "machine": "m} -->", "expires": 5.0})

    def test_not_a_lease(self) -> None:
        bad = ["hello", "<!-- factory-claim {nope} -->",
               '<!-- factory-claim {"key":"k","machine":"m","expires":"x"} -->',
               '<!-- factory-claim {"key":"k","machine":"m","expires":true} -->',
               '<!-- factory-claim {"key":"k","machine":"m","expires":Infinity} -->',
               '<!-- factory-claim {"key":1,"machine":"m","expires":1} -->',
               '<!-- factory-claim {"key":"k","machine":2,"expires":1} -->']
        for body in bad:
            self.assertIsNone(fc.marker_of(body), body)


class ClaimTests(unittest.TestCase):
    def setUp(self) -> None:
        self.gh = FakeGithub()
        self.clock = Clock()
        self.a = self.gh.machine("A", self.clock)
        self.b = self.gh.machine("B", self.clock)

    def test_acquire_sets_label_assignee_and_lease(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.assertEqual(self.gh.labels, {7})
        self.assertEqual(self.gh.assignees[7], {"owner"})
        self.assertEqual(len(self.gh.comments), 1)

    def test_second_machine_backs_off(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.assertFalse(self.b.acquire(7, KEY))
        self.assertEqual(len(self.gh.comments), 1)

    def test_same_machine_acquire_renews(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.clock.now += 60
        self.assertTrue(self.a.acquire(7, KEY))
        self.assertEqual(len(self.gh.comments), 1)
        self.assertEqual(self.a.entries(7, KEY)[0]["expires"], 1160.0)

    def test_stale_lease_is_reclaimed(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.clock.now += 101
        self.assertTrue(self.b.acquire(7, KEY))
        entries = self.b.entries(7, KEY)
        self.assertEqual([e["machine"] for e in entries], ["B"])
        self.assertFalse(self.a.renew(7, KEY))

    def test_renew_of_a_lease_reclaimed_meanwhile_is_a_loss_not_an_error(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.gh.before = ("PATCH repos/o/r/issues/comments/101", self.gh.comments.clear)
        self.assertFalse(self.a.renew(7, KEY))

    def test_acquire_of_a_lease_that_vanishes_claims_anew(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.gh.before = ("PATCH repos/o/r/issues/comments/101", self.gh.comments.clear)
        self.assertTrue(self.a.acquire(7, KEY))
        self.assertEqual([e["machine"] for e in self.a.entries(7, KEY)], ["A"])

    def test_renew_and_acquire_put_the_visible_lock_back(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.gh.labels.clear()
        self.gh.assignees[7].clear()
        self.assertTrue(self.a.renew(7, KEY))
        self.assertEqual((self.gh.labels, self.gh.assignees[7]), ({7}, {"owner"}))
        self.gh.labels.clear()
        self.assertTrue(self.a.acquire(7, KEY))
        self.assertEqual(self.gh.labels, {7})

    def test_a_vanished_lease_does_not_take_a_valid_one_of_another_machine_down(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.gh.comments[500] = fc.body_of(KEY, "B", self.clock() + 100)  # B posted right behind A
        def lose_a() -> None:
            del self.gh.comments[101]
            self.gh.next_id = 999
        self.gh.before = ("PATCH repos/o/r/issues/comments/101", lose_a)
        self.assertFalse(self.a.acquire(7, KEY))  # B's earlier valid claim now wins, and is still there
        self.assertEqual([e["machine"] for e in self.a.entries(7, KEY)], ["B"])

    def test_renew_edits_the_winning_lease_not_an_old_duplicate(self) -> None:
        self.gh.comments[1] = fc.body_of(KEY, "A", self.clock() - 5)  # an old expired lease of A
        self.gh.comments[2] = fc.body_of(KEY, "A", self.clock() + 50)
        self.assertTrue(self.a.renew(7, KEY))
        self.assertEqual(self.a.entries(7, KEY)[1]["expires"], 1100.0)
        self.assertEqual(self.a.entries(7, KEY)[0]["expires"], 995.0)

    def test_a_comment_without_an_id_is_skipped(self) -> None:
        class Odd(fc.GhApi):
            def comments(self, issue: int) -> list[dict[str, object]]:
                return [{"body": fc.body_of(KEY, "Z", 5000.0)}]
        claims = fc.Claims(Odd(REPO, self.gh.runner), "A", self.clock)
        self.assertEqual(claims.entries(7, KEY), [])

    def test_other_keys_are_independent(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.assertTrue(self.b.acquire(7, "porting-other"))

    def test_renew_extends_and_reports_loss(self) -> None:
        self.assertFalse(self.a.renew(7, KEY))  # nothing to renew
        self.assertTrue(self.a.acquire(7, KEY))
        self.clock.now += 99
        self.assertTrue(self.a.renew(7, KEY))
        self.clock.now += 99  # past the first expiry, inside the renewed one
        self.assertTrue(self.a.renew(7, KEY))
        self.clock.now += 101  # expired, but nobody took over
        self.assertTrue(self.a.renew(7, KEY))
        self.gh.comments.clear()
        self.assertFalse(self.a.renew(7, KEY))

    def test_renew_fails_when_another_machine_holds(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.clock.now += 101
        self.assertTrue(self.b.acquire(7, KEY))
        self.assertFalse(self.a.renew(7, KEY))

    def test_release_frees_label_and_assignee(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.a.release(7, KEY)
        self.assertEqual(self.gh.comments, {})
        self.assertEqual(self.gh.labels, set())
        self.assertEqual(self.gh.assignees[7], set())
        self.assertTrue(self.b.acquire(7, KEY))

    def test_release_keeps_label_while_another_claim_lives(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.assertTrue(self.b.acquire(7, "porting-other"))
        self.a.release(7, KEY)
        self.assertEqual(self.gh.labels, {7})
        self.b.release(7, "porting-other")
        self.assertEqual(self.gh.labels, set())

    def test_release_of_nothing_and_of_a_foreign_claim(self) -> None:
        self.b.release(7, KEY)  # label already off: the 404 is fine
        self.assertTrue(self.a.acquire(7, KEY))
        self.b.release(7, KEY)  # B does not delete A's lease
        self.assertEqual(len(self.gh.comments), 1)
        self.assertEqual(self.gh.labels, {7})

    def test_repo_label_is_created_once(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.assertTrue(self.a.acquire(8, KEY))
        self.assertTrue(self.gh.repo_label)

    def test_unrelated_comments_are_ignored(self) -> None:
        self.gh.comments[1] = "just a comment"
        self.gh.comments[2] = fc.body_of("porting-x", "Z", 5000.0)
        self.assertTrue(self.a.acquire(7, KEY))


class RaceTests(unittest.TestCase):
    """Two machines racing for one issue: exactly one holds it afterwards, whoever reads or posts first."""

    def setUp(self) -> None:
        self.gh = FakeGithub()
        self.clock = Clock()
        self.a = self.gh.machine("A", self.clock)
        self.b = self.gh.machine("B", self.clock)

    def holders(self) -> list[str]:
        return [e["machine"] for e in self.a.entries(7, KEY) if e["valid"]]

    def test_b_claims_completely_before_a_posts(self) -> None:
        results: dict[str, bool] = {}
        self.gh.before = (POST_COMMENT, lambda: results.update(B=self.b.acquire(7, KEY)))
        results["A"] = self.a.acquire(7, KEY)
        self.assertEqual(results, {"B": True, "A": False})
        self.assertEqual(self.holders(), ["B"])
        self.assertEqual(self.gh.labels, {7})

    def test_b_posts_between_a_reads_and_a_posts(self) -> None:
        def post_b() -> None:
            self.gh.comments[50] = fc.body_of(KEY, "B", self.clock() + 100)
        self.gh.before = (POST_COMMENT, post_b)
        self.assertFalse(self.a.acquire(7, KEY))
        self.assertEqual(self.holders(), ["B"])

    def test_a_posts_first_and_wins_the_re_read(self) -> None:
        self.assertTrue(self.a.acquire(7, KEY))
        self.assertFalse(self.b.acquire(7, KEY))
        self.assertEqual(self.holders(), ["A"])

    def race_once(self) -> tuple[list[bool], FakeGithub, fc.Claims]:
        gh = FakeGithub()
        clock = Clock()
        machines = [gh.machine("A", clock), gh.machine("B", clock)]
        barrier = threading.Barrier(2)
        won = [False, False]

        def go(i: int) -> None:
            barrier.wait()
            won[i] = machines[i].acquire(7, KEY)

        threads = [threading.Thread(target=go, args=(i,)) for i in range(2)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        return won, gh, machines[0]

    def test_threads_racing_leave_exactly_one_holder(self) -> None:
        for round_number in range(60):
            won, gh, machine = self.race_once()
            self.assertEqual(sum(won), 1, f"round {round_number}: {won}")
            self.assertEqual(len([e for e in machine.entries(7, KEY) if e["valid"]]), 1)
            self.assertEqual(gh.labels, {7})


class GhApiTests(unittest.TestCase):
    def test_failures_raise(self) -> None:
        gh = FakeGithub()
        api = fc.GhApi(REPO, gh.runner)
        gh.fail = "user"
        with self.assertRaises(fc.ApiError):
            api.me()
        gh.fail = "DELETE"
        with self.assertRaises(fc.ApiError):
            api.delete_comment(1)
        gh.fail = "PATCH"
        with self.assertRaises(fc.ApiError):
            api.edit_comment(1, "x")

    def test_gone_is_fine_but_a_failed_add_is_not(self) -> None:
        gh = FakeGithub()
        api = fc.GhApi(REPO, gh.runner)
        api.delete_comment(999)
        api.remove_label(7)
        gh.fail = "POST"
        with self.assertRaises(fc.ApiError):
            api.set_assignee(7, "owner", True)

    def test_edit_and_comment_listing(self) -> None:
        gh = FakeGithub()
        api = fc.GhApi(REPO, gh.runner)
        comment = api.add_comment(7, "one")
        api.edit_comment(comment, "two")
        self.assertEqual(api.comments(7), [{"id": comment, "body": "two"}])

    def test_default_runner_runs_a_process(self) -> None:
        code, output = fc.run([sys.executable, "-c", "import sys; print('out'); print('err', file=sys.stderr); sys.exit(3)"])
        self.assertEqual(code, 3)
        self.assertIn("out", output)
        self.assertIn("err", output)


class MainTests(unittest.TestCase):
    def setUp(self) -> None:
        self.gh = FakeGithub()
        self.api = fc.GhApi(REPO, self.gh.runner)
        self.clock = Clock()

    def main(self, *args: str) -> tuple[int, str]:
        out = io.StringIO()
        code = fc.main([args[0], REPO, KEY, "--issue", "7", "--machine", "A", *args[1:]],
                       api=self.api, clock=self.clock, out=out)
        return code, out.getvalue()

    def test_verbs(self) -> None:
        self.assertEqual(self.main("acquire", "--lease-seconds", "50")[0], 0)
        code, text = self.main("list")
        self.assertEqual((code, json.loads(text)[0]["machine"]), (0, "A"))
        self.assertEqual(self.main("renew")[0], 0)
        self.assertEqual(self.main("release")[0], 0)
        self.assertEqual(self.main("renew")[0], 1)
        self.assertEqual(json.loads(self.main("list")[1]), [])

    def test_held_by_another_exits_one(self) -> None:
        self.gh.machine("B", self.clock).acquire(7, KEY)
        self.assertEqual(self.main("acquire")[0], 1)

    def test_api_failure_exits_two(self) -> None:
        self.gh.fail = "comments"
        stderr = io.StringIO()
        with mock.patch.object(sys, "stderr", stderr):
            self.assertEqual(self.main("acquire")[0], 2)
        self.assertIn("factory_claims", stderr.getvalue())

    def test_issue_comes_from_the_environment_or_is_required(self) -> None:
        with mock.patch.dict(os.environ, {"NV_PORT_CLAIM_ISSUE": "7"}):
            importlib.reload(fc)
            self.assertEqual(fc.main(["list", REPO, KEY], api=self.api, clock=self.clock, out=io.StringIO()), 0)
        with mock.patch.dict(os.environ, {}, clear=True):
            importlib.reload(fc)
            with mock.patch.object(sys, "stderr", io.StringIO()), self.assertRaises(SystemExit):
                fc.main(["list", REPO, KEY], api=self.api, clock=self.clock)
        importlib.reload(fc)

    def test_default_api_is_the_gh_one(self) -> None:
        with mock.patch.object(fc, "GhApi", return_value=self.api) as made:
            out = io.StringIO()
            code = fc.main(["list", REPO, KEY, "--issue", "7"], clock=self.clock, out=out)
        self.assertEqual(code, 0)
        made.assert_called_once_with(REPO)


if __name__ == "__main__":
    unittest.main()
