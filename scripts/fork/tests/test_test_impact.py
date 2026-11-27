# SPDX-License-Identifier: GPL-3.0-only
"""Test impact (docs/fork/CI.md): selecting jobs by the map, every fallback, and the collector's parsing and merge."""
import json
import os
import shutil
import sys
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import components  # noqa: E402
import test_impact  # noqa: E402
from test_components import comp, model, workspace  # noqa: E402

NOW = datetime(2026, 11, 27, 12, 0, tzinfo=timezone.utc)
STAMP = (NOW - timedelta(hours=10)).strftime("%Y-%m-%dT%H:%M:%SZ")


def good_map(**kw):
    m = {"version": 1, "commit": "abc", "generated": STAMP,
         "known": ["crates/a/src/x.rs", "crates/a/src/types.rs", "crates/a/src/mac.rs", "crates/a/src/gone.rs", "crates/b/src/y.rs", "crates/a/src/win.rs"],
         "jobs": {"ca": {"files": ["crates/a/src/x.rs", "crates/a/src/mac.rs", "crates/a/src/gone.rs", "crates/a/src/win.rs"]},
                  "cb": {"files": ["crates/b/src/y.rs", "crates/a/src/x.rs"]}}}
    m.update(kw)
    return m


class Usable(unittest.TestCase):
    def ok(self, impact, ancestor=True):
        return components.impact_usable(impact, lambda sha: ancestor, now=NOW)

    def test_a_fresh_map_of_an_ancestor_is_used(self):
        self.assertIsNotNone(self.ok(good_map())[0])

    def test_every_fallback_says_why(self):
        for bad, why in ((None, "no test-impact map"), (good_map(version=2), "unknown format"), (good_map(generated="soon"), "no readable date"),
                         (good_map(generated=(NOW - timedelta(days=9)).isoformat()), "older than")):
            got, reason = self.ok(bad)
            self.assertIsNone(got)
            self.assertIn(why, reason)
        got, reason = self.ok(good_map(), ancestor=False)
        self.assertIsNone(got)
        self.assertIn("does not descend", reason)


class Selection(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)
        src = self.tmp / "crates" / "a" / "src"
        src.mkdir(parents=True)
        for n, text in (("x.rs", "fn x() {}\n"), ("types.rs", "struct T;\n"), ("mac.rs", "macro_rules! m { () => {} }\n"),
                        ("win.rs", "#[cfg(windows)]\nfn w() {}\n")):
            (src / n).write_text(text)
        (self.tmp / "crates" / "b" / "src").mkdir(parents=True)
        (self.tmp / "crates" / "b" / "src" / "y.rs").write_text("fn y() {}\n")
        meta = workspace({"a": [], "b": ["a"]})
        self.m = model([comp("ca", ["a"], runs_on="both"), comp("cb", ["b"], ["ca"], runs_on="both")], meta, root=str(self.tmp), linux_ok=["a", "b"])

    def names(self, files, impact=None, **kw):
        plan = self.m.plan(False, files, True, False, impact, **kw)
        return sorted(e["component"] for e in plan["include"]), plan

    def test_without_a_map_a_changed_crate_reaches_its_dependents(self):
        self.assertEqual(self.names(["crates/a/src/x.rs"])[0], ["ca", "cb"])

    def test_with_the_map_only_the_jobs_that_execute_the_file_run(self):
        self.assertEqual(self.names(["crates/a/src/x.rs"], good_map())[0], ["ca", "cb"])  # both execute x.rs
        self.assertEqual(self.names(["crates/b/src/y.rs"], good_map())[0], ["cb"])
        got, plan = self.names(["crates/b/src/y.rs"], good_map())
        self.assertEqual(plan["impact"]["mapped_files"], 1)

    def test_a_file_that_only_one_job_executes_runs_that_job_only(self):
        impact = good_map(jobs={"ca": {"files": ["crates/a/src/x.rs"]}, "cb": {"files": ["crates/b/src/y.rs"]}})
        self.assertEqual(self.names(["crates/a/src/x.rs"], impact)[0], ["ca"])

    def test_the_fallbacks_select_by_crate(self):
        impact = good_map(jobs={"ca": {"files": ["crates/a/src/x.rs", "crates/a/src/mac.rs", "crates/a/src/win.rs", "crates/a/src/gone.rs"]},
                                "cb": {"files": ["crates/b/src/y.rs"]}})
        by_crate = ["ca", "cb"]
        (self.tmp / "crates" / "a" / "src" / "new.rs").write_text("fn n() {}\n")
        for f in ("crates/a/src/new.rs",  # a new file: not in the map
                  "crates/a/src/types.rs",  # no job executes it
                  "crates/a/src/mac.rs",  # defines a macro
                  "crates/a/src/win.rs"):  # cfg(windows)
            self.assertEqual(self.names([f], impact)[0], by_crate, f)
        self.assertEqual(self.names(["crates/a/src/gone.rs"], impact, exists=lambda f: False)[0], by_crate)  # deleted or renamed

    def test_cargo_and_toolchain_changes_run_everything(self):
        for f in ("Cargo.lock", "rust-toolchain.toml"):
            got, plan = self.names([f], good_map())
            self.assertEqual(got, ["ca", "cb"], f)

    def test_a_job_the_map_does_not_know_is_selected_by_the_crate_of_the_mapped_file(self):
        impact = good_map(jobs={"ca": {"files": ["crates/a/src/x.rs"]}})
        self.assertEqual(self.names(["crates/a/src/x.rs"], impact)[0], ["ca", "cb"])

    def test_the_whole_run_ignores_the_map(self):
        plan = self.m.plan(True, [], True, True, good_map())
        self.assertEqual(sorted(e["component"] for e in plan["include"]), ["ca", "cb"])
        self.assertFalse(plan["impact"]["used"])

    def test_the_impact_plan_lists_one_linux_job_per_component_with_tests(self):
        jobs = self.m.impact_jobs()
        self.assertEqual([j["key"] for j in jobs], ["ca", "cb"])
        self.assertTrue(all(j["os"] == "ubuntu-latest" and j["runs"][0].startswith("cargo test") for j in jobs))


