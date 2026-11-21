# SPDX-License-Identifier: GPL-3.0-only
"""Reading the highlight of the game's menus off a picture of the nested screen, for the bench driver's menu walk.

The main menu and its boxes mark the selected line with a two pixel outline in one colour (a rectangle around it), and draw
no outline while the keyboard has not yet been given the menu (the first arrow key only wakes it: nv-rs#265). So the
picture says where the selection is, which the game's menu stack (`menu.stack`) does not: the main menu's own boxes ("Start
a new game?", "Continue from your last saved game?") are not on it. Nothing here keeps or echoes the picture: the numbers
are the outlines' corners. Stdlib only, so it runs in CI on synthetic pictures.
"""
import subprocess

# The outline colour of a selected line (the same in the main menu and in a box), 8 bits per channel.
OUTLINE = (200, 143, 52)
# The frame of a message box with several choices (the start choice), a brighter orange.
FRAME = (255, 182, 66)
MIN_RUN = 40  # the shortest horizontal stretch (pixels, at 1080 lines) taken for an edge of an outline
# The main menu's lines at 1080 lines of screen: Continue, New, Load, Settings, Credits, Downloadable Content, Quit; the
# centre of the first and the step between two. The outline of a menu line is a wide rectangle at the right edge, a box's
# button outline is narrow.
FIRST_LINE_Y, LINE_STEP = 377.0, 54.0
MENU_LINE_MIN_WIDTH = 0.15  # of the screen's width
LINES = ("continue", "new", "load", "settings", "credits", "downloadable content", "quit")


def grab(grim, env, timeout=15.0):
    """The nested screen as (width, height, rgb bytes) through grim's PPM output; None when grim fails."""
    try:
        r = subprocess.run([grim, "-t", "ppm", "-"], env=env, capture_output=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return parse_ppm(r.stdout) if r.returncode == 0 else None


def parse_ppm(data):
    """(width, height, bytes) of a binary PPM (P6, 8 bits); None when it is not one."""
    parts = data.split(b"\n", 3)
    if len(parts) < 4 or parts[0] != b"P6":
        return None
    try:
        w, h = (int(v) for v in parts[1].split())
    except ValueError:
        return None
    return (w, h, parts[3]) if len(parts[3]) >= w * h * 3 else None


def _runs(row, w, pattern, color):
    """(start, end) pixel spans of at least len(pattern)//3 outline pixels in a row of w pixels."""
    out, at = [], 0
    n = len(pattern) // 3
    while True:
        i = row.find(pattern, at)
        if i < 0:
            return out
        if i % 3:
            at = i + 1
            continue
        start = i // 3
        end = start + n
        while end < w and tuple(row[end * 3 : end * 3 + 3]) == color:
            end += 1
        out.append((start, end))
        at = end * 3


def _merged(spans, gap):
    """Spans (start, end) in order, with the ones less than `gap` pixels apart joined: the mouse cursor over an edge cuts it."""
    out = []
    for x0, x1 in spans:
        if out and x0 - out[-1][1] < gap:
            out[-1][1] = x1
        else:
            out.append([x0, x1])
    return out


def outlines(picture, color=OUTLINE, max_height=120):
    """The outline rectangles in a picture, as (x0, y0, x1, y1) with x1 and y1 one past the edge, widest first. An outline is
    a top edge and a bottom edge of the same span (a few pixels off where the mouse cursor lies on one), 20 to 120 lines
    (at 1080; `max_height` of them) apart; each edge is two lines thick."""
    w, h, px = picture
    scale = h / 1080.0
    pattern = bytes(color) * max(8, int(MIN_RUN * scale))
    gap = 0.025 * w
    edges = []  # [first line, x0, x1] of each edge: runs on consecutive lines with overlapping spans
    open_edges = []
    for y in range(h):
        row = _merged(_runs(px[y * w * 3 : (y + 1) * w * 3], w, pattern, color), gap)
        nxt = []
        for x0, x1 in row:
            for e in open_edges:
                if e[3] == y - 1 and x0 < e[2] and x1 > e[1]:
                    e[3] = y
                    e[1], e[2] = min(e[1], x0), max(e[2], x1)
                    nxt.append(e)
                    break
            else:
                e = [y, x0, x1, y]
                edges.append(e)
                nxt.append(e)
        open_edges = nxt
    tol = 0.01 * w
    found = []
    for t in edges:
        for b in edges:
            if 20 * scale <= b[0] - t[0] <= max_height * scale and abs(t[1] - b[1]) <= tol and abs(t[2] - b[2]) <= tol:
                found.append((min(t[1], b[1]), t[0], max(t[2], b[2]), b[3] + 1))
                break
    return sorted(found, key=lambda r: r[0] - r[2])


def main_menu_line(picture):
    """The index in LINES of the main menu's selected line, None when no line has the outline."""
    w, h, _ = picture
    for x0, y0, x1, y1 in outlines(picture):
        if (x1 - x0) >= MENU_LINE_MIN_WIDTH * w and x0 > 0.5 * w:
            i = round(((y0 + y1) / 2 * 1080.0 / h - FIRST_LINE_Y) / LINE_STEP)
            return i if 0 <= i < len(LINES) else None
    return None


def box_buttons(picture):
    """The outlines of a box's buttons: the narrow ones, (x0, y0, x1, y1), top first."""
    w, _, _ = picture
    return sorted((r for r in outlines(picture) if (r[2] - r[0]) < MENU_LINE_MIN_WIDTH * w), key=lambda r: r[1])


def choice_line(picture):
    """"first" or "second": which of a two-line choice box's lines is selected, read from where the selection's outline lies
    in the box's frame (below the frame's middle is the second line); None when there is no outline or no frame."""
    marks, frames = outlines(picture), outlines(picture, FRAME, 400)
    if not marks or not frames:
        return None
    mark, frame = marks[0], frames[0]
    return "second" if (mark[1] + mark[3]) > (frame[1] + frame[3]) else "first"
