"""Tests of the private run's checker hash and skip-or-fail behaviour.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
Every input is made in a temporary directory: no game files, no backend and no cargo.
"""
import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
FORK = HERE.parent
sys.path.insert(0, str(FORK))

import private_integrity  # noqa: E402
import private_run  # noqa: E402


def make_tree(root, stored=None):
    """A tree with one stand-in file per protected group, and a stored hash."""
    root = Path(root)
    for paths in private_integrity.GROUPS.values():
        for rel in paths:
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"// " + rel.encode() + b"\n")
    (root / ".fork").mkdir(parents=True, exist_ok=True)
    digest = private_integrity.digest(root)
    (root / ".fork" / private_integrity.STORED).write_text((stored or digest) + "\n")
    return digest


def gate_line(kind, name="some::test", variable="NV_EXE"):
    suffix = "set" if kind == "executed" else "not set"
    return f'print("{kind}: {name} ({variable} {suffix})")'


class IntegrityTests(unittest.TestCase):
    def test_each_group_is_covered_by_the_one_hash(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            original = make_tree(root)
            for group, paths in private_integrity.GROUPS.items():
                with self.subTest(group=group):
                    path = root / paths[0]
                    path.write_bytes(path.read_bytes() + b"// changed\n")
                    self.assertNotEqual(private_integrity.digest(root), original, group)
                    path.write_bytes(b"// " + paths[0].encode() + b"\n")
            self.assertEqual(private_integrity.digest(root), original)

    def test_a_missing_or_changed_file_fails_the_check(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            make_tree(root)
            ok, why = private_integrity.check(root)
            self.assertTrue(ok, why)
            path = root / private_integrity.GROUPS["controls"][0]
            path.write_bytes(b"// a planted change\n")
            ok, why = private_integrity.check(root)
            self.assertFalse(ok)
            self.assertIn("update", why)
            self.assertIn("controls", why)
            path.unlink()
            ok, why = private_integrity.check(root)
            self.assertFalse(ok)
            self.assertIn("controls", why)

    def test_line_endings_do_not_change_the_hash(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            original = make_tree(root)
            path = root / private_integrity.GROUPS["judge"][0]
            path.write_bytes(path.read_bytes().replace(b"\n", b"\r\n"))
            self.assertEqual(private_integrity.digest(root), original)

    def test_main_refuses_to_run_when_the_hash_changed(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            make_tree(root, stored="0" * 64)
            marker = root / "ran"
            command = ("marker", [sys.executable, "-c", f"Path({str(marker)!r}).write_text('yes')"])
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                code = private_run.main(["--root", str(root), "--require"], commands=[command])
            self.assertEqual(code, 1)
            self.assertFalse(marker.exists())
            self.assertIn("checker hash", err.getvalue())


class RunTests(unittest.TestCase):
    def run_commands(self, snippets, require):
        commands = [(f"t{i}", [sys.executable, "-c", snippet]) for i, snippet in enumerate(snippets)]
        with tempfile.TemporaryDirectory() as d:
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = private_run.run(commands, Path(d), require)
        return code, out.getvalue(), err.getvalue()

    def test_public_run_reports_a_skip_and_stays_green(self):
        code, out, _ = self.run_commands([gate_line("executed"), gate_line("skipped")], False)
        self.assertEqual(code, 0)
        self.assertIn("1 executed, 1 skipped", out)

    def test_require_fails_on_any_skip(self):
        code, _, err = self.run_commands([gate_line("executed"), gate_line("skipped")], True)
        self.assertEqual(code, 1)
        self.assertIn("a gated test skipped", err)

    def test_require_fails_when_no_gated_test_executed(self):
        code, _, err = self.run_commands([gate_line("skipped")], True)
        self.assertEqual(code, 1)
        self.assertIn("a gated test skipped", err)
        code, _, err = self.run_commands(["print('no gate here')"], True)
        self.assertEqual(code, 1)
        self.assertIn("no gated test executed", err)

    def test_require_passes_when_every_gated_test_executed(self):
        code, _, _ = self.run_commands([gate_line("executed"), gate_line("executed", "other::test")], True)
        self.assertEqual(code, 0)

    def test_require_fails_on_a_skip_outside_the_data_gate(self):
        code, _, err = self.run_commands(
            [gate_line("executed"), 'print("skipped: peer (backend not available)")'], True
        )
        self.assertEqual(code, 1)
        self.assertIn("outside the data gate", err)

    def test_a_failing_command_fails_the_run(self):
        code, _, err = self.run_commands(["raise SystemExit(3)"], False)
        self.assertEqual(code, 1)
        self.assertIn("failed", err)


class WorkflowTests(unittest.TestCase):
    def test_the_private_workflow_cannot_run_for_a_pull_request(self):
        text = (FORK.parent.parent / ".github/workflows/fork-private.yml").read_text()
        self.assertNotIn("pull_request", text)
        lines = text.splitlines()
        triggers = []
        for i, line in enumerate(lines):
            if line.strip() == "on:":
                for following in lines[i + 1:]:
                    if following and not following.startswith((" ", "\t", "#")):
                        break
                    stripped = following.strip()
                    if stripped and not stripped.startswith(("#", "-")) and following.startswith("  ") and not following.startswith("   "):
                        triggers.append(stripped.split(":", 1)[0])
                break
        self.assertEqual(set(triggers), {"workflow_dispatch", "schedule"}, triggers)

    def test_the_private_workflow_runs_the_check_and_requires_gate_counts(self):
        text = (FORK.parent.parent / ".github/workflows/fork-private.yml").read_text()
        self.assertIn("private_integrity.py --check", text)
        self.assertIn("private_run.py --require", text)


if __name__ == "__main__":
    unittest.main()
