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
        self.assertEqual((v.name, v.x, v.y, v.z, v.heading, v.pitch), ("lake-mead", 53808.0, 38384.0, 3850.0, 60.0, 0.0))
        self.assertEqual(views.parse_view("lake-mead 53808 38384 3850 60"), v)
        self.assertEqual(views.parse_view("lake-mead,53808,38384,3850,60"), v)

    def test_a_view_may_give_its_pitch_and_it_is_kept_within_bounds(self):
        v = views.parse_view("a:1:2:3:90:-12.5")
        self.assertEqual((v.heading, v.pitch), (90.0, -12.5))
        self.assertEqual(views.parse_view("a 1 2 3 90 10").pitch, 10.0)
        for bad in ("a:1:2:3:90:81", "a:1:2:3:90:x", "a:1:2:3:90:1:2"):
            with self.assertRaises(views.ViewError, msg=bad):
                views.parse_view(bad)

    def test_the_heading_is_taken_modulo_a_turn_and_negative_coordinates_are_kept(self):
        v = views.parse_view("a:-70200:2560.5:9500:-90")
        self.assertEqual((v.x, v.y, v.heading), (-70200.0, 2560.5, 270.0))
        self.assertEqual(views.parse_view("a:0:0:0:450").heading, 90.0)

    def test_bad_views_are_refused_with_the_reason(self):
        for bad, word in (("a:1:2:3", "name:x:y:z:heading[:pitch]"), ("a:1:2:3:x", "heading"), ("bad name:1:2:3:4", "not a number"),
                          ("-a:1:2:3:4", "file name"), ("a/b:1:2:3:4", "file name"), ("", "name:x:y:z:heading[:pitch]")):
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

    def test_the_default_list_is_the_five_places_with_a_standing_height_and_a_pitch_each(self):
        self.assertEqual([v.name for v in views.DEFAULT_VIEWS], ["goodsprings-valley", "vegas-from-the-south", "lake-mead", "colorado-river", "black-mountain-west"])
        self.assertTrue(all(-80.0 <= v.pitch <= 80.0 for v in views.DEFAULT_VIEWS))

    def test_a_view_knows_its_cell(self):
        self.assertEqual(views.View("a", 53811.0, 38382.0, 0.0, 0.0).cell(), (13, 9))
        self.assertEqual(views.View("a", -70200.0, 2560.0, 0.0, 0.0).cell(), (-18, 0))
        self.assertEqual(views.View("a", 62699.0, -97604.0, 0.0, 0.0).cell(), (15, -24))

    def test_hour_and_weather(self):
        self.assertEqual(views.parse_hour("6.5"), 6.5)
        for bad in ("24", "-1", "noon"):
            with self.assertRaises(views.ViewError):
                views.parse_hour(bad)
        self.assertEqual(views.parse_weather("0ffc88"), "0FFC88")
        self.assertIsNone(views.parse_weather("none"))
        with self.assertRaises(views.ViewError):
            views.parse_weather("clear")

    def test_the_console_lines_put_the_player_a_little_above_its_standing_height_then_turn_it(self):
        v = views.View("a", -70200.0, 2560.5, 9500.0, 90.0, -5.0)
        self.assertEqual(views.console_lines(v), ["player.setpos x -70200", "player.setpos y 2560.5", f"player.setpos z {9500 + views.DROP_UNITS:g}",
                                                  "player.setangle z 90", "player.setangle x -5"])


def window(n=400, dt=0.01, x=10.0, y=20.0, z=30.0, screen="play"):
    return [(i, screen, x, y, z, dt) for i in range(n)]


