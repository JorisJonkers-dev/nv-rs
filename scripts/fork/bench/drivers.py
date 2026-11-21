"""Scenario drivers: the scenarios of scenarios.py as closed-loop, wall-clock programs for driver.py, valid at any frame
rate (docs/fork/BENCH.md). Each takes a Driver and an options dict (`scale` shortens or lengthens the walking)."""
import os

import menu_view
import views as viewlib

# Form ids of the base game (numbers only; scenarios.PLACES and friends hold the same).
OPENING_QUEST = "104c1c"
OPENING_QUEST_NAME = "VCG01"  # its editor id, for its script's variables
STAGES_FREE = (50, 55, 200)  # 50 removes the sitting package, 55 gives movement and looking back, 200 ends the quest
ROUTE_TOWN = [(10.0, 0.0), (4.0, 14.0)] * 4  # (seconds forward, turn counts per 10 ms in the first second): a loop


def laps(opts, n=3):
    return max(1, round(n * (opts.get("scale") or 1.0)))


# The class ids the game's menu stack holds (docs/fork/LOGGING.md, `menu.stack`): the main menu (over the background
# menu 0x3ef) and a message box. The walk below reads them from the viewer log.
MAIN_MENU, MESSAGE_BOX = 0x3F5, 0x3E9
# The Tale of Two Wastelands start choice: a message box with "Start in Capital Wasteland (Recommended)" and "Start in
# Mojave Wasteland" that comes once the new game has begun (nv-rs#265). The walks of the bench are the Mojave's.
INTRO_FILMS = {"mojave": "FNVIntro", "capital": "Fallout INTRO Vsk"}


FILM_WAIT = 12.0  # seconds after the choice that a film is waited for to open
START_CHOICE_WAIT = 90.0  # seconds after the new game began that the start choice box is waited for


def ttw_start(d):
    """"mojave" or "capital" when the run is on a Tale of Two Wastelands game folder (its Data has TaleOfTwoWastelands.esm,
    any case) and the new game therefore opens with the start choice; None otherwise. NV_BENCH_TTW_START picks the line
    (mojave is the default: the walks of the bench are Mojave's), or "off" turns the choice handling off."""
    want = (os.environ.get("NV_BENCH_TTW_START") or "").lower()
    if want == "off":
        return None
    if want not in ("mojave", "capital"):
        data = os.path.join(d.game_dir or "", "Data")
        try:
            if not any(n.lower() == "taleoftwowastelands.esm" for n in os.listdir(data)):
                return None
        except OSError:
            return None
        want = "mojave"
    return want


def menu_top(d):
    """The menu class on top of the game's menu stack: None with no trace or no menu open."""
    stack = d.menu_stack()
    return stack[-1] if stack else None


def new_game_begun(d, before):
    return d.count_events("load.kind", "new game") > before


NEW = menu_view.LINES.index("new")


def new_game_from_menu(d, step=1.5, tries=14, box_wait=20.0):
    """The main menu to a new game, by what the menu shows and not by a fixed count of keys (nv-rs#265). A Profile with a
    save opens on Continue; one without opens with nothing selected, and the first arrow key only wakes the keyboard, so a
    fixed "Down, Enter" lands on Continue and its box. Each round takes a picture (menu_view) and does one thing: a message
    box of the game (on the menu stack: "No valid save game found") gets Enter; a box of the main menu that the walk did not
    open (it asked for Continue or Load by mistake) gets Escape; the box it did open ("Start a new game?", with Yes
    selected) gets Enter; no outline anywhere is a menu waiting for its first key, so a Down; a selected line above New gets
    Down, below New Up, on New Enter. Done when the game traces `load.kind: the load is a new game`. False after `tries`
    rounds, or when there is no picture to read."""
    before = d.count_events("load.kind", "new game")
    opened = False  # the box on the screen is the one that Enter on New opened
    for n in range(tries):
        if new_game_begun(d, before):
            return True
        if menu_top(d) == MESSAGE_BOX:
            d.note(f"menu walk round {n + 1}: a message box, Enter")
            d.tap("enter", 0.3)
            d.wait(step)
            continue
        pic = d.snapshot()
        if pic is None:
            d.note("menu walk: no picture of the screen")
            return False
        box, line = menu_view.box_buttons(pic), menu_view.main_menu_line(pic)
        if box:
            if opened:
                d.note(f"menu walk round {n + 1}: Yes of the new game box")
                d.tap("enter", 0.3)
                opened = False
                if d.until(lambda: new_game_begun(d, before), box_wait, every=0.25, what="the new game to begin after Yes"):
                    return True
                continue
            d.note(f"menu walk round {n + 1}: a box the walk did not open, Escape")
            d.tap("esc", 0.2)
        elif line is None:
            d.note(f"menu walk round {n + 1}: no line selected, Down to wake the menu")
            d.tap("down", 0.3)
        elif line == NEW:
            d.note(f"menu walk round {n + 1}: New selected, Enter")
            d.tap("enter", 0.3)
            opened = True
        else:
            d.note(f"menu walk round {n + 1}: {menu_view.LINES[line]} selected, {'Down' if line < NEW else 'Up'}")
            d.tap("down" if line < NEW else "up", 0.3)
            opened = False
        d.wait(step)
    return new_game_begun(d, before)


