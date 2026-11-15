"""Scenarios: scripted input as data. The game takes keys and mouse by main-loop frame (NV_MOVE_KEYS,
NV_MOVE_MOUSE; crates/host/src/player_move.rs), so a scenario is a list of timed presses; times are in
seconds and converted to frames at the scenario's reference frame rate (the frame limit in capped mode).
"""
from dataclasses import dataclass, field

# DirectInput key codes
DIK = {
    "esc": 0x01, "enter": 0x1C, "space": 0x39, "tab": 0x0F, "backspace": 0x0E, "lshift": 0x2A, "lctrl": 0x1D,
    "up": 0xC8, "down": 0xD0, "left": 0xCB, "right": 0xCD, "grave": 0x29,
    "w": 0x11, "a": 0x1E, "s": 0x1F, "d": 0x20, "e": 0x12, "r": 0x13, "q": 0x10,
    "f5": 0x3F, "f9": 0x43, "minus": 0x0C, "period": 0x34, "comma": 0x33, "slash": 0x35,
}
_ROWS = {"qwertyuiop": 0x10, "asdfghjkl": 0x1E, "zxcvbnm": 0x2C}
for _row, _base in _ROWS.items():
    for _i, _c in enumerate(_row):
        DIK[_c] = _base + _i
for _i, _c in enumerate("1234567890"):
    DIK[_c] = 0x02 + _i


_PUNCT = {".": "period", ",": "comma", "-": "minus", "/": "slash"}


@dataclass
class Plan:
    name: str
    description: str
    keys: list = field(default_factory=list)  # (frame, dik, hold frames)
    mouse: list = field(default_factory=list)  # (frame, dx, dy, hold frames, button mask)
    end_frame: int = 0  # stop at this main frame (0: not by frames)
    wall_after_first_s: float = 0.0  # or this long after the first frame
    checks: list = field(default_factory=list)  # (name, fn(metrics) -> (ok, detail))
    fps: float = 60.0
    capped_only: bool = False  # frame-count timed: only valid with the frame limit (the game's UI runs on time)
    extra_env: dict = field(default_factory=dict)

    def env(self, opts):
        e = {
            "NV_D3D9": "wgpu",
            "NV_PRESET": opts["preset"],
            "NV_SOUND": opts.get("sound") or "silent",
            "NV_RESOLUTION": opts["resolution"],
        }
        if opts["mode"] == "uncapped":
            e.update(NV_FRAME_LIMIT="0", NV_MENU_FRAME_LIMIT="0", NV_PRESENT="immediate", NV_D3D9_PRESENT="immediate")
        else:
            e.update(NV_FRAME_LIMIT="144", NV_MENU_FRAME_LIMIT="144", NV_PRESENT="mailbox")
        if opts["windowed"]:
            e["NV_D3D9_BACKGROUND"] = "1"
        else:
            e["NV_D3D9_OFFSCREEN"] = "1"
            if e["NV_SOUND"] == "silent":
                # the game's sound clock runs (voice lines end, so dialogue goes on) and the speakers get silence
                e["NV_SOUND"], e["NV_D3D9_HIDDEN"] = "out", "1"
        if opts.get("shots"):
            e["NV_SHOT_WGPU"] = opts["shots"]
        if opts.get("threads"):
            e["NV_THREADS"] = opts["threads"]
        e.update(self.extra_env)
        if self.keys:
            e["NV_MOVE_KEYS"] = ",".join(f"{f}:{d:X}:{h}" for f, d, h in sorted(self.keys))
        if self.mouse:
            e["NV_MOVE_MOUSE"] = ",".join(f"{f}:{x}:{y}:{h}:{b}" for f, x, y, h, b in sorted(self.mouse))
        return e


