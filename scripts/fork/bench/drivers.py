"""Scenario drivers: the scenarios of scenarios.py as closed-loop, wall-clock programs for driver.py, valid at any frame
rate (docs/fork/BENCH.md). Each takes a Driver and an options dict (`scale` shortens or lengthens the walking)."""
import os

import views as viewlib

# Form ids of the base game (numbers only; scenarios.PLACES and friends hold the same).
OPENING_QUEST = "104c1c"
OPENING_QUEST_NAME = "VCG01"  # its editor id, for its script's variables
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
    # The menu opens with Continue highlighted and the first Down already moves the selection (it does not
    # only wake a keyboard mode: nv-rs#410, lodwright-rs#156): one Down is Continue -> New, two land on Load.
    d.tap("down", 0.3)  # Continue -> New
    d.wait(step)
    d.tap("enter", 0.3)  # "Start a new game?"
    d.wait(step)
    d.tap("enter", 0.3)  # Yes: the intro movie starts on the loading screen
    # The loading menu is not always a "loading" row in the frame log (the nv-rs 0.14.0 viewer logs it as "play"):
    # the menu is left, then the new game's player is placed in its first cell (the position leaves the start-up
    # value), the intro movie skipped with Escape on the way.
    if not d.until(lambda: d.screen() != "menu", 40, what="the main menu to be left"):
        return False
    placed = lambda: (d.position() or (2048.0, 2048.0, 128.0))[:2] != (2048.0, 2048.0)
    last = [0.0]

    def loaded():
        if d.clock() - last[0] >= 2.0:
            last[0] = d.clock()
            d.tap("esc", 0.1)
        return placed()

    if not d.until(loaded, 240, every=0.1, what="the new game's first cell"):
        return False
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


def clear_box(d):
    """Before a walk of the measured script: a box that came during the sweeps (the packs' boxes come one after another
    in the first exterior, a few seconds apart) is dismissed, so the walk walks."""
    if d.screen() != "play":
        d.note("a box came during the measured script: dismissed before the walk")
        d.settle_play(quiet=0.3, timeout=20)


def measured(d, opts):
    """The measured script of a location: about 18 s of camera sweeps and a short walk, the same everywhere."""
    k = opts.get("scale") or 1.0
    d.look(10, 0, 4.0 * k)  # yaw sweep to the right (counts per 10 ms for 4 s; the game's sensitivity decides the angle)
    d.look(-10, 0, 4.0 * k)  # and back
    d.look(0, -8, 1.2 * k)  # pitch up
    d.look(0, 8, 2.4 * k)  # down through level (as far below as it went above)
    d.look(0, -8, 1.2 * k)  # level again
    place = getattr(d, "place", "place")
    clear_box(d)
    if place == "doc_house":
        # The opening's package can come back after the warm-up's check (the end of its first line sets it again):
        # checked once more right before the walks.
        ensure_walks(d)
    measured_walk(d, place + ".walk1", 3.0 * k)  # forward
    d.hold(("a",), 1.0 * k)  # strafe left
    d.hold(("d",), 1.0 * k)  # and right
    # forward, turning in its first second: the heading of its last seconds is a new one
    measured_walk(d, place + ".walk2", 3.0 * k, turn=14.0)


def measured_walk(d, name, seconds, turn=None):
    """One walk of the measured script between its phase marks. A pack's box that comes up during the walk stops the
    player (2026-11-17: in the first town, late in the first walk), so the box is dismissed and the walk made once more
    with new marks; the parser takes the last marks of a name."""
    for attempt in range(2):
        clear_box(d)
        phase(d, name, "start")
        if turn is None:
            d.walk(seconds)
        else:
            d.walk(seconds, turn=turn)
        phase(d, name, "end")
        if d.screen() == "play":
            return
        if attempt == 0:
            d.note(f"a box came during {name}: dismissed and the walk made again")
        else:
            d.note(f"a box came during {name} again: the walk is left as it is")


def warm(d, seconds=3.0):
    """The place's warm-up: the play screen held `seconds` with no box (a pack's box can come a few seconds after the
    load; one that came during the measured script would leave it measuring a menu)."""
    d.settle_play(quiet=seconds, timeout=30)


def warm_first_exterior(d, within=6.5):
    """The warm-up of the session's first exterior: one more of the packs' boxes comes there about 5 s after the load
    (2026-11-16: 4.9 to 5.0 s in four quick runs), after a 3 s warm-up was over. So the box is waited for, up to
    `within` s, and dismissed, then the play screen held 1 s."""
    d.until(lambda: d.screen() != "play", within, every=0.1, what="the first exterior's late box")
    d.settle_play(quiet=1.0, timeout=30)



