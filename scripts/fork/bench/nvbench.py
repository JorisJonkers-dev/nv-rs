#!/usr/bin/env python3
"""nv-rs bench: scripted playthrough runs with frame-time, memory and stability metrics (docs/fork/BENCH.md).

  nv-rs-dev bench run <scenario> [--mode capped|uncapped] [--windowed --yes-windowed] [options]
  nv-rs-dev bench list
  nv-rs-dev bench report <run dir | summary.json>
  nv-rs-dev bench compare <baseline> <candidate> [--threshold key=pct ...]
  nv-rs-dev bench watch [latest|RUN|PID]   (live view of a nested run)
  nv-rs-dev bench record [latest|RUN|PID] / bench run --record, then bench replay RUN
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
import drivers  # noqa: E402
import guard  # noqa: E402
import lock  # noqa: E402
import parse  # noqa: E402
import report  # noqa: E402
import replay  # noqa: E402
import suite  # noqa: E402
import runner  # noqa: E402
import scenarios  # noqa: E402
import watch  # noqa: E402

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
    recorder = load_hitches(run_dir)
    if recorder:
        # The hitch recorder's own summary; metrics["hitches"] is the frame log's count.
        metrics["recorder"] = recorder
    checks = verdict_checks(metrics, plan, (run or {}).get("wall_s"))
    series_t = [r["t"] for r in samples]
    out = {
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
    out["headline"] = suite.headline(out)  # for `nv-rs-dev runs` and logs: the numbers first, in the summary itself
    return out


def load_hitches(run_dir):
    """The hitch recorder's run summary (`run-summary-*.json`, docs/fork/HITCHES.md) of a run directory, reduced to
    what the report and the verdict use; None when the build had no recorder or the run left none."""
    for p in sorted(Path(run_dir).glob("run-summary-*.json")):
        try:
            r = json.loads(p.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        h = r.get("hitches", {})
        return {
            "count": h.get("count", 0),
            "per_min": h.get("per_min", 0),
            "slow_windows": h.get("slow_windows", 0),
            "by_cause": h.get("by_cause", {}),
            "over_pct": r.get("over_pct", {}),
            "worst": r.get("worst", [])[:5],
            "flags": r.get("flags", []),
            "verdict": r.get("verdict", ""),
            "p99_ms": r.get("frame_ms", {}).get("p99", 0),
        }
    return None


# Sneaking walks slower than standing: the sneak check passes when the steps sneaking cover at most this share of the
# path of the same steps standing.
SNEAK_RATIO_MAX = 0.8


def verdict_checks(m, plan, run_wall=None):
    c = []

    def add(name, ok, detail=""):
        c.append({"name": name, "ok": bool(ok), "detail": detail})

    add("frames advanced", m["frames"] > 1, f"{m['frames']} frames")
    add("no stop lines", m["stops"] == 0, f"{m['stops']} stops" + (f": {m['stop_lines'][0]}" if m["stop_lines"] else ""))
    add("no wgpu errors", m["wgpu_errors"] == 0, f"{m['wgpu_errors']} errors")
    add("no panic", m["panics"] == 0, f"{m['panics']} panics")
    add("no memory or device-lost line", m["memory_failures"] == 0, f"{m['memory_failures']} lines")
    add("GPU memory watchdog quiet", not m["watchdog"], m["watchdog"] or "")
    add("memory guard quiet", not m.get("guard"), m.get("guard") or (m.get("caps") or ""))
    # The harness stops the game with SIGTERM at the end of a scenario: 143, 124 and a clean 0 are ends.
    add("viewer exit status", m["exit_status"] in (0, 124, 143, -15, None), f"exit {m['exit_status']}")
    for place, deg in (m.get("heading_change_deg") or {}).items():
        if deg is None:
            add(f"the camera turned ({place})", False, "a walk there moved under 30 units: the player did not walk")
        else:
            add(f"the camera turned ({place})", deg >= 15.0, f"the walk's heading changed {deg:.0f} degrees after the view was turned")
    for place, sn in (m.get("sneak") or {}).items():
        ok = bool(sn["ctrl"] and sn["ratio"] is not None and sn["ratio"] <= SNEAK_RATIO_MAX)
        detail = (f"Ctrl {'reached' if sn['ctrl'] else 'did not reach'} the game; the same steps went {sn['stand']:.0f} units "
                  f"standing and {sn['sneak']:.0f} sneaking" + (f" (ratio {sn['ratio']:.2f}, want {SNEAK_RATIO_MAX} or less)" if sn["ratio"] is not None else ""))
        add(f"Ctrl made the player sneak ({place})", ok, detail)
    if plan and plan.budget_s and run_wall is not None:
        add("within the wall-time budget (TOO_LONG)", run_wall <= plan.budget_s, f"{run_wall:.0f} s of {plan.budget_s:.0f} s, start-up and loading included")
    ck = m.get("clock") or {}
    if ck.get("ratio") is not None and ck.get("play_s", 0) >= 5:
        fps = (m.get("frame_time_play") or {}).get("fps") or 0
        add("game time keeps wall time", 0.85 <= ck["ratio"] <= 1.15,
            f"game time / wall time {ck['ratio']:.2f} at {fps:.0f} fps (a frame-rate-dependent game would be off at high fps)")
    if ck.get("settled") is not None and ck.get("play_s", 0) >= 5:
        add("the player landed (height settled)", ck["settled"], f"fell {ck['fall']:.0f} units from the top")
    h = m.get("recorder")
    if h is not None:
        # A run whose hitches pass the recorder's flag level is marked: a stutter is a failure of the run (the level
        # is NV_HITCH_FLAG_PER_MIN, 3 a minute by default; the flags are the recorder's own).
        bad = [f for f in h["flags"] if f in ("hitches_per_min_high", "p99_over_20ms", "slow_windows")]
        add("no hitch flags", not bad, f"{h['count']} hitches ({h['per_min']}/min)" + (f": {', '.join(bad)}" if bad else ""))
    for name, fn in (plan.checks if plan else []):
        ok, detail = fn(m)
        add(name, ok, detail)
    return c


def cmd_list(_a):
    for name, fn in sorted(scenarios.SCENARIOS.items()):
        probe = fn({"fps": 60.0})
        print(f"{name:12} {probe.description}")


def refuse(a):
    """Exit status when the window options are refused (checked before the lock is taken), else 0."""
    if a.cap:
        a.mode = "capped"
    if a.legacy and a.mode == "unlimited":
        a.mode = "capped"
    if a.scenario in drivers.DRIVERS and not a.legacy and not a.nested and not a.windowed:
        a.nested = "headless"  # the driver needs a window to send real input into; the session is never involved
    if a.nested:
        a.windowed = True
        if a.nested == "window" and not a.yes_windowed:
            print("--nested window shows a window in your session: needs --yes-windowed", file=sys.stderr)
            return 2
        a.yes_windowed = a.yes_windowed or a.nested == "headless"
    if a.windowed and not a.yes_windowed:
        print("refusing to run windowed without --yes-windowed (the window opens on your display; offscreen is the default)", file=sys.stderr)
        return 2
    return 0


# Exit status of a run the memory guard refused or that runs are halted for (guard.py).
HALTED = 4
VRAM_LIMIT_PCT_MAX = 70


def guard_refusal(a):
    """Why the memory guard refuses this run before anything starts, or None."""
    h = guard.halted()
    if h:
        return f"bench runs are halted since {h.get('time', '?')}: {h.get('reason')}. Look at the cause, then nv-rs-dev bench clear-halt"
    if a.vram_limit_pct is None or not 0 < a.vram_limit_pct <= VRAM_LIMIT_PCT_MAX:
        return f"--vram-limit-pct (NV_VRAM_LIMIT_PCT) can only be lowered: 1 to {VRAM_LIMIT_PCT_MAX}"
    return None


def cmd_run(a):
    """Runs one scenario holding the machine-wide bench lock (--no-lock for debugging runs that make no numbers)."""
    refused = refuse(a)
    if refused:
        return refused
    why = guard_refusal(a)
    if why:
        print(f"bench: refused: {why}", file=sys.stderr)
        return HALTED
    if a.no_lock:
        print("bench: no lock (--no-lock): the numbers of this run are not valid if anything else runs", file=sys.stderr)
        return run_scenario(a)
    bl = lock.BenchLock(wait_timeout=a.wait_timeout, out=lambda m: print(m, file=sys.stderr, flush=True))
    if not bl.acquire():
        print(f"bench: the lock was not free within {a.wait_timeout} s: held by {lock.describe(bl.read_holder())}", file=sys.stderr)
        return 3
    try:
        # Again under the lock: a run that ended while this one waited may have halted the bench.
        why = guard_refusal(a)
        if why:
            print(f"bench: refused: {why}", file=sys.stderr)
            return HALTED
        return run_scenario(a)
    finally:
        bl.release()


def run_scenario(a):
    viewer = a.viewer or os.environ.get("NV_BENCH_VIEWER") or str(Path.home() / ".cache/nv-rs/target/play/nv-viewer")
    game = a.game or os.environ.get("NV_BENCH_GAME") or str(GAME_DEFAULT)
    if not os.access(viewer, os.X_OK):
        print(f"no viewer at {viewer}: run nv-rs-dev build, or set --viewer", file=sys.stderr)
        return 2
    if not Path(game).is_dir():
        print(f"no game folder at {game}", file=sys.stderr)
        return 2
    fps = a.ref_fps or float(a.cap or (144 if a.mode == "capped" else 60))
    opts = {
        "mode": a.mode, "windowed": a.windowed, "viewer": viewer, "game": game, "preset": a.preset, "threads": a.threads,
        "resolution": a.resolution, "sound": a.sound, "mem": a.mem, "vram_cap_mib": a.vram_cap_mib, "gtt_cap_mib": a.gtt_cap_mib,
        "fps": fps, "timeout_s": a.timeout,
        "vram_limit_pct": a.vram_limit_pct, "shots": a.shots, "nested": a.nested, "record": a.record, "record_fps": a.record_fps, "record_format": a.record_format, "scale": a.scale,
        "cap": a.cap, "legacy": a.legacy, "perf": a.perf,
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
    if a.scale and not plan.self_scaled:
        plan.end_frame = int(plan.end_frame * a.scale)
        plan.wall_after_first_s *= a.scale
    run_dir = runner.run_dir_for(plan.name)
    print(f"bench {plan.name} ({a.mode}, {('nested ' + a.nested) if a.nested else ('windowed' if a.windowed else 'offscreen')}, VRAM limit {a.vram_limit_pct}%) -> {run_dir}")
    try:
        run = runner.launch(plan, opts, run_dir)
    except guard.Refused as e:
        print(f"bench: refused by the memory guard: {e}", file=sys.stderr)
        (run_dir / "refused.txt").write_text(f"{e}\n")
        return HALTED
    build = ""
    try:
        build = subprocess.run(["git", "-C", str(Path(viewer).resolve().parent), "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    except OSError:
        pass
    settings = {
        "mode": a.mode, "window": ("nested " + a.nested) if a.nested else ("windowed" if a.windowed else "offscreen"), "preset": a.preset, "threads": a.threads or "default",
        "resolution": a.resolution, "build": build or Path(viewer).name, "viewer_mtime": int(Path(viewer).stat().st_mtime),
        "vram_limit_pct": a.vram_limit_pct, "ref_fps": fps, "cap": a.cap, "caps": run.get("caps"),
        "watched": watch.was_watched(run_dir), "recorded": (run_dir / "record.json").exists(),
        "limiters": suite.limiters_in(plan.env(opts)), "budget_s": plan.budget_s or None,
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
    if p in ("suite", "latest:suite"):
        suites = sorted(d for d in runner.STATE.glob("suite-*T*") if d.is_dir())
        if suites:
            return suites[-1]
    runs = sorted((d for d in runner.STATE.glob("*-*T*") if d.is_dir() and not d.name.startswith("suite-") and (d / "summary.json").exists()), key=lambda d: d.name.rsplit("-", 1)[1])
    if p.startswith("latest"):
        want = p.split(":", 1)[1] if ":" in p else None
        runs = [d for d in runs if want is None or d.name.rsplit("-", 1)[0] == want]
        if runs:
            return runs[-1]
    if (runner.STATE / p).exists():
        return runner.STATE / p
    raise SystemExit(f"no such run: {p}")


def cmd_suite(a):
    """Runs the scenarios in turn (each holds the bench lock on its own), writes suite-<time>/ with suite.json and a
    combined report, and prints the headline table with the change against the previous suite and the baseline."""
    if a.quick:
        a.scenarios = "quick"
    names = [n.strip() for n in a.scenarios.split(",") if n.strip()]
    unknown = [n for n in names if n not in scenarios.SCENARIOS]
    if unknown:
        print(f"unknown scenarios: {', '.join(unknown)}", file=sys.stderr)
        return 2
    a.perf = True
    name = suite.new_suite_name()
    out = runner.STATE / name
    out.mkdir(parents=True, exist_ok=True)
    rows, failed = [], []
    halted = None
    for i, sc in enumerate(names):
        b = argparse.Namespace(**vars(a))
        b.scenario = sc
        status = cmd_run(b)
        if status == HALTED or guard.halted():
            # A refused or halted run stops the whole suite: no later scenario starts.
            halted = (guard.halted() or {}).get("reason") or "refused by the memory guard"
            failed.extend([f"{sc} (stopped)"] + names[i + 1:])
            break
        if status in (2, 3):
            failed.append(sc)
            continue
        try:
            d = resolve(f"latest:{sc}")
            summary = load_summary(d)
        except SystemExit:
            failed.append(sc)
            continue
        rows.append(suite.headline(summary))
        rows.extend(suite.phase_rows(summary))
    prev = suite.previous_suite(runner.STATE, name)
    base_name = a.baseline or "default"
    base = suite.load_baseline(runner.STATE, base_name)
    settings = {"mode": a.mode, "cap": a.cap, "resolution": a.resolution}
    md = suite.suite_markdown(name, rows, prev, base, a.baseline or ("default" if base else None), settings)
    (out / "suite.json").write_text(json.dumps({"name": name, "settings": settings, "rows": rows, "failed": failed}, indent=1))
    (out / "report.md").write_text(md)
    if a.save_baseline:
        suite.save_baseline(runner.STATE, a.save_baseline, rows)
    print(md)
    if failed:
        print(f"not run: {', '.join(failed)}", file=sys.stderr)
    if halted:
        print(f"suite stopped: {halted}", file=sys.stderr)
        print(f"suite: {out}")
        return HALTED
    print(f"suite: {out}")
    return 0 if rows and not failed and all(r["checks_ok"] and not r.get("too_long") for r in rows) else 1


def cmd_clear_halt(_a):
    h = guard.halted()
    if not h:
        print("bench runs are not halted")
        return 0
    print(f"halted since {h.get('time', '?')}: {h.get('reason')}")
    guard.clear_halt()
    print("cleared: bench runs may start again")
    return 0


def cmd_report(a):
    d = resolve(a.run)
    if Path(d).is_dir() and (Path(d) / "suite.json").exists():
        print((Path(d) / "report.md").read_text())
        return 0
    s = load_summary(d)
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


def run_args(r):
    """The options of `run`, shared with `suite`."""
    r.add_argument("--mode", choices=["unlimited", "capped", "peak"], default="unlimited", help="unlimited (default, as real play): no play limit, menu limit 120, Mailbox; capped: --cap N (default 144), Mailbox (pacing, 1%% lows); peak: Immediate present (peak fps)")
    r.add_argument("--cap", type=int, help="a play frame limit of N frames a second for pacing tests (implies --mode capped)")
    r.add_argument("--legacy", action="store_true", help="the frame-counted scripts (NV_MOVE_KEYS) offscreen at 144 frames a second instead of the wall-clock driver in the nested window")
    r.add_argument("--windowed", action="store_true", help="draw in a real window on your display (needs --yes-windowed)")
    r.add_argument("--yes-windowed", action="store_true")
    r.add_argument("--nested", choices=["headless", "window"], help="real window path in a nested sway: headless (own output, no session involved; valid fps and stability) or window (shown inside your session; can be covered). Implies a windowed run; no --yes-windowed needed for headless")
    r.add_argument("--no-lock", action="store_true", help="do not take the machine-wide bench lock (a debugging run that gives no numbers)")
    r.add_argument("--wait-timeout", type=float, default=None, help="give up (exit 3) when the bench lock is not free after this many seconds (default: wait)")
    r.add_argument("--viewer")
    r.add_argument("--game")
    r.add_argument("--preset", choices=["fixes", "vanilla"], default="fixes")
    r.add_argument("--threads", choices=["real", "stepped"])
    r.add_argument("--resolution", default="1920x1080")
    r.add_argument("--sound", choices=["out", "device", "none", "silent"], help="silent (default): the sound runs, the speakers get nothing; out: audible; none: no sound device at all")
    r.add_argument("--mem", default=os.environ.get("NV_BENCH_MEM") or None,
                   help="lower the host memory cap of the run (systemd scope; the default is half the available memory, 12G at most)")
    r.add_argument("--vram-cap-mib", type=int, default=None, help="lower the run's VRAM cap (default: half the card's VRAM)")
    r.add_argument("--gtt-cap-mib", type=int, default=None, help="lower the run's GTT cap (default: a quarter of the GTT)")
    r.add_argument("--vram-limit-pct", type=int, default=int(os.environ.get("NV_VRAM_LIMIT_PCT", str(VRAM_LIMIT_PCT_MAX))),
                   help=f"also stop the game when the card's whole VRAM or GTT use passes this percent ({VRAM_LIMIT_PCT_MAX} at most)")
    r.add_argument("--timeout", type=float, default=7200.0, help="hard wall limit in seconds")
    r.add_argument("--ref-fps", type=float, help="frames per second the scenario's seconds are converted at")
    r.add_argument("--frames", type=int, help="override: stop at this main frame")
    r.add_argument("--seconds", type=float, help="override: stop this long after the first frame")
    r.add_argument("--scale", type=float, help="scale the scenario's length (soak: 0.1 for a short test)")
    r.add_argument("--shots", help="debugging: save the game's frames (NV_SHOT_WGPU=DIR[,every_ms]) for you to look at; never commit them")
    r.add_argument("--record", action="store_true", help="record the nested screen to screen.mp4 in the run directory (needs --nested headless and wf-recorder); then: bench replay RUN")
    r.add_argument("--record-fps", type=int, default=30)
    r.add_argument("--record-format", choices=["mp4", "webm"], default="mp4")
    r.add_argument("--force", action="store_true", help="run a capped-only scenario uncapped")
    r.add_argument("--mouse", help="extra NV_MOVE_MOUSE items FRAME:DX:DY:HOLD[:BUTTONS],...")
    r.add_argument("--keys", help="extra NV_MOVE_KEYS items FRAME:DIKHEX:HOLD,...")

    r.add_argument("--perf", action="store_true", help="NV_PERF=1: the busiest thread's CPU share (the suite sets it)")


def main(argv=None):
    ap = argparse.ArgumentParser(prog="nv-rs-dev bench", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list").set_defaults(fn=cmd_list)
    r = sub.add_parser("run")
    r.add_argument("scenario")
    run_args(r)
    r.set_defaults(fn=cmd_run)
    su = sub.add_parser("suite", help="scenarios back to back, one headline table")
    su.add_argument("--quick", action="store_true", help="the quick suite: Doc's house, Goodsprings and the Strip in one viewer session, 120 s in all (TOO_LONG over that)")
    su.add_argument("--scenarios", default="doc_house,goodsprings,strip", help="comma separated (soak is 20 minutes and is not in the default)")
    su.add_argument("--save-baseline", metavar="NAME", help="store this suite's headline rows as a baseline")
    su.add_argument("--baseline", metavar="NAME", help="compare with a stored baseline (default: the baseline named 'default' if there is one)")
    run_args(su)
    su.set_defaults(fn=cmd_suite)
    sub.add_parser("clear-halt", help="allow bench runs again after the memory guard halted them (look at the cause first)").set_defaults(fn=cmd_clear_halt)
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
    watch.register(sub, runner.STATE, resolve, replay)
    a = ap.parse_args(argv)
    return a.fn(a) or 0


if __name__ == "__main__":
    sys.exit(main())
