#!/usr/bin/env python3
"""nv-rs-dev runs: the recent real runs, from the per-run summaries the viewer writes
(`run-summary-<unix>-<pid>.json` in ~/.local/state/nv-rs/logs; docs/fork/HITCHES.md).

    runs.py [--last N] [--flagged] [--json] [--dir DIR]
    runs.py show <id|latest|path> [--json] [--dir DIR]

Each row has fps, p99, hitches a minute, the flags, and the change against the run before it
(the previous run in time, flagged or not). Standard library only.
"""
import argparse
import json
import os
import re
import sys
from pathlib import Path

PATTERN = re.compile(r"^run-summary-(\d+)-(\d+)\.json$")


def default_dir():
    if os.environ.get("NV_LOG_DIR"):
        return Path(os.environ["NV_LOG_DIR"])
    state = os.environ.get("XDG_STATE_HOME") or str(Path.home() / ".local/state")
    return Path(state) / "nv-rs" / "logs"


def pid_alive(pid):
    return Path(f"/proc/{pid}").exists() if sys.platform.startswith("linux") else True


def load_runs(directory):
    """Every readable summary, oldest first. A summary left as `running` by a process that is gone is an
    unclean end (a kill -9, a crash of the machine): it is flagged `unclean_exit`."""
    runs = []
    for p in Path(directory).glob("run-summary-*.json"):
        m = PATTERN.match(p.name)
        if not m:
            continue
        try:
            r = json.loads(p.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        r["_path"] = str(p)
        r["_unix"] = int(m.group(1))
        r["_pid"] = int(m.group(2))
        if r.get("ended") == "running" and not pid_alive(r["_pid"]) and "unclean_exit" not in r.get("flags", []):
            r.setdefault("flags", []).append("unclean_exit")
        runs.append(r)
    runs.sort(key=lambda r: (r["_unix"], r["_pid"]))
    return runs


def row(r, prev=None):
    f = r.get("frame_ms", {})
    h = r.get("hitches", {})
    out = {
        "id": r.get("id"),
        "started_unix": r.get("started_unix"),
        "ended": r.get("ended"),
        "play_s": r.get("play_s", 0),
        "fps": f.get("fps", 0),
        "p99_ms": f.get("p99", 0),
        "hitches_per_min": h.get("per_min", 0),
        "hitches": h.get("count", 0),
        "flags": r.get("flags", []),
        "verdict": r.get("verdict", ""),
        "trend": None,
    }
    if prev is not None:
        pf, ph = prev.get("frame_ms", {}), prev.get("hitches", {})
        out["trend"] = {
            "vs": prev.get("id"),
            "fps": round(out["fps"] - pf.get("fps", 0), 1),
            "p99_ms": round(out["p99_ms"] - pf.get("p99", 0), 1),
            "hitches_per_min": round(out["hitches_per_min"] - ph.get("per_min", 0), 2),
        }
    return out


def rows(runs, last=None, flagged=False):
    table = [row(r, runs[i - 1] if i else None) for i, r in enumerate(runs)]
    if flagged:
        table = [t for t in table if t["flags"]]
    return table[-last:] if last else table


def arrow(v, worse_when_up):
    if v == 0:
        return "="
    up = v > 0
    return ("+" if up else "-") + f"{abs(v):g}" + ("!" if up == worse_when_up else "")


def render(table):
    if not table:
        return "no runs"
    lines = [f"{'id':<20}{'play s':>7}{'fps':>7}{'p99 ms':>8}{'hitch/min':>10}  {'vs previous (fps, p99, hitch/min)':<36}flags"]
    for t in table:
        tr = t["trend"]
        trend = "-" if not tr else f"{arrow(tr['fps'], False)} {arrow(tr['p99_ms'], True)} {arrow(tr['hitches_per_min'], True)}"
        lines.append(
            f"{t['id']:<20}{t['play_s']:>7.0f}{t['fps']:>7.1f}{t['p99_ms']:>8.1f}{t['hitches_per_min']:>10.2f}  {trend:<36}"
            + (",".join(t["flags"]) or "ok")
        )
    return "\n".join(lines)


def find(runs, key):
    if key == "latest":
        return runs[-1] if runs else None
    if os.path.isfile(key):
        try:
            return json.loads(Path(key).read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return None
    for r in reversed(runs):
        if r.get("id") == key or r.get("id", "").startswith(key) or r["_path"].endswith(key):
            return r
    return None


def show(r):
    f, h = r.get("frame_ms", {}), r.get("hitches", {})
    lines = [
        f"run {r.get('id')}  ({r.get('ended')})  log {r.get('log')}",
        f"verdict: {r.get('verdict')}",
        "meta: " + ", ".join(f"{k}={v}" for k, v in r.get("meta", {}).items()),
        f"duration {r.get('duration_s')} s, play {r.get('play_s')} s, loading {r.get('loading_s')} s, {r.get('frames')} frames",
        "frame ms: " + ", ".join(f"{k} {v}" for k, v in f.items()),
        "frames over (%): " + ", ".join(f"{k} ms {v}" for k, v in r.get("over_pct", {}).items()),
        f"hitches: {h.get('count')} ({h.get('per_min')}/min), slow windows {h.get('slow_windows')}; by cause: "
        + ", ".join(f"{k} x{v['n']} ({v['ms']} ms)" for k, v in h.get("by_cause", {}).items()),
        "peaks: " + ", ".join(f"{k}={v}" for k, v in r.get("peaks", {}).items()),
        f"soft stops {r.get('soft_stops')}, errors {r.get('errors')}, warnings {r.get('warnings')}, other viewers {r.get('other_viewers')}",
        "flags: " + (", ".join(r.get("flags", [])) or "none"),
    ]
    for w in r.get("worst", []):
        lines.append(
            f"  worst {w['wall_ms']} ms at frame {w['frame']} ({w['run_ms'] / 1000:.0f} s): {w['cause']}, trigger {w['trigger']}, top {', '.join(w.get('top', [])) or '-'}"
        )
    return "\n".join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(prog="nv-rs-dev runs", description=__doc__.split("\n")[0])
    ap.add_argument("what", nargs="*", help="`show <id|latest|path>`, or nothing to list")
    ap.add_argument("--last", type=int, help="only the newest N")
    ap.add_argument("--flagged", action="store_true", help="only runs with flags")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--dir", help="the log folder (default ~/.local/state/nv-rs/logs, NV_LOG_DIR)")
    a = ap.parse_args(argv)
    runs = load_runs(a.dir or default_dir())
    if a.what[:1] == ["show"]:
        r = find(runs, a.what[1] if len(a.what) > 1 else "latest")
        if r is None:
            print("runs: no such run", file=sys.stderr)
            return 2
        print(json.dumps(r) if a.json else show(r))
        return 0
    if a.what:
        print("usage: nv-rs-dev runs [--last N] [--flagged] [--json] | show <id|latest|path>", file=sys.stderr)
        return 2
    table = rows(runs, a.last, a.flagged)
    print(json.dumps(table) if a.json else render(table))
    return 0


if __name__ == "__main__":
    sys.exit(main())
