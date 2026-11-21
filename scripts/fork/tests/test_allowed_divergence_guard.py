# SPDX-License-Identifier: GPL-3.0-only
"""The old `.fork/allowed-divergence` is frozen (nv-rs#438): parallel pull requests that each appended a row to it always
conflicted. A new entry is one file under `.fork/allowed-divergence.d/`. This test fails when the old file changes, so a
pull request that appends a row there is told where the row goes. Changing the file on purpose (a removal that a retired
file needs) means updating FROZEN below with the new digest, in the same pull request, saying why."""
import hashlib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
FROZEN = "71463dd8f11a7df6a858ee31e5e8f8294b4cff61982aaeba71dd98c3afa4f1da"


class AllowedDivergenceGuard(unittest.TestCase):
    def test_the_old_list_is_frozen(self):
        digest = hashlib.sha256((ROOT / ".fork/allowed-divergence").read_bytes()).hexdigest()
        self.assertEqual(
            digest,
            FROZEN,
            "`.fork/allowed-divergence` changed: new entries go in one file each under `.fork/allowed-divergence.d/` "
            "(`path  # reason`), not in this list (nv-rs#438)",
        )

    def test_the_entry_folder_exists_and_holds_only_entry_files(self):
        folder = ROOT / ".fork/allowed-divergence.d"
        self.assertTrue(folder.is_dir())
        for f in folder.iterdir():
            self.assertTrue(f.is_file(), f)


if __name__ == "__main__":
    unittest.main()
