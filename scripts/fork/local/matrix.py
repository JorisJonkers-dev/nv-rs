#!/usr/bin/env python3
"""nv-rs-dev matrix | clean | compare: build, test and benchmark many refs, configurations and hosts (docs/fork/LOCAL.md).

  nv-rs-dev matrix [spec.toml] [--ref R ...] [--baseline R] [--config k=v,...] [--scenario S ...] [--host H ...]
                   [--no-test] [--no-bench] [--scale F] [--json] [--out DIR] [--dry-run]
  nv-rs-dev compare <baseline-ref> <candidate-ref> [--scenario S ...]     (a two-ref matrix)
  nv-rs-dev clean [--all] [--yes]

Python standard library only. Heavy work is delegated to `nv-rs-dev build|ci-lite|bench` (so the memory cap, the shared
target directory and the bench runner are the existing ones); this file adds refs, worktrees, hosts and the report.
Numbers only: no game data goes into a report.
"""
import argparse
import fcntl
import html
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import time
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "bench"))
import report as benchreport  # noqa: E402  (the bench thresholds and compare are reused, not copied)

HOME = Path.home()
CACHE = Path(os.environ.get("NV_MATRIX_CACHE", HOME / ".cache/nv-rs"))
STATE = Path(os.environ.get("NV_MATRIX_STATE", HOME / ".local/state/nv-rs"))
TARGET = Path(os.environ.get("NV_DEV_TARGET", CACHE / "target"))
BIN_CACHE = CACHE / "matrix-bin"
HOSTS_FILE = Path(os.environ.get("NV_MATRIX_HOSTS", HOME / ".config/nv-rs/hosts.toml"))
SSH = shlex.split(os.environ.get("NV_MATRIX_SSH", "ssh"))
RSYNC = shlex.split(os.environ.get("NV_MATRIX_RSYNC", "rsync"))
DEV = os.environ.get("NV_MATRIX_DEV", str(HERE / "nv-rs-dev"))
WT_PREFIX = "matrix-"
SESSIONS = ("offscreen", "xvfb", "nested-wayland")
CONFIG_KEYS = ("name", "session", "mode", "resolution", "preset", "threads", "sound", "vram_limit_pct", "env", "args")
DEFAULT_CONFIG = {"name": "offscreen", "session": "offscreen", "mode": "capped", "resolution": "1920x1080", "preset": "fixes"}


class Fail(Exception):
    pass


def log(*a):
    print(*a, file=sys.stderr, flush=True)


def run(cmd, cwd=None, env=None, check=False, quiet=True):
    r = subprocess.run(cmd, cwd=cwd, env=env, text=True, capture_output=quiet)
    if check and r.returncode != 0:
        raise Fail(f"{' '.join(map(str, cmd))}: {(r.stderr or r.stdout or '').strip()[-400:]}")
    return r


# ---------------------------------------------------------------------------------------------
# spec


def safe(s):
    return re.sub(r"[^A-Za-z0-9._-]+", "-", s).strip("-") or "x"


def parse_config_flag(text):
    cfg = {}
    for item in text.split(","):
        k, _, v = item.partition("=")
        k = k.strip().replace("-", "_")
        if k not in CONFIG_KEYS or k in ("env", "args"):
            raise Fail(f"--config: unknown key '{k}' (keys: name, session, mode, resolution, preset, threads, sound, vram_limit_pct)")
        cfg[k] = int(v) if k == "vram_limit_pct" else v.strip()
    return cfg


def norm_config(c, index):
    cfg = dict(DEFAULT_CONFIG)
    cfg.update({k: v for k, v in c.items() if k in CONFIG_KEYS})
    unknown = set(c) - set(CONFIG_KEYS)
    if unknown:
        raise Fail(f"config {c.get('name', index)}: unknown keys {sorted(unknown)}")
    if cfg["session"] not in SESSIONS:
        raise Fail(f"config {cfg['name']}: session must be one of {', '.join(SESSIONS)}")
    if cfg["mode"] not in ("capped", "uncapped"):
        raise Fail(f"config {cfg['name']}: mode must be capped or uncapped")
    if not re.fullmatch(r"\d+x\d+", str(cfg["resolution"])):
        raise Fail(f"config {cfg['name']}: resolution must be WxH")
    if "name" not in c:
        cfg["name"] = "-".join(safe(str(cfg[k])) for k in ("session", "mode", "resolution")) if c else DEFAULT_CONFIG["name"]
    cfg["name"] = safe(cfg["name"])
    cfg["env"] = {str(k): str(v) for k, v in (cfg.get("env") or {}).items()}
    cfg["args"] = [str(x) for x in (cfg.get("args") or [])]
    return cfg