class Script:
    """A timeline builder. `t` is the cursor in seconds; `fps` converts to frames."""

    def __init__(self, fps=60.0, t=0.0):
        self.fps, self.t = fps, t
        self.keys, self.mouse = [], []

    def frame(self, t=None):
        return int(round((self.t if t is None else t) * self.fps))

    def wait(self, s):
        self.t += s
        return self

    def press(self, key, hold=0.1, then=0.3):
        """Tap a key (hold seconds), then wait `then` seconds."""
        self.keys.append((self.frame(), DIK[key], max(1, self.frame(self.t + hold) - self.frame())))
        self.t += max(hold, then)
        return self

    def hold(self, keys, seconds):
        """Hold one key or several (a tuple) together for `seconds`; the cursor moves past them."""
        for k in (keys,) if isinstance(keys, str) else keys:
            self.keys.append((self.frame(), DIK[k], max(1, self.frame(self.t + seconds) - self.frame())))
        self.t += seconds
        return self

    def type(self, text, gap=0.12):
        """Type text with shift for capitals and the few symbols the console needs, then no Enter."""
        for ch in text:
            if ch == " ":
                self.press("space", 0.05, gap)
            elif ch.isupper():
                self.keys.append((self.frame(), DIK["lshift"], max(1, self.frame(self.t + 0.1) - self.frame() + 1)))
                self.press(ch.lower(), 0.05, gap)
            elif ch in _PUNCT:
                self.press(_PUNCT[ch], 0.05, gap)
            elif ch == "_":
                self.keys.append((self.frame(), DIK["lshift"], max(1, self.frame(self.t + 0.1) - self.frame() + 1)))
                self.press("minus", 0.05, gap)
            elif ch in DIK:
                self.press(ch, 0.05, gap)
            else:
                raise ValueError(f"no key for {ch!r}")
        return self

    def console(self, command, settle=1.0):
        """Open the console, type a command, run it, close the console."""
        self.press("grave", 0.1, 0.5)
        self.type(command)
        self.press("enter", 0.1, settle)
        self.press("grave", 0.1, 0.4)
        return self

    def look(self, dx, dy, seconds):
        """Mouse motion of (dx, dy) counts on every frame for `seconds`."""
        self.mouse.append((self.frame(), dx, dy, max(1, self.frame(self.t + seconds) - self.frame()), 0))
        self.t += seconds
        return self

    def click(self, hold=0.1, then=0.5):
        self.mouse.append((self.frame(), 0, 0, max(1, self.frame(self.t + hold) - self.frame()), 1))
        self.t += max(hold, then)
        return self


# ---------------------------------------------------------------------------------------------
# checks: fn(metrics) -> (ok, detail)


def reached_screen(name):
    def check(m):
        seen = [s for _, _, s in m["screens"]]
        return name in seen, f"screens seen: {', '.join(dict.fromkeys(seen)) or 'none'}"

    return (f"reached the {name} screen", check)


def moved_at_least(units):
    def check(m):
        p = m["movement"]["path"]
        return p >= units, f"player path {p:.0f} units (want {units})"

    return (f"player moved {units}+ units", check)


def frames_at_least(n):
    def check(m):
        return m["frames"] >= n, f"{m['frames']} frames (want {n})"

    return (f"{n}+ frames drawn", check)


# ---------------------------------------------------------------------------------------------
# the scenarios


def play_seconds_at_least(n):
    def check(m):
        play = m["frame_time_play"]
        secs = play.get("frames", 0) * (play.get("avg_ms") or 0) / 1000.0
        return secs >= n, f"{secs:.0f} s on the play screen (want {n})"

    return (f"{n}+ s of play", check)


def player_placed():
    return ("the player was placed in a cell", lambda m: (m["player_placed"], "position " + ("set" if m["player_placed"] else "never left the start-up value")))


# The opening, by main-frame number (loading-screen frames run at about 1 ms, so these are frames, not seconds;
# verified at the 144 frames a second capped mode). Each key is held 30 frames: shorter taps are dropped by the
# menus. The first arrow press only wakes the menu's keyboard mode.
OPENING = [
    *[(f, "esc", 6) for f in (300, 600, 900, 1200)],  # skip the start-up movies
    (2000, "down", 30),  # wakes the keyboard mode
    (2300, "down", 30),  # Continue -> New
    (2600, "enter", 30),  # New; "Start a new game?"
    (2900, "enter", 30),  # Yes; the intro movie starts (loading screen)
    (6000, "esc", 30),  # skip the intro movie; the new game's first frame
    *[(6500 + 400 * k, "enter", 30) for k in range(6)],  # the downloadable packs' message boxes
]
OPENING_END = 7800  # first frames in the first room; Doc Mitchell's house


def opening(plan):
    for f, key, hold in OPENING:
        plan.keys.append((f, DIK[key], hold))


def boot(opts):
    p = Plan("boot", "Start the game, skip the start-up movies and sit on the main menu", wall_after_first_s=50.0, fps=opts["fps"])
    for f, key, hold in OPENING[:4]:
        p.keys.append((f, DIK[key], hold))
    p.checks = [frames_at_least(1000), reached_screen("menu")]
    return p


def newgame(opts):
    p = Plan(
        "newgame",
        "Main menu, New, through the intro and the pack messages to the first room (Doc Mitchell's house), then 20 s there",
        fps=opts["fps"],
        capped_only=True,
    )
    opening(p)
    p.end_frame = OPENING_END + int(20 * opts["fps"])
    p.checks = [reached_screen("loading"), reached_screen("play"), play_seconds_at_least(10), player_placed()]
    return p


def build(name, opts):
    if name not in SCENARIOS:
        raise SystemExit(f"unknown scenario {name!r}; known: {', '.join(sorted(SCENARIOS))}")
    return SCENARIOS[name](opts)


SCENARIOS = {"boot": boot, "newgame": newgame}
