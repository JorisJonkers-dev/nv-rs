# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the porting loop (scripts/fork/porting_loop.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
No cargo, no network, no git repository, no files outside a temporary directory.
The ledger's evidence header is read from the checkout to keep the column test
honest; nothing is written outside the temporary tree.
"""
import contextlib
import datetime
import hashlib
import io
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
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
    "# Evidence that cannot be read from a source marker, one row per function.",
    "address\tbackend\tfailure_class\tfamily_moved\ttranslation_deleted\tby_address_callers",
    "00401000\t0\treal\t0\t0\t1",
    "00401010\t0\tfloat-only\t0\t0\t0",
    "00402000\t1\t\t0\t0\t0",
    "",
])
PASSING = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"
FAILING = "test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out"
FAILING_CASES = "\n".join([
    "Running tests/typed_fade.rs (target/debug/deps/typed_fade-1)",
    "test fade::bad_case ... FAILED",
    "test fade::good_case ... ok",
    "error[E0308]: mismatched types",
    "panicked at crates/engine/src/typed.rs:12:5",
    "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out",
])

# A stub of agent-kit's `claim`: a directory is the cross-process lock. It keeps
# the lock until `release`, so a second loop can see that the family is held.
CLAIM_SCRIPT = '''#{python}
# SPDX-License-Identifier: GPL-3.0-only
import hashlib, os, shutil, sys, time

verb, repo, key = sys.argv[1:4]
issue = sys.argv[sys.argv.index("--issue") + 1] if "--issue" in sys.argv else "0"
locks = os.environ["NV_PORT_TEST_CLAIM_LOCKS"]
path = os.path.join(locks, hashlib.sha256((repo + "|" + key).encode()).hexdigest())
if verb == "acquire":
    try:
        os.mkdir(path)
    except FileExistsError:
        sys.exit(1)
    with open(os.path.join(path, "owner"), "w", encoding="utf-8") as handle:
        handle.write(issue)
    time.sleep(1.0)  # widen the window in which both loops ask for the same family
elif verb == "renew":
    sys.exit(0 if os.path.isdir(path) else 1)
elif verb == "release":
    shutil.rmtree(path, ignore_errors=True)
elif verb == "list":
    print("\\n".join(sorted(os.listdir(locks))))
else:
    sys.exit(2)
'''


def git(cwd, *args):
    """A local git command for the two-clones test; no remote service is used."""
    env = dict(os.environ)
    env.update({
        "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
        "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
        "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull,
    })
    done = subprocess.run(["git", *args], cwd=str(cwd), env=env, check=True,
                          text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    return done.stdout


class Recording:
    """A stand-in for every external command; records what the loop asked for."""

    def __init__(self, queue=QUEUE, lab=PASSING, lab_code=0, review=0, pr=0,
                 merge=0, queue_code=0, fail_worktree=None, after_queue=None):
        self.calls = []
        self.queue = queue
        self.lab = lab
        self.lab_code = lab_code
        self.review = review
        self.pr = pr
        self.merge = merge
        self.queue_code = queue_code
        self.fail_worktree = fail_worktree
        self.after_queue = after_queue

    def __call__(self, argv, cwd=None, env=None, log=None):
        argv = [str(a) for a in argv]
        self.calls.append((argv, Path(cwd) if cwd else None, dict(env or {})))
        joined = " ".join(argv)
        if "worktree add" in joined:
            if self.fail_worktree and self.fail_worktree in joined:
                return 1, "fatal: a branch with that name already exists\n"
            worktree = Path(argv[5])
            worktree.mkdir(parents=True, exist_ok=True)
            for folder in ("docs", "scripts"):
                source = Path(cwd) / folder
                if source.is_dir():
                    shutil.copytree(source, worktree / folder, dirs_exist_ok=True)
            return 0, "worktree ready"
        if "queue" in argv:
            if self.queue_code:
                return self.queue_code, "ledger: queue unavailable"
            if self.after_queue and "worktrees" in str(cwd):
                return 0, self.after_queue
            return 0, self.queue
        if "family_lab.py" in joined:
            return self.lab_code, self.lab
        if "review-tool" in joined:
            return self.review, "reviewed independently"
        if "pr-tool" in joined:
            return self.pr, ""
        if "close-tool" in joined:
            return 0, "closed unmerged"
        if argv[:2] == ["git", "merge"]:
            return self.merge, "merge output"
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


def joined(recorder):
    return [" ".join(call[0]) for call in recorder.calls]


class PortingLoop(unittest.TestCase):
    def test_queue_rows_come_only_from_the_ledger_command(self):
        rows = porting_loop.parse_queue(QUEUE)
        self.assertEqual([r["family"] for r in rows], ["fade", "fade", "lights", "crt"])
        self.assertEqual(rows[0]["address"], "00401000")
        self.assertEqual(rows[0]["state"], "open")
        self.assertEqual(rows[3]["done"], "yes")
        with self.assertRaises(ValueError):
            porting_loop.parse_queue("1\t00401000\topen\tno\tfade\n")

    def test_a_family_claim_is_exclusive_and_records_its_owner(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            held = porting_loop.claim(root, "fade", "worker-a")
            self.assertIsNotNone(held)
            fields = held.read_text(encoding="utf-8").strip().split("\t")
            self.assertEqual(fields[0], "worker-a")
            self.assertEqual(fields[2], str(os.getpid()))
            datetime.datetime.fromisoformat(fields[1])
            self.assertIsNone(porting_loop.claim(root, "fade", "worker-b"))
            self.assertIsNotNone(porting_loop.claim(root, "lights", "worker-b"))
            porting_loop.release(root, "fade")
            self.assertIsNotNone(porting_loop.claim(root, "fade", "worker-b"))

    def test_claims_are_shared_by_worktrees_of_one_clone(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "worktree"
            common = Path(tmp) / "gitdir"
            common.mkdir()
            (common / "commondir").write_text("../gitdir\n", encoding="utf-8")
            (root).mkdir()
            (root / ".git").write_text("gitdir: ../gitdir\n", encoding="utf-8")
            self.assertEqual(porting_loop.claim_directory(root).resolve(),
                             (common / "porting-claims").resolve())

    def test_a_stale_claim_is_reclaimed_and_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            directory = porting_loop.claim_directory(root)
            directory.mkdir(parents=True, exist_ok=True)
            (directory / "fade.claim").write_text(
                "earlier\t2020-01-01T00:00:00+00:00\t4242\n", encoding="utf-8"
            )
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="stale-run", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 0)
            record = porting_loop.batch_record(root, "stale-run")
            self.assertEqual(record["stale"], ["fade"])
            self.assertEqual(record["skipped"], [])
            self.assertTrue(any("fade" in c and "worktree add" in c for c in joined(recorder)))

    def test_pass_counts_and_failure_classes_are_measured(self):
        self.assertEqual(porting_loop.diff_counts(PASSING), {"passed": 3, "failed": 0})
        self.assertEqual(porting_loop.diff_counts(FAILING), {"passed": 1, "failed": 2})
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            self.assertEqual(
                porting_loop.failure_classes(root, {"00401000", "00401010", "00402000"}),
                {"real": 1, "float-only": 1},
            )

    def test_the_evidence_column_is_the_ledgers_failure_class(self):
        lines = (REPO / "scripts" / "ledger" / "function_evidence.tsv").read_text(
            encoding="utf-8"
        ).splitlines()
        header = next(line.split("\t") for line in lines if line.strip() and not line.startswith("#"))
        self.assertEqual(header[:3], ["address", "backend", "failure_class"])
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            evidence = root / "scripts" / "ledger" / "function_evidence.tsv"
            text = evidence.read_text(encoding="utf-8")
            swapped = []
            for line in text.splitlines():
                fields = line.split("\t")
                if len(fields) == 6 and not line.startswith("#"):
                    fields[1], fields[2] = fields[2], fields[1]
                swapped.append("\t".join(fields))
            evidence.write_text("\n".join(swapped) + "\n", encoding="utf-8")
            self.assertEqual(
                porting_loop.failure_classes(root, {"00401000", "00401010"}),
                {"real": 1, "float-only": 1},
            )

    def test_the_pr_body_lists_pass_counts_failure_classes_and_skips(self):
        report = {
            "families": [
                {"family": "fade", "passed": 3, "failed": 0, "review": "accepted"},
                {"family": "lights", "passed": 4, "failed": 1, "review": "accepted"},
            ],
            "classes": {"real": 1, "float-only": 2},
            "skipped": ["crt"],
            "stale": ["crt"],
        }
        body = porting_loop.pr_body("batch-1", report)
        self.assertIn("7 passed", body)
        self.assertIn("1 failed", body)
        self.assertIn("real: 1", body)
        self.assertIn("float-only: 2", body)
        self.assertIn("fade", body)
        self.assertIn("lights", body)
        self.assertIn("## Skipped families", body)
        self.assertIn("crt", body)

    def test_rate_lines_show_the_measured_seconds_and_hourly_rate(self):
        lines = porting_loop.rate_lines({
            "model_runs": 5, "build_seconds": 120.4, "diff_seconds": 30.5,
            "wall_seconds": 7200.0, "delivered": 6,
        })
        self.assertEqual(lines, [
            "", "## Rate", "",
            "- Model runs: 5",
            "- Build seconds: 120.4",
            "- Diff seconds: 30.5",
            "- Wall seconds: 7200.0",
            "- Verified functions per hour: 3.00",
        ])

    def test_rate_lines_do_not_divide_by_a_zero_wall_time(self):
        lines = porting_loop.rate_lines({
            "model_runs": 2, "build_seconds": 1.0, "diff_seconds": 2.0,
            "wall_seconds": 0.0, "delivered": 6,
        })
        self.assertEqual(lines[:3], ["", "## Rate", ""])
        self.assertIn("- Verified functions per hour: 0.00", lines)

    def test_the_pr_body_adds_a_rate_section_only_when_the_report_has_one(self):
        report = {
            "families": [
                {"family": "fade", "passed": 3, "failed": 1, "review": "accepted"},
            ],
            "classes": {"real": 2},
            "rejected": ["crt"],
            "skipped": ["lights"],
            "stale": ["crt"],
        }
        without = porting_loop.pr_body("batch-1", report)
        self.assertEqual(without, "\n".join([
            "# Porting batch batch-1", "", "## Pass counts", "",
            "- Diff cases: 3 passed; 1 failed",
            "- fade: 3 passed, 1 failed (accepted)",
            "", "## Failure classes", "", "- real: 2",
            "", "## Rejected families", "", "- crt",
            "", "## Skipped families", "", "- lights",
            "", "- Claims reclaimed after expiry: crt",
            "", "The checker hash and the review result are recorded in the batch report.", "",
        ]))
        rate = {
            "model_runs": 4, "build_seconds": 10.0, "diff_seconds": 20.0,
            "wall_seconds": 7200.0, "delivered": 6,
        }
        with_rate = porting_loop.pr_body("batch-1", dict(report, rate=rate))
        self.assertEqual(with_rate, without.replace(
            "\n\n## Rejected families",
            "\n" + "\n".join(porting_loop.rate_lines(rate)) + "\n\n## Rejected families",
            1,
        ))

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
            commands = joined(recorder)
            ledger = [c for c in commands if "ledger/Cargo.toml" in c]
            self.assertEqual(len(ledger), 3)  # the batch's queue, then one per accepted family's worktree
            self.assertIn("build_daemon.py", ledger[0])
            self.assertIn("queue", ledger[0])
            adds = [c for c in commands if "worktree add" in c and ("fade" in c or "lights" in c)]
            self.assertEqual(len(adds), 2, commands)
            self.assertIn("fade", adds[0])
            self.assertIn("lights", adds[1])
            self.assertTrue(all(c.endswith("fork") for c in adds))
            workers = [c for c in commands if c.startswith("worker-tool")]
            self.assertEqual(len(workers), 2)
            self.assertIn("pr-tool", " ".join(commands))
            self.assertTrue(any("merge" in c for c in commands))
            self.assertEqual(len([c for c in commands if "family_lab.py" in c]), 2)
            envs = [c[2] for c in recorder.calls if c[0] and c[0][0] == "worker-tool"]
            self.assertEqual({e["NV_PORT_FAMILY"] for e in envs}, {"fade", "lights"})
            pushes = [c for c in commands if c.startswith("git push")]
            self.assertEqual(len(pushes), 1)
            self.assertEqual(pushes[0].split()[-1], porting_loop.batch_record(root, "batch-1")["branch"])

    def test_a_new_run_gets_new_branch_names_even_with_the_same_name(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            branches = []
            for _ in range(2):
                recorder = Recording()
                with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                    self.assertEqual(porting_loop.batch(
                        root=root, count=3, name="daily", env=environment(root),
                        today=datetime.date(2026, 11, 26), call=recorder,
                    ), 0)
                branches.append(porting_loop.batch_record(root, "daily")["branch"])
            self.assertNotEqual(*branches)

    def test_a_batch_takes_whole_families_from_the_queue(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                porting_loop.batch(root=root, count=1, name="one-row", env=environment(root),
                                   today=datetime.date(2026, 11, 26), call=recorder)
            record = porting_loop.batch_record(root, "one-row")
            self.assertEqual(record["functions"], 2)
            self.assertEqual([f["family"] for f in record["families"]], ["fade"])

    def test_chunks_keep_order_and_size(self):
        rows = list(range(7))
        self.assertEqual(porting_loop.chunk_family(rows, 3), [[0, 1, 2], [3, 4, 5], [6]])
        self.assertEqual(porting_loop.chunk_family(rows, 0), [rows])
        self.assertEqual(porting_loop.chunk_family(rows, 3, 2), [[0, 1, 2], [3, 4, 5]])
        self.assertEqual(porting_loop.chunk_limit({"NV_PORT_CHUNKS": "2"}), 2)
        with self.assertRaises(ValueError):
            porting_loop.chunk_limit({"NV_PORT_CHUNKS": "x"})
        self.assertEqual(porting_loop.chunk_size({}), 5)
        self.assertEqual(porting_loop.chunk_size({"NV_PORT_CHUNK": "2"}), 2)
        with self.assertRaises(ValueError):
            porting_loop.chunk_size({"NV_PORT_CHUNK": "x"})
        with self.assertRaises(ValueError):
            porting_loop.chunk_size({"NV_PORT_CHUNK": "-1"})

    def test_a_family_runs_chunk_by_chunk_in_one_worktree(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(root=root, count=3, name="chunks",
                                          env={**environment(root), "NV_PORT_CHUNK": "1"},
                                          today=datetime.date(2026, 11, 26), call=recorder)
            self.assertEqual(code, 0)
            workers = [c for c in recorder.calls if c[0][:1] == ["worker-tool"] and c[2].get("NV_PORT_FAMILY") == "fade"]
            self.assertEqual(len(workers), 2)
            self.assertEqual({c[2]["NV_PORT_ADDRESSES"] for c in workers}, {"00401000", "00401010"})
            self.assertEqual(len({c[1] for c in workers}), 1)
            record = porting_loop.batch_record(root, "chunks")
            fade = [f for f in record["families"] if f["family"] == "fade"][0]
            self.assertTrue(fade["passed"] >= 2)

    def test_excluded_families_are_never_taken(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                porting_loop.batch(root=root, count=9, name="ex", env={**environment(root), "NV_PORT_BASE": "origin/fork",
                                   "NV_PORT_EXCLUDE": "fa*"},
                                   today=datetime.date(2026, 11, 26), call=recorder)
            record = porting_loop.batch_record(root, "ex")
            self.assertNotIn("fade", [f["family"] for f in record["families"]])
            self.assertTrue(any("origin/fork" in " ".join(c[0]) for c in recorder.calls))

    def test_an_existing_family_claim_is_skipped_and_reported(self):
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
            commands = joined(recorder)
            self.assertFalse(any("fade" in c and "worktree add" in c for c in commands))
            self.assertTrue(any("lights" in c and "worktree add" in c for c in commands))
            record = porting_loop.batch_record(root, "batch-2")
            self.assertEqual(record["skipped"], ["fade"])
            body = (porting_loop.paths(root, "batch-2") / "batch-2.pr.md").read_text(encoding="utf-8")
            self.assertIn("## Skipped families", body)
            self.assertIn("fade", body)

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
            commands = joined(recorder)
            self.assertFalse(any("worktree add" in c for c in commands))
            self.assertFalse(any("pr-tool" in c for c in commands))

    def test_a_worker_cannot_change_the_checker_in_its_worktree(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            seen = []
            answers = iter([(True, "ok"), (False, "tampered")])

            def check(path=None):
                seen.append(Path(path))
                return next(answers)

            with mock.patch.object(porting_loop.private_integrity, "check", side_effect=check):
                code = porting_loop.batch(
                    root=root, count=3, name="batch-4", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 1)
            self.assertEqual(seen[0].resolve(), root.resolve())
            self.assertIn("fade", str(seen[1]))
            self.assertNotEqual(seen[1].resolve(), root.resolve())
            commands = joined(recorder)
            self.assertFalse(any("pr-tool" in c for c in commands))
            self.assertFalse(any("merge" in c for c in commands))
            record = porting_loop.batch_record(root, "batch-4")
            self.assertEqual(record["status"], "failed")

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
            commands = joined(recorder)
            self.assertFalse(any("merge" in c and "fade" in c for c in commands))
            self.assertFalse(any("merge" in c and "lights" in c for c in commands))
            record = porting_loop.batch_record(root, "batch-5")
            self.assertEqual(record["families"], [])
            self.assertEqual(len(record["rejected"]), 2)

    def test_a_failing_lab_run_is_rejected_with_its_exit_status(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording(lab=FAILING, lab_code=101)
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="failed-lab", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 0)
            record = porting_loop.batch_record(root, "failed-lab")
            self.assertEqual(record["families"], [])
            self.assertTrue(all("Diff exited 101" in reason for reason in record["rejected"]))

    def test_a_failed_worktree_add_reports_the_error_and_keeps_accepted_branches(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording(fail_worktree="lights")
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="bad-add", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 1)
            commands = joined(recorder)
            self.assertFalse(any("branch -D" in c for c in commands))
            record = porting_loop.batch_record(root, "bad-add")
            self.assertEqual(record["status"], "failed")
            self.assertEqual([f["family"] for f in record["families"]], ["fade"])

    def test_a_late_merge_failure_keeps_the_earlier_branches(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording(merge=1)
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="bad-merge", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 1)
            commands = joined(recorder)
            self.assertFalse(any("branch -D" in c for c in commands))
            self.assertFalse(any("pr-tool" in c for c in commands))
            record = porting_loop.batch_record(root, "bad-merge")
            self.assertEqual(record["status"], "failed")
            self.assertEqual(len(record["families"]), 2)

    def test_the_shakedown_closes_unmerged_with_a_committed_rulebook_update(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="shakedown", shakedown=True,
                    env=environment(root), today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 0)
            commands = joined(recorder)
            self.assertFalse(any("pr-tool" in c for c in commands))
            self.assertFalse(any(" merge " in c for c in commands))
            self.assertTrue(any("close-tool" in c and "closed unmerged" in c for c in commands))
            self.assertTrue(any("git add" in c and "PORTING-RULEBOOK.md" in c for c in commands))
            self.assertTrue(any("git commit" in c and "shakedown" in c for c in commands))
            updated = (porting_loop.paths(root, "shakedown") / "rulebook" / "docs" / "fork"
                       / "PORTING-RULEBOOK.md").read_text(encoding="utf-8")
            self.assertIn("shakedown", updated.lower())
            self.assertIn("closed unmerged", updated)
            self.assertIn("triage real", updated)
            self.assertIn("triage float-only", updated)
            unchanged = (root / "docs" / "fork" / "PORTING-RULEBOOK.md").read_text(encoding="utf-8")
            self.assertIn("_No shakedown has been recorded yet._", unchanged)
            record = porting_loop.batch_record(root, "shakedown")
            self.assertEqual(record["status"], "closed unmerged")
            self.assertEqual(record["functions"], 3)

    def test_a_shakedown_without_a_close_command_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            env = environment(root)
            env.pop("NV_PORT_CLOSE")
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="shakedown", shakedown=True, env=env,
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 2)
            self.assertEqual(joined(recorder), [])
            self.assertIn("_No shakedown has been recorded yet._",
                          (root / "docs" / "fork" / "PORTING-RULEBOOK.md").read_text(encoding="utf-8"))

    def test_a_batch_records_only_what_an_accepted_family_turned_verified(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            # 00402000 was verified before the batch; 00401000 becomes verified in fade's worktree.
            before = QUEUE.replace("3\t00402000\topen", "3\t00402000\treference-verified")
            after = before.replace("1\t00401000\topen", "1\t00401000\treference-verified")
            recorder = Recording(queue=before, after_queue=after)
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="delivery-run", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=recorder,
                )
            self.assertEqual(code, 0)
            records = porting_loop.deliveries(root)
            self.assertEqual([r["address"] for r in records], ["00401000"])
            self.assertEqual(records[0]["date"], "2026-11-26")
            self.assertTrue(any("git add" in c and "delivered.jsonl" in c for c in joined(recorder)))

    def test_a_rejected_family_delivers_nothing_even_if_its_worktree_verifies(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            after = QUEUE.replace("1\t00401000\topen", "1\t00401000\treference-verified")
            recorder = Recording(after_queue=after, review=1)
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                porting_loop.batch(root=root, count=3, name="rej", env=environment(root),
                                   today=datetime.date(2026, 11, 26), call=recorder)
            self.assertEqual(porting_loop.deliveries(root), [])

    def test_claims_in_a_shared_directory_exclude_separate_clones(self):
        with tempfile.TemporaryDirectory() as one, tempfile.TemporaryDirectory() as two, \
                tempfile.TemporaryDirectory() as shared:
            self.assertIsNotNone(porting_loop.claim(one, "fade", "a"))
            # without a shared directory two clones both win
            self.assertIsNotNone(porting_loop.claim(two, "fade", "b"))
            self.assertIsNotNone(porting_loop.claim(one, "crt", "a", directory=shared))
            self.assertIsNone(porting_loop.claim(two, "crt", "b", directory=shared))
            root = tree(one)
            env = environment(root)
            env["NV_PORT_CLAIM_DIR"] = shared
            porting_loop.claim(two, "fade", "other", directory=shared)
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                porting_loop.batch(root=root, count=3, name="shared", env=env,
                                   today=datetime.date(2026, 11, 26), call=Recording())
            self.assertEqual(porting_loop.batch_record(root, "shared")["skipped"], ["fade"])

    def test_a_stale_claim_is_not_reclaimed_while_another_batch_reclaims_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            old = datetime.datetime(2026, 11, 1, tzinfo=datetime.timezone.utc)
            held = porting_loop.claim(root, "fade", "dead", now=old)
            guard = held.with_name(held.name + ".reclaim")
            guard.write_text("", encoding="utf-8")
            self.assertIsNone(porting_loop.claim(root, "fade", "b", max_age=60))
            guard.unlink()
            self.assertIsNotNone(porting_loop.claim(root, "fade", "b", max_age=60))
            self.assertFalse(guard.exists())

    def test_the_checker_is_verified_before_the_diff_run_and_the_review(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            order = []

            def check(path=None):
                order.append("check")
                return (True, "ok") if len(order) == 1 else (False, "tampered")

            with mock.patch.object(porting_loop.private_integrity, "check", side_effect=check):
                code = porting_loop.batch(root=root, count=3, name="early", env=environment(root),
                                          today=datetime.date(2026, 11, 26), call=recorder)
            self.assertEqual(code, 1)
            commands = joined(recorder)
            self.assertFalse(any("family_lab.py" in c for c in commands))
            self.assertFalse(any("review-tool" in c for c in commands))

    def test_the_shakedown_rulebook_branch_is_named_passed_to_close_and_pushed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")), \
                    contextlib.redirect_stdout(io.StringIO()) as out:
                code = porting_loop.batch(root=root, count=3, name="shakedown", shakedown=True,
                                          env=environment(root), today=datetime.date(2026, 11, 26),
                                          call=recorder)
            self.assertEqual(code, 0)
            branch = porting_loop.batch_record(root, "shakedown")["rulebook_branch"]
            self.assertTrue(branch.startswith("porting/shakedown/"))
            self.assertIn(branch, out.getvalue())
            commands = joined(recorder)
            self.assertTrue(any(c.startswith("git push") and c.endswith(branch) for c in commands))
            close = [c for c in commands if "close-tool" in c]
            self.assertTrue(close and f"--branch {branch}" in close[0])

    def test_diff_counts_use_only_the_family_binary(self):
        text = "\n".join([
            "Running tests/typed_fade.rs (target/debug/deps/typed_fade-1)",
            "test result: ok. 3 passed; 0 failed; 0 ignored",
            "Running tests/typed_other.rs (target/debug/deps/typed_other-2)",
            "test result: FAILED. 1 passed; 4 failed; 0 ignored",
            "Doc-tests x",
            "test result: ok. 9 passed; 0 failed; 0 ignored",
        ])
        self.assertEqual(porting_loop.diff_counts(text, "fade"), {"passed": 3, "failed": 0})
        self.assertEqual(porting_loop.diff_counts(PASSING, "fade"), {"passed": 3, "failed": 0})

    def test_the_pr_body_classes_cover_accepted_addresses_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording(review=0)
            with mock.patch.object(porting_loop.private_integrity, "check", return_value=(True, "ok")):
                # fade (real, float-only) is rejected by its lab run; lights is accepted
                def call(argv, cwd=None, env=None, log=None):
                    if "family_lab.py" in " ".join(map(str, argv)) and "fade" in str(cwd):
                        return 101, FAILING
                    return recorder(argv, cwd, env, log)
                porting_loop.batch(root=root, count=3, name="acc", env=environment(root),
                                   today=datetime.date(2026, 11, 26), call=call)
            self.assertEqual(porting_loop.batch_record(root, "acc")["classes"], {})

    def test_family_names_that_sanitise_alike_get_distinct_slugs(self):
        self.assertNotEqual(porting_loop.slug("a/b"), porting_loop.slug("a_b"))
        self.assertEqual(porting_loop.slug("fade"), "fade")

    def test_a_missing_failure_class_column_is_reported_not_guessed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            (root / "scripts" / "ledger" / "function_evidence.tsv").write_text(
                "address\tbackend\tkind\n00401000\t0\treal\n", encoding="utf-8")
            errors = io.StringIO()
            with contextlib.redirect_stderr(errors):
                self.assertEqual(porting_loop.failure_classes(root, {"00401000"}), {})
            self.assertIn("failure_class", errors.getvalue())

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
            self.assertIn("- Window: 2026-11-20 to 2026-11-26", text)
            self.assertIn("- Delivery records without a queue row: 1", text)
            self.assertTrue((root / porting_loop.DELIVERIES).is_file())

    def test_weekly_counts_a_newly_verified_function_on_the_day_it_is_seen(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            queue = "\n".join([
                "# rank\taddress\tstate\tdone\tfamily\tname",
                "1\t00401000\treference-verified\tyes\tfade\ta",
                "",
            ])
            out = root / "weekly.md"
            porting_loop.weekly(root=root, days=7, env=environment(root),
                                today=datetime.date(2026, 11, 26), out=out,
                                call=Recording(queue=queue))
            self.assertIn("2026-11-26 | 1", out.read_text(encoding="utf-8"))

    def test_weekly_reports_delivered_verified_functions_per_hour(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            addresses = [f"{0x00401000 + i * 0x10:08X}" for i in range(14)]
            queue = ["# rank\taddress\tstate\tdone\tfamily\tname"]
            queue += [f"{i}\t{address}\treference-verified\tyes\tfade\tf{i}"
                      for i, address in enumerate(addresses, 1)]
            queue += [
                "15\t00403000\ttyped\tyes\tcrt\tc", "",
            ]
            for address in addresses:
                porting_loop.record_delivery(
                    root, "2026-11-25", "batch-1", address, "fade")
            out = root / "weekly.md"
            porting_loop.weekly(
                root=root, days=7, env=environment(root),
                today=datetime.date(2026, 11, 26), out=out,
                call=Recording(queue="\n".join(queue)),
            )
            text = out.read_text(encoding="utf-8")
            self.assertIn("2026-11-25 | 14", text)
            self.assertIn("- Total: 14", text)
            self.assertIn("- Verified functions per hour: 0.08", text)
            self.assertLess(text.index("- Total: 14"),
                            text.index("- Verified functions per hour: 0.08"))

    def test_weekly_reports_a_queue_failure_without_a_traceback(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            errors = io.StringIO()
            with contextlib.redirect_stderr(errors):
                code = porting_loop.weekly(
                    root=root, days=7, env=environment(root),
                    today=datetime.date(2026, 11, 26), call=Recording(queue_code=2),
                )
            self.assertEqual(code, 1)
            self.assertIn("ledger queue exited 2", errors.getvalue())

    def test_two_clones_with_a_stub_claim_command_never_take_the_same_family(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            seed = tree(root / "seed")
            git(seed, "init", "-q")
            git(seed, "add", "-A")
            git(seed, "commit", "-qm", "chore: base")
            bare = root / "remote.git"
            git(root, "init", "--bare", "-q", str(bare))
            git(seed, "push", "-q", str(bare), "HEAD:fork")
            git(bare, "symbolic-ref", "HEAD", "refs/heads/fork")
            one, two = root / "one", root / "two"
            git(root, "clone", "-q", str(bare), str(one))
            git(root, "clone", "-q", str(bare), str(two))
            locks = root / "locks"
            locks.mkdir()
            script = root / "claim.py"
            script.write_text(CLAIM_SCRIPT.format(python=sys.executable), encoding="utf-8")
            script.chmod(0o755)
            command = shlex.quote(sys.executable) + " " + shlex.quote(str(script))
            shared = {
                "NV_PORT_TEST_CLAIM_LOCKS": str(locks),
                "NV_PORT_CLAIM_REPO": f"owner/remote-{root.name}",
                "NV_PORT_CLAIM_ISSUE": "1027",
                "NV_PORT_CLAIM_CMD": command,
            }
            envs = {}
            for name, clone in (("one", one), ("two", two)):
                envs[name] = dict(environment(clone), **shared)
            recorders = {"one": Recording(), "two": Recording()}
            barrier = threading.Barrier(2)
            results = {}

            def synced(name):
                first = [False]

                def runner(argv, cwd=None, env=None, log=None):
                    joined = " ".join(map(str, argv))
                    if not first[0] and "ledger/Cargo.toml" in joined and "queue" in joined:
                        first[0] = True
                        barrier.wait(timeout=20)
                    return recorders[name](argv, cwd, env, log)

                return runner

            def run(name, clone):
                results[name] = porting_loop.batch(
                    root=clone, count=3, name=f"remote-{name}", env=envs[name],
                    today=datetime.date(2026, 11, 26), call=synced(name),
                )

            with mock.patch.object(porting_loop.private_integrity, "check",
                                   return_value=(True, "ok")):
                threads = [threading.Thread(target=run, args=(name, clone))
                           for name, clone in (("one", one), ("two", two))]
                for thread in threads:
                    thread.start()
                for thread in threads:
                    thread.join(timeout=60)
            self.assertTrue(all(not thread.is_alive() for thread in threads))
            self.assertEqual(results, {"one": 0, "two": 0})
            taken = {}
            for name in ("one", "two"):
                taken[name] = set()
                for argv, _, _ in recorders[name].calls:
                    if "worktree add" in " ".join(argv):
                        taken[name].update(f for f in ("fade", "lights") if f in " ".join(argv))
            self.assertTrue(taken["one"].isdisjoint(taken["two"]), taken)
            self.assertEqual(taken["one"] | taken["two"], {"fade", "lights"})
            loser = "one" if len(taken["one"]) < len(taken["two"]) else "two"
            winner = "two" if loser == "one" else "one"
            record = porting_loop.batch_record(envs[loser]["NV_PORT_ROOT"],
                                               f"remote-{loser}")
            self.assertEqual(set(record["skipped"]), taken[winner])
            self.assertEqual(locks.exists(), True)
            self.assertEqual(list(locks.iterdir()), [])  # both batches released their claims

    def test_a_failing_diff_gives_the_next_attempt_its_failure_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            seen_files, seen_attempts = [], []

            def runner(argv, cwd=None, env=None, log=None):
                joined = " ".join(map(str, argv))
                if "family_lab.py" in joined and "fade" in str(cwd):
                    if env.get("NV_PORT_ATTEMPT") == "1":
                        return 0, FAILING_CASES
                    return 0, PASSING
                if joined.startswith("worker-tool"):
                    seen_attempts.append(env.get("NV_PORT_ATTEMPT"))
                    path = env.get("NV_PORT_FAILURE_FILE")
                    if path and Path(path).is_file():
                        seen_files.append(Path(path).read_text(encoding="utf-8"))
                return recorder(argv, cwd, env, log)

            with mock.patch.object(porting_loop.private_integrity, "check",
                                   return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="retry", env=environment(root),
                    today=datetime.date(2026, 11, 26), call=runner,
                )
            self.assertEqual(code, 0)
            record = porting_loop.batch_record(root, "retry")
            fade = next(f for f in record["families"] if f["family"] == "fade")
            self.assertEqual(fade["attempts"], 2)
            self.assertEqual(record["attempt_counts"]["fade"], 2)
            self.assertEqual([a["failure_class"] for a in record["attempts"]
                              if a["family"] == "fade"], ["diff", "none"])
            self.assertEqual(seen_attempts.count("2"), 1)
            self.assertEqual(len(seen_files), 1)
            self.assertIn("fade::bad_case", seen_files[0])
            self.assertIn("error[E0308]", seen_files[0])
            self.assertIn("panicked at", seen_files[0])
            worker_envs = [c[2] for c in recorder.calls if c[0] and c[0][0] == "worker-tool"]
            self.assertNotIn("NV_PORT_FAILURE_FILE", worker_envs[0])
            commands = joined(recorder)
            self.assertTrue(any(c.startswith("git reset --hard") for c in commands))
            self.assertIn("git clean -fd", commands)
            body = (porting_loop.paths(root, "retry") / "retry.pr.md").read_text(encoding="utf-8")
            self.assertIn("## Attempts", body)
            self.assertIn("fade: 2 attempts", body)
            self.assertIn("diff", body)

    def test_a_worker_exit_is_retried_and_the_last_class_rejects_the_family(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            recorder = Recording()
            calls = []

            def runner(argv, cwd=None, env=None, log=None):
                joined = " ".join(map(str, argv))
                if joined.startswith("worker-tool") and "fade" in env.get("NV_PORT_FAMILY", ""):
                    calls.append(env.get("NV_PORT_ATTEMPT"))
                    return 7, "worker failed to compile\n"
                return recorder(argv, cwd, env, log)

            with mock.patch.object(porting_loop.private_integrity, "check",
                                   return_value=(True, "ok")):
                code = porting_loop.batch(
                    root=root, count=3, name="failed-worker",
                    env={**environment(root), "NV_PORT_ATTEMPTS": "2"},
                    today=datetime.date(2026, 11, 26), call=runner,
                )
            self.assertEqual(code, 0)
            record = porting_loop.batch_record(root, "failed-worker")
            self.assertEqual(record["attempt_counts"]["fade"], 2)
            self.assertEqual(calls, ["1", "2"])
            self.assertTrue(any("fade: worker exited 7" in reason
                                for reason in record["rejected"]))
            self.assertEqual([a["failure_class"] for a in record["attempts"]
                              if a["family"] == "fade"], ["worker", "worker"])
            failure = (porting_loop.paths(root, "failed-worker") / "fade.failure.txt").read_text(
                encoding="utf-8")
            self.assertIn("worker failed to compile", failure)

    def test_local_claims_still_work_without_a_claim_command(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(tmp)
            env = environment(root)
            for key in ("NV_PORT_CLAIM_CMD", "NV_PORT_CLAIM_REPO", "NV_PORT_CLAIM_ISSUE"):
                env.pop(key, None)
            self.assertIsNone(porting_loop.claim_config(env))
            held = porting_loop.claim(root, "fade", "another-worker")
            self.assertIsInstance(held, Path)
            recorder = Recording()
            with mock.patch.object(porting_loop.private_integrity, "check",
                                   return_value=(True, "ok")):
                porting_loop.batch(root=root, count=3, name="local", env=env,
                                   today=datetime.date(2026, 11, 26), call=recorder)
            record = porting_loop.batch_record(root, "local")
            self.assertEqual(record["skipped"], ["fade"])
            self.assertEqual([f["family"] for f in record["families"]], ["lights"])
            directory = porting_loop.claim_directory(root)
            self.assertTrue((directory / "fade.claim").is_file())
            self.assertFalse((directory / "lights.claim").exists())  # released at batch end
            porting_loop.release(root, "fade")

    def test_a_claim_command_is_found_on_path_without_an_explicit_command(self):
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = Path(tmp) / "bin"
            bin_dir.mkdir()
            script = bin_dir / "claim"
            script.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            script.chmod(0o755)
            env = {"PATH": str(bin_dir) + os.pathsep + os.environ.get("PATH", ""),
                   "NV_PORT_CLAIM_REPO": "owner/name", "NV_PORT_CLAIM_ISSUE": "42"}
            config = porting_loop.claim_config(env)
            self.assertEqual(Path(config["argv"][0]), script)
            self.assertEqual(config["issue"], "42")
            self.assertEqual(porting_loop.claim_config({**env, "NV_PORT_CLAIM_REPO": ""}), None)

    def test_the_pr_body_shows_attempts_and_their_failure_classes(self):
        report = {
            "families": [{"family": "fade", "passed": 3, "failed": 0,
                          "review": "accepted", "attempts": 2}],
            "classes": {},
            "attempts": [
                {"family": "fade", "attempt": 1, "failure_class": "diff",
                 "detail": "1 failing cases"},
                {"family": "fade", "attempt": 2, "failure_class": "none",
                 "detail": "passed"},
            ],
        }
        body = porting_loop.pr_body("batch", report)
        self.assertIn("## Attempts", body)
        self.assertIn("fade: 2 attempts", body)
        self.assertIn("attempt 1: diff", body)
        self.assertIn("attempt 2: none", body)


if __name__ == "__main__":
    unittest.main()
