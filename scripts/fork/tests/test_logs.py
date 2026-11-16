"""Tests of scripts/fork/logs.py (nv-rs-dev logs) on synthetic JSON-lines logs, plain-text logs and run summaries
(scripts/fork/tests/fixtures/logs: a normal run and a bad one that stopped). The compact outputs are golden files in
fixtures/logs-golden; UPDATE_GOLDEN=1 rewrites them. No game, no build."""
import contextlib
import io
import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import logs  # noqa: E402

FIXTURES = HERE / "fixtures" / "logs"
GOLDEN = HERE / "fixtures" / "logs-golden"
NORMAL = "1794818419-998250"
BAD = "1794900000-4242"


def run_logs(*args, cwd=FIXTURES):
    """(exit code, stdout) of `logs.py --dir . --color never ...` run inside `cwd`, so the paths it prints are relative."""
    out = io.StringIO()
    old = os.getcwd()
    os.chdir(cwd)
    try:
        with contextlib.redirect_stdout(out):
            code = logs.main(["--dir", ".", "--color", "never", *args])
    finally:
        os.chdir(old)
    return code, out.getvalue()


class GoldenCase(unittest.TestCase):
    def golden(self, name, text):
        path = GOLDEN / name
        if os.environ.get("UPDATE_GOLDEN"):
            GOLDEN.mkdir(exist_ok=True)
            path.write_text(text, encoding="utf-8")
        self.assertEqual(text, path.read_text(encoding="utf-8"), f"{name}: rerun with UPDATE_GOLDEN=1 if the change is meant")


class CompactOutput(GoldenCase):
    def test_a_normal_run_is_a_short_report(self):
        code, out = run_logs("--run", NORMAL)
        self.assertEqual(code, 0)
        self.assertLess(len(out.splitlines()), 40)
        self.assertIn("OK: 143 fps", out)
        self.assertIn("stops: none", out)
        self.golden("normal.txt", out)

    def test_a_bad_run_names_its_stop_and_stays_under_80_lines(self):
        code, out = run_logs("--run", BAD)
        self.assertEqual(code, 0)
        self.assertLess(len(out.splitlines()), 80)
        self.assertIn("FAILED:", out)
        self.assertIn("addresses 0044fb20 00401000 0040a1c0", out)
        # The twelve soft stops of one call are one line, counted.
        self.assertIn("12x stop: soft stop: IDirectSoundBuffer::Lock", out)
        self.golden("bad.txt", out)

    def test_errors_hitches_and_diff(self):
        self.golden("bad-errors.txt", run_logs("--run", BAD, "--errors")[1])
        self.golden("normal-hitches.txt", run_logs("--run", NORMAL, "--hitches")[1])
        code, out = run_logs("--diff", NORMAL, BAD)
        self.assertEqual(code, 1, "a regression is exit status 1")
        self.assertIn("regressions: fps", out)
        self.golden("diff.txt", out)

    def test_a_diff_of_a_run_with_itself_has_no_regression(self):
        code, out = run_logs("--diff", NORMAL, NORMAL)
        self.assertEqual(code, 0)
        self.assertIn("regressions: none", out)

    def test_tail_grep_and_around(self):
        _, tail = run_logs("--run", BAD, "--tail", "3", "--level", "warn")
        self.assertEqual(len(tail.splitlines()), 3)
        self.assertIn("Game's main stopped in frame 2210", tail)
        _, onlywin = run_logs("--run", NORMAL, "--tail", "50", "--target", "window")
        self.assertEqual(len(onlywin.splitlines()), 2)
        self.assertNotIn("files", onlywin)
        _, found = run_logs("--run", NORMAL, "--grep", "SURFACE")
        self.assertIn("2 matching lines", found)
        _, near = run_logs("--run", BAD, "--around", "f1300", "--window", "40", "--level", "warn")
        self.assertIn("f1300", near)
        self.assertNotIn("f900", near)
        _, near_ms = run_logs("--run", BAD, "--around", "12000ms", "--window", "800", "--level", "error")
        self.assertIn("f1300", near_ms)
        self.golden("bad-grep.txt", run_logs("--run", BAD, "--grep", "Lock", "--limit", "4")[1])


