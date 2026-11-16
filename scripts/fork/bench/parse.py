"""Parsing and statistics for one bench run: the frame log, the viewer log, the memory timeline.

Numbers only. Nothing here reads or echoes game content; the viewer's log lines it matches carry counters.
Stdlib only, so it runs in CI against fixtures with no game and no build.
"""
import math
import re

# ---------------------------------------------------------------------------------------------
# statistics


def percentile(sorted_values, q):
    """Nearest-rank percentile (q in 0..100) of an ascending list; None for an empty list."""
    if not sorted_values:
        return None
    k = max(1, math.ceil(q / 100.0 * len(sorted_values)))
    return sorted_values[min(k, len(sorted_values)) - 1]


def frame_stats(ms):
    """Frame-time distribution of a list of frame times in milliseconds.

    avg/p50/p95/p99/max in ms; fps = 1000/avg; low1 = average fps of the slowest 1% of frames ("1% lows"),
    low01 = the same for the slowest 0.1%.
    """
    if not ms:
        return {"frames": 0}
    s = sorted(ms)
    n = len(s)
    avg = sum(s) / n

    def low(frac):
        k = max(1, int(round(n * frac)))
        worst = s[-k:]
        return 1000.0 / (sum(worst) / len(worst)) if sum(worst) > 0 else None

    return {
        "frames": n,
        "avg_ms": avg,
        "p50_ms": percentile(s, 50),
        "p95_ms": percentile(s, 95),
        "p99_ms": percentile(s, 99),
        "max_ms": s[-1],
        "fps": 1000.0 / avg if avg > 0 else None,
        "low1_fps": low(0.01),
        "low01_fps": low(0.001),
    }


# ---------------------------------------------------------------------------------------------
# frame log (NV_FRAME_LOG): one tab separated line per main frame, a header first, `#` lines are notes


def parse_frame_log(text):
    """Rows of the frame log as dicts with t (seconds since the first frame), dt_ms, screen and position.

    The clock column `us` is the engine's microsecond clock; a frame's time is the difference to the
    frame before. Returns a list; the first frame has dt_ms None.
    """
    rows = []
    cols = None
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if cols is None:
            if parts[0] == "frame":
                cols = {name: i for i, name in enumerate(parts)}
            continue
        try:
            us = int(parts[cols["us"]])
            rows.append(
                {
                    "frame": int(parts[cols["frame"]]),
                    "us": us,
                    "screen": parts[cols["screen"]].strip().lower() if "screen" in cols else "",
                    "pos": [float(parts[cols[k]]) for k in ("px", "py", "pz")],
                    "fdelta": float(parts[cols["m_fdelta"]]) if "m_fdelta" in cols else None,
                }
            )
        except (KeyError, ValueError, IndexError):
            continue  # a line cut short by a kill
    t0 = rows[0]["us"] if rows else 0
    prev = None
    for r in rows:
        r["t"] = (r["us"] - t0) / 1e6
        r["dt_ms"] = None if prev is None else (r["us"] - prev) / 1e3
        prev = r["us"]
    return rows


def frame_summary(rows):
    """Distribution over all frames, per screen name, and the fps timeline (frames per second, 1 s buckets)."""
    # Loading-screen frames are not drawn frames (the main loop spins through them at about 1 ms each), so the
    # headline numbers and the fps timeline leave them out; by_screen keeps them under "loading".
    dts = [r["dt_ms"] for r in rows if r["dt_ms"] is not None and r["dt_ms"] >= 0 and r["screen"] != "loading"]
    by_screen = {}
    for r in rows:
        if r["dt_ms"] is not None and r["dt_ms"] >= 0:
            by_screen.setdefault(r["screen"] or "unknown", []).append(r["dt_ms"])
    buckets = {}
    for r in rows:
        if r["screen"] != "loading":
            buckets.setdefault(int(r["t"]), []).append(r)
    timeline = []
    for sec in sorted(buckets):
        rs = buckets[sec]
        d = sorted(x["dt_ms"] for x in rs if x["dt_ms"] is not None)
        timeline.append(
            {
                "t": sec,
                "fps": len(rs),
                "p99_ms": percentile(d, 99) if d else None,
                "max_ms": d[-1] if d else None,
                "screen": max(set(x["screen"] for x in rs), key=[x["screen"] for x in rs].count),
            }
        )
    loading_s = sum(r["dt_ms"] for r in rows if r["screen"] == "loading" and r["dt_ms"]) / 1000.0
    return {
        "loading_s": loading_s,
        "all": frame_stats(dts),
        "by_screen": {k: frame_stats(v) for k, v in sorted(by_screen.items())},
        "timeline": timeline,
        "wall_s": rows[-1]["t"] if rows else 0.0,
        "frame_count": len(rows),
    }


