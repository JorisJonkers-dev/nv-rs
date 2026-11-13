"""Tests of the component map tooling: components.py, run-component.sh and the three summary scripts.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v        (no network, no cargo, nothing created)

Two kinds of tests: small synthetic workspaces that pin the rules (an unassigned crate fails, a duplicate fails,
...), and the real map (.fork/components.toml) against a stored copy of the workspace graph
(fixtures/workspace-graph.json, a reduced `cargo metadata --no-deps`; `components.py check` in the pipeline's
Plan job runs the same check against the live metadata).
"""
import contextlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
FORK = HERE.parent
REPO = FORK.parent.parent
sys.path.insert(0, str(FORK))

import clippy_annotations  # noqa: E402
import components  # noqa: E402
import libtest_summary  # noqa: E402
import pipeline_summary  # noqa: E402

FIXTURE = HERE / "fixtures" / "workspace-graph.json"


def workspace(deps, dev=None):
    """A cargo-metadata-shaped workspace: deps {crate: [crates]}, dev {crate: [crates]}; crates live in crates/<name>."""
    dev = dev or {}
    packages = []
    for name, ds in deps.items():
        entries = [{"name": d, "kind": None, "features": []} for d in ds]
        entries += [{"name": d, "kind": "dev", "features": []} for d in dev.get(name, [])]
        packages.append({"name": name, "manifest_path": f"/ws/crates/{name}/Cargo.toml", "features": [], "dependencies": entries})
    return {"workspace_root": "/ws", "packages": packages}


def comp(name, crates, requires=(), **kw):
    return dict({"name": name, "crates": list(crates), "requires": list(requires), "runs_on": "windows"}, **kw)


def model(cfg_components, meta, rules=(), root="/nonexistent", **kw):
    cfg = {"component": cfg_components, "rule": list(rules)}
    m = components.Model(cfg, meta, root=root, **kw)
    m.check()
    return m


def real_model(**kw):
    with open(REPO / ".fork" / "components.toml", "rb") as fh:
        cfg = components.tomllib.load(fh)
    with open(FIXTURE, encoding="utf-8") as fh:
        meta = json.load(fh)
    m = components.Model(cfg, meta, root=str(REPO), overlay=components.read_list(REPO / ".fork" / "overlay-paths"),
                         linux_ok=kw.get("linux_ok", components.read_list(REPO / ".fork" / "linux-ok.txt")))
    m.check()
    return m


class Rules(unittest.TestCase):
    META = workspace({"a": [], "b": ["a"], "c": ["b"]})

    def test_a_complete_map_passes(self):
        m = model([comp("low", ["a", "b"]), comp("high", ["c"], ["low"])], self.META)
        self.assertEqual(m.errors, [])

    def test_an_unassigned_crate_fails(self):
        m = model([comp("low", ["a", "b"])], self.META)
        self.assertTrue(any("c belongs to no component" in e for e in m.errors), m.errors)

    def test_an_unassigned_crate_claimed_by_a_rule_is_a_warning_not_silent(self):
        m = model([comp("low", ["a", "b"])], self.META, rules=[{"match": "c", "component": "low"}])
        self.assertEqual(m.errors, [])
        self.assertTrue(any("assigned by the rule" in w for w in m.warnings), m.warnings)
        self.assertIn("c", m.comps["low"]["crates"])  # so it is tested

    def test_a_rule_naming_an_unknown_component_fails(self):
        m = model([comp("low", ["a", "b"])], self.META, rules=[{"match": "c", "component": "nope"}])
        self.assertTrue(any("unknown component nope" in e for e in m.errors), m.errors)

    def test_a_duplicate_assignment_fails(self):
        m = model([comp("low", ["a", "b"]), comp("high", ["b", "c"], ["low"])], self.META)
        self.assertTrue(any("b is in both low and high" in e for e in m.errors), m.errors)

    def test_an_unknown_component_in_requires_fails(self):
        m = model([comp("low", ["a", "b"]), comp("high", ["c"], ["ghost"])], self.META)
        self.assertTrue(any("requires unknown component ghost" in e for e in m.errors), m.errors)

    def test_a_crate_that_is_not_in_the_workspace_fails(self):
        m = model([comp("low", ["a", "b", "zzz"]), comp("high", ["c"], ["low"])], self.META)
        self.assertTrue(any("zzz, which is not a workspace crate" in e for e in m.errors), m.errors)

    def test_a_dependency_outside_requires_fails(self):
        m = model([comp("low", ["a", "b"]), comp("high", ["c"], [])], self.META)
        self.assertTrue(any("c depends on b (component low), which high does not require" in e for e in m.errors), m.errors)

    def test_a_cycle_fails(self):
        m = model([comp("x", ["a", "b"], ["y"]), comp("y", ["c"], ["x"])], self.META)
        self.assertTrue(any("cycle" in e for e in m.errors), m.errors)

    def test_a_component_without_crates_or_commands_fails(self):
        m = model([comp("low", ["a", "b", "c"]), comp("empty", [])], self.META)
        self.assertTrue(any("no crates and no commands" in e for e in m.errors), m.errors)

    def test_runs_on_is_checked(self):
        m = model([comp("low", ["a", "b", "c"], runs_on="mac")], self.META)
        self.assertTrue(any("runs_on" in e for e in m.errors), m.errors)


