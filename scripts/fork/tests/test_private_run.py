"""Tests of the private run's checker hash and skip-or-fail behaviour.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
Every input is made in a temporary directory: no game files, no backend and no cargo.
"""
import contextlib
import io
import os
import re
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

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


# Every file with a private-input test the private run drives. The test below derives the tree's set and
# refuses a new one until it is added here or to UNCOVERED_DATA_FILES (with a reason in docs/fork/TESTING.md).
COVERED_DATA_FILES = frozenset({
    "crates/engine-core/src/exe.rs",
    "crates/engine-core/src/typed/backend_gaps.rs",
    "crates/engine-core/src/typed/backend_peer.rs",
    "crates/engine-core/src/units/crt/cvt.rs",
    "crates/engine-core/src/units/crt/fdiv.rs",
    "crates/engine-core/src/units/crt/math.rs",
    "crates/engine-core/src/units/crt/printf.rs",
    "crates/engine-core/src/units/crt/trig.rs",
    "crates/engine-havok/src/units/havok_sdk/hkalookatiksolver.rs",
    "crates/host/src/lib.rs",
    "crates/host/src/native_inflate.rs",
    "crates/engine/tests/diff_reference_peer.rs",
    "crates/engine/tests/havok_bodies.rs",
    "crates/engine/tests/real_scda.rs",
    "crates/engine/tests/typed_archives.rs",
    "crates/engine/tests/typed_script_lines.rs",
    "crates/engine/tests/typed_scripts.rs",
    "crates/esm/tests/cell_offsets_real.rs",
    "crates/host/tests/live_player_move.rs",
    "crates/host/tests/static_initializers.rs",
    "crates/nvse/src/decompile_tests.rs",
})
# Each value is the private input the workflow does not carry; it is recorded in docs/fork/TESTING.md.
UNCOVERED_DATA_FILES = {
    "crates/engine-platform/src/vorbisfile.rs": "NV_VORBIS_TEST_DIR",
    "crates/engine/tests/jip_call_arity.rs": "NV_PROBE",
    "crates/host/tests/live_movement_capture.rs": "NV_CAPTURE",
    "crates/nvse/src/compiler_tests.rs": "NV_COMPILE_PLUGINS",
    "crates/nvse/src/docs_tests.rs": "NV_XNVSE_DLL",
    "crates/nvse/src/newlang/compile_tests.rs": "NV_XNVSE_NEW_COMPILER_TESTS",
}


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

    def test_github_tokens_are_not_passed_to_the_child(self):
        with tempfile.TemporaryDirectory() as d:
            marker = Path(d) / "env"
            snippet = (
                "from pathlib import Path; import os; "
                f"Path({str(marker)!r}).write_text('clean' if not "
                "(os.environ.get('GITHUB_TOKEN') or os.environ.get('GH_TOKEN')) else 'leaked')"
            )
            command = ("env", [sys.executable, "-c", snippet])
            with mock.patch.dict(os.environ, {"GITHUB_TOKEN": "secret", "GH_TOKEN": "secret"}):
                code = private_run.run([command], Path(d), False)
            self.assertEqual(code, 0)
            self.assertEqual(marker.read_text(), "clean")


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

    def test_the_private_workflow_uses_the_runners_default_shell(self):
        # A self-hosted Windows runner is not guaranteed a bash on PATH; the steps are plain python3 commands.
        text = (FORK.parent.parent / ".github/workflows/fork-private.yml").read_text()
        self.assertNotIn("shell:", text)