def free_player(d):
    """The quest stages that give the player movement and looking back (once per session), then play again with the
    boxes the stages bring dismissed.

    The opening runs on its own clock too: its quest script steps the early stages on a timer, and the end of the
    opening's first spoken line sets stage 5, which puts the sitting package back on the player (and starts the timer to
    stage 7, which does it again). When that line ends after these stages, which depends on how long the start-up took,
    the player is held by the package again a few seconds later: movement and looking enabled, but nothing moves. So the
    timer is stopped here, and `ensure_walks` takes a package that came back off before each measured script."""
    if getattr(d, "freed", False):
        return True
    d.console_open()
    for stage in (50, 55):
        d.console_line(f"setstage {OPENING_QUEST} {stage}", 0.4)
    d.console_line(f"set {OPENING_QUEST_NAME}.bRunTimer to 0", 0.4)
    d.freed = True
    return d.console_close(quiet=0.5)


def walks(d, keys=("w", "s"), seconds=0.4, units=30.0):
    """Whether a short step moves the player (forward, else back: a wall in front stops one of them only). A free step
    of 0.4 s goes 70 units or more; the opening's package moves a held player a few units on its own, so a smaller
    threshold took that drift for a step."""
    for key in keys:
        before = d.position()
        d.hold((key,), seconds)
        d.wait(0.2)
        if moved(before, d.position(), units):
            return True
    return False


def ensure_walks(d, tries=2):
    """Closed loop: the player answers the movement keys, or the opening's package that came back (free_player) is
    taken off by the console and the step tried again. False (and a note) when it still does not walk."""
    for attempt in range(tries + 1):
        if walks(d):
            return True
        if attempt == tries:
            break
        d.note("the player does not walk: the opening's package is taken off again")
        d.console_open()
        d.console_line("player.removescriptpackage", 0.6)
        d.console_close(quiet=0.5)
    d.note("the player still does not walk")
    return False


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


def sneak_check(d, place, seconds=0.8):
    """Whether the real Ctrl key makes the player sneak, read from the walking speed: a step forward standing
    (`<place>.stand`) and back, Ctrl, the same step forward sneaking (`<place>.sneak`) and back, Ctrl again to stand
    up. Sneaking is slower, so parse.sneak_ratios compares the two forward paths. Each step goes back over the ground
    just walked, so the second one meets nothing the first did not."""
    for stance in ("stand", "sneak"):
        if stance == "sneak":
            d.tap("ctrl", 0.1)
            d.wait(0.6)
        phase(d, f"{place}.{stance}", "start")
        d.hold(("w",), seconds)
        phase(d, f"{place}.{stance}", "end")
        d.hold(("s",), seconds)
    d.tap("ctrl", 0.1)
    d.wait(0.6)


PLACES_Q = {
    "doc_house": lambda d, o: at_doc_house(d, o),
    "goodsprings": lambda d, o: at_marker(d, "1658B7"),
    "strip": lambda d, o: at_marker(d, "16ACB9"),
    "novac": lambda d, o: at_marker(d, "8D0A3"),
}


def visit(d, opts, name, sneak=False):
    d.place = name
    phase(d, name, "start")
    if not PLACES_Q[name](d, opts):
        d.note(f"{name}: not reached")
    d.visited = (getattr(d, "visited", None) or []) + [name]
    phase(d, name + ".warm", "start")
    if name != "doc_house" and not getattr(d, "outside", False):
        d.outside = True
        warm_first_exterior(d)
    elif name == "doc_house" and opts.get("brisk"):
        # The quick suite: the first room's play screen has just held its `quiet` seconds at the end of the start-up,
        # and the console that freed the player held it again; a second warm-up there only costs the 120 s budget.
        pass
    else:
        warm(d)
    ensure_walks(d)
    phase(d, name + ".warm", "end")
    if sneak:
        # Stage 55 gives movement and looking back but not sneaking: the opening's stage 0 turned sneaking off with the
        # rest, and the opening turns it back on only later. Without this line Ctrl reaches the game and nothing happens.
        d.console_open()
        d.console_line("enableplayercontrols", 0.4)
        d.console_close(quiet=0.5)
        warm(d, 6.0)  # the packs' boxes come one after another in the first exterior: none during the check
        sneak_check(d, name)
    phase(d, name + ".measured", "start")
    measured(d, opts)
    phase(d, name + ".measured", "end")
    phase(d, name, "end")