def load_spec(path, args):
    raw = {}
    if path:
        try:
            raw = tomllib.loads(Path(path).read_text())
        except (OSError, tomllib.TOMLDecodeError) as e:
            raise Fail(f"spec {path}: {e}")
    refs = list(args.ref or raw.get("refs") or [])
    if not refs:
        raise Fail("no refs: give --ref (repeatable) or a spec with refs = [...]")
    baseline = args.baseline or raw.get("baseline") or refs[0]
    if baseline not in refs:
        refs.insert(0, baseline)
    configs = [norm_config(c, i) for i, c in enumerate(raw.get("config") or [])]
    configs += [norm_config(parse_config_flag(t), 1000 + i) for i, t in enumerate(args.config or [])]
    if not configs:
        configs = [norm_config({}, 0)]
    names = [c["name"] for c in configs]
    if len(set(names)) != len(names):
        raise Fail(f"duplicate config names: {names}")
    test = raw.get("test", True) and not args.no_test
    bench = raw.get("bench", True) and not args.no_bench
    scenarios = list(args.scenario or raw.get("scenarios") or (["boot"] if bench else []))
    return {
        "refs": refs, "baseline": baseline, "configs": configs, "scenarios": scenarios, "test": bool(test), "bench": bool(bench),
        "hosts": list(args.host or raw.get("hosts") or ["local"]), "scale": args.scale if args.scale is not None else raw.get("scale"),
        "thresholds": {str(k): float(v) for k, v in (raw.get("thresholds") or {}).items()},
        "mem": raw.get("mem"), "profile": raw.get("profile"),
    }


# ---------------------------------------------------------------------------------------------
# refs and worktrees


def main_clone(repo):
    """The main clone behind a checkout (a worktree's common git dir sits in it)."""
    r = run(["git", "-C", str(repo), "rev-parse", "--path-format=absolute", "--git-common-dir"])
    if r.returncode != 0:
        raise Fail(f"{repo} is not a git checkout")
    g = Path(r.stdout.strip())
    return g.parent if g.name == ".git" else g


def worktree_base(repo):
    """~/Projects/<owner>/worktrees/<repo>/ for ~/Projects/<owner>/<repo> (the layout rule); a sibling folder otherwise."""
    clone = main_clone(repo)
    return clone.parent / "worktrees" / clone.name


def resolve_ref(repo, ref):
    """-> (label, sha or None for the working tree). Fetches a pull request, a branch or a tag from origin when needed."""
    if ref in ("working", "."):
        head = run(["git", "-C", str(repo), "rev-parse", "--short=8", "HEAD"]).stdout.strip()
        return f"working@{head}", None
    m = re.fullmatch(r"(?:pr:|#)?(\d+)", ref)
    if m:
        n = m.group(1)
        r = run(["git", "-C", str(repo), "fetch", "-q", "origin", f"pull/{n}/head"])
        if r.returncode != 0:
            raise Fail(f"cannot fetch pull request {n}: {r.stderr.strip()[-200:]}")
        sha = run(["git", "-C", str(repo), "rev-parse", "FETCH_HEAD"], check=True).stdout.strip()
        return f"pr{n}@{sha[:8]}", sha
    for cand in (ref, f"origin/{ref}", f"refs/tags/{ref}"):
        r = run(["git", "-C", str(repo), "rev-parse", "--verify", "-q", f"{cand}^{{commit}}"])
        if r.returncode == 0:
            break
    else:
        run(["git", "-C", str(repo), "fetch", "-q", "origin", ref])
        r = run(["git", "-C", str(repo), "rev-parse", "--verify", "-q", "FETCH_HEAD^{commit}"])
        if r.returncode != 0:
            raise Fail(f"unknown ref {ref} (not a commit, branch, tag or pull request number)")
    sha = r.stdout.strip()
    return f"{safe(ref)}@{sha[:8]}", sha


def ensure_worktree(repo, label, sha):
    wt = worktree_base(repo) / f"{WT_PREFIX}{safe(label)}"
    if wt.exists():
        head = run(["git", "-C", str(wt), "rev-parse", "HEAD"]).stdout.strip()
        if head == sha:
            return wt
        run(["git", "-C", str(main_clone(repo)), "worktree", "remove", "--force", str(wt)])
    wt.parent.mkdir(parents=True, exist_ok=True)
    run(["git", "-C", str(main_clone(repo)), "worktree", "add", "--detach", "-q", str(wt), sha], check=True)
    return wt


