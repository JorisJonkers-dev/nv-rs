# SPDX-License-Identifier: GPL-3.0-only
"""The memory guard of every bench run: caps on host and GPU memory that no run can skip, and a halt of all runs when
the host session is in trouble.

A bench run once started while the card's memory was already nearly full; the driver then refused the desktop's own
command submissions (`Not enough memory for command submission`) and the owner's session crashed. So every run:

- runs the viewer and its nested compositor in transient systemd scopes with `MemoryMax`, `MemoryHigh` and
  `MemorySwapMax=0`, sized well below the free RAM (no scope, no run);
- is refused before it starts when the card's free VRAM or GTT cannot hold the run's budget plus a margin kept for the
  desktop;
- is watched every 100 ms: the viewer's whole process group is stopped (TERM, then KILL) before the card's use reaches
  the run's cap, or the margin kept for the desktop;
- hands its VRAM cap to the engine (`NV_GPU_BUDGET_MB`), which refuses allocations past it with a `stop:` line; the
  run ends at that line;
- writes a halt file and stops every later run (and the rest of a suite) when the session is in trouble: the desktop's
  compositor gone, a GPU reset or an out-of-memory command-submission rejection in the kernel log, or the viewer stopped
  by this guard. Only `nv-rs-dev bench clear-halt` removes it.

The caps are the defaults and can only be lowered. Everything that reads the machine is passed in, so the tests use
fakes."""
import json
import os
import re
import shutil
import signal
import subprocess
import threading
import time
from dataclasses import asdict, dataclass
from pathlib import Path

MIB = 1 << 20
GIB = 1 << 30

# Host memory: the viewer's scope gets this share of the memory available at start, at most HOST_MAX; the nested
# compositor's scope gets SWAY_MAX at most.
HOST_SHARE = 0.5
HOST_MAX = 12 * GIB
SWAY_MAX = 2 * GIB
# GPU memory: the run's caps as shares of the card's totals, and what is kept free for the desktop on top.
VRAM_SHARE = 0.5
GTT_SHARE = 0.25
VRAM_MARGIN_MIB = 2048
GTT_MARGIN_MIB = 1024
# The guard stops the viewer at this share of the way to a limit, so it stops before the limit, not after.
STOP_AT = 0.9
SAMPLE_S = 0.1

# Fixed, with no environment override: a halt is not skipped by pointing a run at another file.
HALT = Path.home() / ".cache/nv-rs/bench.halt"

# Kernel log lines that mean the GPU or the session is in trouble.
KERNEL_TROUBLE = re.compile(
    r"Not enough memory for command submission|GPU reset|gpu_recover|ring \S+ timeout|amdgpu_job_timedout|"
    r"GPU recovery|device lost|VRAM is lost",
    re.I,
)
# The desktop's compositors (process names as /proc/PID/comm has them, 15 characters at most) whose loss means the session
# is in trouble. The bench's own nested sway is not one of them.
DESKTOP_NAMES = ("gnome-shell", ".gnome-shell-wr", "kwin_wayland", "kwin_wayland_wr", "mutter")


class Refused(Exception):
    """A run that may not start (the reason is the message)."""


@dataclass
class Caps:
    host_bytes: int
    sway_bytes: int
    vram_mib: int
    gtt_mib: int
    vram_margin_mib: int = VRAM_MARGIN_MIB
    gtt_margin_mib: int = GTT_MARGIN_MIB

    def as_dict(self):
        return asdict(self)


@dataclass
class GpuMem:
    """The card's use and totals in MiB (every process on the card counts)."""
    vram_used: float
    vram_total: float
    gtt_used: float
    gtt_total: float


def parse_size(text):
    """`12G`, `512M`, `4096K` or bytes; None for empty."""
    if text in (None, ""):
        return None
    t = str(text).strip().upper()
    mult = {"K": 1 << 10, "M": MIB, "G": GIB}.get(t[-1:], 1)
    return int(float(t[:-1] if t[-1:] in "KMG" else t) * mult)


def mem_available(meminfo="/proc/meminfo"):
    """Bytes of MemAvailable, or None."""
    try:
        for line in Path(meminfo).read_text().splitlines():
            if line.startswith("MemAvailable:"):
                return int(line.split()[1]) * 1024
    except (OSError, ValueError, IndexError):
        pass
    return None


