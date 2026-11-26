"""Tests of the component census contract in fork-ci.yml (no YAML dependency, no cargo).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
"""
import contextlib
import io
import posixpath
import re
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
WORKFLOW = (ROOT / ".github/workflows/fork-ci.yml").read_text(encoding="utf-8")
PROGRESS = (ROOT / ".github/workflows/fork-progress.yml").read_text(encoding="utf-8")


def job(name):
    """The lines of one job, from its id to the next top-level key."""
    found = re.search(r"^  " + name + r":\n(?P<body>(?:^    .*\n|^      .*\n|^ *\n)*)", WORKFLOW, re.M)
    return found.group("body") if found else ""


def step(name):
    """The lines of the step with this name, up to the next step."""
    found = re.search(r"^      - name: " + re.escape(name) + r"\n(?P<body>(?:^        .*\n|^\s*\n)*)", WORKFLOW, re.M)
    return found.group("body") if found else ""


def report_files(text=WORKFLOW):
    """The paths in a workflow's REPORT_FILES list."""
    block = text.split("REPORT_FILES: >-", 1)[1].split("\njobs:", 1)[0]
    return [line.strip() for line in block.splitlines() if line.strip() and not line.strip().startswith("#")]


def upload_paths():
    """The paths of the report-files upload in the catalogue job (single value or a block)."""
    body = re.search(r"^      - uses: actions/upload-artifact@[^\n]*\n(?P<body>(?:^        .*\n|^\s*\n)*)",
                     job("catalogue"), re.M).group("body")
    single = re.search(r"^          path: (\S.*)$", body, re.M)
    if single and single.group(1) not in ("|", ">-"):
        return [single.group(1)]
    block = re.search(r"^          path: \|\n(?P<paths>(?:^            .*\n)+)", body, re.M)
    return [line.strip() for line in block.group("paths").splitlines()]


class CensusWorkflowTests(unittest.TestCase):
    def test_the_pull_request_check_is_limited_to_census_changes(self):
        check = step("Component census")
        self.assertIn("github.event_name == 'pull_request'", check)
        self.assertIn("needs.changes.outputs.census == 'true'", check)
        self.assertIn("--check", check)

    def test_the_changed_files_job_detects_the_tool_and_its_declarations(self):
        changes = job("changes")
        self.assertIn("census: ${{ steps.diff.outputs.census }}", changes)
        self.assertIn("research/component-census/", changes)
        self.assertIn("docs/fork/COMPONENT-CENSUS.md", changes)

    def test_the_diff_rule_selects_the_tool_and_the_report_only(self):
        block = WORKFLOW.split("census_files = (", 1)[1].split("\n          '", 1)[0]
        code = textwrap.dedent("          census_files = (" + block)

        def selected(files):
            namespace = {"diff": lambda _: files}
            with contextlib.redirect_stdout(io.StringIO()):
                exec(code, namespace)
            return namespace["census"]

        self.assertEqual(selected(["research/component-census/src/main.rs"]),
                         ["research/component-census/src/main.rs"])
        self.assertEqual(selected(["research/component-census/components.tsv"]),
                         ["research/component-census/components.tsv"])
        self.assertEqual(selected(["docs/fork/COMPONENT-CENSUS.md"]), ["docs/fork/COMPONENT-CENSUS.md"])
        self.assertEqual(selected(["crates/engine-core/src/lib.rs"]), [])
        self.assertEqual(selected([]), [])

    def test_the_census_tests_run_in_ci(self):
        self.assertIn("cargo test --locked --manifest-path research/component-census/Cargo.toml",
                      step("Census tool tests"))

    def test_the_report_is_regenerated_on_fork(self):
        catalogue = job("catalogue")
        lines = [l.strip() for l in catalogue.splitlines()]
        render = next(i for i, l in enumerate(lines) if l.startswith('cd "$RUNNER_TEMP/render"'))
        write = next(i for i, l in enumerate(lines)
                     if "cargo run --locked --quiet --manifest-path research/component-census/Cargo.toml" in l)
        self.assertGreater(write, render)
        self.assertNotIn("--check", lines[write])

    def test_the_generated_report_is_committed_with_the_other_reports(self):
        env = WORKFLOW.split("REPORT_FILES: >-", 1)[1].split("jobs:", 1)[0]
        self.assertIn("docs/fork/COMPONENT-CENSUS.md", env)
        self.assertIn("chore(catalogue): update the generated reports [skip ci]", WORKFLOW)
        reports = job("reports")
        self.assertIn("github.event_name == 'push' && github.ref == 'refs/heads/fork'", reports)

    def test_the_artifact_publishes_every_report_at_the_path_the_reports_job_copies(self):
        # Model upload-artifact: files are stored relative to the common root of the upload paths.
        reports = job("reports")
        uploads = [p.replace("${{ runner.temp }}", "/runner") for p in upload_paths()]
        root = posixpath.commonpath(uploads)
        render = "/runner/render"
        prefix = re.search(r'cp "\$RUNNER_TEMP/report-files/\$\{f#([^}]*)\}"', reports).group(1)
        self.assertIn("for f in $REPORT_FILES; do", reports)
        self.assertIn('test -s "$f"', reports)
        self.assertIn('"${git[@]}" add -- $REPORT_FILES', reports)
        for f in report_files():
            in_artifact = posixpath.relpath(posixpath.join(render, f), root)
            published = any(
                in_artifact.startswith(posixpath.relpath(p, root) + "/") if p.endswith("/")
                else in_artifact == posixpath.relpath(p, root)
                for p in uploads
            )
            self.assertTrue(published, f"{f} is missing from the artifact (as {in_artifact})")
            self.assertEqual(f.removeprefix(prefix), in_artifact, f"the copy of {f} has the wrong artifact path")

    def test_the_progress_bot_accepts_the_reports_between_its_commit_and_the_head(self):
        # A reports commit replayed under a progress commit is not a reason to skip the latter.
        self.assertTrue(set(report_files()).issubset(set(report_files(PROGRESS))))


if __name__ == "__main__":
    unittest.main()
