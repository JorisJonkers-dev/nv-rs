"""Scenarios: scripted input as data. The game takes keys and mouse by main-loop frame (NV_MOVE_KEYS,
NV_MOVE_MOUSE; crates/host/src/player_move.rs), so a scenario is a list of timed presses; times are in
seconds and converted to frames at the scenario's reference frame rate (the frame limit in capped mode).
"""
from dataclasses import dataclass, field

from drivers import DRIVERS

# DirectInput key codes
MENU_LIMIT = "120"

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


def _order(item):
    """Sort key of a script item: a frame is a number (main-loop frames) or "p" and a number (frames that are not a
    loading screen, `NV_MOVE_KEYS` in crates/host/src/player_move.rs); the two clocks are not comparable, so the
    host takes them as they come."""
    f = item[0]
    return (isinstance(f, str), int(str(f).lstrip("p")))


def on_play_clock(items):
    """Items of a Script built in play frames, as the game takes them: frame numbers with the "p" prefix."""
    return [(f"p{it[0]}",) + tuple(it[1:]) for it in items]


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
    real: list = field(default_factory=list)  # real input steps (inject.py), sent in the nested window after the teleport
    real_after_x: float = -60000.0  # the teleport is done once the player's x is below this (Goodsprings is at -67856)
    real_settle_frames: int = 1200  # and the play screen has held this many frames
    budget_s: float = 0.0  # a wall-time budget for the whole run; over it the verdict is TOO_LONG
    drive: object = None  # a drivers.py function: the scenario as a closed-loop wall-clock program (driver.py)
    self_scaled: bool = False  # the scenario applied --scale itself (by its laps)

    def env(self, opts):
        e = {
            "NV_D3D9": "wgpu",
            "NV_PRESET": opts["preset"],
            "NV_SOUND": opts.get("sound") or "silent",
            "NV_RESOLUTION": opts["resolution"],
        }
        # The menu limit stays: the game's menus run on time and drop keys when they run at thousands of frames a second.
        if opts["mode"] in ("peak", "uncapped"):
            e.update(NV_FRAME_LIMIT="0", NV_MENU_FRAME_LIMIT=MENU_LIMIT, NV_PRESENT="immediate", NV_D3D9_PRESENT="immediate")
        elif opts["mode"] == "capped":
            cap = str(int(opts.get("cap") or 144))
            e.update(NV_FRAME_LIMIT=cap, NV_FPS_LIMIT=cap, NV_MENU_FRAME_LIMIT=cap, NV_PRESENT="mailbox")
        else:  # unlimited, as real play is
            e.update(NV_FRAME_LIMIT="0", NV_FPS_LIMIT="0", NV_MENU_FRAME_LIMIT=MENU_LIMIT, NV_PRESENT="mailbox")
        if opts["windowed"]:
            e["NV_D3D9_BACKGROUND"] = "1"
        else:
            e["NV_D3D9_OFFSCREEN"] = "1"
        if e["NV_SOUND"] == "silent":
            # The mixer runs on the game's clock into the null sink: voice lines end, so Doc's dialogue goes on and the
            # speakers get nothing. (NV_D3D9_HIDDEN=1 muted the device instead and the opening stalled at Doc's
            # second line: found 2026-11-16, docs/fork/BENCH.md.)
            e["NV_SOUND"], e["NV_SOUND_BACKEND"] = "out", "null"
        if opts.get("perf"):
            e["NV_PERF"] = "1"
        if opts.get("shots"):
            e["NV_SHOT_WGPU"] = opts["shots"]
        # Real threads are the default: stepped ran about half the frame rate (70 against 135-143 fps) and is not how the
        # game is played; --threads stepped asks for it.
        e["NV_THREADS"] = opts.get("threads") or "real"
        if self.real:
            # a scripted run shuts the window's keyboard and mouse out unless asked (player_move.rs); the mouse comes in
            # after the last scripted key is released
            e["NV_MOVE_REAL_INPUT"] = "1"
        e.update(self.extra_env)
        if self.keys:
            e["NV_MOVE_KEYS"] = ",".join(f"{f}:{d:X}:{h}" for f, d, h in sorted(self.keys, key=_order))
        if self.mouse:
            e["NV_MOVE_MOUSE"] = ",".join(f"{f}:{x}:{y}:{h}:{b}" for f, x, y, h, b in sorted(self.mouse, key=_order))
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

    def console(self, command, settle=1.0, close=True):
        """Open the console, type a command, run it, close the console."""
        self.press("grave", 0.1, 0.5)
        self.type(command)
        self.press("enter", 0.1, settle)
        if close:
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
        shown = f"screens seen: {', '.join(dict.fromkeys(seen)) or 'none'}"
        if name == "loading" and name not in seen and m.get("loading_menu"):
            # The nv-rs 0.14 viewer logs the loading menu as "play" in the frame log; its game event says it was up.
            return True, shown + " (the loading menu's game event was logged)"
        return name in seen, shown

    return (f"reached the {name} screen", check)


