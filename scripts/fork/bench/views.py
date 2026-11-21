# SPDX-License-Identifier: GPL-3.0-only
"""The `views` scenario's data: the view list, its parsing, the manifest and the comparison of two runs (nv-rs#343).

A view is a place, a heading, a pitch and a name: the player is put at (x, y, z) in the Mojave Wasteland, turned to
`heading` degrees (the game's compass: 0 is north, +y, and 90 is east, +x; set by the console's `player.setangle z`, so
it is exact and not read back) and `pitch` degrees (`player.setangle x`, positive looks down), and one frame is captured.
`z` is the height the player STANDS at: the driver loads the cells around the place with the game's own centre-on-world
load (`cow`), drops the player a few units onto that height and takes the picture only once the player has stood
there still for STAND_S seconds of play (nv-rs#TBD). The driver (drivers.views) sets the game hour, stops the clock and
can force a weather before the first view, so every run shows the same sky. Pictures and the manifest are written in the
run directory; no game data is in them but the pictures themselves, which never go in a repository.
"""
import json
import math
import re
from dataclasses import dataclass

NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]*$")
MANIFEST = "views.json"
DIR = "views"  # the pictures: <run directory>/views/<name>.png
HOUR = 12.0
# `set gamehour` stops the port (an unmapped write at 4 in the sky's update), so the driver runs the clock on from the
# new game's start (night: about 22:30, read from pictures taken at known lapses) at CLOCK_SCALE game seconds a second until the hour
# asked for is reached; CLOCK_LATENCY_S is the time the console's open takes while the clock still runs.
START_HOUR = 22.5
CLOCK_SCALE = 600.0
CLOCK_LATENCY_S = 0.15
# The base game's clear daytime weather (NVWastelandClear, form id 0FFC88; FalloutNV.esm): the one the long views are
# taken in, so they compare. "none" in --weather leaves the game's own weather (the manifest then says so).
WEATHER = "0FFC88"
SETTLE_S = 3.0  # seconds of play after the centre-on-world load before the player is placed in it
VANITY_DELAY = 1000000.0  # fVanityModeAutoDelay: the game turns the camera round an idle player after a few seconds; this stops it
STAND_S = 3.0  # seconds of play the player must have stood still, at the view's height, before its picture
STAND_Z_UNITS = 2.0  # "at the view's height": within this many units of the view's z (the player stands on the ground)
STAND_STILL_UNITS = 0.3  # "still": the position moved by no more than this over the stand window
DROP_UNITS = 40.0  # the player is put this far above the standing height and falls onto it (god mode: no damage); a few units are no fall: it stands where it is put
LOAD_TIMEOUT_S = 120.0  # the centre-on-world load and the stand together: a view that did not stand by then FAILS (no picture)
POSITION_UNITS = 1.0  # two runs agree at a view within this many units in x and y (the issue's acceptance) ...
HEIGHT_UNITS = 2.0  # ... and in z (the standing height)
HEADING_DEGREES = 1.0  # ... and this many degrees, in heading and in pitch


@dataclass(frozen=True)
class View:
    name: str
    x: float
    y: float
    z: float
    heading: float
    pitch: float = 0.0

    def as_dict(self):
        return {"name": self.name, "x": self.x, "y": self.y, "z": self.z, "heading": self.heading, "pitch": self.pitch}

    def cell(self):
        """The exterior cell grid coordinates (4096 units a cell) of the place: what `cow` centres the load on."""
        return math.floor(self.x / 4096.0), math.floor(self.y / 4096.0)


# Mojave Wasteland places (the base game's map markers, numbers only), each with the height the player STANDS at there, found
# by loading the cells with `cow` and reading where the player rests, and the heading that looks at the far land or water the
# view is named for. A place in water would not do: the player floats and the height bobs.
DEFAULT_VIEWS = [
    View("goodsprings-valley", -70200.0, 2560.0, 8354.8, 90.0),  # from the east edge of Goodsprings east over the valley
    View("vegas-from-the-south", -9320.0, 85176.0, 4368.4, 0.0),  # NCR Sharecropper Farms, north toward Vegas
    View("lake-mead", 53799.8, 38375.3, 2916.7, 60.0),  # Boulder Beach, northeast over the lake
    View("colorado-river", 62699.0, -97604.0, 17.5, 90.0),  # Cottonwood Overlook, east across the river
    View("black-mountain-west", 3171.0, 11634.0, 10000.0, 270.0),  # Black Mountain, west over the valley toward Goodsprings
]


class ViewError(ValueError):
    pass


