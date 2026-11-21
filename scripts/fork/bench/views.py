# SPDX-License-Identifier: GPL-3.0-only
"""The `views` scenario's data: the view list, its parsing, the manifest and the comparison of two runs (nv-rs#343).

A view is a place, a heading and a name: the player is put at (x, y, z) in the Mojave Wasteland, turned to `heading`
degrees (the game's compass: 0 is north, +y, and 90 is east, +x; set by the console's `player.setangle z`, so it is
exact and not read back), and one frame is captured. The driver (drivers.views) sets the game hour, stops the clock and
can force a weather before the first view, so every run shows the same sky. Pictures and the manifest are written in the
run directory; no game data is in them but the pictures themselves, which never go in a repository.
"""
import json
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
SETTLE_S = 15.0  # seconds of play after the load and the frame-time check before the picture
POSITION_UNITS = 1.0  # two runs agree at a view within this many units in x and y (the issue's acceptance) ...
HEIGHT_UNITS = 30.0  # ... and in z, which is where the player landed: a slope or a prop under the landing moves it by a few units
HEADING_DEGREES = 1.0  # ... and this many degrees


@dataclass(frozen=True)
class View:
    name: str
    x: float
    y: float
    z: float
    heading: float

    def as_dict(self):
        return {"name": self.name, "x": self.x, "y": self.y, "z": self.z, "heading": self.heading}


# Mojave Wasteland positions, taken from the base game's map markers (FalloutNV.esm, numbers only), with the height a
# little over the ground: the player falls onto it (god mode, so no damage), and the picture is taken once it stands.
DEFAULT_VIEWS = [
    View("goodsprings-valley", -70200.0, 2560.0, 8500.0, 90.0),  # from the east edge of Goodsprings east over the valley
    View("vegas-from-the-south", -9320.0, 85176.0, 5000.0, 0.0),  # NCR Sharecropper Farms, north toward Vegas
    View("lake-mead", 53808.0, 38384.0, 3850.0, 60.0),  # Boulder Beach, northeast over the lake
    View("colorado-river", 62699.0, -97604.0, 600.0, 90.0),  # Cottonwood Overlook, east across the river
]


class ViewError(ValueError):
    pass


def number(text, what):
    try:
        return float(text)
    except ValueError:
        raise ViewError(f"{what} is not a number: {text!r}") from None


def parse_view(spec):
    """`name:x:y:z:heading` (also `name,x,y,z,heading` or blanks between the fields)."""
    parts = [p for p in re.split(r"[:,\s]+", spec.strip()) if p]
    if len(parts) != 5:
        raise ViewError(f"a view is name:x:y:z:heading, got {spec!r}")
    name = parts[0]
    if not NAME.match(name):
        raise ViewError(f"view name {name!r}: letters, digits, - and _ only (it is the picture's file name)")
    x, y, z, heading = (number(p, f"{name}: {w}") for p, w in zip(parts[1:], ("x", "y", "z", "heading")))
    return View(name, x, y, z, heading % 360.0)


def parse_views(text):
    """A view file's text: one view a line (`name x y z heading`), blank lines and `#` comments ignored; or a `;` list."""
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
    """The console lines that put the player at a view."""
    return [f"player.setpos x {view.x:g}", f"player.setpos y {view.y:g}", f"player.setpos z {view.z:g}", f"player.setangle z {view.heading:g}"]


def manifest(views, hour, weather, settle, captures, resolution=None, build=None, timescale=0):
    """The run's manifest: the setting of the sky and, per view, what was commanded and what the frame log showed.
    `captures` maps a view name to {"observed": [x, y, z] or None, "file": name or None}."""
    rows = []
    for v in views:
        c = captures.get(v.name) or {}
        rows.append(dict(v.as_dict(), observed=c.get("observed"), file=c.get("file"), captured=bool(c.get("file"))))
    return {
        "scenario": "views",
        "hour": hour,
        "hour_note": (f"reached by running the clock on from the new game's start hour ({START_HOUR:g}) at {CLOCK_SCALE:g}x; then the clock stopped" if hour is not None else "not set: the new game's own hour"),
        "timescale": timescale,
        "weather": weather,
        "weather_note": f"forced with fw {weather}" if weather else "not set: the game's own weather at each place (not fixed)",
        "settle_s": settle,
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
    return out


def observed_from_states(states, views):
    """Per view the position the driver noted (`state view.NAME x= y= z=`, parse.parse_log's `states`)."""
    return {v.name: states.get(f"view.{v.name}") for v in views}
