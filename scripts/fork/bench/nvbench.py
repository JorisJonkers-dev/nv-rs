#!/usr/bin/env python3
"""nv-rs bench: scripted playthrough runs with frame-time, memory and stability metrics (docs/fork/BENCH.md).

  nv-rs-dev bench run <scenario> [--mode capped|uncapped] [--windowed --yes-windowed] [options]
  nv-rs-dev bench list
  nv-rs-dev bench report <run dir | summary.json>
  nv-rs-dev bench compare <baseline> <candidate> [--threshold key=pct ...]
  nv-rs-dev bench analyze <viewer.log> [--frames frames.tsv] [--samples samples.tsv]   (parse an existing run)

Runs write ~/.local/state/nv-rs/bench/<scenario>-<time>/ (NV_BENCH_DIR changes it): numbers and logs only.
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import parse  # noqa: E402
import report  # noqa: E402
import runner  # noqa: E402
import scenarios  # noqa: E402

GAME_DEFAULT = Path.home() / "Games/SteamLibrary/steamapps/common/Fallout New Vegas"


def read(p):
    try:
        return Path(p).read_text(errors="replace")
    except OSError:
        return ""


def analyze_dir(run_dir, plan=None, run=None, settings=None):
    """Build summary.json's content from the files of a run directory."""
    run_dir = Path(run_dir)
    log = parse.parse_log(read(run_dir / "viewer.log"))
    rows = parse.parse_frame_log(read(run_dir / "frames.tsv"))
    samples = parse.parse_samples(read(run_dir / "samples.tsv"))
    missing = parse.count_missing(read(run_dir / "missing.tsv"))
    stops = parse.count_stops(read(run_dir / "missing.stops.tsv"))
    metrics = parse.summarize(rows, log, samples, missing, stops)
    checks = verdict_checks(metrics, plan)
    series_t = [r["t"] for r in samples]
    return {
        "name": run_dir.name,
        "scenario": (run or {}).get("scenario") or run_dir.name.rsplit("-", 1)[0],
        "settings": settings or {},
        "run": run or {"ended_by": "unknown"},
        "metrics": metrics,
        "verdict": {"ok": all(c["ok"] for c in checks), "checks": checks},
        "series_t": series_t,
        "series": {
            "rss": [r["rss_mib"] for r in samples],
            "vram": [r["vram_mib"] for r in samples],
            "gtt": [r["gtt_mib"] for r in samples],
        },
    }


def verdict_checks(m, plan):
    c = []

    def add(name, ok, detail=""):
        c.append({"name": name, "ok": bool(ok), "detail": detail})

    add("frames advanced", m["frames"] > 1, f"{m['frames']} frames")
    add("no stop lines", m["stops"] == 0, f"{m['stops']} stops" + (f": {m['stop_lines'][0]}" if m["stop_lines"] else ""))
    add("no wgpu errors", m["wgpu_errors"] == 0, f"{m['wgpu_errors']} errors")
    add("no panic", m["panics"] == 0, f"{m['panics']} panics")
    add("no memory or device-lost line", m["memory_failures"] == 0, f"{m['memory_failures']} lines")
    add("GPU memory watchdog quiet", not m["watchdog"], m["watchdog"] or "")
    # The harness stops the game with SIGTERM at the end of a scenario: 143, 124 and a clean 0 are ends.
    add("viewer exit status", m["exit_status"] in (0, 124, 143, -15, None), f"exit {m['exit_status']}")
    for name, fn in (plan.checks if plan else []):
        ok, detail = fn(m)
        add(name, ok, detail)
    return c


def cmd_list(_a):
    for name, fn in sorted(scenarios.SCENARIOS.items()):
        probe = fn({"fps": 60.0})
        print(f"{name:12} {probe.description}")