class StandingTests(unittest.TestCase):
    view = views.View("a", 10.0, 20.0, 30.0, 0.0)

    def test_a_player_that_stood_still_at_the_height_for_the_whole_window_stands(self):
        ok, why, stats = views.standing(window(), self.view)
        self.assertTrue(ok, why)
        self.assertEqual((stats["z_range"], stats["xy_range"], stats["dz"], stats["dxy"]), (0.0, 0.0, 0.0, 0.0))
        self.assertGreaterEqual(stats["window_s"], views.STAND_S)

    def test_too_few_frames_to_judge_is_not_standing(self):
        ok, why, _ = views.standing(window(n=100), self.view)
        self.assertFalse(ok)
        self.assertIn("only 1.0 s", why)

    def test_a_player_still_falling_is_not_standing(self):
        rows = window() [:300] + [(300 + i, "play", 10.0, 20.0, 30.0 + (100 - i), 0.01) for i in range(100)]
        ok, why, _ = views.standing(rows, self.view)
        self.assertFalse(ok)
        self.assertIn("moving", why)

    def test_a_player_that_moved_or_was_pushed_is_not_standing(self):
        rows = window(n=399) + [(399, "play", 10.0, 20.5, 30.0, 0.01)]
        self.assertIn("moving", views.standing(rows, self.view)[1])

    def test_a_player_at_the_wrong_height_or_place_is_not_standing(self):
        ok, why, _ = views.standing(window(z=45.0), self.view)
        self.assertFalse(ok)
        self.assertIn("from the view's height", why)  # standing still, but on a roof or in the water
        ok, why, _ = views.standing(window(x=13.0), self.view)
        self.assertFalse(ok)
        self.assertIn("from the view's place", why)

    def test_a_load_or_a_menu_in_the_window_is_not_standing(self):
        spike = window()[:200] + [(200, "play", 10.0, 20.0, 30.0, 0.4)] + window()[201:]
        self.assertIn("a frame of 0.40 s", views.standing(spike, self.view)[1])
        menu = window()[:300] + window(n=100, screen="menu")
        self.assertIn("play screen", views.standing(menu, self.view)[1])

    def test_the_window_counts_the_frames_deltas_not_the_frames(self):
        slow = window(n=40, dt=0.09)  # 40 frames of 90 ms: 3.6 s
        self.assertTrue(views.standing(slow, self.view)[0])

    def test_the_stance_record_says_what_was_proved_and_what_was_never_allowed(self):
        rec = views.stance_record({"window_s": 3.0})
        self.assertEqual((rec["pose"], rec["sneaking"], rec["moving"], rec["falling"], rec["camera"], rec["weapon"]), ("standing", False, False, False, "first person", "none drawn"))


def png(path, w, h, fill, rgba=False, filt=0):
    import struct
    import zlib

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    bpp = 4 if rgba else 3
    raw = b"".join(bytes([filt]) + bytes(fill(x, y, c) for x in range(w) for c in range(bpp)) for y in range(h))
    Path(path).write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6 if rgba else 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


