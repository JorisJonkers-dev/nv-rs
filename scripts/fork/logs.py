#!/usr/bin/env python3
"""nv-rs-dev logs: what happened in a run, in a few lines (docs/fork/LOGS.md). Reads the run's JSON-lines
log and run summary (schema nv-rs-run-summary/1) from ~/.local/state/nv-rs/logs (NV_LOG_DIR), and falls back
to the plain-text log (nv-rs-<unix>-<pid>.log) when the JSON file is gone. Standard library only.

    logs.py [--run ID|latest|latest-bad|PATH] [--dir DIR]
            [--errors | --hitches | --tail N | --grep PATTERN | --around FRAME|MSms [--window N]]
            [--level LEVEL] [--target a,b] [--json] [--bundle [FILE]] [--color auto|always|never]
    logs.py --diff A B

A run is named by its id (`<unix>-<pid>`, a prefix is enough), `latest`, `latest-bad` (the newest run that
was flagged, stopped or logged an error) or a path to a .jsonl, .log or run-summary file.
"""
import argparse
import json
import os
import platform
import re
import sys
from pathlib import Path

LEVELS = {"error": 1, "warn": 2, "info": 3, "debug": 4, "trace": 5}
HARD = {"stopped", "killed", "unclean_exit"}
FILE_RE = re.compile(r"^nv-rs-(\d+)-(\d+)\.(jsonl|log)$")
SUMMARY_RE = re.compile(r"^run-summary-(\d+)-(\d+)\.json$")
TEXT_RE = re.compile(r"^(\S+Z) (\w+)\s+(\S+) f(-|\d+): (.*)$")
SHAPE_RE = re.compile(r"0x[0-9a-fA-F]+|\d+")


def default_dir():
    if os.environ.get("NV_LOG_DIR"):
        return Path(os.environ["NV_LOG_DIR"])
    if os.name == "nt" and os.environ.get("LOCALAPPDATA"):
        return Path(os.environ["LOCALAPPDATA"]) / "nv-rs" / "logs"
    state = os.environ.get("XDG_STATE_HOME") or str(Path.home() / ".local/state")
    return Path(state) / "nv-rs" / "logs"


# ----- finding runs -----