def screen_transitions(rows):
    """[(frame, t, screen)] at each change of the screen column."""
    out, last = [], None
    for r in rows:
        if r["screen"] != last:
            out.append((r["frame"], round(r["t"], 3), r["screen"]))
            last = r["screen"]
    return out


def distance_moved(rows, screen="play"):
    """(total path length, straight distance start to end, max distance from the start) in game units
    while the player is on `screen` and has a position (not all zero)."""
    pts = [r["pos"] for r in rows if r["screen"] == screen and any(r["pos"])]
    if len(pts) < 2:
        return {"path": 0.0, "net": 0.0, "max_from_start": 0.0}
    path = sum(math.dist(a, b) for a, b in zip(pts, pts[1:]) if math.dist(a, b) < 20000)  # ignore teleports
    return {
        "path": path,
        "net": math.dist(pts[0], pts[-1]),
        "max_from_start": max(math.dist(pts[0], p) for p in pts),
    }


# ---------------------------------------------------------------------------------------------
# the viewer's own log

RE_MOVEMENT = re.compile(r"Game's movement: main (\d+) frames; .*?(\d+) untranslated addresses reached")
RE_PRESENT = re.compile(
    r"present (\d+): (\d+) draws, (\d+) skipped, (\d+) pipelines, (\d+) bind groups, (\d+) wgpu errors"
)
RE_SLOW_PRESENTS = re.compile(r"at most ([\d.]+) ms in one present .*?(\d+) presents over 20 ms")
RE_STOP = re.compile(r"^\W*(game's main )?stopped\b.*|.*Game's main stopped in frame.*", re.I)
RE_MEMLOG = re.compile(r"^NV_MEMLOG ((?:\w+=[-\d.]+\s*)+)")
BAD_MEMORY = re.compile(r"not enough memory|device.?lost|out of memory|ErrorOutOfDeviceMemory", re.I)


# the busiest thread comes first; its name may hold spaces ("game main threa/806899 42%")
RE_BUSY = re.compile(r"NV_PERF: thread busy over [\d.]+ s, [\d.]+ cores in all: [^,]*?/\d+ (\d+(?:\.\d+)?)%")


RE_STATE = re.compile(r"# driver: state (\S+) x=(-?[\d.]+) y=(-?[\d.]+) z=(-?[\d.]+)")
RE_PHASE = re.compile(r"# driver: phase (\S+) (start|end) frame=(\d+) t=([\d.]+)")
RE_LOG_FRAME = re.compile(r"^\[f(\d+) ")