def moved_at_least(units):
    def check(m):
        p = m["movement"]["path"]
        return p >= units, f"player path {p:.0f} units (want {units})"

    return (f"player moved {units}+ units", check)


def sneak_checked():
    """The sneak check ran (its verdict is nvbench's "Ctrl made the player sneak")."""
    def check(m):
        return bool(m.get("sneak")), "the standing and sneaking steps were taken" if m.get("sneak") else "no sneak check in the log"

    return ("the sneak check ran", check)


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
]
OPENING_END = 7800  # first frames in the first room; Doc Mitchell's house
# The rest of the opening counts play frames (frames that are not a loading screen), so a slow load does not move it.
# The loading screen began at main frame 2899, so the new game's first frame is play frame 2899.
PLAY_START = 2899
PACKS = [(PLAY_START + 500 + 400 * k, "enter", 30) for k in range(6)]  # the downloadable packs' message boxes
PLAY_END = PLAY_START + 1800  # first frames in the first room; Doc Mitchell's house
# main frames the loading screen of the new game took in the reference runs; only for when a run is ended by frames
LOADING_FRAMES = 3200


def opening(plan):
    for f, key, hold in OPENING:
        plan.keys.append((f, DIK[key], hold))
    plan.keys.extend((f"p{f}", DIK[key], hold) for f, key, hold in PACKS)


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


# The opening quest's id (a form id of the base game) and its stages that matter here. The quest runs on timers and on
# lines Doc says; those lines never finish on this port (docs/fork/BENCH.md), so the scenario sets the stage the line
# would have set, through the game's own console. The console takes ids, not editor names, on this port.
OPENING_QUEST = "104c1c"
STAGE_NAME, STAGE_SEX, STAGE_REFLECTRON = 10, 17, 36
# play frames the quest needs to reach the stage in which Doc's second line is due (it does by about 10 s)
QUEST_SETTLE_S = 20.0


def chargen(opts):
    """The opening's choices with fixed answers: name, sex, then the mirror (the face menu) opened. Reaches the face
    menu and stops: it does not draw its head or take keys yet (docs/fork/BENCH.md)."""
    p = Plan(
        "chargen",
        "New game, then through the opening quest's name and sex choices to the face menu, by the console's stage command",
        fps=opts["fps"],
        capped_only=True,
    )
    opening(p)
    sc = Script(fps=opts["fps"], t=PLAY_END / opts["fps"] + QUEST_SETTLE_S)
    stage = lambda n: f"setstage {OPENING_QUEST} {n}"  # noqa: E731
    sc.console(stage(STAGE_NAME), settle=2.0, close=False)  # the name box opens over the console
    sc.wait(2.0).type("Courier").press("enter", 0.25, 2.0)  # accepting the name closes the console too
    sc.console(stage(STAGE_SEX), settle=2.0, close=False)
    sc.press("grave", 0.25, 2.5)  # the sex box (first choice is the default)
    sc.press("enter", 0.25, 2.0)
    sc.console(stage(STAGE_REFLECTRON), settle=2.0, close=False)
    sc.wait(8.0)
    p.keys += on_play_clock(sc.keys)
    p.end_frame = LOADING_FRAMES + sc.frame() + 600
    p.checks = [reached_screen("loading"), reached_screen("play"), player_placed()]
    return p


def probe(opts):
    """A debugging scenario: PROBE is a `;`-list of console commands and `key:`, `type:`, `wait:`, `mouse:`, `click`,
    `close` steps after the opening, PROBE_START seconds into the first room."""
    import os
    p = Plan("probe", "probe", fps=opts["fps"], capped_only=True)
    opening(p)
    sc = Script(fps=opts["fps"], t=PLAY_END / opts["fps"] + float(os.environ.get("PROBE_START", "2")))
    cmds = [c for c in os.environ.get("PROBE", "").split(";") if c]
    for i, c in enumerate(cmds):
        if c.startswith("key:"):
            sc.press(c[4:], 0.25, 1.5)
        elif c.startswith("wait:"):
            sc.wait(float(c[5:]))
        elif c.startswith("type:"):
            sc.type(c[5:])
        elif c.startswith("mouse:"):
            dx, dy, secs = c[6:].split(":")
            sc.look(int(dx), int(dy), float(secs))
        elif c == "click":
            sc.click(0.3, 1.0)
        elif c == "close":
            sc.press("grave", 0.25, 0.8)
        elif c.startswith("run:"):  # a command in the console that is already open
            sc.type(c[4:]).press("enter", 0.25, 2.5)
        else:
            sc.console(c, settle=2.5, close=False)
    p.keys += on_play_clock(sc.keys)
    p.mouse += on_play_clock(sc.mouse)
    p.end_frame = LOADING_FRAMES + sc.frame() + 400
    p.checks = []
    return p