def number(text, what):
    try:
        return float(text)
    except ValueError:
        raise ViewError(f"{what} is not a number: {text!r}") from None


def parse_view(spec):
    """`name:x:y:z:heading[:pitch]` (also `name,x,y,z,heading,pitch` or blanks between the fields). z is the height the
    player stands at; the pitch is in degrees, positive looks down, and 0 when left out."""
    parts = [p for p in re.split(r"[:,\s]+", spec.strip()) if p]
    if len(parts) not in (5, 6):
        raise ViewError(f"a view is name:x:y:z:heading[:pitch], got {spec!r}")
    name = parts[0]
    if not NAME.match(name):
        raise ViewError(f"view name {name!r}: letters, digits, - and _ only (it is the picture's file name)")
    x, y, z, heading = (number(p, f"{name}: {w}") for p, w in zip(parts[1:5], ("x", "y", "z", "heading")))
    pitch = number(parts[5], f"{name}: pitch") if len(parts) == 6 else 0.0
    if not -80.0 <= pitch <= 80.0:
        raise ViewError(f"{name}: pitch {pitch:g} is outside -80 to 80 degrees")
    return View(name, x, y, z, heading % 360.0, pitch)


def parse_views(text):
    """A view file's text: one view a line (`name x y z heading [pitch]`), blank lines and `#` comments ignored; or a `;` list."""
    views = []
    for line in text.replace(";", "\n").splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            views.append(parse_view(line))
    return unique(views)


def unique(views):
    if not views:
        raise ViewError("no views")
    names = [v.name for v in views]
    dup = sorted({n for n in names if names.count(n) > 1})
    if dup:
        raise ViewError(f"view names must differ: {', '.join(dup)} twice")
    return list(views)


def resolve(spec=None, path=None):
    """The view list of a run: `--views` text, else `--views-file`, else the default list."""
    if spec and path:
        raise ViewError("--views and --views-file together: give one")
    if spec:
        return parse_views(spec)
    if path:
        try:
            with open(path, encoding="utf-8") as f:
                return parse_views(f.read())
        except OSError as e:
            raise ViewError(f"cannot read the view file: {e}") from None
    return list(DEFAULT_VIEWS)


def parse_hour(text):
    """The game hour, or None for `none` (the hour is left as the game has it)."""
    if (text or "").strip().lower() == "none":
        return None
    h = number(text, "--hour")
    if not 0.0 <= h < 24.0:
        raise ViewError(f"--hour {text}: 0 up to 24")
    return h


def parse_timescale(text):
    """The game's time scale (game seconds per second; 0 stops the clock), or None for `none` (left as it is)."""
    if (text or "").strip().lower() == "none":
        return None
    t = number(text, "--timescale")
    if t < 0:
        raise ViewError(f"--timescale {text}: 0 or more")
    return t


def parse_weather(text):
    """A weather form id in hex (`0FFC88`), or `none` for the game's own weather: returns the id text or None."""
    t = (text or "").strip()
    if t.lower() in ("", "none", "game"):
        return None
    if not re.fullmatch(r"[0-9A-Fa-f]{1,8}", t):
        raise ViewError(f"--weather {text!r}: a weather form id in hex, or none")
    return t.upper()


def console_lines(view):
    """The console lines that put the player at a view, DROP_UNITS above the standing height (it falls onto it), turned to
    the heading and the pitch."""
    return [f"player.setpos x {view.x:g}", f"player.setpos y {view.y:g}", f"player.setpos z {view.z + DROP_UNITS:g}", f"player.setangle z {view.heading:g}", f"player.setangle x {view.pitch:g}"]