# ---------------------------------------------------------------------------------------------
# build, test, bench (local host)


def dev_env(**extra):
    env = dict(os.environ)
    env.update({k: str(v) for k, v in extra.items()})
    return env


def build_ref(repo, label, sha, spec):
    """Build nv-viewer for a ref once per commit; the binary is kept under BIN_CACHE/<sha>. -> result dict, binary path."""
    wt = repo if sha is None else ensure_worktree(repo, label, sha)
    exe = BIN_CACHE / (sha or "working") / "nv-viewer"   # a working tree changes under us: always rebuilt, never cached
    if sha and exe.exists():
        return {"status": "cached", "seconds": 0.0}, exe, wt
    env = dev_env(NV_DEV_REPO=wt, NV_DEV_TARGET=TARGET)
    if spec.get("mem"):
        env["NV_DEV_MEM"] = spec["mem"]
    if spec.get("profile"):
        env["NV_DEV_PROFILE"] = spec["profile"]
    if shutil.which("sccache") and os.environ.get("NV_MATRIX_SCCACHE", "auto") != "0":
        env.update(RUSTC_WRAPPER="sccache", SCCACHE_DIR=str(CACHE / "sccache"), CARGO_INCREMENTAL="0")
    t0 = time.time()
    r = subprocess.run([DEV, "build"], env=env, text=True, capture_output=True)
    secs = round(time.time() - t0, 1)
    built = TARGET / env.get("NV_DEV_PROFILE", "play") / "nv-viewer"
    if r.returncode != 0 or not built.exists():
        return {"status": "failed", "seconds": secs, "detail": (r.stdout + r.stderr).strip()[-300:]}, None, wt
    exe.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(built, exe)
    return {"status": "built", "seconds": secs}, exe, wt


def failure_detail(text):
    """The lines that say what failed (test names, errors), not the tail of the noise."""
    keys = [ln.strip() for ln in text.splitlines() if re.search(r"\bFAILED\b|^error(\[|:)|panicked at|# ci-lite: FAILED", ln.strip())]
    return " | ".join(dict.fromkeys(keys))[-400:] or text.strip()[-300:]


def test_ref(wt, base_sha):
    """The affected components' Linux tests (nv-rs-dev ci-lite) against the baseline commit."""
    t0 = time.time()
    env = dev_env(NV_DEV_REPO=wt, NV_DEV_TARGET=TARGET)
    r = subprocess.run([DEV, "ci-lite", base_sha or "origin/fork"], env=env, text=True, capture_output=True)
    return {"status": "pass" if r.returncode == 0 else "fail", "seconds": round(time.time() - t0, 1),
            "detail": "" if r.returncode == 0 else failure_detail(r.stdout + "\n" + r.stderr)}


def bench_help():
    r = subprocess.run([DEV, "bench", "run", "--help"], text=True, capture_output=True)
    return r.stdout + r.stderr


def bench_cell(exe, cfg, scenario, out_dir, scale, game, help_text):
    """One bench run through `nv-rs-dev bench run`; a private display for any session that needs a window."""
    cmd = [DEV, "bench", "run", scenario, "--mode", cfg["mode"], "--resolution", cfg["resolution"], "--preset", cfg["preset"]]
    for k in ("threads", "sound"):
        if cfg.get(k):
            cmd += [f"--{k}", cfg[k]]
    if cfg.get("vram_limit_pct") is not None:
        cmd += ["--vram-limit-pct", str(cfg["vram_limit_pct"])]
    if scale:
        cmd += ["--scale", str(scale)]
    env = dev_env(NV_BENCH_VIEWER=exe, NV_BENCH_DIR=out_dir, **cfg["env"])
    if game:
        env["NV_GAME"] = game
    sess = cfg["session"]
    if sess == "xvfb":
        # A private X server: the window can never reach the maintainer's desktop.
        if not shutil.which("xvfb-run"):
            return {"status": "skipped", "detail": "xvfb-run is not installed"}
        for v in ("WAYLAND_DISPLAY", "DISPLAY"):
            env.pop(v, None)
        env["XDG_SESSION_TYPE"] = "x11"
        w, h = cfg["resolution"].split("x")
        cmd = ["xvfb-run", "-a", "-s", f"-screen 0 {w}x{h}x24"] + cmd + ["--windowed", "--yes-windowed"]
    elif sess == "nested-wayland":
        if "--session" not in help_text:
            return {"status": "skipped", "detail": "the bench runner has no --session (nested compositor) option yet"}
        cmd += ["--session", "nested-wayland"]
    cmd += cfg["args"]
    Path(out_dir).mkdir(parents=True, exist_ok=True)
    r = subprocess.run(cmd, env=env, text=True, capture_output=True)
    runs = sorted((d for d in Path(out_dir).glob(f"{scenario}-*") if (d / "summary.json").exists()), key=lambda d: d.name)
    if not runs:
        return {"status": "error", "detail": (r.stderr or r.stdout).strip()[-300:]}
    summary = json.loads((runs[-1] / "summary.json").read_text())
    return {"status": "ok" if summary["verdict"]["ok"] else "verdict-failed", "summary": slim(summary), "run": runs[-1].name,
            "checks": [c for c in summary["verdict"]["checks"] if not c["ok"]]}