class WiringTests(unittest.TestCase):
    ROOT = FORK.parent.parent
    DATA_READ = re.compile(r'require_data\(|\b(?:var|var_os)\s*\(\s*"NV_[A-Z_]+"\s*\)\s*\.expect\(')

    def task(self, name):
        lines = (self.ROOT / "Taskfile.yml").read_text().splitlines()
        start = next(i for i, line in enumerate(lines) if line.strip() == f"{name}:")
        block = []
        for line in lines[start + 1:]:
            if line and not line[0].isspace():
                break
            block.append(line)
        return "\n".join(block)

    def test_the_required_public_check_runs_the_integrity_check(self):
        scripts = self.task("scripts")
        self.assertIn("python3 -I scripts/fork/private_integrity.py --check", scripts)
        ci = (self.ROOT / ".github/workflows/fork-ci.yml").read_text()
        self.assertIn("run: mise exec -- task scripts", ci)

    def test_the_hash_and_its_files_are_owner_reviewed(self):
        owners = []
        for line in (self.ROOT / ".github/CODEOWNERS").read_text().splitlines():
            parts = line.split()
            if parts and not parts[0].startswith("#"):
                owners.append((parts[0], set(parts[1:])))
        paths = [".fork/private-integrity.sha256"]
        paths += [p for group in private_integrity.GROUPS.values() for p in group]
        for path in paths:
            with self.subTest(path=path):
                self.assertTrue(any(
                    (pattern == path or (pattern.endswith("/") and path.startswith(pattern)))
                    and "@ExtraToast" in who
                    for pattern, who in owners
                ), path)

    def test_the_codeowners_divergence_is_recorded(self):
        entry = self.ROOT / ".fork/allowed-divergence.d/codeowners-private-integrity"
        self.assertIn(".github/CODEOWNERS", entry.read_text())
        record = (self.ROOT / "docs/fork/divergence.d/codeowners-private-integrity.md").read_text()
        self.assertTrue(record.startswith("### "))
        for field in ("Files:", "Upstream approach:", "Fork approach:", "Decision:", "Reason:", "Evidence:"):
            with self.subTest(field=field):
                self.assertIn(f"- {field}", record)

    def test_the_protected_groups_are_pinned(self):
        self.assertEqual(set(private_integrity.GROUPS), {"judge", "thresholds", "controls", "ledger generator"})
        self.assertEqual(private_integrity.GROUPS["judge"], ("crates/engine-core/src/typed/diff.rs",))
        self.assertEqual(private_integrity.GROUPS["thresholds"],
                         ("crates/engine-core/src/typed/mod.rs", "crates/engine-core/src/x87.rs"))
        self.assertEqual(private_integrity.GROUPS["controls"], ("crates/engine/tests/diff_reference_peer.rs",))
        self.assertEqual(private_integrity.GROUPS["ledger generator"], (
            "scripts/ledger/Cargo.toml", "scripts/ledger/Cargo.lock",
            "scripts/ledger/function_evidence.tsv", "scripts/ledger/fallout_crates.tsv",
            "scripts/ledger/src/main.rs", "scripts/ledger/src/units.rs", "scripts/ledger/src/typed_marker.rs",
        ))

    def test_a_missing_protected_file_reports_no_environment_path(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            make_tree(root)
            (root / private_integrity.GROUPS["judge"][0]).unlink()
            ok, why = private_integrity.check(root)
            self.assertFalse(ok)
            self.assertNotIn(str(root), why)
            self.assertIn("judge", why)

    def test_every_typed_family_suite_runs_with_the_backend(self):
        commands = private_run.backend_commands(self.ROOT)
        self.assertEqual(len(commands), 3, "ordinary, data-gated typed suites and the real gaps")
        gaps = commands[2]
        self.assertIn("typed::backend_gaps", gaps[1])
        self.assertIn("NV_BACKEND_LIBRARY", gaps[2])
        self.assertEqual(gaps[2]["NV_BACKEND_GAPS"], "1")
        argv = commands[0][1]
        targets = {argv[i + 1] for i, word in enumerate(argv) if word == "--test"}
        components = (self.ROOT / ".fork" / "components.toml").read_text()
        expected = set(re.findall(r"--test (typed_[a-z_]+)", components))
        self.assertEqual(targets, expected)
        self.assertIn("NV_BACKEND_LIBRARY", commands[0][2])
        gated = {commands[1][1][i + 1] for i, word in enumerate(commands[1][1]) if word == "--test"}
        self.assertEqual(gated, {"typed_archives", "typed_script_lines", "typed_scripts"})

    def test_every_private_input_test_is_covered_or_documented(self):
        found = set()
        for path in (self.ROOT / "crates").glob("**/*.rs"):
            if self.DATA_READ.search(path.read_text(encoding="utf-8", errors="replace")):
                found.add(path.relative_to(self.ROOT).as_posix())
        self.assertEqual(found, COVERED_DATA_FILES | set(UNCOVERED_DATA_FILES),
                         "a new private-input test needs a private-run command or a documented reason")
        argv = " ".join(" ".join(cmd) for _, cmd in private_run.COMMANDS)
        for rel in COVERED_DATA_FILES:
            with self.subTest(file=rel):
                if "/tests/" in rel:
                    self.assertIn(Path(rel).stem, argv)
                else:
                    self.assertIn(f"-p {rel.split('/')[1]}", argv)
        for substring in ("matches_the_recorded_results_on_the_exes_tables",
                          "the_players_compiled_scripts_decompile_by_the_rules"):
            self.assertIn(substring, argv)
        for rel in COVERED_DATA_FILES:
            if not rel.startswith("crates/engine-core/src/"):
                continue
            lines = (self.ROOT / rel).read_text().splitlines()
            for i, line in enumerate(lines):
                if '#[ignore = "needs NV_EXE' in line:
                    name = next(l for l in lines[i + 1:i + 4] if l.strip().startswith("fn "))
                    self.assertIn("matches_the_recorded_results_on_the_exes_tables", name, rel)
        testing = (self.ROOT / "docs/fork/TESTING.md").read_text()
        for rel, variable in UNCOVERED_DATA_FILES.items():
            with self.subTest(file=rel):
                self.assertIn(rel, testing)
                self.assertIn(variable, testing)


if __name__ == "__main__":
    unittest.main()
