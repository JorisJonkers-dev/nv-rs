# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the scheduled family fuzz runs (scripts/fork/fuzz_schedule.py): a fake container, no cargo, no docker."""
import io
import json
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import fuzz_gate
import fuzz_schedule as fs
import verifier

SUITE = "use engine as _;\npub fn replay_bytes(_b: &[u8]) {}\n"
PLANTED = bytes([0, 0, 0, 0, 0, 0, 0, 0, 0x80, 7, 7])


class Lab:
    """A stand-in for the family lab."""

    INCLUDE = re.compile(r'include!\(\s*"([^"]+)"')

    def assembly_for(self, path: Path) -> list[str]:
        return ["engine-fallout-cell"]

    def rewrite_source(self, text: str, parts: list[str]) -> str:
        return text


class Clock:
    def __init__(self) -> None:
        self.t = 1000.0

    def __call__(self) -> float:
        self.t += 5
        return self.t


class Container:
    """A fake verifier container: remembers its commands, may leave crash artifacts in the crate."""

    def __init__(self, record: dict[str, Any] | None = None, crash: bytes | None = None, minimised: bytes | None = None,
                 fail_run: Exception | None = None) -> None:
        self.record = {"image": "img", "run_seconds": 3.0, "exit": 0, "peak_bytes": 5, "sccache_hits": 1,
                       "container_left": False, "output_tail": "", **(record or {})}
        self.crash, self.minimised, self.fail_run = crash, minimised, fail_run
        self.calls: list[tuple[Path, Path, list[str], dict[str, Any]]] = []

    def __call__(self, root: Path, seed: Path, command: list[str], **kw: Any) -> dict[str, Any]:
        self.calls.append((root, seed, command, kw))
        crate = Path(re.search(r"cd (\S+) &&", command[2]).group(1))  # type: ignore[union-attr]
        if self.fail_run is not None:
            raise self.fail_run
        if self.crash is not None:
            (crate / "artifacts" / "fx").mkdir(parents=True)
            (crate / "artifacts" / "fx" / "crash-abc").write_bytes(self.crash)
            if self.minimised is not None:
                (crate / "artifacts" / "fx" / "minimized-from-abc").write_bytes(self.minimised)
        return self.record


class Repo(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name) / "repo"
        tests = self.root / "crates" / "engine" / "tests"
        tests.mkdir(parents=True)
        (tests / "typed_fx.rs").write_text(SUITE, encoding="utf-8")
        (tests / "typed_aa.rs").write_text("// no replay here\n", encoding="utf-8")
        (tests / "typed_zz.rs").write_text(SUITE, encoding="utf-8")
        self.work = Path(self.tmp.name) / "work"
        self.clock = Clock()


class Listing(Repo):
    def test_only_suites_with_a_replay_function_can_be_fuzzed(self) -> None:
        self.assertEqual(fs.families(self.root), ["fx", "zz"])

    def test_the_oldest_run_is_next_and_a_family_never_run_comes_first(self) -> None:
        self.assertIsNone(fs.next_family([], {}))
        self.assertEqual(fs.next_family(["fx", "zz"], {}), "fx")
        self.assertEqual(fs.next_family(["fx", "zz"], {"fx": {"last": 50}}), "zz")
        self.assertEqual(fs.next_family(["fx", "zz"], {"fx": {"last": 50}, "zz": {"last": 40}}), "zz")

    def test_the_state_is_written_whole_and_an_unreadable_one_is_empty(self) -> None:
        path = self.work / "deep" / "state.json"
        self.assertEqual(fs.load_state(path), {})
        fs.save_state(path, {"fx": {"last": 1}})
        self.assertEqual(fs.load_state(path), {"fx": {"last": 1}})
        self.assertEqual([p.name for p in path.parent.iterdir()], ["state.json"])
        path.write_text("[1]", encoding="utf-8")
        self.assertEqual(fs.load_state(path), {})
        path.write_text("{", encoding="utf-8")
        self.assertEqual(fs.load_state(path), {})


