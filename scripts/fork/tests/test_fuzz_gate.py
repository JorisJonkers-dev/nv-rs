# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the fuzz gate (scripts/fork/fuzz_gate.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
No cargo: the family lab is a stand-in; git runs on a temporary repository.
"""
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import fuzz_gate

ENV = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.com", GIT_COMMITTER_NAME="t",
           GIT_COMMITTER_EMAIL="t@example.com", GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull)
SUITE = "// suite\npub fn replay_bytes(_b: &[u8]) {}\n"
PASSING = "test result: ok. 2 passed; 0 failed; 0 ignored\n"
FAILING = ("test fx::regression_aaaaaaaaaaaa ... FAILED\ntest fx::regression_bbbbbbbbbbbb ... ok\n"
           "test result: FAILED. 1 passed; 1 failed; 0 ignored\n")


def git(cwd, *args):
    subprocess.run(["git", *args], cwd=str(cwd), env=ENV, check=True, capture_output=True)


class Lab:
    """Real git, a stand-in for the family lab."""

    def __init__(self, lab=(0, PASSING)):
        self.lab, self.calls = lab, []

    def __call__(self, argv, cwd):
        argv = [str(a) for a in argv]
        self.calls.append(argv)
        if argv[0] == "git":
            return fuzz_gate.run(argv, cwd)
        return self.lab


class Repo(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        suite = self.root / "crates" / "engine" / "tests" / "typed_fx.rs"
        suite.parent.mkdir(parents=True)
        suite.write_text(SUITE, encoding="utf-8")
        git(self.root, "init", "-q")
        git(self.root, "add", "-A")
        git(self.root, "commit", "-qm", "base")
        self.suite = suite

    def add_tracked(self, rel):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"\x00\x01")
        git(self.root, "add", "-A")


class Check(Repo):
    def test_passing_regression_cases_pass_and_are_counted(self):
        lab = Lab()
        got = fuzz_gate.check(self.root, "fx", lab)
        self.assertEqual((got["pass"], got["regression_cases"], got["failures"]), (True, 2, []))
        self.assertEqual(lab.calls[-1][-2:], ["fx", "regression_"])

    def test_a_family_without_regression_cases_passes_with_none_counted(self):
        got = fuzz_gate.check(self.root, "fx", Lab((0, "running 0 tests\ntest result: ok. 0 passed; 0 failed\n")))
        self.assertEqual((got["pass"], got["regression_cases"]), (True, 0))

    def test_each_failing_saved_case_is_a_finding_in_the_suite(self):
        got = fuzz_gate.check(self.root, "fx", Lab((101, FAILING)))
        self.assertFalse(got["pass"])
        self.assertEqual([(f["rule"], f["file"]) for f in got["failures"]],
                         [("fuzz-regression", "crates/engine/tests/typed_fx.rs")])
        self.assertIn("regression_aaaaaaaaaaaa", got["failures"][0]["message"])

    def test_a_lab_failure_without_a_failed_case_still_fails(self):
        got = fuzz_gate.check(self.root, "fx", Lab((1, "error[E0432]: unresolved import\n")))
        self.assertEqual((got["pass"], got["failures"][0]["message"]), (False, "error[E0432]: unresolved import"))
        silent = fuzz_gate.check(self.root, "fx", Lab((3, "")))
        self.assertEqual(silent["failures"][0]["message"], "exit status 3")

    def test_a_failed_summary_fails_even_when_the_exit_status_is_zero(self):
        got = fuzz_gate.check(self.root, "fx", Lab((0, "test result: FAILED. 1 passed; 1 failed; 0 ignored\n")))
        self.assertEqual((got["pass"], got["failures"][0]["message"]), (False, "1 saved case(s) fail"))

    def test_a_missing_lab_is_a_failure(self):
        got = fuzz_gate.check(self.root, "fx", Lab((127, "python3: gone\n")))
        self.assertFalse(got["pass"])

    def test_a_tracked_corpus_or_artifact_fails_the_gate(self):
        self.add_tracked("fuzz/corpus/fx/seed1")
        self.add_tracked("crates/engine-typed/fuzz/artifacts/fx/crash-1")
        self.add_tracked("fuzz/fuzz_targets/ok.rs")
        got = fuzz_gate.check(self.root, "fx", Lab())
        self.assertEqual(sorted(f["file"] for f in got["failures"]),
                         ["crates/engine-typed/fuzz/artifacts/fx/crash-1", "fuzz/corpus/fx/seed1"])
        self.assertTrue(all(f["rule"] == "fuzz-corpus" for f in got["failures"]))

    def test_outside_a_git_checkout_the_gate_fails_instead_of_passing(self):
        with tempfile.TemporaryDirectory() as bare:
            suite = Path(bare) / "crates" / "engine" / "tests" / "typed_fx.rs"
            suite.parent.mkdir(parents=True)
            suite.write_text(SUITE, encoding="utf-8")
            got = fuzz_gate.check(Path(bare), "fx", Lab())
        self.assertEqual(got["failures"][0]["rule"], "git")

    def test_a_bad_family_name_is_refused(self):
        for name in ("../x", "a b", ""):
            with self.assertRaises(ValueError):
                fuzz_gate.check(self.root, name, Lab())

    def test_run_gives_127_for_a_missing_program(self):
        self.assertEqual(fuzz_gate.run([sys.executable, "-c", "print(1)"], ".")[0], 0)
        self.assertEqual(fuzz_gate.run(["no-such-program-here"], ".")[0], 127)


class ToTest(Repo):
    def test_a_finding_becomes_a_byte_literal_test_named_after_its_hash(self):
        name, new = fuzz_gate.to_test(self.root, "fx", bytes(range(14)))
        self.assertTrue(new)
        self.assertRegex(name, r"^regression_[0-9a-f]{12}$")
        text = self.suite.read_text(encoding="utf-8")
        self.assertIn(f"fn {name}()", text)
        self.assertIn("0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,\n", text)
        self.assertIn("0x0c, 0x0d,\n", text)
        self.assertIn("replay_bytes(bytes);", text)
        self.assertTrue(text.startswith(SUITE))

    def test_the_same_input_is_added_once(self):
        first = fuzz_gate.to_test(self.root, "fx", b"\x01\x02")
        again = fuzz_gate.to_test(self.root, "fx", b"\x01\x02")
        self.assertEqual((first[1], again), (True, (first[0], False)))
        self.assertEqual(self.suite.read_text(encoding="utf-8").count("#[test]"), 1)
        self.assertNotEqual(fuzz_gate.to_test(self.root, "fx", b"\x01\x03")[0], first[0])

    def test_a_comment_that_mentions_a_name_does_not_count_as_the_test(self):
        data = b"abc"
        name = "regression_" + __import__("hashlib").sha256(data).hexdigest()[:12]
        self.suite.write_text(SUITE + f"// fn {name}() was here\n", encoding="utf-8")
        self.assertEqual(fuzz_gate.to_test(self.root, "fx", data), (name, True))

    def test_a_suite_without_a_public_replay_function_is_refused(self):
        for text in ("fn replay_bytes(_b: &[u8]) {}\n", "// pub fn replay_bytes(x)\n", "// none\n"):
            self.suite.write_text(text, encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "pub fn replay_bytes"):
                fuzz_gate.to_test(self.root, "fx", b"\x01")
            with self.assertRaisesRegex(ValueError, "pub fn replay_bytes"):
                fuzz_gate.scaffold(self.root, "fx", self.root / "o")

    def test_an_empty_or_unminimised_input_is_refused(self):
        with self.assertRaises(ValueError):
            fuzz_gate.to_test(self.root, "fx", b"")
        with self.assertRaisesRegex(ValueError, "minimise"):
            fuzz_gate.to_test(self.root, "fx", bytes(513))
        self.assertEqual(fuzz_gate.to_test(self.root, "fx", bytes(512))[1], True)

    def test_a_suite_can_be_named_explicitly(self):
        other = self.root / "other.rs"
        other.write_text("pub fn replay_bytes(_b: &[u8]) {}", encoding="utf-8")
        fuzz_gate.to_test(self.root, "fx", b"\x09", suite=other)
        self.assertIn("#[test]", other.read_text(encoding="utf-8"))
        self.assertNotIn("#[test]", self.suite.read_text(encoding="utf-8"))


class LabParts:
    """A stand-in for family_lab: the parts a suite reaches, and the suite with its facade line assembled."""

    INCLUDE = re.compile(r'include!\(\s*"([^"]+)"')

    def __init__(self, parts=("engine-fallout-cell",)):
        self.parts, self.seen = list(parts), []

    def assembly_for(self, path):
        self.seen.append(Path(path))
        return self.parts

    def rewrite_source(self, text, parts):
        self.seen.append(parts)
        return text.replace("use engine as _;", "engine_core::assemble!(engine_typed::units::funcs);")


class Scaffold(Repo):
    def setUp(self):
        super().setUp()
        self.suite.write_text("use engine as _;\npub fn replay_bytes(_b: &[u8]) {}\n", encoding="utf-8")

    def test_the_family_gets_its_own_workspace_with_the_parts_its_cases_reach(self):
        out = self.root / "fuzz" / "families" / "fx"
        lab = LabParts(("engine-fallout-cell", "engine-bs"))
        written = fuzz_gate.scaffold(self.root, "fx", out, lab)
        self.assertEqual(written, ["Cargo.toml", "README.md", "fuzz_targets/fx.rs", "fuzz_targets/suite.rs"])
        manifest = (out / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn("[workspace]", manifest)
        self.assertIn('path = "../../../crates/engine-core"\nfeatures = ["test-support"]', manifest)
        self.assertIn('[dependencies.engine-typed]\npath = "../../../crates/engine-typed"', manifest)
        self.assertIn('[dependencies.engine-fallout-cell]\npath = "../../../crates/engine-fallout-cell"', manifest)
        self.assertIn('[dependencies.engine-bs]\npath = "../../../crates/engine-bs"', manifest)
        self.assertNotIn("[dependencies.engine]\n", manifest)  # the facade assembles every part: the lab's rule is its own
        (self.root / "crates" / "engine-core").mkdir(parents=True)
        dep = (re.search(r'\[dependencies.engine-core\]\npath = "([^"]+)"', manifest) or self.fail("no dep")).group(1)
        self.assertEqual((out / dep).resolve(), (self.root / "crates" / "engine-core").resolve())
        target = (out / "fuzz_targets" / "fx.rs").read_text(encoding="utf-8")
        self.assertIn("mod suite;", target)
        self.assertIn("suite::replay_bytes(data)", target)
        self.assertNotIn("#[path", target)
        copy = (out / "fuzz_targets" / "suite.rs").read_text(encoding="utf-8")
        self.assertIn("engine_core::assemble!(engine_typed::units::funcs);", copy)
        self.assertNotIn("use engine as _;", copy)
        self.assertEqual(lab.seen, [self.suite, ["engine-fallout-cell", "engine-bs"]])
        self.assertIn(f"cargo +{fuzz_gate.FUZZ_NIGHTLY} fuzz run fx", (out / "README.md").read_text(encoding="utf-8"))

    def test_a_table_the_suite_includes_is_copied_beside_it(self):
        self.suite.write_text('use engine as _;\nconst T: &[u32] = include!("fx_table.in");\npub fn replay_bytes(_b: &[u8]) {}\n',
                              encoding="utf-8")
        (self.suite.parent / "fx_table.in").write_text("&[1, 2]", encoding="utf-8")
        out = self.root / "o"
        written = fuzz_gate.scaffold(self.root, "fx", out, LabParts())
        self.assertIn("fuzz_targets/fx_table.in", written)
        self.assertEqual((out / "fuzz_targets" / "fx_table.in").read_text(encoding="utf-8"), "&[1, 2]")
        (self.suite.parent / "fx_table.in").unlink()
        with self.assertRaisesRegex(ValueError, "fx_table.in"):
            fuzz_gate.scaffold(self.root, "fx", out, LabParts())

    def test_a_directory_outside_the_repository_still_reaches_the_crates(self):
        (self.root / "crates" / "engine-core").mkdir(parents=True)
        with tempfile.TemporaryDirectory() as elsewhere:
            out = Path(elsewhere) / "x"
            fuzz_gate.scaffold(self.root, "fx", out, LabParts())
            manifest = (out / "Cargo.toml").read_text(encoding="utf-8")
            dep = (re.search(r'\[dependencies.engine-core\]\npath = "([^"]+)"', manifest) or self.fail("no dep")).group(1)
            self.assertEqual((out / dep).resolve(), (self.root / "crates" / "engine-core").resolve())

    def test_a_bad_family_name_is_refused(self):
        with self.assertRaises(ValueError):
            fuzz_gate.scaffold(self.root, "../x", self.root / "o", LabParts())

    def test_the_real_family_lab_is_the_default(self):
        self.assertIs((fuzz_gate.scaffold.__defaults__ or (None,))[0], fuzz_gate.family_lab)


class Main(Repo):
    def test_check_prints_the_verdict_and_exits_by_it(self):
        out = io.StringIO()
        self.assertEqual(fuzz_gate.main(["check", "fx"], self.root, Lab(), out), 0)
        self.assertTrue(json.loads(out.getvalue())["pass"])
        self.assertEqual(fuzz_gate.main(["check", "fx"], self.root, Lab((101, FAILING)), io.StringIO()), 1)

    def test_to_test_reads_the_input_file(self):
        data = self.root / "crash"
        data.write_bytes(b"\x05\x06")
        out = io.StringIO()
        self.assertEqual(fuzz_gate.main(["to-test", "fx", str(data)], self.root, Lab(), out), 0)
        self.assertTrue(json.loads(out.getvalue())["added"])

    def test_scaffold_lists_the_files(self):
        out = io.StringIO()
        self.assertEqual(fuzz_gate.main(["scaffold", "fx", str(self.root / "fz")], self.root, Lab(), out, LabParts()), 0)
        self.assertIn("Cargo.toml", json.loads(out.getvalue())["written"])

    def test_errors_exit_with_two(self):
        self.assertEqual(fuzz_gate.main(["check", "../x"], self.root, Lab(), io.StringIO()), 2)
        self.assertEqual(fuzz_gate.main(["to-test", "fx", str(self.root / "missing")], self.root, Lab(), io.StringIO()), 2)

    def test_the_script_runs_as_a_command(self):
        done = subprocess.run([sys.executable, "-I", str(HERE.parent / "fuzz_gate.py"), "check", "no/such"],
                              capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 2)


if __name__ == "__main__":
    unittest.main()