def slim(summary):
    """Only what the matrix needs: the metrics (numbers) and the settings, no series."""
    return {"name": summary["name"], "scenario": summary["scenario"], "settings": summary.get("settings", {}),
            "metrics": summary["metrics"], "verdict": summary["verdict"]}


def acquire_lock(name="matrix.lock"):
    CACHE.mkdir(parents=True, exist_ok=True)
    f = open(CACHE / name, "w")
    try:
        fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        raise Fail("another matrix run is in progress (one cargo process at a time)")
    return f


def execute_local(spec, repo, game, out):
    """Run the whole matrix on this machine -> the result dict (also written to out/results.json)."""
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    lock = acquire_lock()
    res = {"host": "local", "refs": {}, "cells": [], "baseline": spec["baseline"]}
    labels = {}
    for ref in spec["refs"]:
        labels[ref] = resolve_ref(repo, ref)
    base_label, base_sha = labels[spec["baseline"]]
    help_text = bench_help() if spec["bench"] else ""
    exes = {}
    for ref in spec["refs"]:
        label, sha = labels[ref]
        log(f"== {ref} ({label})")
        b, exe, wt = build_ref(repo, label, sha, spec)
        entry = {"label": label, "sha": sha, "build": b}
        if b["status"] != "failed" and spec["test"]:
            entry["test"] = test_ref(wt, base_sha)
        res["refs"][ref] = entry
        exes[ref] = exe
    if spec["bench"]:
        for cfg in spec["configs"]:
            for sc in spec["scenarios"]:
                for ref in spec["refs"]:
                    if exes[ref] is None:
                        res["cells"].append({"config": cfg["name"], "scenario": sc, "ref": ref, "status": "no-build"})
                        continue
                    log(f"-- bench {sc} / {cfg['name']} / {ref}")
                    cell = bench_cell(exes[ref], cfg, sc, out / "runs" / safe(res["refs"][ref]["label"]) / cfg["name"],
                                      spec.get("scale"), game, help_text)
                    cell.update({"config": cfg["name"], "scenario": sc, "ref": ref})
                    res["cells"].append(cell)
    res["configs"] = [c["name"] for c in spec["configs"]]
    (out / "results.json").write_text(json.dumps(res, indent=1))
    lock.close()
    return res


# ---------------------------------------------------------------------------------------------
# hosts


def load_hosts(path=None):
    p = Path(path or HOSTS_FILE)
    if not p.exists():
        return {}
    try:
        data = tomllib.loads(p.read_text())
    except tomllib.TOMLDecodeError as e:
        raise Fail(f"{p}: {e}")
    hosts = data.get("host", {})
    for name, h in hosts.items():
        if "ssh" not in h or "repo" not in h:
            raise Fail(f"{p}: host {name} needs ssh and repo")
        if h.get("sync", "git") not in ("git", "rsync"):
            raise Fail(f"{p}: host {name}: sync must be git or rsync")
    return hosts


def remote_commands(name, h, spec_file_remote, out_remote):
    """The remote shell command that runs this matrix on a host's own checkout."""
    repo = h["repo"]
    env = []
    if h.get("game"):
        env.append(f"NV_GAME={shlex.quote(h['game'])}")
    if h.get("cache"):
        env.append(f"NV_MATRIX_CACHE={shlex.quote(h['cache'])} NV_DEV_TARGET={shlex.quote(h['cache'] + '/target')}")
    pre = f"cd {repo} && " + ("git fetch -q origin && " if h.get("sync", "git") == "git" else "")
    return pre + " ".join(env) + f" scripts/fork/local/nv-rs-dev matrix --local-only --spec-json {spec_file_remote} --out {out_remote}"


