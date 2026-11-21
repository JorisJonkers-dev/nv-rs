# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the bench driver's menu walk (scripts/fork/bench/menu_view.py and drivers.new_game_from_menu,
answer_start_choice): pictures drawn here with the outline geometry measured on real menus (numbers only, no game
picture), and a small model of the menus' key behaviour as seen on a Profile with and without a save (nv-rs#265).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
"""
import os
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent / "bench"
sys.path.insert(0, str(BENCH))

import drivers  # noqa: E402
import menu_view  # noqa: E402

W, H = 1920, 1080
OUT = bytes(menu_view.OUTLINE)


def picture(rects, w=W, h=H, frames=(), cursor=None):
    """A picture with a dark background and a two pixel outline for each (x0, y0, x1, y1) in `rects` (and a brighter one
    for each of `frames`); `cursor` (x, y) paints a 14 pixel square over whatever lies there, as the mouse cursor does."""
    px = bytearray(bytes((20, 8, 4)) * (w * h))
    for color, group in ((OUT, rects), (bytes(menu_view.FRAME), frames)):
        for x0, y0, x1, y1 in group:
            for y in (y0, y0 + 1, y1 - 2, y1 - 1):
                px[(y * w + x0) * 3 : (y * w + x1) * 3] = color * (x1 - x0)
            for y in range(y0, y1):
                for x in (x0, x0 + 1, x1 - 2, x1 - 1):
                    px[(y * w + x) * 3 : (y * w + x) * 3 + 3] = color
    if cursor:
        cx, cy = cursor
        for y in range(cy, cy + 14):
            px[(y * w + cx) * 3 : (y * w + cx + 14) * 3] = bytes((120, 120, 120)) * 14
    return w, h, bytes(px)


def menu_rect(line, w=W, h=H):
    """The outline of main menu line `line` as measured at 1080 lines: 444 by 53 at the right edge, 54 lines apart."""
    s = h / 1080.0
    y0 = round((351 + 54 * line) * s)
    return round(1437 * w / 1920), y0, round(1881 * w / 1920), y0 + round(53 * s)


def box_rect(first_y, button, w=W, h=H):
    """A box button's outline (84 by 53) at x 1329, the buttons 54 lines apart; `first_y` the first one's top."""
    s = h / 1080.0
    y0 = round((first_y + 54 * button) * s)
    return round(1329 * w / 1920), y0, round(1413 * w / 1920), y0 + round(53 * s)


class Menus:
    """The key behaviour seen on the real menus: the first arrow key only wakes the keyboard (an awake menu shows the
    outline on Continue), Enter on Continue opens "Continue from your last saved game?" (Yes selected), Enter on New opens
    "Start a new game?" (Yes selected), Down in a box moves to No, Escape closes a box, Yes on the new game box starts it."""

    def __init__(self, awake=False, popup=None):
        self.awake, self.line, self.popup, self.pick = awake, 0, popup, 0
        self.begun, self.loaded_save, self.keys = False, False, []

    def key(self, name):
        self.keys.append(name)
        if not self.awake and name in ("down", "up"):
            self.awake = True
        elif self.popup:
            if name == "down":
                self.pick = 1
            elif name == "up":
                self.pick = 0
            elif name == "esc":
                self.popup = None
            elif name == "enter":
                if self.pick == 0 and self.popup == "new":
                    self.begun = True
                elif self.pick == 0:
                    self.loaded_save = True
                self.popup = None
        elif self.awake:
            if name == "down":
                self.line = min(self.line + 1, 6)
            elif name == "up":
                self.line = max(self.line - 1, 0)
            elif name == "enter" and self.line in (0, 1):
                self.popup, self.pick = ("continue", "new")[self.line], 0

    def picture(self):
        if self.popup:
            return picture([box_rect(284 if self.popup == "continue" else 338, self.pick)])
        return picture([menu_rect(self.line)] if self.awake else [])


CHOICE_FRAME = (585, 452, 1336, 631)  # the start choice box's frame as measured: two lines inside, 58 apart
CHOICE_LINES = ((655, 490, 1273, 547), (655, 548, 1273, 605))  # the selection's outline on the first and the second line


class Choice:
    """The start choice box: two lines, no outline until a first key wakes it (which of the lines it then shows is
    `wakes_on`), Down and Up wrapping or stopping at the ends."""

    def __init__(self, awake=False, line=0, wakes_on=0, wrap=True, cursor=None):
        self.awake, self.line, self.wakes_on, self.wrap, self.cursor = awake, line, wakes_on, wrap, cursor
        self.keys, self.entered = [], None

    def key(self, name):
        self.keys.append(name)
        if not self.awake and name in ("down", "up"):
            self.awake, self.line = True, self.wakes_on
        elif name in ("down", "up"):
            step = 1 if name == "down" else -1
            self.line = (self.line + step) % 2 if self.wrap else min(1, max(0, self.line + step))
        elif name == "enter":
            self.entered = self.line

    def picture(self):
        return picture([CHOICE_LINES[self.line]] if self.awake else [], frames=[CHOICE_FRAME], cursor=self.cursor)


class Pilot:
    """The parts of driver.Driver the walks use, over a model."""

    def __init__(self, model):
        self.m, self.t, self.notes = model, 0.0, []

    def tap(self, key, hold=0.25):
        self.m.key(key)

    def wait(self, s):
        self.t += s

    def until(self, cond, timeout, every=0.25, what=""):
        return bool(cond())

    def note(self, text):
        self.notes.append(text)

    def snapshot(self):
        return self.m.picture()

    def menu_stack(self):
        return (0x3EF, 0x3F5)

    def count_events(self, kind, needle=""):
        return 1 if getattr(self.m, "begun", False) and kind == "load.kind" else 0


class MenuView(unittest.TestCase):
    def test_a_menu_line_outline_is_found_and_numbered(self):
        for line in range(7):
            self.assertEqual(menu_view.main_menu_line(picture([menu_rect(line)])), line)
        self.assertEqual(menu_view.outlines(picture([menu_rect(1)])), [(1437, 405, 1881, 458)])

    def test_no_outline_means_no_line_and_no_button(self):
        pic = picture([])
        self.assertIsNone(menu_view.main_menu_line(pic))
        self.assertEqual(menu_view.box_buttons(pic), [])

    def test_a_box_button_is_not_a_menu_line(self):
        pic = picture([box_rect(284, 0)])
        self.assertIsNone(menu_view.main_menu_line(pic))
        self.assertEqual(menu_view.box_buttons(pic), [(1329, 284, 1413, 337)])

    def test_the_geometry_scales_with_the_screen(self):
        w, h = 1280, 720
        pic = picture([menu_rect(1, w, h)], w, h)
        self.assertEqual(menu_view.main_menu_line(pic), 1)
        self.assertEqual(menu_view.box_buttons(picture([box_rect(284, 1, w, h)], w, h))[0][1], round((284 + 54) * 720 / 1080))

    def test_a_ppm_is_read_and_a_short_one_is_refused(self):
        px = bytes(range(256)) * 36  # 64 x 48 pixels
        self.assertEqual(menu_view.parse_ppm(b"P6\n64 48\n255\n" + px), (64, 48, px))
        self.assertIsNone(menu_view.parse_ppm(b"P6\n64 48\n255\n" + px[:-3]))
        self.assertIsNone(menu_view.parse_ppm(b"P5\n64 48\n255\n" + px))


class NewGameWalk(unittest.TestCase):
    def walk(self, menus, **kw):
        pilot = Pilot(menus)
        return drivers.new_game_from_menu(pilot, **kw), pilot

    def test_a_menu_with_nothing_selected_is_woken_and_walked_to_new(self):
        # The Profile without a save: the first Down only wakes the menu, which a fixed Down, Enter, Enter mistook for Continue.
        menus = Menus(awake=False)
        ok, _ = self.walk(menus)
        self.assertTrue(ok)
        self.assertEqual(menus.keys, ["down", "down", "enter", "enter"])
        self.assertFalse(menus.loaded_save)

    def test_a_menu_that_opens_on_continue_takes_one_down(self):
        menus = Menus(awake=True)
        ok, _ = self.walk(menus)
        self.assertTrue(ok)
        self.assertEqual(menus.keys, ["down", "enter", "enter"])

    def test_a_box_it_did_not_open_is_closed_with_escape_not_answered(self):
        menus = Menus(awake=True, popup="continue")
        ok, pilot = self.walk(menus)
        self.assertTrue(ok)
        self.assertEqual(menus.keys, ["esc", "down", "enter", "enter"])
        self.assertFalse(menus.loaded_save)
        self.assertTrue(any("did not open" in n for n in pilot.notes))

    def test_a_selection_below_new_goes_up(self):
        menus = Menus(awake=True)
        menus.line = 3
        ok, _ = self.walk(menus)
        self.assertTrue(ok)
        self.assertEqual(menus.keys, ["up", "up", "enter", "enter"])

    def test_it_gives_up_after_its_rounds_when_nothing_starts(self):
        class Stuck(Menus):
            def key(self, name):
                self.keys.append(name)

        menus = Stuck(awake=True)
        ok, _ = self.walk(menus, tries=4)
        self.assertFalse(ok)
        self.assertEqual(len(menus.keys), 4)


class StartChoice(unittest.TestCase):
    def pick(self, choice, want="mojave"):
        pilot = Pilot(choice)
        return drivers.answer_start_choice(pilot, want), pilot

    def test_mojave_is_proven_from_the_outline_for_every_way_the_box_behaves(self):
        for awake in (False, True):
            for line in (0, 1):
                for wakes_on in (0, 1):
                    for wrap in (True, False):
                        choice = Choice(awake=awake, line=line, wakes_on=wakes_on, wrap=wrap)
                        ok, _ = self.pick(choice)
                        self.assertTrue(ok, (awake, line, wakes_on, wrap))
                        self.assertEqual(choice.entered, 1, (awake, line, wakes_on, wrap, choice.keys))

    def test_capital_is_the_first_line_every_way_too(self):
        for awake in (False, True):
            for line in (0, 1):
                for wakes_on in (0, 1):
                    for wrap in (True, False):
                        choice = Choice(awake=awake, line=line, wakes_on=wakes_on, wrap=wrap)
                        ok, _ = self.pick(choice, "capital")
                        self.assertTrue(ok)
                        self.assertEqual(choice.entered, 0, (awake, line, wakes_on, wrap, choice.keys))

    def test_no_enter_goes_in_before_the_line_is_read(self):
        choice = Choice(awake=False, wakes_on=0)
        self.pick(choice)
        self.assertEqual(choice.keys, ["down", "down", "enter"])
        choice = Choice(awake=True, line=1)
        self.pick(choice)
        self.assertEqual(choice.keys, ["enter"])

    def test_the_mouse_cursor_over_an_edge_does_not_hide_the_outline(self):
        choice = Choice(awake=True, line=1, cursor=(960, 548))  # the arrow sits on the second line's top edge
        ok, _ = self.pick(choice)
        self.assertTrue(ok)
        self.assertEqual(choice.entered, 1)

    def test_it_does_not_press_enter_on_a_line_it_cannot_read(self):
        class Dead(Choice):
            def key(self, name):
                self.keys.append(name)
                if name == "enter":
                    self.entered = self.line

        choice = Dead(awake=False)
        ok, _ = self.pick(choice)
        self.assertFalse(ok)
        self.assertNotIn("enter", choice.keys)


class Opening:
    """start_new_game over a timeline of what the game traces (times in seconds since the new game began): the main menu
    is left at 1 s, the player is placed at 2 s, a box over nothing (the Tale of Two Wastelands start choice) is up from
    `box_at` to the answer, the film plays from `film_at` to `film_end`."""

    def __init__(self, game_dir, box_at=4.0, film_at=6.0, film_end=15.0, films=("FNVIntro",)):
        self.game_dir, self.box_at, self.film_at, self.film_end, self.films = game_dir, box_at, film_at, film_end, films
        self.t, self.keys, self.notes, self.answered_at, self.vi = 0.0, [], [], None, None
        self.settled, self.base = False, 0.0

    @property
    def rel(self):
        return self.t - self.base

    def clock(self):
        return self.t

    def wait(self, s):
        self.t += s

    def until(self, cond, timeout, every=0.25, what=""):
        end = self.t + timeout
        while self.t < end:
            if cond():
                return True
            self.t += every
        return False

    def tap(self, key, hold=0.25):
        self.keys.append((self.t, key))

    def note(self, text):
        self.notes.append(text)

    def tail(self, nbytes=4096):
        return [(1, "play", 0.0, 0.0, 0.0, 1.0)]

    def screen(self):
        return "play" if self.rel >= 1.0 else "menu"

    def position(self):
        return (10.0, 20.0, 5.0) if self.rel >= 2.0 else (2048.0, 2048.0, 128.0)

    def menu_stack(self):
        if self.answered_at is None and self.rel >= self.box_at:
            return (0x3E9,)
        return ()

    def film_playing(self):
        return self.film_at <= self.rel < self.film_end and self.answered_at is not None

    def count_events(self, kind, needle=""):
        if kind == "movie.open" and self.answered_at is not None and self.rel >= self.film_at:
            return sum(1 for f in self.films if needle in f)
        return 0

    def snapshot(self):
        return (1, 1, b"\0\0\0")

    def settle_play(self, **kw):
        self.settled = True
        return True


class StartNewGame(unittest.TestCase):
    def run_opening(self, ttw, want_env=None, film="FNVIntro", pick_ok=True):
        with tempfile.TemporaryDirectory() as tmp:
            os.makedirs(os.path.join(tmp, "Data"))
            if ttw:
                open(os.path.join(tmp, "Data", "TaleOfTwoWastelands.esm"), "wb").close()
            d = Opening(tmp, films=(film,))
            seen = {}
            real = (drivers.new_game_from_menu, drivers.answer_start_choice)

            def answer(dd, want, tries=8):
                seen["want"], seen["at"] = want, dd.t
                dd.answered_at = dd.t
                return pick_ok

            drivers.new_game_from_menu = lambda dd, step=1.5: (setattr(dd, "base", dd.t), True)[1]
            drivers.answer_start_choice = answer
            old = os.environ.get("NV_BENCH_TTW_START")
            os.environ.pop("NV_BENCH_TTW_START", None)
            if want_env:
                os.environ["NV_BENCH_TTW_START"] = want_env
            try:
                ok = drivers.start_new_game(d)
            finally:
                drivers.new_game_from_menu, drivers.answer_start_choice = real
                os.environ.pop("NV_BENCH_TTW_START", None)
                if old is not None:
                    os.environ["NV_BENCH_TTW_START"] = old
            return ok, d, seen

    def test_a_ttw_game_folder_gets_its_start_choice_answered_before_the_opening_is_settled(self):
        ok, d, seen = self.run_opening(ttw=True)
        self.assertTrue(ok)
        self.assertEqual(seen["want"], "mojave")
        self.assertTrue(d.settled)
        # the player is placed at 2 s but the walk waits for the box (4 s) and does not press Enter itself
        self.assertGreaterEqual(seen["at"] - d.base, 4.0)
        self.assertNotIn("enter", [k for _, k in d.keys])

    def test_no_escape_is_tapped_while_the_box_is_up(self):
        ok, d, seen = self.run_opening(ttw=True)
        box = [t for t, k in d.keys if k == "esc" and d.base + d.box_at <= t < seen["at"]]
        self.assertEqual(box, [])

    def test_the_film_after_the_choice_is_skipped_before_the_walk_goes_on(self):
        ok, d, _ = self.run_opening(ttw=True)
        self.assertTrue(ok)
        escapes = [t for t, k in d.keys if k == "esc" and t >= d.base + 6.0]
        self.assertTrue(escapes, "the film is skipped with Escape")

    def test_a_game_folder_without_ttw_never_answers_a_choice(self):
        ok, d, seen = self.run_opening(ttw=False)
        self.assertTrue(ok)
        self.assertEqual(seen, {})

    def test_the_capital_start_is_not_taken_for_the_mojave_one(self):
        ok, d, _ = self.run_opening(ttw=True, film="Fallout INTRO Vsk")
        self.assertFalse(ok)
        self.assertTrue(any("Capital Wasteland start was taken" in n for n in d.notes))

    def test_a_choice_that_cannot_be_proven_stops_the_walk(self):
        ok, d, _ = self.run_opening(ttw=True, pick_ok=False)
        self.assertFalse(ok)
        self.assertFalse(d.settled)

    def test_the_env_picks_the_capital_line_and_off_turns_it_away(self):
        ok, d, seen = self.run_opening(ttw=True, want_env="capital", film="Fallout INTRO Vsk")
        self.assertTrue(ok)
        self.assertEqual(seen["want"], "capital")
        ok, d, seen = self.run_opening(ttw=True, want_env="off")
        self.assertEqual(seen, {})

    def test_no_box_within_the_wait_goes_on_without_answering(self):
        with tempfile.TemporaryDirectory() as tmp:
            os.makedirs(os.path.join(tmp, "Data"))
            open(os.path.join(tmp, "Data", "taleoftwowastelands.esm"), "wb").close()
            d = Opening(tmp, box_at=1e9)
            real = drivers.new_game_from_menu
            drivers.new_game_from_menu = lambda dd, step=1.5: (setattr(dd, "base", dd.t), True)[1]
            try:
                os.environ.pop("NV_BENCH_TTW_START", None)
                ok = drivers.start_new_game(d)
            finally:
                drivers.new_game_from_menu = real
            self.assertTrue(ok)
            self.assertTrue(any("no start choice box came" in n for n in d.notes))


if __name__ == "__main__":
    unittest.main()