def standing(rows, view, units_z=None):
    """Whether the player has STOOD at the view for STAND_S seconds of play, from the end of the frame rows (frame, screen,
    x, y, z, frame delta): `(ok, why, stats)`. Standing is: every frame of the window on the play screen, no frame longer
    than a tenth of a second (a load or a stream), the position inside STAND_STILL_UNITS over the whole window (not
    falling, not moving, not pushed), the height within STAND_Z_UNITS of the view's z, and x and y within POSITION_UNITS of
    the view. The window is the last STAND_S seconds of the rows' frame deltas; with fewer rows than that it is not ok."""
    units_z = STAND_Z_UNITS if units_z is None else units_z
    window, total = [], 0.0
    for r in reversed(rows):
        window.append(r)
        total += r[5]
        if total >= STAND_S:
            break
    stats = {"window_s": round(total, 2), "frames": len(window)}
    if total < STAND_S:
        return False, f"only {total:.1f} s of frames to look at (need {STAND_S:g})", stats
    if any(r[1] != "play" for r in window):
        return False, "not on the play screen for the whole window", stats
    longest = max(r[5] for r in window)
    stats["longest_frame_s"] = round(longest, 3)
    if longest > 0.1:
        return False, f"a frame of {longest:.2f} s in the window (a load or a stream)", stats
    xs, ys, zs = [r[2] for r in window], [r[3] for r in window], [r[4] for r in window]
    zr, xyr = max(zs) - min(zs), max(max(xs) - min(xs), max(ys) - min(ys))
    stats.update(z_range=round(zr, 3), xy_range=round(xyr, 3))
    if zr > STAND_STILL_UNITS or xyr > STAND_STILL_UNITS:
        return False, f"the player is moving (z moved {zr:.2f}, x/y {xyr:.2f} units over {total:.1f} s)", stats
    x, y, z = window[0][2:5]
    dz, dxy = z - view.z, math.hypot(x - view.x, y - view.y)
    stats.update(dz=round(dz, 3), dxy=round(dxy, 3))
    if abs(dz) > units_z:
        return False, f"the player stands {dz:+.1f} units from the view's height {view.z:g} (limit {units_z:g})", stats
    if dxy > POSITION_UNITS:
        return False, f"the player stands {dxy:.1f} units from the view's place (limit {POSITION_UNITS:g})", stats
    return True, "standing", stats


def manifest(views, hour, weather, settle, captures, resolution=None, build=None, timescale=0, console=None):
    """The run's manifest: the setting of the sky and the camera and, per view, what was commanded and what the frame log
    showed. `captures` maps a view name to {"observed": [x, y, z] or None, "file": name or None, "frame": the frame of the
    picture, "stance": standing() stats, "failed": why the view was not captured}."""
    rows = []
    for v in views:
        c = captures.get(v.name) or {}
        rows.append(dict(v.as_dict(), observed=c.get("observed"), file=c.get("file"), captured=bool(c.get("file")), frame=c.get("frame"),
                         stance=c.get("stance"), failed=c.get("failed")))
    return {
        "scenario": "views",
        "hour": hour,
        "hour_note": (f"reached by running the clock on from the new game's start hour ({START_HOUR:g}) at {CLOCK_SCALE:g}x; then the clock stopped" if hour is not None else "not set: the new game's own hour"),
        "timescale": timescale,
        "weather": weather,
        "weather_note": f"forced with fw {weather}" if weather else "not set: the game's own weather at each place (not fixed)",
        "settle_s": settle,
        "stand_s": STAND_S,
        "camera_note": f"first person, no idle camera: fVanityModeAutoDelay set to {VANITY_DELAY:.0f} s; the sneak key never reaches the game",
        "interface_note": "hidden with the console's tm",
        "console": list(console or []),
        "console_note": "console lines run once before the first view (a variant of the run)" if console else "none",
        "placement_note": "cells loaded with the game's own `cow WastelandNV <cell x> <cell y>` (a load screen), then player.setpos and player.setangle x/z; the heading and pitch are commanded, not read back",
        "heading_note": "commanded with player.setangle z (0 is north, 90 is east); the frame log has no heading column",
        "resolution": resolution,
        "build": build,
        "views": rows,
    }


def write_manifest(run_dir, data):
    with open(f"{run_dir}/{MANIFEST}", "w", encoding="utf-8") as f:
        json.dump(data, f, indent=1)
        f.write("\n")


def angle_difference(a, b):
    """The smallest difference of two headings in degrees (0 to 180)."""
    d = abs(a - b) % 360.0
    return min(d, 360.0 - d)