def parse_log(text):
    """Counters out of a viewer log; the last report of each cumulative counter wins."""
    out = {
        "main_frames": 0,
        "status_lines": [],  # (main frames, untranslated) at each 5 s status line
        "untranslated_reported": 0,
        "presents": 0,
        "draws": 0,
        "skipped_draws": 0,
        "pipelines": 0,
        "wgpu_errors": 0,
        "slow_presents_over_20ms": 0,
        "stop_lines": [],
        "panics": 0,
        "memory_failures": 0,
        "watchdog": None,
        "guard": None,  # the memory guard's stop line (guard.py), if it stopped the run
        "caps_line": None,
        "exit_status": None,
        "memlog": [],
        "phases": [],  # (name, start|end, frame, seconds since the run began) from the driver's notes
        "busiest_thread_pct": [],  # NV_PERF's report: the busiest thread's CPU share, one per report
        "phase_busy_pct": {},  # phase name -> the busiest-thread shares of the reports logged while it was open
        "states": {},  # the driver's noted player positions by name (the save round trip: saved, loaded)
        "ctrl_down_frames": [],  # the frames where a real Ctrl press reached the game (input.key lines)
    }
    open_phases = set()
    for line in text.splitlines():
        if line.startswith("# driver: state "):
            m = RE_STATE.match(line)
            if m:
                out["states"][m.group(1)] = [float(m.group(2)), float(m.group(3)), float(m.group(4))]
            continue
        if line.startswith("# driver: phase "):
            m = RE_PHASE.match(line)
            if m:
                out["phases"].append((m.group(1), m.group(2), int(m.group(3)), float(m.group(4))))
                (open_phases.add if m.group(2) == "start" else open_phases.discard)(m.group(1))
            continue
        if "input.key: LeftControl (0x1d) down reaches the game" in line:
            m = RE_LOG_FRAME.match(line)
            out["ctrl_down_frames"].append(int(m.group(1)) if m else None)
            continue
        if "NV_PERF: thread busy" in line:
            m = RE_BUSY.search(line)
            if m:
                out["busiest_thread_pct"].append(float(m.group(1)))
                for name in open_phases:
                    out["phase_busy_pct"].setdefault(name, []).append(float(m.group(1)))
            continue
        m = RE_MOVEMENT.search(line)
        if m:
            out["status_lines"].append((int(m.group(1)), int(m.group(2))))
            out["main_frames"] = max(out["main_frames"], int(m.group(1)))
            out["untranslated_reported"] = max(out["untranslated_reported"], int(m.group(2)))
            continue
        m = RE_PRESENT.search(line)
        if m:
            p, d, s, pl, _bg, e = (int(x) for x in m.groups())
            out["presents"], out["draws"], out["skipped_draws"] = max(out["presents"], p), d, s
            out["pipelines"], out["wgpu_errors"] = pl, max(out["wgpu_errors"], e)
            continue
        m = RE_SLOW_PRESENTS.search(line)
        if m:
            out["slow_presents_over_20ms"] = int(m.group(2))
            continue
        m = RE_MEMLOG.match(line)
        if m:
            out["memlog"].append({k: float(v) for k, v in (kv.split("=") for kv in m.group(1).split())})
            continue
        if RE_STOP.match(line) and "static initializer skipped" not in line and "wgpu" not in line.lower():
            out["stop_lines"].append(re.sub(r"/\S+", "<path>", line.strip())[:300])
        elif "panicked at" in line:
            out["panics"] += 1
        elif BAD_MEMORY.search(line):
            out["memory_failures"] += 1
        if line.startswith("# memory guard:"):
            out["guard"] = line.strip()[:300]
            continue
        if line.startswith("# memory caps:"):
            out["caps_line"] = line.strip()[2:400]
            continue
        if line.startswith("# GPU memory watchdog"):
            out["watchdog"] = line.strip()[:200]
        m = re.match(r"# exit status: (-?\d+)", line)
        if m:
            out["exit_status"] = int(m.group(1))
    return out


def count_missing(tsv_text):
    """(distinct untranslated addresses, total hits) from the --movement-missing list."""
    n = hits = 0
    for i, line in enumerate(tsv_text.splitlines()):
        if i == 0 or not line.strip():
            continue
        cols = line.split("\t")
        n += 1
        try:
            hits += int(cols[1])
        except (IndexError, ValueError):
            hits += 1
    return n, hits


def count_stops(tsv_text):
    return sum(1 for i, line in enumerate(tsv_text.splitlines()) if i > 0 and line.strip())


# ---------------------------------------------------------------------------------------------
# sampler timeline (written by the runner): t, rss_mib, cpu_pct, vram_mib, gtt_mib

SAMPLE_COLS = ["t", "rss_mib", "cpu_pct", "vram_mib", "gtt_mib"]


