# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the fast-path gate (scripts/fork/port_check.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
No cargo and no network: the external tools are stand-ins, git runs on a temporary repository.
"""
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import port_check

GOOD_SUITE = "// suite\n"
FACEGEN = """\
// Typed from 00401280
pub fn empty_name(e: &mut Engine) -> u32 { 0 }

// Typed from 00867d60
pub fn calendar_day(e: &mut Engine) -> u32 { 1 }
"""
CALLER = """\
fn funcs() -> Vec<(u32, AbiFn)> {
    vec![
        entry!(0x0040_1280, facegen::empty_name(Ptr) -> u32),
    ]
}

fn run(e: &mut Engine) {
    let a = typed_call!(FAMILY, 0x0040_1280, facegen::empty_name(e));
    let b = typed_call!(
        FAMILY,
        0x0086_7d60,
        facegen::calendar_day(e)
    );
    let c = e.call(0x0040_1280, &[]); // facegen::empty_name(e) in a comment
    let d = facegen::empty_name(e);
}

#[cfg(test)]
mod tests {
    fn t(e: &mut Engine) { facegen::empty_name(e); }
}
"""


def write(root, rel, text):
    path = Path(root) / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def git(cwd, *args):
    env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.com",
               GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@example.com",
               GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull)
    done = subprocess.run(["git", *args], cwd=str(cwd), env=env, check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    return done.stdout


class Runner:
    """A stand-in for every external command: answers by the program or script it is asked for."""

    def __init__(self, answers=None, real_git=False):
        self.answers, self.calls, self.real_git = answers or {}, [], real_git

    def __call__(self, argv, cwd):
        argv = [str(a) for a in argv]
        self.calls.append(argv)
        if self.real_git and argv[0] == "git":
            return port_check.run(argv, cwd)
        for key, answer in self.answers.items():
            if key in " ".join(argv):
                return answer
        return 0, ""


class Helpers(unittest.TestCase):
    def test_a_finding_carries_rule_place_message_and_fix(self):
        self.assertEqual(port_check.finding("r", Path("a.rs"), "3", "m", "f"),
                         {"rule": "r", "file": "a.rs", "line": 3, "message": "m", "fix": "f"})

    def test_run_returns_status_and_output_and_127_for_a_missing_program(self):
        self.assertEqual(port_check.run([sys.executable, "-c", "print('hi')"], "."), (0, "hi\n"))
        code, output = port_check.run([sys.executable, "-c", "import sys; sys.exit(3)"], ".")
        self.assertEqual(code, 3)
        code, output = port_check.run(["no-such-program-here"], ".")
        self.assertEqual(code, 127)
        self.assertIn("no-such-program-here", output)

    def test_code_lines_stop_at_the_test_module_and_drop_comments(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write(tmp, "a.rs", "let x = 1; // note\n// whole\n#[cfg(test)]\nmod t {}\n")
            self.assertEqual(port_check.code_lines(path), [(1, "let x = 1; "), (2, "")])

    def test_a_mod_rs_is_its_folder_and_any_other_file_its_stem(self):
        self.assertEqual(port_check.module_of(Path("x/facegen/mod.rs")), "facegen")
        self.assertEqual(port_check.module_of(Path("x/facegen/morph.rs")), "morph")

    def test_from_tool_is_silent_on_success_and_reads_the_first_location_on_failure(self):
        self.assertEqual(port_check.from_tool("r", 0, "anything", "fix", "d"), [])
        got = port_check.from_tool("r", 1, "noise\nerror at crates/a/src/b.rs:12:4\nlast line\n", "fix", "d")
        self.assertEqual((got[0]["file"], got[0]["line"], got[0]["message"], got[0]["fix"]),
                         ("crates/a/src/b.rs", 12, "last line", "fix"))

    def test_from_tool_falls_back_to_the_default_file_and_the_exit_status(self):
        got = port_check.from_tool("r", 2, "", "fix", "d.toml")
        self.assertEqual((got[0]["file"], got[0]["line"], got[0]["message"]), ("d.toml", 1, "exit status 2"))

    def test_a_missing_tool_is_its_own_rule(self):
        got = port_check.from_tool("r", 127, "rustfmt: not found", "fix", "d")
        self.assertEqual((got[0]["rule"], got[0]["message"]), ("tool-missing", "rustfmt: not found"))
        self.assertEqual(port_check.from_tool("r", 127, "", "fix", "d")[0]["message"], "tool not found")


class Scans(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        write(self.root, "crates/engine-typed/src/facegen/mod.rs", FACEGEN)

    def test_registered_functions_are_read_from_the_typed_markers(self):
        write(self.root, "crates/engine-typed/src/lone.rs", "// Typed from 00000001\n// Typed from 00000002\n")
        self.assertEqual(port_check.registered_functions(self.root), {
            ("facegen", "empty_name"): "00401280", ("facegen", "calendar_day"): "00867d60"})

    def test_a_marker_belongs_to_the_function_below_it_and_not_to_the_next_markers(self):
        write(self.root, "crates/engine-typed/src/lone.rs",
              "// Typed from 00000001\n// Typed from 00000002\npub fn two() {}\n"
              "#[cfg(test)]\nmod t {\n    // Typed from 00000003\n    fn three() {}\n}\n")
        got = port_check.registered_functions(self.root)
        self.assertEqual(got[("lone", "two")], "00000002")
        self.assertNotIn(("lone", "three"), got)
        self.assertEqual(sum(1 for key in got if key[0] == "lone"), 1)

    def test_a_call_with_a_turbofish_is_still_a_plain_call(self):
        write(self.root, "crates/engine-typed/src/caller.rs", "fn f(e: &mut E) {\n    facegen::empty_name::<u32>(e);\n}\n")
        self.assertEqual([f["line"] for f in port_check.scan_typed_calls(self.root)], [2])

    def test_a_typed_call_may_start_a_few_lines_above_the_call(self):
        write(self.root, "crates/engine-typed/src/caller.rs",
              "fn f(e: &mut E) {\n    typed_call!(\n        FAMILY,\n        0x0040_1280,\n        // the leaf\n"
              "        // of the family\n        facegen::empty_name(\n            e,\n        )\n    );\n}\n")
        self.assertEqual(port_check.scan_typed_calls(self.root), [])

    def test_a_plain_call_of_a_registered_function_is_found_with_its_address(self):
        write(self.root, "crates/engine-typed/src/caller.rs", CALLER)
        found = port_check.scan_typed_calls(self.root)
        self.assertEqual([(f["rule"], f["file"], f["line"]) for f in found],
                         [("typed-call-plain", "crates/engine-typed/src/caller.rs", 15)])
        self.assertIn("00401280", found[0]["message"])

    def test_calls_in_entry_typed_call_and_e_call_statements_are_fine(self):
        write(self.root, "crates/engine-typed/src/caller.rs", CALLER.replace("    let d = facegen::empty_name(e);\n", ""))
        self.assertEqual(port_check.scan_typed_calls(self.root), [])

    def test_a_call_inside_the_functions_own_module_is_fine(self):
        write(self.root, "crates/engine-typed/src/facegen/more.rs", "fn f(e: &mut E) { facegen::empty_name(e); }\n")
        found = port_check.scan_typed_calls(self.root)
        self.assertEqual([f["file"] for f in found], ["crates/engine-typed/src/facegen/more.rs"])
        write(self.root, "crates/engine-typed/src/facegen/mod.rs", FACEGEN + "fn g(e: &mut E) { facegen::empty_name(e); }\n")
        self.assertEqual(len(port_check.scan_typed_calls(self.root)), 1)

    def test_a_typed_call_needs_a_literal_address(self):
        write(self.root, "crates/engine-typed/src/caller.rs",
              "fn f(e: &mut E) {\n    typed_call!(FAMILY, HOOKED, a::b(e));\n    typed_call![F, 0x12, a::b(e)];\n"
              "    typed_call!(@bind $family, $addr);\n}\n")
        found = port_check.scan_typed_calls(self.root)
        self.assertEqual([(f["rule"], f["line"]) for f in found], [("typed-call-address", 2)])
        self.assertIn("HOOKED", found[0]["message"])

    def test_family_ids_must_be_unique_and_below_64(self):
        write(self.root, "crates/engine-typed/src/a/mod.rs", "pub const FAMILY: FamilyId = FamilyId::new(7);\n")
        write(self.root, "crates/engine-typed/src/b/mod.rs", "pub const FAMILY: FamilyId = FamilyId::new(7);\n")
        write(self.root, "crates/engine-typed/src/c/mod.rs", "pub const FAMILY: FamilyId = FamilyId::new(64);\n")
        write(self.root, "crates/engine-typed/src/d/mod.rs", "pub const FAMILY: FamilyId = FamilyId::new(63);\n")
        found = port_check.scan_family_ids(self.root)
        self.assertEqual([(f["rule"], f["file"]) for f in found], [
            ("family-id-unique", "crates/engine-typed/src/b/mod.rs"),
            ("family-id-cap", "crates/engine-typed/src/c/mod.rs")])
        self.assertIn("src/a/mod.rs:1", found[0]["message"])

    def test_family_ids_in_tests_and_docs_are_not_families(self):
        write(self.root, "crates/engine-core/src/typed/mod.rs",
              "/// const PAST: FamilyId = FamilyId::new(64);\npub const FAMILY: FamilyId = FamilyId::new(1);\n"
              "#[cfg(test)]\nmod t {\n    pub const FAMILY: FamilyId = FamilyId::new(1);\n}\n")
        self.assertEqual(port_check.scan_family_ids(self.root), [])


class GitRepoCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        write(self.root, "crates/engine-typed/Cargo.toml", '[package]\nname = "engine-typed"\n')
        write(self.root, "crates/engine/Cargo.toml", '[package]\nname = "engine"\n')
        write(self.root, "crates/engine/tests/typed_fx.rs", GOOD_SUITE)
        write(self.root, "crates/engine-typed/src/facegen/mod.rs", FACEGEN)
        git(self.root, "init", "-q", "-b", "base")
        git(self.root, "add", "-A")
        git(self.root, "commit", "-qm", "base")
        git(self.root, "checkout", "-qb", "work")

    def commit(self, rel, text):
        write(self.root, rel, text)
        git(self.root, "add", "-A")
        git(self.root, "commit", "-qm", "work")


class Repo(GitRepoCase):
    def test_changed_files_are_the_branch_against_its_base(self):
        self.commit("crates/engine-typed/src/new.rs", "fn a() {}\n")
        self.assertEqual(port_check.changed_files(self.root, "base", port_check.run),
                         ["crates/engine-typed/src/new.rs"])
        self.commit("docs/a name with spaces.md", "x\n")
        self.assertIn("docs/a name with spaces.md", port_check.changed_files(self.root, "base", port_check.run))
        self.assertIsNone(port_check.changed_files(self.root, "no-such-ref", port_check.run))

    def test_touched_packages_come_from_the_manifests_of_changed_crates(self):
        files = ["crates/engine-typed/src/a.rs", "crates/engine/tests/x.rs", "crates/ghost/src/a.rs", "docs/a.md"]
        self.assertEqual(port_check.touched_packages(self.root, files), ["engine", "engine-typed"])
        write(self.root, "crates/odd/Cargo.toml", "[workspace]\n")
        self.assertEqual(port_check.touched_packages(self.root, ["crates/odd/a.rs"]), [])
        (self.root / "crates" / "bin").mkdir()
        (self.root / "crates" / "bin" / "Cargo.toml").write_bytes(b"\xff\xfe")
        self.assertEqual(port_check.touched_packages(self.root, ["crates/bin/a.rs"]), [])

    def test_format_checks_changed_rust_files_only(self):
        self.commit("crates/engine-typed/src/new.rs", "fn a() {}\n")
        runner = Runner(real_git=True)
        self.assertEqual(port_check.check_format(self.root, "base", runner), [])
        self.assertEqual(runner.calls[-1][:5], ["rustfmt", "--check", "--edition", "2021", "--"])
        self.assertEqual(runner.calls[-1][5:], ["crates/engine-typed/src/new.rs"])

    def test_format_has_nothing_to_check_without_changed_rust_files(self):
        self.commit("docs/a.md", "x\n")
        runner = Runner(real_git=True)
        self.assertEqual(port_check.check_format(self.root, "base", runner), [])
        self.assertTrue(all(c[0] == "git" for c in runner.calls))

    def test_format_names_each_unformatted_place(self):
        self.commit("crates/engine-typed/src/new.rs", "fn a( ) {}\n")
        out = "Diff in crates/engine-typed/src/new.rs:1:\n-fn a( ) {}\n+fn a() {}\nDiff in crates/engine-typed/src/new.rs:1:\n"
        runner = Runner({"rustfmt": (1, out)}, real_git=True)
        found = port_check.check_format(self.root, "base", runner)
        self.assertEqual([(f["rule"], f["file"], f["line"]) for f in found],
                         [("fmt", "crates/engine-typed/src/new.rs", 1)])

    def test_format_fails_when_rustfmt_is_missing_or_unreadable(self):
        self.commit("crates/engine-typed/src/new.rs", "fn a() {}\n")
        missing = port_check.check_format(self.root, "base", Runner({"rustfmt": (127, "rustfmt: gone")}, real_git=True))
        self.assertEqual(missing[0]["rule"], "tool-missing")
        odd = port_check.check_format(self.root, "base", Runner({"rustfmt": (1, "boom")}, real_git=True))
        self.assertEqual((odd[0]["rule"], odd[0]["file"]), ("fmt", "crates/engine-typed/src/new.rs"))

    def test_format_and_clippy_fail_on_an_unknown_base(self):
        for check in (port_check.check_format, port_check.check_clippy):
            found = check(self.root, "no-such-ref", Runner(real_git=True))
            self.assertEqual(found[0]["rule"], "git")

    def test_clippy_runs_through_the_daemon_for_the_touched_crates(self):
        self.commit("crates/engine-typed/src/new.rs", "fn a() {}\n")
        runner = Runner(real_git=True)
        self.assertEqual(port_check.check_clippy(self.root, "base", runner), [])
        argv = runner.calls[-1]
        self.assertTrue(argv[1].endswith("build_daemon.py"))
        self.assertEqual(argv[2:], ["run", "--", "clippy", "-p", "engine-typed", "--all-targets", "--", "-D", "warnings"])

    def test_clippy_reports_the_first_warning_place(self):
        self.commit("crates/engine-typed/src/new.rs", "fn a() {}\n")
        runner = Runner({"clippy": (101, "error: x\n --> crates/engine-typed/src/new.rs:9:5\n")}, real_git=True)
        found = port_check.check_clippy(self.root, "base", runner)
        self.assertEqual((found[0]["rule"], found[0]["file"], found[0]["line"]), ("clippy", "crates/engine-typed/src/new.rs", 9))

    def test_clippy_has_nothing_to_do_when_no_crate_changed(self):
        self.commit("docs/a.md", "x\n")
        runner = Runner(real_git=True)
        self.assertEqual(port_check.check_clippy(self.root, "base", runner), [])
        self.assertTrue(all(c[0] == "git" for c in runner.calls))

    def test_differential_passes_only_with_a_clean_exit_and_no_failed_case(self):
        self.assertEqual(port_check.check_differential(self.root, "fx", Runner()), [])
        self.assertEqual(port_check.check_differential(self.root, "fx", Runner({"family_lab": (0, "test a::b ... FAILED\n")}))[0]["rule"], "diff")

    def test_each_failed_case_is_a_finding_in_the_suite(self):
        out = "test one ... FAILED\ntest two ... ok\ntest three ... FAILED\n"
        found = port_check.check_differential(self.root, "fx", Runner({"family_lab": (101, out)}))
        self.assertEqual([(f["file"], f["message"][:10]) for f in found],
                         [("crates/engine/tests/typed_fx.rs", "case one d"), ("crates/engine/tests/typed_fx.rs", "case three")])

    def test_a_lab_failure_without_cases_is_still_a_failure(self):
        found = port_check.check_differential(self.root, "fx", Runner({"family_lab": (1, "error[E0432]: unresolved import\n")}))
        self.assertEqual((found[0]["rule"], found[0]["file"]), ("diff", "crates/engine/tests/typed_fx.rs"))
        self.assertEqual(port_check.check_differential(self.root, "fx", Runner({"family_lab": (127, "")}))[0]["rule"], "tool-missing")


class Gate(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        write(self.root, "crates/engine-typed/Cargo.toml", '[package]\nname = "engine-typed"\n')
        write(self.root, "crates/engine/tests/typed_fx.rs", GOOD_SUITE)
        write(self.root, "crates/engine-typed/src/facegen/mod.rs", FACEGEN)
        git(self.root, "init", "-q", "-b", "base")
        git(self.root, "add", "-A")
        git(self.root, "commit", "-qm", "base")

    def ids(self, results):
        return [name for name, _, _ in results]

    def test_the_static_gate_needs_no_cargo_and_no_network(self):
        runner = Runner(real_git=True)
        results = port_check.run_checks(self.root, "fx", "base", "static", runner)
        self.assertEqual(self.ids(results), ["family", "components", "private-integrity", "public-text",
                                             "typed-calls", "family-ids", "fmt"])
        joined = " ".join(" ".join(c) for c in runner.calls)
        self.assertNotIn("verify-sync", joined)
        self.assertNotIn("clippy", joined)
        self.assertNotIn("family_lab", joined)

    def test_a_saved_cargo_metadata_goes_to_components_check(self):
        runner = Runner(real_git=True)
        port_check.run_checks(self.root, "fx", "base", "static", runner, metadata="graph.json")
        components = next(c for c in runner.calls if c[1].endswith("components.py"))
        self.assertEqual(components[2:], ["check", "--metadata", "graph.json"])
        plain = Runner(real_git=True)
        port_check.run_checks(self.root, "fx", "base", "static", plain)
        self.assertEqual(next(c for c in plain.calls if c[1].endswith("components.py"))[2:], ["check"])

    def test_the_fast_gate_adds_verify_sync_clippy_and_the_differential_cases(self):
        results = port_check.run_checks(self.root, "fx", "base", "fast", Runner(real_git=True))
        self.assertEqual(self.ids(results)[-3:], ["verify-sync", "clippy", "differential"])

    def test_a_verify_sync_failure_names_both_ways_out(self):
        runner = Runner({"verify-sync.sh": (1, "DIVERGED from upstream, outside the overlay:\ncrates/a/src/b.rs\n")}, real_git=True)
        results = port_check.run_checks(self.root, "fx", "base", False, runner)
        found = next(found for name, found, _ in results if name == "verify-sync")
        self.assertEqual(found[0]["message"], "crates/a/src/b.rs")
        self.assertIn(".fork/overlay-paths", found[0]["fix"])
        self.assertIn(".fork/allowed-divergence.d/", found[0]["fix"])

    def test_an_unknown_family_stops_the_gate_at_once(self):
        for name in ("nope", "../x", ""):
            results = port_check.run_checks(self.root, name, "base", "fast", Runner(real_git=True))
            self.assertEqual(self.ids(results), ["family"])
            self.assertEqual(results[0][1][0]["rule"], "family-suite")

    def test_the_verdict_lists_checks_and_every_failure(self):
        found = [port_check.finding("r", "f.rs", 1, "m", "x")]
        got = port_check.verdict("fx", "fast", [("a", [], 0.123), ("b", found, 1.0)])
        self.assertEqual(got, {"family": "fx", "mode": "fast", "pass": False,
                               "checks": [{"id": "a", "ok": True, "seconds": 0.12}, {"id": "b", "ok": False, "seconds": 1.0}],
                               "failures": found})
        self.assertTrue(port_check.verdict("fx", "static", [("a", [], 0)])["pass"])
        self.assertEqual(port_check.verdict("fx", "static", [])["mode"], "static")

    def test_end_to_end_a_planted_violation_fails_with_file_line_and_fix(self):
        git(self.root, "checkout", "-qb", "work")
        write(self.root, "crates/engine-typed/src/caller.rs", CALLER)
        git(self.root, "add", "-A")
        git(self.root, "commit", "-qm", "work")
        out_file = self.root / "verdict.json"
        stream = io.StringIO()
        runner = Runner({"components.py": (1, "error: crate x is in no component\n")}, real_git=True)
        code = port_check.main(["fx", "--static", "--base", "base", "--out", str(out_file)],
                               root=self.root, runner=runner, out=stream)
        self.assertEqual(code, 1)
        got = json.loads(stream.getvalue())
        self.assertEqual(json.loads(out_file.read_text(encoding="utf-8")), got)
        self.assertFalse(got["pass"])
        self.assertEqual(got["mode"], "static")
        by_rule = {f["rule"]: f for f in got["failures"]}
        self.assertEqual(by_rule["typed-call-plain"]["file"], "crates/engine-typed/src/caller.rs")
        self.assertEqual(by_rule["typed-call-plain"]["line"], 15)
        self.assertIn("typed_call!(FAMILY, 0x00401280", by_rule["typed-call-plain"]["fix"])
        self.assertEqual(by_rule["components"]["file"], ".fork/components.toml")
        self.assertEqual(sorted(c["id"] for c in got["checks"] if not c["ok"]), ["components", "typed-calls"])

    def test_end_to_end_a_clean_attempt_passes_with_exit_zero(self):
        git(self.root, "checkout", "-qb", "work")
        stream = io.StringIO()
        code = port_check.main(["fx", "--static", "--base", "base"], root=self.root,
                               runner=Runner(real_git=True), out=stream)
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(stream.getvalue())["failures"], [])

    def test_the_script_runs_as_a_command_and_exits_one_on_an_unknown_family(self):
        done = subprocess.run([sys.executable, "-I", str(HERE.parent / "port_check.py"), "no_such_family", "--static"],
                              text=True, capture_output=True, check=False)
        self.assertEqual(done.returncode, 1)
        self.assertEqual(json.loads(done.stdout)["failures"][0]["rule"], "family-suite")

    def test_the_source_scans_pass_on_this_checkout(self):
        repo = HERE.parent.parent.parent
        self.assertEqual(port_check.scan_typed_calls(repo), [])
        self.assertEqual(port_check.scan_family_ids(repo), [])


LIB = "crates/engine-typed/src/facegen/more.rs"
SOURCE = "pub fn a(e: u32) -> u32 {\n    let x = e + 1;\n    x\n}\n// done\n"


def lcov(file, hits):
    """An lcov file with one record: hits maps a line to its hit count."""
    return f"SF:{file}\n" + "".join(f"DA:{n},{h}\n" for n, h in hits.items()) + "end_of_record\n"


class Measuring(Runner):
    """A runner whose llvm-cov commands write the lcov text they are given for their --output-path."""

    def __init__(self, text=None, code=0, **kwargs):
        super().__init__(**kwargs)
        self.text, self.code = text, code

    def __call__(self, argv, cwd):
        argv = [str(a) for a in argv]
        if "llvm-cov" in argv:
            self.calls.append(argv)
            if self.text is not None and "--output-path" in argv:
                Path(argv[argv.index("--output-path") + 1]).write_text(self.text, encoding="utf-8")
            return self.code, "error: tests failed\n" if self.code else ""
        return super().__call__(argv, cwd)


class Diffs(unittest.TestCase):
    def test_ranges_are_the_new_side_of_each_hunk(self):
        diff = ("+++ b/a.rs\n@@ -1,2 +1,3 @@\n@@ -9 +10 @@\n@@ -20,2 +30,0 @@\n+++ /dev/null\n@@ -1 +1 @@\n"
                "+++ b/b.rs\n@@ -0,0 +1,2 @@\n")
        self.assertEqual(port_check.parse_ranges(diff), {"a.rs": {1, 2, 3, 10}, "b.rs": {1, 2}})
        self.assertEqual(port_check.parse_ranges("@@ -1 +1 @@\n"), {})

    def test_a_changed_file_whose_name_has_spaces_is_read(self):
        with tempfile.TemporaryDirectory() as tmp:
            git(tmp, "init", "-q", "-b", "base")
            write(tmp, "a b.rs", "x\n")
            git(tmp, "add", "-A")
            git(tmp, "commit", "-qm", "base")
            git(tmp, "checkout", "-qb", "work")
            write(tmp, "a b.rs", "x\ny\n")
            git(tmp, "commit", "-qam", "work")
            self.assertEqual(port_check.changed_ranges(tmp, "base", port_check.run), {"a b.rs": {2}})

    def test_only_crate_sources_are_measured(self):
        ranges = {"crates/a/src/x.rs": {1}, "crates/a/tests/t.rs": {1}, "scripts/x.rs": {1}, "crates/a/benches/b.rs": {1}}
        self.assertEqual(port_check.library_ranges(ranges), {"crates/a/src/x.rs": {1}})

    def test_a_line_is_code_unless_blank_comment_or_brackets(self):
        for text, expected in (("", False), ("   ", False), ("// x", False), ("  }", False), ("  );", False),
                               ("let a = 1;", True), ("  x", True), ("}  // end", False), ("} else {", True)):
            self.assertEqual(port_check.looks_like_code(text), expected, text)


class Measure(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_lcov_files_merge_with_the_most_hits_and_unreadable_ones_are_skipped(self):
        a = write(self.root, "a.info", lcov("/x/crates/c/src/f.rs", {1: 0, 2: 3}))
        b = write(self.root, "b.info", lcov("/y/crates/c/src/f.rs", {1: 2, 3: 0}) + lcov("/y/crates/c/src/g.rs", {1: 1}))
        merged = port_check.merge_lcov([a, self.root / "missing.info", b])
        self.assertEqual(merged, {"crates/c/src/f.rs": {1: 2, 2: 3, 3: 0}, "crates/c/src/g.rs": {1: 1}})
        self.assertIsNone(port_check.merge_lcov([self.root / "missing.info"]))
        self.assertIsNone(port_check.merge_lcov([]))

    def test_changed_executable_lines_no_test_ran_are_findings(self):
        write(self.root, LIB, SOURCE)
        measured = {LIB: {1: 4, 2: 0, 3: 1}}
        found = port_check.coverage_gaps(self.root, {LIB: {1, 2, 3, 5}}, measured)
        self.assertEqual([(f["rule"], f["file"], f["line"]) for f in found], [("coverage", LIB, 2)])
        self.assertEqual(port_check.coverage_gaps(self.root, {LIB: {1, 3, 4, 5}}, measured), [])

    def test_changed_code_in_an_unmeasured_file_is_a_missing_baseline_of_its_own(self):
        write(self.root, LIB, SOURCE)
        found = port_check.coverage_gaps(self.root, {LIB: {4, 5, 2, 99}}, {})
        self.assertEqual([(f["rule"], f["line"]) for f in found], [("coverage-missing", 2)])

    def test_a_file_with_a_record_but_no_executable_line_is_not_measured_either(self):
        write(self.root, LIB, SOURCE)
        found = port_check.coverage_gaps(self.root, {LIB: {2}}, {LIB: {}})
        self.assertEqual([f["rule"] for f in found], ["coverage-missing"])

    def test_changed_comments_in_an_unmeasured_file_need_no_coverage(self):
        write(self.root, LIB, SOURCE)
        self.assertEqual(port_check.coverage_gaps(self.root, {LIB: {4, 5}}, {}), [])

    def test_the_lab_crate_is_measured_in_two_steps_and_so_is_every_other_touched_crate(self):
        out = self.root / "out"
        commands = port_check.coverage_commands(self.root, "fx", ["engine-typed", "engine-core"], out)
        self.assertEqual([str(lcov) for _, lcov in commands], [str(out / "lab.info"), str(out / "engine-core.info")])
        (run, report), (other,) = ([[str(a) for a in argv] for argv in steps] for steps, _ in commands)
        self.assertEqual(run[2:7], ["run", "--", "llvm-cov", "test", "--no-report"])
        self.assertIn("typed_fx", run)
        self.assertEqual([run[i + 1] for i, a in enumerate(run) if a == "--dep-coverage"], ["engine-typed", "engine-core"])
        self.assertEqual(report[4:6], ["llvm-cov", "report"])
        self.assertNotIn("--dep-coverage", report)
        for argv in (report, other):
            self.assertEqual(argv[argv.index("--ignore-filename-regex") + 1], port_check.COVERAGE_IGNORE)
        self.assertEqual(other[-2:], ["-p", "engine-core"])


class Full(GitRepoCase):
    def setUp(self):
        super().setUp()
        self.abs = f"{self.root}/{LIB}"

    def test_a_clean_covered_change_passes(self):
        self.commit(LIB, SOURCE)
        runner = Measuring(lcov(self.abs, {1: 2, 2: 2, 3: 2}), real_git=True)
        self.assertEqual(port_check.check_coverage(self.root, "fx", "base", runner), [])
        calls = [c for c in runner.calls if "llvm-cov" in c]
        self.assertEqual([c[c.index("llvm-cov") + 1] for c in calls], ["test", "report"])
        self.assertTrue(any("family_lab.py" in " ".join(c) and "generate" in c for c in runner.calls))

    def test_an_uncovered_changed_line_names_its_file_and_line(self):
        self.commit(LIB, SOURCE)
        found = port_check.check_coverage(self.root, "fx", "base", Measuring(lcov(self.abs, {1: 2, 2: 0, 3: 2}), real_git=True))
        self.assertEqual([(f["rule"], f["file"], f["line"]) for f in found], [("coverage", LIB, 2)])

    def test_no_lcov_data_is_a_failure_not_a_pass(self):
        self.commit(LIB, SOURCE)
        found = port_check.check_coverage(self.root, "fx", "base", Measuring(None, real_git=True))
        self.assertEqual([f["rule"] for f in found], ["coverage-missing-baseline"])

    def test_a_failing_coverage_run_is_a_failure_with_its_output(self):
        self.commit(LIB, SOURCE)
        found = port_check.check_coverage(self.root, "fx", "base", Measuring(None, code=101, real_git=True))
        self.assertEqual((found[0]["rule"], found[0]["message"]), ("coverage", "error: tests failed"))

    def test_a_failing_lab_generation_stops_the_coverage_check(self):
        self.commit(LIB, SOURCE)
        runner = Measuring(None, real_git=True, answers={"family_lab.py": (2, "no suite\n")})
        found = port_check.check_coverage(self.root, "fx", "base", runner)
        self.assertEqual(found[0]["rule"], "coverage")
        self.assertFalse([c for c in runner.calls if "llvm-cov" in c])

    def test_a_change_with_no_crate_source_needs_no_measurement(self):
        self.commit("docs/a.md", "x\n")
        runner = Measuring(None, real_git=True)
        self.assertEqual(port_check.check_coverage(self.root, "fx", "base", runner), [])
        self.assertFalse([c for c in runner.calls if "llvm-cov" in c])

    def test_an_unknown_base_fails_the_coverage_and_quality_checks(self):
        self.assertEqual(port_check.check_coverage(self.root, "fx", "nope", Runner(real_git=True))[0]["rule"], "git")
        self.assertEqual(port_check.check_quality(self.root, "nope", Runner(real_git=True))[0]["rule"], "git")

    def test_quality_fails_when_only_the_python_diff_fails(self):
        self.commit(LIB, SOURCE)

        class Flaky(Runner):
            def __call__(self, argv, cwd):
                if "*.py" in argv:
                    return 1, "fatal"
                return super().__call__(argv, cwd)

        found = port_check.check_quality(self.root, "base", Flaky(real_git=True))
        self.assertEqual([f["rule"] for f in found], ["git"])

    def quality(self, text):
        write(self.root, LIB, text)
        return port_check.scan_quality(self.root, {LIB: set(range(1, len(text.splitlines()) + 1))})

    def test_unsafe_needs_a_safety_comment_above_or_on_the_line(self):
        found = self.quality("fn a() {\n    unsafe { f() }\n}\n")
        self.assertEqual([(f["rule"], f["line"]) for f in found], [("quality-unsafe", 2)])
        self.assertEqual(self.quality("fn a() {\n    // SAFETY: f is fine\n    unsafe { f() }\n}\n"), [])
        self.assertEqual(self.quality("fn a() {\n    unsafe { f() } // SAFETY: fine\n}\n"), [])
        self.assertEqual(self.quality("// not unsafe at all\nfn a() {}\n"), [])

    def test_unwrap_and_expect_are_findings_unless_allowed_or_in_tests(self):
        found = self.quality("fn a() {\n    x.unwrap();\n    y.expect(\"m\");\n    z.unwrap(); // port-check: allow unwrap (never empty)\n}\n")
        self.assertEqual([(f["rule"], f["line"]) for f in found], [("quality-unwrap", 2), ("quality-unwrap", 3)])
        self.assertEqual(self.quality("fn a() {}\n#[cfg(test)]\nmod t {\n    fn b() { x.unwrap(); }\n}\n"), [])

    def test_only_changed_lines_are_judged(self):
        write(self.root, LIB, "fn a() {\n    x.unwrap();\n}\n")
        self.assertEqual(port_check.scan_quality(self.root, {LIB: {1}}), [])

    def test_a_function_over_the_limit_that_a_change_touches_is_too_long(self):
        body = "fn big() {\n" + "    step();\n" * 150 + "}\n"
        write(self.root, LIB, body + "fn small() {}\ntrait T {\n    fn decl();\n}\n")
        found = port_check.scan_quality(self.root, {LIB: {50}})
        self.assertEqual([(f["rule"], f["line"]) for f in found], [("quality-size", 1)])
        self.assertEqual(port_check.scan_quality(self.root, {LIB: {153}}), [])

    def test_function_spans_handle_declarations_and_nesting(self):
        lines = ["trait T {", "    fn decl();", "}", "fn a() {", "    if x {", "        y();", "    }", "}", "fn b() { z(); }"]
        self.assertEqual(port_check.function_spans(lines), [(4, 8), (9, 9)])

    def test_braces_in_strings_do_not_move_a_functions_end(self):
        lines = ['fn a() {', '    let s = "}}} {";', "    let c = '{';", "}", "fn b() {}"]
        self.assertEqual(port_check.function_spans(lines), [(1, 4), (5, 5)])

    def test_a_signature_over_several_lines_is_one_function(self):
        lines = ["fn a(", "    x: u32,", ") -> u32 {", "    x", "}"]
        self.assertEqual(port_check.function_spans(lines), [(1, 5)])
        self.assertEqual(port_check.function_spans(["fn open() {", "    x();"]), [])

    def test_quality_runs_ruff_on_changed_scripts_only(self):
        self.commit("scripts/fork/x.py", "x = 1\n")
        runner = Runner(real_git=True)
        self.assertEqual(port_check.check_quality(self.root, "base", runner), [])
        self.assertEqual(runner.calls[-1], ["ruff", "check", "--output-format", "concise", "scripts/fork/x.py"])
        failing = Runner({"ruff": (1, "scripts/fork/x.py:1:1: F401 unused\n")}, real_git=True)
        found = port_check.check_quality(self.root, "base", failing)
        self.assertEqual((found[0]["rule"], found[0]["file"], found[0]["line"]), ("ruff", "scripts/fork/x.py", 1))
        self.commit("other/y.py", "x = 1\n")
        quiet = Runner(real_git=True)
        self.assertEqual(port_check.check_quality(self.root, "base", quiet), [])
        self.assertEqual(sum(1 for c in quiet.calls if c[0] == "ruff"), 1)

    def test_a_pass_verdict_passes_and_a_fail_verdict_lists_its_findings(self):
        path = write(self.root, "v.json", json.dumps({"verdict": "pass", "findings": []}))
        self.assertEqual(port_check.check_review(path), [])
        write(self.root, "v.json", json.dumps({"verdict": "fail", "findings": [
            {"file": "a.rs", "line": 4, "issue": "off by one", "fix": "use <"}, "junk", {"issue": "late"}]}))
        found = port_check.check_review(path)
        self.assertEqual([(f["rule"], f["file"], f["line"], f["message"], f["fix"]) for f in found],
                         [("review", "a.rs", 4, "off by one", "use <"), ("review", "?", 1, "late", "")])

    def test_a_fail_verdict_without_findings_still_fails(self):
        path = write(self.root, "v.json", json.dumps({"verdict": "fail"}))
        self.assertEqual(port_check.check_review(path)[0]["rule"], "review")

    def test_a_missing_or_unreadable_verdict_is_absent_never_a_pass(self):
        for text in (None, "not json", "[]", json.dumps({"verdict": "maybe"}), json.dumps({"verdict": "pass", "findings": 3}), "{}"):
            path = write(self.root, "v.json", text) if text is not None else self.root / "none.json"
            self.assertEqual(port_check.check_review(path)[0]["rule"], "review-absent", text)
        self.assertEqual(port_check.check_review(None)[0]["rule"], "review-absent")

    def test_the_full_gate_adds_coverage_quality_and_review_to_the_fast_path(self):
        results = port_check.run_checks(self.root, "fx", "base", "full", Measuring(None, real_git=True))
        self.assertEqual([name for name, _, _ in results][-4:], ["differential", "coverage", "quality", "review"])

    def test_end_to_end_a_full_run_reports_an_uncovered_line_an_unwrap_and_a_missing_review(self):
        git(self.root, "checkout", "-qb", "feature")
        self.commit(LIB, "pub fn a(e: Option<u32>) -> u32 {\n    let x = e.unwrap();\n    x\n}\n")
        stream = io.StringIO()
        runner = Measuring(lcov(self.abs, {1: 1, 2: 1, 3: 0}), real_git=True)
        code = port_check.main(["fx", "--full", "--base", "base"], root=self.root, runner=runner, out=stream)
        got = json.loads(stream.getvalue())
        self.assertEqual((code, got["mode"], got["pass"]), (1, "full", False))
        by_rule = {f["rule"]: f for f in got["failures"]}
        self.assertEqual((by_rule["coverage"]["file"], by_rule["coverage"]["line"]), (LIB, 3))
        self.assertEqual(by_rule["quality-unwrap"]["line"], 2)
        self.assertIn("review-absent", by_rule)

    def test_the_modes_are_exclusive(self):
        with self.assertRaises(SystemExit):
            port_check.main(["fx", "--static", "--full"], root=self.root, runner=Runner(), out=io.StringIO())


if __name__ == "__main__":
    unittest.main()