class Jobs(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)
        (self.tmp / "crates" / "big" / "tests").mkdir(parents=True)
        for t in ("one", "two"):
            (self.tmp / "crates" / "big" / "tests" / f"{t}.rs").write_text("")
        (self.tmp / "crates" / "big" / "tests" / "dir").mkdir()
        (self.tmp / "crates" / "big" / "tests" / "dir" / "main.rs").write_text("")
        self.meta = workspace({"big": [], "small": []})

    def split(self, *job_args):
        jobs = [{"name": f"job {i}", "args": a} for i, a in enumerate(job_args)]
        return model([comp("c", ["big", "small"], job=jobs)], self.meta, root=str(self.tmp))

    def test_every_test_file_in_a_job_passes(self):
        m = self.split(["-p big --test one --test dir", "-p small"], ["-p big --test two --lib"])
        self.assertEqual(m.errors, [])
        self.assertEqual([e["name"] for e in m.entries("c")], ["Component: c / job 0 (Windows)", "Component: c / job 1 (Windows)"])

    def test_a_test_file_in_no_job_fails(self):
        m = self.split(["-p big --test one --test dir", "-p small"])
        self.assertTrue(any("big/tests/two.rs is in no job" in e for e in m.errors), m.errors)

    def test_a_job_naming_a_missing_file_fails(self):
        m = self.split(["-p big --test one --test two --test dir --test gone", "-p small"])
        self.assertTrue(any("--test gone" in e for e in m.errors), m.errors)

    def test_a_crate_in_no_job_fails(self):
        m = self.split(["-p big --test one --test two --test dir"])
        self.assertTrue(any("crate small is in no job" in e for e in m.errors), m.errors)

    def test_a_job_testing_a_foreign_crate_fails(self):
        meta = workspace({"big": [], "small": [], "other": []})
        m = model([comp("c", ["big", "small"], job=[{"name": "j", "args": ["-p big --test one --test two --test dir -p small -p other"]}]),
                   comp("o", ["other"])], meta, root=str(self.tmp))
        self.assertTrue(any("tests other, which is not a crate of c" in e for e in m.errors), m.errors)

    def test_two_jobs_with_the_same_name_fail(self):
        jobs = [{"name": "same", "args": ["-p big --test one --test two --test dir"]}, {"name": "same", "args": ["-p small"]}]
        m = model([comp("c", ["big", "small"], job=jobs)], self.meta, root=str(self.tmp))
        self.assertTrue(any("two jobs are named" in e for e in m.errors), m.errors)

    def test_examples_that_a_split_would_skip_fail(self):
        (self.tmp / "crates" / "big" / "examples").mkdir()
        m = self.split(["-p big --test one --test two --test dir", "-p small"])
        self.assertTrue(any("big/examples exists" in e for e in m.errors), m.errors)