def parse_samples(text):
    rows = []
    for i, line in enumerate(text.splitlines()):
        if i == 0 or not line.strip():
            continue
        try:
            rows.append(dict(zip(SAMPLE_COLS, (float(x) for x in line.split("\t")))))
        except ValueError:
            continue
    return rows


def sample_summary(rows):
    if not rows:
        return {}

    def peak(k):
        return max(r[k] for r in rows)

    def growth(k):
        # slope over the second half of the run, MiB per minute: warm-up (shader and cell loads) excluded
        half = [r for r in rows if r["t"] >= rows[-1]["t"] / 2]
        if len(half) < 3 or half[-1]["t"] - half[0]["t"] < 1:
            return None
        n = len(half)
        mt, mv = sum(r["t"] for r in half) / n, sum(r[k] for r in half) / n
        den = sum((r["t"] - mt) ** 2 for r in half)
        return 60.0 * sum((r["t"] - mt) * (r[k] - mv) for r in half) / den if den else None

    return {
        "rss_peak_mib": peak("rss_mib"),
        "rss_end_mib": rows[-1]["rss_mib"],
        "rss_growth_mib_per_min": growth("rss_mib"),
        "cpu_avg_pct": sum(r["cpu_pct"] for r in rows) / len(rows),
        "vram_peak_mib": peak("vram_mib"),
        "vram_growth_mib_per_min": growth("vram_mib"),
        "gtt_peak_mib": peak("gtt_mib"),
        "gtt_growth_mib_per_min": growth("gtt_mib"),
        "samples": len(rows),
    }


def phase_stats(rows, phases, busy=None):
    """Per phase of a driven run: its seconds of wall time, and for the measured stretches the play frames' fps, p99 and
    1% low (frames between the phase's first and last frame). Phases with no end (the run was cut) end at the last
    frame. With `busy` (parse_log's phase_busy_pct) a measured phase also gets the busiest thread's mean CPU share over
    the NV_PERF reports logged while it ran, and that share as milliseconds of a mean frame (`busy_ms`). A report covers
    the seconds since the previous one, so the first report of a phase reaches a few seconds back (into the warm-up of
    the same place)."""
    last_frame = rows[-1]["frame"] if rows else 0
    spans, open_ = [], {}
    for name, kind, frame, t in phases:
        if kind == "start":
            open_[name] = (frame, t)
        elif name in open_:
            f0, t0 = open_.pop(name)
            spans.append((name, f0, frame, t - t0))
    for name, (f0, t0) in open_.items():
        spans.append((name, f0, last_frame, None))
    out = []
    for name, f0, f1, secs in spans:
        row = {"name": name, "wall_s": None if secs is None else round(secs, 1)}
        if name.endswith(".measured"):
            dts = [r["dt_ms"] for r in rows if f0 <= r["frame"] <= f1 and r["screen"] == "play" and r["dt_ms"] is not None and 0 <= r["dt_ms"] < 5000]
            st = frame_stats(dts) if dts else None
            if st:
                row.update(fps=st["fps"], p99_ms=st["p99_ms"], low1_fps=st["low1_fps"], frames=st["frames"])
                pcts = (busy or {}).get(name)
                if pcts and st["fps"]:
                    pct = sum(pcts) / len(pcts)
                    row.update(busy_pct=round(pct, 1), busy_ms=round(pct / 100.0 * 1000.0 / st["fps"], 2))
        out.append(row)
    return out