def read_gpu(sysfs):
    """The card's memory from its amdgpu sysfs directory, or None when it cannot be read."""
    def val(name):
        try:
            return int(Path(sysfs, name).read_text()) / MIB
        except (OSError, ValueError):
            return None

    vals = [val(n) for n in ("mem_info_vram_used", "mem_info_vram_total", "mem_info_gtt_used", "mem_info_gtt_total")]
    if any(v is None for v in vals) or not vals[1] or not vals[3]:
        return None
    return GpuMem(*vals)


def find_gpu(pattern="/sys/class/drm/card*/device"):
    """The amdgpu sysfs directory with the most VRAM, or None."""
    import glob
    best, size = None, -1
    for d in glob.glob(pattern):
        try:
            total = int(Path(d, "mem_info_vram_total").read_text())
        except (OSError, ValueError):
            continue
        if total > size:
            best, size = d, total
    return best


def run_guarded(cmd, env=None, timeout=None, caps=None, gpu_dir=None, out=print, popen=subprocess.Popen):
    """Runs a GPU program (not the viewer: the bench runner has its own loop) under the same guard: refused while runs
    are halted or when the card has no room, in a capped scope, with the engine budget, sampled every 100 ms and
    stopped as a group. The exit status, or 4 when refused."""
    h = halted()
    if h:
        out(f"bench runs are halted since {h.get('time', '?')}: {h.get('reason')} (nv-rs-dev bench clear-halt)")
        return 4
    gpu_dir = gpu_dir or find_gpu()
    try:
        base = read_gpu(gpu_dir) if gpu_dir else None
        caps = caps or default_caps(mem_available(), base)
        preflight(caps, base)
        scope = scope_prefix(caps.host_bytes)
    except Refused as e:
        out(f"refused by the memory guard: {e}")
        return 4
    env = dict(env or os.environ)
    env["NV_GPU_BUDGET_MB"] = str(caps.vram_mib)
    started = time.time()
    desktop = desktop_pids()
    p = popen(scope + list(cmd), env=env, start_new_session=True)
    g = Guard(caps, base, read=lambda: read_gpu(gpu_dir), stop=lambda: stop_group(p.pid, alive=lambda: p.poll() is None),
              health=lambda: session_trouble(kernel_lines_since(started), desktop, desktop_pids(), {p.pid}))
    g.start()
    try:
        rc = p.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        out(f"the run was stopped after {timeout} s")
        stop_group(p.pid, alive=lambda: p.poll() is None)
        rc = 124
    g.finish()
    if g.stopped:
        out(f"memory guard: {g.stopped}: stopped; later runs are halted (nv-rs-dev bench clear-halt)")
    return rc


