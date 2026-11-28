# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the two-review gate (scripts/fork/review_gate.py). Synthetic reviewer names; the gateway is scripted."""
import io
import json
import subprocess
import sys
import tempfile
import unittest
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import review_gate as rg

POOL = [
    {"client": "c", "model": "w1", "family": "wfam"},
    {"client": "c", "model": "a1", "family": "afam"},
    {"client": "c", "model": "a2", "family": "afam"},
    {"client": "c", "model": "b1", "family": "bfam"},
    {"client": "c", "model": "d1", "family": "dfam"},
]
PASS = '{"verdict":"pass","findings":[]}'
FAIL = '{"verdict":"fail","findings":[{"file":"a.py","line":3,"issue":"bad","fix":"good"}]}'


def answer(summary: str = PASS, status: str = "ok") -> tuple[int, str]:
    return 0, json.dumps({"status": status, "summary": summary})


LIMITED = (0, json.dumps({"status": "failed", "summary": "429 every free deployment is limited"}))


class Gateway:
    """Answers each review by the model named in the request, from `script` (default: pass)."""

    def __init__(self, script: Mapping[str, Sequence[tuple[int, str]]] | None = None) -> None:
        self.script = {k: list(v) for k, v in (script or {}).items()}
        self.asked: list[str] = []
        self.argv: list[list[str]] = []

    def __call__(self, argv: list[str], cwd: str | Path = ".") -> tuple[int, str]:
        self.argv.append(argv)
        request = json.loads(Path(argv[argv.index("--input") + 1]).read_text(encoding="utf-8"))
        self.asked.append(f"{request['model']}:{'red' if 'red team' in request['prompt'] else 'ordinary'}")
        queue = self.script.get(request["model"])
        return queue.pop(0) if queue else answer()


def fresh() -> dict[str, Any]:
    return {"last_red": None, "cooldown": {}}


class Choosing(unittest.TestCase):
    def test_a_reviewer_is_never_of_the_writers_family_nor_of_the_review_before(self) -> None:
        got = rg.candidates(POOL, "red", "wfam", ["afam"], fresh(), 0.0)
        self.assertEqual([r["model"] for r in got], ["b1", "d1"])

    def test_the_adversarial_reviewer_is_never_the_one_of_the_last_adversarial_review(self) -> None:
        state: dict[str, Any] = {"last_red": "c/b1", "cooldown": {}}
        self.assertEqual([r["model"] for r in rg.candidates(POOL, "red", "wfam", ["afam"], state, 0.0)], ["d1"])
        self.assertIn("b1", [r["model"] for r in rg.candidates(POOL, "ordinary", "wfam", [], state, 0.0)])

    def test_a_reviewer_on_a_cooldown_is_skipped_until_it_ends(self) -> None:
        state: dict[str, Any] = {"last_red": None, "cooldown": {"c/a1": 100.0}}
        self.assertNotIn("a1", [r["model"] for r in rg.candidates(POOL, "ordinary", "wfam", [], state, 50.0)])
        self.assertIn("a1", [r["model"] for r in rg.candidates(POOL, "ordinary", "wfam", [], state, 100.0)])


class Reviewing(unittest.TestCase):
    def go(self, gateway: Gateway, state: dict[str, Any] | None = None, now: float = 0.0) -> dict[str, Any]:
        return rg.review_candidate(POOL, "wfam", "diff text", "a context", state or fresh(), gateway, now)

    def test_two_passes_from_two_different_families_one_after_the_other(self) -> None:
        gateway = Gateway()
        result = self.go(gateway)
        self.assertEqual(result["verdict"], "pass")
        self.assertEqual(gateway.asked, ["a1:ordinary", "b1:red"])
        self.assertEqual(result["reviewers"], [{"kind": "ordinary", "family": "afam"}, {"kind": "red", "family": "bfam"}])
        self.assertEqual(gateway.argv[0][:4], ["capped", "3G", "model-gateway", "run"])

    def test_a_failing_review_fails_the_candidate_and_keeps_its_findings(self) -> None:
        result = self.go(Gateway({"b1": [answer(FAIL)]}))
        self.assertEqual((result["verdict"], result["findings"][0]["issue"]), ("fail", "bad"))

    def test_the_request_is_read_only_with_escalation_off_and_carries_the_diff_and_context(self) -> None:
        request = rg.request("red", POOL[1], "the diff", "the context", "wf")
        self.assertEqual((request["mode"], request["escalate"], request["workflow"]), ("read-only", False, "wf"))
        self.assertIn("the diff", request["prompt"])
        self.assertIn("the context", request["prompt"])
        self.assertIn("red team", request["prompt"])
        self.assertNotIn("red team", rg.request("ordinary", POOL[1], "d", "c", "w")["prompt"])

    def test_a_rate_limited_reviewer_is_replaced_and_put_on_a_cooldown(self) -> None:
        state = fresh()
        gateway = Gateway({"a1": [LIMITED]})
        result = self.go(gateway, state, now=10.0)
        self.assertEqual(result["verdict"], "pass")
        self.assertEqual(state["cooldown"]["c/a1"], 10.0 + rg.COOLDOWN_SECONDS)
        self.assertEqual(gateway.asked[:2], ["a1:ordinary", "a2:ordinary"])

    def test_an_answer_without_a_readable_verdict_is_replaced_too(self) -> None:
        for bad in (answer("no verdict at all"), (1, "boom"), (0, "not json"), (0, "[]"), answer(PASS, status="failed")):
            gateway = Gateway({"a1": [bad]})
            self.assertEqual(self.go(gateway)["verdict"], "pass", bad)
            self.assertEqual(gateway.asked[:2], ["a1:ordinary", "a2:ordinary"], bad)

    def test_when_no_reviewer_is_left_the_verdict_is_absent(self) -> None:
        everyone = {m["model"]: [LIMITED] for m in POOL}
        result = self.go(Gateway(everyone))
        self.assertEqual((result["verdict"], result["findings"]), ("absent", []))

    def test_a_second_review_that_cannot_be_had_is_absent_not_a_pass(self) -> None:
        only_one_family = [m for m in POOL if m["family"] in ("wfam", "afam")]
        result = rg.review_candidate(only_one_family, "wfam", "d", "c", fresh(), Gateway(), 0.0)
        self.assertEqual(result["verdict"], "absent")
        self.assertEqual(len(result["reviewers"]), 1)

    def test_the_ordinary_reviewer_is_chosen_so_that_an_adversarial_one_is_left(self) -> None:
        pool = [{"client": "c", "model": "w", "family": "wfam"}, {"client": "c", "model": "x", "family": "xfam"},
                {"client": "c", "model": "y", "family": "yfam"}]
        state: dict[str, Any] = {"last_red": "c/y", "cooldown": {}}
        gateway = Gateway()
        result = rg.review_candidate(pool, "wfam", "d", "c", state, gateway, 0.0)
        self.assertEqual((result["verdict"], gateway.asked), ("pass", ["y:ordinary", "x:red"]))

    def test_the_adversarial_reviewer_changes_from_one_candidate_to_the_next(self) -> None:
        state = fresh()
        first, second = Gateway(), Gateway()
        self.go(first, state)
        self.go(second, state)
        self.assertEqual(first.asked[1], "b1:red")
        self.assertEqual(second.asked[1], "d1:red")

    def test_a_verdict_in_the_middle_of_prose_is_read_from_the_last_object(self) -> None:
        self.assertEqual((rg.read_verdict('hints {"x": 1} then ' + FAIL + " done") or {})["verdict"], "fail")
        for bad in ("", "{", '{"verdict": "maybe"}', '{"verdict": "pass", "findings": 3}', "[1]"):
            self.assertIsNone(rg.read_verdict(bad), bad)

    def test_run_gives_127_for_a_missing_program(self) -> None:
        self.assertEqual(rg.run([sys.executable, "-c", "print(1)"])[0], 0)
        self.assertEqual(rg.run(["no-such-program-here"])[0], 127)


