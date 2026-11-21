# SPDX-License-Identifier: GPL-3.0-only
"""Tests of layout_gate.py (nv-rs#272): a synthetic constant change is seen, so the sync pull request is held."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import layout_gate as lg  # noqa: E402

MEM = """
/// The heap's lower range ends here.
pub const HEAP_BASE: u32 = 0x2000_0000;
pub const HEAP_LOW_END: u32 = 0x7E00_0000; // a note
pub const HEAP_HIGH_BASE: u32 = 0x8000_0000;
pub const HEAP_END: u32 = 0xF000_0000;
pub const PLATFORM_AREA: std::ops::Range<u32> = HEAP_LOW_END..HEAP_HIGH_BASE;
pub const HEAP_LARGEST_BLOCK: u32 = {
    let low = HEAP_LOW_END - HEAP_BASE;
    low
};
pub const GUARD: u32 = 16;
"""


class LayoutGate(unittest.TestCase):
    def test_nothing_changed(self):
        self.assertEqual(lg.changed_constants(MEM, MEM), [])

    def test_a_comment_or_an_unrelated_constant_is_not_a_change(self):
        after = MEM.replace("// a note", "// another note").replace("GUARD: u32 = 16", "GUARD: u32 = 32")
        self.assertEqual(lg.changed_constants(MEM, after), [])

    def test_a_moved_constant_is_named(self):
        after = MEM.replace("0x7E00_0000", "0x7D00_0000")
        self.assertEqual(lg.changed_constants(MEM, after), ["HEAP_LOW_END"])

    def test_a_block_constant_that_changes_inside_is_seen(self):
        after = MEM.replace("let low = HEAP_LOW_END - HEAP_BASE;", "let low = HEAP_LOW_END - HEAP_BASE - 8;")
        self.assertEqual(lg.changed_constants(MEM, after), ["HEAP_LARGEST_BLOCK"])

    def test_an_added_or_removed_constant_is_a_change(self):
        self.assertEqual(lg.changed_constants(MEM, MEM.replace("pub const HEAP_END: u32 = 0xF000_0000;\n", "")), ["HEAP_END"])
        self.assertEqual(lg.changed_constants("", MEM), list(lg.CONSTANTS)[:4] + ["PLATFORM_AREA", "HEAP_LARGEST_BLOCK"])


if __name__ == "__main__":
    unittest.main()