def session(places, measure=1.0, quiet=6.0, brisk=False, sneak_at=()):
    """One viewer session: start-up, the new game's first room, then each place in turn by a console teleport.
    `measure` scales each place's measured script (the quick suite shortens it to fit its 120 s), `quiet` is how long
    the first room's play screen must hold before the places start."""

    def run(d, opts):
        opts = dict(opts, scale=(opts.get("scale") or 1.0) * measure, brisk=brisk)
        phase(d, "startup", "start")
        ok = start_new_game(d, hold_first_room=0.0, quiet=quiet, brisk=brisk)
        phase(d, "startup", "end")
        if not ok:
            return
        for name in places:
            if not d.alive():
                return
            visit(d, opts, name, sneak=name in sneak_at)

    return run



def near_xy(pos, view, units=60.0):
    return bool(pos) and ((pos[0] - view.x) ** 2 + (pos[1] - view.y) ** 2) ** 0.5 <= units


def calm(d, frames=60, limit=0.1, steady=0.5):
    """Whether the last `frames` frames of the log are even: no frame took more than `limit` s (a load or a stream
    shows as one long frame) and the player stopped moving (landed): `steady` units over the stretch."""
    rows = d.tail(16384)[-frames:]
    if len(rows) < frames:
        return False
    if max(r[5] for r in rows) > limit:
        return False
    first, last = rows[0][2:5], rows[-1][2:5]
    return sum((a - b) ** 2 for a, b in zip(first, last)) ** 0.5 <= steady


def run_clock(d, hours, scale=viewlib.CLOCK_SCALE, timeout=300.0):
    """Let the game's clock run `hours` game hours at `scale` (the console is shut meanwhile) and see it done: the
    seconds of play the frame log shows (the game stands still in a menu) are added up, so the console's open and
    shut times do not count. `set gamehour` stops the port (an unmapped write at 4, the sky's update after a jump), so the
    hour is reached the way the game itself reaches one. Returns the game seconds that passed."""
    need = hours * 3600.0 / scale - viewlib.CLOCK_LATENCY_S
    rows = d.tail(1)
    passed, last = 0.0, [rows[-1][0] if rows else 0]
    d.console_line(f"set timescale to {scale:g}", 0.3)
    d.console_close(quiet=0.0, timeout=60)

    def done():
        nonlocal passed
        if d.screen() == "menu":  # a box that came (the packs' boxes come in the first exterior): dismissed, the clock goes on
            d.settle_play(quiet=0.2, timeout=30)
        for r in d.tail(16384):
            if r[0] > last[0]:
                last[0] = r[0]
                if r[1] == "play":
                    passed += r[5]
        return passed >= need

    d.until(done, timeout, every=0.05, what=f"the clock to run {hours:g} hours")
    d.console_open()
    return passed


def set_sky(d, hour, weather, timescale=0):
    """The game hour, the clock stopped (the hour then stays for the whole run) and, when given, a forced weather;
    an argument that is None is left as the game has it. The hour is reached by running the clock on from the new
    game's start hour (viewlib.START_HOUR)."""
    d.console_open()
    d.console_line("tgm", 0.6)  # god mode: a fall (a view from the console's own height) does no damage (it killed the player)
    d.console_line("set timescale to 0", 0.4)
    if hour is not None:
        hours = (hour - viewlib.START_HOUR) % 24.0
        if hours > 0.0:
            run_clock(d, hours)
    if timescale is not None:
        d.console_line(f"set timescale to {timescale:g}", 0.6)
    if weather:
        d.console_line(f"fw {weather}", 1.0)
    d.note(f"sky hour={'game' if hour is None else format(hour, 'g')} timescale={'game' if timescale is None else format(timescale, 'g')} weather={weather or 'game'}")
    return d.console_close(quiet=1.0, timeout=60)


def place_view(d, view, settle):
    """Put the player at a view by the console, wait for the load, the play screen to hold `settle` s and the frames to
    even out; the level of the settle is noted. False when the player did not get there."""
    d.console_open()
    for line in viewlib.console_lines(view):
        d.console_line(line, 0.5)
    ok = d.until(lambda: near_xy(d.position(), view), 20, every=0.1, what=f"the move to {view.name}")
    d.until(lambda: d.screen() != "loading", 60, every=0.1, what="the end of the load")
    d.wait(0.3)
    d.console_close(quiet=2.0, timeout=60)
    d.settle_play(quiet=settle, timeout=settle + 90)
    if not d.until(lambda: calm(d), 60, every=0.25, what=f"the frames at {view.name} to even out"):
        d.note(f"{view.name}: the frame times did not even out; captured as it is")
    return ok