class Collector(unittest.TestCase):
    def test_executed_files_are_those_with_a_hit_in_the_repository(self):
        lcov = "SF:/r/crates/a/src/x.rs\nDA:1,0\nDA:2,3\nend_of_record\nSF:/r/crates/a/src/cold.rs\nDA:1,0\nend_of_record\nSF:/home/u/.cargo/registry/dep.rs\nDA:1,9\nend_of_record\n"
        self.assertEqual(test_impact.executed_files(lcov, "/r"), {"crates/a/src/x.rs"})

    def test_a_test_command_is_one_run_per_binary(self):
        got = test_impact.binaries("cargo test --locked -p engine --test one --test two --config x=1")
        self.assertEqual([label for label, _ in got], ["-p engine --test one", "-p engine --test two"])
        self.assertEqual(test_impact.binaries("cargo test --locked -p engine --doc"), [])
        self.assertEqual([a for _, a in test_impact.binaries("cargo test --locked -p a --lib --test t")][0], ["--locked", "-p", "a", "--lib"])

    def test_merge_leaves_a_job_with_a_failed_binary_out_and_keeps_the_rest(self):
        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, tmp, True)
        (tmp / "ok.json").write_text(json.dumps({"key": "ok", "os": "ubuntu-latest", "binaries": {"-p a": {"status": "ok", "files": ["crates/a/src/x.rs"]}}}))
        (tmp / "bad.json").write_text(json.dumps({"key": "bad", "os": "ubuntu-latest", "binaries": {"-p b": {"status": "tests failed", "files": ["crates/b/src/y.rs"]}}}))
        class A:  # noqa: N801
            dir, commit, out = str(tmp), "abc123", str(tmp / "map" / "m.json")
        os.makedirs(tmp / "map")
        cwd = os.getcwd()
        os.chdir(Path(__file__).resolve().parents[3])
        try:
            test_impact.merge(A)
        finally:
            os.chdir(cwd)
        got = json.loads((tmp / "map" / "m.json").read_text())
        self.assertEqual(sorted(got["jobs"]), ["ok"])
        self.assertIn("bad", got["skipped"])
        self.assertEqual(got["version"], 1)
        self.assertTrue(got["known"])


if __name__ == "__main__":
    unittest.main()