def cmd_run(a):
    if a.windowed and not a.yes_windowed:
        print("refusing to run windowed without --yes-windowed (the window opens on your display; offscreen is the default)", file=sys.stderr)
        return 2
    viewer = a.viewer or os.environ.get("NV_BENCH_VIEWER") or str(Path.home() / ".cache/nv-rs/target/play/nv-viewer")
    game = a.game or os.environ.get("NV_BENCH_GAME") or str(GAME_DEFAULT)
    if not os.access(viewer, os.X_OK):
        print(f"no viewer at {viewer}: run nv-rs-dev build, or set --viewer", file=sys.stderr)
        return 2
    if not Path(game).is_dir():
        print(f"no game folder at {game}", file=sys.stderr)
        return 2
    fps = a.ref_fps or (144.0 if a.mode == "capped" else 60.0)
    opts = {
        "mode": a.mode, "windowed": a.windowed, "viewer": viewer, "game": game, "preset": a.preset, "threads": a.threads,
        "resolution": a.resolution, "sound": a.sound, "mem": a.mem, "fps": fps, "timeout_s": a.timeout,
        "vram_limit_pct": a.vram_limit_pct, "shots": a.shots,
    }
    plan = scenarios.build(a.scenario, opts)
    if plan.capped_only and a.mode != "capped" and not a.force:
        print(f"{plan.name} is timed in main-loop frames and only valid in capped mode (the game's menus run on time); --force to try", file=sys.stderr)
        return 2
    if a.keys:
        plan.keys.extend(parse_keys(a.keys))
    if a.mouse:
        plan.mouse.extend(tuple(int(x) for x in i.split(":")) + (0,) * (5 - len(i.split(":"))) for i in a.mouse.split(","))
    if a.frames:
        plan.end_frame = a.frames
    if a.seconds:
        plan.wall_after_first_s = a.seconds
    if a.scale:
        plan.end_frame = int(plan.end_frame * a.scale)
        plan.wall_after_first_s *= a.scale
    run_dir = runner.run_dir_for(plan.name)
    print(f"bench {plan.name} ({a.mode}, {'windowed' if a.windowed else 'offscreen'}, VRAM limit {a.vram_limit_pct}%) -> {run_dir}")
    run = runner.launch(plan, opts, run_dir)
    build = ""
    try:
        build = subprocess.run(["git", "-C", str(Path(viewer).resolve().parent), "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    except OSError:
        pass
    settings = {
        "mode": a.mode, "window": "windowed" if a.windowed else "offscreen", "preset": a.preset, "threads": a.threads or "default",
        "resolution": a.resolution, "build": build or Path(viewer).name, "viewer_mtime": int(Path(viewer).stat().st_mtime),
        "vram_limit_pct": a.vram_limit_pct, "ref_fps": fps,
    }
    summary = analyze_dir(run_dir, plan, run, settings)
    write_reports(run_dir, summary)
    print((run_dir / "report.md").read_text())
    print(f"report: {run_dir / 'report.html'}")
    return 0 if summary["verdict"]["ok"] else 1


def parse_keys(spec):
    out = []
    for item in spec.split(","):
        f, k, *h = item.split(":")
        out.append((int(f), int(k, 16), int(h[0]) if h else 1))
    return out


def write_reports(run_dir, summary):
    run_dir = Path(run_dir)
    (run_dir / "summary.json").write_text(json.dumps(summary, indent=1))
    (run_dir / "report.md").write_text(report.markdown(summary))
    (run_dir / "report.html").write_text(report.page(summary))


def load_summary(p):
    p = Path(p)
    if p.is_dir():
        if (p / "summary.json").exists():
            p = p / "summary.json"
        else:
            return analyze_dir(p)
    return json.loads(p.read_text())


def resolve(p):
    """A run directory, a summary.json, or a name or `latest[:scenario]` under the bench directory."""
    q = Path(p)
    if q.exists():
        return q
    runs = sorted((d for d in runner.STATE.glob("*-*T*") if d.is_dir()), key=lambda d: d.name.rsplit("-", 1)[1])
    if p.startswith("latest"):
        want = p.split(":", 1)[1] if ":" in p else None
        runs = [d for d in runs if want is None or d.name.rsplit("-", 1)[0] == want]
        if runs:
            return runs[-1]
    if (runner.STATE / p).exists():
        return runner.STATE / p
    raise SystemExit(f"no such run: {p}")


def cmd_report(a):
    s = load_summary(resolve(a.run))
    print(report.markdown(s))
    return 0


def cmd_compare(a):
    sa, sb = load_summary(resolve(a.baseline)), load_summary(resolve(a.candidate))
    th = {}
    for t in a.threshold or []:
        k, v = t.split("=")
        th[k] = float(v)
    rows, ok = report.compare(sa, sb, th)
    text = report.compare_markdown(sa, sb, rows, ok)
    print(text)
    if a.out:
        Path(a.out).write_text(text)
    return 0 if ok else 1


def cmd_analyze(a):
    d = Path(a.log).resolve()
    # analyze a loose log as a run directory of one file: copy nothing, point the readers at the files given
    tmp = d.parent
    summary = analyze_dir(tmp) if d.name == "viewer.log" else None
    if summary is None:
        log = parse.parse_log(read(d))
        rows = parse.parse_frame_log(read(a.frames)) if a.frames else []
        samples = parse.parse_samples(read(a.samples)) if a.samples else []
        metrics = parse.summarize(rows, log, samples)
        checks = verdict_checks(metrics, None)
        summary = {"name": d.stem, "scenario": a.scenario or "log", "settings": {}, "run": {"ended_by": "analyzed"}, "metrics": metrics,
                   "verdict": {"ok": all(c["ok"] for c in checks), "checks": checks}, "series_t": [r["t"] for r in samples],
                   "series": {"rss": [r["rss_mib"] for r in samples], "vram": [r["vram_mib"] for r in samples], "gtt": [r["gtt_mib"] for r in samples]}}
    print(report.markdown(summary))
    if a.out:
        write_reports(a.out, summary)
    return 0 if summary["verdict"]["ok"] else 1


def main(argv=None):
    ap = argparse.ArgumentParser(prog="nv-rs-dev bench", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list").set_defaults(fn=cmd_list)
    r = sub.add_parser("run")
    r.add_argument("scenario")
    r.add_argument("--mode", choices=["capped", "uncapped"], default="capped", help="capped: 144 Hz limit, mailbox present (pacing, 1%% lows); uncapped: no limit, immediate present (peak fps)")
    r.add_argument("--windowed", action="store_true", help="draw in a real window on your display (needs --yes-windowed)")
    r.add_argument("--yes-windowed", action="store_true")
    r.add_argument("--viewer")
    r.add_argument("--game")
    r.add_argument("--preset", choices=["fixes", "vanilla"], default="fixes")
    r.add_argument("--threads", choices=["real", "stepped"])
    r.add_argument("--resolution", default="1920x1080")
    r.add_argument("--sound", choices=["out", "device", "none", "silent"], help="silent (default): the sound runs, the speakers get nothing; out: audible; none: no sound device at all")
    r.add_argument("--mem", default=os.environ.get("NV_BENCH_MEM", "12G"), help="host memory cap of the run (systemd scope); empty for none")
    r.add_argument("--vram-limit-pct", type=int, default=int(os.environ.get("NV_VRAM_LIMIT_PCT", "70")), help="stop the game when the card's VRAM or GTT use passes this percent (0: off)")
    r.add_argument("--timeout", type=float, default=7200.0, help="hard wall limit in seconds")
    r.add_argument("--ref-fps", type=float, help="frames per second the scenario's seconds are converted at")
    r.add_argument("--frames", type=int, help="override: stop at this main frame")
    r.add_argument("--seconds", type=float, help="override: stop this long after the first frame")
    r.add_argument("--scale", type=float, help="scale the scenario's length (soak: 0.1 for a short test)")
    r.add_argument("--shots", help="debugging: save the game's frames (NV_SHOT_WGPU=DIR[,every_ms]) for you to look at; never commit them")
    r.add_argument("--force", action="store_true", help="run a capped-only scenario uncapped")
    r.add_argument("--mouse", help="extra NV_MOVE_MOUSE items FRAME:DX:DY:HOLD[:BUTTONS],...")
    r.add_argument("--keys", help="extra NV_MOVE_KEYS items FRAME:DIKHEX:HOLD,...")
    r.set_defaults(fn=cmd_run)
    p = sub.add_parser("report")
    p.add_argument("run")
    p.set_defaults(fn=cmd_report)
    c = sub.add_parser("compare")
    c.add_argument("baseline")
    c.add_argument("candidate")
    c.add_argument("--threshold", action="append", help="key=percent, e.g. fps=3 (keys: " + ", ".join(k[0] for k in report.COMPARED) + ")")
    c.add_argument("--out")
    c.set_defaults(fn=cmd_compare)
    an = sub.add_parser("analyze")
    an.add_argument("log")
    an.add_argument("--frames")
    an.add_argument("--samples")
    an.add_argument("--scenario")
    an.add_argument("--out")
    an.set_defaults(fn=cmd_analyze)
    a = ap.parse_args(argv)
    return a.fn(a) or 0


if __name__ == "__main__":
    sys.exit(main())
