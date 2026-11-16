#!/usr/bin/env python3
"""nv-rs-dev hitches <jsonl>: reads a run's JSON log and prints the hitches ranked by cause and by
trigger, the worst ones, the slow windows and a timeline (docs/fork/HITCHES.md). Standard library only.

    hitches.py RUN.jsonl [--top N] [--bucket SECONDS] [--ledger FILE] [--json]

--ledger takes a TSV of `address<TAB>name` (hex address) to name the guest functions the sampler found.
"""
import argparse
import json
import sys
from collections import defaultdict


def load(path):
    """The records of kind hitch, perf.slow_window and hitch_summary in a log, in order."""
    hitches, windows, summary = [], [], None
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                r = json.loads(line)
            except ValueError:
                continue
            kind = r.get("kind")
            if kind == "hitch" and "hitch" in r:
                hitches.append(r["hitch"])
            elif kind == "perf.slow_window" and "slow_window" in r:
                windows.append(r["slow_window"])
            elif kind == "hitch_summary":
                summary = r.get("summary")
    return hitches, windows, summary


def trigger_kind(trigger):
    """`cell.change 120 ms before` -> `cell.change`; a slow call keeps its name."""
    return (trigger or "none").split(" ")[0] or "none"


def rank(hitches):
    """Causes and triggers ranked by the time they cost: (key, count, total ms, worst ms)."""
    out = {}
    for name, key in (("cause", lambda h: h.get("cause", "unknown")), ("trigger", lambda h: trigger_kind(h.get("trigger")))):
        acc = defaultdict(lambda: [0, 0.0, 0.0])
        for h in hitches:
            a = acc[key(h)]
            ms = float(h.get("wall_ms", 0))
            a[0] += 1
            a[1] += ms
            a[2] = max(a[2], ms)
        out[name] = sorted(((k, v[0], v[1], v[2]) for k, v in acc.items()), key=lambda r: (-r[2], r[0]))
    return out


def load_ledger(path):
    names = {}
    if not path:
        return names
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) >= 2:
                try:
                    names[int(parts[0], 16)] = parts[1]
                except ValueError:
                    pass
    return names


def sampler_line(h, names):
    s = h.get("samples")
    if not s:
        return ""
    bits = []
    for c in s.get("calls", [])[:2]:
        arg = f" ({c['arg']})" if c.get("arg") else ""
        bits.append(f"{c['call']}{arg} x{c['n']}")
    hot = []
    for e in s.get("hot", [])[:3]:
        a = int(e["addr"], 16)
        hot.append(f"{names.get(a, e['addr'])} x{e['n']}")
    if s.get("in_guest"):
        chain = s.get("last_call_chain") or []
        after = f" after {names.get(int(chain[-1], 16), chain[-1])}" if chain else ""
        bits.append(f"game code x{s['in_guest']}{after}")
    return "; ".join(bits) + (" | " + ", ".join(hot) if hot else "")


def timeline(hitches, bucket_s):
    buckets = defaultdict(lambda: [0, 0.0, defaultdict(int)])
    for h in hitches:
        t = int(h.get("frame", {}).get("end_ms", 0) / 1000 // bucket_s)
        b = buckets[t]
        b[0] += 1
        b[1] += float(h.get("wall_ms", 0))
        b[2][h.get("cause", "unknown")] += 1
    return sorted(buckets.items())


def report(hitches, windows, summary, top=8, bucket_s=10, names=None):
    names = names or {}
    lines = []
    total = sum(float(h.get("wall_ms", 0)) for h in hitches)
    lines.append(f"{len(hitches)} hitches, {total:.0f} ms in all, {len(windows)} slow windows")
    if summary:
        o = summary.get("over_ms", {})
        lines.append(
            f"run: {summary.get('frames', 0)} frames, {summary.get('per_minute', 0)} hitches a minute, frames over "
            + ", ".join(f"{k} ms: {v}" for k, v in o.items())
        )
    ranked = rank(hitches)
    for title, rows in (("Cause", ranked["cause"]), ("Trigger", ranked["trigger"])):
        lines.append("")
        lines.append(f"{title:<24}{'count':>6}{'total ms':>10}{'worst ms':>10}")
        for k, n, t, w in rows:
            lines.append(f"{k:<24}{n:>6}{t:>10.0f}{w:>10.1f}")
    lines.append("")
    lines.append("Worst hitches")
    for h in sorted(hitches, key=lambda h: -float(h.get("wall_ms", 0)))[:top]:
        fr = h.get("frame", {})
        ctx = f"cell {fr.get('cell', '?')} {fr.get('screen', '?')}"
        lines.append(
            f"  {float(h.get('wall_ms', 0)):7.1f} ms  frame {h.get('frame_idx')}  at {fr.get('end_ms', 0) / 1000:.1f} s  "
            f"{h.get('cause')}  trigger {h.get('trigger')}  {ctx}"
        )
        sl = sampler_line(h, names)
        if sl:
            lines.append(f"            sampled: {sl}")
    if windows:
        lines.append("")
        lines.append("Slow windows (10 s, p99 over the limit)")
        for w in windows:
            mix = ", ".join(f"{k} x{v}" for k, v in w.get("cause_mix", {}).items())
            lines.append(
                f"  {w['from_ms'] / 1000:.0f}-{w['to_ms'] / 1000:.0f} s  p99 {w['p99_ms']} ms  {w['fps']} fps  {mix}"
            )
    lines.append("")
    lines.append(f"Timeline ({bucket_s} s buckets: hitches, ms)")
    for t, (n, ms, causes) in timeline(hitches, bucket_s):
        mix = ", ".join(f"{k} x{v}" for k, v in sorted(causes.items()))
        lines.append(f"  {t * bucket_s:5d} s  {'#' * min(n, 40):<40} {n:3d}  {ms:7.0f} ms  {mix}")
    return "\n".join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(prog="nv-rs-dev hitches", description=__doc__.split("\n")[0])
    ap.add_argument("log", help="the run's JSON log (.jsonl)")
    ap.add_argument("--top", type=int, default=8)
    ap.add_argument("--bucket", type=int, default=10, help="timeline bucket, seconds")
    ap.add_argument("--ledger", help="TSV of hex address and name for the guest functions")
    ap.add_argument("--json", action="store_true", help="the ranked tables as JSON")
    a = ap.parse_args(argv)
    try:
        hitches, windows, summary = load(a.log)
    except OSError as e:
        print(f"hitches: {e}", file=sys.stderr)
        return 2
    if a.json:
        print(json.dumps({"hitches": len(hitches), "rank": rank(hitches), "slow_windows": windows, "summary": summary}))
        return 0
    print(report(hitches, windows, summary, a.top, a.bucket, load_ledger(a.ledger)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