def default_caps(available, gpu):
    """The caps of a run on this machine: host memory from what is available now, GPU memory from the card's totals."""
    if not available:
        raise Refused("the free host memory cannot be read (/proc/meminfo): no run without a host memory cap")
    if gpu is None:
        raise Refused("the card's memory cannot be read (amdgpu sysfs mem_info_*): no run without a GPU memory cap")
    # The viewer and the compositor together stay within the share: the compositor's part comes out of it.
    total = min(HOST_MAX, int(available * HOST_SHARE))
    sway = min(SWAY_MAX, total // 5)
    return Caps(host_bytes=total - sway, sway_bytes=sway, vram_mib=int(gpu.vram_total * VRAM_SHARE), gtt_mib=int(gpu.gtt_total * GTT_SHARE))


def lowered(caps, host=None, vram_mib=None, gtt_mib=None):
    """The caps with the asked values, which may only be lower than the defaults."""
    out = Caps(**caps.as_dict())
    for name, asked, current in (("host memory", host, caps.host_bytes), ("VRAM", vram_mib, caps.vram_mib), ("GTT", gtt_mib, caps.gtt_mib)):
        if asked is None:
            continue
        if asked <= 0 or asked > current:
            raise Refused(f"the {name} cap can only be lowered: asked {asked}, the default here is {current}")
    if host is not None:
        out.host_bytes = host
        out.sway_bytes = min(out.sway_bytes, host // 4)
    if vram_mib is not None:
        out.vram_mib = vram_mib
    if gtt_mib is not None:
        out.gtt_mib = gtt_mib
    return out


def preflight(caps, gpu):
    """Refuses a run whose GPU budget plus the desktop's margin does not fit in the card's free memory now."""
    free_v, free_g = gpu.vram_total - gpu.vram_used, gpu.gtt_total - gpu.gtt_used
    need_v, need_g = caps.vram_mib + caps.vram_margin_mib, caps.gtt_mib + caps.gtt_margin_mib
    if free_v < need_v:
        raise Refused(f"VRAM: {free_v:.0f} MiB free of {gpu.vram_total:.0f}, the run needs {caps.vram_mib} plus {caps.vram_margin_mib} kept for the desktop")
    if free_g < need_g:
        raise Refused(f"GTT: {free_g:.0f} MiB free of {gpu.gtt_total:.0f}, the run needs {caps.gtt_mib} plus {caps.gtt_margin_mib} kept for the desktop")


def limits(caps, base):
    """The use (MiB, the whole card) at which the guard stops the run: STOP_AT of the way to the lower of the run's cap
    over the use at start and the card's total less the desktop's margin."""
    def one(used0, cap, total, margin):
        ceiling = min(used0 + cap, total - margin)
        return used0 + max(0.0, ceiling - used0) * STOP_AT

    return (one(base.vram_used, caps.vram_mib, base.vram_total, caps.vram_margin_mib),
            one(base.gtt_used, caps.gtt_mib, base.gtt_total, caps.gtt_margin_mib))


def over(caps, base, now):
    """Why the run must stop now, or None."""
    lv, lg = limits(caps, base)
    if now.vram_used >= lv:
        return f"VRAM {now.vram_used:.0f} MiB reached the guard's stop at {lv:.0f} (cap {caps.vram_mib} over {base.vram_used:.0f} at start)"
    if now.gtt_used >= lg:
        return f"GTT {now.gtt_used:.0f} MiB reached the guard's stop at {lg:.0f} (cap {caps.gtt_mib} over {base.gtt_used:.0f} at start)"
    return None


def scope_prefix(limit_bytes):
    """The systemd-run prefix of a capped transient scope. Refuses when systemd-run cannot make one: no run without it."""
    if not shutil.which("systemd-run"):
        raise Refused("systemd-run is not here: no run without a host memory cap")
    mib = max(64, limit_bytes // MIB)
    high = max(48, mib * 9 // 10)
    cmd = ["systemd-run", "--user", "--scope", "--quiet", "--collect",
           "-p", f"MemoryMax={mib}M", "-p", f"MemoryHigh={high}M", "-p", "MemorySwapMax=0"]
    try:
        ok = subprocess.run(cmd + ["true"], capture_output=True, timeout=10).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        ok = False
    if not ok:
        raise Refused("systemd-run could not make a memory-capped scope: no run without a host memory cap")
    return cmd


# --- the halt file ---------------------------------------------------------------------------------------------------

def halted(path=None):
    """The halt record (a dict) when runs are halted, else None."""
    p = Path(path or HALT)
    if not p.exists():
        return None
    try:
        return json.loads(p.read_text())
    except (OSError, ValueError):
        return {"reason": "unreadable halt file"}


def halt(reason, path=None):
    p = Path(path or HALT)
    p.parent.mkdir(parents=True, exist_ok=True)
    tmp = p.with_name(f"{p.name}.{os.getpid()}.tmp")
    tmp.write_text(json.dumps({"time": time.strftime("%Y-%m-%dT%H:%M:%S"), "reason": reason}))
    tmp.replace(p)  # whole or not at all


def clear_halt(path=None):
    p = Path(path or HALT)
    if p.exists():
        p.unlink()
        return True
    return False


# --- session health --------------------------------------------------------------------------------------------------

def desktop_pids(proc="/proc"):
    """The pids of the desktop's compositors running now (by their process name)."""
    out = set()
    for d in Path(proc).glob("[0-9]*"):
        try:
            name = (d / "comm").read_text().strip()
        except OSError:
            continue
        if name in DESKTOP_NAMES:
            out.add(int(d.name))
    return out


def kernel_lines_since(since_unix):
    """The kernel log since a time, as text ("" when journalctl cannot read it)."""
    try:
        r = subprocess.run(["journalctl", "-k", "-q", "-o", "cat", "--no-pager", "--since", f"@{int(since_unix)}"],
                           capture_output=True, text=True, timeout=5)
        return r.stdout
    except (OSError, subprocess.TimeoutExpired):
        return ""


def kernel_log_readable():
    """Whether journalctl can read the kernel log here (the session check needs it)."""
    try:
        return subprocess.run(["journalctl", "-k", "-q", "-n", "1", "--no-pager"], capture_output=True, timeout=5).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def session_trouble(kernel_text, desktop_at_start, desktop_now, own=()):
    """Why the host session is in trouble, or None: a GPU reset or an out-of-memory submission in the kernel log, or a
    desktop compositor that ran at the start and is gone (the run's own nested compositor is not the desktop's)."""
    for line in (kernel_text or "").splitlines():
        if "amdgpu" not in line and "[drm]" not in line:
            continue
        m = KERNEL_TROUBLE.search(line)
        if m:
            return f"kernel: {m.group(0)}"
    gone = set(desktop_at_start) - set(desktop_now) - set(own)
    if gone:
        return f"the desktop's compositor is gone (pid {', '.join(str(p) for p in sorted(gone))})"
    return None


# --- the guard during a run ------------------------------------------------------------------------------------------

def stop_group(pgid, grace=2.0, killpg=os.killpg, alive=None, sleep=time.sleep):
    """TERM to a process group, KILL after `grace` s if it is still there."""
    try:
        killpg(pgid, signal.SIGTERM)
    except OSError:
        return
    end = time.monotonic() + grace
    while time.monotonic() < end:
        if alive is not None and not alive():
            return
        sleep(0.05)
    try:
        killpg(pgid, signal.SIGKILL)
    except OSError:
        pass


ENGINE_REFUSAL = "the layer's video-memory budget is used up"


class Guard:
    """Samples the card every SAMPLE_S during a run and stops the viewer's process group before it passes a cap; checks
    the session's health and the engine's own budget refusal once a second. `stopped` is the reason, if it stopped.

    `read` gives a GpuMem, `health` a reason or None, `log_text` the viewer's log so far, `stop` stops the viewer; all
    passed in."""

    def __init__(self, caps, base, read, stop, health=lambda: None, log_text=lambda: "", on_sample=None,
                 interval=SAMPLE_S, health_every=1.0, halt_path=None, clock=time.monotonic):
        self.caps, self.base, self.read, self.stop_viewer = caps, base, read, stop
        self.health, self.log_text, self.on_sample = health, log_text, on_sample
        self.interval, self.health_every, self.halt_path, self.clock = interval, health_every, halt_path, clock
        self.stopped = None
        self.trouble = None
        self.peak = {"vram_mib": base.vram_used, "gtt_mib": base.gtt_used}
        self._stop = threading.Event()
        self.thread = threading.Thread(target=self.loop, daemon=True)

    def start(self):
        self.thread.start()

    def finish(self):
        self._stop.set()
        self.thread.join(timeout=10)

    def check(self, now, health_due):
        """One step: the reason to stop, or None. Also records the peaks."""
        if now is None:
            return "the card's memory can no longer be read: the run is not guarded"
        if now is not None:
            self.peak["vram_mib"] = max(self.peak["vram_mib"], now.vram_used)
            self.peak["gtt_mib"] = max(self.peak["gtt_mib"], now.gtt_used)
            why = over(self.caps, self.base, now)
            if why:
                return why
        if health_due:
            t = self.health()
            if t:
                self.trouble = t
                return f"session trouble: {t}"
            if ENGINE_REFUSAL in (self.log_text() or ""):
                return "the engine refused an allocation past its GPU budget (NV_GPU_BUDGET_MB)"
        return None

    def loop(self):
        last_health = -1e9
        while not self._stop.wait(self.interval):
            now = self.read()
            t = self.clock()
            due = t - last_health >= self.health_every
            if due:
                last_health = t
            if self.on_sample:
                self.on_sample(now)
            why = self.check(now, due)
            if why:
                self.stopped = why
                self.stop_viewer()
                # The viewer stopped by the guard, or a session in trouble: no later run starts.
                halt(why, self.halt_path)
                return
