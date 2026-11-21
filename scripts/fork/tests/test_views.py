# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the bench `views` scenario (nv-rs#343): the view list, the manifest, the comparison of two runs and the
driver's order of steps against a fake game. Synthetic only: no game, no GPU. Run:
python3 -I -m unittest discover -s scripts/fork/tests -v
"""
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "bench"))

import drivers  # noqa: E402
import nvbench  # noqa: E402
import scenarios  # noqa: E402
import views  # noqa: E402


class ParseTests(unittest.TestCase):
    def test_a_view_is_name_x_y_z_heading_in_either_separator(self):
        v = views.parse_view("lake-mead:53808:38384:3850:60")
        self.assertEqual((v.name, v.x, v.y, v.z, v.heading), ("lake-mead", 53808.0, 38384.0, 3850.0, 60.0))
        self.assertEqual(views.parse_view("lake-mead 53808 38384 3850 60"), v)
        self.assertEqual(views.parse_view("lake-mead,53808,38384,3850,60"), v)

    def test_the_heading_is_taken_modulo_a_turn_and_negative_coordinates_are_kept(self):
        v = views.parse_view("a:-70200:2560.5:9500:-90")
        self.assertEqual((v.x, v.y, v.heading), (-70200.0, 2560.5, 270.0))
        self.assertEqual(views.parse_view("a:0:0:0:450").heading, 90.0)

    def test_bad_views_are_refused_with_the_reason(self):
        for bad, word in (("a:1:2:3", "name:x:y:z:heading"), ("a:1:2:3:x", "heading"), ("bad name:1:2:3:4", "name:x:y:z:heading"),
                          ("-a:1:2:3:4", "file name"), ("a/b:1:2:3:4", "file name"), ("", "name:x:y:z:heading")):
            with self.assertRaises(views.ViewError, msg=bad) as cm:
                views.parse_view(bad)
            self.assertIn(word, str(cm.exception))

    def test_a_file_has_one_view_a_line_with_comments_and_blanks(self):
        text = "# the long views\n\nnorth 0 100 50 0   # up the road\n\neast 0 100 50 90\n"
        self.assertEqual([v.name for v in views.parse_views(text)], ["north", "east"])
        self.assertEqual([v.name for v in views.parse_views("a:0:0:0:0; b:1:1:1:1")], ["a", "b"])

    def test_names_must_differ_and_the_list_must_not_be_empty(self):
        with self.assertRaises(views.ViewError):
            views.parse_views("a 0 0 0 0\na 1 1 1 1")
        with self.assertRaises(views.ViewError):
            views.parse_views("# nothing\n")

    def test_resolve_takes_the_command_line_then_the_file_then_the_default(self):
        self.assertEqual(views.resolve(), views.DEFAULT_VIEWS)
        self.assertEqual([v.name for v in views.resolve("x:0:0:0:0")], ["x"])
        with tempfile.TemporaryDirectory() as t:
            p = Path(t) / "v.txt"
            p.write_text("f 1 2 3 4\n")
            self.assertEqual([v.name for v in views.resolve(None, str(p))], ["f"])
            with self.assertRaises(views.ViewError):
                views.resolve("x:0:0:0:0", str(p))
            with self.assertRaises(views.ViewError):
                views.resolve(None, str(Path(t) / "missing.txt"))

    def test_the_default_list_is_valid_and_has_land_and_water_views(self):
        names = [v.name for v in views.DEFAULT_VIEWS]
        self.assertEqual(len(names), len(set(names)))
        self.assertTrue(all(views.NAME.match(n) for n in names))
        self.assertTrue(any("lake" in n or "river" in n for n in names) and any("valley" in n or "vegas" in n for n in names))
        self.assertTrue(all(0.0 <= v.heading < 360.0 for v in views.DEFAULT_VIEWS))

    def test_hour_and_weather(self):
        self.assertEqual(views.parse_hour("6.5"), 6.5)
        for bad in ("24", "-1", "noon"):
            with self.assertRaises(views.ViewError):
                views.parse_hour(bad)
        self.assertEqual(views.parse_weather("0ffc88"), "0FFC88")
        self.assertIsNone(views.parse_weather("none"))
        with self.assertRaises(views.ViewError):
            views.parse_weather("clear")

    def test_the_console_lines_set_the_position_then_the_heading(self):
        v = views.View("a", -70200.0, 2560.5, 9500.0, 90.0)
        self.assertEqual(views.console_lines(v), ["player.setpos x -70200", "player.setpos y 2560.5", "player.setpos z 9500", "player.setangle z 90"])


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.vs = [views.View("a", 0.0, 0.0, 10.0, 0.0), views.View("b", 100.0, 0.0, 10.0, 90.0)]

    def manifest(self, **kw):
        caps = {"a": {"observed": [0.2, 0.1, 9.0], "file": "views/a.png"}, "b": {"observed": [100.0, 0.0, 9.0], "file": "views/b.png"}}
        caps.update(kw)
        return views.manifest(self.vs, 12.0, "0FFC88", 15.0, caps)

    def test_the_manifest_has_the_sky_and_each_view_with_what_was_seen(self):
        m = self.manifest()
        self.assertEqual((m["hour"], m["timescale"], m["weather"]), (12.0, 0, "0FFC88"))
        self.assertIn("fw 0FFC88", m["weather_note"])
        self.assertEqual([v["name"] for v in m["views"]], ["a", "b"])
        self.assertTrue(all(v["captured"] for v in m["views"]))
        self.assertEqual(m["views"][0]["observed"], [0.2, 0.1, 9.0])
        json.dumps(m)

    def test_an_unset_weather_is_recorded_as_not_fixed(self):
        m = views.manifest(self.vs, 12.0, None, 15.0, {})
        self.assertIsNone(m["weather"])
        self.assertIn("not fixed", m["weather_note"])
        self.assertFalse(any(v["captured"] for v in m["views"]))

    def test_two_runs_that_agree_have_no_problem(self):
        self.assertEqual(views.compare(self.manifest(), self.manifest()), [])
        near = self.manifest(a={"observed": [0.6, 0.6, 25.0], "file": "views/a.png"})  # landed 16 units higher: a slope
        self.assertEqual(views.compare(self.manifest(), near), [])

    def test_positions_headings_and_sky_that_differ_are_reported(self):
        far = self.manifest(a={"observed": [3.2, 0.1, 9.0], "file": "views/a.png"})
        self.assertTrue(any("a: positions differ by 3.00" in p for p in views.compare(self.manifest(), far)))
        high = self.manifest(a={"observed": [0.2, 0.1, 49.0], "file": "views/a.png"})
        self.assertTrue(any("a: heights differ by 40.0" in p for p in views.compare(self.manifest(), high)))
        turned = self.manifest()
        turned["views"][1]["heading"] = 92.0
        self.assertTrue(any("b: headings differ by 2.00" in p for p in views.compare(self.manifest(), turned)))
        wrapped = self.manifest()
        wrapped["views"][0]["heading"] = 359.5
        self.assertEqual(views.compare(self.manifest(), wrapped), [])  # 0.5 degrees across north
        other = self.manifest()
        other["hour"] = 9.0
        self.assertTrue(any(p.startswith("hour:") for p in views.compare(self.manifest(), other)))

    def test_a_view_that_was_not_captured_or_is_in_one_run_only_is_reported(self):
        missing = self.manifest(b={"observed": None, "file": None})
        self.assertTrue(any("b: not captured in one run" in p for p in views.compare(self.manifest(), missing)))
        fewer = self.manifest()
        fewer["views"].pop()
        self.assertTrue(any("b: only in one of the runs" in p for p in views.compare(self.manifest(), fewer)))

    def test_the_observed_positions_come_from_the_driver_s_state_lines(self):
        got = views.observed_from_states({"view.a": [1.0, 2.0, 3.0]}, self.vs)
        self.assertEqual(got, {"a": [1.0, 2.0, 3.0], "b": None})


class ScenarioTests(unittest.TestCase):
    def test_views_is_a_driven_scenario_with_checks_for_its_views(self):
        self.assertIn("views", scenarios.SCENARIOS)
        self.assertIs(drivers.DRIVERS["views"], drivers.views)
        plan = scenarios.build("views", {"fps": 60.0, "views": [views.View("a", 0.0, 0.0, 0.0, 0.0)]})
        self.assertIs(plan.drive, drivers.views)
        self.assertEqual(plan.keys, [])
        check = dict(plan.checks)["every view has its picture"]
        self.assertEqual(check({"states": {"view.a": [0, 0, 0]}}), (True, "1 views captured"))
        ok, detail = check({"states": {}})
        self.assertFalse(ok)
        self.assertIn("a", detail)

    def test_bench_list_shows_it(self):
        self.assertIn("views", scenarios.SCENARIOS)
        self.assertEqual(scenarios.SCENARIOS["views"]({"fps": 60.0}).name, "views")

    def test_the_command_line_makes_the_options(self):
        import argparse

        def args(**kw):
            base = dict(scenario="views", views=None, views_file=None, hour=None, timescale=None, weather=None, views_settle=None)
            base.update(kw)
            return argparse.Namespace(**base)

        o = nvbench.views_options(args())
        self.assertEqual((o["hour"], o["weather"], o["views_settle"]), (views.HOUR, views.WEATHER, views.SETTLE_S))
        self.assertEqual(o["views"], views.DEFAULT_VIEWS)
        o = nvbench.views_options(args(views="x:1:2:3:4", hour="9", weather="none", views_settle=5.0))
        self.assertEqual((o["hour"], o["weather"], o["views_settle"], [v.name for v in o["views"]]), (9.0, None, 5.0, ["x"]))
        self.assertEqual(nvbench.views_options(args(scenario="goodsprings")), {})
        with self.assertRaises(views.ViewError):
            nvbench.views_options(args(views="broken"))

    def test_the_run_options_exist_on_the_command_line(self):
        import io
        from contextlib import redirect_stdout

        a = nvbench.argparse.ArgumentParser()
        nvbench.run_args(a)
        got = a.parse_args(["--views", "a:0:0:0:0", "--hour", "10", "--weather", "none", "--views-settle", "3"])
        self.assertEqual((got.views, got.hour, got.weather, got.views_settle), ("a:0:0:0:0", "10", "none", 3.0))
        with redirect_stdout(io.StringIO()):
            self.assertEqual(nvbench.main(["list"]), 0)


class FakeGame:
    """Stands in for driver.Driver: a player that goes where the console's setpos lines send it."""

    def __init__(self, run_dir, grim=True):
        self.frames = str(Path(run_dir) / "frames.tsv")
        self.lines, self.notes, self.shots, self.grim = [], [], [], grim
        self.pos = [0.0, 0.0, 0.0]
        self.open = False
        self.t0 = 0.0

    def alive(self):
        return True

    def position(self):
        return tuple(self.pos)

    def screen(self):
        return "menu" if self.open else "play"

    def note(self, text):
        self.notes.append(text)

    def clock(self):
        return 0.0

    def wait(self, s):
        pass

    def tail(self, n=1024):
        return [(1, "play", 0.0, 0.0, 0.0, 0.01)]

    def until(self, cond, timeout, every=0.25, what=""):
        return cond() or self.alive()

    def settle_play(self, **kw):
        self.open = False
        return True

    def console_open(self, tries=3):
        self.open = True
        return True

    def console_close(self, quiet=2.0, timeout=30.0):
        self.open = False
        return True

    def console_line(self, text, settle=1.0):
        self.lines.append(text)
        parts = text.split()
        if parts[0] == "player.setpos":
            self.pos["xyz".index(parts[1])] = float(parts[2])
        return True

    def capture(self, path):
        if not self.grim:
            self.note("no picture: grim is not installed")
            return False
        Path(path).write_bytes(b"png")
        self.shots.append(path)
        return True