def execute_remote(name, h, spec, out, repo, dry=False):
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%dT%H%M%S")
    cache = h.get("cache", "~/.cache/nv-rs")
    remote_dir = f"{cache}/matrix-runs/{stamp}"
    spec_remote = f"{remote_dir}/spec.json"
    local_spec = out / f"spec-{name}.json"
    local_spec.write_text(json.dumps(dict(spec, hosts=["local"])))
    cmds = [SSH + [h["ssh"], f"mkdir -p {remote_dir}"]]
    if h.get("sync", "git") == "rsync":
        cmds.append(RSYNC + ["-a", "--exclude", "/target", "--exclude", "/.git/worktrees", f"{repo}/", f"{h['ssh']}:{h['repo']}/"])
    cmds.append(RSYNC + ["-a", str(local_spec), f"{h['ssh']}:{spec_remote}"])
    cmds.append(SSH + [h["ssh"], remote_commands(name, h, spec_remote, remote_dir)])
    # Reports only: the numbers, the two report files and the settings. Logs stay on the host (they name local paths).
    cmds.append(RSYNC + ["-a", "--include=*/", "--include=results.json", "--include=summary.json", "--include=report.md", "--include=report.html",
                         "--exclude=*", f"{h['ssh']}:{remote_dir}/", f"{out}/"])
    if dry:
        return {"host": name, "dry_run": [" ".join(map(shlex.quote, c)) for c in cmds]}
    for i, c in enumerate(cmds):
        r = subprocess.run(c, text=True, capture_output=True)
        if r.returncode != 0 and i != len(cmds) - 2:
            raise Fail(f"host {name}: {' '.join(c[:3])}: {(r.stderr or r.stdout).strip()[-300:]}")
        if i == len(cmds) - 2 and r.returncode != 0:
            log(f"host {name}: the remote run exited {r.returncode} (a regression or a failure; pulling its reports)")
    rj = out / "results.json"
    if not rj.exists():
        raise Fail(f"host {name}: no results.json came back")
    res = json.loads(rj.read_text())
    rj.rename(out / f"results-{name}.json")
    res["host"] = name
    return res


# ---------------------------------------------------------------------------------------------
# comparison and report


def judge(results, spec):
    """Per host, config and scenario: each non-baseline ref against the baseline with bench's thresholds.
    -> list of rows {host, config, scenario, ref, rows, ok}, and the overall verdict."""
    cmp_rows, ok = [], True
    for res in results:
        base = spec["baseline"]
        by = {(c["config"], c["scenario"], c["ref"]): c for c in res["cells"]}
        for cfg in res.get("configs", []):
            for sc in spec["scenarios"]:
                b = by.get((cfg, sc, base))
                for ref in spec["refs"]:
                    if ref == base:
                        continue
                    c = by.get((cfg, sc, ref))
                    entry = {"host": res["host"], "config": cfg, "scenario": sc, "ref": ref, "rows": [], "verdict": "n/a", "ok": True}
                    if not b or not c or "summary" not in b or "summary" not in c:
                        why = (c or {}).get("status") or "missing"
                        bw = (b or {}).get("status") or "missing"
                        entry["verdict"] = f"not compared (candidate {why}, baseline {bw})"
                        entry["ok"] = why in ("skipped",) and bw in ("skipped",) or why == "skipped" or bw == "skipped"
                        cmp_rows.append(entry)
                        ok = ok and entry["ok"]
                        continue
                    rows, good = benchreport.compare(b["summary"], c["summary"], spec.get("thresholds"))
                    entry["rows"] = [{"key": k, "label": lbl, "base": va, "cand": vb, "change_pct": ch, "verdict": v} for k, lbl, va, vb, ch, v in rows]
                    entry["verdict"] = "OK" if good else "REGRESSION"
                    entry["ok"] = good
                    ok = ok and good
                    cmp_rows.append(entry)
        for ref, e in res["refs"].items():
            if e["build"]["status"] == "failed" or e.get("test", {}).get("status") == "fail":
                ok = False
        for c in res["cells"]:
            if c.get("status") in ("error", "no-build"):
                ok = False
    return cmp_rows, ok


def f(v, nd=1):
    if v is None:
        return "n/a"
    return f"{v:.{nd}f}" if isinstance(v, float) else str(v)