# ---------------------------------------------------------------------------------------------
# Into the world without a save. Form ids of the base game (FalloutNV.esm; numbers only, no data): the opening quest
# VCG01 finished by its last stage (the game frees the player and ends Doc's lines), then a console `player.moveto`
# to a placed reference, which also loads the exterior. Checked 2026-11-16 with the nested window and offscreen.
PLACES = {
    "goodsprings": ("1658B7", "GoodspringsArrivalMarker"),
    "novac": ("8D0A3", "NovacCOCMarkerREF"),
    "strip": ("16ACB9", "VStripNPCPortToNorthMarker"),
    "freeside": ("13A669", "FreesideFastTravelMarker"),
}
QUEST_DONE_STAGE = 200
HOSTILE = "168D0B"  # VSpawnSpecialTier1GeckoSmallRoachHunter: a small gecko, hostile on sight
PISTOL, AMMO_9MM = "E3778", "8ED03"  # WeapNV9mmPistol, Ammo9mm
ARRIVE_S = 8.0  # play seconds the exterior takes to load and settle after a teleport (the console is shut after)


def walk(sc, seconds, turn=0, sprint=False):
    """Walk forward for `seconds`, turning by `turn` mouse counts per frame over the first second (a fixed route)."""
    keys = ("w", "lshift") if sprint else ("w",)
    t0 = sc.t
    for k in keys:
        sc.keys.append((sc.frame(), DIK[k], max(1, sc.frame(sc.t + seconds) - sc.frame())))
    if turn:
        sc.mouse.append((sc.frame(), turn, 0, max(1, sc.frame(sc.t + 1.0) - sc.frame()), 0))
    sc.t = t0 + seconds
    return sc


def teleport(sc, place, boxes=24):
    """The console is open and ready: move the player to a place, wait for the load, then dismiss the downloadable
    packs' message boxes that arrive with the first exterior. The first box shuts the console itself (a console key
    after them opens it again and the script is lost), so no console key follows."""
    sc.type(f"player.moveto {PLACES[place][0]}").press("enter", 0.25, ARRIVE_S)
    for _ in range(boxes):
        sc.press("enter", 0.25, 1.0)
    return sc


def into_world(opts, place, name, description):
    """A plan that finishes the opening quest and teleports to `place`; returns (plan, script positioned after it)."""
    p = Plan(name, description, fps=opts["fps"], capped_only=True)
    opening(p)
    sc = Script(fps=opts["fps"], t=PLAY_END / opts["fps"] + 2.0)
    # The quest's stage 50 removes the sitting animation's script package and stage 55 gives the player movement and
    # looking back (stage 0 took them: DisablePlayerControls 1 1 1 1 0 0 1; read from the quest's result scripts);
    # a jump straight to the last stage leaves the player seated in the animated camera, unable to walk or turn.
    sc.console(f"setstage {OPENING_QUEST} 50", settle=1.5, close=False)
    for stage in (55, QUEST_DONE_STAGE):
        sc.type(f"setstage {OPENING_QUEST} {stage}").press("enter", 0.25, 1.5)
    teleport(sc, place)
    return p, sc


def finish(p, sc, checks):
    p.keys += on_play_clock(sc.keys)
    p.mouse += on_play_clock(sc.mouse)
    p.end_frame = LOADING_FRAMES + sc.frame() + 300
    p.self_scaled = True
    p.checks = [reached_screen("loading"), reached_screen("play"), player_placed()] + checks
    return p


ROUTE_TOWN = [(10, 0), (4, 14), (10, 0), (4, 14), (10, 0), (4, 14), (10, 0), (4, 14)]  # (seconds forward, turn): a loop


def laps(opts, n=3):
    """Laps of a route: `n`, shortened or lengthened by --scale (the key script cannot be cut short, so the length is
    chosen when the plan is built)."""
    return max(1, round(n * (opts.get("scale") or 1.0)))