class PictureTests(unittest.TestCase):
    def test_equal_pictures_differ_by_nothing_and_changed_ones_by_the_mean_and_the_peak(self):
        with tempfile.TemporaryDirectory() as t:
            a, b, c = (Path(t) / n for n in ("a.png", "b.png", "c.png"))
            png(a, 4, 2, lambda x, y, c: 10)
            png(b, 4, 2, lambda x, y, c: 10)
            png(c, 4, 2, lambda x, y, c: 10 if x else 90)  # one column of four, 80 higher
            self.assertEqual(views.picture_difference(a, b), (0.0, 0, ""))
            mean, peak, note = views.picture_difference(a, c)
            self.assertEqual((mean, peak, note), (20.0, 80, ""))

    def test_the_filters_of_a_png_are_undone_and_alpha_is_ignored(self):
        with tempfile.TemporaryDirectory() as t:
            a, b = Path(t) / "a.png", Path(t) / "b.png"
            png(a, 3, 3, lambda x, y, c: 50, rgba=True)
            png(b, 3, 3, lambda x, y, c: 50)
            self.assertEqual(views.picture_difference(a, b)[:2], (0.0, 0))
            self.assertEqual(views.read_png(a)[:3], (3, 3, 4))

    def test_pictures_of_different_sizes_are_not_compared(self):
        with tempfile.TemporaryDirectory() as t:
            a, b = Path(t) / "a.png", Path(t) / "b.png"
            png(a, 3, 3, lambda x, y, c: 1)
            png(b, 4, 3, lambda x, y, c: 1)
            self.assertIn("sizes differ", views.picture_difference(a, b)[2])


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.vs = [views.View("a", 0.0, 0.0, 10.0, 0.0), views.View("b", 100.0, 0.0, 10.0, 90.0, -3.0)]

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
        near = self.manifest(a={"observed": [0.6, 0.6, 10.5], "file": "views/a.png"})  # 1.5 units higher, 0.8 aside
        self.assertEqual(views.compare(self.manifest(), near), [])

    def test_positions_headings_and_sky_that_differ_are_reported(self):
        far = self.manifest(a={"observed": [3.2, 0.1, 9.0], "file": "views/a.png"})
        self.assertTrue(any("a: positions differ by 3.00" in p for p in views.compare(self.manifest(), far)))
        high = self.manifest(a={"observed": [0.2, 0.1, 49.0], "file": "views/a.png"})
        self.assertTrue(any("a: heights differ by 40.0" in p for p in views.compare(self.manifest(), high)))
        tilted = self.manifest()
        tilted["views"][0]["pitch"] = 4.0
        self.assertTrue(any("a: pitches differ by 4.00" in p for p in views.compare(self.manifest(), tilted)))
        crouched = self.manifest(a={"observed": [0.2, 0.1, 9.0], "file": "views/a.png", "stance": dict(views.stance_record({}), pose="sneaking")})
        self.assertTrue(any("a: stance pose 'sneaking' against None" in p or "stance pose" in p for p in views.compare(self.manifest(a={"observed": [0.2, 0.1, 9.0], "file": "views/a.png", "stance": views.stance_record({})}), crouched)))
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
        check = dict(plan.checks)["every view has its picture, taken standing"]
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
            base = dict(scenario="views", views=None, views_file=None, hour=None, timescale=None, weather=None, views_settle=None, views_console=None)
            base.update(kw)
            return argparse.Namespace(**base)

        o = nvbench.views_options(args())
        self.assertEqual((o["hour"], o["weather"], o["views_settle"]), (views.HOUR, views.WEATHER, views.SETTLE_S))
        self.assertEqual(o["views"], views.DEFAULT_VIEWS)
        o = nvbench.views_options(args(views="x:1:2:3:4", hour="9", weather="none", views_settle=5.0))
        self.assertEqual((o["hour"], o["weather"], o["views_settle"], [v.name for v in o["views"]]), (9.0, None, 5.0, ["x"]))
        self.assertEqual(nvbench.views_options(args(views_console=["tll all"]))["views_console"], ["tll all"])
        self.assertEqual(nvbench.views_options(args(scenario="goodsprings")), {})
        with self.assertRaises(views.ViewError):
            nvbench.views_options(args(views="broken"))

    def test_the_run_options_exist_on_the_command_line(self):
        import io
        from contextlib import redirect_stdout

        a = nvbench.argparse.ArgumentParser()
        nvbench.run_args(a)
        got = a.parse_args(["--views", "a:0:0:0:0", "--hour", "10", "--weather", "none", "--views-settle", "3", "--views-console", "tll all", "--views-console", "tm"])
        self.assertEqual((got.views, got.hour, got.weather, got.views_settle, got.views_console), ("a:0:0:0:0", "10", "none", 3.0, ["tll all", "tm"]))
        with redirect_stdout(io.StringIO()):
            self.assertEqual(nvbench.main(["list"]), 0)


