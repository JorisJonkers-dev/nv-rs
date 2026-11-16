"""`nv-rs-dev bench suite`: scenarios back to back, one headline table, deltas against the previous suite and a stored
baseline (docs/fork/BENCH.md). Pure functions here; nvbench.py runs the scenarios (one bench-lock hold each)."""
import json
import time
from pathlib import Path


CAP_VALUES = (24, 30, 48, 50, 60, 72, 75, 90, 100, 120, 144, 165, 240)


def limiters_in(env):
    """The frame limiters an environment asks for: variable -> value, for every limit that is not 0."""
    out = {}
    for k in ("NV_FRAME_LIMIT", "NV_FPS_LIMIT", "NV_D3D9_FRAME_LIMIT"):
        v = (env.get(k) or "").strip()
        if v and v not in ("0", "off", "none"):
            out[k] = v
    return out


def capped_reason(settings, fps):
    """Why a run's fps is not a headline, or None: a limiter was set, --cap was asked, or the measured fps sits within 2%
    of a usual cap value (a limiter that was not in the environment: the display, a compositor, the driver)."""
    lim = settings.get("limiters") or {}
    if lim:
        return "limiter set: " + ", ".join(f"{k}={v}" for k, v in sorted(lim.items()))
    if settings.get("mode") == "capped":
        return "--cap"
    if fps:
        for c in CAP_VALUES:
            if abs(fps - c) / c <= 0.02:
                return f"measured {fps:.0f} fps is within 2% of the {c} fps cap value"
    return None


def too_long(summary):
    """Whether the run went over its wall-time budget (the world scenarios: 120 s in all, start-up included)."""
    budget = summary["settings"].get("budget_s")
    wall = summary["run"].get("wall_s")
    return bool(budget and wall and wall > budget)


def phase_rows(summary):
    """One headline row for each measured phase of a driven run (a place of the quick suite), with the phase's own fps."""
    base = headline(summary)
    out = []
    for ph in (summary["metrics"].get("phases") or []):
        if ph["name"].endswith(".measured") and ph.get("fps"):
            place = ph["name"].split(".")[0]
            out.append(dict(base, scenario=f"{summary['scenario']}:{place}", fps=ph["fps"], p50_ms=None, p99_ms=ph.get("p99_ms"),
                            low1_fps=ph.get("low1_fps"), low01_fps=None, busy_ms=None, hitches_per_min=None, wall_s=ph.get("wall_s")))
    return out


def headline(summary):
    """The numbers a person wants first, from one run's summary.json (play frames only, loading screens left out)."""
    m = summary["metrics"]
    play = m.get("frame_time_play") or {}
    avg = play.get("avg_ms")
    busy = m.get("main_busy_pct")
    mem = m.get("memory") or {}
    return {
        "scenario": summary["scenario"],
        "run": summary["name"],
        "mode": summary["settings"].get("mode"),
        "window": summary["settings"].get("window"),
        "fps": play.get("fps"),
        "p50_ms": play.get("p50_ms"),
        "p99_ms": play.get("p99_ms"),
        "low1_fps": play.get("low1_fps"),
        "low01_fps": play.get("low01_fps"),
        "busy_ms": round(busy / 100.0 * avg, 2) if busy is not None and avg else None,
        "hitches_per_min": (m.get("hitches") or {}).get("per_min"),
        "vram_peak_mib": mem.get("vram_peak_mib"),
        "capped": capped_reason(summary["settings"], play.get("fps")),
        "wall_s": summary["run"].get("wall_s"),
        "too_long": too_long(summary),
        "verdict": "TOO_LONG" if too_long(summary) else ("CAPPED" if capped_reason(summary["settings"], play.get("fps")) else ("PASS" if summary["verdict"]["ok"] else "FAIL")),
        "checks_ok": bool(summary["verdict"]["ok"]),
        "noisy": bool(summary["run"].get("noisy")),
    }


COLUMNS = [
    ("scenario", "Scenario", "{}"),
    ("fps", "avg fps", "{:.0f}"),
    ("p50_ms", "p50 ms", "{:.2f}"),
    ("p99_ms", "p99 ms", "{:.2f}"),
    ("low1_fps", "1% low", "{:.0f}"),
    ("low01_fps", "0.1% low", "{:.0f}"),
    ("busy_ms", "main busy ms/frame", "{:.2f}"),
    ("hitches_per_min", "hitches/min", "{:.1f}"),
    ("vram_peak_mib", "VRAM peak MiB", "{:.0f}"),
    ("verdict", "verdict", "{}"),
]


def cell(row, key, fmt):
    v = row.get(key)
    return "n/a" if v is None else fmt.format(v)


def delta_pct(new, old):
    if new is None or old is None or old == 0:
        return None
    return (new - old) / old * 100.0


def table(rows, previous=None, baseline=None):
    """Markdown headline table; with `previous` and `baseline` (dicts scenario -> row) the fps change against each is
    added, and a run that shared the machine is marked NOISY."""
    head = [c[1] for c in COLUMNS] + ["noisy"]
    if previous is not None:
        head.append("fps vs previous")
    if baseline is not None:
        head.append("fps vs baseline")
    out = ["| " + " | ".join(head) + " |", "| " + " | ".join("---" for _ in head) + " |"]
    for r in rows:
        cells = [cell(r, k, f) for k, _, f in COLUMNS] + ["NOISY" if r.get("noisy") else ""]
        for other in (previous, baseline):
            if other is None:
                continue
            d = delta_pct(r.get("fps"), (other.get(r["scenario"]) or {}).get("fps"))
            cells.append("n/a" if d is None else f"{d:+.1f}%")
        out.append("| " + " | ".join(cells) + " |")
    return "\n".join(out)


def baselines_dir(state):
    return Path(state) / "baselines"


def save_baseline(state, name, rows):
    d = baselines_dir(state)
    d.mkdir(parents=True, exist_ok=True)
    (d / f"{name}.json").write_text(json.dumps({r["scenario"]: r for r in rows}, indent=1))


def load_baseline(state, name):
    try:
        return json.loads((baselines_dir(state) / f"{name}.json").read_text())
    except (OSError, ValueError):
        return None


def previous_suite(state, before):
    """The rows (scenario -> row) of the newest suite directory older than `before`'s name, or None."""
    dirs = sorted(d for d in Path(state).glob("suite-*T*") if d.is_dir() and d.name != before and (d / "suite.json").exists())
    dirs = [d for d in dirs if d.name < before]
    if not dirs:
        return None
    try:
        return {r["scenario"]: r for r in json.loads((dirs[-1] / "suite.json").read_text())["rows"]}
    except (OSError, ValueError, KeyError):
        return None


def suite_markdown(name, rows, previous, baseline, baseline_name, settings):
    out = [f"# Bench suite {name}", "", f"Mode {settings.get('mode')}, cap {settings.get('cap') or 'none (as real play)'}, {settings.get('resolution')}.", ""]
    out.append(table(rows, previous, baseline))
    for r in rows:
        if r.get("capped"):
            out += ["", f"CAPPED: {r['scenario']} ({r['capped']}): its fps is not a headline number."]
    if any(r.get("noisy") for r in rows):
        out += ["", "NOISY: other nv-viewer processes ran during the marked scenarios; their numbers are not valid."]
    if baseline_name:
        out += ["", f"Baseline: {baseline_name}" + ("" if baseline else " (not found)")]
    return "\n".join(out) + "\n"


def new_suite_name():
    return "suite-" + time.strftime("%Y%m%dT%H%M%S")