def heading_changes(rows, phases):
    """Per place of a driven run: the angle in degrees between the direction walked in `<place>.walk1` and the direction of
    the last 1.5 s of `<place>.walk2` (the view turned in between, so the walk goes another way): the camera turned
    if it is well over 0. None when either leg moved less than 30 units (the player did not walk there). Pure."""
    import math
    spans = {}
    for name, kind, frame, t in phases:
        spans.setdefault(name, {})[kind] = (frame, t)
    out = {}
    for name in [n for n in spans if n.endswith(".walk1")]:
        place = name[:-6]
        w1, w2 = spans.get(name, {}), spans.get(place + ".walk2", {})
        if "start" not in w1 or "end" not in w1 or "start" not in w2 or "end" not in w2:
            continue

        def leg(f0, f1, tail_s=None):
            rs = [r for r in rows if f0 <= r["frame"] <= f1 and r["screen"] == "play"]
            if tail_s is not None and rs:
                cut = rs[-1]["t"] - tail_s
                rs = [r for r in rs if r["t"] >= cut]
            if len(rs) < 2:
                return None
            dx, dy = rs[-1]["pos"][0] - rs[0]["pos"][0], rs[-1]["pos"][1] - rs[0]["pos"][1]
            return (dx, dy) if math.hypot(dx, dy) >= 30 else None

        a, b = leg(w1["start"][0], w1["end"][0]), leg(w2["start"][0], w2["end"][0], 1.5)
        if a and b:
            ang = abs(math.degrees(math.atan2(a[0] * b[1] - a[1] * b[0], a[0] * b[0] + a[1] * b[1])))
            out[place] = round(ang, 1)
        else:
            out[place] = None
    return out


def reach_units(rows, f0, f1, lag_s=0.6):
    """How far the player got from where it stood at frame `f0`, over the play frames from `f0` to `lag_s` seconds past
    frame `f1` (x and y only). The game answers a key some frames after the driver's note, so the window runs on past
    the end of the step; the farthest point is taken, before the step back returns the player."""
    rs = [r for r in rows if r["frame"] >= f0 and r["screen"] == "play"]
    end = next((r["t"] for r in rs if r["frame"] >= f1), None)
    if end is not None:
        rs = [r for r in rs if r["t"] <= end + lag_s]
    if not rs:
        return 0.0
    x0, y0 = rs[0]["pos"][0], rs[0]["pos"][1]
    return max(((r["pos"][0] - x0) ** 2 + (r["pos"][1] - y0) ** 2) ** 0.5 for r in rs)


def sneak_ratios(rows, phases, ctrl_frames=()):
    """Per place of a driven run with a sneak check: how far the same step forward went standing (`<place>.stand`) and
    sneaking (`<place>.sneak`), their ratio (sneaking is slower: well under 1 when Ctrl made the player sneak), and
    whether a real Ctrl press reached the game between the two. The ratio is None when the standing step moved under
    30 units. Pure."""
    spans = {}
    for name, kind, frame, _t in phases:
        spans.setdefault(name, {})[kind] = frame
    out = {}
    for name in [n for n in spans if n.endswith(".stand")]:
        place = name[:-6]
        a, b = spans[name], spans.get(place + ".sneak", {})
        if "start" not in a or "end" not in a or "start" not in b or "end" not in b:
            continue
        stand, sneak = reach_units(rows, a["start"], a["end"]), reach_units(rows, b["start"], b["end"])
        ctrl = any(f is not None and a["end"] <= f <= b["start"] for f in ctrl_frames)
        out[place] = {"stand": round(stand, 1), "sneak": round(sneak, 1), "ratio": round(sneak / stand, 2) if stand >= 30 else None, "ctrl": ctrl}
    return out