def route(sc, legs, scale=1.0):
    for secs, turn in legs:
        walk(sc, secs * scale, turn)
    return sc


def goodsprings(opts):
    p, sc = into_world(opts, "goodsprings", "goodsprings", "Finish the opening by its quest stage, teleport to Goodsprings, walk a fixed loop of the town for 2 min")
    for _ in range(laps(opts)):
        route(sc, ROUTE_TOWN)
    return finish(p, sc, [moved_at_least(500 * laps(opts)), play_seconds_at_least(40 * laps(opts))])


def novac(opts):
    p, sc = into_world(opts, "novac", "novac", "Teleport to Novac (a town of many actors and objects), walk a fixed loop for 2 min")
    for _ in range(laps(opts)):
        route(sc, ROUTE_TOWN)
    return finish(p, sc, [moved_at_least(500 * laps(opts)), play_seconds_at_least(40 * laps(opts))])


def strip(opts):
    p, sc = into_world(opts, "strip", "strip", "Teleport to the Strip (the heaviest exterior), walk a fixed loop for 2 min")
    for _ in range(laps(opts)):
        route(sc, ROUTE_TOWN)
    return finish(p, sc, [moved_at_least(500 * laps(opts)), play_seconds_at_least(40 * laps(opts))])


def combat(opts):
    """Gunfire: three hostile geckos placed at the player, a pistol and ammo, 40 s of aiming around and firing."""
    p, sc = into_world(opts, "goodsprings", "combat", "In Goodsprings: spawn hostile creatures, give and equip a pistol, turn and fire for 40 s")
    sc.press("grave", 0.25, 1.0)
    for cmd in (f"player.additem {AMMO_9MM} 300", f"player.additem {PISTOL} 1", f"player.equipitem {PISTOL}", f"player.placeatme {HOSTILE} 3"):
        sc.type(cmd).press("enter", 0.25, 1.5)
    sc.press("grave", 0.25, 2.0)
    for _ in range(10):  # a burst of shots, a turn
        for _ in range(8):
            sc.click(0.15, 0.35)
        sc.look(12, 0, 0.8)
    return finish(p, sc, [play_seconds_at_least(30)])


def soak(opts):
    """Long: four regions in turn, walking in each, looped; memory and GPU growth are the numbers (20 min; --scale)."""
    p, sc = into_world(opts, "goodsprings", "soak", "Goodsprings, Novac, the Strip, Freeside in turn, walking each, repeated for 20 min")
    places = ["novac", "strip", "freeside", "goodsprings"]
    for lap in range(laps(opts, 4)):
        for place in places:
            for _ in range(2):
                route(sc, ROUTE_TOWN)
            sc.press("grave", 0.25, 0.8)
            teleport(sc, place, boxes=3)
    return finish(p, sc, [moved_at_least(2000), play_seconds_at_least(600)])

def realwalk(opts):
    """Real keyboard and pointer events (nested window only, inject.py) after the teleport: walk, turn, Ctrl (sneak)."""
    p, sc = into_world(opts, "goodsprings", "realwalk", "Goodsprings, then real events into the window: walk, turn, walk, Ctrl taps")
    p = finish(p, sc, [moved_at_least(300)])
    p.real = [
        ("wait", 2.0), ("hold", ("w",), 6.0), ("move", 12, 0, 60, 0.01), ("hold", ("w",), 6.0),
        ("tap", "ctrl"), ("wait", 1.0), ("hold", ("ctrl",), 1.0), ("wait", 1.0), ("hold", ("w",), 4.0),
    ]
    p.extra_env["NV_LOG"] = "game=info"
    p.end_frame += p.real_settle_frames + int(40 * opts["fps"])
    return p


# The world scenarios of the benchmark: one viewer session each, start-up and the new game's first room at the front,
# then ~25 s a place (3 s warm-up, ~18 s measured: camera sweeps and a short walk). Budget: 120 s of wall time for the
# whole run (`quick` is all places in one session), checked by the verdict as TOO_LONG.
BUDGET_S = 120.0
WORLD = {
    "doc_house": (("doc_house",), "Doc Mitchell's house: start-up to the first room, then a camera sweep and a short walk"),
    "goodsprings": (("goodsprings",), "Goodsprings: start-up, then a console teleport and a camera sweep and a short walk"),
    "strip": (("strip",), "The Strip: start-up, then a console teleport and a camera sweep and a short walk"),
    "novac": (("novac",), "Novac: start-up, then a console teleport and a camera sweep and a short walk"),
    "sneak": (("goodsprings",), "Goodsprings: start-up, a console teleport, then the sneak check (real Ctrl) and the measured script"),
    "quick": (("doc_house", "goodsprings", "strip"), "The quick suite in one session: Doc's house, Goodsprings, the Strip, under 120 s in all"),
}


