"""A nested headless Wayland compositor (sway on wlroots' headless backend) for window-path bench runs.

The game gets a real Wayland surface, a real swap chain, real focus and input events at the output's size, and the
maintainer's session (GNOME) is never involved: the compositor has its own socket and no display. `window` mode instead
shows the nested compositor as one window inside the maintainer's session (opt-in; it can be covered, then frame callbacks
stop and the fps numbers are not valid; use headless for numbers).
"""
import os
import shutil
import signal
import subprocess
import time
from pathlib import Path


def find_sway():
    for name in (os.environ.get("NV_BENCH_SWAY"), shutil.which("sway")):
        if name and os.access(name, os.X_OK):
            return name
    return None


def sway_config(width, height):
    """The compositor's configuration text: one output at the game's size, no Xwayland, no borders, every window full screen."""
    return (
        "xwayland disable\n"
        f"output * mode {width}x{height}@60Hz\n"
        "default_border none\n"
        "focus_follows_mouse no\n"
        'for_window [app_id=".*"] fullscreen enable\n'
    )


def parse_resolution(text):
    w, h = text.lower().split("x")
    return int(w), int(h)


class Nested:
    def __init__(self, sway, run_dir, resolution, mode="headless", scope=None):
        """`scope` is the systemd-run prefix of the compositor's memory-capped scope (guard.scope_prefix): required."""
        if not scope:
            raise ValueError("the nested compositor runs only in a memory-capped scope")
        self.sway, self.dir, self.mode, self.scope = sway, Path(run_dir), mode, list(scope)
        self.w, self.h = parse_resolution(resolution)
        self.proc = None
        self.socket = f"nv-bench-{os.getpid()}"
        self.runtime = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"

    def env(self):
        """Environment for the game: the nested compositor's display only."""
        return {"WAYLAND_DISPLAY": self.socket, "XDG_RUNTIME_DIR": self.runtime, "XDG_SESSION_TYPE": "wayland"}

    def start(self, timeout=15.0):
        conf = self.dir / "sway.conf"
        conf.write_text(sway_config(self.w, self.h))
        env = dict(os.environ)
        for k in ("DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "I3SOCK"):
            env.pop(k, None)
        env["WLR_LIBINPUT_NO_DEVICES"] = "1"
        if self.mode == "headless":
            env["WLR_BACKENDS"] = "headless"
        else:  # a window in the session; WAYLAND_DISPLAY of the maintainer's session is the host
            env["WAYLAND_DISPLAY"] = os.environ.get("WAYLAND_DISPLAY", "wayland-0")
            env["WLR_BACKENDS"] = "wayland"
        cmd = self.scope + [self.sway, "-c", str(conf)]
        self.log = open(self.dir / "compositor.log", "w")
        before = set(Path(self.runtime).glob("wayland-*"))
        self.proc = subprocess.Popen(cmd, stdout=self.log, stderr=subprocess.STDOUT, env=env, start_new_session=True)
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            new = [p for p in set(Path(self.runtime).glob("wayland-*")) - before if not p.name.endswith(".lock")]
            if new:
                self.socket = new[0].name
                time.sleep(0.5)
                return True
            if self.proc.poll() is not None:
                return False
            time.sleep(0.1)
        return False

    def stop(self):
        if self.proc and self.proc.poll() is None:
            try:
                os.killpg(self.proc.pid, signal.SIGTERM)
            except OSError:
                self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        if getattr(self, "log", None):
            self.log.close()
