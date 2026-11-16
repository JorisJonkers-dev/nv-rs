"""The timeline of a bench run lined up with its recording: frame times, fps, hitches, screens and scenario steps.

Numbers and labels only (no game content). The time axis is the video's: a frame at engine time t after the first
frame is at `offset + t` seconds in the recording, `offset` being how long the recording ran before the first frame
(record.json's `first_frame_unix - start_unix`). Stdlib only.
"""
import csv
import io
import json
from pathlib import Path

import parse

MAX_POINTS = 6000


def read(p):
    try:
        return Path(p).read_text(errors="replace")
    except OSError:
        return ""


def read_json(p):
    try:
        return json.loads(read(p))
    except ValueError:
        return {}


def parse_hitches(text):
    """Rows of the hitch recorder's `<log>.hitches.csv` (unix_ms,run_ms,frame,wall_ms,cause) as dicts."""
    out = []
    for r in csv.DictReader(io.StringIO(text)):
        try:
            out.append({"unix_ms": int(r["unix_ms"]), "run_ms": int(r["run_ms"]), "frame": int(r["frame"]), "wall_ms": float(r["wall_ms"]), "cause": r.get("cause", "")})
        except (KeyError, ValueError, TypeError):
            continue
    return out


def downsample(points, limit=MAX_POINTS):
    """At most `limit` (t, value) points keeping each bucket's largest value, so spikes survive."""
    if len(points) <= limit:
        return points
    step = len(points) / limit
    out = []
    i = 0.0
    while int(i) < len(points):
        chunk = points[int(i):int(i + step) or int(i) + 1]
        out.append(max(chunk, key=lambda p: p[1]))
        i += step
    return out


def fps_series(rows, window=1.0):
    """(t, fps) per `window` seconds from the rows' t."""
    out, start, n = [], None, 0
    for r in rows:
        if start is None:
            start = r["t"]
        if r["t"] - start >= window:
            out.append((round(start + window, 3), round(n / (r["t"] - start), 2)))
            start, n = r["t"], 0
        n += 1
    return out


def step_label(item):
    return item if isinstance(item, str) else str(item)


def build(run_dir):
    """The timeline dict of a run directory: duration, frames, fps, hitches, screens, steps and the video name."""
    d = Path(run_dir)
    rows = [r for r in parse.parse_frame_log(read(d / "frames.tsv")) if r.get("dt_ms") is not None]
    rec = read_json(d / "record.json")
    offset = float(rec.get("first_frame_unix", 0) - rec.get("start_unix", 0)) if rec.get("first_frame_unix") and rec.get("start_unix") else 0.0
    offset = max(offset, 0.0)
    by_frame = {r["frame"]: r for r in rows}
    hit_rows = parse_hitches(read(d / "hitches.csv"))
    # the hitch recorder stamps wall time on each hitch: that pins the first frame's wall time to the ms, finer than
    # the runner's half-second poll
    est = sorted(h["unix_ms"] / 1000.0 - by_frame[h["frame"]]["t"] for h in hit_rows if h["frame"] in by_frame)
    if est and rec.get("start_unix"):
        rec["first_frame_unix"] = est[len(est) // 2]
        offset = max(rec["first_frame_unix"] - rec["start_unix"], 0.0)
    frames = [(round(offset + r["t"], 4), round(r["dt_ms"], 3)) for r in rows]
    hitches = []
    first_unix = rec.get("first_frame_unix")
    for h in hit_rows:
        r = by_frame.get(h["frame"])
        if r is not None:
            t = offset + r["t"]
        elif first_unix:
            t = offset + (h["unix_ms"] / 1000.0 - first_unix)
        else:
            continue
        hitches.append({"t": round(t, 3), "ms": h["wall_ms"], "cause": h["cause"], "frame": h["frame"]})
    hitches.sort(key=lambda h: h["t"])
    screens, last = [], None
    for r in rows:
        if r["screen"] != last:
            screens.append({"t": round(offset + r["t"], 3), "name": r["screen"] or "?"})
            last = r["screen"]
    steps = []
    for s in read_json(d / "steps.json").get("steps", []):
        r = by_frame.get(s.get("frame"))
        if r is None and rows:  # the nearest logged frame at or after it
            r = next((x for x in rows if x["frame"] >= s.get("frame", 0)), None)
        if r is not None:
            steps.append({"t": round(offset + r["t"], 3), "name": step_label(s.get("name", "")), "frame": s.get("frame")})
    duration = frames[-1][0] if frames else 0.0
    return {
        "run": d.name,
        "video": rec.get("file"),
        "offset_s": round(offset, 3),
        "duration_s": round(duration, 3),
        "frame_count": len(frames),
        "frames": downsample(frames),
        "fps": fps_series([{"t": offset + r["t"]} for r in rows]),
        "hitches": hitches,
        "screens": screens,
        "steps": steps,
    }


def to_tsv(tl):
    """timeline.tsv: kind, t_s, frame, value, label; frame rows are all kept (not downsampled) only when small."""
    lines = ["kind\tt_s\tframe\tvalue\tlabel"]
    for t, ms in tl["frames"]:
        lines.append(f"frame\t{t}\t\t{ms}\t")
    for h in tl["hitches"]:
        lines.append(f"hitch\t{h['t']}\t{h['frame']}\t{h['ms']}\t{h['cause']}")
    for s in tl["screens"]:
        lines.append(f"screen\t{s['t']}\t\t\t{s['name']}")
    for s in tl["steps"]:
        lines.append(f"step\t{s['t']}\t{s.get('frame') or ''}\t\t{s['name']}")
    return "\n".join(lines) + "\n"


def ass_time(t):
    t = max(t, 0.0)
    return f"{int(t // 3600)}:{int(t // 60) % 60:02d}:{t % 60:05.2f}"


def to_ass(tl, hold=2.0):
    """An ASS subtitle file: a top-left status line (screen, step) and a hitch marker for `hold` seconds at each hitch."""
    head = (
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n"
        "[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\n"
        "Style: Info,monospace,28,&H00FFFFFF,&H00FFFFFF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,0,7,20,20,20,1\n"
        "Style: Hitch,monospace,34,&H004040FF,&H004040FF,&H00000000,&H80000000,1,0,0,0,100,100,0,0,1,2,0,9,20,20,20,1\n\n"
        "[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\n"
    )
    ev = []
    marks = sorted([(s["t"], "screen " + s["name"]) for s in tl["screens"]] + [(s["t"], "step " + s["name"]) for s in tl["steps"]])
    cur = {"screen": "", "step": ""}
    for i, (t, label) in enumerate(marks):
        kind, _, val = label.partition(" ")
        cur[kind] = val
        end = marks[i + 1][0] if i + 1 < len(marks) else tl["duration_s"] + 1
        if end > t:
            ev.append(f"Dialogue: 0,{ass_time(t)},{ass_time(end)},Info,,0,0,0,,screen {cur['screen']}  step {cur['step']}")
    for h in tl["hitches"]:
        ev.append(f"Dialogue: 1,{ass_time(h['t'])},{ass_time(h['t'] + hold)},Hitch,,0,0,0,,HITCH {h['ms']:.0f} ms {h['cause']}")
    return head + "\n".join(ev) + "\n"
