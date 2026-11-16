"""Scenario drivers: the scenarios of scenarios.py as closed-loop, wall-clock programs for driver.py, valid at any frame
rate (docs/fork/BENCH.md). Each takes a Driver and an options dict (`scale` shortens or lengthens the walking)."""

# Form ids of the base game (numbers only; scenarios.PLACES and friends hold the same).
OPENING_QUEST = "104c1c"
STAGES_FREE = (50, 55, 200)  # 50 removes the sitting package, 55 gives movement and looking back, 200 ends the quest
ROUTE_TOWN = [(10.0, 0.0), (4.0, 14.0)] * 4  # (seconds forward, turn counts per 10 ms in the first second): a loop


def laps(opts, n=3):
    return max(1, round(n * (opts.get("scale") or 1.0)))


def start_new_game(d, hold_first_room=0.0, quiet=6.0, brisk=False):
    """Start-up movies skipped, main menu, New, the intro movie skipped, the pack boxes dismissed, the first room.
    `brisk` (the quick suite) shortens the fixed waits of the start-up movies and the main menu."""
    if not d.until(lambda: bool(d.tail()), 90, what="the first frame"):
        return False
    movie, menu, step = (1.5, 4.5, 1.0) if brisk else (2.0, 6.0, 1.5)
    for _ in range(4):  # the start-up movies
        d.wait(movie)
        d.tap("esc", 0.1)
    d.wait(menu)
    d.tap("down", 0.3)  # wakes the menu's keyboard mode
    d.wait(step)
    d.tap("down", 0.3)  # Continue -> New
    d.wait(step)
    d.tap("enter", 0.3)  # "Start a new game?"
    d.wait(step)
    d.tap("enter", 0.3)  # Yes: the intro movie starts on the loading screen
    if not d.until_screen("loading", 40):
        return False
    d.until_not_screen("loading", 180, tap="esc", tap_every=2.0)
    if not d.settle_play(quiet=quiet, timeout=240, tap="enter", console_after=None):
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
    d.look(0, 8, 2.4 * k)  # down through level (as far below as it went above)
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
    """The place's warm-up: the play screen held `seconds` with no box (a pack's box can come a few seconds after the
    load; one that came during the measured script would leave it measuring a menu)."""
    d.settle_play(quiet=seconds, timeout=30)


def free_player(d):
    """The quest stages that give the player movement and looking back (once per session), then play again with the
    boxes the stages bring dismissed."""
    if getattr(d, "freed", False):
        return True
    d.console_open()
    for stage in (50, 55):
        d.console_line(f"setstage {OPENING_QUEST} {stage}", 0.6)
    d.freed = True
    return d.console_close(quiet=0.5)


def at_doc_house(d, opts):
    """Doc Mitchell's house: the new game's first room, where the session already is; the player freed by the quest
    stages (a later visit teleports back to it)."""
    if getattr(d, "visited", None):
        return at_marker(d, "107235")  # GSDocMitchellBesideMarker (its trigger asks for the name on a new game)
    return free_player(d)


def moved(before, after, units=200.0):
    return bool(before and after and sum((a - b) ** 2 for a, b in zip(after, before)) ** 0.5 >= units)


def at_marker(d, marker, quiet=0.5, tries=2):
    """A console teleport to a placed reference, seen to happen (the player is 200+ units from where it was)."""
    free_player(d)
    for _ in range(tries):
        before = d.position()
        d.console_open()
        d.console_line(f"player.moveto {marker}", 0.0)
        # The console stays open over the load. Shut it as soon as the player has moved and the load is over, before
        # the packs' boxes come (a few seconds later): a box that comes over the console never gets the keyboard.
        d.until(lambda: moved(before, d.position()), 20, every=0.1, what="the teleport")
        d.until(lambda: d.screen() != "loading", 60, every=0.1, what="the end of the load")
        d.wait(0.3)
        d.console_close(quiet=quiet, timeout=60)
        if moved(before, d.position()):
            return True
        d.note(f"the teleport to {marker} did not move the player: again")
    return False


PLACES_Q = {
    "doc_house": lambda d, o: at_doc_house(d, o),
    "goodsprings": lambda d, o: at_marker(d, "1658B7"),
    "strip": lambda d, o: at_marker(d, "16ACB9"),
    "novac": lambda d, o: at_marker(d, "8D0A3"),
}


def visit(d, opts, name):
    d.place = name
    phase(d, name, "start")
    if not PLACES_Q[name](d, opts):
        d.note(f"{name}: not reached")
    d.visited = (getattr(d, "visited", None) or []) + [name]
    phase(d, name + ".warm", "start")
    warm(d)
    phase(d, name + ".warm", "end")
    phase(d, name + ".measured", "start")
    measured(d, opts)
    phase(d, name + ".measured", "end")
    phase(d, name, "end")


def session(places, measure=1.0, quiet=6.0, brisk=False):
    """One viewer session: start-up, the new game's first room, then each place in turn by a console teleport.
    `measure` scales each place's measured script (the quick suite shortens it to fit its 120 s), `quiet` is how long
    the first room's play screen must hold before the places start."""

    def run(d, opts):
        opts = dict(opts, scale=(opts.get("scale") or 1.0) * measure)
        phase(d, "startup", "start")
        ok = start_new_game(d, hold_first_room=0.0, quiet=quiet, brisk=brisk)
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
    d.console_close(quiet=2.0)
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
    "quick": session(["doc_house", "goodsprings", "strip"], measure=0.5, quiet=3.0, brisk=True),
    "combat": combat,
    "soak": soak,
}

