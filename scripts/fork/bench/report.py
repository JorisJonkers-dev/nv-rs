"""Reports and run comparison: markdown with text sparklines, HTML with inline SVG charts. Numbers only."""
import html

SPARKS = "▁▂▃▄▅▆▇█"

# metric key -> (label, path into summary["metrics"], unit, higher is better, default regression threshold in %,
#                 absolute floor below which a change is noise)
COMPARED = [
    ("fps", "Average fps (all frames)", ("frame_time", "fps"), "fps", True, 5.0, 1.0),
    ("fps_play", "Average fps (play screen)", ("frame_time_play", "fps"), "fps", True, 5.0, 1.0),
    ("low1", "1% low fps", ("frame_time", "low1_fps"), "fps", True, 10.0, 1.0),
    ("p99", "Frame time p99", ("frame_time", "p99_ms"), "ms", False, 10.0, 0.3),
    ("max_ms", "Frame time max", ("frame_time", "max_ms"), "ms", False, 50.0, 5.0),
    ("rss", "RSS peak", ("memory", "rss_peak_mib"), "MiB", False, 10.0, 50.0),
    ("rss_growth", "RSS growth (second half)", ("memory", "rss_growth_mib_per_min"), "MiB/min", False, 50.0, 2.0),
    ("vram", "VRAM peak", ("memory", "vram_peak_mib"), "MiB", False, 10.0, 100.0),
    ("gtt", "GTT peak", ("memory", "gtt_peak_mib"), "MiB", False, 10.0, 100.0),
    ("wgpu_errors", "wgpu errors", ("wgpu_errors",), "", False, 0.0, 0.0),
    ("stops", "Stops", ("stops",), "", False, 0.0, 0.0),
    ("untranslated", "Untranslated addresses", ("untranslated_addresses",), "", False, 0.0, 0.0),
    ("skipped", "Skipped draws", ("skipped_draws",), "", False, 20.0, 50.0),
]


def dig(d, path):
    for k in path:
        if not isinstance(d, dict) or k not in d:
            return None
        d = d[k]
    return d


def sparkline(values, width=60):
    vals = [v for v in values if v is not None]
    if not vals:
        return ""
    if len(vals) > width:  # average into `width` cells
        step = len(vals) / width
        vals = [sum(vals[int(i * step): max(int((i + 1) * step), int(i * step) + 1)]) / max(1, len(vals[int(i * step): max(int((i + 1) * step), int(i * step) + 1)])) for i in range(width)]
    lo, hi = min(vals), max(vals)
    if hi == lo:
        return SPARKS[3] * len(vals)
    return "".join(SPARKS[min(7, int((v - lo) / (hi - lo) * 8))] for v in vals)


def fmt(v, nd=1):
    if v is None:
        return "n/a"
    if isinstance(v, float):
        return f"{v:.{nd}f}"
    return str(v)


# ---------------------------------------------------------------------------------------------
# markdown


def headline_line(summary):
    import suite
    h = suite.headline(summary)
    parts = [f"{suite.cell(h, 'fps', '{:.0f}')} fps average", f"p50 {suite.cell(h, 'p50_ms', '{:.2f}')} ms", f"p99 {suite.cell(h, 'p99_ms', '{:.2f}')} ms",
             f"1% low {suite.cell(h, 'low1_fps', '{:.0f}')} fps", f"0.1% low {suite.cell(h, 'low01_fps', '{:.0f}')} fps"]
    if h["busy_ms"] is not None:
        parts.append(f"main thread busy {h['busy_ms']:.2f} ms/frame")
    if h["hitches_per_min"] is not None:
        parts.append(f"{h['hitches_per_min']:.1f} hitches/min")
    if h["vram_peak_mib"] is not None:
        parts.append(f"VRAM peak {h['vram_peak_mib']:.0f} MiB")
    if h["capped"]:
        return f"CAPPED ({h['capped']}): fps is not a headline for this run. Play screen: " + ", ".join(parts) + "."
    return f"Headline ({h['mode']}, no frame limit, play screen): " + ", ".join(parts) + "."


