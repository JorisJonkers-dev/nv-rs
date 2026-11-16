"""Tests of bench watch, record and replay: timeline generation, the replay page, session lookup, recorder commands.

Synthetic run directories only: no game, no compositor, no video.
"""
import json
import os
import re
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "bench"))

import record  # noqa: E402
import replay  # noqa: E402
import timeline  # noqa: E402
import watch  # noqa: E402

HEADER = "frame\tus\tm_fdelta\thk_count\thk_time\tpx\tpy\tpz\tscreen\tqpc\tpoll_qpc\tpresent\tshown\tshown_qpc\tinput_to_present_us"


def make_run(root, name="soak-20260101T000000", n=300, hitch_at=(100, 200), video=True):
    d = Path(root) / name
    d.mkdir(parents=True)
    lines, us = [HEADER], 1_000_000
    for i in range(n):
        us += 40000 if i in hitch_at else 10000
        screen = "menu" if i < 50 else "play"
        lines.append(f"{i}\t{us}\t0.016\t1\t0.0\t0\t0\t0\t{screen}\t0\t0\t0\t0\t0\t")
    (d / "frames.tsv").write_text("\n".join(lines) + "\n")
    # first frame (i=0) is at wall time 1000.0 + 0 ; frame i is at (us_i - us_0) seconds
    t0 = {i: (sum(40000 if k in hitch_at else 10000 for k in range(1, i + 1))) / 1e6 for i in hitch_at}
    csv = ["unix_ms,run_ms,frame,wall_ms,cause"] + [f"{int((1005.0 + t0[i]) * 1000)},1,{i},40.0,io" for i in hitch_at]
    (d / "hitches.csv").write_text("\n".join(csv) + "\n")
    (d / "steps.json").write_text(json.dumps({"scenario": "soak", "steps": [{"frame": 60, "name": "key 1C x2"}]}))
    if video:
        (d / "record.json").write_text(json.dumps({"file": "screen.mp4", "start_unix": 1000.0, "first_frame_unix": 1004.0}))
    return d


class TimelineTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.d = make_run(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_hitches_land_on_the_video_axis(self):
        tl = timeline.build(self.d)
        # the hitch stamps say the first frame was at 1005.0 (not the runner's poll 1004.0): offset 5 s
        self.assertAlmostEqual(tl["offset_s"], 5.0, places=2)
        self.assertEqual([h["frame"] for h in tl["hitches"]], [100, 200])
        self.assertAlmostEqual(tl["hitches"][0]["t"], 5.0 + 0.01 * 99 + 0.04, places=2)
        self.assertEqual(tl["hitches"][0]["cause"], "io")

    def test_screens_and_steps(self):
        tl = timeline.build(self.d)
        self.assertEqual([s["name"] for s in tl["screens"]], ["menu", "play"])
        self.assertEqual(tl["steps"][0]["name"], "key 1C x2")
        self.assertAlmostEqual(tl["steps"][0]["t"] - tl["screens"][1]["t"], 0.1, places=2)

    def test_no_recording_means_zero_offset_and_no_video(self):
        d = make_run(self.tmp.name, "x-20260101T000001", video=False)
        tl = timeline.build(d)
        self.assertIsNone(tl["video"])
        self.assertEqual(tl["offset_s"], 0.0)

    def test_empty_run_is_not_an_error(self):
        d = Path(self.tmp.name) / "empty-20260101T000002"
        d.mkdir()
        tl = timeline.build(d)
        self.assertEqual((tl["frame_count"], tl["hitches"], tl["duration_s"]), (0, [], 0.0))
        replay.page(tl)

    def test_downsample_keeps_spikes(self):
        pts = [(i * 0.01, 10.0) for i in range(60000)]
        pts[31337] = (313.37, 900.0)
        out = timeline.downsample(pts, 1000)
        self.assertLessEqual(len(out), 1100)
        self.assertIn(900.0, [p[1] for p in out])

    def test_tsv_and_ass(self):
        tl = timeline.build(self.d)
        tsv = timeline.to_tsv(tl).splitlines()
        self.assertEqual(tsv[0], "kind\tt_s\tframe\tvalue\tlabel")
        self.assertEqual(sum(1 for l in tsv if l.startswith("hitch\t")), 2)
        ass = timeline.to_ass(tl)
        self.assertEqual(ass.count("HITCH 40 ms io"), 2)
        self.assertIn("screen play", ass)
        self.assertEqual(timeline.ass_time(3725.5), "1:02:05.50")

    def test_malformed_hitch_rows_are_skipped(self):
        rows = timeline.parse_hitches("unix_ms,run_ms,frame,wall_ms,cause\n1,2,3,4.5,x\nbad,row\n")
        self.assertEqual(len(rows), 1)


class ReplayTests(unittest.TestCase):
    def test_page_is_self_contained(self):
        with tempfile.TemporaryDirectory() as t:
            d = make_run(t)
            tl = replay.generate(d)
            html = (d / "replay.html").read_text()
            for f in ("timeline.json", "timeline.tsv", "timeline.ass"):
                self.assertTrue((d / f).exists())
            self.assertNotRegex(html, r"<script[^>]+src=")
            self.assertNotRegex(html, r"https?://")
            self.assertIn('src="screen.mp4"', html)
            data = json.loads(re.search(r"const T=(\{.*?\});\n", html, re.S).group(1))
            self.assertEqual(data["hitches"], tl["hitches"])

    def test_script_close_in_a_label_cannot_break_the_page(self):
        tl = {"run": "r", "video": None, "duration_s": 1, "frames": [], "fps": [], "hitches": [], "screens": [{"t": 0, "name": "</script><b>"}], "steps": []}
        html = replay.page(tl)
        self.assertEqual(html.count("</script>"), 1)

    def test_mpv_command(self):
        self.assertEqual(replay.mpv_command("/r", "screen.mp4", "/r/timeline.ass"), ["mpv", "--sub-file=/r/timeline.ass", "/r/screen.mp4"])


class SessionTests(unittest.TestCase):
    def test_find_by_latest_pid_and_name_and_watch_mark(self):
        with tempfile.TemporaryDirectory() as t:
            a = make_run(t, "soak-20260101T000000")
            b = make_run(t, "soak-20260102T000000")
            (a / "session.json").write_text(json.dumps({"bench_pid": 999999, "compositor_pid": 5, "socket": "w-1", "runtime": t}))
            (b / "session.json").write_text(json.dumps({"bench_pid": os.getpid(), "compositor_pid": 7, "socket": "w-2", "runtime": t}))
            resolve = lambda p: Path(t) / p  # noqa: E731
            self.assertEqual(watch.find_session("latest", t, resolve)[0], b)
            self.assertEqual(watch.find_session(str(os.getpid()), t, resolve)[0], b)
            self.assertEqual(watch.find_session("5", t, resolve)[0], a)
            self.assertEqual(watch.find_session(a.name, t, resolve)[1]["socket"], "w-1")
            with self.assertRaises(SystemExit):
                watch.find_session("424242", t, resolve)
            self.assertFalse(watch.was_watched(b))
            watch.mark_watched(b, "web")
            watch.mark_watched(b, "record")
            self.assertTrue(watch.was_watched(b))
            self.assertEqual(len(json.loads((b / "watch.json").read_text())["attached"]), 2)

    def test_write_steps(self):
        class P:
            name = "s"
            keys = [(10, 0x1C, 2), (5, 0x11, 1)]
            mouse = [(7, 3, -4, 1, 0)]
        with tempfile.TemporaryDirectory() as t:
            watch.write_steps(t, P)
            steps = json.loads((Path(t) / "steps.json").read_text())["steps"]
            self.assertEqual([s["frame"] for s in steps], [5, 7, 10])
            self.assertEqual(steps[1]["name"], "mouse +3,-4")


class RecorderTests(unittest.TestCase):
    def test_commands(self):
        c = record.build_command("wf", "HEADLESS-1", "/r/screen.mp4", 30, "vaapi", "mp4")
        self.assertEqual(c[:6], ["wf", "-y", "-r", "30", "-f", "/r/screen.mp4"])
        self.assertIn("h264_vaapi", c)
        self.assertIn("libx264", record.build_command("wf", None, "x", 60, "x264", "mp4"))
        self.assertNotIn("-o", record.build_command("wf", None, "x", 60, "x264", "mp4"))
        self.assertIn("libvpx-vp9", record.build_command("wf", None, "x.webm", 30, "x264", "webm"))

    def test_fps_is_clamped(self):
        with tempfile.TemporaryDirectory() as t:
            self.assertEqual(record.Recorder(t, "s", t, fps=500).fps, 60)
            self.assertEqual(record.Recorder(t, "s", t, fps=0).fps, 1)

    def test_missing_tool_gives_a_hint_not_a_crash(self):
        with tempfile.TemporaryDirectory() as t:
            old = os.environ.get("PATH")
            os.environ["PATH"] = t
            try:
                r = record.Recorder(t, "s", t)
                self.assertFalse(r.start())
                self.assertIn("nix shell", r.note)
            finally:
                os.environ["PATH"] = old


if __name__ == "__main__":
    unittest.main()