class Matrix(unittest.TestCase):
    META = workspace({"a": [], "b": ["a"], "c": ["b"]})

    def cfg(self, **kw):
        return [comp("low", ["a", "b"], runs_on="both", **kw), comp("high", ["c"], ["low"])]

    def test_names_and_systems(self):
        m = model(self.cfg(), self.META)
        names = [(e["name"], e["os"], e["informational"]) for e in m.plan()["include"]]
        self.assertEqual(names, [("Component: low (Windows)", "windows-latest", False),
                                 ("Component: low (Linux, informational)", "ubuntu-latest", True),
                                 ("Component: high (Windows)", "windows-latest", False)])

    def test_the_command_is_cargo_test_of_the_crates_only(self):
        m = model(self.cfg(), self.META)
        self.assertEqual(m.plan()["include"][0]["script"], "cargo test --locked -p a -p b")

    def test_a_component_listed_in_linux_ok_runs_on_linux_only(self):
        m = model(self.cfg(), self.META, linux_ok=["a", "b"])
        self.assertEqual([(e["os"], e["informational"]) for e in m.entries("low")], [("ubuntu-latest", False)])

    def test_windows_required_keeps_the_windows_job(self):
        m = model(self.cfg(windows="required"), self.META, linux_ok=["a", "b"])
        self.assertEqual([(e["os"], e["informational"]) for e in m.entries("low")], [("windows-latest", False), ("ubuntu-latest", False)])

    def test_a_partly_listed_component_stays_windows_with_an_informational_twin(self):
        m = model(self.cfg(), self.META, linux_ok=["a"])
        self.assertEqual([(e["os"], e["informational"]) for e in m.entries("low")], [("windows-latest", False), ("ubuntu-latest", True)])

    def test_a_pull_request_runs_only_what_reaches_the_change(self):
        m = model(self.cfg(), self.META)
        plan = m.plan(False, ["crates/c/src/lib.rs"])
        self.assertEqual({e["component"] for e in plan["include"]}, {"high"})
        self.assertEqual([c["tested"] for c in plan["components"]], [False, True])
        plan = m.plan(False, ["crates/a/src/lib.rs"])  # a is a dependency of c: both run
        self.assertEqual({e["component"] for e in plan["include"]}, {"low", "high"})

    def test_pr_always_runs_regardless(self):
        m = model([comp("low", ["a", "b"], pr="always"), comp("high", ["c"], ["low"])], self.META)
        self.assertEqual({e["component"] for e in m.plan(False, ["crates/c/src/lib.rs"])["include"]}, {"low", "high"})

    def test_extra_commands_follow_the_tests(self):
        m = model([comp("low", ["a", "b", "c"], extra=["cargo clippy"])], self.META)
        self.assertEqual(m.plan()["include"][0]["script"], "cargo test --locked -p a -p b -p c\ncargo clippy")

    def test_commands_components_name_their_workspaces_for_the_cache(self):
        cmds = ["cargo test --locked --manifest-path scripts/x/Cargo.toml", "cargo fmt --manifest-path scripts/y/Cargo.toml"]
        m = model([comp("low", ["a", "b", "c"]), comp("own", [], commands=cmds, paths=["scripts/x/"])], self.META)
        entry = m.entries("own")[0]
        self.assertEqual(entry["script"], "\n".join(cmds))
        self.assertEqual(entry["workspaces"], "scripts/x -> target\nscripts/y -> target")

    def test_windows_gets_the_long_timeout(self):
        m = model(self.cfg(), self.META)
        self.assertEqual([e["timeout"] for e in m.entries("low")], [180, 90])

    def test_an_enabled_feature_is_passed_to_a_job_that_would_miss_it(self):
        meta = workspace({"a": [], "b": ["a"]})
        meta["packages"][0]["features"] = ["test-support"]
        meta["packages"][1]["dependencies"][0]["features"] = ["test-support"]  # b turns a's feature on
        m = model([comp("low", ["a"]), comp("high", ["b"], ["low"])], meta)
        self.assertEqual(m.entries("low")[0]["script"], "cargo test --locked -p a --features a/test-support")
        self.assertEqual(m.entries("high")[0]["script"], "cargo test --locked -p b")  # b gets it through its own dependency