class FakeGame:
    """Stands in for driver.Driver: a player that goes where the console's cow and setpos lines send it, and that stands at
    `ground(x, y)` (the view's height by default) once put there, so the frame rows the driver reads are those of a player
    standing still."""

    def __init__(self, run_dir, grim=True, ground=None):
        self.frames = str(Path(run_dir) / "frames.tsv")
        self.lines, self.notes, self.shots, self.grim = [], [], [], grim
        self.pos = [0.0, 0.0, 0.0]
        self.open = False
        self.t0 = 0.0
        self.ground = ground or (lambda x, y: None)  # None: the height asked for, less the drop
        self.rows_override = None
        self.load_frame = 100
        self.loads = True  # whether a `cow` is followed by the game's load event

    def game_events(self, nbytes=0):
        return [(self.load_frame, "load.kind", "the load is a door or another cell change")]

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
        if self.rows_override is not None:
            return self.rows_override
        return [(i, "play", self.pos[0], self.pos[1], self.pos[2], 0.01) for i in range(400)]

    def until(self, cond, timeout, every=0.25, what=""):
        return cond()

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
        if parts[0] == "cow":
            if self.loads:
                self.load_frame += 500
            self.pos = [parts[2] and float(parts[2]) * 4096.0 + 2000.0, float(parts[3]) * 4096.0 + 2000.0, 1.0]
        if parts[0] == "player.setpos":
            self.pos["xyz".index(parts[1])] = float(parts[2])
            if parts[1] == "z":
                ground = self.ground(self.pos[0], self.pos[1])
                self.pos[2] = ground if ground is not None else self.pos[2] - views.DROP_UNITS  # it fell onto the ground
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
        saved = (drivers.start_new_game, drivers.at_marker, drivers.warm_first_exterior)
        drivers.start_new_game = lambda d, **k: True
        drivers.at_marker = lambda d, m, **k: True
        drivers.warm_first_exterior = lambda d, **k: None
        try:
            drivers.views(game, opts)
        finally:
            drivers.start_new_game, drivers.at_marker, drivers.warm_first_exterior = saved

    def view_lines(self, view, weather="0FFC88"):
        cx, cy = view.cell()
        return [f"cow WastelandNV {cx} {cy}"] + ([f"fw {weather}"] * 2 if weather else []) + views.console_lines(view)

    def test_the_sky_and_camera_are_fixed_once_then_each_view_is_loaded_placed_photographed_and_noted_in_order(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            vs = [views.View("a", 10.0, 20.0, 30.0, 45.0, 5.0), views.View("b", -5000.0, 6.0, 7.0, 270.0)]
            self.run_views(g, {"views": vs, "weather": "0FFC88", "hour": 12.0, "timescale": 0})
            # god mode, the clock stopped, run on to the hour (reached by the clock, not by `set gamehour`: that stops the port),
            # stopped again, the weather forced and sent again; the idle camera set off and the interface hidden once; then the views
            self.assertEqual(g.lines[:8], ["tgm", "set timescale to 0", "set timescale to 600", "set timescale to 0", "fw 0FFC88",
                                           "setgs fVanityModeAutoDelay 1000000", "tm", self.view_lines(vs[0])[0]])
            self.assertFalse(any("gamehour" in line for line in g.lines))
            self.assertEqual(g.lines[7:], self.view_lines(vs[0]) + self.view_lines(vs[1]))  # the weather again after each cow: a new region brings its own
            self.assertEqual(g.lines.count("tm"), 1)
            self.assertEqual([Path(s).name for s in g.shots], ["a.png", "b.png"])
            self.assertEqual([n for n in g.notes if n.startswith("state ")], ["state view.a x=10.0 y=20.0 z=30.0", "state view.b x=-5000.0 y=6.0 z=7.0"])
            m = json.loads((Path(t) / views.MANIFEST).read_text())
            self.assertEqual([v["file"] for v in m["views"]], ["views/a.png", "views/b.png"])
            self.assertEqual(m["views"][0]["frame"], 399)
            self.assertEqual(m["views"][0]["pitch"], 5.0)
            self.assertEqual(m["views"][0]["stance"]["pose"], "standing")
            self.assertEqual(m["views"][0]["stance"]["camera"], "first person")
            self.assertEqual(m["weather"], "0FFC88")

    def test_no_weather_line_when_the_game_s_own_weather_is_kept(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            self.run_views(g, {"views": [views.View("a", 1.0, 2.0, 3.0, 0.0)], "weather": None, "hour": None})
            self.assertFalse(any(line.startswith("fw ") for line in g.lines))
            self.assertEqual(g.lines[-6:], self.view_lines(views.View("a", 1.0, 2.0, 3.0, 0.0), None))
            m = json.loads((Path(t) / views.MANIFEST).read_text())
            self.assertIsNone(m["weather"])
            self.assertIn("not fixed", m["weather_note"])

    def test_extra_console_lines_run_once_after_the_camera_and_before_the_first_view(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            vs = [views.View("a", 1.0, 2.0, 3.0, 0.0), views.View("b", 4.0, 5.0, 6.0, 0.0)]
            self.run_views(g, {"views": vs, "views_console": ["tll all"]})
            self.assertEqual(g.lines.count("tll all"), 1)
            self.assertEqual(g.lines.index("tll all"), g.lines.index("tm") + 1)
            self.assertEqual(json.loads((Path(t) / views.MANIFEST).read_text())["console"], ["tll all"])

    def test_a_missing_picture_tool_fails_the_view_with_no_state_line(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t, grim=False)
            self.run_views(g, {"views": [views.View("a", 1.0, 2.0, 3.0, 0.0)]})
            self.assertFalse(any(n.startswith("state ") for n in g.notes))
            self.assertIn("view-failed a: no picture could be taken", g.notes)
            m = json.loads((Path(t) / views.MANIFEST).read_text())
            self.assertFalse(m["views"][0]["captured"])
            self.assertEqual(m["views"][0]["failed"], "no picture could be taken")

    def test_a_player_that_does_not_stand_gets_no_picture_and_the_next_view_goes_on(self):
        with tempfile.TemporaryDirectory() as t:
            vs = [views.View("a", 1.0, 2.0, 3.0, 0.0), views.View("b", 4.0, 5.0, 6.0, 0.0)]
            g = FakeGame(t, ground=lambda x, y: 500.0 if x < 2 else None)  # at a: the player stands 497 units above the view (a roof, a cliff)
            self.run_views(g, {"views": vs})
            self.assertEqual([Path(s).name for s in g.shots], ["b.png"])
            failed = [n for n in g.notes if n.startswith("view-failed a: ")]
            self.assertEqual(len(failed), 1)
            self.assertIn("from the view's height", failed[0])
            m = json.loads((Path(t) / views.MANIFEST).read_text())
            self.assertEqual([v["captured"] for v in m["views"]], [False, True])
            self.assertIn("view's height", m["views"][0]["failed"])

    def test_a_move_onto_loaded_cells_traces_no_load_and_is_noted_not_failed(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            g.loads = False
            self.run_views(g, {"views": [views.View("a", 1.0, 2.0, 3.0, 0.0)]})
            self.assertEqual([Path(s).name for s in g.shots], ["a.png"])
            self.assertTrue(any(n.startswith("no cell load was traced for the cells of a") for n in g.notes))

    def test_a_player_still_falling_when_time_is_up_fails_the_view_and_a_picture_is_never_taken_anyway(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            g.rows_override = [(i, "play", 1.0, 2.0, 100.0 - i * 0.1, 0.01) for i in range(400)]
            g.until = lambda cond, timeout, every=0.25, what="": cond()  # the time is up after one look
            self.run_views(g, {"views": [views.View("a", 1.0, 2.0, 60.0, 0.0)]})
            self.assertEqual(g.shots, [])
            self.assertTrue(any(n.startswith("view-failed a: the player is moving") for n in g.notes))

    def test_a_player_that_moved_while_the_picture_was_taken_has_no_picture(self):
        with tempfile.TemporaryDirectory() as t:
            g = FakeGame(t)
            real = g.capture

            def capture(path):
                ok = real(path)
                g.rows_override = [(i, "play", g.pos[0], g.pos[1], g.pos[2] - i * 0.01, 0.01) for i in range(400)]
                return ok

            g.capture = capture
            self.run_views(g, {"views": [views.View("a", 1.0, 2.0, 3.0, 0.0)]})
            self.assertFalse((Path(t) / "views" / "a.png").exists())
            self.assertTrue(any(n.startswith("view-failed a: the player did not stay standing") for n in g.notes))

    def test_the_verdict_fails_a_view_without_a_picture_and_says_why(self):
        names = ["a", "b"]
        check = dict(scenarios.views_plan({"views": [views.View("a", 0.0, 0.0, 0.0, 0.0), views.View("b", 0.0, 0.0, 0.0, 0.0)]}).checks)["every view has its picture, taken standing"]
        ok, detail = check({"states": {"view.a": [0, 0, 0]}, "view_failures": {"b": "the player is moving"}})
        self.assertFalse(ok)
        self.assertEqual(detail, "no picture of: b (the player is moving)")
        self.assertEqual(names, ["a", "b"])

    def test_a_ctrl_press_that_reached_the_game_fails_the_run(self):
        check = dict(scenarios.views_plan({"views": [views.View("a", 0.0, 0.0, 0.0, 0.0)]}).checks)["the player was not sneaking (no Ctrl reached the game)"]
        self.assertTrue(check({"ctrl_down": 0})[0])
        self.assertFalse(check({"ctrl_down": 2})[0])

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
            self.assertIn("view-failed b: the game stopped before the picture", g.notes)
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
