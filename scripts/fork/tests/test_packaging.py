"""Self-test of packaging/linux (no build, no network): the staged tree and tarball obey what nvmod's install-nv-rs
requires, the install script and the launcher work, and smoke-test.sh passes on a stand-in nv-viewer that behaves as the
real one does for the cases the smoke test runs (the real binary is run by the release build, fork-release-build.yml)."""

import json
import os
import pathlib
import subprocess
import tarfile
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]
PKG = ROOT / "packaging" / "linux"

STAND_IN = r"""#!/bin/sh
case "${1:-}" in
--help) echo "nv-viewer - run Fallout: New Vegas through the engine port"; exit 0 ;;
"") echo "error: the game's Data folder is missing" >&2; exit 2 ;;
esac
data=""
for d in "$1" "$1/Data"; do
    [ -f "$d/FalloutNV.esm" ] && { data="$d"; break; }
done
[ -n "$data" ] || { echo "error: neither $1 nor a Data folder inside it contains FalloutNV.esm" >&2; exit 1; }
exe="$(dirname "$data")/FalloutNV.exe"
[ -f "$exe" ] || { echo "error: $exe: not found" >&2; exit 1; }
echo "error: $exe: other builds are not supported. nv-rs needs FalloutNV.exe version 1.4.0.525" >&2
exit 1
"""


def run(*args, **kw):
    return subprocess.run([str(a) for a in args], capture_output=True, text=True, **kw)


class Packaging(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.dir = pathlib.Path(cls.tmp.name)
        viewer = cls.dir / "nv-viewer"
        viewer.write_text(STAND_IN)
        viewer.chmod(0o755)
        files = cls.dir / "files"
        files.mkdir()
        (files / "release-manifest.json").write_text(json.dumps({"schema_version": 1}))
        (files / "capabilities.json").write_text("{}")
        out = cls.dir / "out"
        done = run(PKG / "package.sh", viewer, "2026-11-14.1", out, files)
        assert done.returncode == 0, done.stderr
        cls.tarball = out / "nv-rs-linux-x86_64-dev.tar.gz"
        cls.unpacked = cls.dir / "unpacked"
        with tarfile.open(cls.tarball) as t:
            cls.members = t.getmembers()
            t.extractall(cls.unpacked, filter="data")
        cls.tree = cls.unpacked / "nv-rs-2026-11-14.1-linux-x86_64"

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_tarball_name_is_the_one_nvmod_fetches(self):
        # crates/nvmod-install/src/install.rs, DEFAULT_ASSET
        self.assertEqual(self.tarball.name, "nv-rs-linux-x86_64-dev.tar.gz")

    def test_members_are_plain_and_stay_inside(self):
        # crates/nvmod-install/src/release.rs, extract(): no link, no absolute path, no `..`
        for m in self.members:
            self.assertTrue(m.isfile() or m.isdir(), m.name)
            p = pathlib.PurePosixPath(m.name)
            self.assertFalse(p.is_absolute() or ".." in p.parts, m.name)

    def test_layout(self):
        for rel in [
            "bin/nv-viewer", "bin/nv-rs-launch", "install.sh", "share/applications/nv-rs.desktop",
            "share/icons/hicolor/256x256/apps/nv-rs.png", "share/doc/nv-rs/README.txt",
            "share/nv-rs/release-manifest.json", "share/nv-rs/capabilities.json",
        ]:
            self.assertTrue((self.tree / rel).is_file(), rel)
        # nvmod finds nv-viewer and the manifest by name anywhere in the tree: exactly one of each
        names = [m.name.rsplit("/", 1)[-1] for m in self.members if m.isfile()]
        self.assertEqual(names.count("nv-viewer"), 1)
        self.assertEqual(names.count("release-manifest.json"), 1)

    def test_build_is_reproducible(self):
        again = self.dir / "again"
        run(PKG / "package.sh", self.dir / "nv-viewer", "2026-11-14.1", again, self.dir / "files")
        self.assertEqual((again / self.tarball.name).read_bytes(), self.tarball.read_bytes())

    def test_desktop_entry(self):
        fields = dict(
            line.split("=", 1) for line in (PKG / "nv-rs.desktop").read_text().splitlines()
            if "=" in line
        )
        for key in ("Type", "Name", "Exec", "Icon", "Categories"):
            self.assertIn(key, fields)
        self.assertEqual(fields["Type"], "Application")
        self.assertTrue(fields["Categories"].endswith(";"))
        self.assertEqual(fields["Icon"], "nv-rs")

    def test_install_script(self):
        prefix = self.dir / "prefix"
        done = run(self.tree / "install.sh", "--prefix", prefix)
        self.assertEqual(done.returncode, 0, done.stderr)
        for rel in ["bin/nv-viewer", "bin/nv-rs-launch", "share/icons/hicolor/256x256/apps/nv-rs.png"]:
            self.assertTrue((prefix / rel).is_file(), rel)
        desktop = (prefix / "share/applications/nv-rs.desktop").read_text()
        self.assertIn(f"Exec={prefix}/bin/nv-rs-launch", desktop)
        self.assertEqual(run(self.tree / "install.sh", "--bogus").returncode, 2)

    def test_smoke_test_passes_and_catches_a_wrong_exit(self):
        done = run(PKG / "smoke-test.sh", self.tree)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertIn("smoke test passed", done.stdout)
        broken = self.dir / "broken"
        (broken / "bin").mkdir(parents=True)
        (broken / "bin" / "nv-viewer").write_text("#!/bin/sh\nexit 0\n")
        (broken / "bin" / "nv-viewer").chmod(0o755)
        done = run(PKG / "smoke-test.sh", broken)
        self.assertEqual(done.returncode, 1)
        self.assertIn("FAIL", done.stdout)


if __name__ == "__main__":
    unittest.main()