def answer_start_choice(d, want="mojave", tries=8):
    """The start choice box: its first line is Capital Wasteland (Recommended), its second Mojave Wasteland. Each round
    takes a picture and reads which line is selected (menu_view.choice_line: where the selection's outline lies in the box's
    frame); Enter goes in only on the wanted line, otherwise a Down or Up goes toward it, and with no outline yet (the
    first key only wakes the box) a Down wakes it. True once Enter went in on the wanted line, False after `tries` rounds
    without the wanted line proven or with no picture."""
    target = "first" if want == "capital" else "second"
    for n in range(tries):
        pic = d.snapshot()
        if pic is None:
            d.note("start choice: no picture of the screen")
            return False
        line = menu_view.choice_line(pic)
        d.note(f"start choice round {n + 1}: selected line {line}")
        if line == target:
            d.tap("enter", 0.3)
            return True
        d.tap("up" if line == "second" else "down", 0.3)
        d.wait(1.0)
    d.note("start choice: the wanted line could not be proven")
    return False


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
    if d.menu_stack() is not None and d.snapshot() is not None:
        if not new_game_from_menu(d, step):
            return False
    else:
        # No game trace in this viewer: fixed keys. A menu with a save opens with Continue highlighted and the first Down
        # already moves the selection (nv-rs#410, lodwright-rs#156): one Down is Continue -> New, two land on Load.
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
    want = ttw_start(d)
    begun_at = d.clock()
    state = {"answered": False, "failed": False, "at": 0.0, "films": 0}

    def loaded():
        stack = d.menu_stack() or ()
        if want and not state["answered"]:
            if stack == (MESSAGE_BOX,):
                # A box over nothing after the new game began is the Tale of Two Wastelands start choice, which comes
                # a second or two after the player is placed: answered once, and never with an Enter meant for a box
                # of the opening or an Escape meant for a film.
                state["answered"], state["at"] = True, d.clock()
                state["films"] = d.count_events("movie.open")
                d.note(f"the start choice box: {want}")
                state["failed"] = not answer_start_choice(d, want)
                state["at"] = d.clock()
                return state["failed"]
            if d.clock() - begun_at > START_CHOICE_WAIT:
                d.note(f"no start choice box came within {START_CHOICE_WAIT:.0f} s")
                state["answered"], state["films"], state["at"] = True, d.count_events("movie.open"), -1e9
        elif stack and stack[-1] == MESSAGE_BOX:
            return placed() and not d.film_playing()
        if d.clock() - last[0] >= 2.0 and not (stack and stack[-1] == MESSAGE_BOX):
            last[0] = d.clock()
            d.tap("esc", 0.1)
        if not (placed() and (not want or state["answered"])):
            return False
        # The film that follows the choice opens a moment after the answer: it is skipped (the Escape above) and waited for.
        film_done = d.count_events("movie.open") > state["films"] or d.clock() - state["at"] > FILM_WAIT
        return not want or (film_done and not d.film_playing())

    if not d.until(loaded, 240, every=0.1, what="the new game's first cell"):
        return False
    if state["failed"]:
        return False
    if want and d.count_events("movie.open", INTRO_FILMS["capital" if want == "mojave" else "mojave"]):
        d.note(f"the {'Capital' if want == 'mojave' else 'Mojave'} Wasteland start was taken, not the {want}'s")
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


def fix_camera_and_interface(d):
    """Once, before the first view: the idle camera off and the interface hidden. The game turns the camera round an idle
    player after about nine seconds (its "vanity" camera: third person, every picture at another angle); its delay is the
    game setting fVanityModeAutoDelay, set far beyond a run's length (fVanityModeDelay is another setting and does nothing
    for it). `tm` hides the interface (toggles it: called once)."""
    d.console_open()
    d.console_line(f"setgs fVanityModeAutoDelay {viewlib.VANITY_DELAY:.0f}", 0.8)
    d.console_line("tm", 0.8)
    return d.console_close(quiet=1.0, timeout=60)


def last_cell_load(d):
    """The frame of the last `load.kind: the load is a door or another cell change` event of the game trace (-1: none)."""
    frames = [f for f, kind, text in d.game_events() if kind == "load.kind" and "cell change" in text]
    return max(frames) if frames else -1


def wait_load(d, what, since, timeout=15.0):
    """The load of a move: the game traces `load.kind: ... another cell change` when it takes place (the frame log shows its
    loading screen for one row or none: it is one long frame), so it is seen by an event newer than `since` (last_cell_load
    before the move). A centre-on-world onto the cell the player is in loads nothing: no event within `timeout` is noted, not
    failed; the cells are loaded already and the standing condition that follows is the one that decides."""
    if d.until(lambda: last_cell_load(d) > since, timeout, every=0.1, what=f"the load of {what}"):
        return True
    d.note(f"no cell load was traced for {what}: the cells were loaded already")
    return False