def markdown(summary):
    m, v = summary["metrics"], summary["verdict"]
    ft, fp = m["frame_time"], m.get("frame_time_play") or {}
    mem = m.get("memory") or {}
    out = [f"# Bench run: {summary['scenario']}", "", headline_line(summary), ""]
    out.append(f"Verdict: **{'PASS' if v['ok'] else 'FAIL'}**  ")
    if summary["run"].get("noisy"):
        out.append(f"NOISY: {summary['run'].get('other_viewers_max')} other nv-viewer process(es) ran during this run, so its frame times are not valid.  ")
    out.append(
        f"Mode {summary['settings'].get('mode')}, {summary['settings'].get('window')}, "
        f"build {summary['settings'].get('build')}, wall {fmt(m['wall_s'])} s, ended by {summary['run']['ended_by']}."
    )
    out += ["", "| Check | Result | Detail |", "| --- | --- | --- |"]
    for c in v["checks"]:
        out.append(f"| {c['name']} | {'pass' if c['ok'] else 'FAIL'} | {c['detail']} |")
    out += ["", "## Frame times", "", "| Frames | fps | avg ms | p50 | p95 | p99 | max | 1% low fps | 0.1% low fps |", "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
    for label, s in (("all", ft), ("play screen", fp)):
        if s.get("frames"):
            out.append(
                f"| {label}: {s['frames']} | {fmt(s['fps'])} | {fmt(s['avg_ms'], 2)} | {fmt(s['p50_ms'], 2)} | {fmt(s['p95_ms'], 2)} | "
                f"{fmt(s['p99_ms'], 2)} | {fmt(s['max_ms'], 2)} | {fmt(s['low1_fps'])} | {fmt(s['low01_fps'])} |"
            )
    tl = m.get("fps_timeline") or []
    if tl:
        out += ["", "fps over time (1 s buckets):", "", "```", sparkline([t["fps"] for t in tl]) + f"   min {min(t['fps'] for t in tl)}, max {max(t['fps'] for t in tl)}", "```"]
    if mem:
        out += ["", "## Memory", "", "| RSS peak MiB | RSS end MiB | RSS growth MiB/min | VRAM peak MiB | GTT peak MiB | CPU avg % |", "| --- | --- | --- | --- | --- | --- |"]
        out.append(
            f"| {fmt(mem.get('rss_peak_mib'), 0)} | {fmt(mem.get('rss_end_mib'), 0)} | {fmt(mem.get('rss_growth_mib_per_min'), 2)} | "
            f"{fmt(mem.get('vram_peak_mib'), 0)} | {fmt(mem.get('gtt_peak_mib'), 0)} | {fmt(mem.get('cpu_avg_pct'), 0)} |"
        )
        for key, label in (("rss", "RSS"), ("vram", "VRAM"), ("gtt", "GTT")):
            ser = summary.get("series", {}).get(key)
            if ser:
                out += ["", f"{label} over time: `{sparkline(ser)}`"]
    ck = m.get("clock") or {}
    if ck.get("ratio") is not None:
        out += ["", "## Game clock", "", f"Game time / wall time {ck['ratio']:.3f} over {ck['play_s']} s of play; walking speed {ck['speed']} units/s; fell {ck['fall']} units, landed: {ck['settled']}; longest stand-still after moving {ck['stuck_s']} s."]
    hi = m.get("recorder")
    if hi:
        loads = hi.get("loads") or {"n": 0, "ms": 0}
        out += ["", "## Hitches", "", f"{hi.get('in_play', hi['count'])} hitches in play ({hi['per_min']} a minute), {loads['n']} slow frames of loads ({loads['ms']} ms), "
                f"{hi['slow_windows']} slow windows, p99 {hi['p99_ms']} ms. {hi['verdict']}"]
        if hi["by_cause"]:
            out.append("By cause: " + ", ".join(f"{k} x{v['n']} ({v['ms']} ms)" for k, v in hi["by_cause"].items()))
        if hi["over_pct"]:
            out.append("Frames over: " + ", ".join(f"{k} ms {v}%" for k, v in hi["over_pct"].items()))
        for w in hi["worst"]:
            out.append(f"- worst {w['wall_ms']} ms at frame {w['frame']}: {w['cause']}, trigger {w['trigger']}, top {', '.join(w.get('top', [])) or '-'}")
    out += ["", "## Counters", ""]
    out.append(
        f"wgpu errors {m['wgpu_errors']}, skipped draws {m['skipped_draws']} of {m['draws']}, stops {m['stops']}, "
        f"untranslated addresses {m['untranslated_addresses']} ({m['untranslated_hits']} hits), panics {m['panics']}, "
        f"memory failures {m['memory_failures']}, presents over 20 ms {m['slow_presents_over_20ms']}, exit {m['exit_status']}."
    )
    for s in m["stop_lines"][:5]:
        out.append(f"- stop: `{s}`")
    if m["screens"]:
        out += ["", "Screens: " + ", ".join(f"{s} at frame {f} ({t} s)" for f, t, s in m["screens"][:12])]
    mv = m.get("movement") or {}
    if mv.get("path"):
        out += ["", f"Player path {mv['path']:.0f} units, net {mv['net']:.0f}, furthest from start {mv['max_from_start']:.0f}."]
    return "\n".join(out) + "\n"


# ---------------------------------------------------------------------------------------------
# inline SVG


def svg_chart(title, xs, ys, unit, width=720, height=150, color="#2a6fdb"):
    """One line chart as inline SVG (no script, no external file); axis labels are min/max only."""
    pts = [(x, y) for x, y in zip(xs, ys) if y is not None]
    if len(pts) < 2:
        return f"<p>{html.escape(title)}: no data</p>"
    x0, x1 = pts[0][0], pts[-1][0]
    lo, hi = 0.0, max(y for _, y in pts) * 1.05 or 1.0
    pad_l, pad_b, pad_t = 52, 20, 18

    def sx(x):
        return pad_l + (x - x0) / max(x1 - x0, 1e-9) * (width - pad_l - 8)

    def sy(y):
        return height - pad_b - (y - lo) / max(hi - lo, 1e-9) * (height - pad_b - pad_t)

    path = " ".join(f"{'M' if i == 0 else 'L'}{sx(x):.1f},{sy(y):.1f}" for i, (x, y) in enumerate(pts))
    return (
        f'<svg viewBox="0 0 {width} {height}" role="img" aria-label="{html.escape(title)}" style="width:100%;max-width:{width}px">'
        f'<text x="{pad_l}" y="12" font-size="12" fill="currentColor">{html.escape(title)} ({html.escape(unit)})</text>'
        f'<line x1="{pad_l}" y1="{height - pad_b}" x2="{width - 8}" y2="{height - pad_b}" stroke="currentColor" opacity=".3"/>'
        f'<line x1="{pad_l}" y1="{pad_t}" x2="{pad_l}" y2="{height - pad_b}" stroke="currentColor" opacity=".3"/>'
        f'<text x="{pad_l - 4}" y="{pad_t + 4}" font-size="11" text-anchor="end" fill="currentColor">{hi:.0f}</text>'
        f'<text x="{pad_l - 4}" y="{height - pad_b}" font-size="11" text-anchor="end" fill="currentColor">0</text>'
        f'<text x="{pad_l}" y="{height - 4}" font-size="11" fill="currentColor">{x0:.0f} s</text>'
        f'<text x="{width - 8}" y="{height - 4}" font-size="11" text-anchor="end" fill="currentColor">{x1:.0f} s</text>'
        f'<path d="{path}" fill="none" stroke="{color}" stroke-width="1.5"/></svg>'
    )


def page(summary):
    """The markdown report plus charts, as one HTML file."""
    m = summary["metrics"]
    tl = m.get("fps_timeline") or []
    charts = [svg_chart("fps", [t["t"] for t in tl], [t["fps"] for t in tl], "frames per second")]
    charts.append(svg_chart("slowest frame per second", [t["t"] for t in tl], [t["max_ms"] for t in tl], "ms", color="#c0392b"))
    ts = summary.get("series_t") or []
    for key, label in (("rss", "RSS"), ("vram", "VRAM"), ("gtt", "GTT")):
        ser = summary.get("series", {}).get(key)
        if ser:
            charts.append(svg_chart(label, ts, ser, "MiB", color="#27864a"))
    body = html.escape(markdown(summary))
    return (
        "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>"
        f"<title>nv-rs bench {html.escape(summary['scenario'])}</title>"
        "<style>:root{color-scheme:light dark}body{font:14px/1.4 system-ui,sans-serif;margin:16px auto;max-width:780px;padding:0 16px}"
        "pre{white-space:pre-wrap}</style>"
        + "".join(charts)
        + f"<pre>{body}</pre>"
    )


# ---------------------------------------------------------------------------------------------
# comparison


def compare(a, b, thresholds=None):
    """Compare two summaries (a = baseline, b = candidate). Rows of (key, label, a, b, change %, verdict).

    A row regresses when it is worse by more than its threshold percent and by more than its noise floor;
    the three counters (wgpu errors, stops, untranslated) regress on any increase. Returns (rows, ok).
    """
    thresholds = thresholds or {}
    rows, ok = [], True
    for key, label, path, unit, higher_better, pct, floor in COMPARED:
        va, vb = dig(a["metrics"], path), dig(b["metrics"], path)
        if va is None or vb is None:
            rows.append((key, label, va, vb, None, "n/a"))
            continue
        limit = thresholds.get(key, pct)
        worse = (va - vb) if higher_better else (vb - va)
        change = (vb - va) / va * 100.0 if va else (0.0 if vb == va else float("inf"))
        bad = worse > floor and (worse / abs(va) * 100.0 > limit if va else True)
        if limit == 0.0 and floor == 0.0:
            bad = worse > 0
        better = (-worse) > floor and not bad
        rows.append((key, label, va, vb, change, "REGRESSION" if bad else ("better" if better else "ok")))
        ok = ok and not bad
    return rows, ok


def compare_markdown(a, b, rows, ok):
    out = [f"# Compare {a['scenario']} ({a['name']}) with {b['scenario']} ({b['name']})", "", f"Result: **{'OK' if ok else 'REGRESSION'}**", ""]
    out += ["| Metric | Baseline | Candidate | Change | Verdict |", "| --- | --- | --- | --- | --- |"]
    for key, label, va, vb, ch, verdict in rows:
        out.append(f"| {label} | {fmt(va, 2)} | {fmt(vb, 2)} | {'n/a' if ch is None else f'{ch:+.1f}%'} | {verdict} |")
    for label, x in (("baseline", a), ("candidate", b)):
        if x["run"].get("noisy"):
            out.append(f"\nNOISY: the {label} ran with other nv-viewer processes on the machine; its timing rows are not valid.")
    if a["scenario"] != b["scenario"]:
        out.append("\nWarning: the two runs are of different scenarios.")
    if a["settings"].get("mode") != b["settings"].get("mode"):
        out.append("\nWarning: the two runs used different frame-rate modes.")
    return "\n".join(out) + "\n"
