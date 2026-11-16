"""Scenario drivers: the scenarios of scenarios.py as closed-loop, wall-clock programs for driver.py, valid at any frame
rate (docs/fork/BENCH.md). Each takes a Driver and an options dict (`scale` shortens or lengthens the walking)."""

# Form ids of the base game (numbers only; scenarios.PLACES and friends hold the same).
OPENING_QUEST = "104c1c"
STAGES_FREE = (50, 55, 200)  # 50 removes the sitting package, 55 gives movement and looking back, 200 ends the quest
ROUTE_TOWN = [(10.0, 0.0), (4.0, 14.0)] * 4  # (seconds forward, turn counts per 10 ms in the first second): a loop


def laps(opts, n=3):
    return max(1, round(n * (opts.get("scale") or 1.0)))


def start_new_game(d, hold_first_room=0.0):
    """Start-up movies skipped, main menu, New, the intro movie skipped, the pack boxes dismissed, the first room."""
    if not d.until(lambda: bool(d.tail()), 90, what="the first frame"):
        return False
    for _ in range(4):  # the start-up movies
        d.wait(2.0)
        d.tap("esc", 0.1)
    d.wait(6.0)
    d.tap("down", 0.3)  # wakes the menu's keyboard mode
    d.wait(1.5)
    d.tap("down", 0.3)  # Continue -> New
    d.wait(1.5)
    d.tap("enter", 0.3)  # "Start a new game?"
    d.wait(1.5)
    d.tap("enter", 0.3)  # Yes: the intro movie starts on the loading screen
    if not d.until_screen("loading", 40):
        return False
    d.until_not_screen("loading", 180, tap="esc", tap_every=2.0)
    if not d.settle_play(quiet=6.0, timeout=240):
        return False
    d.wait(hold_first_room)
    return True


def go_to(d, marker, first=True):
    """The console: finish the opening by its stages (first time) and teleport to a placed reference."""
    d.console_open()
    if first:
        for stage in STAGES_FREE:
            d.console_line(f"setstage {OPENING_QUEST} {stage}", 1.2)
    d.console_line(f"player.moveto {marker}", 1.0)
    # the packs' boxes arrive with the first exterior; the first one shuts the console
    return d.settle_play(quiet=8.0, timeout=240)


def boot(d, opts):
    if not d.until(lambda: bool(d.tail()), 90, what="the first frame"):
        return
    for _ in range(4):
        d.wait(2.0)
        d.tap("esc", 0.1)
    d.wait(40.0 * (opts.get("scale") or 1.0))


def newgame(d, opts):
    if start_new_game(d, hold_first_room=20.0 * (opts.get("scale") or 1.0)):
        d.note("first room reached")


def phase(d, name, what):
    """A phase boundary in the log with the frame and the seconds since the run began (parse.phases reads it)."""
    rows = d.tail(1024)
    frame = rows[-1][0] if rows else 0
    d.note(f"phase {name} {what} frame={frame} t={d.clock() - d.t0:.2f}")


def measured(d, opts):
    """The measured script of a location: about 18 s of camera sweeps and a short walk, the same everywhere."""
    k = opts.get("scale") or 1.0
    d.look(10, 0, 4.0 * k)  # yaw sweep to the right (counts per 10 ms for 4 s; the game's sensitivity decides the angle)
    d.look(-10, 0, 4.0 * k)  # and back
    d.look(0, -8, 1.2 * k)  # pitch up
    d.look(0, 16, 2.4 * k)  # down through level
    d.look(0, -8, 1.2 * k)  # level again
    place = getattr(d, "place", "place")
    phase(d, place + ".walk1", "start")
    d.walk(3.0 * k)  # forward
    phase(d, place + ".walk1", "end")
    d.hold(("a",), 1.0 * k)  # strafe left
    d.hold(("d",), 1.0 * k)  # and right
    phase(d, place + ".walk2", "start")
    d.walk(3.0 * k, turn=14.0)  # forward, turning in its first second: the heading of its last seconds is a new one
    phase(d, place + ".walk2", "end")


def warm(d, seconds=3.0):
    d.wait(seconds)


def free_player(d):
    """The quest stages that give the player movement and looking back (once per session); the console is left open."""
    d.console_open()
    if not getattr(d, "freed", False):
        for stage in (50, 55):
            d.console_line(f"setstage {OPENING_QUEST} {stage}", 0.8)
        d.freed = True


def at_doc_house(d, opts):
    """Doc Mitchell's house: the new game's first room (or back to it), the player freed by the quest stages."""
    free_player(d)
    d.console_line("player.moveto 107235", 0.8)  # GSDocMitchellBesideMarker; the first room already when new
    d.tap("grave", 0.15)
    d.wait(1.0)


def at_marker(d, marker, quiet=3.0):
    free_player(d)
    d.console_line(f"player.moveto {marker}", 0.8)
    return d.settle_play(quiet=quiet, timeout=60)


PLACES_Q = {
    "doc_house": lambda d, o: at_doc_house(d, o),
    "goodsprings": lambda d, o: at_marker(d, "1658B7", quiet=4.0),
    "strip": lambda d, o: at_marker(d, "16ACB9"),
    "novac": lambda d, o: at_marker(d, "8D0A3"),
}


def visit(d, opts, name):
    d.place = name
    phase(d, name, "start")
    PLACES_Q[name](d, opts)
    phase(d, name + ".warm", "start")
    warm(d)
    phase(d, name + ".measured", "start")
    measured(d, opts)
    phase(d, name + ".measured", "end")
    phase(d, name, "end")


def session(places):
    """One viewer session: start-up, the new game's first room, then each place in turn by a console teleport."""

    def run(d, opts):
        phase(d, "startup", "start")
        ok = start_new_game(d, hold_first_room=0.0)
        phase(d, "startup", "end")
        if not ok:
            return
        for name in places:
            if not d.alive():
                return
            visit(d, opts, name)

    return run


def combat(d, opts, marker="1658B7", hostile="168D0B", pistol="E3778", ammo="8ED03"):
    if not start_new_game(d, hold_first_room=0.0):
        return
    at_marker(d, marker, quiet=4.0)
    d.console_open()
    for cmd in (f"player.additem {ammo} 300", f"player.additem {pistol} 1", f"player.equipitem {pistol}", f"player.placeatme {hostile} 3"):
        d.console_line(cmd, 1.0)
    d.tap("grave", 0.15)
    d.wait(2.0)
    phase(d, "combat.measured", "start")
    for _ in range(max(1, round(3 * (opts.get("scale") or 1.0)))):
        for _ in range(6):
            d.click(0.15)
            d.wait(0.3)
        d.look(12, 0, 0.8)
    phase(d, "combat.measured", "end")


def soak(d, opts):
    places = ["novac", "strip", "goodsprings", "doc_house"]
    if not start_new_game(d, hold_first_room=0.0):
        return
    for _ in range(laps(opts, 4)):
        for name in places:
            if not d.alive():
                return
            if name != "doc_house":
                at_marker(d, {"novac": "8D0A3", "strip": "16ACB9", "goodsprings": "1658B7"}[name], quiet=4.0)
            for _ in range(8):
                measured(d, dict(opts, scale=1.0))


DRIVERS = {
    "boot": boot,
    "newgame": newgame,
    "doc_house": session(["doc_house"]),
    "goodsprings": session(["doc_house", "goodsprings"][1:]),
    "strip": session(["strip"]),
    "novac": session(["novac"]),
    "quick": session(["doc_house", "goodsprings", "strip"]),
    "combat": combat,
    "soak": soak,
}