class DriverTests(unittest.TestCase):
    def run_views(self, game, opts):
        saved = (drivers.start_new_game, drivers.at_marker, drivers.warm_first_exterior, drivers.calm)
        drivers.start_new_game = lambda d, **k: True
        drivers.at_marker = lambda d, m, **k: True
        drivers.warm_first_exterior = lambda d, **k: None
        drivers.calm = lambda d, **k: True
        try:
            drivers.views(game, opts)
        finally:
            drivers.start_new_game, drivers.at_marker, drivers.warm_first_exterior, drivers.calm = saved

    def test_the_sky_is_set_once_then_each_view_is_placed_photographed_and_noted_in_order(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            vs = [views.View("a", 10.0, 20.0, 30.0, 45.0), views.View("b", -5.0, 6.0, 7.0, 270.0)]
            self.run_views(g, {"views": vs, "weather": "0FFC88", "hour": 12.0, "timescale": 0})
            # god mode and no collision, the clock stopped, run on to the hour (reached by the clock, not by `set gamehour`:
            # that stops the port), stopped again, the weather forced; then the views
            self.assertEqual(g.lines[:6], ["tgm", "set timescale to 0", "set timescale to 600", "set timescale to 0", "fw 0FFC88", views.console_lines(vs[0])[0]])
            self.assertFalse(any("gamehour" in line for line in g.lines))
            self.assertEqual(g.lines[5:], views.console_lines(vs[0]) + views.console_lines(vs[1]))
            self.assertEqual([Path(s).name for s in g.shots], ["a.png", "b.png"])
            self.assertEqual([n for n in g.notes if n.startswith("state ")], ["state view.a x=10.0 y=20.0 z=30.0", "state view.b x=-5.0 y=6.0 z=7.0"])
            m = json.loads((Path(t) / views.MANIFEST).read_text())
            self.assertEqual([v["file"] for v in m["views"]], ["views/a.png", "views/b.png"])
            self.assertEqual(m["views"][1]["observed"], [-5.0, 6.0, 7.0])
            self.assertEqual(m["weather"], "0FFC88")

    def test_no_weather_line_when_the_game_s_own_weather_is_kept(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            self.run_views(g, {"views": [views.View("a", 1.0, 2.0, 3.0, 0.0)], "weather": None, "hour": None})
            self.assertFalse(any(line.startswith("fw ") for line in g.lines))
            m = json.loads((Path(t) / views.MANIFEST).read_text())
            self.assertIsNone(m["weather"])
            self.assertIn("not fixed", m["weather_note"])

    def test_a_missing_picture_tool_leaves_the_view_uncaptured_and_no_state_line(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t, grim=False)
            self.run_views(g, {"views": [views.View("a", 1.0, 2.0, 3.0, 0.0)]})
            self.assertFalse(any(n.startswith("state ") for n in g.notes))
            m = json.loads((Path(t) / views.MANIFEST).read_text())
            self.assertFalse(m["views"][0]["captured"])

    def test_a_player_that_does_not_arrive_is_noted_and_the_next_view_goes_on(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            real = g.until
            g.until = lambda cond, timeout, every=0.25, what="": False if what.startswith("the move to a") else real(cond, timeout, every, what)
            vs = [views.View("a", 1.0, 2.0, 3.0, 0.0), views.View("b", 4.0, 5.0, 6.0, 0.0)]
            self.run_views(g, {"views": vs})
            self.assertIn("a: not reached", g.notes)
            self.assertEqual([Path(s).name for s in g.shots], ["b.png"])

    def test_the_clock_runs_until_the_play_seconds_of_the_frame_log_add_up_and_a_menu_does_not_count(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            frames = iter([[(1, "play", 0, 0, 0, 0.5)], [(1, "play", 0, 0, 0, 0.5), (2, "menu", 0, 0, 0, 9.0), (3, "play", 0, 0, 0, 1.0)],
                           [(3, "play", 0, 0, 0, 1.0), (4, "play", 0, 0, 0, 2.0), (4, "play", 0, 0, 0, 7.0)], [(4, "play", 0, 0, 0, 2.0)] * 3])
            seen = []
            g.tail = lambda n=1024: seen.append(n) or next(frames, [(4, "play", 0, 0, 0, 2.0)])
            g.until = lambda cond, timeout, every=0.25, what="": [cond() for _ in range(6)][-1]
            # 600 game seconds a second: one game hour is 6 s of play (less the open's latency)
            passed = drivers.run_clock(g, 1.0, scale=600.0)
            self.assertEqual(passed, 3.0)  # 1.0 and 2.0 of play; the menu frame's 9 s and a repeated frame are not counted
            self.assertIn("set timescale to 600", g.lines)
            self.assertTrue(g.open)  # the console is open again, ready to stop the clock

    def test_a_game_that_stopped_on_the_way_gets_no_picture_and_no_state_line(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            lines = g.console_line

            def line(text, settle=1.0):
                if text.startswith("player.setpos y 5"):
                    g.alive = lambda: False  # the port stopped while the player was moving
                return lines(text, settle)

            g.console_line = line
            vs = [views.View("a", 1.0, 2.0, 3.0, 0.0), views.View("b", 4.0, 5.0, 6.0, 0.0), views.View("c", 7.0, 8.0, 9.0, 0.0)]
            self.run_views(g, {"views": vs, "hour": None})
            self.assertEqual([Path(s).name for s in g.shots], ["a.png"])
            self.assertIn("b: the game stopped before the picture", g.notes)
            m = json.loads((Path(t) / views.MANIFEST).read_text())
            self.assertEqual([v["captured"] for v in m["views"]], [True, False, False])

    def test_near_xy_and_calm(self):
        v = views.View("a", 100.0, 100.0, 0.0, 0.0)
        self.assertTrue(drivers.near_xy((130.0, 100.0, 5.0), v))
        self.assertFalse(drivers.near_xy((500.0, 100.0, 5.0), v))
        self.assertFalse(drivers.near_xy(None, v))

        class D:
            def __init__(self, rows):
                self.rows = rows

            def tail(self, n):
                return self.rows

        even = [(i, "play", 1.0, 2.0, 3.0, 0.008) for i in range(80)]
        self.assertTrue(drivers.calm(D(even)))
        self.assertFalse(drivers.calm(D(even[:10])))  # too few frames to judge
        spike = even[:40] + [(40, "play", 1.0, 2.0, 3.0, 0.5)] + even[41:]
        self.assertFalse(drivers.calm(D(spike)))
        falling = [(i, "play", 1.0, 2.0, 3.0 - i, 0.008) for i in range(80)]
        self.assertFalse(drivers.calm(D(falling)))


if __name__ == "__main__":
    unittest.main()