class Selecting(unittest.TestCase):
    def test_latest_latest_bad_prefix_and_path(self):
        runs = logs.find_runs(FIXTURES)
        self.assertEqual([r["id"] for r in runs], [NORMAL, BAD])
        self.assertEqual(logs.pick(runs, "latest")["id"], BAD)
        self.assertEqual(logs.pick(runs, "latest-bad")["id"], BAD)
        self.assertEqual(logs.pick(runs[:1], "latest-bad"), None)
        self.assertEqual(logs.pick(runs, "17948")["id"], NORMAL)
        self.assertEqual(logs.pick(runs, str(FIXTURES / f"nv-rs-{BAD}.log"))["id"], BAD)
        self.assertIsNone(logs.pick(runs, "nope"))

    def test_json_output_parses(self):
        for args in ([], ["--errors"], ["--hitches"], ["--tail", "2"]):
            code, out = run_logs("--run", BAD, "--json", *args)
            self.assertEqual(code, 0)
            json.loads(out)
        data = json.loads(run_logs("--run", BAD, "--json")[1])
        self.assertEqual(data["source"], "jsonl")
        self.assertEqual(data["stops"][0]["class"], "platform")


class Fallbacks(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        for p in FIXTURES.iterdir():
            shutil.copy(p, self.dir / p.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_the_text_log_stands_in_for_a_lost_json_file(self):
        (self.dir / f"nv-rs-{BAD}.jsonl").unlink()
        code, out = run_logs("--run", BAD, cwd=self.dir)
        self.assertEqual(code, 0)
        self.assertIn("FAILED:", out)
        self.assertIn("12x stop: soft stop", out)
        self.assertEqual(json.loads(run_logs("--run", BAD, "--json", cwd=self.dir)[1])["source"], "text")

    def test_a_run_without_a_summary_is_read_from_its_log(self):
        (self.dir / f"run-summary-{NORMAL}.json").unlink()
        code, out = run_logs("--run", NORMAL, cwd=self.dir)
        self.assertEqual(code, 0)
        self.assertIn("no run summary", out)
        self.assertIn("2x window", out)

    def test_the_rotated_half_is_read_first(self):
        old = self.dir / f"nv-rs-{NORMAL}.jsonl"
        (self.dir / f"nv-rs-{NORMAL}.jsonl.1").write_text(
            '{"ts":"2026-11-16T09:00:00.000Z","level":"error","target":"d3d9","message":"older error","frame":1}\n')
        _, out = run_logs("--run", NORMAL, "--errors", cwd=self.dir)
        self.assertIn("older error", out)
        self.assertTrue(old.exists())

    def test_an_unclean_end_is_a_bad_run(self):
        p = self.dir / f"run-summary-{NORMAL}.json"
        s = json.loads(p.read_text())
        s["ended"], s["flags"] = "running", []
        p.write_text(json.dumps(s))
        # The pid of the fixture is not a live process.
        runs = logs.find_runs(self.dir)
        self.assertTrue(logs.is_bad(runs[0]) or not sys.platform.startswith("linux"))

    def test_an_empty_folder_is_a_clear_error(self):
        empty = self.dir / "empty"
        empty.mkdir()
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code, _ = run_logs(cwd=empty)
        self.assertEqual(code, 2)
        self.assertIn("no run found", err.getvalue())


class Bundle(unittest.TestCase):
    def test_the_bundle_has_no_home_path_user_name_or_game_folder(self):
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "bundle.txt"
            code, out = run_logs("--run", BAD, "--bundle", str(target))
            self.assertEqual(code, 0)
            text = target.read_text(encoding="utf-8")
        for bad in ("someone", "Fallout New Vegas", "/home/", str(Path.home())):
            self.assertNotIn(bad, text)
        self.assertIn("FAILED:", text)
        self.assertIn("environment:", text)
        self.assertIn("worst hitches:", text)
        self.assertLess(len(text), 8000)

    def test_scrub(self):
        self.assertEqual(logs.scrub("/home/bob/x and /" "Users/al/y and C:" "\\Users\\cy\\z", home="/home/bob"), "~/x and ~/y and ~\\z")
        self.assertIn("<game folder>", logs.scrub("/mnt/SteamLibrary/steamapps/common/Fallout New Vegas/Data/x.esp"))
        self.assertEqual(logs.scrub("%USERPROFILE%\\Documents", home="/home/bob"), "~\\Documents")


class Colors(unittest.TestCase):
    def test_colors_only_when_asked_or_on_a_terminal(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            os.chdir(FIXTURES)
            try:
                logs.main(["--dir", ".", "--color", "always", "--run", BAD, "--errors"])
            finally:
                os.chdir(HERE)
        self.assertIn("\x1b[31m", out.getvalue())
        self.assertNotIn("\x1b[", run_logs("--run", BAD, "--errors")[1])


if __name__ == "__main__":
    unittest.main()