def state_noted(name):
    """The driver noted the player's position as `name` (the save round trip)."""
    def check(m):
        st = (m.get("states") or {}).get(name)
        return bool(st), (f"{name} at " + ", ".join(f"{v:.1f}" for v in st)) if st else f"no {name} position in the log"

    return (f"the {name} position was noted", check)


def round_trip_plan(name, opts):
    """The save round trip's two runs (docs/fork/RELEASE-TEST.md): `save` writes a save in a new game, `continue` loads
    it in a new session from the main menu. Each notes the player's position; the release test compares them."""
    desc = {
        "save": "New game, Goodsprings, a short walk, the position noted, then the console's save",
        "continue": "Start-up, Continue on the main menu (the newest save), the position noted",
    }[name]
    p = Plan(name, desc, fps=opts.get("fps", 60.0))
    p.checks = [reached_screen("play"), player_placed(), state_noted("saved" if name == "save" else "loaded")]
    if name == "save":
        p.checks.insert(0, reached_screen("loading"))
    p.drive, p.self_scaled = DRIVERS[name], True
    return p


def world_plan(name, opts):
    places, desc = WORLD[name]
    n = len(places)
    p = Plan(name, desc, fps=opts.get("fps", 60.0))
    p.checks = [reached_screen("loading"), reached_screen("play"), player_placed(), moved_at_least(150 * n), play_seconds_at_least(14 * n)]
    if name == "sneak":
        p.checks.append(sneak_checked())
    p.drive, p.self_scaled, p.budget_s = DRIVERS[name], True, BUDGET_S
    return p


def views_captured(names):
    """Every view of the list has its picture and its position noted (`state view.<name>`: written after the picture),
    and says why when it has none (`view-failed`: the player did not stand still at the view's height, in a loaded
    place, within the time allowed: a picture taken in a bad state is no picture)."""
    def check(m):
        states = m.get("states") or {}
        failed = m.get("view_failures") or {}
        missing = [n for n in names if f"view.{n}" not in states]
        if not missing:
            return True, f"{len(names)} views captured"
        return False, "no picture of: " + ", ".join(f"{n} ({failed[n]})" if n in failed else n for n in missing)

    return ("every view has its picture, taken standing", check)


def no_sneak_key():
    """No Ctrl press reached the game: the player cannot have been sneaking at any picture (sneaking is the game's Ctrl toggle)."""
    return ("the player was not sneaking (no Ctrl reached the game)", lambda m: (not m.get("ctrl_down"), f"{m.get('ctrl_down', 0)} Ctrl presses reached the game"))


def views_plan(opts):
    """Fixed views of the world (nv-rs#343, docs/fork/BENCH.md): the same places, headings and time of day each run."""
    import views as viewlib

    names = [v.name for v in (opts.get("views") or viewlib.DEFAULT_VIEWS)]
    p = Plan("views", f"Fixed views of the Mojave at a set hour: {len(names)} places and headings, one picture each, with a manifest", fps=opts.get("fps", 60.0))
    p.checks = [reached_screen("play"), player_placed(), views_captured(names), no_sneak_key()]
    p.drive, p.self_scaled = DRIVERS["views"], True
    return p


def build(name, opts):
    if name not in SCENARIOS:
        raise SystemExit(f"unknown scenario {name!r}; known: {', '.join(sorted(SCENARIOS))}")
    if name in WORLD and not opts.get("legacy"):
        return world_plan(name, opts)
    p = SCENARIOS[name](opts)
    if name in DRIVERS and not opts.get("legacy"):
        # the same description and verdicts, but driven in seconds and by the game's screen: valid at any frame rate
        p.keys, p.mouse, p.real, p.end_frame, p.capped_only, p.self_scaled = [], [], [], 0, False, True
        p.drive = DRIVERS[name]
    return p


SCENARIOS = {
    "boot": boot, "newgame": newgame, "chargen": chargen, "probe": probe,
    "goodsprings": goodsprings, "novac": novac, "strip": strip, "combat": combat, "soak": soak, "realwalk": realwalk,
    "doc_house": lambda opts: world_plan("doc_house", opts), "quick": lambda opts: world_plan("quick", opts),
    "sneak": lambda opts: world_plan("sneak", opts),
    "views": views_plan,
    "save": lambda opts: round_trip_plan("save", opts), "continue": lambda opts: round_trip_plan("continue", opts),
}
