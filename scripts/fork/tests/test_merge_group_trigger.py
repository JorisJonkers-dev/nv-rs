# SPDX-License-Identifier: GPL-3.0-only
"""The pipeline runs for a merge queue's merge group (.github/workflows/fork-ci.yml, fork-build.yml).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
The workflows are read as text: no YAML library, no network.
"""
import re
import unittest
from pathlib import Path

WORKFLOWS = Path(__file__).resolve().parents[3] / ".github" / "workflows"
PR_BASE = "github.event.pull_request.base.sha"
MERGE_BASE = "github.event.merge_group.base_sha"


def read(name):
    return (WORKFLOWS / name).read_text(encoding="utf-8")


def triggers(text):
    """The top-level keys of the `on:` block."""
    block = re.search(r"^on:\n((?:[ \t#].*\n|\n)+)", text, re.MULTILINE).group(1)
    return re.findall(r"^  ([a-z_]+):", block, re.MULTILINE)


class MergeGroupTrigger(unittest.TestCase):
    def test_the_pipeline_runs_on_a_merge_group(self):
        text = read("fork-ci.yml")
        self.assertIn("merge_group", triggers(text))
        self.assertRegex(text, r"merge_group:\n    types: \[checks_requested\]")

    def test_the_existing_triggers_stay(self):
        self.assertEqual(triggers(read("fork-ci.yml")),
                         ["pull_request", "merge_group", "push", "workflow_dispatch", "schedule"])

    def test_every_base_that_falls_back_to_a_push_also_reads_the_merge_groups(self):
        for name in ("fork-ci.yml", "fork-build.yml"):
            lines = [x for x in read(name).splitlines() if "BASE:" in x and PR_BASE in x and "github.event.before" in x]
            self.assertTrue(lines, name)
            for line in lines:
                self.assertIn(MERGE_BASE, line, f"{name}: {line.strip()}")

    def test_the_build_plans_a_merge_group_like_a_pull_request(self):
        text = read("fork-build.yml")
        self.assertIn('if [ "$EVENT" = merge_group ]; then EVENT=pull_request; fi', text)
        self.assertLess(text.index("EVENT=pull_request"), text.index("components.py matrix --event"))

    def test_jobs_that_need_the_pull_request_itself_do_not_run_in_a_merge_group(self):
        text = read("fork-ci.yml")
        hygiene = text[text.index("  hygiene:"):]
        self.assertIn("if: ${{ github.event_name == 'pull_request' }}", hygiene.split("\n\n")[0])


if __name__ == "__main__":
    unittest.main()