class Pieces(unittest.TestCase):
    def test_the_command_names_the_pinned_nightly_a_fixed_length_and_a_limit_on_the_build(self) -> None:
        crate = Path("/r/target/fuzz-families/fx")
        run = fs.container_command("fx", 600, crate)
        self.assertEqual(run[:2], ["bash", "-c"])
        # cargo-fuzz looks for `fuzz/` in the project root unless told where the family's crate is
        self.assertIn(f"cargo +{fuzz_gate.FUZZ_NIGHTLY} fuzz run {fs.BUILD_FLAGS} --fuzz-dir {crate} fx -- -max_total_time=600", run[2])
        self.assertIn("timeout 4200 ", run[2])
        self.assertTrue(run[2].startswith(f"cd {crate} && "))
        self.assertIn(f"cargo +{fuzz_gate.FUZZ_NIGHTLY} fuzz tmin {fs.BUILD_FLAGS} --fuzz-dir {crate} fx", run[2])
        self.assertIn("--dev --sanitizer none", run[2])  # the build that fits: see BUILD_FLAGS
        self.assertTrue(run[2].endswith("exit $status"))  # the run's own status, not the minimiser's

    @unittest.skipUnless(shutil.which("bash"), "needs bash")
    def test_the_command_minimises_each_crash_and_keeps_the_runs_status(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            crate = Path(tmp) / "crate"
            (crate / "artifacts" / "fx").mkdir(parents=True)
            (crate / "artifacts" / "fx" / "crash-1").write_bytes(b"1")
            (crate / "artifacts" / "fx" / "crash-2").write_bytes(b"2")
            bin_ = Path(tmp) / "bin"
            bin_.mkdir()
            fake = bin_ / "cargo"
            fake.write_text('#!/bin/sh\necho "$@" >> "$LOG"\n[ "$3" = run ] && exit 77\nexit 0\n', encoding="utf-8")
            fake.chmod(0o755)
            log = Path(tmp) / "log"
            env = {"PATH": f"{bin_}:/usr/bin:/bin", "LOG": str(log)}
            done = subprocess.run(fs.container_command("fx", 5, crate), env=env, capture_output=True, text=True, check=False)
            self.assertEqual(done.returncode, 77)
            lines = log.read_text(encoding="utf-8").splitlines()
            self.assertEqual([line.split()[2] for line in lines], ["run", "tmin"])  # one crash is minimised, not each
            self.assertTrue(lines[1].endswith("artifacts/fx/crash-1"))

    def test_a_finding_is_a_byte_literal_test_named_after_the_input(self) -> None:
        found = fs.finding("fx", PLANTED)
        self.assertEqual(found["bytes"], 11)
        self.assertEqual(found["hex"], PLANTED.hex())
        self.assertRegex(found["test"], r"^regression_[0-9a-f]{12}$")
        self.assertIn(f"fn {found['test']}()", found["test_text"])
        self.assertIn("0x80", found["test_text"])
        self.assertIn("replay_bytes(bytes);", found["test_text"])
        self.assertEqual(found["test"], fs.finding("fx", PLANTED)["test"])

    def test_the_status_says_what_happened(self) -> None:
        self.assertEqual([fs.exit_status({"status": s}) for s in ("clean", "finding", "failed")], [0, 1, 3])

    def test_crash_inputs_are_found_apart_from_minimised_ones_and_folders(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp) / "artifacts" / "fx"
            (folder / "sub").mkdir(parents=True)
            (folder / "crash-b").write_bytes(b"b")
            (folder / "crash-a").write_bytes(b"a")
            (folder / "minimized-from-a").write_bytes(b"m")
            self.assertEqual([p.name for p in fs.crash_files(Path(tmp), "fx")], ["crash-a", "crash-b"])
            self.assertEqual(fs.crash_files(Path(tmp), "none"), [])

    def test_run_gives_127_for_a_missing_program_and_the_output_otherwise(self) -> None:
        self.assertEqual(fs.run(["/no/such/program"])[0], 127)
        self.assertEqual(fs.run([sys.executable, "-c", "print('x')"]), (0, "x\n"))


class Fuzzing(Repo):
    def fuzz(self, container: Container, family: str = "fx", suite: str | None = None) -> dict[str, Any]:
        if suite is not None:
            (self.root / "crates" / "engine" / "tests" / "typed_fx.rs").write_text(suite, encoding="utf-8")
        return fs.fuzz(self.root, family, 60, self.work, container, self.clock, "8g", Lab())

    def test_a_clean_run_reports_clean_and_removes_the_crate(self) -> None:
        container = Container()
        report = self.fuzz(container)
        self.assertEqual((report["status"], report["findings"]), ("clean", []))
        self.assertEqual(report["container"]["image"], "img")
        self.assertGreater(report["finished"], report["started"])
        self.assertFalse((self.root / "target" / "fuzz-families" / "fx").exists())
        root, seed, command, kw = container.calls[0]
        self.assertEqual((root, kw["memory"], kw["workdir"]), (self.root, "8g", self.work))
        self.assertTrue(seed.is_dir() and not list(seed.iterdir()))  # the empty seed: a fuzz build has no seed target
        self.assertIn("fuzz run --dev --sanitizer none --fuzz-dir", command[2])
        self.assertIn(" fx -- -max_total_time=60", command[2])

    def test_a_crash_is_minimised_and_becomes_a_byte_literal_test(self) -> None:
        container = Container(crash=PLANTED, minimised=PLANTED[:9])
        report = self.fuzz(container)
        self.assertEqual(report["status"], "finding")
        self.assertEqual(len(report["findings"]), 1)
        self.assertEqual(report["findings"][0]["hex"], PLANTED[:9].hex())  # the minimised bytes, not the first ones
        self.assertEqual(len(container.calls), 1)  # one container: the build is not made twice

    def test_the_minimised_input_is_the_one_of_this_crash(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            (folder / "crash-a").write_bytes(b"first")
            (folder / "crash-b").write_bytes(b"second")
            (folder / "minimized-from-b").write_bytes(b"s")
            self.assertEqual(fs.minimised(folder / "crash-a"), b"first")  # b's minimised file is not a's
            self.assertEqual(fs.minimised(folder / "crash-b"), b"s")

    def test_a_crash_the_minimiser_cannot_shrink_is_kept_as_found(self) -> None:
        report = self.fuzz(Container(crash=PLANTED))
        self.assertEqual(report["findings"][0]["hex"], PLANTED.hex())

    def test_a_run_that_cannot_run_says_why_and_is_not_clean(self) -> None:
        failed = self.fuzz(Container({"exit": 101, "output_tail": "error: could not compile `engine-core`"}))
        self.assertEqual(failed["status"], "failed")
        self.assertIn("could not compile", failed["detail"])
        silent = self.fuzz(Container({"exit": 137, "output_tail": ""}))
        self.assertEqual((silent["status"], silent["detail"]), ("failed", "exit status 137"))
        left = self.fuzz(Container({"exit": 0, "container_left": True}))
        self.assertEqual(left["status"], "failed")
        unknown = Container({"exit": 0})
        del unknown.record["container_left"]  # a record that does not say the container is gone is not a clean run
        self.assertEqual(self.fuzz(unknown)["status"], "failed")
        gone = self.fuzz(Container(fail_run=verifier.VerifierError("docker is not running")))
        self.assertEqual((gone["status"], gone["detail"]), ("failed", "docker is not running"))
        os_error = self.fuzz(Container(fail_run=OSError("disk full")))
        self.assertIn("disk full", os_error["detail"])

    def test_a_family_that_cannot_be_scaffolded_fails_the_run(self) -> None:
        report = self.fuzz(Container(), suite="// no replay\n")
        self.assertEqual(report["status"], "failed")
        self.assertIn("replay_bytes", report["detail"])
        self.assertFalse((self.root / "target" / "fuzz-families" / "fx").exists())

    def test_a_planted_bug_gives_the_expected_report_and_issue(self) -> None:
        report = self.fuzz(Container(crash=PLANTED, minimised=PLANTED))
        title, body = fs.issue_text(report)
        self.assertEqual(title, f"fuzz: the fx family has a mismatch ({report['findings'][0]['test']})")
        self.assertIn("typed_fx.rs", body)
        self.assertIn(report["findings"][0]["test_text"], body)
        self.assertIn(PLANTED.hex(), body)
        self.assertEqual(fs.exit_status(report), 1)


class Issues(unittest.TestCase):
    def runner(self, *answers: tuple[int, str]) -> Any:
        calls: list[list[str]] = []
        queue = list(answers)

        def run(argv: list[str]) -> tuple[int, str]:
            calls.append(argv)
            return queue.pop(0)

        run.calls = calls  # type: ignore[attr-defined]
        return run

    def failed(self) -> dict[str, Any]:
        return {"family": "fx", "status": "failed", "seconds": 60, "findings": [], "detail": "no docker"}

    def test_a_failed_run_has_an_issue_of_its_own(self) -> None:
        title, body = fs.issue_text(self.failed())
        self.assertEqual(title, "fuzz: the scheduled run of the fx family could not run")
        self.assertIn("no docker", body)
        self.assertIn("```", body)

    def test_an_issue_is_opened_once(self) -> None:
        run = self.runner((0, ""), (0, "https://example.invalid/1"))
        self.assertEqual(fs.open_issue(self.failed(), "o/r", run), "opened")
        self.assertEqual(run.calls[0][:3], ["gh", "issue", "list"])
        self.assertEqual(run.calls[1][:3], ["gh", "issue", "create"])
        self.assertIn("could not run", run.calls[1][run.calls[1].index("--title") + 1])
        title = fs.issue_text(self.failed())[0]
        again = self.runner((0, f"other\n{title}\n"))
        self.assertEqual(fs.open_issue(self.failed(), "o/r", again), "exists")
        self.assertEqual(len(again.calls), 1)

    def test_a_failed_listing_or_creation_is_said_not_hidden(self) -> None:
        run = self.runner((1, "boom"), (1, "no token\n"))
        self.assertEqual(fs.open_issue(self.failed(), "o/r", run), "not opened: no token")


class Command(Repo):
    def main(self, *argv: str, container: Container | None = None, runner: Any = None) -> tuple[int, str]:
        out = io.StringIO()
        code = fs.main([*argv], self.root, out, container or Container(), runner or (lambda a: (0, "")), self.clock, Lab())
        return code, out.getvalue()

    def test_list_and_next(self) -> None:
        state = str(self.work / "state.json")
        self.assertEqual(self.main("list"), (0, '["fx", "zz"]\n'))
        self.assertEqual(self.main("next", "--state", state), (0, '"fx"\n'))
        fs.save_state(self.work / "state.json", {"fx": {"last": 9}})
        self.assertEqual(self.main("next", "--state", state), (0, '"zz"\n'))

    def test_a_run_of_the_next_family_updates_the_state(self) -> None:
        state = self.work / "state.json"
        code, text = self.main("run", "--seconds", "30", "--state", str(state))
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(text)["family"], "fx")
        saved = json.loads(state.read_text(encoding="utf-8"))
        self.assertEqual((saved["fx"]["status"], saved["fx"]["findings"]), ("clean", 0))
        self.assertEqual(self.main("run", "--state", str(state))[0], 0)
        self.assertEqual(json.loads(self.main("run", "--state", str(state))[1])["family"], "fx")  # zz ran in between

    def test_a_finding_exits_one_and_opens_its_issue_when_asked(self) -> None:
        calls: list[list[str]] = []

        def runner(argv: list[str]) -> tuple[int, str]:
            calls.append(argv)
            return 0, ""

        code, text = self.main("run", "fx", "--state", str(self.work / "s.json"), "--repo", "o/r",
                               container=Container(crash=PLANTED, minimised=PLANTED), runner=runner)
        self.assertEqual(code, 1)
        self.assertEqual(json.loads(text)["issue"], "opened")
        self.assertEqual([c[2] for c in calls], ["list", "create"])

    def test_a_clean_run_opens_no_issue_and_a_failed_one_exits_three(self) -> None:
        def runner(argv: list[str]) -> tuple[int, str]:
            raise AssertionError(argv)

        code, text = self.main("run", "fx", "--state", str(self.work / "s.json"), "--repo", "o/r", runner=runner)
        self.assertEqual((code, "issue" in json.loads(text)), (0, False))
        code, text = self.main("run", "fx", "--state", str(self.work / "t.json"),
                               container=Container({"exit": 2, "output_tail": "x"}))
        self.assertEqual(code, 3)
        self.assertEqual(json.loads((self.work / "t.json").read_text(encoding="utf-8"))["fx"]["status"], "failed")

    def test_a_family_that_cannot_be_fuzzed_or_a_bad_length_exits_two(self) -> None:
        err = io.StringIO()
        real = sys.stderr
        sys.stderr = err
        try:
            self.assertEqual(self.main("run", "aa", "--state", str(self.work / "s.json"))[0], 2)
            self.assertEqual(self.main("run", "fx", "--seconds", "0", "--state", str(self.work / "s.json"))[0], 2)
        finally:
            sys.stderr = real
        self.assertIn("nothing to fuzz", err.getvalue())
        self.assertIn("--seconds", err.getvalue())

    def test_the_script_runs_as_a_command(self) -> None:
        done = subprocess.run([sys.executable, "-I", str(HERE.parent / "fuzz_schedule.py"), "list"],
                              capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 0)
        self.assertIsInstance(json.loads(done.stdout), list)


if __name__ == "__main__":
    unittest.main()
