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
    """The amdgpu sysfs directory with the most VRAM, or None (no sampling, no watchdog)."""
    best, size = None, -1
    for d in glob.glob("/sys/class/drm/card*/device"):
        try:
            total = int(Path(d, "mem_info_vram_total").read_text())
        except (OSError, ValueError):
            continue
        if total > size:
            best, size = d, total
    return best


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
    total use (every process on it), checked as nv-rs-run's watchdog does, but here the first lines of
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


def terminate(pid, grace=5):
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        return
    end = time.monotonic() + grace
    while time.monotonic() < end:
        if proc_stat(pid) is None:
            return
        time.sleep(0.2)
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


def memory_cap_prefix(limit):
    """A systemd user scope with a hard memory cap and no CPU limit or nice (a quota would distort the
    measurement); empty when the scope is unavailable."""
    if not limit or not shutil.which("systemd-run"):
        return []
    cmd = ["systemd-run", "--user", "--scope", "--quiet", "--collect", "-p", f"MemoryMax={limit}", "-p", "MemorySwapMax=0"]
    try:
        if subprocess.run(cmd + ["true"], capture_output=True, timeout=10).returncode == 0:
            return cmd
    except (OSError, subprocess.TimeoutExpired):
        pass
    return []


def borrowed_libs(env):
    """What nv-rs-run does: the viewer dlopens Wayland, xkbcommon, Vulkan and pulse-simple; where the system has none
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
            "RUST_BACKTRACE": "1",
            "RUST_LOG": env.get("RUST_LOG", "info"),
        }
    )
    env.update(borrowed_libs(env))
    comp, vi = None, None
    if opts.get("nested"):
        sway = nested.find_sway()
        if not sway:
            raise SystemExit("no sway on PATH (nix shell nixpkgs#sway, or NV_BENCH_SWAY): needed by --nested")
        comp = nested.Nested(sway, run_dir, opts["resolution"], opts["nested"])
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
    cmd = memory_cap_prefix(opts.get("mem")) + [opts["viewer"], opts["game"], "--movement-missing", str(missing)]
    gpu = find_gpu()
    started = time.time()
    with open(log, "w") as lf:
        lf.write(f"# nv-rs bench {plan.name} {time.strftime('%Y%m%dT%H%M%S')}\n")
        lf.write(f"# keys: {env.get('NV_MOVE_KEYS', '')}\n# mouse: {env.get('NV_MOVE_MOUSE', '')}\n")
        lf.write(f"# mode: {opts['mode']}, window: {'on screen' if opts['windowed'] else 'offscreen'}, nested: {opts.get('nested') or 'no'}\n")
        lf.flush()
        proc = subprocess.Popen(cmd, stdout=lf, stderr=subprocess.STDOUT, env=env, cwd=run_dir)
        drive_done = threading.Event()
        if plan.drive:
            if not vi:
                proc.terminate()
                raise SystemExit("this scenario is a wall-clock driver and needs real input into a window: --nested headless (or --legacy for the frame-counted script)")
            drv = driver.Driver(vi, frames, proc, lf)
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
            terminate(proc.pid)
        rc = proc.wait()
        wd.finish()
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
    }
