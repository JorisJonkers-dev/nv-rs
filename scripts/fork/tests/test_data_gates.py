"""Tests of check_data_gates.py and gate_counts.py (the real-data gate, scripts/fork/test_gate.rs).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v        (no network, no cargo, nothing created)
"""
import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
FORK = HERE.parent
REPO = FORK.parent.parent
sys.path.insert(0, str(FORK))

import check_data_gates  # noqa: E402
import gate_counts  # noqa: E402


def bad(source):
    return check_data_gates.violations(source)


class LintTests(unittest.TestCase):
    def test_a_test_that_returns_silently_fails(self):
        self.assertTrue(bad('fn t() { let Ok(p) = std::env::var("NV_EXE") else {\n return;\n };\n }'))
        self.assertTrue(bad('fn t() { let Some(p) = std::env::var("NV_EXE").ok().filter(|p| !p.is_empty()) else { return }; }'))
        self.assertTrue(bad('fn e() -> Option<Engine> { let p = std::env::var_os("NV_DATA")?; None }'))
        self.assertTrue(bad('let d = std::env::var("NV_ORACLE_EXE").unwrap_or_default();'))
        self.assertTrue(bad('const X: Option<&str> = option_env!("NV_EXE");'))

    def test_the_gate_and_a_loud_read_pass(self):
        self.assertFalse(bad('let Some(p) = crate::test_gate::require_data("NV_EXE") else { return };'))
        self.assertFalse(bad('#[ignore = "needs NV_DATA"]\nfn t() { let d = PathBuf::from(std::env::var("NV_DATA").expect("NV_DATA")); }'))
        self.assertFalse(bad('let d = std::env::var("NV_EXE")\n    .unwrap();'))

    def test_comments_and_other_variables_are_not_reads(self):
        self.assertFalse(bad('//! NV_DATA="C:/x" cargo test; std::env::var("NV_EXE") in a comment'))
        self.assertFalse(bad('let a = std::env::var("NV_INFLATE_EVERY").ok();'))

    def test_it_reports_the_file_and_line(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "crates" / "x").mkdir(parents=True)
            (root / "crates" / "x" / "lib.rs").write_text('fn a() {}\nfn t() {\n let Ok(p) = std::env::var("NV_EXE") else { return };\n}\n')
            (root / "research" / "nv-oracle").mkdir(parents=True)
            (root / "research" / "nv-oracle" / "main.rs").write_text('fn m() { let Some(e) = std::env::var_os("NV_ORACLE_EXE") else { return }; }\n')
            problems = check_data_gates.check(root)
        self.assertEqual(len(problems), 1, problems)
        self.assertTrue(problems[0].startswith("crates/x/lib.rs:3:"), problems)

    def test_the_repository_passes(self):
        self.assertEqual(check_data_gates.check(REPO), [])

    def test_main_exit_codes(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "a.rs").write_text('fn t() { let Ok(p) = std::env::var("NV_EXE") else { return }; }')
            argv = sys.argv
            try:
                sys.argv = ["x", d]
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(check_data_gates.main(), 1)
            finally:
                sys.argv = argv


class CountTests(unittest.TestCase):
    OUT = [
        "running 3 tests",
        "executed: exe::tests::a (NV_EXE set)",
        "skipped: exe::tests::b (NV_EXE not set)",
        "skipped: exe::tests::b (NV_EXE not set)",
        "test exe::tests::a ... ok",
        "skipped: (unnamed) (NV_EXE not set)",
    ]

    def test_counts_by_test(self):
        executed, skipped = gate_counts.count(self.OUT)
        self.assertEqual(sorted(executed), ["exe::tests::a"])
        self.assertEqual(sorted(skipped), ["(unnamed)", "exe::tests::b"])

    def run_main(self, lines, *args):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write("\n".join(lines))
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = gate_counts.main([f.name, *args])
        Path(f.name).unlink()
        return code, out.getvalue(), err.getvalue()

    def test_require_fails_on_a_skip_or_nothing_executed(self):
        code, out, _ = self.run_main(self.OUT)
        self.assertEqual(code, 0)
        self.assertIn("1 executed, 2 skipped", out)
        self.assertEqual(self.run_main(self.OUT, "--require")[0], 1)
        self.assertEqual(self.run_main([], "--require")[0], 1)
        self.assertEqual(self.run_main(["executed: t (NV_EXE set)"], "--require")[0], 0)


if __name__ == "__main__":
    unittest.main()