def key_metrics(summary):
    m = summary["metrics"]
    d = benchreport.dig
    return {"fps": d(m, ("frame_time", "fps")), "low1": d(m, ("frame_time", "low1_fps")), "p99": d(m, ("frame_time", "p99_ms")),
            "rss": d(m, ("memory", "rss_peak_mib")), "vram": d(m, ("memory", "vram_peak_mib"))}


def markdown(spec, results, cmp_rows, ok, when):
    o = [f"# nv-rs matrix {when}", "", f"Baseline: `{spec['baseline']}`. Result: **{'OK' if ok else 'REGRESSION or failure'}**.", ""]
    for res in results:
        o += [f"## Host {res['host']}", "", "| Ref | Commit | Build | Tests |", "| --- | --- | --- | --- |"]
        for ref, e in res["refs"].items():
            t = e.get("test")
            o.append(f"| {ref}{' (baseline)' if ref == spec['baseline'] else ''} | {e['label']} | {e['build']['status']} {e['build']['seconds']} s | "
                     f"{'skipped' if not t else t['status'] + f' {t['seconds']} s'} |")
        o.append("")
        by = {(c["config"], c["scenario"], c["ref"]): c for c in res["cells"]}
        for cfg in res.get("configs", []):
            for sc in spec["scenarios"]:
                o += [f"### {sc} / {cfg}", "", "| Ref | Run | fps | 1% low | p99 ms | RSS MiB | VRAM MiB | vs baseline |", "| --- | --- | --- | --- | --- | --- | --- | --- |"]
                for ref in spec["refs"]:
                    c = by.get((cfg, sc, ref)) or {}
                    if "summary" in c:
                        k = key_metrics(c["summary"])
                        verdict = "baseline" if ref == spec["baseline"] else next((r["verdict"] for r in cmp_rows if (r["host"], r["config"], r["scenario"], r["ref"]) == (res["host"], cfg, sc, ref)), "n/a")
                        o.append(f"| {ref} | {c['status']} | {f(k['fps'])} | {f(k['low1'])} | {f(k['p99'], 2)} | {f(k['rss'], 0)} | {f(k['vram'], 0)} | {verdict} |")
                    else:
                        o.append(f"| {ref} | {c.get('status', 'missing')}: {c.get('detail', '')} | | | | | | |")
                o.append("")
        for r in cmp_rows:
            if r["host"] == res["host"] and r["rows"]:
                bad = [x for x in r["rows"] if x["verdict"] in ("REGRESSION", "better")]
                if bad:
                    o += [f"Deltas of `{r['ref']}` ({r['scenario']} / {r['config']}) against the baseline:", "", "| Metric | Baseline | Candidate | Change | Verdict |", "| --- | --- | --- | --- | --- |"]
                    for x in bad:
                        ch = "n/a" if x["change_pct"] is None else f"{x['change_pct']:+.1f}%"
                        o.append(f"| {x['label']} | {f(x['base'], 2)} | {f(x['cand'], 2)} | {ch} | {x['verdict']} |")
                    o.append("")
    return "\n".join(o) + "\n"


def page(md_text, ok):
    """A self-contained HTML page of the markdown report (tables and headings only; no script, no external files)."""
    body, table = [], []

    def flush():
        if table:
            rows = [r for r in table if not re.fullmatch(r"\|[ -|]+\|", r)]
            out = ["<table>"]
            for i, r in enumerate(rows):
                cells = [html.escape(c.strip()) for c in r.strip().strip("|").split("|")]
                tag = "th" if i == 0 else "td"
                cls = ' class="bad"' if any("REGRESSION" in c for c in cells) else ""
                out.append(f"<tr{cls}>" + "".join(f"<{tag}>{c}</{tag}>" for c in cells) + "</tr>")
            body.append("\n".join(out) + "</table>")
            table.clear()

    for line in md_text.splitlines():
        if line.startswith("|"):
            table.append(line)
            continue
        flush()
        m = re.match(r"(#+) (.*)", line)
        if m:
            n = len(m.group(1))
            body.append(f"<h{n}>{html.escape(m.group(2))}</h{n}>")
        elif line.strip():
            body.append("<p>" + re.sub(r"\*\*(.+?)\*\*", r"<b>\1</b>", re.sub(r"`(.+?)`", r"<code>\1</code>", html.escape(line))) + "</p>")
    flush()
    return ("<!doctype html><meta charset=utf-8><title>nv-rs matrix</title><style>body{font:14px system-ui;margin:2em;max-width:70em}"
            "table{border-collapse:collapse;margin:1em 0}td,th{border:1px solid #8886;padding:3px 8px;text-align:right}"
            "td:first-child,th:first-child{text-align:left}tr.bad{background:#d223}</style>\n" + "\n".join(body))