def hitches(rows):
    """Hitches in the play frames: a frame over 25 ms and over three times the median. Count and a minute's worth."""
    dts = [r["dt_ms"] for r in rows if r["screen"] == "play" and r["dt_ms"] is not None and 0 <= r["dt_ms"] < 5000]
    if len(dts) < 10:
        return {"count": 0, "per_min": None}
    med = sorted(dts)[len(dts) // 2]
    n = sum(1 for d in dts if d > 25.0 and d > 3 * med)
    minutes = sum(dts) / 60000.0
    return {"count": n, "per_min": round(n / minutes, 2) if minutes > 0 else None}


def game_clock_sanity(rows):
    """Game time against wall time, and the player's fall and landing, over the play frames (500+ fps must not change
    what the game does): `ratio` is the sum of the game's frame deltas over the wall time of the same frames (1.0: the
    game keeps real time); `speed` is the walked path over the wall seconds the player was walking (units a second);
    `fall` is how far the player dropped below the highest point since arrival and `settled` whether the height
    stopped changing (landed) in the last fifth of the run; `stuck_s` the longest stretch of play with no movement at
    all after the player first moved. Pure."""
    play = [r for r in rows if r["screen"] == "play" and r["dt_ms"] is not None and 0 <= r["dt_ms"] < 1000 and r.get("fdelta") is not None]
    out = {"ratio": None, "speed": None, "fall": None, "settled": None, "stuck_s": None, "play_s": 0.0}
    if len(play) < 10:
        return out
    wall = sum(r["dt_ms"] for r in play) / 1000.0
    out["play_s"] = round(wall, 1)
    out["ratio"] = round(sum(r["fdelta"] for r in play) / wall, 3) if wall > 0 else None
    zs = [r["pos"][2] for r in play]
    top = max(zs[: max(1, len(zs) // 5)])
    out["fall"] = round(max(0.0, top - min(zs)), 1)
    tail = zs[-max(10, len(zs) // 5):]
    out["settled"] = (max(tail) - min(tail)) < 200.0
    moving_s, path, last, still, longest = 0.0, 0.0, None, 0.0, 0.0
    started = False
    for r in play:
        p = r["pos"]
        if last is not None:
            d = ((p[0] - last[0]) ** 2 + (p[1] - last[1]) ** 2) ** 0.5
            if d < 20000:
                if d > 0.01:
                    started = True
                    path += d
                    moving_s += r["dt_ms"] / 1000.0
                    still = 0.0
                elif started:
                    still += r["dt_ms"] / 1000.0
                    longest = max(longest, still)
        last = p
    out["speed"] = round(path / moving_s, 1) if moving_s > 1.0 else None
    out["stuck_s"] = round(longest, 1) if started else None
    return out


def summarize(frame_rows, log, samples, missing=(0, 0), stops=0):
    """The run's metrics as one flat-ish dict (what summary.json holds under `metrics`)."""
    fs = frame_summary(frame_rows)
    a = fs["all"]
    play = fs["by_screen"].get("play", {})
    sm = sample_summary(samples)
    return {
        "frame_time": a,
        "frame_time_play": play,
        "frame_time_loading": fs["by_screen"].get("loading", {}),
        "fps_timeline": fs["timeline"],
        "wall_s": fs["wall_s"],
        "frames": fs["frame_count"] or log["main_frames"],
        "loading_s": fs["loading_s"],
        "screens": screen_transitions(frame_rows),
        "movement": distance_moved(frame_rows),
        # the frame log shows (2048, 2048, 128) until the game puts the player in a cell
        "player_placed": any(r["pos"] not in ([0.0, 0.0, 0.0], [2048.0, 2048.0, 128.0]) for r in frame_rows),
        "memory": sm,
        "clock": game_clock_sanity(frame_rows),
        "hitches": hitches(frame_rows),
        "phases": phase_stats(frame_rows, log["phases"], log.get("phase_busy_pct")),
        "heading_change_deg": heading_changes(frame_rows, log["phases"]),
        "states": log.get("states") or {},
        "sneak": sneak_ratios(frame_rows, log["phases"], log.get("ctrl_down_frames") or ()),
        "main_busy_pct": round(sum(log["busiest_thread_pct"]) / len(log["busiest_thread_pct"]), 1) if log["busiest_thread_pct"] else None,
        "untranslated_addresses": missing[0],
        "untranslated_hits": missing[1],
        "stop_lines": log["stop_lines"],
        "stops": max(len(log["stop_lines"]), stops),
        "wgpu_errors": log["wgpu_errors"],
        "skipped_draws": log["skipped_draws"],
        "draws": log["draws"],
        "pipelines": log["pipelines"],
        "slow_presents_over_20ms": log["slow_presents_over_20ms"],
        "panics": log["panics"],
        "memory_failures": log["memory_failures"],
        "watchdog": log["watchdog"],
        "guard": log.get("guard"),
        "caps": log.get("caps_line"),
        "exit_status": log["exit_status"],
        "memlog_lines": len(log["memlog"]),
        "heap_used_end": log["memlog"][-1].get("heap_used") if log["memlog"] else None,
    }
