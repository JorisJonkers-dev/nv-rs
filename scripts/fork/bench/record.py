"""Recording the nested compositor's output of a bench run to a video (wf-recorder; AMD hardware encode when it works).

`Recorder` is started after the nested compositor is up and stopped before it goes down. It writes `record.json`
(file, codec, fps, start and first-frame times for the timeline) next to the video. The video stays in the run
directory: it shows game content, so it is never committed. A size cap stops the recording (the file stays valid).
"""
import json
import os
import signal
import subprocess
import threading
import time
from pathlib import Path

import tools

CAP_DEFAULT = 2 * 1024**3
VAAPI_DEVICE = "/dev/dri/renderD128"


def codec_args(codec, fmt):
    """wf-recorder arguments of an encoder choice: vaapi (hardware), x264 (ultrafast) or vp9 (webm)."""
    if fmt == "webm":
        return ["-c", "libvpx-vp9", "-p", "deadline=realtime", "-p", "cpu-used=8", "-p", "crf=34", "-p", "b=0", "-p", "row-mt=1"]
    if codec == "vaapi":
        return ["-c", "h264_vaapi", "-d", VAAPI_DEVICE, "-F", "scale_vaapi=format=nv12", "-p", "qp=26"]
    return ["-c", "libx264", "-p", "preset=ultrafast", "-p", "crf=28", "-x", "yuv420p"]


def build_command(wf, socket_env_free_output, path, fps, codec, fmt):
    """The wf-recorder command line (environment WAYLAND_DISPLAY names the nested compositor)."""
    cmd = [wf, "-y", "-r", str(fps), "-f", str(path)]
    if socket_env_free_output:
        cmd += ["-o", socket_env_free_output]
    return cmd + codec_args(codec, fmt)


class Recorder:
    def __init__(self, run_dir, socket, runtime, fps=30, fmt="mp4", cap=CAP_DEFAULT, output=None, codec=None):
        self.dir, self.socket, self.runtime = Path(run_dir), socket, runtime
        self.fps, self.fmt, self.cap, self.output = max(1, min(int(fps), 60)), fmt, cap, output
        self.codec = codec or os.environ.get("NV_BENCH_RECORD_CODEC") or ("vaapi" if os.path.exists(VAAPI_DEVICE) else "x264")
        self.path = self.dir / f"screen.{fmt}"
        self.proc = None
        self.start_unix = None
        self.truncated = False
        self.note = ""

    def env(self):
        e = dict(os.environ)
        e.pop("DISPLAY", None)
        e.update(WAYLAND_DISPLAY=self.socket, XDG_RUNTIME_DIR=self.runtime)
        return e

    def spawn(self, codec):
        wf = tools.need("wf-recorder", "record the screen")
        cmd = build_command(wf, self.output, self.path, self.fps, codec, self.fmt)
        self.errlog = open(self.dir / "record.log", "w")
        self.proc = subprocess.Popen(cmd, stdout=self.errlog, stderr=subprocess.STDOUT, env=self.env(), start_new_session=True)
        self.start_unix = time.time()
        # an encoder or output that is refused exits at once
        for _ in range(15):
            time.sleep(0.2)
            if self.proc.poll() is not None:
                return False
        return True

    def start(self):
        """True when recording; False (a note says why) when it could not start: the run goes on unrecorded."""
        try:
            ok = self.spawn(self.codec)
            if not ok and self.codec == "vaapi":
                self.note = "hardware encode refused, fell back to x264"
                self.codec = "x264"
                ok = self.spawn("x264")
        except SystemExit as e:
            self.note = str(e)
            return False
        if not ok:
            self.note = f"wf-recorder exited: see {self.dir / 'record.log'}"
            return False
        threading.Thread(target=self.watch_size, daemon=True).start()
        return True

    def watch_size(self):
        while self.proc and self.proc.poll() is None:
            time.sleep(2)
            try:
                if self.path.stat().st_size > self.cap:
                    self.truncated = True
                    self.interrupt()
                    return
            except OSError:
                pass

    def interrupt(self):
        try:
            os.killpg(self.proc.pid, signal.SIGINT)
        except OSError:
            pass

    def stop(self, first_frame_unix=None):
        if self.proc is None:
            return
        if self.proc.poll() is None:
            self.interrupt()
            try:
                self.proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.errlog.close()
        meta = {
            "file": self.path.name if self.path.exists() else None,
            "codec": self.codec, "fps": self.fps, "format": self.fmt,
            "start_unix": self.start_unix, "first_frame_unix": first_frame_unix,
            "bytes": self.path.stat().st_size if self.path.exists() else 0,
            "truncated": self.truncated, "note": self.note,
        }
        (self.dir / "record.json").write_text(json.dumps(meta, indent=1))
