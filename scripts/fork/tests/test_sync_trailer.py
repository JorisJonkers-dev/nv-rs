"""Tests of sync_trailer.py (the Verify sync job), on synthetic fixtures: a temporary git repository.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v        (no network, no cargo)
"""
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import sync_trailer as st  # noqa: E402

A, B = "a" * 40, "b" * 40


class Verdict(unittest.TestCase):
    def test_a_pull_request_that_replays_nothing_needs_no_trailer(self):
        self.assertIsNone(st.verdict(None, "Just a fix."))

    def test_the_newest_trailer_on_the_last_line_passes(self):
        self.assertIsNone(st.verdict(B, f"Replays upstream.\n\nUpstream-Commit: {B}\n\n"))

    def test_crlf_bodies_from_the_web_editor_pass(self):
        self.assertIsNone(st.verdict(B, f"Replays upstream.\r\n\r\nUpstream-Commit: {B}\r\n"))

    def test_a_missing_trailer_fails_and_names_the_commit(self):
        error = st.verdict(B, "Replays upstream. Merge by rebase.")
        self.assertIn(f"Upstream-Commit: {B}", error)

    def test_an_older_trailer_fails(self):
        self.assertIn("newest replayed commit", st.verdict(B, f"Upstream-Commit: {A}"))

    def test_a_trailer_followed_by_more_text_fails(self):
        self.assertIsNotNone(st.verdict(B, f"Upstream-Commit: {B}\n\nEND_COMMIT_OVERRIDE"))


class NewestReplayed(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.invalid",
                    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.invalid"}
        self.git("init", "-q")
        self.commit(f"base\n\nUpstream-Commit: {A}")
        self.base = self.git("rev-parse", "HEAD")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.tmp.name, env=self.env, check=True,
                              capture_output=True, text=True).stdout.strip()

    def commit(self, message):
        self.git("commit", "-q", "--allow-empty", "-m", message)

    def newest(self):
        cwd = os.getcwd()
        os.chdir(self.tmp.name)
        try:
            return st.newest_replayed(self.base, "HEAD")
        finally:
            os.chdir(cwd)

    def test_a_trailer_only_on_the_base_does_not_count(self):
        self.commit("fix(fork): something")
        self.assertIsNone(self.newest())

    def test_the_newest_replayed_commit_of_the_pull_request(self):
        self.commit(f"fix(a): one\n\nUpstream-Commit: {A}")
        self.commit(f"fix(b): two\n\nUpstream-Commit: {B}")
        self.commit("chore(fork): regenerate the capabilities file")
        self.assertEqual(self.newest(), B)


if __name__ == "__main__":
    unittest.main()