class Affected(unittest.TestCase):
    def setUp(self):
        self.m = real_model()

    def reached(self, files):
        plan = self.m.plan(False, files)
        return plan, {c["component"] for c in plan["components"] if c["tested"]}

    def test_a_workspace_file_runs_everything(self):
        for f in ("Cargo.lock", "Cargo.toml", "rust-toolchain.toml", ".cargo/config.toml", ".fork/components.toml",
                  ".github/workflows/fork-build.yml", "scripts/fork/components.py"):
            plan, got = self.reached([f])
            self.assertEqual(got, set(self.m.comps), f)

    def test_documentation_and_fork_owned_files_run_nothing_but_the_always_components(self):
        for f in ("docs/fork/CI.md", "README.md", ".fork/overlay-paths", "scripts/fork/setup-repo.sh", ".github/workflows/fork-sync.yml"):
            self.assertEqual(self.reached([f])[1], {"repo-checks"}, f)

    def test_a_leaf_crate_reaches_few_components(self):
        self.assertEqual(self.reached(["crates/jip/src/lib.rs"])[1], {"scripting", "integration", "repo-checks"})

    def test_a_core_change_reaches_the_slow_jobs(self):
        plan, got = self.reached(["crates/engine-core/src/mem.rs"])
        self.assertIn("integration", got)
        self.assertEqual(sum(1 for e in plan["include"] if e["component"] == "integration"), 6)

    def test_a_fallout_change_reaches_every_component_whose_tests_link_it(self):
        _, got = self.reached(["crates/engine-fallout-cell/src/lib.rs"])
        self.assertTrue({"fallout-world", "fallout-gameplay", "fallout-front", "integration"} <= got, got)  # Phase A dev-dependencies
        self.assertNotIn("core", got)

    def test_the_vendored_wgpu_hal_reaches_the_crates_that_use_wgpu(self):
        _, got = self.reached(["vendor/wgpu-hal/src/lib.rs"])
        self.assertEqual(got, {"rendering", "platform", "integration", "repo-checks"})

    def test_the_launcher_and_the_distribution_scripts_run_their_component(self):
        for f in ("scripts/launcher/src/main.rs", "scripts/ledger/src/main.rs", "distribution/Settings.ps1"):
            self.assertIn("launcher-and-ledger", self.reached([f])[1], f)

    def test_a_file_outside_every_crate_runs_everything(self):
        plan, got = self.reached(["tools/new-thing.py"])
        self.assertEqual(got, set(self.m.comps))
        self.assertIn("tools/new-thing.py", plan["reason"])

    def test_a_push_runs_every_component(self):
        plan = self.m.plan()
        self.assertEqual({c["component"] for c in plan["components"] if c["tested"]}, set(self.m.comps))


