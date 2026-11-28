# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the factory service (scripts/fork/factory_service.py), its units and its install script."""
import configparser
import io
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
FORK = HERE.parent
sys.path.insert(0, str(FORK))

import factory_queue as fq
import factory_service as fs

NOW = 1_800_000_000.0


class Case(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.queue = fq.Queue(self.dir / "queue")
        self.state = self.dir / "state"

    def attempt(self, name: str, state: str = "queued", age: float = 0.0, **extra: object) -> dict[str, object]:
        record: dict[str, object] = {"id": name, "family": name, "worktree": "w", "state": state, "retries": 0, "infra": 0,
                                     "feedback": [], "submitted": 1.0, "history": [], **extra}
        self.queue.save(record)
        os.utime(self.queue.path(name), (NOW - age, NOW - age))
        return record


class Helpers(Case):
    def test_backoff_doubles_and_stops_at_an_hour(self) -> None:
        self.assertEqual([fs.backoff(n) for n in (0, 1, 2, 3, 7, 50)], [0, 60, 120, 240, 3600, 3600])
        self.assertEqual(fs.backoff(10**7), 3600)  # no huge power

    def test_tick_results(self) -> None:
        def answer(code: int, text: str) -> fs.Runner:
            return lambda argv: (code, text)
        self.assertEqual(fs.tick("a b", answer(0, "null\n")), "idle")
        self.assertEqual(fs.tick("a b", answer(0, '{"id": 1}\n')), "worked")
        self.assertEqual(fs.tick("a b", answer(0, "")), "worked")
        self.assertEqual(fs.tick("a b", answer(3, "")), "limited")
        self.assertEqual(fs.tick("a b", answer(1, "")), "failed")

    def test_run_wraps_a_process_and_a_missing_program(self) -> None:
        self.assertEqual(fs.run([sys.executable, "-c", "print('x')"]), (0, "x\n"))
        # standard error is not part of the answer, so a warning after `null` cannot turn an idle tick into work
        self.assertEqual(fs.run([sys.executable, "-c", "import sys; print('null'); print('warn', file=sys.stderr)"]),
                         (0, "null\n"))
        code, text = fs.run(["/nonexistent/program"])
        self.assertEqual(code, 127)
        self.assertIn("/nonexistent/program", text)

    def test_json_files(self) -> None:
        path = self.dir / "a" / "b.json"
        fs.write_json(path, {"x": 1})
        self.assertEqual(fs.read_json(path), {"x": 1})
        self.assertEqual(fs.read_json(self.dir / "missing.json"), {})
        path.write_text("[1]", encoding="utf-8")
        self.assertEqual(fs.read_json(path), {})
        path.write_text("{", encoding="utf-8")
        self.assertEqual(fs.read_json(path), {})

    def test_state_dir_follows_xdg_or_home(self) -> None:
        with mock.patch.dict(os.environ, {"XDG_STATE_HOME": "/s"}):
            self.assertEqual(fs.state_dir(), Path("/s/nv-factory"))
        with mock.patch.dict(os.environ, {}, clear=True), mock.patch.object(Path, "home", return_value=Path("/h")):
            self.assertEqual(fs.state_dir(), Path("/h/.local/state/nv-factory"))

    def test_notify_only_with_a_command(self) -> None:
        calls: list[list[str]] = []
        fs.notify(None, "m", lambda argv: (calls.append(argv), (0, ""))[1])
        fs.notify("tell --urgent", "m", lambda argv: (calls.append(argv), (0, ""))[1])
        self.assertEqual(calls, [["tell", "--urgent", "m"]])

    def test_disk_line(self) -> None:
        self.assertRegex(fs.disk_line(str(self.dir)), r"^disk .*: \d+ GiB used of \d+ GiB, \d+ GiB free$")


class DefaultTick(unittest.TestCase):
    def test_the_default_tick_splits_into_the_interpreter_the_script_and_the_verb(self) -> None:
        self.assertEqual(shlex.split(fs.DEFAULT_TICK), [sys.executable, "-I", str(FORK / "factory_queue.py"), "verify"])
        self.assertEqual(shlex.split(shlex.quote("/a b/c.py")), ["/a b/c.py"])  # what the quoting buys with a space


class Resuming(Case):
    def test_a_verifying_attempt_goes_back_to_the_queue_with_its_claim_gone(self) -> None:
        self.attempt("a", "verifying")
        self.attempt("b", "accepted")
        (self.queue.directory / "a.claim").write_text("", encoding="utf-8")
        (self.queue.directory / "b.claim").write_text("", encoding="utf-8")
        self.assertEqual(fs.recover(self.queue), ["a"])
        self.assertEqual(self.queue.load("a")["state"], "queued")
        self.assertFalse((self.queue.directory / "a.claim").exists())
        self.assertEqual(fs.recover(self.queue), [])
        # the recovered attempt can be taken by the verifier again
        claimed = self.queue.claim_next(NOW)
        self.assertEqual(claimed["id"] if claimed else None, "a")


class Serving(Case):
    def run_loop(self, results: list[tuple[int, str]]) -> tuple[int, list[float]]:
        sleeps: list[float] = []
        script = iter(results)
        budget = iter(range(len(results)))
        ticks = fs.serve(self.queue, self.state, "tick", lambda argv: next(script), sleeps.append, lambda: NOW,
                         lambda: next(budget, None) is not None)
        return ticks, sleeps

    def test_idle_sleeps_a_minute_work_does_not_sleep(self) -> None:
        ticks, sleeps = self.run_loop([(0, "{}\n"), (0, "null\n")])
        self.assertEqual((ticks, sleeps), (2, [60]))
        self.assertEqual(fs.read_json(self.state / "service.json")["result"], "idle")

    def test_rate_limits_and_failures_back_off_and_reset(self) -> None:
        ticks, sleeps = self.run_loop([(3, ""), (3, ""), (1, ""), (0, "{}\n")])
        self.assertEqual((ticks, sleeps), (4, [60, 120, 240]))
        self.assertEqual(fs.read_json(self.state / "service.json")["failures"], 0)

    def test_serve_resumes_the_queue_first(self) -> None:
        self.attempt("a", "verifying")
        self.run_loop([(0, "null\n")])
        self.assertEqual(self.queue.load("a")["state"], "queued")


class Watching(Case):
    def test_an_empty_or_finished_queue_is_not_a_stall(self) -> None:
        self.assertFalse(fs.stalled(self.queue, NOW))
        self.attempt("a", "accepted", age=99999)
        self.assertFalse(fs.stalled(self.queue, NOW))

    def test_waiting_attempts_with_no_change_for_an_hour_are_a_stall(self) -> None:
        self.attempt("a", "queued", age=3601)
        self.assertTrue(fs.stalled(self.queue, NOW))
        self.attempt("b", "retry", age=10)  # something moved lately
        self.assertFalse(fs.stalled(self.queue, NOW))
        self.attempt("b", "accepted", age=3601)
        self.assertTrue(fs.stalled(self.queue, NOW))

    def test_the_alert_goes_out_once_per_six_hours(self) -> None:
        self.attempt("a", "verifying", age=4000)
        said: list[list[str]] = []
        def runner(argv: list[str]) -> tuple[int, str]:
            said.append(argv)
            return 0, ""
        self.assertEqual(fs.watchdog(self.queue, self.state, NOW, notifier="tell", runner=runner, disk=str(self.dir)), 1)
        self.assertEqual(len(said), 1)
        self.assertIn("no progress for 1 h", said[0][1])
        report = next((self.state / "diagnosis").glob("stalled-*.txt")).read_text(encoding="utf-8")
        self.assertIn("a verifying", report)
        self.assertIn('"queued": 0', report)
        self.assertEqual(fs.watchdog(self.queue, self.state, NOW + 600, notifier="tell", runner=runner, disk=str(self.dir)), 1)
        self.assertEqual(len(said), 1)  # not again
        self.assertEqual(fs.watchdog(self.queue, self.state, NOW + 7 * 3600, notifier="tell", runner=runner, disk=str(self.dir)), 1)
        self.assertEqual(len(said), 2)

    def test_a_healthy_queue_exits_zero(self) -> None:
        self.attempt("a", "queued", age=10)
        self.assertEqual(fs.watchdog(self.queue, self.state, NOW), 0)
        self.assertFalse((self.state / "diagnosis").exists())

    def test_diagnosis_of_an_empty_queue(self) -> None:
        self.assertIn("last change: never", fs.diagnose(self.queue, self.state, NOW, str(self.dir)))


class Summarising(Case):
    def test_a_file_that_vanishes_and_an_unknown_state_do_not_crash(self) -> None:
        self.attempt("a", "accepted")
        self.attempt("odd", "mystery")
        real = Path.stat
        def stat(path: Path, *args: object, **kw: object) -> os.stat_result:
            if path.name == "a.json":
                raise FileNotFoundError(path)
            return real(path, *args, **kw)  # type: ignore[arg-type]
        with mock.patch.object(Path, "stat", stat):
            text = fs.summary(self.queue, NOW, str(self.dir))
        self.assertIn("0 accepted", text)
        self.assertEqual(fs.counts(self.queue)["mystery"], 1)

    def test_the_last_day(self) -> None:
        self.attempt("old", "accepted", age=2 * fs.DAY)
        self.attempt("ok", "accepted", retries=1)
        self.attempt("bad", "rejected", retries=4, infra=1, feedback=["ruff x.py:1: unused -> fix: remove"])
        self.attempt("mute", "rejected")
        text = fs.summary(self.queue, NOW, str(self.dir))
        lines = text.splitlines()
        self.assertEqual(lines[0], "porting factory, last 24 h: 1 accepted, 2 rejected, 5 retries, 1 verifier failures")
        self.assertIn('"accepted": 2', lines[1])
        self.assertIn("blocked: bad: ruff x.py:1: unused -> fix: remove", text)
        self.assertIn("blocked: mute: no feedback", text)


class Command(Case):
    def main(self, *args: str, runner: fs.Runner | None = None, more: object = None) -> tuple[int, str]:
        out = io.StringIO()
        code = fs.main([args[0], "--queue", str(self.queue.directory), "--state", str(self.state), "--disk", str(self.dir),
                        *args[1:]], runner or (lambda argv: (0, "null\n")), lambda s: None, lambda: NOW,
                       more or (lambda: False), out)  # type: ignore[arg-type]
        return code, out.getvalue()

    def test_run_ends_when_told_to(self) -> None:
        self.assertEqual(self.main("run", "--tick", "x y")[0], 0)

    def test_run_uses_the_default_tick_from_the_environment(self) -> None:
        seen: list[list[str]] = []
        calls = iter([True, False])
        with mock.patch.dict(os.environ, {"NV_FACTORY_TICK": "my tick"}):
            self.main("run", runner=lambda argv: (seen.append(argv), (0, "null"))[1], more=lambda: next(calls))
        self.assertEqual(seen, [["my", "tick"]])

    def test_watchdog_prints_ok_or_the_diagnosis(self) -> None:
        self.assertEqual(self.main("watchdog"), (0, "ok\n"))
        self.attempt("a", "queued", age=7200)
        said: list[list[str]] = []
        with mock.patch.dict(os.environ, {"NV_FACTORY_NOTIFY": "tell"}):
            code, text = self.main("watchdog", "--stall", "3600", runner=lambda argv: (said.append(argv), (0, ""))[1])
        self.assertEqual(code, 1)
        self.assertIn("a queued", text)
        self.assertEqual(len(said), 1)

    def test_summary_is_written_and_announced(self) -> None:
        self.attempt("a", "accepted")
        said: list[list[str]] = []
        with mock.patch.dict(os.environ, {"NV_FACTORY_NOTIFY": "tell"}):
            code, text = self.main("summary", runner=lambda argv: (said.append(argv), (0, ""))[1])
        self.assertEqual(code, 0)
        written = (self.state / "summary" / "2027-01-15.txt").read_text(encoding="utf-8")
        self.assertEqual(written, text)
        self.assertIn("1 accepted", said[0][1])

    def test_status(self) -> None:
        self.attempt("a", "queued")
        code, text = self.main("status")
        self.assertEqual(code, 0)
        self.assertIn("a queued", text)


def units() -> dict[str, configparser.ConfigParser]:
    found: dict[str, configparser.ConfigParser] = {}
    for path in sorted((FORK / "systemd").iterdir()):
        parser = configparser.ConfigParser(strict=False, interpolation=None)
        parser.optionxform = str  # type: ignore[assignment,method-assign]
        parser.read(path, encoding="utf-8")
        found[path.name] = parser
    return found


class Units(unittest.TestCase):
    def test_the_slice_caps_memory_at_forty_gigabytes_with_no_swap(self) -> None:
        slice_ = units()["nv-factory.slice"]["Slice"]
        self.assertEqual((slice_["MemoryMax"], slice_["MemorySwapMax"]), ("40G", "0"))
        self.assertLess(int(slice_["MemoryHigh"].rstrip("G")), 40)

    def test_every_service_runs_in_the_slice_from_the_configured_clone(self) -> None:
        for name, unit in units().items():
            if name.endswith(".service"):
                self.assertEqual(unit["Service"]["Slice"], "nv-factory.slice", name)
                script = "fuzz_schedule.py" if name == "nv-factory-fuzz.service" else "factory_service.py"
                self.assertIn("${NV_FACTORY_REPO}/scripts/fork/" + script, unit["Service"]["ExecStart"], name)
                self.assertEqual(unit["Service"]["EnvironmentFile"], "-%E/nv-factory/env", name)

    def test_the_service_resumes_and_does_not_restart_in_a_loop(self) -> None:
        unit = units()["nv-factory.service"]
        self.assertEqual(unit["Service"]["Restart"], "on-failure")
        self.assertGreaterEqual(int(unit["Service"]["RestartSec"]), 30)
        self.assertEqual(unit["Unit"]["StartLimitBurst"], "5")
        self.assertEqual(unit["Install"]["WantedBy"], "default.target")

    def test_timers_have_a_service_each(self) -> None:
        names = units()
        for name in ("nv-factory-watchdog", "nv-factory-summary"):
            self.assertIn(f"{name}.service", names)
            self.assertEqual(names[f"{name}.service"]["Service"]["Type"], "oneshot")
        self.assertEqual(names["nv-factory-watchdog.timer"]["Timer"]["OnUnitActiveSec"], "10min")
        self.assertEqual(names["nv-factory-summary.timer"]["Timer"]["OnCalendar"], "*-*-* 08:00:00")

    def test_the_fuzz_timer_runs_nightly_and_a_finding_is_not_a_failed_unit(self) -> None:
        names = units()
        self.assertEqual(names["nv-factory-fuzz.timer"]["Timer"]["OnCalendar"], "*-*-* 02:30:00")
        self.assertEqual(names["nv-factory-fuzz.timer"]["Timer"]["Persistent"], "true")
        service = names["nv-factory-fuzz.service"]["Service"]
        self.assertEqual((service["Type"], service["SuccessExitStatus"]), ("oneshot", "1"))
        self.assertIn("--seconds 600", service["ExecStart"])
        self.assertIn("--repo JorisJonkers-dev/nv-rs", service["ExecStart"])
        self.assertEqual(service["TimeoutStartSec"], "3h")

    def test_every_unit_names_its_licence(self) -> None:
        for path in (FORK / "systemd").iterdir():
            self.assertIn("SPDX-License-Identifier: GPL-3.0-only", path.read_text(encoding="utf-8"), path.name)


@unittest.skipUnless(shutil.which("bash"), "needs bash")
class Install(unittest.TestCase):
    """The install script against a fake systemctl and a throwaway home."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.home = Path(self.tmp.name)
        bin_ = self.home / "bin"
        bin_.mkdir()
        fake = bin_ / "systemctl"
        fake.write_text(f'#!/bin/sh\necho "$@" >> "{self.home}/systemctl.log"\n', encoding="utf-8")
        fake.chmod(0o755)
        self.env = {"PATH": f"{bin_}:{os.environ['PATH']}", "HOME": str(self.home), "XDG_CONFIG_HOME": str(self.home / "cfg")}

    def install(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["bash", str(FORK / "install-factory-service.sh"), *args], env=self.env, text=True,
                              capture_output=True, check=False)

    def log(self) -> str:
        path = self.home / "systemctl.log"
        return path.read_text(encoding="utf-8") if path.exists() else ""

    def test_install_copies_and_reloads_but_enables_nothing(self) -> None:
        done = self.install()
        self.assertEqual(done.returncode, 0, done.stderr)
        target = self.home / "cfg" / "systemd" / "user"
        self.assertEqual(sorted(p.name for p in target.iterdir()), sorted(p.name for p in (FORK / "systemd").iterdir()))
        env = (self.home / "cfg" / "nv-factory" / "env").read_text(encoding="utf-8")
        self.assertEqual(env.strip(), f"NV_FACTORY_REPO={FORK.parent.parent}")
        self.assertEqual(self.log().strip(), "--user daemon-reload")
        self.assertIn("installed, not enabled", done.stdout)

    def test_an_existing_env_file_is_kept(self) -> None:
        envfile = self.home / "cfg" / "nv-factory" / "env"
        envfile.parent.mkdir(parents=True)
        envfile.write_text("NV_FACTORY_REPO=/mine\n", encoding="utf-8")
        self.assertEqual(self.install().returncode, 0)
        self.assertEqual(envfile.read_text(encoding="utf-8"), "NV_FACTORY_REPO=/mine\n")

    def test_enable_starts_the_service_and_the_timers(self) -> None:
        self.assertEqual(self.install("--enable").returncode, 0)
        log = self.log()
        for unit in ("nv-factory.service", "nv-factory-watchdog.timer", "nv-factory-summary.timer",
                     "nv-factory-fuzz.timer"):
            self.assertIn(f"--user enable --now {unit}", log)

    def test_dry_run_changes_nothing(self) -> None:
        done = self.install("--dry-run", "--enable")
        self.assertEqual(done.returncode, 0)
        self.assertIn("would: systemctl --user enable --now nv-factory.service", done.stdout)
        self.assertIn("would write", done.stdout)
        self.assertFalse((self.home / "cfg").exists())
        self.assertEqual(self.log(), "")

    def test_uninstall_disables_and_removes(self) -> None:
        self.install()
        done = self.install("--uninstall")
        self.assertEqual(done.returncode, 0)
        self.assertEqual(list((self.home / "cfg" / "systemd" / "user").iterdir()), [])
        self.assertIn("--user disable --now nv-factory.service", self.log())

    def test_a_bad_argument_is_refused(self) -> None:
        done = self.install("--nope")
        self.assertEqual(done.returncode, 2)
        self.assertIn("usage", done.stderr)


class Entry(unittest.TestCase):
    def test_the_script_runs_as_a_program(self) -> None:
        done = subprocess.run([sys.executable, "-I", str(FORK / "factory_service.py"), "--help"], text=True, capture_output=True,
                              check=False)
        self.assertEqual(done.returncode, 0)
        self.assertIn("watchdog", done.stdout)


if __name__ == "__main__":
    unittest.main()
