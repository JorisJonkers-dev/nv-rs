"""Launch the viewer for one scenario run, sample it, watch the GPU memory, and collect the run directory.

The run directory (default ~/.local/state/nv-rs/bench/<scenario>-<time>/) holds numbers and logs only:
  run.json      the settings and the verdict
  viewer.log    the viewer's stdout and stderr
  frames.tsv    NV_FRAME_LOG (one line per main frame: clock, player position, screen)
  samples.tsv   one line a second: RSS, CPU, VRAM, GTT of the card (amdgpu sysfs)
  missing.tsv*  --movement-missing lists
"""
import glob
import json
import os
import shutil
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

import driver
import guard
import lock
import nested
import record
import watch
import parse
import vinput

HOME = Path.home()
STATE = Path(os.environ.get("NV_BENCH_DIR", HOME / ".local/state/nv-rs/bench"))
CLK_TCK = os.sysconf("SC_CLK_TCK")


def find_gpu():
    """The amdgpu sysfs directory with the most VRAM, or None (guard.find_gpu)."""
    return guard.find_gpu()


def read_int(path):
    try:
        return int(Path(path).read_text())
    except (OSError, ValueError):
        return None


def proc_stat(pid):
    """(cpu seconds, RSS MiB) of a process, or None once it is gone."""
    try:
        f = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        cpu = (int(f[11]) + int(f[12])) / CLK_TCK
        rss = 0
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                rss = int(line.split()[1]) / 1024.0
        return cpu, rss
    except (OSError, IndexError, ValueError):
        return None


class Watchdog:
    """Sample VRAM/GTT/RSS/CPU once a second into samples.tsv; stop the viewer past the limit.

    A GPU out of memory once took the whole desktop session down with it, so the limit is on the card's
    total use (every process on it), checked as a local wrapper's watchdog does, but here the first lines of
    defence are early: a SIGTERM at the limit, a SIGKILL three seconds later.
    """

    def __init__(self, pid, out, limit_pct, gpu=None, interval=1.0):
        self.pid, self.out, self.limit, self.gpu, self.interval = pid, Path(out), limit_pct, gpu, interval
        self.killed = None
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.loop, daemon=True)

    def start(self):
        self.thread.start()

    def finish(self):
        self.stop.set()
        self.thread.join(timeout=5)

    def loop(self):
        t0 = time.monotonic()
        last_cpu, last_t = None, t0
        with open(self.out, "w") as f:
            f.write("\t".join(parse.SAMPLE_COLS) + "\n")
            while not self.stop.wait(self.interval):
                ps = proc_stat(self.pid)
                if ps is None or ps[1] == 0:  # gone, or a zombie that is still being reaped
                    break
                now = time.monotonic()
                cpu_pct = 0.0 if last_cpu is None else 100.0 * (ps[0] - last_cpu) / max(now - last_t, 1e-6)
                last_cpu, last_t = ps[0], now
                vram = gtt = 0.0
                over = None
                if self.gpu:
                    vu, vt = read_int(f"{self.gpu}/mem_info_vram_used"), read_int(f"{self.gpu}/mem_info_vram_total")
                    gu, gt = read_int(f"{self.gpu}/mem_info_gtt_used"), read_int(f"{self.gpu}/mem_info_gtt_total")
                    if vu is not None and vt:
                        vram = vu / 1048576.0
                        if self.limit and vu * 100 >= vt * self.limit:
                            over = f"VRAM {vram:.0f} MiB of {vt / 1048576:.0f} (limit {self.limit}%)"
                    if gu is not None and gt:
                        gtt = gu / 1048576.0
                        if self.limit and gu * 100 >= gt * self.limit:
                            over = f"GTT {gtt:.0f} MiB of {gt / 1048576:.0f} (limit {self.limit}%)"
                f.write(f"{now - t0:.1f}\t{ps[1]:.1f}\t{cpu_pct:.1f}\t{vram:.1f}\t{gtt:.1f}\n")
                f.flush()
                if over:
                    self.killed = over
                    terminate(self.pid, grace=3)
                    break


def gone(pid):
    """Whether a process has ended: no /proc entry, or a zombie (our own child that has not been waited for yet keeps
    its entry until then, so a check for the entry alone waited out the whole grace time of every run)."""
    try:
        return Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0] in ("Z", "X")
    except (OSError, IndexError):
        return True


def terminate(pid, grace=5):
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        return
    end = time.monotonic() + grace
    while time.monotonic() < end:
        if gone(pid):
            return
        time.sleep(0.1)
    try:
        os.kill(pid, signal.SIGKILL)
    except OSError:
        pass


