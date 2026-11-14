"""Tests of the sync tooling: replay-upstream.sh (conflict policy, dropped paths, stop file), sync_report.py (tracking
issue text), capabilities_diff.py and verify-sync.sh, on small fixture repositories.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v        (no network, no GitHub, no cargo)
"""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
FORK = HERE.parent
sys.path.insert(0, str(FORK))

import capabilities_diff  # noqa: E402
import sync_report  # noqa: E402

ENV = {**os.environ, "GIT_AUTHOR_NAME": "Up Stream", "GIT_AUTHOR_EMAIL": "up@example.org", "GIT_COMMITTER_NAME": "Fork Bot",
       "GIT_COMMITTER_EMAIL": "bot@example.org", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}
ENV.pop("ANTHROPIC_API_KEY", None)


def run(cwd, *cmd, check=True, **kw):
    return subprocess.run(cmd, cwd=cwd, check=check, capture_output=True, text=True, env={**ENV, **kw.pop("env", {})}, **kw)


class Fixture:
    """A bare `origin` with main (upstream's copy) and fork, and a work clone on fork. Helpers commit on either."""

    def __init__(self, tmp):
        self.origin = Path(tmp) / "origin.git"
        self.work = Path(tmp) / "work"
        run(tmp, "git", "init", "-q", "--bare", "-b", "main", str(self.origin))
        run(tmp, "git", "clone", "-q", str(self.origin), str(self.work))
        run(self.work, "git", "checkout", "-q", "-b", "main")
        self.write({
            "README.md": "readme\n",
            "crates/a/lib.rs": "one\ntwo\nthree\n",
            "crates/b/lib.rs": "b\n",
            ".github/workflows/ci.yml": "ci 1\n",
            ".fork/overlay-paths": "# fork-only\n.fork/\nFORK.md\nshared.txt\n",
            ".fork/allowed-divergence": "README.md  # the notice\ncrates/a/lib.rs  # the fork's change\n",
            "shared.txt": "upstream\n",
        })
        self.commit("chore: base")
        run(self.work, "git", "push", "-q", "origin", "main")
        run(self.work, "git", "checkout", "-q", "-b", "fork")
        run(self.work, "git", "push", "-q", "origin", "fork")

    def write(self, files):
        for name, text in files.items():
            p = self.work / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8")

    def commit(self, msg):
        run(self.work, "git", "add", "-A")
        run(self.work, "git", "commit", "-q", "-m", msg)
        return run(self.work, "git", "rev-parse", "HEAD").stdout.strip()

    def on(self, branch, files, msg, delete=()):
        run(self.work, "git", "checkout", "-q", branch)
        self.write(files)
        for d in delete:
            (self.work / d).unlink()
        sha = self.commit(msg)
        run(self.work, "git", "push", "-q", "origin", branch)
        return sha

    def replay(self, **env):
        env = {"SYNC_STOP_FILE": str(self.work.parent / "stop"), "SYNC_DROPPED_FILE": str(self.work.parent / "dropped"), **env}
        run(self.work, "git", "checkout", "-q", "fork")
        return run(self.work, "bash", str(FORK / "replay-upstream.sh"), check=False, env=env)

    def files(self, ref="sync/upstream"):
        return set(run(self.work, "git", "ls-tree", "-r", "--name-only", ref).stdout.split())

    def show(self, ref, path):
        return run(self.work, "git", "show", f"{ref}:{path}").stdout


class Classify(unittest.TestCase):
    def test_classes(self):
        got = sync_report.classify(["a.rs", "README.md", ".fork/x", "dir/f"], ["README.md"], [".fork/", "dir/f"])
        self.assertEqual(got, {"allowed": ["README.md"], "overlay": [".fork/x", "dir/f"], "other": ["a.rs"]})

    def test_allowed_wins_over_overlay(self):
        self.assertEqual(sync_report.classify(["f"], ["f"], ["f"])["allowed"], ["f"])