def wait_standing(d, view, timeout):
    """Until the player has stood still at the view's height and place for viewlib.STAND_S seconds of play (viewlib.standing);
    `(ok, why, stats)` with the last reason on a timeout (the view then FAILS: no picture is taken anyway)."""
    result = [False, "no frames", {}]

    def ok():
        result[:] = viewlib.standing(d.tail(262144), view)
        return result[0]

    d.until(ok, timeout, every=0.25, what=f"the player to stand at {view.name}")
    return tuple(result)


def place_view(d, view, weather=None, timeout=viewlib.LOAD_TIMEOUT_S):
    """Put the player at a view and wait until it stands there: the cells around the place are loaded with the game's own
    centre-on-world load (`cow WastelandNV x y`: a loading screen, the cells' land, objects and collision, as a fast travel
    loads them), the weather is forced again (a new region brings its own: a dust storm came at the Colorado), then the
    player is put DROP_UNITS above the view's standing height, turned to its heading and pitch, and falls onto it.
    `(ok, why, stats)`: ok only once the player has stood at the view's height, still, for viewlib.STAND_S seconds of play,
    inside `timeout` seconds. The console is not closed after the `cow` line: the load closes it, and a key sent then
    would open it again."""
    end = d.clock() + timeout
    cx, cy = view.cell()
    since = last_cell_load(d)
    d.console_open()
    d.console_line(f"cow WastelandNV {cx} {cy}", 0.3)
    wait_load(d, f"the cells of {view.name}", since)
    d.settle_play(quiet=viewlib.SETTLE_S, timeout=60)
    d.console_open()
    if weather:
        for _ in range(2):
            d.console_line(f"fw {weather}", 0.4)
    for line in viewlib.console_lines(view):
        d.console_line(line, 0.4)
    d.console_close(quiet=0.5, timeout=60)
    return wait_standing(d, view, max(5.0, end - d.clock()))


def views(d, opts):
    """The views scenario (nv-rs#343, docs/fork/BENCH.md): a new game, the Mojave exterior, the sky fixed, the idle camera off
    and the interface hidden, then each view of the list in turn: its cells loaded, the player standing at its height with
    its heading and pitch, one picture (`views/<name>.png` in the run directory) only once the player has stood still for
    a set time, and the position, frame and stance noted (`state view.<name>`); `views.json` is the manifest, rewritten after
    every view. A view whose condition does not hold in time gets no picture and a `view-failed` line: the verdict fails."""
    plan = opts.get("views") or viewlib.DEFAULT_VIEWS
    hour = opts["hour"] if "hour" in opts else viewlib.HOUR
    timescale = opts["timescale"] if "timescale" in opts else 0
    weather = opts["weather"] if "weather" in opts else viewlib.WEATHER
    settle = opts.get("views_settle", viewlib.SETTLE_S)
    run_dir = os.path.dirname(d.frames)
    os.makedirs(os.path.join(run_dir, viewlib.DIR), exist_ok=True)
    captures = {}

    def write():
        viewlib.write_manifest(run_dir, viewlib.manifest(plan, hour, weather, settle, captures, resolution=opts.get("resolution"), timescale=timescale, console=opts.get("views_console")))

    write()
    if not start_new_game(d, hold_first_room=0.0):
        return
    if not at_marker(d, "1658B7"):  # Goodsprings: the way into the Mojave exterior
        d.note("the Mojave exterior was not reached")
        return
    warm_first_exterior(d)
    set_sky(d, hour, weather, timescale)
    fix_camera_and_interface(d)
    extra = opts.get("views_console") or []
    if extra:
        d.console_open()
        for line in extra:
            d.console_line(line, 1.5)
        d.console_close(quiet=1.0, timeout=60)
    for view in plan:
        if not d.alive():
            return
        phase(d, f"view.{view.name}", "start")
        ok, why, stats = place_view(d, view, weather)

        def fail(reason):
            captures[view.name] = {"failed": reason}
            d.note(f"view-failed {view.name}: {reason}")
            phase(d, f"view.{view.name}", "end")
            write()

        if not d.alive():  # the game stopped on the way: a picture now would be of nothing
            d.note(f"view-failed {view.name}: the game stopped before the picture")
            return
        if not ok:
            fail(why)
            continue
        name = f"{viewlib.DIR}/{view.name}.png"
        if not d.capture(os.path.join(run_dir, name)):
            fail("no picture could be taken")
            continue
        rows = d.tail(262144)
        again, why_again, _ = viewlib.standing(rows, view)  # still standing when the picture was taken
        if not again:
            os.remove(os.path.join(run_dir, name))
            fail(f"the player did not stay standing while the picture was taken: {why_again}")
            continue
        pos = d.position()
        captures[view.name] = {"observed": list(pos) if pos else None, "file": name, "frame": rows[-1][0], "stance": viewlib.stance_record(stats)}
        note_state(d, f"view.{view.name}")
        d.note(f"view {view.name} file={name} frame={rows[-1][0]}")
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

