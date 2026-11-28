# SPDX-License-Identifier: GPL-3.0-only
"""test_archive.py: package names, the archive's manifest and its extraction."""
import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import test_archive  # noqa: E402


class Archive(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.arch = self.tmp / "archive"
        exe = self.arch / "target/debug/deps/typed_a-1"
        exe.parent.mkdir(parents=True)
        exe.write_text("binary")
        manifest = [{"crate": "engine", "name": "typed_a", "kind": "test", "executable": "target/debug/deps/typed_a-1", "cwd": "crates/engine"},
                    {"crate": "engine", "name": "typed_a", "kind": "bin", "executable": "target/debug/deps/other", "cwd": "crates/engine"}]
        (self.arch / "archive.json").write_text(json.dumps({"binaries": manifest}))

    def test_the_package_name_is_the_folder_or_the_name_before_the_version(self):
        self.assertEqual(test_archive.package_name("path+file:///r/crates/engine#0.1.0"), "engine")
        self.assertEqual(test_archive.package_name("path+file:///r/crates/nv-viewer#nv_viewer@0.1.0"), "nv_viewer")
        self.assertEqual(test_archive.package_name("path+file:///D:/a/r/crates/engine-core#0.1.0"), "engine-core")

    def test_find_names_the_test_binary_and_its_folder_and_misses_a_binary_or_another_crate(self):
        self.assertEqual(test_archive.find(str(self.arch), "engine", "typed_a"), 0)
        self.assertEqual(test_archive.find(str(self.arch), "engine", "typed_b"), 1)
        self.assertEqual(test_archive.find(str(self.arch), "host", "typed_a"), 1)

    def test_extract_puts_the_binaries_back_at_their_workspace_paths_executable(self):
        root = self.tmp / "checkout"
        root.mkdir()
        # extract copies listed files only: the second entry has no file, so give it one
        (self.arch / "target/debug/deps/other").write_text("x")
        test_archive.extract(str(self.arch), str(root))
        got = root / "target/debug/deps/typed_a-1"
        self.assertEqual(got.read_text(), "binary")
        self.assertTrue(os.access(got, os.X_OK))


if __name__ == "__main__":
    unittest.main()
