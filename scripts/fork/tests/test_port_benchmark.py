# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the porting benchmark (scripts/fork/port_benchmark.py)."""
import copy
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

import port_benchmark as pb


def checkout(root: Path, count: int = 30) -> None:
    """A tiny repository: `count` typed functions in two files, each named by a suite; one marker with no suite."""
    typed = root / "crates/engine-typed/src/fam"
    typed.mkdir(parents=True)
    suites = root / "crates/engine/tests"
    suites.mkdir(parents=True)
    for part, name in ((range(count // 2), "a.rs"), (range(count // 2, count), "b.rs")):
        (typed / name).write_text("".join(f"// Typed from {0x00400000 + 16 * i:08x}\npub fn f{i}() {{}}\n" for i in part), encoding="utf-8")
    (typed / "lonely.rs").write_text("// Typed from 00999999\npub fn lone() {}\n", encoding="utf-8")
    (suites / "typed_fam.rs").write_text("".join(f"entry!(0x{0x00400000 + 16 * i:08x}, f{i});\n".replace("0x0040", "0x0040_")
                                                for i in range(count)), encoding="utf-8")
    (suites / "typed_other.rs").write_text("const X: u32 = 0x0040_0010;\n", encoding="utf-8")


def frozen_set(root: Path, count: int = 25) -> dict[str, Any]:
    checkout(root)
    return pb.select(root, count)


def result(address: str, passed: bool = True, **kw: Any) -> dict[str, Any]:
    return {"address": address, "passed": passed, "retries": kw.get("retries", 0), "seconds": kw.get("seconds", 60),
            "cost": kw.get("cost", 0.0)}


class Selecting(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_only_functions_a_suite_names_are_ported_and_underscores_are_read(self) -> None:
        checkout(self.root, 24)
        found = pb.ported(self.root)
        self.assertEqual(len(found), 24)
        self.assertNotIn("00999999", [e["address"] for e in found])
        first = next(e for e in found if e["address"] == "00400010")
        self.assertEqual(first["suites"], ["crates/engine/tests/typed_fam.rs", "crates/engine/tests/typed_other.rs"])

    def test_the_set_is_the_same_every_time_and_has_the_asked_size_and_a_hash(self) -> None:
        first = frozen_set(self.root)
        self.assertEqual(first, pb.select(self.root, 25))
        self.assertEqual(len(first["entries"]), 25)
        self.assertEqual(first["hash"], pb.set_hash(first["entries"]))
        self.assertNotEqual(pb.select(self.root, 22)["hash"], first["hash"])

    def test_a_set_outside_20_to_30_or_larger_than_the_pool_is_refused(self) -> None:
        checkout(self.root, 24)
        for count in (19, 31):
            with self.assertRaises(ValueError):
                pb.select(self.root, count)
        with self.assertRaisesRegex(ValueError, "only 24"):
            pb.select(self.root, 25)

    def test_a_frozen_set_checks_clean_and_any_change_is_found(self) -> None:
        frozen = frozen_set(self.root)
        self.assertEqual(pb.check(frozen), [])
        edited = copy.deepcopy(frozen)
        edited["entries"][0]["suites"].append("x.rs")
        self.assertIn("another benchmark", pb.check(edited)[0])
        short = copy.deepcopy(frozen)
        short["entries"] = short["entries"][:5]
        self.assertIn("20 to 30", pb.check(short)[0])
        self.assertIn("20 to 30", pb.check({})[0])
        twice = copy.deepcopy(frozen)
        twice["entries"][1] = twice["entries"][0]
        twice["hash"] = pb.set_hash(twice["entries"])
        self.assertIn("twice", pb.check(twice)[0])
        odd = copy.deepcopy(frozen)
        odd["entries"][2] = 7
        self.assertTrue(pb.check(odd))

    def test_a_blind_workspace_hides_the_accepted_code_and_every_test_that_names_it(self) -> None:
        frozen = frozen_set(self.root)
        entry = frozen["entries"][0]
        hidden = pb.hide(frozen, "0x" + entry["address"])
        self.assertEqual(hidden, [entry["source"], *entry["suites"]])
        with self.assertRaises(ValueError):
            pb.hide(frozen, "deadbeef")


class Scoring(unittest.TestCase):
    def frozen(self) -> dict[str, Any]:
        with tempfile.TemporaryDirectory() as tmp:
            return frozen_set(Path(tmp))

    def test_the_interval_is_wide_for_few_and_narrow_for_many(self) -> None:
        low, high = pb.wilson(20, 25)
        self.assertLess(low, 0.8)
        self.assertGreater(high, 0.8)
        self.assertEqual(pb.wilson(0, 0), (0.0, 1.0))
        self.assertEqual(pb.wilson(25, 25)[1], 1.0)
        self.assertEqual(pb.wilson(0, 25)[0], 0.0)
        self.assertLess(pb.wilson(200, 250)[1] - pb.wilson(200, 250)[0], pb.wilson(20, 25)[1] - pb.wilson(20, 25)[0])

    def test_the_measures_count_what_passed_the_hidden_cases_and_what_was_never_attempted_as_failed(self) -> None:
        frozen = self.frozen()
        addresses = [e["address"] for e in frozen["entries"]]
        results = [result(a, passed=i < 20, retries=1 if i < 10 else (3 if i >= 20 else 0), seconds=120, cost=0.5)
                   for i, a in enumerate(addresses[:22])]
        got = pb.summarise(frozen, results)
        self.assertEqual((got["functions"], got["attempted"], got["correct"], got["correct_rate"]), (25, 22, 20, 0.8))
        self.assertAlmostEqual(got["functions_per_hour"], 20 / (22 * 120 / 3600))
        self.assertAlmostEqual(got["cost_per_function"], 22 * 0.5 / 20)
        self.assertEqual(got["retries_per_success"], (10 + 6) / 20)  # the failed ports' retries count too

    def test_a_run_with_no_success_has_no_per_function_figures(self) -> None:
        frozen = self.frozen()
        got = pb.summarise(frozen, [result(frozen["entries"][0]["address"], passed=False)])
        self.assertEqual((got["correct"], got["cost_per_function"], got["retries_per_success"]), (0, None, None))
        self.assertEqual(pb.summarise(frozen, [])["functions_per_hour"], 0.0)

    def test_results_for_functions_outside_the_set_are_refused(self) -> None:
        with self.assertRaisesRegex(ValueError, "outside the set"):
            pb.summarise(self.frozen(), [result("deadbeef")])

    def test_a_record_names_the_set_the_prompt_and_the_implementers(self) -> None:
        frozen = self.frozen()
        line = pb.record(frozen, [], "route-a", "prompt text", 6)
        self.assertEqual((line["set"], line["label"], line["n_impl"]), (frozen["hash"], "route-a", 6))
        self.assertRegex(line["prompt"], r"^[0-9a-f]{16}$")
        self.assertNotEqual(pb.record(frozen, [], "r", "other prompt", 6)["prompt"], line["prompt"])
        broken = copy.deepcopy(frozen)
        broken["entries"][0]["source"] = "changed"
        with self.assertRaises(ValueError):
            pb.record(broken, [], "r", "p", 1)

    def test_a_difference_is_real_only_when_the_intervals_do_not_overlap(self) -> None:
        frozen = self.frozen()
        addresses = [e["address"] for e in frozen["entries"]]

        def run(label: str, correct: int) -> dict[str, Any]:
            return pb.record(frozen, [result(a, passed=i < correct) for i, a in enumerate(addresses)], label, "p", 4)

        verdicts = pb.compare([run("a", 12), run("b", 5), run("a", 14), run("a", 25), run("a", 3), run("b", 5)])
        self.assertEqual([v.rsplit(": ", 1)[1] for v in verdicts], ["noise", "better", "worse", "noise"])
        self.assertEqual(pb.compare([run("a", 12)]), [])
        other = dict(run("a", 12), set="another")
        self.assertEqual(pb.compare([run("a", 12), other]), [])


class Command(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        checkout(self.root)

    def go(self, *args: str) -> tuple[int, str]:
        out = io.StringIO()
        return pb.main(list(args), root=self.root, out=out), out.getvalue()

    def test_select_check_hide_record_and_compare_end_to_end(self) -> None:
        target = self.root / "set.json"
        self.assertEqual(self.go("select", "--count", "25", "--out", str(target))[0], 0)
        self.assertIn("ok: 25 functions", self.go("check", str(target))[1])
        entry = json.loads(target.read_text(encoding="utf-8"))["entries"][0]
        self.assertIn(entry["source"], self.go("hide", str(target), entry["address"])[1])
        results = self.root / "r.json"
        results.write_text(json.dumps([result(entry["address"])]), encoding="utf-8")
        prompt = self.root / "p.txt"
        prompt.write_text("prompt", encoding="utf-8")
        log = self.root / "log.jsonl"
        for _ in range(2):
            code, out = self.go("record", str(target), str(results), "--label", "x", "--prompt", str(prompt),
                                "--n-impl", "3", "--out", str(log))
            self.assertEqual((code, json.loads(out)["correct"]), (0, 1))
        self.assertEqual(len(log.read_text(encoding="utf-8").splitlines()), 2)
        self.assertIn("noise", self.go("compare", str(log))[1])

    def test_a_changed_set_fails_the_check_and_errors_exit_with_two(self) -> None:
        target = self.root / "set.json"
        self.go("select", "--out", str(target))
        data = json.loads(target.read_text(encoding="utf-8"))
        data["entries"][0]["source"] = "x"
        target.write_text(json.dumps(data), encoding="utf-8")
        code, out = self.go("check", str(target))
        self.assertEqual(code, 1)
        self.assertIn("another benchmark", out)
        self.assertEqual(self.go("check", str(self.root / "none.json"))[0], 2)
        self.assertEqual(self.go("select", "--count", "5")[0], 2)
        self.assertEqual(self.go("hide", str(target), "deadbeef")[0], 2)

    def test_select_without_an_output_file_only_prints_the_set(self) -> None:
        code, out = self.go("select", "--count", "20")
        self.assertEqual((code, len(json.loads(out)["entries"])), (0, 20))

    def test_the_script_runs_as_a_command(self) -> None:
        done = subprocess.run([sys.executable, "-I", str(HERE.parent / "port_benchmark.py"), "check", "/none"],
                              capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 2)

    def test_the_committed_set_is_still_the_set_of_this_checkout(self) -> None:
        committed = pb.REPO / "docs" / "fork" / "port-benchmark" / "set.json"
        frozen = json.loads(committed.read_text(encoding="utf-8"))
        self.assertEqual(pb.check(frozen), [])
        for entry in frozen["entries"]:
            self.assertTrue((pb.REPO / entry["source"]).is_file(), entry["source"])
            self.assertTrue(all((pb.REPO / s).is_file() for s in entry["suites"]), entry["address"])


if __name__ == "__main__":
    unittest.main()