class RealMap(unittest.TestCase):
    def test_the_map_is_valid_for_the_stored_workspace(self):
        m = real_model()
        self.assertEqual(m.errors, [])
        self.assertEqual(m.warnings, [])

    def test_every_workspace_crate_is_in_exactly_one_component(self):
        m = real_model()
        listed = [c for comp_ in m.comps.values() for c in comp_["crates"]]
        self.assertEqual(sorted(listed), sorted(m.pk))
        self.assertEqual(len(listed), len(set(listed)))

    def test_every_engine_test_file_is_in_a_job(self):
        m = real_model()
        named = {t for c in m.comps["integration"]["job"] for run in c["args"] for t in m.flags(run, "--test")}
        self.assertEqual(named, m.tests_of("engine"))

    def test_a_new_upstream_crate_fails_the_check(self):
        meta = json.loads(FIXTURE.read_text())
        meta["packages"].append({"name": "brand-new", "manifest_path": "/ws/crates/brand-new/Cargo.toml", "features": [], "dependencies": []})
        with open(REPO / ".fork" / "components.toml", "rb") as fh:
            cfg = components.tomllib.load(fh)
        m = components.Model(cfg, meta, root=str(REPO))
        m.check()
        self.assertTrue(any("brand-new belongs to no component" in e for e in m.errors), m.errors)

    def test_a_new_fallout_crate_is_assigned_by_rule_with_a_warning(self):
        meta = json.loads(FIXTURE.read_text())
        meta["packages"].append({"name": "engine-fallout-newarea", "manifest_path": "/ws/crates/engine-fallout-newarea/Cargo.toml",
                                 "features": [], "dependencies": [{"name": "engine-core", "kind": None, "features": []}]})
        with open(REPO / ".fork" / "components.toml", "rb") as fh:
            cfg = components.tomllib.load(fh)
        m = components.Model(cfg, meta, root=str(REPO))
        m.check()
        self.assertEqual(m.errors, [])
        self.assertEqual(m.owner["engine-fallout-newarea"], "fallout-front")
        self.assertTrue(m.warnings)

    def test_the_core_job_switches_on_the_feature_the_workspace_gives_engine_core(self):
        script = real_model().entries("core")[0]["script"]
        self.assertIn("--features engine-core/test-support", script)  # tests/typed_seam.rs is cfg(feature = "test-support")

    def test_the_old_shards_are_gone(self):
        self.assertFalse((REPO / ".fork" / "test-groups.json").exists())
        names = [e["name"] for e in real_model().plan()["include"]]
        self.assertFalse([n for n in names if "rest" in n.split() or "fallout-a" in n or "fallout-b" in n], names)
        self.assertTrue(all(n.startswith("Component: ") for n in names))

    def test_the_cli_prints_the_matrix_from_a_saved_graph(self):
        out = subprocess.run([sys.executable, "-I", str(FORK / "components.py"), "matrix", "--event", "push", "--metadata", str(FIXTURE)],
                             capture_output=True, text=True, cwd=REPO, check=True).stdout
        data = json.loads(out)
        self.assertEqual(len(data["include"]), 26)
        self.assertEqual({"name", "component", "cache", "os", "informational", "workspaces", "timeout", "script"}, set(data["include"][0]))


