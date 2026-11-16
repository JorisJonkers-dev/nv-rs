"""A closed-loop, wall-clock driver for the nested window: real keyboard and pointer events (vinput.py) timed in
seconds and in the game's own state (the screen column of the frame log), never in frames, so a scenario is valid at any
frame rate: unlimited play, the menu limit, a slow machine.

A scenario is a function `drive(d)` of a Driver. `d.until_screen("play", 60)` waits for the game, `d.tap("enter")`,
`d.hold(("w",), 8)`, `d.console("player.moveto 1658B7")` act. The driver is fed by the frame log the viewer writes
(NV_FRAME_LOG), read from its tail.
"""
import os
import time

SCREEN_COL, PX, PY, PZ, FDELTA = 8, 5, 6, 7, 2


def parse_tail(text):
    """Rows (frame, screen, x, y, z, fdelta) from the tail of a frame log. The first line is dropped (the tail starts
    inside it), and so is a last line the viewer has not finished writing: cut after a column, its screen would read
    as an empty string, not "play", and a closed loop waiting for the play screen would tap keys into the game."""
    lines = text.split("\n")[1:]
    if lines:
        lines.pop()  # the text after the last newline: empty, or a line still being written
    rows = []
    for line in lines:
        p = line.split("\t")
        if len(p) > SCREEN_COL and p[0].isdigit():
            try:
                rows.append((int(p[0]), p[SCREEN_COL].strip().lower(), float(p[PX]), float(p[PY]), float(p[PZ]), float(p[FDELTA])))
            except ValueError:
                pass
    return rows


