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
        "exit_status": None,
        "memlog": [],
    }
    for line in text.splitlines():
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
        "exit_status": log["exit_status"],
        "memlog_lines": len(log["memlog"]),
        "heap_used_end": log["memlog"][-1].get("heap_used") if log["memlog"] else None,
    }