def last_frame(path):
    """The highest frame number in the frame log so far (reads only its tail)."""
    try:
        with open(path, "rb") as f:
            f.seek(0, os.SEEK_END)
            f.seek(max(0, f.tell() - 4096))
            lines = f.read().decode("utf-8", "replace").splitlines()
    except OSError:
        return None
    for line in reversed(lines[1:]):
        head = line.split("\t", 1)[0]
        if head.isdigit() and line.count("\t") > 8:
            return int(head)
    return None


def run_dir_for(name):
    d = STATE / f"{name}-{time.strftime('%Y%m%dT%H%M%S')}"
    d.mkdir(parents=True, exist_ok=False)
    return d


def run_caps(opts, card):
    """The run's memory caps (guard.py) for the card as read now: the defaults of this machine, lowered where the
    options ask. Refuses a run when the caps cannot be made."""
    caps = guard.default_caps(guard.mem_available(), card)
    return guard.lowered(caps, host=guard.parse_size(opts.get("mem")), vram_mib=opts.get("vram_cap_mib"), gtt_mib=opts.get("gtt_cap_mib"))


def stop_all(proc, comp):
    """The guard's stop: the viewer's process group, then the nested compositor's."""
    guard.stop_group(proc.pid, alive=lambda: proc.poll() is None)
    if comp and comp.proc and comp.proc.poll() is None:
        guard.stop_group(comp.proc.pid, alive=lambda: comp.proc.poll() is None)


class LogTail:
    """The text a growing log file gained since the last call."""

    def __init__(self, path):
        self.path, self.pos = Path(path), 0

    def __call__(self):
        try:
            with open(self.path, "rb") as f:
                f.seek(self.pos)
                data = f.read()
                self.pos += len(data)
                return data.decode("utf-8", "replace")
        except OSError:
            return ""


def borrowed_libs(env):
    """What a local wrapper does: the viewer dlopens Wayland, xkbcommon, Vulkan and pulse-simple; where the system has none
    (NixOS), take the Nix store's copies. Without them a windowed run silently falls back to off screen."""
    out = {}
    have = subprocess.run(["ldconfig", "-p"], capture_output=True, text=True).stdout if shutil.which("ldconfig") else ""
    if "libwayland-client" not in have:
        dirs = []
        for pat in ("*-wayland-1.*/lib", "*-libxkbcommon-*/lib", "*-vulkan-loader-*/lib"):
            dirs += [d for d in sorted(glob.glob(f"/nix/store/{pat}")) if "-dev/" not in d and "-bin/" not in d][-1:]
        if dirs:
            out["LD_LIBRARY_PATH"] = ":".join(dirs + ([env["LD_LIBRARY_PATH"]] if env.get("LD_LIBRARY_PATH") else []))
    if not env.get("NV_PULSE_SIMPLE") and "libpulse-simple" not in have:
        found = sorted(glob.glob("/nix/store/*-libpulseaudio-*/lib/libpulse-simple.so.0"))
        if found:
            out["NV_PULSE_SIMPLE"] = found[-1]
    return out


def screen_tail(path, n=40):
    """The last rows of the frame log as (frame, screen, x) tuples."""
    try:
        with open(path, "rb") as f:
            f.seek(0, os.SEEK_END)
            f.seek(max(0, f.tell() - max(65536, n * 160)))
            lines = f.read().decode("utf-8", "replace").splitlines()
    except OSError:
        return []
    rows = []
    for line in lines[1:]:
        p = line.split("\t")
        if len(p) > 9 and p[0].isdigit():
            try:
                rows.append((int(p[0]), p[8].strip(), float(p[5])))
            except ValueError:
                pass
    return rows[-n:]


def run_real(plan, frames, comp, vi, lf, proc):
    """Wait until the player has been teleported (x moved past plan.real_after_x) and the play screen has held for
    plan.real_settle_frames, then send the plan's real input steps."""
    stable = 0
    while proc.poll() is None:
        time.sleep(0.5)
        rows = screen_tail(frames, plan.real_settle_frames + 100)
        if not rows:
            continue
        run = 0
        for _, scr, x in reversed(rows):
            if scr != "play" or x > plan.real_after_x:
                break
            run += 1
        stable = run
        if stable >= plan.real_settle_frames:
            break
    if proc.poll() is not None:
        return
    lf.write(f"# inject: real input starts (frame {rows[-1][0] if rows else 0})\n")
    lf.flush()
    try:
        vi.run(plan.real)
    except (OSError, ValueError, RuntimeError) as e:
        lf.write(f"# inject: failed: {e}\n")
    lf.write("# inject: real input done\n")
    lf.flush()


