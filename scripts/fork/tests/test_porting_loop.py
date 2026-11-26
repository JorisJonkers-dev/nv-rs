# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the porting loop (scripts/fork/porting_loop.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
No cargo, no network, no git repository, no files outside a temporary directory.
"""
import datetime
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
FORK = HERE.parent
REPO = FORK.parent.parent
sys.path.insert(0, str(FORK))

import porting_loop  # noqa: E402

QUEUE = "\n".join([
    "# rank\taddress\tstate\tdone\tfamily\tname",
    "1\t00401000\topen\tno\tfade\tfirst",
    "2\t00401010\topen\tno\tfade\tsecond",
    "3\t00402000\topen\tno\tlights\tthird",
    "4\t00403000\ttyped\tyes\tcrt\tdone_already",
    "",
])
EVIDENCE = "\n".join([
    "# address\tbackend\tfailure_class\tfamily_moved\ttranslation_deleted\tby_address_callers",
    "00401000\t0\treal\t0\t0\t1",
    "00401010\t0\tfloat-only\t0\t0\t0",
    "00402000\t1\t\t0\t0\t0",
    "",
])
PASSING = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"


class Recording:
    """A stand-in for every external command; records what the loop asked for."""

    def __init__(self, queue=QUEUE, lab=PASSING, review=0, pr=0):
        self.calls = []
        self.queue = queue
        self.lab = lab
        self.review = review
        self.pr = pr

    def __call__(self, argv, cwd=None, env=None, log=None):
        argv = [str(a) for a in argv]
        self.calls.append((argv, Path(cwd) if cwd else None, dict(env or {})))
        joined = " ".join(argv)
        if "queue" in argv:
            return 0, self.queue
        if "family_lab.py" in joined:
            return 0, self.lab
        if "review-tool" in joined:
            return self.review, "reviewed independently"
        if "pr-tool" in joined:
            return self.pr, ""
        if log:
            log.parent.mkdir(parents=True, exist_ok=True)
            log.write_text("command output\n", encoding="utf-8")
        return 0, ""


def environment(tmp):
    return {
        "PATH": "/bin:/usr/bin",
        "NV_PORT_WORKER": "worker-tool --family",
        "NV_PORT_REVIEW": "review-tool --check",
        "NV_PORT_PR": "pr-tool --open",
        "NV_PORT_CLOSE": "close-tool --close",
        "NV_PORT_ROOT": str(tmp),
    }


def tree(tmp):
    root = Path(tmp)
    (root / "scripts" / "ledger").mkdir(parents=True)
    (root / "scripts" / "ledger" / "function_evidence.tsv").write_text(EVIDENCE, encoding="utf-8")
    (root / "docs" / "fork").mkdir(parents=True)
    (root / "docs" / "fork" / "PORTING-RULEBOOK.md").write_text(
        "<!-- SPDX-License-Identifier: GPL-3.0-only -->\n# Porting rulebook\n\n"
        "<!-- shakedown:start -->\n_No shakedown has been recorded yet._\n"
        "<!-- shakedown:end -->\n",
        encoding="utf-8",
    )
    return root


class PortingLoop(unittest.TestCase):
    def test_queue_rows_come_only_from_the_ledger_command(self):
        rows = porting_loop.parse_queue(QUEUE)
        self.assertEqual([r["family"] for r in rows], ["fade", "fade", "lights", "crt"])
        self.assertEqual(rows[0]["address"], "00401000")
        self.assertEqual(rows[0]["state"], "open")
        self.assertEqual(rows[3]["done"], "yes")
        with self.assertRaises(ValueError):
            porting_loop.parse_queue("1\t00401000\topen\tno\tfade\n")

    def test_a_family_claim_is_exclusive(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.assertIsNotNone(porting_loop.claim(root, "fade", "worker-a"))
            self.assertIsNone(porting_loop.claim(root, "fade", "worker-b"))
            self.assertIsNotNone(porting_loop.claim(root, "lights", "worker-b"))
            porting_loop.release(root, "fade")
            self.assertIsNotNone(porting_loop.claim(root, "fade", "worker-b"))

    def test_pass_counts_and_failure_classes_are_measured(self):
        self.assertEqual(porting_loop.diff_counts(PASSING), {"passed": 3, "failed": 0})
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            self.assertEqual(
                porting_loop.failure_classes(root, {"00401000", "00401010", "00402000"}),
                {"real": 1, "float-only": 1},
            )

    def test_the_pr_body_lists_pass_counts_and_failure_classes(self):
        report = {
            "families": [
                {"family": "fade", "passed": 3, "failed": 0, "review": "accepted"},
                {"family": "lights", "passed": 4, "failed": 1, "review": "accepted"},
            ],
            "classes": {"real": 1, "float-only": 2},
        }
        body = porting_loop.pr_body("batch-1", report)
        self.assertIn("7 passed", body)
        self.assertIn("1 failed", body)
        self.assertIn("real: 1", body)
        self.assertIn("float-only: 2", body)
        self.assertIn("fade", body)
        self.assertIn("lights", body)

    def test_a_batch_uses_the_daemon_a_worktree_per_family_and_two_configured_commands(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="batch-1", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 0)
            joined = [" ".join(c[0]) for c in recorder.calls]
            ledger = [c for c in joined if "ledger/Cargo.toml" in c]
            self.assertEqual(len(ledger), 1)
            self.assertIn("build_daemon.py", ledger[0])
            self.assertIn("queue", ledger[0])
            adds = [c for c in joined if "worktree add" in c and ("fade" in c or "lights" in c)]
            self.assertEqual(len(adds), 2, joined)
            self.assertIn("fade", adds[0])
            self.assertIn("lights", adds[1])
            workers = [c for c in joined if c.startswith("worker-tool")]
            self.assertEqual(len(workers), 2)
            self.assertIn("pr-tool", " ".join(joined))
            self.assertTrue(any("merge" in c for c in joined))
            # Each family's Diff cases ran through the lab, which owns the daemon.
            self.assertEqual(len([c for c in joined if "family_lab.py" in c]), 2)
            envs = [c[2] for c in recorder.calls if c[0] and c[0][0] == "worker-tool"]
            self.assertEqual({e["NV_PORT_FAMILY"] for e in envs}, {"fade", "lights"})

    def test_an_existing_family_claim_is_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            porting_loop.claim(root, "fade", "another-worker")
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="batch-2", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 0)
            joined = [" ".join(c[0]) for c in recorder.calls]
            self.assertFalse(any("fade" in c and "worktree add" in c for c in joined))
            self.assertTrue(any("lights" in c and "worktree add" in c for c in joined))

    def test_a_changed_checker_hash_refuses_the_batch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            with mock.patch.object(
                porting_loop.private_integrity, "check", return_value=(False, "checker hash changed")
            ):
                code = porting_loop.batch(
                    root=root, count=3, name="batch-3", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 1)
            joined = [" ".join(c[0]) for c in recorder.calls]
            self.assertFalse(any("worktree add" in c for c in joined))
            self.assertFalse(any("pr-tool" in c for c in joined))

    def test_a_worker_cannot_change_the_checker_in_its_worktree(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            with mock.patch.object(
                porting_loop.private_integrity, "check", side_effect=[(True, "ok"), (False, "tampered")]
            ):
                code = porting_loop.batch(
                    root=root, count=3, name="batch-4", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 1)
            joined = [" ".join(c[0]) for c in recorder.calls]
            self.assertFalse(any("pr-tool" in c for c in joined))
            self.assertFalse(any("merge" in c for c in joined))

    def test_a_failed_review_keeps_the_family_out_of_the_batch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording(review=1)
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="batch-5", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 0)
            joined = [" ".join(c[0]) for c in recorder.calls]
            self.assertFalse(any("merge" in c and "fade" in c for c in joined))
            self.assertFalse(any("merge" in c and "lights" in c for c in joined))

    def test_the_shakedown_closes_unmerged_with_a_rulebook_update(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="shakedown", shakedown=True,
                    env=environment(root), today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 0)
            joined = [" ".join(c[0]) for c in recorder.calls]
            self.assertFalse(any("pr-tool" in c for c in joined))
            self.assertFalse(any(" merge " in c for c in joined))
            self.assertTrue(any("close-tool" in c and "closed unmerged" in c for c in joined))
            rulebook = (root / "docs" / "fork" / "PORTING-RULEBOOK.md").read_text(encoding="utf-8")
            self.assertIn("shakedown", rulebook.lower())
            self.assertIn("closed unmerged", rulebook)
            self.assertIn("real", rulebook)
            self.assertIn("float-only", rulebook)
            record = porting_loop.batch_record(root, "shakedown")
            self.assertEqual(record["status"], "closed unmerged")
            self.assertEqual(record["functions"], 3)

    def test_the_weekly_report_counts_delivered_verified_functions_per_day(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            queue = "\n".join([
                "# rank\taddress\tstate\tdone\tfamily\tname",
                "1\t00401000\treference-verified\tyes\tfade\ta",
                "2\t00402000\treference-verified\tyes\tlights\tb",
                "3\t00403000\ttyped\tyes\tcrt\tc",
                "",
            ])
            porting_loop.record_delivery(root, "2026-11-25", "batch-1", "00401000", "fade")
            porting_loop.record_delivery(root, "2026-11-25", "batch-1", "00402000", "lights")
            porting_loop.record_delivery(root, "2026-11-25", "batch-1", "00403000", "crt")
            porting_loop.record_delivery(root, "2026-11-18", "batch-0", "00401000", "fade")
            out = root / "weekly.md"
            recorder = Recording(queue=queue)
            code = porting_loop.weekly(
                root=root, days=7, env=environment(root),
                today=datetime.date(2026, 11, 26), out=out, call=recorder,
            )
            self.assertEqual(code, 0)
            text = out.read_text(encoding="utf-8")
            self.assertIn("2026-11-25 | 2", text)
            self.assertNotIn("2026-11-18 |", text)
            self.assertIn("delivered verified functions", text)


if __name__ == "__main__":
    unittest.main()
