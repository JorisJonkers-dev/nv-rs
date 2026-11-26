# SPDX-License-Identifier: GPL-3.0-only
"""Fork-only. Test the real-gap lister (nv-rs#930). Every input is synthetic source text."""
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import real_gaps as rg  # noqa: E402

SOURCE = """\
// Translation incomplete: 00c55010 - the C++ exception-unwinding frame (FS:[0] chain) is not translated
// Translation incomplete: 00ec1111 - the /GS cookie check is not modelled.
// Translation incomplete: 00ec2222 - SEH unwinding is unsupported.
// Translation incomplete: 00ed282b - the x87 status word's precision flag is not modelled
// Translation incomplete: 00aaa430 - the record's words are uninitialised stack in the exe;
//   they are 0 here, and the exception frame is separate.
// Translation incomplete: 00aaa431 - reads uninitialised stack;
//   the FS:[0] frame is not translated.
//! A module note naming `Translation incomplete` is not a marker.
"""


class RealGapTests(unittest.TestCase):
    def test_exception_only_markers_are_not_real_gaps(self):
        found = dict(rg.markers("x.rs", SOURCE))
        self.assertEqual(set(found), {"00c55010", "00ec1111", "00ec2222", "00ed282b", "00aaa430", "00aaa431"})
        gaps = {a for a, why in found.items() if not rg.is_exception_only(why)}
        self.assertEqual(gaps, {"00ed282b"})

    def test_a_marker_reads_its_continuation_lines(self):
        found = dict(rg.markers("x.rs", SOURCE))
        self.assertIn("exception frame is separate", found["00aaa430"])

    def test_scan_lists_gaps_with_their_file_and_counts_the_rest(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "crates/a/src").mkdir(parents=True)
            (root / "crates/a/src/lib.rs").write_text(SOURCE)
            (root / "crates/a/target").mkdir()
            (root / "crates/a/target/skip.rs").write_text("// Translation incomplete: 00000001 - x87 gap\n")
            gaps, exception_only = rg.scan(root)
        self.assertEqual(gaps, {"00ed282b": "crates/a/src/lib.rs"})
        self.assertEqual(exception_only, {"00c55010", "00ec1111", "00ec2222", "00aaa430", "00aaa431"})

    def test_a_list_round_trips_and_the_check_notices_a_stale_one(self):
        text = rg.render({"00ed282b": "a.rs", "00aaa430": "b.rs"})
        self.assertEqual(rg.parse(text), [0x00AAA430, 0x00ED282B])
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "crates/a").mkdir(parents=True)
            (root / "crates/engine-core/src/typed").mkdir(parents=True)
            (root / "crates/a/lib.rs").write_text("// Translation incomplete: 00ed282b - x87\n")
            self.assertEqual(rg.main(["--root", str(root), "--check"]), 1)
            self.assertEqual(rg.main(["--root", str(root)]), 0)
            self.assertEqual(rg.main(["--root", str(root), "--check"]), 0)

    def test_the_committed_list_is_well_formed(self):
        repo = Path(__file__).resolve().parents[3]
        addresses = rg.parse((repo / rg.OUTPUT).read_text())
        self.assertEqual(addresses, sorted(set(addresses)))
        self.assertGreater(len(addresses), 50)


if __name__ == "__main__":
    unittest.main()
