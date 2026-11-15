"""Tests of scripts/fork/desktop-hint.sh with stand-ins for the compositors' command line tools.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v        (no network, no display, nothing created)

The script runs with a PATH of its own: links to the few system tools it needs plus stub tools that print what the real
ones print (shapes taken from hyprctl, swaymsg, kscreen-doctor, gdbus and xrandr). A missing tool must leave no output.
"""
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "desktop-hint.sh"
BASICS = ["bash", "env", "awk", "sed", "tr", "grep", "head", "python3", "dirname", "cat", "printf"]

HYPR = '[{"id":0,"name":"DP-1","focused":false},{"id":1,"name":"HDMI-A-1","focused":true}]'
SWAY = '[{"name":"eDP-1","focused":false},{"name":"DP-2","focused":true}]'
KSCREEN = (
    "\x1b[01;32mOutput: \x1b[0;0m1 DP-3 enabled connected priority 2 DisplayPort\n"
    "Output: 2 HDMI-A-1 enabled connected priority 1 HDMI\n"
    "Output: 3 DP-4 disabled disconnected priority 0 DisplayPort\n"
)
GNOME = (
    "(uint32 3, [('DP-1', 'GSM', 'LG', '1')], [(0, 0, 1.0, uint32 0, false, [('DP-1', 'GSM', 'LG', '1')], {}),"
    " (2560, 0, 1.0, uint32 0, true, [('HDMI-1', 'DEL', 'Dell', '2')], {})], {})\n"
)
XRANDR = (
    "Screen 0: minimum 8 x 8, current 4480 x 1440\n"
    "DP-1 connected 1920x1080+0+0 (normal left inverted) 527mm x 296mm\n"
    "HDMI-1 connected primary 2560x1440+1920+0 (normal left inverted) 597mm x 336mm\n"
    "DP-2 disconnected (normal left inverted right x axis y axis)\n"
)


class Hint(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.bin = Path(self.tmp.name)
        for tool in BASICS:
            found = shutil.which(tool)
            if found:
                (self.bin / tool).symlink_to(found)

    def tearDown(self):
        self.tmp.cleanup()

    def stub(self, name, output):
        f = self.bin / name
        f.write_text(f"#!/usr/bin/env bash\ncat <<'END'\n{output}\nEND\n")
        f.chmod(0o755)

    def run_hint(self, args=(), **env):
        e = {"PATH": str(self.bin), "HOME": self.tmp.name}
        e.update(env)
        r = subprocess.run(
            [str(self.bin / "bash"), str(SCRIPT), *args], env=e, capture_output=True, text=True, timeout=30
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout.strip()

    def test_the_desktop_is_told_from_the_environment(self):
        kind = lambda **e: self.run_hint(["kind"], **e)  # noqa: E731
        self.assertEqual(kind(HYPRLAND_INSTANCE_SIGNATURE="x", XDG_CURRENT_DESKTOP="GNOME"), "hyprland")
        self.assertEqual(kind(SWAYSOCK="/run/sway.sock"), "sway")
        self.assertEqual(kind(XDG_CURRENT_DESKTOP="KDE", WAYLAND_DISPLAY="wayland-0"), "kde")
        self.assertEqual(kind(XDG_CURRENT_DESKTOP="ubuntu:GNOME"), "gnome")
        self.assertEqual(kind(XDG_CURRENT_DESKTOP="XFCE", XDG_SESSION_TYPE="x11", DISPLAY=":0"), "x11")
        self.assertEqual(kind(XDG_CURRENT_DESKTOP="weston", WAYLAND_DISPLAY="wayland-0"), "")

    def test_hyprland_names_the_focused_monitor(self):
        self.stub("hyprctl", HYPR)
        self.assertEqual(self.run_hint(["output"], HYPRLAND_INSTANCE_SIGNATURE="x"), "HDMI-A-1")

    def test_sway_names_the_focused_output(self):
        self.stub("swaymsg", SWAY)
        self.assertEqual(self.run_hint(["output"], SWAYSOCK="/s"), "DP-2")

    def test_kde_names_the_primary_and_ignores_colour(self):
        self.stub("kscreen-doctor", KSCREEN)
        self.assertEqual(self.run_hint(["output"], XDG_CURRENT_DESKTOP="KDE"), "HDMI-A-1")

    def test_gnome_names_the_primary_monitor(self):
        self.stub("gdbus", GNOME)
        self.assertEqual(self.run_hint(["output"], XDG_CURRENT_DESKTOP="GNOME"), "HDMI-1")

    def test_x11_names_the_output_under_the_pointer_else_the_primary(self):
        self.stub("xrandr", XRANDR)
        env = {"XDG_SESSION_TYPE": "x11", "DISPLAY": ":0"}
        self.assertEqual(self.run_hint(["output"], **env), "HDMI-1")
        self.stub("xdotool", "X=100\nY=200\nSCREEN=0\nWINDOW=1")
        self.assertEqual(self.run_hint(["output"], **env), "DP-1")
        self.stub("xdotool", "X=3000\nY=200\nSCREEN=0\nWINDOW=1")
        self.assertEqual(self.run_hint(["output"], **env), "HDMI-1")

    def test_a_missing_tool_is_silent(self):
        for env in (
            {"HYPRLAND_INSTANCE_SIGNATURE": "x"},
            {"SWAYSOCK": "/s"},
            {"XDG_CURRENT_DESKTOP": "KDE"},
            {"XDG_CURRENT_DESKTOP": "GNOME"},
            {"XDG_SESSION_TYPE": "x11", "DISPLAY": ":0"},
            {},
        ):
            self.assertEqual(self.run_hint(["output"], **env), "", env)
            self.assertEqual(self.run_hint(["export"], **env), "", env)

    def test_garbage_from_a_tool_is_silent(self):
        self.stub("hyprctl", "not json")
        self.assertEqual(self.run_hint(["output"], HYPRLAND_INSTANCE_SIGNATURE="x"), "")

    def test_nv_monitor_is_set_only_when_unset(self):
        self.stub("hyprctl", HYPR)
        env = {"HYPRLAND_INSTANCE_SIGNATURE": "x"}
        self.assertEqual(self.run_hint(["export"], **env), "NV_MONITOR=HDMI-A-1")
        self.assertEqual(self.run_hint(["export"], NV_MONITOR="DP-1", **env), "NV_MONITOR=DP-1")
        self.assertEqual(self.run_hint(["export"], NV_MONITOR="", **env), "NV_MONITOR=HDMI-A-1")

    def test_kde_and_gnome_leave_placement_to_the_compositor_unless_asked(self):
        self.stub("gdbus", GNOME)
        self.assertEqual(self.run_hint(["export"], XDG_CURRENT_DESKTOP="GNOME"), "")
        self.assertEqual(
            self.run_hint(["export"], XDG_CURRENT_DESKTOP="GNOME", NV_DESKTOP_HINT_PRIMARY="1"), "NV_MONITOR=HDMI-1"
        )

    def test_sourcing_exports_into_the_callers_shell(self):
        self.stub("swaymsg", SWAY)
        r = subprocess.run(
            [str(self.bin / "bash"), "-c", f'. "{SCRIPT}"; nv_desktop_export; bash -c \'echo "[$NV_MONITOR]"\''],
            env={"PATH": str(self.bin), "SWAYSOCK": "/s", "HOME": self.tmp.name},
            capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(r.stdout.strip(), "[DP-2]", r.stderr)


if __name__ == "__main__":
    unittest.main()