def summary_json(spec, results, cmp_rows, ok, out):
    """The machine-readable summary (CI mode): numbers and verdicts only."""
    return {
        "ok": ok, "baseline": spec["baseline"], "report": str(out / "matrix.md"),
        "hosts": [{"host": r["host"], "refs": {k: {"label": e["label"], "build": e["build"]["status"], "build_seconds": e["build"]["seconds"],
                                                    "test": e.get("test", {}).get("status", "skipped")} for k, e in r["refs"].items()}} for r in results],
        "comparisons": [{"host": c["host"], "config": c["config"], "scenario": c["scenario"], "ref": c["ref"], "verdict": c["verdict"],
                         "regressions": [x["key"] for x in c["rows"] if x["verdict"] == "REGRESSION"]} for c in cmp_rows],
    }


# ---------------------------------------------------------------------------------------------
# commands


def default_repo():
    r = run(["git", "rev-parse", "--show-toplevel"], cwd=os.environ.get("NV_DEV_REPO") or None)
    return Path(os.environ.get("NV_DEV_REPO") or r.stdout.strip() or ".").resolve()


def cmd_matrix(a):
    if a.local_only:
        spec = json.loads(Path(a.spec_json).read_text())
        execute_local(spec, default_repo(), os.environ.get("NV_GAME"), a.out)
        return 0
    spec = load_spec(a.spec, a)
    repo = default_repo()
    when = time.strftime("%Y-%m-%d %H:%M:%S")
    out = Path(a.out or STATE / "matrix" / time.strftime("%Y%m%dT%H%M%S"))
    if a.dry_run:
        plan = {"spec": spec, "out": str(out), "repo": str(repo)}
        hosts = load_hosts() if any(h != "local" for h in spec["hosts"]) else {}
        for h in spec["hosts"]:
            if h != "local":
                if h not in hosts:
                    raise Fail(f"unknown host {h}: add it to {HOSTS_FILE}")
                plan.setdefault("remote", []).append(execute_remote(h, hosts[h], spec, out, repo, dry=True))
        print(json.dumps(plan, indent=1))
        return 0
    out.mkdir(parents=True, exist_ok=True)
    hosts = load_hosts() if any(h != "local" for h in spec["hosts"]) else {}
    results = []
    for h in spec["hosts"]:
        if h == "local":
            results.append(execute_local(spec, repo, os.environ.get("NV_GAME"), out / "local"))
        elif h in hosts:
            results.append(execute_remote(h, hosts[h], spec, out / h, repo))
        else:
            raise Fail(f"unknown host {h}: add it to {HOSTS_FILE}")
    cmp_rows, ok = judge(results, spec)
    md = markdown(spec, results, cmp_rows, ok, when)
    (out / "matrix.md").write_text(md)
    (out / "matrix.html").write_text(page(md, ok))
    summ = summary_json(spec, results, cmp_rows, ok, out)
    (out / "matrix.json").write_text(json.dumps(summ, indent=1))
    if a.json:
        print(json.dumps(summ))
    else:
        print(md)
        print(f"report: {out / 'matrix.html'}")
    return 0 if ok else 1


def cmd_compare(a):
    a.ref, a.baseline = [a.baseline_ref, a.candidate_ref], a.baseline_ref
    a.spec = a.config = a.host = None
    a.local_only = False
    return cmd_matrix(a)


def du(path):
    total = 0
    for dp, dn, fn in os.walk(path, followlinks=False):
        for n in fn:
            try:
                total += os.lstat(os.path.join(dp, n)).st_size
            except OSError:
                pass
    return total


def human(n):
    for u in ("B", "KiB", "MiB", "GiB"):
        if n < 1024 or u == "GiB":
            return f"{n:.1f} {u}" if u != "B" else f"{n} B"
        n /= 1024


def clean_targets(repo, everything):
    """(label, path, kind): only this loop's own caches. Nothing outside these roots is ever removed."""
    t = [("cargo target dir", TARGET, "dir"), ("cold-build target dir", Path(str(TARGET) + "-cold"), "dir"), ("matrix binaries", BIN_CACHE, "dir")]
    try:
        base = worktree_base(repo)
        for wt in sorted(base.glob(WT_PREFIX + "*")):
            t.append((f"matrix worktree {wt.name}", wt, "worktree"))
    except Fail:
        pass
    if everything:
        t += [("sccache", CACHE / "sccache", "dir"), ("matrix run reports", STATE / "matrix", "dir"),
              ("bench runs and scratch saves", STATE / "bench", "dir"), ("dev logs", STATE / "logs", "dir"),
              ("remote run dirs", CACHE / "matrix-runs", "dir")]
    return t