class Summaries(unittest.TestCase):
    LOG = """   Compiling x
     Running unittests src/lib.rs (target/debug/deps/x-1)
running 3 tests
test a ... ok
test b ... FAILED
test c ... ignored
test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 12.50s
   Doc-tests x
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
"""

    def run_main(self, module, argv):
        old, buf = sys.argv, io.StringIO()
        sys.argv = ["x"] + argv
        try:
            with contextlib.redirect_stdout(buf):
                module.main()
        finally:
            sys.argv = old
        return buf.getvalue()

    def test_libtest_summary_counts_binaries_failures_and_phases(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "t.log").write_text(self.LOG)
            (Path(d) / "timings.txt").write_text("compile 120\ntest 30\ntest 30\nother 5\n")
            out = self.run_main(libtest_summary, ["--title", "Component: x (Windows)", "--timings", f"{d}/timings.txt", f"{d}/t.log"])
        self.assertIn("### Component: x (Windows)", out)
        self.assertIn("| 2 | 5 | 1 | 1 | 2.0 min | 1.0 min |", out)
        self.assertIn("`b` in `unittests src/lib.rs`", out)

    def test_clippy_annotations_deduplicate_and_escape(self):
        msg = {"reason": "compiler-message", "message": {"level": "warning", "message": "bad\nthing", "rendered": "R",
               "code": {"code": "clippy::x"}, "spans": [{"is_primary": True, "file_name": "a\\b.rs", "line_start": 3, "column_start": 4}]}}
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "c.json").write_text(json.dumps(msg) + "\n" + json.dumps(msg) + "\nnot json\n")
            out = self.run_main(clippy_annotations, [f"{d}/c.json"])
        self.assertIn("clippy: 1 diagnostic(s)", out)
        self.assertIn("::warning file=a/b.rs,line=3,col=4,title=clippy::x::bad%0Athing", out)

    def test_pipeline_summary_groups_jobs_by_component_and_flags_informational_failures(self):
        def job(name, start, end, conclusion="success", steps=()):
            return {"name": name, "conclusion": conclusion, "created_at": "2026-11-13T10:00:00Z", "started_at": start,
                    "completed_at": end, "steps": [{"name": s, "conclusion": "failure"} for s in steps]}
        jobs = [
            job("Build / Component: core (Windows)", "2026-11-13T10:00:10Z", "2026-11-13T10:02:10Z"),
            job("Build / Component: core (Linux, informational)", "2026-11-13T10:00:10Z", "2026-11-13T10:01:00Z", steps=["Test core"]),
            job("Build / Component: integration / typed shader family (Windows)", "2026-11-13T10:00:10Z", "2026-11-13T11:00:10Z"),
            job("Build / Component: integration / typed animation (Windows)", "2026-11-13T10:00:10Z", "2026-11-13T10:20:10Z"),
            job("Build / Clippy (whole workspace)", "2026-11-13T10:00:10Z", "2026-11-13T10:08:10Z", "failure", ["Run clippy"]),
            job("Pipeline Complete", "2026-11-13T11:00:20Z", "2026-11-13T11:00:30Z"),
        ]
        out = pipeline_summary.render(jobs, ["Pipeline Complete"])
        self.assertIn("| integration | ok | 60m00s (integration / typed shader family (Windows)) | 2 |", out)
        self.assertIn("| core | ok (its Linux twin failed: informational) | 2m00s (core (Windows)) | 2 |", out)
        self.assertIn("Last job to finish: **Build / Component: integration / typed shader family (Windows)**, 60m10s", out)
        self.assertIn("- Build / Clippy (whole workspace): Run clippy", out)
        self.assertNotIn("Pipeline Complete", out)


@unittest.skipUnless(shutil.which("bash"), "needs bash")
class RunComponent(unittest.TestCase):
    def run_script(self, script, cargo_body):
        with tempfile.TemporaryDirectory() as d:
            fake = Path(d) / "cargo"
            fake.write_text("#!/bin/sh\n" + cargo_body)
            fake.chmod(0o755)
            env = dict(os.environ, PATH=f"{d}:{os.environ['PATH']}", SCRIPT=script, RUNNER_TEMP=d)
            done = subprocess.run(["bash", str(FORK / "run-component.sh")], capture_output=True, text=True, env=env, cwd=d)
            return done, (Path(d) / "test.log").read_text(), (Path(d) / "timings.txt").read_text()

    OK = 'case "$*" in *--no-run*) echo "  Executable a";; *) echo "     Running a"; echo "test a ... ok"; echo "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s";; esac\n'
    BAD = ('case "$*" in *--no-run*) echo "  Executable a";; *) echo "     Running a"; echo "test a ... FAILED"; '
           'echo "failures:"; echo "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s"; exit 101;; esac\n')

    def test_groups_the_compile_and_each_binary_and_succeeds(self):
        done, log, timings = self.run_script("cargo test --locked -p x", self.OK)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertIn("::group::compile: cargo test --locked -p x", done.stdout)
        self.assertIn("::group::     Running a", done.stdout)
        self.assertIn("compile ", timings)
        self.assertIn("test ", timings)
        self.assertIn("test result: ok", log)

    def test_a_failed_test_is_an_annotation_and_the_exit_status_is_failure(self):
        done, _, _ = self.run_script("cargo test --locked -p x", self.BAD)
        self.assertEqual(done.returncode, 1)
        self.assertIn("::error title=test failed::a", done.stdout)
        self.assertIn("::error title=tests failed::cargo test --locked -p x", done.stdout)

    def test_a_later_command_still_runs_after_a_failure(self):
        done, log, _ = self.run_script("cargo test --locked -p x\ncargo clippy", self.BAD)
        self.assertEqual(done.returncode, 1)
        self.assertIn("::group::cargo clippy", done.stdout)


if __name__ == "__main__":
    unittest.main()
