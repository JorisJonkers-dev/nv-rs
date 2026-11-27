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
        self.assertEqual(gaps, {"00ed282b", "00aaa430", "00aaa431"})

    def test_a_marker_that_also_names_a_real_gap_is_one(self):
        self.assertTrue(rg.is_exception_only("the /GS cookie check is not modeled."))
        self.assertFalse(rg.is_exception_only("the FS:[0] frame is not translated; slot 0xA4 is uninitialised stack"))
        self.assertFalse(rg.is_exception_only("SEH frame is not translated, and MXCSR values other than 0x1f80 are not modelled"))

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
        self.assertEqual(set(gaps), {"00ed282b", "00aaa430", "00aaa431"})
        self.assertEqual(exception_only, {"00c55010", "00ec1111", "00ec2222"})

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

    def test_the_committed_list_is_what_the_tree_generates(self):
        repo = Path(__file__).resolve().parents[3]
        gaps, _ = rg.scan(repo)
        self.assertEqual((repo / rg.OUTPUT).read_text(), rg.render(gaps))
        self.assertIn("Do not edit; run", rg.render(gaps))

    def test_each_marker_is_classified_by_what_it_says_is_left_out(self):
        repo = Path(__file__).resolve().parents[3]
        gaps, exception_only = rg.scan(repo)
        for address in ("00c65a80", "00ed6af6", "00daf540", "00b6d370", "00e39ab0", "00aaa430"):
            self.assertIn(address, gaps, address)
        # Only the frame or the cookie is left out: not a real gap.
        self.assertIn("00c55010", exception_only)
        # The frame and an uninitialised read: a real gap.
        self.assertIn("00ca77f0", gaps)
        self.assertNotIn("00ca77f0", exception_only)


if __name__ == "__main__":
    unittest.main()


class ResultColumnTests(unittest.TestCase):
    """What the backend's one-word export can answer, decided from the marker and the translation."""

    def test_reworded_mixed_markers_are_real_gaps(self):
        for why in (
            "the FS:[0] frame is not modeled, and the new block's slots are read before they are written",
            "the FS:[0] exception frame is not modeled; the third argument keeps three bytes of whatever register the exe left there",
            "the /GS cookie check is not modeled; the record is left as stack garbage except for its first word",
            "the /GS cookie check (00ec408c) is not modeled; the ray records are whatever the stack held",
            "the FS:[0] frame is not modeled; the word at +0xC, which the exe never writes, is zero here",
            "the caller's registers and EFLAGS are machine state the engine does not have; the /GS cookie check is not translated",
        ):
            self.assertFalse(rg.is_exception_only(why), why)

    def test_frame_and_cookie_prose_stays_exception_only(self):
        for why in (
            "the FS:[0] handler is 00f22f4e and calls filter 00f22f4b; the frame is not translated",
            "filters and the __except block are entered with EBP = the node + 0x10",
            "the C++ exception frame (FS:[0] chain, unwind states 0 to 3) and the security-cookie bookkeeping are not translated",
        ):
            self.assertTrue(rg.is_exception_only(why), why)

    def test_register_state_in_the_marker_keeps_a_gap_on_its_translation(self):
        body = "pub fn f(e: &mut Engine, this: Ptr) -> u32 {\n    0\n}\n"
        for why in (
            "takes EDI (the world) as a register input, which has no uniform-ABI form",
            "lane 3 of the axis vector is left over from an earlier XMM2 register; zero here",
            "the upper three bytes of the flag word are the stale ECX left by 00d61d00",
            "the x87 status word (FSTSW) is not modelled",
            "the answer is returned in EDX",
        ):
            self.assertFalse(rg.one_word(why, body), why)
        self.assertTrue(rg.one_word("reads uninitialised stack words (zero here)", body))

    def test_a_wide_or_float_result_keeps_a_gap_on_its_translation(self):
        why = "reads uninitialised stack"
        for sig in ("-> u64", "-> f64", "-> Ret"):
            self.assertFalse(rg.one_word(why, "pub fn f(e: &mut Engine) %s {\n    0\n}\n" % sig), sig)
        self.assertFalse(rg.one_word(why, "fn f() {\n    Ret { eax: 1, edx: 2, st0: 0.0 }\n}\n"))
        # A local named edx, or a callee's st0 that is only read, is not a result.
        self.assertTrue(rg.one_word(why, "fn f(e: &mut Engine) {\n    let mut edx = 1;\n    let x = e.call(1, &[]).st0;\n}\n"))

    def test_scan_reads_the_function_after_the_marker(self):
        text = (
            "// Translation incomplete: 00aaa001 - reads uninitialised stack\n"
            "/// doc\n"
            "pub fn wide(e: &mut Engine) -> u64 {\n    1\n}\n"
            "// Translation incomplete: 00aaa002 - reads uninitialised stack\n"
            "pub fn narrow(e: &mut Engine) -> u32 {\n    1\n}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "crates/a").mkdir(parents=True)
            (root / "crates/a/lib.rs").write_text(text)
            gaps, _ = rg.scan(root)
        self.assertEqual(gaps, {"00aaa001": ("crates/a/lib.rs", False), "00aaa002": ("crates/a/lib.rs", True)})
        self.assertIn("00aaa001 crates/a/lib.rs registers", rg.render(gaps))
        self.assertIn("00aaa002 crates/a/lib.rs eax", rg.render(gaps))

    def test_the_committed_list_keeps_register_gaps_off_the_export(self):
        repo = Path(__file__).resolve().parents[3]
        rows = {line.split()[0]: line.split()[2] for line in (repo / rg.OUTPUT).read_text().splitlines()
                if line and not line.startswith("#")}
        for address in ("00c90f40", "00c912e0", "00c927c0", "00c92860", "00c92950",  # hkpWorld, register inputs
                        "00d68030", "00e27c90",                                       # stale ECX, XMM lane
                        "00ed6af6", "00ed5a9a"):                                      # x87
            self.assertEqual(rows[address], "registers", address)
        for address in ("00c65a80", "00e191a0", "00ca47f0", "00c806b0", "00ed159c" ):
            self.assertIn(address, rows, address)
        self.assertEqual(rows["00e191a0"], "eax")