class Driver:
    def __init__(self, vi, frames, proc, log=None, scale=1.0, clock=time.monotonic, sleep=time.sleep):
        self.vi, self.frames, self.proc, self.log, self.scale = vi, frames, proc, log, scale
        self.clock, self.sleep = clock, sleep
        self.t0 = clock()

    # state ---------------------------------------------------------------------------------------
    def alive(self):
        return self.proc.poll() is None

    def tail(self, nbytes=4096):
        try:
            with open(self.frames, "rb") as f:
                f.seek(0, os.SEEK_END)
                f.seek(max(0, f.tell() - nbytes))
                return parse_tail(f.read().decode("utf-8", "replace"))
        except OSError:
            return []

    def screen(self):
        rows = self.tail(2048)
        return rows[-1][1] if rows else ""

    def position(self):
        rows = self.tail(2048)
        return rows[-1][2:5] if rows else None

    def note(self, text):
        if self.log:
            self.log.write(f"# driver: {text}\n")
            self.log.flush()

    # waiting -------------------------------------------------------------------------------------
    def wait(self, seconds):
        """Wall-clock seconds (scaled by --scale only where the caller says so)."""
        end = self.clock() + seconds
        while self.clock() < end and self.alive():
            self.sleep(min(0.25, max(0.0, end - self.clock())))

    def until(self, cond, timeout, every=0.25, what=""):
        """Wait until cond() is true; False on timeout or when the game is gone."""
        end = self.clock() + timeout
        while self.alive():
            if cond():
                return True
            if self.clock() >= end:
                self.note(f"timed out waiting for {what or 'a condition'} after {timeout:.0f} s")
                return False
            self.sleep(every)
        return False

    def until_screen(self, name, timeout=120.0, stable=0.0):
        """Wait for the screen to be `name`, and to have stayed so for `stable` seconds."""
        since = [None]

        def ok():
            if self.screen() == name:
                since[0] = since[0] or self.clock()
                return self.clock() - since[0] >= stable
            since[0] = None
            return False

        return self.until(ok, timeout, what=f"the {name} screen")

    def until_not_screen(self, name, timeout=120.0, tap=None, tap_every=1.5):
        """Wait for the screen to leave `name`, tapping a key now and then (a movie skipped, a box dismissed)."""
        last = [0.0]

        def ok():
            if self.screen() != name:
                return True
            if tap and self.clock() - last[0] >= tap_every:
                last[0] = self.clock()
                self.tap(tap)
            return False

        return self.until(ok, timeout, every=0.1, what=f"the end of the {name} screen")

    def settle_play(self, quiet=6.0, timeout=180.0, tap=("enter", "down", "enter"), tap_every=0.75, console_after=6.0):
        """Dismiss message boxes with the keys of `tap` in turn until the play screen has held `quiet` seconds without a
        box. A box's button takes Enter only once it has the keyboard focus, which an arrow key gives it (a box that came
        while the mouse moved has none), hence a Down between the Enters. Not for the start-up: in the pause menu Down
        moves to Save (pass tap="enter" there). A box that came while the console was open never gets the keyboard
        (the console has it): when a stretch of menu outlasts `console_after` s the console key is tapped, and again
        every `console_after` s while the menu stays (a tap over a box with no console under it opens one, which the
        next tap shuts). None: never."""
        keys = (tap,) if isinstance(tap, str) else tuple(tap)
        since, last, menu_since, tried, turn = [None], [0.0], [None], [None], [0]

        def ok():
            now = self.clock()
            scr = self.screen()
            if scr == "play":
                since[0] = since[0] or now
                menu_since[0], tried[0] = None, None
                return now - since[0] >= quiet
            since[0] = None
            if scr != "loading":
                menu_since[0] = menu_since[0] or now
                if console_after is not None and now - (tried[0] or menu_since[0]) >= console_after:
                    tried[0] = now
                    self.tap("grave", 0.15)
                    return False
            else:
                menu_since[0], tried[0] = None, None
            if now - last[0] >= tap_every:
                last[0] = now
                self.tap(keys[turn[0] % len(keys)], 0.1)
                turn[0] += 1
            return False

        return self.until(ok, timeout, every=0.1, what="the play screen to settle")

    # input ---------------------------------------------------------------------------------------
    def tap(self, key, hold=0.25):
        if self.alive():
            self.vi.tap(key, hold)

    def hold(self, keys, seconds):
        if self.alive():
            self.vi.hold((keys,) if isinstance(keys, str) else tuple(keys), seconds)

    def look(self, dx, dy, seconds, step=0.01):
        """Relative pointer motion (dx, dy) every `step` s for `seconds`."""
        end = self.clock() + seconds
        while self.clock() < end and self.alive():
            self.vi.move(dx, dy)
            self.sleep(step)

    def click(self, seconds=0.12):
        if self.alive():
            self.vi.click("left", seconds)

    def walk(self, seconds, turn=0.0, sprint=False):
        """Hold W for `seconds`, turning the view by `turn` counts per 10 ms in the first second."""
        keys = ("w", "shift") if sprint else ("w",)
        import threading
        t = threading.Thread(target=self.hold, args=(keys, seconds), daemon=True)
        t.start()
        if turn:
            self.look(turn, 0, 1.0)
        t.join()

    def type(self, text, gap=0.025):
        for ch in text:
            if ch == " ":
                self.vi.tap("space", 0.04)
            elif ch.isupper():
                self.vi.hold(("shift", ch.lower()), 0.05)
            elif ch == "_":
                self.vi.hold(("shift", "minus"), 0.05)
            elif ch == ".":
                self.vi.tap("period", 0.04)
            elif ch == "-":
                self.vi.tap("minus", 0.04)
            else:
                self.vi.tap(ch, 0.04)
            self.sleep(gap)

    def ensure_play(self, timeout=20.0):
        """Back to the play screen: boxes dismissed with Enter, a console left open shut with its key (closed loop)."""
        return self.settle_play(quiet=1.0, timeout=timeout)

    def console_open(self, tries=3):
        """Open the console from the play screen and see it open (the screen turns to menu). A box in the way is
        dismissed first; a box shuts the console by itself, so the console is never assumed open."""
        for _ in range(tries):
            if self.screen() != "play":
                self.ensure_play()
            self.tap("grave", 0.15)
            if self.until_screen("menu", 2.0):
                self.wait(0.3)
                return True
        self.note("the console did not open")
        return False

    def console_close(self, quiet=2.0, timeout=30.0):
        """Shut the console and wait for the play screen to hold `quiet` seconds, dismissing a box that comes with what
        the console did (the downloadable packs' boxes come after the opening and with the first exterior)."""
        if self.screen() == "menu":
            self.tap("grave", 0.15)
            self.until_screen("play", 2.0)
        return self.settle_play(quiet=quiet, timeout=timeout)

    def console_line(self, text, settle=1.0):
        """A line into the console; if a box shut it since it was opened, it is opened again first."""
        if self.screen() == "play" and not self.console_open():
            return False
        self.type(text)
        self.tap("enter", 0.15)
        self.wait(settle)
        return True