def compare(first, second):
    """Problems between two manifests of the same view list, as text lines; none when the runs agree at every view:
    the same views and sky, positions within POSITION_UNITS in x and y and HEIGHT_UNITS in z (the observed ones when both have them, else
    the commanded) and headings within HEADING_DEGREES."""
    out = []
    for key in ("hour", "weather", "timescale"):
        if first.get(key) != second.get(key):
            out.append(f"{key}: {first.get(key)!r} against {second.get(key)!r}")
    a = {v["name"]: v for v in first.get("views", [])}
    b = {v["name"]: v for v in second.get("views", [])}
    for name in sorted(set(a) ^ set(b)):
        out.append(f"{name}: only in one of the runs")
    for name in sorted(set(a) & set(b)):
        va, vb = a[name], b[name]
        if not (va.get("captured") and vb.get("captured")):
            out.append(f"{name}: not captured in {'both runs' if not (va.get('captured') or vb.get('captured')) else 'one run'}")
            continue
        pa = va.get("observed") or [va["x"], va["y"], va["z"]]
        pb = vb.get("observed") or [vb["x"], vb["y"], vb["z"]]
        dist = ((pa[0] - pb[0]) ** 2 + (pa[1] - pb[1]) ** 2) ** 0.5
        if dist > POSITION_UNITS:
            out.append(f"{name}: positions differ by {dist:.2f} units (limit {POSITION_UNITS:g})")
        if abs(pa[2] - pb[2]) > HEIGHT_UNITS:
            out.append(f"{name}: heights differ by {abs(pa[2] - pb[2]):.1f} units (limit {HEIGHT_UNITS:g})")
        turn = angle_difference(va["heading"], vb["heading"])
        if turn > HEADING_DEGREES:
            out.append(f"{name}: headings differ by {turn:.2f} degrees (limit {HEADING_DEGREES:g})")
        if abs(va.get("pitch", 0.0) - vb.get("pitch", 0.0)) > HEADING_DEGREES:
            out.append(f"{name}: pitches differ by {abs(va.get('pitch', 0.0) - vb.get('pitch', 0.0)):.2f} degrees (limit {HEADING_DEGREES:g})")
        for key in ("pose", "sneaking", "moving", "falling", "camera", "weapon"):
            sa, sb = (va.get("stance") or {}).get(key), (vb.get("stance") or {}).get(key)
            if sa != sb:
                out.append(f"{name}: stance {key} {sa!r} against {sb!r}")
    return out


def stance_record(stats):
    """What the manifest says about the player at the picture, once standing() was ok: the pose the driver proved (it
    stood still at the view's height for the whole window) and the ones it did not allow (a sneak key never reaches the
    game in this scenario, nothing is drawn, the camera is the first person one), with the measured window."""
    return dict(stats, pose="standing", sneaking=False, moving=False, falling=False, camera="first person", weapon="none drawn")


def read_png(path):
    """(width, height, bytes per pixel, rows as bytearrays) of an 8-bit, non-interlaced PNG (grim's), pure Python."""
    import struct
    import zlib

    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ViewError(f"{path}: not a PNG")
    pos, idat, ihdr = 8, [], None
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        pos += 12 + n
        if kind == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    if ihdr is None or ihdr[2] != 8 or ihdr[3] not in (2, 6) or ihdr[6] != 0:
        raise ViewError(f"{path}: only 8-bit RGB or RGBA PNGs that are not interlaced are read")
    w, h, bpp = ihdr[0], ihdr[1], 3 if ihdr[3] == 2 else 4
    raw = zlib.decompress(b"".join(idat))
    stride, rows, prev = w * bpp, [], bytearray(w * bpp)
    for y in range(h):
        ft, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        if ft == 1:
            for i in range(bpp, stride):
                line[i] = (line[i] + line[i - bpp]) & 255
        elif ft == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 255
        elif ft == 3:
            for i in range(stride):
                line[i] = (line[i] + (((line[i - bpp] if i >= bpp else 0) + prev[i]) >> 1)) & 255
        elif ft == 4:
            for i in range(stride):
                a, b, c = (line[i - bpp] if i >= bpp else 0), prev[i], (prev[i - bpp] if i >= bpp else 0)
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        elif ft != 0:
            raise ViewError(f"{path}: PNG filter {ft}")
        rows.append(line)
        prev = line
    return w, h, bpp, rows


def picture_difference(path_a, path_b):
    """(mean, maximum) absolute difference per colour channel value (0 to 255) between two pictures of the same size, over
    the red, green and blue values of every pixel; (None, None) with a note in the third place when the sizes differ."""
    wa, ha, ba, ra = read_png(path_a)
    wb, hb, bb, rb = read_png(path_b)
    if (wa, ha) != (wb, hb):
        return None, None, f"sizes differ: {wa}x{ha} against {wb}x{hb}"
    total, peak, count = 0, 0, 0
    for la, lb in zip(ra, rb):
        if ba == 4:
            la = bytes(v for i, v in enumerate(la) if i % 4 != 3)
        if bb == 4:
            lb = bytes(v for i, v in enumerate(lb) if i % 4 != 3)
        diff = [abs(x - y) for x, y in zip(la, lb)]
        total += sum(diff)
        peak = max(peak, max(diff))
        count += len(diff)
    return total / count, peak, ""


def observed_from_states(states, views):
    """Per view the position the driver noted (`state view.NAME x= y= z=`, parse.parse_log's `states`)."""
    return {v.name: states.get(f"view.{v.name}") for v in views}
