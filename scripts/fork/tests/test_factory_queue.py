# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the factory queue and verifier loop (scripts/fork/factory_queue.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
No cargo and no network: the gate and the family lab are scripted stand-ins; files live in temporary directories.
"""
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import factory_queue as fq

PASS = json.dumps({"family": "fx", "pass": True, "failures": [], "checks": []})
FAIL = json.dumps({"family": "fx", "pass": False, "checks": [], "failures": [
    {"rule": "typed-call-plain", "file": "crates/a.rs", "line": 7, "message": "plain call", "fix": "use typed_call!"},
    {"rule": "fmt", "file": "crates/b.rs", "line": 1, "message": "not formatted", "fix": "run rustfmt"}]})


class Script:
    """A runner: the gate answers from `gates` in order, the family lab from `labs` (default: passes)."""

    def __init__(self, gates: list[tuple[int, str]], labs: list[int] | None = None) -> None:
        self.gates, self.labs = list(gates), list(labs or [])
        self.calls: list[list[str]] = []

    def __call__(self, argv: list[str], cwd: str | Path = ".") -> tuple[int, str]:
        self.calls.append([str(a) for a in argv])
        if argv[0] == "git":
            return 0, "crates/engine-typed/src/fx/mod.rs\n"
        if str(argv[1]).endswith("port_check.py"):
            return self.gates.pop(0) if self.gates else (0, PASS)
        return (self.labs.pop(0) if self.labs else 0), "lab output\n"


class Base(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.queue = fq.Queue(self.root / "queue")
        self.worktree = self.root / "wt"
        src = self.worktree / "crates" / "engine-typed" / "src" / "fx"
        src.mkdir(parents=True)
        (src / "mod.rs").write_text("pub fn f(a: u32, b: u32) -> u32 {\n    a.wrapping_add(b)\n}\n", encoding="utf-8")

    def verify(self, script: Script, **kw: Any) -> dict[str, Any] | None:
        return fq.verify_next(self.queue, script, control=kw.pop("control", False), **kw)


class QueueTest(Base):
    def test_a_submission_is_a_queued_attempt_on_disk(self) -> None:
        attempt = self.queue.submit("fx", self.worktree, now=5.0)
        self.assertEqual((attempt["state"], attempt["retries"], attempt["submitted"]), ("queued", 0, 5.0))
        self.assertEqual(self.queue.load(attempt["id"]), attempt)
        self.assertEqual(fq.Queue(self.root / "queue").all()[0]["id"], attempt["id"])

    def test_bad_family_names_and_a_family_already_in_the_queue_are_refused(self) -> None:
        for name in ("../x", "a b", ""):
            with self.assertRaises(ValueError):
                self.queue.submit(name, self.worktree)
        self.queue.submit("fx", self.worktree)
        with self.assertRaisesRegex(ValueError, "already"):
            self.queue.submit("fx", self.worktree)

    def test_the_oldest_attempt_is_taken_first_and_only_once(self) -> None:
        first = self.queue.submit("aa", self.worktree, now=1.0)
        second = self.queue.submit("bb", self.worktree, now=2.0)
        got = self.queue.claim_next(now=10.0)
        self.assertEqual((got or {})["id"], first["id"])
        self.assertEqual((self.queue.claim_next(now=11.0) or {})["id"], second["id"])
        self.assertIsNone(self.queue.claim_next(now=12.0))

    def test_a_claim_that_outlived_its_lease_is_taken_over_and_a_fresh_one_is_not(self) -> None:
        self.queue.submit("fx", self.worktree)
        self.queue.claim_next(now=100.0)
        self.assertIsNone(self.queue.claim_next(now=100.0 + fq.LEASE_SECONDS - 1))
        again = self.queue.claim_next(now=100.0 + fq.LEASE_SECONDS + 1)
        self.assertEqual((again or {})["state"], "verifying")

    def test_a_held_claim_file_keeps_a_second_verifier_out(self) -> None:
        attempt = self.queue.submit("fx", self.worktree)
        (self.queue.directory / f"{attempt['id']}.claim").write_text("", encoding="utf-8")
        self.assertIsNone(self.queue.claim_next())

    def test_an_unreadable_file_is_not_an_attempt(self) -> None:
        (self.queue.directory / "junk.json").write_text("{not json", encoding="utf-8")
        self.queue.submit("fx", self.worktree)
        self.assertEqual(len(self.queue.all()), 1)
        self.assertFalse(list(self.queue.directory.glob(".*.tmp")))

    def test_a_family_waiting_for_a_retry_is_resubmitted_as_the_same_attempt(self) -> None:
        attempt = self.queue.submit("fx", self.worktree)
        attempt["state"] = "retry"
        self.queue.save(attempt)
        again = self.queue.submit("fx", self.root / "other", now=9.0)
        self.assertEqual((again["id"], again["state"], again["worktree"]), (attempt["id"], "queued", str(self.root / "other")))
        self.assertEqual(len(self.queue.all()), 1)


class Pieces(Base):
    @staticmethod
    def survivor(argv: list[str], cwd: str | Path = ".") -> tuple[int, str]:
        return (0, "crates/engine-typed/src/fx/mod.rs\n") if argv[0] == "git" else (0, "ok\n")

    def test_feedback_is_one_short_line_per_failure(self) -> None:
        lines = fq.feedback_lines(json.loads(FAIL))
        self.assertEqual(lines[0], "typed-call-plain crates/a.rs:7: plain call -> fix: use typed_call!")
        self.assertEqual(len(lines), 2)
        many = {"failures": [{"rule": "r", "message": "x" * 500}] * 30}
        shown = fq.feedback_lines(many)
        self.assertEqual((len(shown), max(len(x) for x in shown)), (fq.FEEDBACK_LINES, fq.FEEDBACK_WIDTH))
        self.assertEqual(fq.feedback_lines({}), [])

    def test_a_verdict_is_read_from_the_gates_output_or_not_at_all(self) -> None:
        self.assertEqual((fq.read_verdict("note\n" + PASS) or {})["pass"], True)
        for bad in ("", "no json", "{broken", "[]", json.dumps({"pass": "yes", "failures": []}),
                    json.dumps({"pass": True}), '{"pass": true, "failures": 3}'):
            self.assertIsNone(fq.read_verdict(bad), bad)

    def test_the_mutation_changes_one_operator_in_a_copy_only(self) -> None:
        scratch = self.root / "scratch"
        mutation = fq.mutate_sources(self.worktree, scratch)
        self.assertIn("wrapping_add", mutation or "")
        copy = (scratch / "crates" / "engine-typed" / "src" / "fx" / "mod.rs").read_text(encoding="utf-8")
        self.assertIn("a.wrapping_sub(b)", copy)
        self.assertIn("wrapping_add", (self.worktree / "crates/engine-typed/src/fx/mod.rs").read_text(encoding="utf-8"))

    def test_an_operator_in_a_comment_or_a_string_is_not_mutated(self) -> None:
        source = self.worktree / "crates/engine-typed/src/fx/mod.rs"
        source.write_text('// a == b\nconst S: &str = "x + y";\npub fn f(a: u32) -> bool { a < 2 }\n', encoding="utf-8")
        mutation = fq.mutate_sources(self.worktree, self.root / "s")
        self.assertIn("`<` -> `>=`", mutation or "")
        copy = (self.root / "s/crates/engine-typed/src/fx/mod.rs").read_text(encoding="utf-8")
        self.assertIn("// a == b", copy)
        self.assertIn('"x + y"', copy)
        self.assertIn("a >= 2", copy)

    def test_a_verdict_with_text_around_it_is_still_read(self) -> None:
        self.assertEqual((fq.read_verdict("warning\n" + PASS + "\ntrailing words") or {})["pass"], True)

    def test_a_mutation_that_does_not_build_says_nothing_about_the_cases(self) -> None:
        def broken(argv: list[str], cwd: str | Path = ".") -> tuple[int, str]:
            return (0, "crates/engine-typed/src/fx/mod.rs\n") if argv[0] == "git" else (101, "error[E0308]: mismatched\n")

        outcome, detail = fq.planted_bug_control(self.worktree, "fx", broken)
        self.assertEqual(outcome, "skipped")
        self.assertIn("did not build", detail)

    def test_when_git_cannot_say_what_changed_any_typed_source_may_be_mutated(self) -> None:
        def no_git(argv: list[str], cwd: str | Path = ".") -> tuple[int, str]:
            return (1, "fatal") if argv[0] == "git" else (1, "case failed\n")

        self.assertEqual(fq.planted_bug_control(self.worktree, "fx", no_git)[0], "failed")

    def test_code_after_the_test_module_is_not_mutated(self) -> None:
        source = self.worktree / "crates/engine-typed/src/fx/mod.rs"
        source.write_text("pub fn f() {}\n#[cfg(test)]\nmod t { fn g(a: u32) -> bool { a == 1 } }\n", encoding="utf-8")
        self.assertIsNone(fq.mutate_sources(self.worktree, self.root / "s"))

    def test_the_control_fails_when_the_cases_see_the_change_and_restores_the_sources(self) -> None:
        source = self.worktree / "crates/engine-typed/src/fx/mod.rs"
        before = source.read_text(encoding="utf-8")
        seen: list[str] = []

        def runner(argv: list[str], cwd: str | Path = ".") -> tuple[int, str]:
            if argv[0] == "git":
                return 0, "crates/engine-typed/src/fx/mod.rs\ndocs/x.md\n"
            seen.append(source.read_text(encoding="utf-8"))
            return 1, "case failed\n"

        self.assertEqual(fq.planted_bug_control(self.worktree, "fx", runner)[0], "failed")
        self.assertIn("wrapping_sub", seen[0])
        self.assertEqual(source.read_text(encoding="utf-8"), before)
        self.assertFalse((self.worktree / "target" / "planted-bug").exists())

    def test_the_control_only_changes_files_the_attempt_changed(self) -> None:
        def other_files(argv: list[str], cwd: str | Path = ".") -> tuple[int, str]:
            return (0, "crates/engine-typed/src/other/mod.rs\n") if argv[0] == "git" else (1, "")

        self.assertEqual(fq.planted_bug_control(self.worktree, "fx", other_files)[0], "skipped")
        self.assertEqual(fq.mutate_sources(self.worktree, self.root / "s", ["nothing.rs"]), None)

    def test_the_control_reports_a_survivor_and_a_skip(self) -> None:
        self.assertEqual(fq.planted_bug_control(self.worktree, "fx", self.survivor)[0], "survived")
        (self.worktree / "crates/engine-typed/src/fx/mod.rs").write_text("pub fn f() {}\n", encoding="utf-8")
        self.assertEqual(fq.planted_bug_control(self.worktree, "fx", self.survivor)[0], "skipped")

    def test_hidden_cases_run_from_a_copy_that_is_removed_afterwards(self) -> None:
        hidden = self.root / "hidden"
        hidden.mkdir()
        (hidden / "typed_fx.rs").write_text("// hidden\n", encoding="utf-8")
        (self.worktree / "crates/engine/tests").mkdir(parents=True)
        script = Script([], labs=[0])
        self.assertEqual(fq.run_hidden(self.worktree, "fx", hidden, script)[0], True)
        self.assertEqual(script.calls[0][-1], "fx_hidden")
        self.assertFalse((self.worktree / "crates/engine/tests/typed_fx_hidden.rs").exists())
        self.assertEqual(fq.run_hidden(self.worktree, "fx", hidden, Script([], labs=[1]))[0], False)

    def test_no_hidden_cases_is_not_a_failure(self) -> None:
        self.assertEqual(fq.run_hidden(self.worktree, "fx", None, Script([])), (True, "no hidden cases for this family"))
        self.assertTrue(fq.run_hidden(self.worktree, "fx", self.root / "none", Script([]))[0])

    def test_run_gives_127_for_a_missing_program(self) -> None:
        self.assertEqual(fq.run([sys.executable, "-c", "print(1)"])[0], 0)
        self.assertEqual(fq.run(["no-such-program-here"])[0], 127)


class Verifying(Base):
    def test_an_empty_queue_has_nothing_to_verify(self) -> None:
        self.assertIsNone(self.verify(Script([])))

    def test_a_passing_gate_accepts_and_runs_from_the_attempts_own_checkout(self) -> None:
        self.queue.submit("fx", self.worktree)
        script = Script([(0, PASS)])
        done = self.verify(script)
        self.assertEqual((done or {})["state"], "accepted")
        self.assertEqual(script.calls[0][1:], ["scripts/fork/port_check.py", "fx", "--base", "origin/fork"])
        self.assertFalse(list(self.queue.directory.glob("*.claim")))

    def test_a_failing_gate_sends_one_line_per_rule_back_to_the_same_attempt(self) -> None:
        attempt = self.queue.submit("fx", self.worktree)
        done = self.verify(Script([(1, FAIL)]))
        self.assertEqual(((done or {})["id"], (done or {})["state"], (done or {})["retries"]), (attempt["id"], "retry", 1))
        self.assertEqual(len((done or {})["feedback"]), 2)
        self.assertIn("typed-call-plain", (done or {})["feedback"][0])

    def test_an_attempt_that_keeps_failing_is_rejected_after_the_retries(self) -> None:
        self.queue.submit("fx", self.worktree)
        states: list[str] = []
        for _ in range(fq.MAX_RETRIES + 1):
            done = self.verify(Script([(1, FAIL)]))
            states.append((done or {})["state"])
            if states[-1] == "retry":
                self.queue.submit("fx", self.worktree)
        self.assertEqual(states, ["retry"] * fq.MAX_RETRIES + ["rejected"])

    def test_a_gate_with_no_readable_verdict_requeues_without_using_a_retry(self) -> None:
        self.queue.submit("fx", self.worktree)
        for expected in ("queued", "queued", "rejected"):
            done = self.verify(Script([(0, "Traceback: boom")]))
            self.assertEqual(((done or {})["state"], (done or {})["retries"]), (expected, 0))
        self.assertEqual((done or {})["infra"], fq.MAX_INFRA)

    def test_a_failing_hidden_case_goes_back_to_the_implementer(self) -> None:
        hidden = self.root / "hidden"
        hidden.mkdir()
        (hidden / "typed_fx.rs").write_text("// h\n", encoding="utf-8")
        (self.worktree / "crates/engine/tests").mkdir(parents=True)
        self.queue.submit("fx", self.worktree)
        done = self.verify(Script([(0, PASS)], labs=[1]), hidden=hidden)
        self.assertEqual((done or {})["state"], "retry")
        self.assertIn("hidden-cases", (done or {})["feedback"][0])

    def test_a_gate_that_failed_without_naming_a_rule_still_fails_the_attempt(self) -> None:
        self.queue.submit("fx", self.worktree)
        done = self.verify(Script([(1, json.dumps({"pass": False, "failures": []}))]))
        self.assertEqual((done or {})["state"], "retry")
        self.assertIn("named no rule", (done or {})["feedback"][0])

    def test_a_planted_bug_the_cases_do_not_see_goes_back_with_what_was_changed(self) -> None:
        self.queue.submit("fx", self.worktree)
        done = self.verify(Script([(0, PASS)], labs=[0]), control=True)
        self.assertEqual((done or {})["state"], "retry")
        self.assertIn("planted-bug", (done or {})["feedback"][0])
        self.assertIn("wrapping_add", (done or {})["feedback"][0])

    def test_a_planted_bug_the_cases_do_see_lets_the_attempt_through(self) -> None:
        self.queue.submit("fx", self.worktree)
        done = self.verify(Script([(0, PASS)], labs=[1]), control=True)
        self.assertEqual((done or {})["state"], "accepted")
        self.assertEqual([h["outcome"] for h in (done or {})["history"] if h["event"] == "control"], ["failed"])

    def test_a_verifier_killed_mid_run_leaves_an_attempt_the_next_one_takes_over(self) -> None:
        self.queue.submit("fx", self.worktree)
        self.queue.claim_next(now=1.0)  # the verifier dies here: state verifying, claim held
        self.assertIsNone(fq.verify_next(self.queue, Script([(0, PASS)]), control=False, now=2.0))
        done = fq.verify_next(self.queue, Script([(0, PASS)]), control=False, now=1.0 + fq.LEASE_SECONDS + 5)
        self.assertEqual((done or {})["state"], "accepted")

    def test_end_to_end_two_failures_feedback_to_the_prompt_then_acceptance(self) -> None:
        self.queue.submit("fx", self.worktree)
        script = Script([(1, FAIL), (1, FAIL), (0, PASS)])
        done: dict[str, Any] = {}
        for _ in range(3):
            done = self.verify(script) or {}
            if done["state"] == "retry":
                self.assertIn("typed-call-plain crates/a.rs:7", "\n".join(done["feedback"]))
                self.queue.submit("fx", self.worktree)
        self.assertEqual((done["state"], done["retries"]), ("accepted", 2))
        self.assertEqual([a["state"] for a in self.queue.all()], ["accepted"])


def prompt_fixture(root: Path) -> Path:
    """A tiny checkout with the files the prompt reads."""
    diff = root / "crates/engine-core/src/typed"
    diff.mkdir(parents=True)
    (diff / "diff.rs").write_text(
        "//! docs\npub enum Input {\n    Word,\n}\n    pub fn run(&self) -> Report {\n}\nfn hidden() {}\n", encoding="utf-8")
    (diff / "mod.rs").write_text(
        "/// intro\n/// `typed_call!(FAMILY, ADDR, f(e))` form\n#[macro_export]\nmacro_rules! typed_call {\n}\n",
        encoding="utf-8")
    tests = root / "crates/engine/tests"
    tests.mkdir(parents=True)
    (tests / "typed_big.rs").write_text("big\n" * 50, encoding="utf-8")
    (tests / "typed_small.rs").write_text("small\nsuite\n", encoding="utf-8")
    return root


class Prompt(Base):
    def fixture(self) -> Path:
        return prompt_fixture(self.root)

    def test_the_harness_excerpt_has_the_public_items_and_the_hash_of_its_source(self) -> None:
        excerpt = fq.api_excerpt(self.fixture())
        self.assertRegex(excerpt.splitlines()[0], r"^// diff\.rs sha256 [0-9a-f]{16}$")
        self.assertIn("pub enum Input {", excerpt)
        self.assertIn("pub fn run(&self) -> Report {", excerpt)
        self.assertNotIn("hidden", excerpt)

    def test_the_example_is_the_smallest_suite_and_is_cut_to_the_limit(self) -> None:
        self.assertEqual(fq.worked_example(self.fixture()), "// typed_small.rs\nsmall\nsuite")
        self.assertEqual(len(fq.worked_example(self.root, limit=1).splitlines()), 2)
        with tempfile.TemporaryDirectory() as empty:
            self.assertEqual(fq.worked_example(Path(empty)), "")

    def test_the_forms_are_the_macros_doc_comment(self) -> None:
        self.assertEqual(fq.typed_call_forms(self.fixture()), "intro\n`typed_call!(FAMILY, ADDR, f(e))` form")

    def test_the_prompt_says_never_cargo_names_the_static_check_and_carries_the_feedback(self) -> None:
        prompt = fq.implementer_prompt(self.fixture(), "fx", ["fmt a.rs:1: x -> fix: y"])
        self.assertIn("port_check.py fx --static", prompt)
        self.assertIn("Never run cargo", prompt)
        self.assertIn("fmt a.rs:1: x -> fix: y", prompt)
        self.assertNotIn("## The verifier's lines", fq.implementer_prompt(self.root, "fx"))

    def test_the_prompt_for_this_checkout_has_every_part(self) -> None:
        prompt = fq.implementer_prompt(fq.REPO, "facegen_sizes")
        for part in ("pub struct Diff", "## A worked example", "typed_call!(FAMILY, ADDR", "Never run cargo"):
            self.assertIn(part, prompt)


class Command(Base):
    def go(self, *args: str, script: Script | None = None) -> tuple[int, str]:
        out = io.StringIO()
        code = fq.main(["--queue", str(self.root / "q2"), *args], root=self.root, runner=script or Script([]), out=out)
        return code, out.getvalue()

    def test_queue_can_follow_the_subcommand_as_documented(self) -> None:
        queue = str(self.root / "q4")
        out = io.StringIO()
        self.assertEqual(fq.main(["submit", "fx", str(self.worktree), "--queue", queue], root=self.root, out=out), 0)
        out = io.StringIO()
        self.assertEqual(fq.main(["list", "--queue", queue], root=self.root, out=out), 0)
        self.assertEqual(json.loads(out.getvalue())[0]["family"], "fx")

    def test_submit_verify_and_list(self) -> None:
        code, out = self.go("submit", "fx", str(self.worktree))
        self.assertEqual((code, json.loads(out)["state"]), (0, "queued"))
        code, out = self.go("verify", "--no-control", script=Script([(0, PASS)]))
        self.assertEqual((code, json.loads(out)["state"]), (0, "accepted"))
        self.assertEqual(json.loads(self.go("list")[1])[0]["state"], "accepted")
        self.assertEqual(json.loads(self.go("verify")[1]), None)

    def test_the_prompt_command_reads_a_feedback_file(self) -> None:
        feedback = self.root / "fb.txt"
        feedback.write_text("fmt a.rs:1: bad -> fix: format\n", encoding="utf-8")
        prompt_fixture(self.root)
        code, out = self.go("prompt", "fx", "--feedback", str(feedback))
        self.assertEqual(code, 0)
        self.assertIn("fmt a.rs:1: bad -> fix: format", out)

    def test_errors_exit_with_two(self) -> None:
        self.assertEqual(self.go("submit", "../x", str(self.worktree))[0], 2)
        self.assertEqual(self.go("prompt", "fx", "--feedback", str(self.root / "missing"))[0], 2)

    def test_the_script_runs_as_a_command(self) -> None:
        done = subprocess.run([sys.executable, "-I", str(HERE.parent / "factory_queue.py"), "--queue", str(self.root / "q3"), "list"],
                              capture_output=True, text=True, check=False)
        self.assertEqual((done.returncode, json.loads(done.stdout)), (0, []))


if __name__ == "__main__":
    unittest.main()
