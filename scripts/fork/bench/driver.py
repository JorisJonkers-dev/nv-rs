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
    """Rows (frame, screen, x, y, z, fdelta) from the tail of a frame log; the first partial line is dropped."""
    rows = []
    for line in text.splitlines()[1:]:
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

    def settle_play(self, quiet=6.0, timeout=180.0, tap="enter", tap_every=1.5, console_after=4.0):
        """Dismiss message boxes with `tap` until the play screen has held `quiet` seconds without a box. A menu screen
        that stays for `console_after` s may be the console left open (the first box shuts it, a teleport without a box
        does not): the console key is tried once for each stretch of menu."""
        since, last, menu_since, tried = [None], [0.0], [None], [False]

        def ok():
            now = self.clock()
            scr = self.screen()
            if scr == "play":
                since[0] = since[0] or now
                menu_since[0], tried[0] = None, False
                return now - since[0] >= quiet
            since[0] = None
            if scr != "loading":
                menu_since[0] = menu_since[0] or now
                if not tried[0] and now - menu_since[0] >= console_after:
                    tried[0] = True
                    self.tap("grave", 0.15)
                    return False
            else:
                menu_since[0], tried[0] = None, False
            if now - last[0] >= tap_every:
                last[0] = now
                self.tap(tap)
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

    def type(self, text, gap=0.08):
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

    def console_open(self):
        self.tap("grave", 0.15)
        self.wait(0.5)

    def console_line(self, text, settle=1.0):
        """A line into the console that is already open."""
        self.type(text)
        self.tap("enter", 0.15)
        self.wait(settle)