class Replay(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.fx = Fixture(self.tmp.name)

    def test_plain_commit_replays_with_trailer(self):
        sha = self.fx.on("main", {"crates/b/lib.rs": "b2\n"}, "Fix the b crate\n\nsee #7")
        res = self.fx.replay()
        self.assertEqual(res.returncode, 0, res.stderr)
        msg = run(self.fx.work, "git", "log", "-1", "--format=%B", "sync/upstream").stdout
        self.assertIn(f"Upstream-Commit: {sha}", msg)
        self.assertIn("slaterain/nv-rs#7", msg)
        self.assertEqual(run(self.fx.work, "git", "log", "-1", "--format=%an", "sync/upstream").stdout.strip(), "Up Stream")

    def test_ci_yml_with_a_crate_file_replays_without_the_workflow_hunk(self):
        self.fx.on("main", {"crates/b/lib.rs": "b2\n", ".github/workflows/ci.yml": "ci 2\n"}, "Feature plus ci")
        res = self.fx.replay()
        self.assertEqual(res.returncode, 0, res.stderr)
        self.assertEqual(self.fx.show("sync/upstream", "crates/b/lib.rs"), "b2\n")
        self.assertEqual(self.fx.show("sync/upstream", ".github/workflows/ci.yml"), "ci 1\n")
        self.assertIn(".github/workflows/ci.yml", (Path(self.tmp.name) / "dropped").read_text())
        self.assertIn("Not replayed", run(self.fx.work, "git", "log", "-1", "--format=%B", "sync/upstream").stdout)

    def test_other_and_new_workflow_files_are_dropped_too(self):
        self.fx.on("main", {"crates/b/lib.rs": "b3\n", ".github/workflows/other.yml": "x\n"}, "Adds a workflow")
        res = self.fx.replay()
        self.assertEqual(res.returncode, 0, res.stderr)
        self.assertNotIn(".github/workflows/other.yml", self.fx.files())

    def test_deleted_workflow_stays(self):
        self.fx.on("main", {}, "Drops ci", delete=[".github/workflows/ci.yml"])
        self.assertEqual(self.fx.replay().returncode, 0)
        self.assertIn(".github/workflows/ci.yml", self.fx.files())

    def test_apply_policy_replays_workflow_changes(self):
        self.fx.on("main", {".github/workflows/ci.yml": "ci 2\n"}, "ci change")
        self.assertEqual(self.fx.replay(WORKFLOW_POLICY="apply").returncode, 0)
        self.assertEqual(self.fx.show("sync/upstream", ".github/workflows/ci.yml"), "ci 2\n")

    def test_fork_wins_on_an_overlay_path(self):
        self.fx.on("fork", {"shared.txt": "fork\n"}, "fork owns shared")
        self.fx.on("main", {"shared.txt": "upstream 2\n", "crates/b/lib.rs": "b4\n"}, "Touches a fork path")
        res = self.fx.replay()
        self.assertEqual(res.returncode, 0, res.stderr)
        self.assertEqual(self.fx.show("sync/upstream", "shared.txt"), "fork\n")
        self.assertEqual(self.fx.show("sync/upstream", "crates/b/lib.rs"), "b4\n")
        self.assertIn("shared.txt", (Path(self.tmp.name) / "dropped").read_text())

    def test_conflict_in_allowed_divergence_stops_and_is_reported(self):
        self.fx.on("fork", {"crates/a/lib.rs": "one\nFORK\nthree\n"}, "fork change")
        sha = self.fx.on("main", {"crates/a/lib.rs": "one\nUP\nthree\n"}, "upstream change")
        res = self.fx.replay()
        self.assertEqual(res.returncode, 1)
        self.assertIn("does not apply", res.stderr)
        info = sync_report.parse_stop(str(Path(self.tmp.name) / "stop"))
        self.assertEqual((info["reason"], info["commit"], info["subject"]), ("conflict", sha, "upstream change"))
        # the work tree is left clean on the partial branch, as the workflow expects
        files = sync_report.conflict_files(sha, "HEAD", cwd=self.fx.work)
        self.assertEqual(files, ["crates/a/lib.rs"])
        body = sync_report.build_body(info, files, sync_report.read_list(str(self.fx.work / ".fork/allowed-divergence")),
                                      sync_report.read_list(str(self.fx.work / ".fork/overlay-paths")), "https://run")
        self.assertIn(sha, body)
        self.assertIn("`crates/a/lib.rs`: listed in `.fork/allowed-divergence`", body)
        self.assertIn("a commit does not apply", body)

    def test_body_classes_each_file(self):
        info = {"reason": "conflict", "commit": "c" * 40, "subject": "s"}
        body = sync_report.build_body(info, ["README.md", ".fork/x", "crates/z.rs"], ["README.md"], [".fork/"])
        self.assertIn("`README.md`: listed in `.fork/allowed-divergence`", body)
        self.assertIn("`.fork/x`: listed in `.fork/overlay-paths`", body)
        self.assertIn("`crates/z.rs`: anything else", body)

    def test_clean_replay_writes_no_stop_file(self):
        self.fx.on("main", {"crates/b/lib.rs": "b5\n"}, "ok")
        self.assertEqual(self.fx.replay().returncode, 0)
        self.assertFalse((Path(self.tmp.name) / "stop").exists())


class VerifySync(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.fx = Fixture(self.tmp.name)
        # the fork starts at upstream's commit: the merge base is the last replayed point
        run(self.fx.work, "git", "remote", "get-url", "origin")

    def verify(self):
        run(self.fx.work, "git", "checkout", "-q", "fork")
        return run(self.fx.work, "bash", str(FORK / "verify-sync.sh"), check=False)

    def test_listed_divergence_passes(self):
        self.fx.on("fork", {"crates/a/lib.rs": "fork\n", "FORK.md": "x\n"}, "ok")
        res = self.verify()
        self.assertEqual(res.returncode, 0, res.stderr)

    def test_unlisted_upstream_file_fails(self):
        self.fx.on("fork", {"crates/b/lib.rs": "fork\n"}, "bad")
        res = self.verify()
        self.assertEqual(res.returncode, 1)
        self.assertIn("crates/b/lib.rs", res.stderr)

    def test_workflow_differences_are_ignored(self):
        self.fx.on("fork", {".github/workflows/ci.yml": "fork ci\n"}, "own ci")
        self.assertEqual(self.verify().returncode, 0)


class CapabilitiesDiff(unittest.TestCase):
    def doc(self, names, total):
        return {"extenders": [{"id": "x", "name": "X", "commands": {"implemented": len(names), "implemented_names": names, "total": total}}]}

    def test_added_and_removed(self):
        text = capabilities_diff.diff(self.doc(["A", "B"], 5), self.doc(["B", "C", "D"], 6))
        self.assertIn("2 to 3 implemented, total 5 to 6", text)
        self.assertIn("added (2): `C`, `D`", text)
        self.assertIn("removed (1): `A`", text)

    def test_no_change_is_empty(self):
        self.assertEqual(capabilities_diff.diff(self.doc(["A"], 1), self.doc(["A"], 1)), "")


if __name__ == "__main__":
    unittest.main()
