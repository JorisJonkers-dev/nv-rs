#!/usr/bin/env python3
"""Print the smoke checklist for one run log. Numbers and verdicts only; never echoes game data."""
import re, sys

log, rc, secs = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(log, errors="replace").read()
lines = text.splitlines()

def find(pat):
    r = re.compile(pat, re.I)
    return [l for l in lines if r.search(l)]

results = []
def check(name, ok, detail=""):
    results.append((name, ok, detail))

bad_window = find(r"could not be made|off ?screen")
check("real window created", not bad_window and bool(find(r"is real window")),
      f"{len(bad_window)} bad-window lines" if bad_window else "")
disp = [l for l in find(r"display") if re.search(r"\d+\s*x\s*\d+", l) and re.search(r"\d+(\.\d+)?\s*hz", l, re.I)]
m = re.search(r"(\d+)\s*x\s*(\d+).*?(\d+(?:\.\d+)?)\s*hz", disp[0], re.I) if disp else None
check("display line (size, Hz)", bool(m), f"{m.group(1)}x{m.group(2)} @ {m.group(3)} Hz" if m else "none")
frames = [int(x) for l in find(r"movement: main \d+ frames") for x in re.findall(r"main (\d+) frames", l)]
check("frames advanced", len(frames) >= 2 and max(frames) > min(frames),
      f"{min(frames)}..{max(frames)}" if frames else "no frame counters")
# wgpu present reports say "N wgpu errors"; any other wgpu line naming an error or validation failure counts too.
rep = [int(x) for l in find(r"\d+ wgpu errors") for x in re.findall(r"(\d+) wgpu errors", l)]
other = [l for l in find(r"wgpu") if re.search(r"validation|panic|\berror:|device lost", l, re.I) and not re.search(r"\d+ wgpu errors", l)]
nerr = (max(rep) if rep else 0) + len(other)
check("wgpu errors == 0", rep and nerr == 0, f"{nerr} errors over {len(rep)} reports" if rep else "no wgpu report line")
# A game stop is a line that starts with "stopped:" or says the game stopped; the ledger's "static initializer
# skipped: ... stopped: open function" notes are counted as information, not a stop.
stop = [l for l in lines if re.search(r"^\W*(game )?stopped:", l, re.I)]
skipped = [l for l in find(r"stopped:") if l not in stop]
check("no 'stopped:' line", not stop, f"{len(stop)} lines; {len(skipped)} untranslated-function notes")
mem = find(r"not enough memory|device.?lost|out of memory")
check("no memory / device-lost lines", not mem, f"{len(mem)} lines")
snd = find(r"sound|audio|pulse")
check("sound line present", bool(snd), f"{len(snd)} lines")
panic = find(r"panicked at")
check("no panic", not panic, f"{len(panic)} lines")

for n, ok, d in results:
    print(f"{'PASS' if ok else 'FAIL'}  {n}" + (f"  [{d}]" if d else ""))
allok = all(ok for _, ok, _ in results)
print(f"{'PASS' if allok else 'FAIL'}: {sum(ok for _,ok,_ in results)}/{len(results)} checks; ran {secs} s, exit {rc}; log {log}")
sys.exit(0 if allok else 1)
