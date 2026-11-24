"""Tests of divergence_report.py (fork-divergence.yml), on synthetic fixtures: a temporary git repository.

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

import divergence_report as dr  # noqa: E402

OVERLAY = ["docs/fork/", ".fork/", "crates/fork-log/", "crates/a/src/own.rs"]
DOC = """# D

| Fork-only change | Why |
|---|---|
| `a/src/table.rs` | a reason |

## Records

### Window hook
- Files: `crates/a/src/full.rs`
- Upstream approach: none off Windows
- Fork approach: a hook
- Decision: keep
- Reason: the game runs on Linux
- Evidence: nv-linux#1

### Half
- Files: `crates/a/src/half.rs`
- Reason: because
"""


class Classify(unittest.TestCase):
    def test_owner(self):
        c = lambda p: dr.classify(p, OVERLAY, {"fork-hitch"})  # noqa: E731
        self.assertEqual(c("docs/fork/X.md"), "fork")
        self.assertEqual(c(".github/workflows/fork-ci.yml"), "fork")
        self.assertEqual(c("crates/fork-hitch/src/lib.rs"), "fork")
        self.assertEqual(c("crates/a/src/own.rs"), "fork")
        self.assertEqual(c("crates/a/src/other.rs"), "upstream")
        self.assertEqual(c("Cargo.toml"), "upstream")

    def test_patterns_drop_comments(self):
        self.assertEqual(dr.read_patterns("# c\nREADME.md  # why\n\n"), ["README.md"])


class Numstat(unittest.TestCase):
    def test_lines_and_binary_and_rename(self):
        n = dr.parse_numstat("3\t2\ta.rs\n-\t-\timg.png\n1\t1\tcrates/{x => y}/z.rs\nnoise\n")
        self.assertEqual(n, {"a.rs": 5, "img.png": 1, "crates/y/z.rs": 2})

    def test_divergence_counts_upstream_owned_only(self):
        n = {"crates/a/src/x.rs": 10, "docs/fork/X.md": 99}
        self.assertEqual(dr.divergence(n, OVERLAY, set()), (1, 10))


class Records(unittest.TestCase):
    def test_statuses(self):
        self.assertEqual(dr.record_status("crates/a/src/full.rs", DOC), "complete")
        self.assertTrue(dr.record_status("crates/a/src/half.rs", DOC).startswith("incomplete: Upstream approach"))
        self.assertEqual(dr.record_status("crates/a/src/table.rs", DOC), "table row only")
        self.assertEqual(dr.record_status("crates/a/src/none.rs", DOC), "missing")


class Budget(unittest.TestCase):
    def test_defaults_and_override(self):
        self.assertEqual(dr.load_budget("")["pr_changed_lines"], 300)
        self.assertEqual(dr.load_budget("pr_changed_lines = 5")["pr_changed_lines"], 5)
        self.assertEqual(dr.load_budget("not toml [")["pr_changed_lines"], 300)

    def test_render_warns(self):
        res = {"budget": dr.DEFAULT_BUDGET, "last": "x", "pr_files": 2, "fork_files": 0, "pr_upstream_lines": 301,
               "touched": {}, "base_div": (1, 100), "head_div": (2, 120), "week_div": (1, 100),
               "files": [{"path": "crates/a/b.rs", "lines": 301, "listed": False, "new": True, "record": "missing"}]}
        text, warnings = dr.render(res)
        self.assertEqual(len(warnings), 3)
        self.assertIn("301 lines", text)
        self.assertIn("grew 20.0%", text)
        self.assertIn("`crates/a/b.rs`", text)

    def test_render_clean(self):
        res = {"budget": dr.DEFAULT_BUDGET, "last": None, "pr_files": 0, "fork_files": 0, "pr_upstream_lines": 0,
               "touched": {}, "files": []}
        text, warnings = dr.render(res)
        self.assertEqual(warnings, [])
        self.assertIn("No warning", text)


def run(cwd, *cmd, env=None):
    subprocess.run(cmd, cwd=cwd, check=True, capture_output=True, env={**os.environ, **(env or {})})


class Repo(unittest.TestCase):
    def test_end_to_end(self):
        d = tempfile.mkdtemp()
        git = lambda *a: run(d, "git", "-c", "user.name=t", "-c", "user.email=t@e", *a)  # noqa: E731
        w = lambda name, text: (Path(d, name).parent.mkdir(parents=True, exist_ok=True), Path(d, name).write_text(text))  # noqa: E731
        git("init", "-q", "-b", "fork")
        w("crates/a/src/lib.rs", "1\n2\n3\n")
        w("crates/a/src/known.rs", "1\n")
        w(".fork/overlay-paths", "docs/fork/\n.fork/\n")
        w(".fork/allowed-divergence", "crates/a/src/known.rs\n")
        w(".fork/allowed-divergence.d/second", "crates/a/src/second.rs  # its own file\n")
        w("crates/a/src/second.rs", "1\n")
        w("crates/a/src/own.rs", "1\n")
        w("docs/fork/DIVERGENCE.md", DOC)
        git("add", "-A")
        git("commit", "-qm", "init\n\nUpstream-Commit: " + "a" * 40)
        git("checkout", "-qb", "pr")
        w("crates/a/src/lib.rs", "1\n2\n3\n4\n5\n")
        w("crates/a/src/known.rs", "1\n2\n")
        w("crates/a/src/second.rs", "1\n2\n")
        w("crates/a/src/own.rs", "1\n2\n")
        # One file per record (docs/fork/divergence.d/): the pull request adds its own, DIVERGENCE.md is untouched.
        w("docs/fork/divergence.d/own-file.md", "### Own file\n- Files: `crates/a/src/own.rs`\n- Upstream approach: u\n"
          "- Fork approach: f\n- Decision: keep\n- Reason: r\n- Evidence: e\n")
        w("docs/fork/NOTE.md", "x\n")
        git("add", "-A")
        git("commit", "-qm", "change")
        res = dr.analyse("fork", "pr", d)
        paths = {f["path"]: f for f in res["files"]}
        self.assertEqual(sorted(paths), ["crates/a/src/known.rs", "crates/a/src/lib.rs", "crates/a/src/own.rs",
                                         "crates/a/src/second.rs"])
        self.assertEqual(paths["crates/a/src/own.rs"]["record"], "complete")
        self.assertTrue(paths["crates/a/src/second.rs"]["listed"])
        self.assertTrue(paths["crates/a/src/lib.rs"]["new"])
        self.assertEqual(paths["crates/a/src/lib.rs"]["record"], "missing")
        self.assertTrue(paths["crates/a/src/known.rs"]["listed"])
        self.assertEqual(res["fork_files"], 2)
        self.assertEqual(res["pr_upstream_lines"], 5)  # lib.rs 2, known.rs 1, second.rs 1, own.rs 1


if __name__ == "__main__":
    unittest.main()
