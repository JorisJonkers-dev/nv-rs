# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the porting loop's worker and reviewer commands (scripts/fork/porting_agent.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
The gateway is a stub; no model, network or cargo is used.
"""
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

FORK = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(FORK))

import porting_agent  # noqa: E402


def git(cwd, *args):
    return subprocess.run(["git", *args], cwd=cwd, check=True, text=True, stdout=subprocess.PIPE,
                          env={"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
                               "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
                               "PATH": "/usr/bin:/bin:/run/current-system/sw/bin"}).stdout


class AgentTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.wt = root / "wt"
        (self.wt / "docs" / "fork").mkdir(parents=True)
        (self.wt / "docs" / "fork" / "PORTING-RULEBOOK.md").write_text("RULES\n")
        (self.wt / "crates" / "engine" / "tests").mkdir(parents=True)
        (self.wt / "crates" / "engine" / "tests" / "typed_small.rs").write_text("x")
        git(self.wt, "init", "-q")
        for key, value in (("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false"),
                           ("core.hooksPath", "/dev/null")):
            git(self.wt, "config", key, value)
        git(self.wt, "add", "-A")
        git(self.wt, "commit", "-qm", "chore: base")
        self.queue = root / "q.tsv"
        self.queue.write_text("1\t00401000\topen\tno\tsmall\tf\n")
        patcher = mock.patch.object(porting_agent, "clean")
        patcher.start()
        self.addCleanup(patcher.stop)
        self.env = {
            "NV_PORT_WORKTREE": str(self.wt), "NV_PORT_FAMILY": "small", "NV_PORT_BATCH": "b",
            "NV_PORT_QUEUE_FILE": str(self.queue), "NV_PORT_METRICS": str(root / "m.jsonl"),
            "NV_PORT_SLOT_DIR": str(root / "slots"), "NV_PORT_COOLDOWN": str(root / "cool.json"), "PATH": "/usr/bin:/bin:/run/current-system/sw/bin",
            "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
            "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
        }

    def test_missing_environment_is_refused(self):
        self.assertEqual(porting_agent.main(["work"], env={}), 2)
        self.assertEqual(porting_agent.main(["bogus"], env=self.env), 2)

    def test_work_applies_the_gateway_patch_and_commits(self):
        gw = Path(self.tmp.name) / "gw"
        subprocess.run(["git", "clone", "-q", "--no-hardlinks", str(self.wt), str(gw)], check=True,
                       env={"PATH": "/usr/bin:/bin:/run/current-system/sw/bin"})
        (gw / "new.rs").write_text("// SPDX-License-Identifier: GPL-3.0-only\n")
        git(gw, "add", "-A")
        seen = {}

        def fake(env, payload, sandbox=None):
            seen.update(payload)
            return {"status": "ok", "worktree": str(gw), "staged_files": ["new.rs"]}

        with mock.patch.object(porting_agent, "gateway", fake):
            self.assertEqual(porting_agent.work(self.env), 0)
        self.assertEqual(seen["mode"], "write")
        self.assertIn("00401000", seen["prompt"])
        self.assertIn("RULES", seen["prompt"])
        self.assertTrue((self.wt / "new.rs").is_file())
        self.assertIn("type the small family", git(self.wt, "log", "-1", "--format=%s"))
        record = json.loads(Path(self.env["NV_PORT_METRICS"]).read_text().splitlines()[-1])
        self.assertEqual(record["outcome"], "committed")

    def test_work_fails_on_gateway_failure_and_empty_change(self):
        with mock.patch.object(porting_agent, "gateway", lambda *a, **k: {"status": "failed"}):
            self.assertEqual(porting_agent.work(self.env), 1)
        gw = Path(self.tmp.name) / "gw"
        subprocess.run(["git", "clone", "-q", "--no-hardlinks", str(self.wt), str(gw)], check=True,
                       env={"PATH": "/usr/bin:/bin:/run/current-system/sw/bin"})
        ok = {"status": "ok", "worktree": str(gw)}
        with mock.patch.object(porting_agent, "gateway", lambda *a, **k: ok):
            self.assertEqual(porting_agent.work(self.env), 1)

    def test_review_needs_accept_and_a_different_model(self):
        (self.wt / "f.rs").write_text("a")
        git(self.wt, "add", "-A")
        git(self.wt, "commit", "-qm", "feat: f")
        seen = {}

        def fake(text):
            def run(env, payload, sandbox=None):
                seen["sandbox"], seen["mode"] = sandbox, payload["mode"]
                return {"status": "ok", "output": text}
            return run

        with mock.patch.object(porting_agent, "gateway", fake("fine\nVERDICT: ACCEPT\n")):
            self.assertEqual(porting_agent.review(self.env), 0)
        self.assertEqual(seen, {"sandbox": "sbx", "mode": "read-only"})
        with mock.patch.object(porting_agent, "gateway", fake("VERDICT: REJECT")):
            self.assertEqual(porting_agent.review(self.env), 1)
        with mock.patch.object(porting_agent, "gateway", fake("no verdict")):
            self.assertEqual(porting_agent.review(self.env), 1)
        same = dict(self.env, NV_PORT_REVIEW_CLIENT="pi-free", NV_PORT_REVIEW_MODEL="free/deepseek-v4.1-flash")
        self.assertEqual(porting_agent.review(same), 2)

    def test_a_limited_model_falls_over_to_the_next_and_cools_down(self):
        env = dict(self.env, NV_PORT_WORKER_MODELS="free/deepseek-v4.1-flash,opencode:opencode-go/hy4-flash-free",
                   NV_PORT_FAMILY="small")
        order = porting_agent.candidates(env, "NV_PORT_WORKER", porting_agent.DEFAULT_WORKER)
        self.assertEqual(len(order), 2)
        limited = {"status": "failed", "summary": "every free deployment is limited; back in 20021s"}
        self.assertTrue(porting_agent.is_limited(limited))
        self.assertFalse(porting_agent.is_limited({"status": "ok", "summary": "429 mentioned"}))
        porting_agent.mark_limited(env, order[0][1], json.dumps(limited))
        self.assertGreater(porting_agent.cooling(env)[order[0][1]], 20000 + __import__("time").time() - 5)
        again = porting_agent.candidates(env, "NV_PORT_WORKER", porting_agent.DEFAULT_WORKER)
        self.assertEqual(again[-1], order[0])
        self.assertEqual(sorted(p[0] for p in order), ["opencode", "pi-free"])

    def test_work_tries_the_next_model_when_the_first_is_limited(self):
        gw = Path(self.tmp.name) / "gw"
        subprocess.run(["git", "clone", "-q", "--no-hardlinks", str(self.wt), str(gw)], check=True,
                       env={"PATH": "/usr/bin:/bin:/run/current-system/sw/bin"})
        (gw / "new.rs").write_text("x\n")
        git(gw, "add", "-A")
        models = []

        def fake(env, payload, sandbox=None):
            models.append(payload["model"])
            if len(models) == 1:
                return {"status": "failed", "summary": "429 every free deployment is limited; back in 100s"}
            return {"status": "ok", "worktree": str(gw)}

        env = dict(self.env, NV_PORT_WORKER_MODELS="free/a-1,free/b-1")
        with mock.patch.object(porting_agent, "gateway", fake):
            self.assertEqual(porting_agent.work(env), 0)
        self.assertEqual(len(set(models)), 2)

    def test_template_family_is_the_smallest_suite(self):
        self.assertEqual(porting_agent.template_family(self.wt), "small")


if __name__ == "__main__":
    unittest.main()