def run_drive(plan, drv, opts, done, lf):
    try:
        lf.write("# driver: starts\n")
        lf.flush()
        plan.drive(drv, opts)
    except Exception as e:  # a driver's failure ends the run with its message in the log
        lf.write(f"# driver: failed: {e!r}\n")
    finally:
        lf.write("# driver: done\n")
        lf.flush()
        done.set()


def launch(plan, opts, run_dir):
    """Run the viewer under `plan` (scenarios.Plan) and `opts` (a dict); returns run.json's content."""
    run_dir = Path(run_dir)
    # The memory guard first: no caps, or a card without room for the run and the desktop, no run (guard.Refused).
    h = guard.halted()
    if h:
        raise guard.Refused(f"bench runs are halted since {h.get('time', '?')}: {h.get('reason')}")
    gpu = find_gpu()
    base = guard.read_gpu(gpu) if gpu else None  # one reading: the caps and the room are judged on the same numbers
    caps = run_caps(opts, base)
    guard.preflight(caps, base)
    kernel_log_ok = guard.kernel_log_readable()
    (run_dir / "caps.json").write_text(json.dumps(caps.as_dict(), indent=1))
    log = run_dir / "viewer.log"
    frames = run_dir / "frames.tsv"
    missing = run_dir / "missing.tsv"
    env = dict(os.environ)
    env.update(plan.env(opts))
    env.update(
        {
            "NV_FRAME_LOG": str(frames),
            "NV_HITCH_CSV": env.get("NV_HITCH_CSV", str(run_dir / "hitches.csv")),
            "NV_MEMLOG": env.get("NV_MEMLOG", "1"),
            # The JSON log, the hitch records and the run summary go in the run directory (docs/fork/HITCHES.md).
            "NV_LOG_JSON": env.get("NV_LOG_JSON", str(run_dir / "nv-rs.jsonl")),
            "RUST_BACKTRACE": "1",
            "RUST_LOG": env.get("RUST_LOG", "info"),
        }
    )
    # The engine's own budget (d3d9-wgpu budget.rs): allocations past the run's VRAM cap are refused with a stop line.
    # A lower NV_GPU_BUDGET_MB in the environment stays; a higher one, or 0 (which would switch it off), does not.
    asked = env.get("NV_GPU_BUDGET_MB", "").strip()
    budget_mib = min(caps.vram_mib, int(asked)) if asked.isdigit() and int(asked) > 0 else caps.vram_mib
    env["NV_GPU_BUDGET_MB"] = str(budget_mib)
    env.update(borrowed_libs(env))
    comp, vi = None, None
    if opts.get("nested"):
        sway = nested.find_sway()
        if not sway:
            raise SystemExit("no sway on PATH (nix shell nixpkgs#sway, or NV_BENCH_SWAY): needed by --nested")
        comp = nested.Nested(sway, run_dir, opts["resolution"], opts["nested"], scope=guard.scope_prefix(caps.sway_bytes))
        if not comp.start():
            comp.stop()
            raise SystemExit(f"the nested compositor did not start: see {run_dir}/compositor.log")
        env.update(comp.env())
        env.pop("DISPLAY", None)
        # the virtual keyboard and pointer exist before the game binds the seat, and live for the whole run
        try:
            vi = vinput.VirtualInput(f"{comp.runtime}/{comp.socket}")
            vi.start()
        except (OSError, RuntimeError) as e:
            vi = None
            print(f"bench: no virtual input ({e}): real input steps are skipped", file=sys.stderr)
    rec = None
    watch.write_steps(run_dir, plan)
    watch.write_session(run_dir, comp, opts)
    if opts.get("record"):
        if comp:
            rec = record.Recorder(run_dir, comp.socket, comp.runtime, fps=opts.get("record_fps", 30), fmt=opts.get("record_format", "mp4"))
            if not rec.start():
                print(f"bench: not recording: {rec.note}", file=sys.stderr)
        else:
            print("bench: --record needs --nested headless (nothing to record offscreen)", file=sys.stderr)
    try:
        scope = guard.scope_prefix(caps.host_bytes)
    except guard.Refused:
        if comp:
            comp.stop()
        raise
    cmd = scope + [opts["viewer"], opts["game"], "--movement-missing", str(missing)]
    started = time.time()
    desktop = guard.desktop_pids()
    with open(log, "w") as lf:
        lf.write(f"# nv-rs bench {plan.name} {time.strftime('%Y%m%dT%H%M%S')}\n")
        lf.write(f"# keys: {env.get('NV_MOVE_KEYS', '')}\n# mouse: {env.get('NV_MOVE_MOUSE', '')}\n")
        lf.write(f"# mode: {opts['mode']}, window: {'on screen' if opts['windowed'] else 'offscreen'}, nested: {opts.get('nested') or 'no'}\n")
        lf.write(f"# memory caps: host {caps.host_bytes // guard.MIB} MiB, compositor {caps.sway_bytes // guard.MIB} MiB, VRAM {caps.vram_mib} MiB "
                 f"(engine budget {budget_mib} MiB), GTT {caps.gtt_mib} MiB; kept for the desktop {caps.vram_margin_mib} MiB VRAM, "
                 f"{caps.gtt_margin_mib} MiB GTT; card at start {base.vram_used:.0f} of {base.vram_total:.0f} MiB VRAM, "
                 f"{base.gtt_used:.0f} of {base.gtt_total:.0f} MiB GTT\n")
        if not kernel_log_ok:
            lf.write("# memory guard warning: the kernel log cannot be read (journalctl -k); GPU resets are not seen, "
                     "only the card's use and the desktop's compositor\n")
        lf.flush()
        # Its own process group, so the guard stops the viewer and everything it started at once.
        proc = subprocess.Popen(cmd, stdout=lf, stderr=subprocess.STDOUT, env=env, cwd=run_dir, start_new_session=True)
        tail = LogTail(log)
        own = {proc.pid} | ({comp.proc.pid} if comp and comp.proc else set())
        gd = guard.Guard(
            caps, base, read=lambda: guard.read_gpu(gpu),
            stop=lambda: stop_all(proc, comp),
            health=lambda: guard.session_trouble(guard.kernel_lines_since(started), desktop, guard.desktop_pids(), own),
            log_text=tail,
        )
        gd.start()
        drive_done = threading.Event()
        if plan.drive:
            if not vi:
                proc.terminate()
                raise SystemExit("this scenario is a wall-clock driver and needs real input into a window: --nested headless (or --legacy for the frame-counted script)")
            drv = driver.Driver(vi, frames, proc, lf)
            if comp:
                drv.wayland = {"WAYLAND_DISPLAY": comp.socket, "XDG_RUNTIME_DIR": comp.runtime}
            threading.Thread(target=run_drive, args=(plan, drv, opts, drive_done, lf), daemon=True).start()
        elif vi and plan.real:
            threading.Thread(target=run_real, args=(plan, frames, comp, vi, lf, proc), daemon=True).start()
        wd = Watchdog(proc.pid, run_dir / "samples.tsv", opts["vram_limit_pct"], gpu)
        wd.start()
        reason = None
        first_frame_at = None
        others_max, others_at = lock.other_viewers([proc.pid]), time.time()
        try:
            while proc.poll() is None:
                time.sleep(0.5)
                now = time.time()
                if now - others_at >= 5:
                    others_max, others_at = max(others_max, lock.other_viewers([proc.pid])), now
                lf_n = last_frame(frames)
                if lf_n is not None and first_frame_at is None:
                    first_frame_at = now
                if gd.stopped:
                    reason = "memory guard"
                    break
                if wd.killed:
                    reason = "watchdog"
                    break
                if drive_done.is_set():
                    reason = "scenario end (driver)"
                    break
                if lf_n is not None and plan.end_frame and lf_n >= plan.end_frame:
                    reason = "scenario end (frame)"
                    break
                if first_frame_at and plan.wall_after_first_s and now - first_frame_at >= plan.wall_after_first_s:
                    reason = "scenario end (wall)"
                    break
                if now - started > opts["timeout_s"]:
                    reason = "timeout"
                    break
        except KeyboardInterrupt:
            reason = "interrupted"
        if proc.poll() is None:
            guard.stop_group(proc.pid, grace=5, alive=lambda: proc.poll() is None)
        rc = proc.wait()
        gd.finish()
        wd.finish()
        if gd.stopped:
            lf.write(f"# memory guard: {gd.stopped}: stopped the game; later runs are halted (nv-rs-dev bench clear-halt)\n")
        if wd.killed:
            lf.write(f"# GPU memory watchdog: {wd.killed}: stopped the game\n")
        lf.write(f"# exit status: {rc}\n")
    if rec and rec.proc:
        rec.stop(first_frame_at)
    if vi:
        vi.close()
    if comp:
        comp.stop()
    return {
        "scenario": plan.name,
        "ended_by": reason or "viewer exited",
        "other_viewers_max": others_max,
        "noisy": others_max > 0,
        "exit_status": rc,
        "wall_s": round(time.time() - started, 1),
        "watchdog": wd.killed,
        "gpu_sysfs": bool(gpu),
        "caps": dict(caps.as_dict(), engine_budget_mib=budget_mib),
        "guard": gd.stopped,
        "session_trouble": gd.trouble,
        "card_at_start_mib": {"vram": round(base.vram_used), "gtt": round(base.gtt_used)},
        "card_peak_mib": {"vram": round(gd.peak["vram_mib"]), "gtt": round(gd.peak["gtt_mib"])},
    }
