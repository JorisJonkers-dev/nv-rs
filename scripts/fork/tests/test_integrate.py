# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the deterministic integrator (scripts/fork/integrate.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
Real git on temporary repositories with a local bare remote; the pull request tool is a stand-in.
"""
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import integrate

ENV = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.com", GIT_COMMITTER_NAME="t",
           GIT_COMMITTER_EMAIL="t@example.com", GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull)


def git(cwd, *args):
    done = subprocess.run(["git", *args], cwd=str(cwd), env=ENV, check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    return done.stdout


def put(root, name, text):
    path = Path(root) / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


class Gh:
    """Real git, a stand-in for gh: it records the calls and answers `pr list` from `existing`."""

    def __init__(self, existing="", code=0):
        self.calls, self.existing, self.code = [], existing, code

    def __call__(self, argv, cwd):
        argv = [str(a) for a in argv]
        if argv[0] != "gh":
            done = subprocess.run(argv, cwd=str(cwd), env=ENV, text=True, stdout=subprocess.PIPE,
                                  stderr=subprocess.STDOUT, check=False)
            return done.returncode, done.stdout
        self.calls.append(argv)
        if argv[1:3] == ["pr", "list"]:
            return 0, self.existing
        return self.code, "https://example.invalid/pull/7\n" if not self.code else "boom\n"


class Repo(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        base = Path(self.tmp.name)
        self.remote, self.work = base / "remote.git", base / "work"
        git(base, "init", "-q", "--bare", "-b", "fork", str(self.remote))
        git(base, "clone", "-q", str(self.remote), str(self.work))
        git(self.work, "checkout", "-q", "-b", "fork")
        put(self.work, "base.txt", "base\n")
        git(self.work, "add", "-A")
        git(self.work, "commit", "-qm", "base")
        git(self.work, "push", "-q", "-u", "origin", "fork")
        self.task("feat/a", "a.txt", "a\n")
        self.task("feat/b", "b.txt", "b\n")

    def task(self, branch, name, text):
        git(self.work, "checkout", "-q", "-b", branch, "origin/fork")
        put(self.work, name, text)
        git(self.work, "add", "-A")
        git(self.work, "commit", "-qm", f"feat: {branch}")
        git(self.work, "checkout", "-q", "fork")

    def run_main(self, *args, runner=None):
        out = io.StringIO()
        code = integrate.main(["--name", "n1", *args], root=self.work, runner=runner or Gh(), out=out)
        return code, out.getvalue()

    def files(self, ref):
        return set(git(self.work, "ls-tree", "-r", "--name-only", ref).split())


class Parsing(unittest.TestCase):
    def test_a_task_is_a_branch_and_an_optional_issue(self):
        self.assertEqual(integrate.parse_task("feat/a:12"), ("feat/a", 12))
        self.assertEqual(integrate.parse_task("feat/a"), ("feat/a", None))

    def test_bad_branches_and_issues_are_refused(self):
        for bad in ("-x", "a b", "a:x", ":3", "a;b", "a\n", "a:\u0663"):
            with self.assertRaises(integrate.IntegrateError, msg=bad):
                integrate.parse_task(bad)

    def test_run_gives_127_for_a_missing_program(self):
        self.assertEqual(integrate.run([sys.executable, "-c", "print(3)"], ".")[0], 0)
        self.assertEqual(integrate.run(["no-such-program-here"], ".")[0], 127)

    def test_the_body_closes_every_accepted_issue_and_names_the_dropped(self):
        body = integrate.pr_body("n", [{"task": "a", "issue": 5}, {"task": "b", "issue": None}],
                                 [{"task": "c", "issue": 6, "reason": "gate"}])
        self.assertIn("Closes #5", body)
        self.assertNotIn("#6", body.split("## Back in the queue")[0])
        self.assertIn("- c: gate", body)
        self.assertIn("- a", body)
        empty = integrate.pr_body("n", [], [])
        self.assertIn("- none", empty)
        self.assertIn("No issue is closed.", empty)


class Stacking(Repo):
    def test_tasks_are_merged_in_order_onto_a_batch_branch(self):
        code, out = self.run_main("--task", "feat/a:1", "--task", "feat/b:2")
        self.assertEqual(code, 0)
        result = json.loads(out)
        self.assertEqual([a["task"] for a in result["accepted"]], ["feat/a", "feat/b"])
        self.assertEqual(result["batch"], "batch/n1")
        self.assertEqual(self.files("batch/n1"), {"base.txt", "a.txt", "b.txt"})

    def test_a_conflicting_task_is_dropped_and_the_rest_lands(self):
        self.task("feat/c", "a.txt", "other\n")
        code, out = self.run_main("--task", "feat/a:1", "--task", "feat/c:3", "--task", "feat/b:2")
        result = json.loads(out)
        self.assertEqual(code, 0)
        self.assertEqual([d["task"] for d in result["dropped"]], ["feat/c"])
        self.assertEqual(result["dropped"][0]["reason"], "conflict")
        self.assertEqual([a["task"] for a in result["accepted"]], ["feat/a", "feat/b"])
        self.assertEqual(self.files("batch/n1"), {"base.txt", "a.txt", "b.txt"})
        self.assertEqual(git(self.work, "status", "--porcelain"), "")

    def test_a_task_whose_gate_fails_is_taken_off_again_with_its_output(self):
        gate = f"{sys.executable} -c \"import os,sys; print('b is bad'); sys.exit(1 if os.path.exists('b.txt') else 0)\""
        code, out = self.run_main("--task", "feat/a:1", "--task", "feat/b:2", "--gate", gate)
        result = json.loads(out)
        self.assertEqual(code, 0)
        self.assertEqual([(d["task"], d["reason"]) for d in result["dropped"]], [("feat/b", "gate")])
        self.assertIn("b is bad", result["dropped"][0]["detail"])
        self.assertEqual(self.files("batch/n1"), {"base.txt", "a.txt"})

    def test_when_every_task_is_dropped_the_exit_status_is_one(self):
        code, out = self.run_main("--task", "feat/a:1", "--gate", f"{sys.executable} -c \"import sys; sys.exit(1)\"")
        self.assertEqual((code, json.loads(out)["accepted"]), (1, []))

    def test_no_tasks_is_not_a_failure(self):
        code, out = self.run_main()
        self.assertEqual((code, json.loads(out)["accepted"]), (0, []))

    def test_a_batch_takes_at_most_max_tasks_and_defers_the_rest(self):
        _, out = self.run_main("--task", "feat/a:1", "--task", "feat/b:2", "--max", "1")
        result = json.loads(out)
        self.assertEqual([a["task"] for a in result["accepted"]], ["feat/a"])
        self.assertEqual(result["deferred"], [{"task": "feat/b", "issue": 2}])

    def test_the_same_inputs_give_the_same_tree(self):
        self.run_main("--task", "feat/a:1", "--task", "feat/b:2")
        first = git(self.work, "rev-parse", "batch/n1^{tree}")
        self.run_main("--task", "feat/a:1", "--task", "feat/b:2")
        self.assertEqual(git(self.work, "rev-parse", "batch/n1^{tree}"), first)

    def test_bad_arguments_exit_with_two(self):
        for args in (["--task=-x"], ["--max", "0"], ["--gate", "'unclosed"], ["--gate", " "]):
            self.assertEqual(self.run_main(*args)[0], 2)
        out = io.StringIO()
        self.assertEqual(integrate.main(["--name", "a b"], root=self.work, runner=Gh(), out=out), 2)

    def test_an_unknown_base_is_a_git_failure(self):
        self.assertEqual(self.run_main("--task", "feat/a", "--base", "origin/nope")[0], 2)

    def test_the_result_can_be_written_to_a_file(self):
        target = Path(self.tmp.name) / "result.json"
        self.run_main("--task", "feat/a:1", "--out", str(target))
        self.assertEqual(json.loads(target.read_text(encoding="utf-8"))["accepted"][0]["task"], "feat/a")


class Publishing(Repo):
    def advance_fork(self, name, text):
        """Another clone adds a commit to fork on the remote, as a merged pull request would."""
        other = Path(self.tmp.name) / "other"
        if not other.exists():
            git(self.tmp.name, "clone", "-q", "-b", "fork", str(self.remote), str(other))
        git(other, "pull", "-q", "origin", "fork")
        put(other, name, text)
        git(other, "add", "-A")
        git(other, "commit", "-qm", "later")
        git(other, "push", "-q", "origin", "fork")

    def test_push_publishes_the_batch_branch(self):
        code, _ = self.run_main("--task", "feat/a:1", "--push")
        self.assertEqual(code, 0)
        self.assertIn("batch/n1", git(self.work, "ls-remote", "--heads", "origin"))

    def test_the_batch_is_rebased_onto_a_base_that_moved_before_the_push(self):
        self.run_main("--task", "feat/a:1", "--task", "feat/b:2")
        self.advance_fork("later.txt", "later\n")
        self.run_main("--task", "feat/a:1", "--task", "feat/b:2", "--push")
        git(self.work, "fetch", "-q", "origin")
        self.assertEqual(self.files("origin/batch/n1"), {"base.txt", "a.txt", "b.txt", "later.txt"})

    def test_a_pushed_batch_is_updated_in_place_by_the_next_run(self):
        self.run_main("--task", "feat/a:1", "--push")
        self.run_main("--task", "feat/a:1", "--task", "feat/b:2", "--push")
        git(self.work, "fetch", "-q", "origin")
        self.assertIn("b.txt", self.files("origin/batch/n1"))

    def test_a_gate_that_still_passes_after_the_rebase_lets_the_push_through(self):
        class Moving(Gh):
            def __call__(_me, argv, cwd):
                if argv[:2] == ["git", "fetch"]:
                    self.advance_fork("later.txt", "later\n")
                return super(Moving, _me).__call__(argv, cwd)

        code, _ = self.run_main("--task", "feat/a:1", "--push", "--gate", f"{sys.executable} -c pass", runner=Moving())
        self.assertEqual(code, 0)
        git(self.work, "fetch", "-q", "origin")
        self.assertIn("later.txt", self.files("origin/batch/n1"))

    def test_a_late_gate_failure_after_the_rebase_is_an_error_and_nothing_is_pushed(self):
        # The gate passes while the batch is stacked (before the base moves) and fails after the rebase.
        marker = Path(self.tmp.name) / "stacked"
        gate = (f"{sys.executable} -c \"import os,sys; m='{marker}'; "
                "first = not os.path.exists(m); open(m, 'w').close(); sys.exit(0 if first else 1)\"")

        class Moving(Gh):
            def __call__(_me, argv, cwd):
                if argv[:2] == ["git", "fetch"]:
                    self.advance_fork("later.txt", "later\n")
                return super(Moving, _me).__call__(argv, cwd)

        code, _ = self.run_main("--task", "feat/a:1", "--push", "--gate", gate, runner=Moving())
        self.assertEqual(code, 2)
        self.assertNotIn("batch/n1", git(self.work, "ls-remote", "--heads", "origin"))

    def test_a_batch_that_cannot_rebase_is_an_error(self):
        class Conflicting(Gh):
            def __call__(_me, argv, cwd):
                if argv[:2] == ["git", "fetch"]:
                    self.advance_fork("a.txt", "different\n")
                return super(Conflicting, _me).__call__(argv, cwd)

        code, _ = self.run_main("--task", "feat/a:1", "--push", runner=Conflicting())
        self.assertEqual(code, 2)
        self.assertEqual(git(self.work, "status", "--porcelain"), "")

    def test_pr_opens_one_pull_request_with_the_closing_lines_and_the_assignee(self):
        gh = Gh()
        code, out = self.run_main("--task", "feat/a:11", "--task", "feat/b:12", "--push", "--pr", "--assignee", "someone",
                                  runner=gh)
        self.assertEqual(code, 0)
        create = next(c for c in gh.calls if c[1:3] == ["pr", "create"])
        body = create[create.index("--body") + 1]
        self.assertIn("Closes #11", body)
        self.assertIn("Closes #12", body)
        self.assertEqual(create[create.index("--base") + 1], "fork")
        self.assertEqual(create[create.index("--assignee") + 1], "someone")
        self.assertEqual(json.loads(out)["pr"], "https://example.invalid/pull/7")

    def test_an_existing_pull_request_is_edited_not_duplicated(self):
        gh = Gh(existing="7\n")
        self.run_main("--task", "feat/a:11", "--push", "--pr", runner=gh)
        self.assertTrue(any(c[1:4] == ["pr", "edit", "7"] for c in gh.calls))
        self.assertFalse(any(c[1:3] == ["pr", "create"] for c in gh.calls))

    def test_gh_printing_null_for_no_pull_request_means_there_is_none(self):
        gh = Gh(existing="null\n")
        self.run_main("--task", "feat/a:11", "--push", "--pr", runner=gh)
        self.assertTrue(any(c[1:3] == ["pr", "create"] for c in gh.calls))
        self.assertFalse(any(c[1:3] == ["pr", "edit"] for c in gh.calls))

    def test_a_failing_pull_request_tool_is_an_error(self):
        self.assertEqual(self.run_main("--task", "feat/a:11", "--push", "--pr", runner=Gh(code=1))[0], 2)

    def test_nothing_is_pushed_when_nothing_was_accepted(self):
        code, _ = self.run_main("--task", "feat/a:1", "--push", "--gate", f"{sys.executable} -c \"import sys; sys.exit(1)\"")
        self.assertEqual(code, 1)
        self.assertNotIn("batch/n1", git(self.work, "ls-remote", "--heads", "origin"))

    def test_the_script_runs_as_a_command(self):
        done = subprocess.run([sys.executable, "-I", str(HERE.parent / "integrate.py"), "--name", "x"],
                              cwd=str(self.work), text=True, capture_output=True, check=False,
                              env=dict(ENV, GIT_DIR=str(self.work / ".git")))
        self.assertEqual((done.returncode, json.loads(done.stdout)["accepted"]), (0, []))


if __name__ == "__main__":
    unittest.main()
