"""Tests of the artifact contract in fork-coverage.yml (no YAML dependency, no network, no cargo).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
"""
import fnmatch
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
WORKFLOW = (ROOT / ".github/workflows/fork-coverage.yml").read_text(encoding="utf-8")


def artifact_step_values(action):
    """The `with:` values of every step that uses actions/<action>-artifact."""
    steps = []
    current = None
    for line in WORKFLOW.splitlines():
        if re.match(r"^\s*-\s+uses:\s*actions/" + action + r"-artifact@", line):
            current = {}
            steps.append(current)
            continue
        if re.match(r"^\s*-\s+(uses|name):", line):
            current = None
            continue
        if current is not None:
            m = re.match(r"^\s{10}(\w+):\s*(.+?)\s*$", line)
            if m:
                current[m.group(1)] = m.group(2).strip("'\"")
    return steps


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.uploads = artifact_step_values("upload")
        self.downloads = artifact_step_values("download")
        self.names = [step.get("name") for step in self.uploads]
        self.patterns = [step["pattern"] for step in self.downloads if "pattern" in step]

    def test_the_plan_artifact_has_the_cov_prefix(self):
        self.assertIn("cov-plan", self.names)
        self.assertNotIn("coverage-plan", self.names)

    def test_the_report_and_gate_download_only_cov_artifacts(self):
        self.assertEqual(len(self.patterns), 2)
        for pattern in self.patterns:
            self.assertEqual(pattern, "cov-*")
            self.assertTrue(fnmatch.fnmatchcase("cov-plan", pattern))
            self.assertTrue(fnmatch.fnmatchcase("cov-${{ matrix.component }}", pattern))

    def test_the_comment_artifact_stays_outside_the_downloaded_prefix(self):
        comments = [name for name in self.names if name and "comment" in name]
        self.assertEqual(comments, ["coverage-comment"])
        for pattern in self.patterns:
            self.assertFalse(fnmatch.fnmatchcase(comments[0], pattern))
        by_name = [step.get("name") for step in self.downloads if "name" in step]
        self.assertIn(comments[0], by_name)

    def test_both_jobs_read_the_plan_from_the_uploaded_artifact(self):
        self.assertEqual(re.findall(r"--plan\s+(\S+)", WORKFLOW),
                         ["artifacts/cov-plan/plan.json", "artifacts/cov-plan/plan.json"])


if __name__ == "__main__":
    unittest.main()