def views(d, opts):
    """The views scenario (nv-rs#343, docs/fork/BENCH.md): a new game, the Mojave exterior, the sky fixed, then each
    view of the list in turn: placed by the console, settled, one picture (`views/<name>.png` in the run directory) and
    the position noted (`state view.<name>`); `views.json` is the manifest, rewritten after every view."""
    plan = opts.get("views") or viewlib.DEFAULT_VIEWS
    hour = opts["hour"] if "hour" in opts else viewlib.HOUR
    timescale = opts["timescale"] if "timescale" in opts else 0
    weather = opts["weather"] if "weather" in opts else viewlib.WEATHER
    settle = opts.get("views_settle", viewlib.SETTLE_S)
    run_dir = os.path.dirname(d.frames)
    os.makedirs(os.path.join(run_dir, viewlib.DIR), exist_ok=True)
    captures = {}

    def write():
        viewlib.write_manifest(run_dir, viewlib.manifest(plan, hour, weather, settle, captures, resolution=opts.get("resolution"), timescale=timescale))

    write()
    if not start_new_game(d, hold_first_room=0.0):
        return
    if not at_marker(d, "1658B7"):  # Goodsprings: the way into the Mojave exterior
        d.note("the Mojave exterior was not reached")
        return
    warm_first_exterior(d)
    set_sky(d, hour, weather, timescale)
    for view in plan:
        if not d.alive():
            return
        phase(d, f"view.{view.name}", "start")
        if not place_view(d, view, settle):
            d.note(f"{view.name}: not reached")
            phase(d, f"view.{view.name}", "end")
            continue
        if not d.alive():  # the game stopped on the way: a picture now would be of nothing
            d.note(f"{view.name}: the game stopped before the picture")
            return
        name = f"{viewlib.DIR}/{view.name}.png"
        if d.capture(os.path.join(run_dir, name)):
            pos = d.position()
            captures[view.name] = {"observed": list(pos) if pos else None, "file": name}
            note_state(d, f"view.{view.name}")
            d.note(f"view {view.name} file={name}")
        else:
            d.note(f"{view.name}: no picture taken")
        phase(d, f"view.{view.name}", "end")
        write()


ROUND_TRIP_SAVE = "nvbench-roundtrip"  # the save the save round trip writes (docs/fork/RELEASE-TEST.md)


def note_state(d, what):
    """The player's position in the log (`# driver: state WHAT x=.. y=.. z=..`), which parse.parse_log reads."""
    pos = d.position()
    if pos:
        d.note(f"state {what} x={pos[0]:.1f} y={pos[1]:.1f} z={pos[2]:.1f}")


def save_game(d, opts):
    """The first half of the save round trip: a new game, the player freed, Goodsprings, a short walk off the marker,
    the position noted, then the console's `save` and a few seconds for the file to be written."""
    if not start_new_game(d, hold_first_room=0.0):
        return
    free_player(d)
    at_marker(d, "1658B7")
    warm_first_exterior(d)
    ensure_walks(d)
    d.walk(1.5)
    d.settle_play(quiet=2.0, timeout=30)
    note_state(d, "saved")
    d.console_open()
    d.console_line(f"save {ROUND_TRIP_SAVE}", 3.0)
    d.console_close(quiet=1.0)
    d.wait(3.0)
    d.note("saved")


def continue_game(d, opts):
    """The second half: a new session, the start-up movies skipped, Continue on the main menu (it loads the newest
    save), the play screen, and the position noted."""
    if not d.until(lambda: bool(d.tail()), 90, what="the first frame"):
        return
    for _ in range(4):
        d.wait(2.0)
        d.tap("esc", 0.1)
    d.wait(6.0)
    # Continue is highlighted when the menu opens: no key before Enter (a Down would move to New).
    d.tap("enter", 0.3)  # Continue: "Continue from your last saved game?"
    d.wait(1.5)
    d.tap("enter", 0.3)  # Yes: the newest save loads
    # The load of a save shows no loading screen in the frame log, and a box comes with it: wait for the player to be
    # placed (the position leaves the start-up value), then dismiss the boxes.
    placed = lambda: (d.position() or (2048.0, 2048.0, 128.0))[:2] != (2048.0, 2048.0)
    if not d.until(placed, 180, what="the loaded game"):
        return
    d.settle_play(quiet=3.0, timeout=60)
    note_state(d, "loaded")
    d.wait(2.0)


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
    "sneak": session(["goodsprings"], sneak_at=("goodsprings",)),
    "quick": session(["doc_house", "goodsprings", "strip"], measure=0.5, quiet=3.0, brisk=True),
    "combat": combat,
    "save": save_game,
    "continue": continue_game,
    "soak": soak,
    "views": views,
}