class State(unittest.TestCase):
    def test_the_state_is_kept_between_runs_and_a_damaged_file_is_ignored(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "state.json"
            rg.save_state(path, {"last_red": "c/b1", "cooldown": {"c/a1": 5.0}})
            self.assertEqual(rg.load_state(path), {"last_red": "c/b1", "cooldown": {"c/a1": 5.0}})
            for text in ("{not json", "[]", '{"cooldown": 3}'):
                path.write_text(text, encoding="utf-8")
                self.assertEqual(rg.load_state(path)["cooldown"], {})
            self.assertEqual(rg.load_state(Path(tmp) / "none.json"), {"last_red": None, "cooldown": {}})
            self.assertEqual(rg.load_state(None), {"last_red": None, "cooldown": {}})
            rg.save_state(None, fresh())


class Command(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        (root / "pool.json").write_text(json.dumps(POOL), encoding="utf-8")
        (root / "d.diff").write_text("diff", encoding="utf-8")
        (root / "c.txt").write_text("ctx\n", encoding="utf-8")
        self.args = ["--pool", str(root / "pool.json"), "--writer-family", "wfam", "--diff", str(root / "d.diff"),
                     "--context", str(root / "c.txt"), "--state", str(root / "s.json"), "--out", str(root / "v.json")]
        self.root = root

    def test_the_verdict_file_is_what_port_check_reads_and_the_exit_status_follows_it(self) -> None:
        out = io.StringIO()
        self.assertEqual(rg.main(self.args, Gateway(), out), 0)
        written = json.loads((self.root / "v.json").read_text(encoding="utf-8"))
        self.assertEqual((written["verdict"], json.loads(out.getvalue())), ("pass", written))
        self.assertEqual(json.loads((self.root / "s.json").read_text(encoding="utf-8"))["last_red"], "c/b1")
        self.assertEqual(rg.main(self.args, Gateway({"d1": [answer(FAIL)]}), io.StringIO()), 1)
        everyone = Gateway({m["model"]: [LIMITED, LIMITED, LIMITED] for m in POOL})
        self.assertEqual(rg.main(self.args, everyone, io.StringIO()), 3)

    def test_without_an_output_file_the_verdict_is_only_printed(self) -> None:
        out = io.StringIO()
        self.assertEqual(rg.main(self.args[:-2], Gateway(), out), 0)
        self.assertEqual(json.loads(out.getvalue())["verdict"], "pass")

    def test_a_bad_pool_or_a_missing_file_exits_with_two(self) -> None:
        (self.root / "pool.json").write_text('[{"client": "c"}]', encoding="utf-8")
        self.assertEqual(rg.main(self.args, Gateway(), io.StringIO()), 2)
        (self.root / "pool.json").write_text("{}", encoding="utf-8")
        self.assertEqual(rg.main(self.args, Gateway(), io.StringIO()), 2)
        self.assertEqual(rg.main([*self.args[:-4], "--diff", str(self.root / "gone")], Gateway(), io.StringIO()), 2)

    def test_the_script_runs_as_a_command(self) -> None:
        done = subprocess.run([sys.executable, "-I", str(HERE.parent / "review_gate.py"), "--pool", "/none",
                               "--writer-family", "x", "--diff", "/none", "--context", "/none"],
                              capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 2)


if __name__ == "__main__":
    unittest.main()