def allowed(path):
    p = Path(os.path.abspath(path))
    if p.is_symlink():
        return False
    ok_roots = [Path(os.path.abspath(x)) for x in (CACHE, STATE, TARGET, str(TARGET) + "-cold")]
    in_root = any(p == r or r in p.parents for r in ok_roots) or WT_PREFIX in p.name
    # Never the home dir, a filesystem root or a very short path.
    return in_root and p != HOME and len(p.parts) >= 4 and "nv-rs" in str(p)


def cmd_clean(a):
    repo = default_repo()
    items = [(lbl, p, k, du(p)) for lbl, p, k in clean_targets(repo, a.all) if p.exists() or p.is_symlink()]
    if not items:
        print("nothing to clean")
        return 0
    print(f"{'what':40} {'size':>10}  path")
    for lbl, p, k, sz in items:
        print(f"{lbl:40} {human(sz):>10}  {p}")
    print(f"{'total':40} {human(sum(i[3] for i in items)):>10}")
    if a.dry_run or not (a.yes or (sys.stdin.isatty() and input("remove these? [y/N] ").strip().lower() == "y")):
        print("nothing removed" + ("" if a.dry_run or a.yes else " (pass --yes to remove without asking)"))
        return 0
    try:
        lock = acquire_lock()
    except Fail as e:
        print(f"clean: {e}", file=sys.stderr)
        return 1
    freed = 0
    for lbl, p, k, sz in items:
        if not allowed(p):
            print(f"refused: {p} is not under this loop's cache or state folders", file=sys.stderr)
            continue
        if k == "worktree":
            r = run(["git", "-C", str(main_clone(repo)), "worktree", "remove", "--force", str(p)])
            if r.returncode != 0:
                shutil.rmtree(p, ignore_errors=True)
            run(["git", "-C", str(main_clone(repo)), "worktree", "prune"])
        else:
            shutil.rmtree(p, ignore_errors=True)
        freed += sz
        print(f"removed {p}")
    lock.close()
    print(f"freed {human(freed)}")
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(prog="nv-rs-dev", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    m = sub.add_parser("matrix")
    m.add_argument("spec", nargs="?")
    m.add_argument("--ref", action="append", help="commit, branch, tag, pull request number (93 or pr:93), or 'working' (this checkout as it is); repeatable")
    m.add_argument("--baseline")
    m.add_argument("--config", action="append", help="name=lo,session=offscreen,mode=uncapped,resolution=1280x720,preset=vanilla,threads=stepped,sound=silent")
    m.add_argument("--scenario", action="append")
    m.add_argument("--host", action="append", help="'local' or a name in ~/.config/nv-rs/hosts.toml")
    m.add_argument("--no-test", action="store_true")
    m.add_argument("--no-bench", action="store_true")
    m.add_argument("--scale", type=float)
    m.add_argument("--json", action="store_true", help="CI mode: print one machine-readable JSON summary on stdout (progress goes to stderr)")
    m.add_argument("--out")
    m.add_argument("--dry-run", action="store_true", help="print the resolved plan and the remote commands, run nothing")
    m.add_argument("--local-only", action="store_true", help=argparse.SUPPRESS)
    m.add_argument("--spec-json", help=argparse.SUPPRESS)
    m.set_defaults(fn=cmd_matrix)
    c = sub.add_parser("compare")
    c.add_argument("baseline_ref")
    c.add_argument("candidate_ref")
    c.add_argument("--scenario", action="append")
    c.add_argument("--no-test", action="store_true")
    c.add_argument("--no-bench", action="store_true")
    c.add_argument("--scale", type=float)
    c.add_argument("--json", action="store_true")
    c.add_argument("--out")
    c.add_argument("--dry-run", action="store_true")
    c.set_defaults(fn=cmd_compare)
    cl = sub.add_parser("clean")
    cl.add_argument("--all", action="store_true", help="also sccache, matrix reports, bench runs, dev logs and remote run dirs")
    cl.add_argument("--yes", action="store_true")
    cl.add_argument("--dry-run", action="store_true")
    cl.set_defaults(fn=cmd_clean)
    a = ap.parse_args(argv)
    try:
        return a.fn(a) or 0
    except Fail as e:
        print(f"nv-rs-dev {a.cmd}: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