def find_runs(directory):
    """Every run in the folder, oldest first: {id, unix, pid, jsonl, text, summary_path, summary}."""
    runs = {}
    d = Path(directory)
    if not d.is_dir():
        return []
    for p in d.iterdir():
        m = FILE_RE.match(p.name) or SUMMARY_RE.match(p.name)
        if not m:
            continue
        rid = f"{m.group(1)}-{m.group(2)}"
        r = runs.setdefault(rid, {"id": rid, "unix": int(m.group(1)), "pid": int(m.group(2)),
                                  "jsonl": None, "text": None, "summary_path": None, "summary": None})
        if p.name.endswith(".jsonl"):
            r["jsonl"] = p
        elif p.name.endswith(".log"):
            r["text"] = p
        else:
            r["summary_path"] = p
            try:
                r["summary"] = json.loads(p.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                pass
    out = sorted(runs.values(), key=lambda r: (r["unix"], r["pid"]))
    for r in out:
        s = r["summary"]
        if s and s.get("ended") == "running" and not pid_alive(r["pid"]):
            s.setdefault("flags", [])
            if "unclean_exit" not in s["flags"]:
                s["flags"].append("unclean_exit")
    return out


def pid_alive(pid):
    return Path(f"/proc/{pid}").exists() if sys.platform.startswith("linux") else True


def is_bad(run):
    s = run["summary"]
    if s:
        return bool(s.get("flags")) or s.get("errors", 0) > 0
    return any(r.get("level") == "error" or r.get("event") in ("stop", "panic") for r in load_records(run))


def pick(runs, key):
    """The run `key` names, or None."""
    if key is None or key == "latest":
        return runs[-1] if runs else None
    if key == "latest-bad":
        bad = [r for r in runs if is_bad(r)]
        return bad[-1] if bad else None
    p = Path(key)
    if p.is_file():
        m = FILE_RE.match(p.name) or SUMMARY_RE.match(p.name)
        stem = f"{m.group(1)}-{m.group(2)}" if m else None
        for r in runs:
            if r["id"] == stem:
                return r
        r = {"id": stem or p.stem, "unix": 0, "pid": 0, "jsonl": None, "text": None, "summary_path": None, "summary": None}
        if p.suffix == ".jsonl":
            r["jsonl"] = p
        elif p.suffix == ".log":
            r["text"] = p
        else:
            r["summary_path"] = p
            try:
                r["summary"] = json.loads(p.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                return None
        return r
    for r in reversed(runs):
        if r["id"] == key or r["id"].startswith(key):
            return r
    return None


# ----- reading a run -----

def read_lines(path):
    """The lines of a log file, the rotated older half (`.1`) first."""
    out = []
    for p in (Path(str(path) + ".1"), Path(path)):
        try:
            with open(p, encoding="utf-8", errors="replace") as f:
                out.extend(f.read().splitlines())
        except OSError:
            pass
    return out


def parse_text(line):
    m = TEXT_RE.match(line)
    if not m:
        return None
    ts, level, target, frame, message = m.groups()
    return {"ts": ts, "level": level.lower(), "target": target, "message": message,
            "frame": None if frame == "-" else int(frame), "_text": True}


def load_records(run):
    """The run's records in order: from the JSON lines, else the text log."""
    if "_records" in run:
        return run["_records"]
    recs = []
    if run.get("jsonl"):
        for line in read_lines(run["jsonl"]):
            if line.startswith("{"):
                try:
                    recs.append(json.loads(line))
                except ValueError:
                    pass
    if not recs and run.get("text"):
        recs = [r for r in map(parse_text, read_lines(run["text"])) if r]
    run["_records"] = recs
    return recs


def source_name(run):
    if run.get("jsonl") and any(not r.get("_text") for r in load_records(run)):
        return "jsonl"
    return "text" if run.get("text") and load_records(run) else "none"


def shape(rec):
    return rec.get("target", "") + ":" + SHAPE_RE.sub("#", rec.get("message", ""))[:160]


def is_event(rec):
    return rec.get("event") in ("stop", "panic")


def problems(recs):
    """Warnings and errors by shape: [{level, target, message, count, first_frame, last_frame}], worst first."""
    seen = {}
    for r in recs:
        lv = r.get("level")
        if lv not in ("warn", "error") or "message" not in r or r.get("kind"):
            continue
        s = seen.get(shape(r))
        if s is None:
            seen[shape(r)] = s = {"level": lv, "target": r.get("target", ""), "message": r["message"], "count": 0,
                                  "first_frame": r.get("frame"), "last_frame": r.get("frame")}
        s["count"] += 1
        s["last_frame"] = r.get("frame")
        if lv == "error":
            s["level"] = "error"
    return sorted(seen.values(), key=lambda s: (s["level"] != "error", -s["count"], s["message"]))


def stops(recs):
    return [r for r in recs if is_event(r)]


def hitch_records(recs):
    return [r["hitch"] for r in recs if r.get("kind") == "hitch" and "hitch" in r]


def worst_hitches(run, n=3):
    hs = sorted(hitch_records(load_records(run)), key=lambda h: -float(h.get("wall_ms", 0)))
    if hs:
        return [{"wall_ms": float(h.get("wall_ms", 0)), "frame": (h.get("frame") or {}).get("idx", h.get("frame_idx")),
                 "run_ms": (h.get("frame") or {}).get("end_ms", 0), "cause": h.get("cause", "?"),
                 "trigger": h.get("trigger", "none"), "top": [c["call"] for c in (h.get("samples") or {}).get("calls", [])[:2]]}
                for h in hs[:n]]
    s = run["summary"] or {}
    return [{"wall_ms": w["wall_ms"], "frame": w["frame"], "run_ms": w["run_ms"], "cause": w["cause"],
             "trigger": w["trigger"], "top": w.get("top", [])[:2]} for w in s.get("worst", [])[:n]]


# ----- colors -----

class Paint:
    CODES = {"red": "31", "yellow": "33", "green": "32", "dim": "2", "bold": "1", "cyan": "36"}

    def __init__(self, on):
        self.on = on

    def __call__(self, color, text):
        return f"\x1b[{self.CODES[color]}m{text}\x1b[0m" if self.on and text else text

    def level(self, level, text=None):
        color = {"error": "red", "warn": "yellow"}.get(level)
        text = level if text is None else text
        return self(color, text) if color else self("dim", text)


def verdict_color(verdict):
    return "green" if verdict.startswith("OK") else "red" if verdict.startswith("FAILED") else "yellow"


# ----- views -----

def span(secs):
    s = int(round(secs or 0))
    return f"{s // 3600}h {s % 3600 // 60:02d}m" if s >= 3600 else f"{s // 60}m {s % 60:02d}s" if s >= 60 else f"{s}s"


def fmt_frame(f):
    return "-" if f is None else f"f{f}"


def problem_line(p, paint, width=150):
    where = fmt_frame(p["first_frame"]) if p["count"] == 1 else f"{fmt_frame(p['first_frame'])}..{fmt_frame(p['last_frame'])}"
    n = f"{p['count']}x " if p["count"] > 1 else ""
    msg = p["message"] if len(p["message"]) <= width else p["message"][: width - 3] + "..."
    return f"  {paint.level(p['level'], p['level'].ljust(5))} {n}{p['target']}: {msg}  {paint('dim', where)}"


def stop_lines(recs, paint):
    out = []
    for r in stops(recs):
        if r["event"] == "panic":
            out.append(f"  {paint('red', 'panic')} {r.get('reason', '')} ({r.get('location', '')}) at {fmt_frame(r.get('frame'))}")
        else:
            chain = " ".join(r.get("addresses", [])[:6])
            out.append(f"  {paint('red', 'stop')} [{r.get('class', '?')}] {r.get('reason', '')} at {fmt_frame(r.get('frame'))}"
                       + (f"; addresses {chain}" if chain else ""))
    return out


def hitch_line(h, paint):
    top = f", {'/'.join(h['top'])}" if h["top"] else ""
    return f"  {h['wall_ms']:.0f} ms at {fmt_frame(h['frame'])} ({span(h['run_ms'] / 1000)}): {h['cause']}, trigger {h['trigger']}{top}"


def facts_line(s):
    m = (s or {}).get("meta", {})
    parts = []
    if m.get("version"):
        parts.append(f"nv-rs {m['version']}" + (f" ({m['profile']})" if m.get("profile") else ""))
    for k in ("gpu", "driver", "resolution", "session", "os", "threads"):
        if m.get(k):
            parts.append(f"{k} {m[k]}")
    return "; ".join(parts)


def report(run, paint, top=6):
    """The default view: under 40 lines for a normal run, under 80 for a bad one."""
    s = run["summary"]
    recs = load_records(run)
    out = []
    if s:
        v = s.get("verdict", "").split("; flags")[0]
        out.append(f"{paint('bold', 'run ' + run['id'])}  {paint(verdict_color(v), v)}")
        f, h = s.get("frame_ms", {}), s.get("hitches", {})
        out.append(f"  ended {s.get('ended')}; {span(s.get('duration_s'))} ({span(s.get('play_s'))} play, {span(s.get('loading_s'))} loading), {s.get('frames', 0)} frames")
        out.append(f"  {f.get('fps', 0):.1f} fps, p99 {f.get('p99', 0):.1f} ms, 1% low {f.get('low1_fps', 0):.0f} fps, 0.1% low {f.get('low01_fps', 0):.0f} fps, max {f.get('max', 0):.0f} ms")
        causes = ", ".join(f"{k} x{c['n']} ({c['ms']} ms)" for k, c in h.get("by_cause", {}).items())
        out.append(f"  hitches {h.get('count', 0)} ({h.get('per_min', 0)}/min), slow windows {h.get('slow_windows', 0)}" + (f": {causes}" if causes else ""))
        peaks = ", ".join(f"{k} {v}" for k, v in s.get("peaks", {}).items())
        out.append(f"  errors {s.get('errors', 0)}, warnings {s.get('warnings', 0)}, soft stops {s.get('soft_stops', 0)}; flags: {', '.join(s.get('flags', [])) or 'none'}"
                   + (f"; peaks {peaks}" if peaks else ""))
        if facts_line(s):
            out.append(f"  {facts_line(s)}")
    else:
        out.append(f"{paint('bold', 'run ' + run['id'])}  {paint('yellow', 'no run summary')} (read from the {source_name(run)} log)")
        out.append(f"  {len(recs)} records; errors {sum(1 for r in recs if r.get('level') == 'error')}, warnings {sum(1 for r in recs if r.get('level') == 'warn')}")
    st = stops(recs)
    out.append(f"{paint('bold', 'stops')}" + ("" if st else ": none"))
    out.extend(stop_lines(recs, paint))
    ps = problems(recs)
    if st:
        # The stop is said above, with its addresses; its own log line says the same.
        ps = [p for p in ps if not p["message"].startswith("Game's main stopped")]
    out.append(f"{paint('bold', 'errors and warnings')}" + (f" ({len(ps)} kinds)" if ps else ": none"))
    out.extend(problem_line(p, paint) for p in ps[:top])
    if len(ps) > top:
        out.append(f"  ... {len(ps) - top} more kinds (nv-rs-dev logs --errors)")
    wh = worst_hitches(run)
    if wh:
        out.append(f"{paint('bold', 'worst hitches')}")
        out.extend(hitch_line(h, paint) for h in wh)
    if st:
        last = [r for r in recs if r.get("event") in ("stop", "panic")][-1].get("last_lines", [])[-4:]
        if last:
            out.append(f"{paint('bold', 'last lines before the end')}")
            out.extend(f"  {paint('dim', l[:150])}" for l in last)
    files = [str(p) for p in (run.get("text"), run.get("jsonl"), run.get("summary_path")) if p]
    out.append(f"{paint('bold', 'files')}: " + "; ".join(files))
    return out


def errors_view(run, paint):
    recs = load_records(run)
    out = stop_lines(recs, paint)
    ps = problems(recs)
    out.extend(problem_line(p, paint, 400) for p in ps)
    return out or ["no errors, warnings or stops"]


def hitches_view(run, paint):
    recs = load_records(run)
    hs = sorted(hitch_records(recs), key=lambda h: -float(h.get("wall_ms", 0)))
    s = run["summary"] or {}
    out = []
    if s.get("hitches"):
        h = s["hitches"]
        out.append(f"{h.get('count', 0)} hitches ({h.get('per_min', 0)}/min), slow windows {h.get('slow_windows', 0)}")
        for k, c in h.get("by_cause", {}).items():
            out.append(f"  {k:<14} x{c['n']:<5} {c['ms']} ms")
    if hs:
        out.append(f"worst {min(len(hs), 10)} of {len(hs)} recorded:")
        for h in worst_hitches(run, 10):
            out.append(hitch_line(h, paint))
    elif s.get("worst"):
        out.append("worst (from the run summary; the log holds no hitch records):")
        out.extend(hitch_line(h, paint) for h in worst_hitches(run, 10))
    return out or ["no hitches recorded"]


def fmt_record(r, paint):
    if "message" not in r:
        return None
    lv = r.get("level", "info")
    return f"{paint('dim', (r.get('ts') or '')[11:23])} {paint.level(lv, lv.upper()[:5].ljust(5))} {r.get('target', '')} {paint('dim', fmt_frame(r.get('frame')))}: {r['message']}"


def select(recs, level=None, targets=None):
    mx = LEVELS.get(level, 5) if level else 5
    ts = set(targets.split(",")) if targets else None
    return [r for r in recs if "message" in r and LEVELS.get(r.get("level"), 3) <= mx and (ts is None or r.get("target") in ts)]


def to_epoch_ms(ts):
    import datetime
    try:
        return datetime.datetime.strptime(ts, "%Y-%m-%dT%H:%M:%S.%fZ").replace(tzinfo=datetime.timezone.utc).timestamp() * 1000
    except (TypeError, ValueError):
        return None


def parse_ms(ts, recs):
    first = next((to_epoch_ms(r.get("ts")) for r in recs if to_epoch_ms(r.get("ts")) is not None), None)
    now = to_epoch_ms(ts)
    return None if first is None or now is None else now - first


def around(recs, spec, window, paint, level=None, targets=None):
    sel = select(recs, level, targets)
    if spec.endswith("ms"):
        centre = float(spec[:-2])
        span_ms = window if window is not None else 2000
        hit = [r for r in sel if (m := parse_ms(r.get("ts"), recs)) is not None and abs(m - centre) <= span_ms]
    else:
        centre = int(spec.lstrip("f"))
        span_f = window if window is not None else 30
        hit = [r for r in sel if r.get("frame") is not None and abs(r["frame"] - centre) <= span_f]
    return hit


def diff(a, b):
    """The delta of two runs' summaries: rows (name, a, b, delta) and the regressions."""
    sa, sb = a["summary"] or {}, b["summary"] or {}

    def g(s, *path):
        for p in path:
            s = s.get(p, {}) if isinstance(s, dict) else {}
        return s if not isinstance(s, dict) else 0

    metrics = [("fps", ("frame_ms", "fps"), False), ("p99 ms", ("frame_ms", "p99"), True), ("1% low fps", ("frame_ms", "low1_fps"), False),
               ("hitches/min", ("hitches", "per_min"), True), ("hitches", ("hitches", "count"), True), ("loading s", ("loading_s",), True),
               ("errors", ("errors",), True), ("warnings", ("warnings",), True), ("soft stops", ("soft_stops",), True)]
    rows, regressions = [], []
    for name, path, worse_up in metrics:
        x, y = g(sa, *path), g(sb, *path)
        rows.append((name, x, y, y - x))
        rel = (y - x) / x if x else (1.0 if y else 0.0)
        if name in ("errors", "warnings", "soft stops"):
            bad = y > x
        elif name == "hitches/min":
            bad = y > x and (y - x) >= 0.5
        elif name == "hitches":
            bad = False
        else:
            bad = (rel > 0.10) if worse_up else (rel < -0.05)
        if bad:
            regressions.append(f"{name}: {x:g} -> {y:g}")
    for k in sorted(set(sa.get("peaks", {})) | set(sb.get("peaks", {}))):
        x, y = sa.get("peaks", {}).get(k, 0), sb.get("peaks", {}).get(k, 0)
        rows.append((f"peak {k}", x, y, y - x))
        if x and (y - x) / x > 0.10:
            regressions.append(f"peak {k}: {x} -> {y}")
    new = [f for f in sb.get("flags", []) if f not in sa.get("flags", [])]
    gone = [f for f in sa.get("flags", []) if f not in sb.get("flags", [])]
    regressions.extend(f"new flag {f}" for f in new)
    return rows, regressions, new, gone


def diff_view(a, b, paint):
    rows, regs, new, gone = diff(a, b)
    out = [f"{a['id']}  ->  {b['id']}", f"{'':<14}{'A':>10}{'B':>10}{'change':>10}"]
    for name, x, y, d in rows:
        out.append(f"{name:<14}{x:>10g}{y:>10g}{d:>+10.4g}")
    if new or gone:
        out.append(f"flags: new {', '.join(new) or '-'}; gone {', '.join(gone) or '-'}")
    out.append(paint("red" if regs else "green", "regressions: " + ("; ".join(regs) if regs else "none")))
    return out


# ----- the bundle -----

def scrub(text, home=None):
    """No home paths, user names or game folder paths in what is shared."""
    home = home or str(Path.home())
    if home and len(home) > 1:
        text = text.replace(home, "~")
    text = re.sub(r"/home/[^/\s\"']+", "~", text)
    text = re.sub(r"/Users/[^/\s\"']+", "~", text)
    text = re.sub(r"[A-Za-z]:\\+Users\\+[^\\\s\"']+", "~", text)
    text = re.sub(r"%USERPROFILE%", "~", text, flags=re.I)
    text = re.sub(r"(?:~|/)[^\s\"']*(?:steamapps|SteamLibrary|Fallout[ _]New[ _]Vegas|Fallout New Vegas)[^\s\"')]*", "<game folder>", text, flags=re.I)
    text = re.sub(r"/nix/store/[a-z0-9]{32}-", "/nix/store/", text)
    return text


def bundle_text(run):
    paint = Paint(False)
    recs = load_records(run)
    out = [f"nv-rs log bundle for run {run['id']}", "(no game data; home paths and the game folder are removed)", ""]
    head = []
    for line in report(run, paint):
        if line.startswith("stops"):
            break
        head.append(line)
    out.extend(head)
    out += ["", "stops:"]
    out.extend(stop_lines(recs, paint) or ["  none"])
    out += ["", "errors and warnings (all kinds):"]
    out.extend(problem_line(p, paint, 300) for p in problems(recs))
    out += ["", "worst hitches:"]
    out.extend(hitch_line(h, paint) for h in worst_hitches(run, 10))
    s = run["summary"] or {}
    out += ["", "environment:"]
    out.append(f"  summary meta: {json.dumps(s.get('meta', {}), sort_keys=True)}")
    out.append(f"  machine now: {platform.platform()}, python {platform.python_version()}")
    ring = [r for r in recs if is_event(r)]
    if ring:
        out += ["", "last lines before the end:"]
        out.extend("  " + l[:300] for l in ring[-1].get("last_lines", [])[-30:])
    return scrub("\n".join(out)) + "\n"


# ----- main -----

def emit_json(obj):
    print(json.dumps(obj, indent=None, sort_keys=True))


def main(argv=None):
    ap = argparse.ArgumentParser(prog="nv-rs-dev logs", description=__doc__.split("\n\n")[0])
    ap.add_argument("--run", default=None, help="id, latest, latest-bad or a path")
    ap.add_argument("--dir", help="the log folder (default ~/.local/state/nv-rs/logs, NV_LOG_DIR)")
    view = ap.add_mutually_exclusive_group()
    view.add_argument("--errors", action="store_true", help="every kind of warning and error, and the stops")
    view.add_argument("--hitches", action="store_true", help="hitch counts by cause and the worst ones")
    view.add_argument("--tail", type=int, metavar="N", help="the last N log lines")
    view.add_argument("--grep", metavar="PATTERN", help="lines matching a regular expression (case-insensitive)")
    view.add_argument("--around", metavar="FRAME|MSms", help="lines near a frame (1234, f1234) or a run time (5000ms)")
    view.add_argument("--diff", nargs=2, metavar=("A", "B"), help="the summary change from run A to run B")
    ap.add_argument("--window", type=int, help="with --around: frames (default 30) or ms (default 2000) each side")
    ap.add_argument("--level", choices=list(LEVELS), help="the most verbose level shown (warn: errors and warnings)")
    ap.add_argument("--target", help="only these targets, comma separated (window,d3d9,sound,files,game,...)")
    ap.add_argument("--limit", type=int, default=60, help="most lines of --grep and --around (default 60)")
    ap.add_argument("--json", action="store_true", help="machine-readable output")
    ap.add_argument("--bundle", nargs="?", const="", metavar="FILE", help="write a small shareable text file")
    ap.add_argument("--color", choices=["auto", "always", "never"], default="auto")
    a = ap.parse_args(argv)
    paint = Paint(a.color == "always" or (a.color == "auto" and sys.stdout.isatty() and not os.environ.get("NO_COLOR")))
    runs = find_runs(a.dir or default_dir())

    if a.diff:
        ra, rb = pick(runs, a.diff[0]), pick(runs, a.diff[1])
        if not ra or not rb:
            print("logs: no such run", file=sys.stderr)
            return 2
        rows, regs, new, gone = diff(ra, rb)
        if a.json:
            emit_json({"a": ra["id"], "b": rb["id"], "rows": rows, "regressions": regs, "new_flags": new, "gone_flags": gone})
        else:
            print("\n".join(diff_view(ra, rb, paint)))
        return 1 if regs else 0

    run = pick(runs, a.run)
    if run is None:
        print(f"logs: no run found ({a.run or 'latest'}) in {a.dir or default_dir()}", file=sys.stderr)
        return 2
    recs = load_records(run)
    if not recs and not run["summary"]:
        print(f"logs: run {run['id']} has no readable log or summary", file=sys.stderr)
        return 2

    if a.bundle is not None:
        path = Path(a.bundle or f"nv-rs-bundle-{run['id']}.txt")
        path.write_text(bundle_text(run), encoding="utf-8")
        print(f"wrote {path} ({path.stat().st_size} bytes)")
        return 0

    if a.tail is not None or a.grep or a.around:
        sel = select(recs, a.level, a.target)
        if a.grep:
            try:
                rx = re.compile(a.grep, re.I)
            except re.error as e:
                print(f"logs: bad pattern: {e}", file=sys.stderr)
                return 2
            sel = [r for r in sel if rx.search(r["message"]) or rx.search(r.get("target", ""))]
            note = f"{len(sel)} matching lines" + (f", the first {a.limit}" if len(sel) > a.limit else "")
            sel = sel[: a.limit]
        elif a.around:
            sel = around(recs, a.around, a.window, paint, a.level, a.target)
            note = f"{len(sel)} lines near {a.around}" + (f", the first {a.limit}" if len(sel) > a.limit else "")
            sel = sel[: a.limit]
        else:
            sel = sel[-a.tail:] if a.tail else []
            note = None
        if a.json:
            emit_json(sel)
        else:
            for r in sel:
                print(fmt_record(r, paint))
            if note:
                print(paint("dim", note))
        return 0

    if a.errors:
        if a.json:
            emit_json({"id": run["id"], "stops": stops(recs), "problems": problems(recs)})
        else:
            print("\n".join(errors_view(run, paint)))
        return 0
    if a.hitches:
        if a.json:
            emit_json({"id": run["id"], "hitches": (run["summary"] or {}).get("hitches"), "worst": worst_hitches(run, 10)})
        else:
            print("\n".join(hitches_view(run, paint)))
        return 0
    if a.json:
        emit_json({"id": run["id"], "source": source_name(run), "summary": run["summary"], "stops": stops(recs),
                   "problems": problems(recs), "worst": worst_hitches(run), "files": [str(p) for p in (run.get("text"), run.get("jsonl"), run.get("summary_path")) if p]})
        return 0
    print("\n".join(report(run, paint)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
