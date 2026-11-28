"""Fork-only. Test the mapper from backend harness lines to ledger evidence (nv-rs#928).

Every input is made in a temporary directory: no game files, no backend and no cargo.
"""
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import backend_evidence as be  # noqa: E402

HEADER = "# verified peers and rejected functions\n" + be.HEADER + "\n"


class BackendEvidenceTests(unittest.TestCase):
    def test_parse_keeps_the_five_classes(self):
        results = be.parse([
            "backend-pass: 00ef1000",
            "backend-fail: 00ef2000 float-only",
            "backend-fail: 00ef3000 uninitialised read",
            "backend-fail: 00ef4000 hang",
            "backend-fail: 00ef5000 fault",
            "backend-fail: 00ef6000 real",
            "an unrelated test line",
        ])
        self.assertEqual(results, {
            "00ef1000": (True, ""),
            "00ef2000": (False, "float-only"),
            "00ef3000": (False, "uninitialised read"),
            "00ef4000": (False, "hang"),
            "00ef5000": (False, "fault"),
            "00ef6000": (False, "real"),
        })

    def test_a_mismatch_wins_over_a_pass_on_the_same_address(self):
        results = be.parse(["backend-pass: 00ef1000", "backend-fail: 00ef1000 real"])
        self.assertEqual(results, {"00ef1000": (False, "real")})

    def test_an_unknown_class_is_not_evidence(self):
        self.assertEqual(be.parse(["backend-fail: 00ef1000 nonsense"]), {})

    def test_merge_keeps_other_rows_and_their_columns(self):
        text = HEADER + "00aa0000\t1\t\t0\t0\t0\n00bb0000\t0\treal\t1\t1\t2\n"
        merged = be.merge(be.parse([
            "backend-pass: 00ef1000",
            "backend-fail: 00bb0000 float-only",
        ]), text)
        self.assertTrue(merged.startswith(HEADER))
        rows = {line.split("\t")[0]: line.split("\t") for line in merged.splitlines()
                if line and not line.startswith("#") and not line.startswith("address\t")}
        self.assertEqual(rows["00aa0000"][1:], ["1", "", "0", "0", "0"])
        self.assertEqual(rows["00bb0000"][1:], ["0", "float-only", "1", "1", "2"])
        self.assertEqual(rows["00ef1000"][1:], ["1", "", "0", "0", "0"])

    def test_main_requires_some_backend_case(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lines, evidence = root / "out.log", root / "function_evidence.tsv"
            lines.write_text("no backend lines\n")
            self.assertEqual(be.main(["--lines", str(lines), "--evidence", str(evidence), "--require"]), 1)
            self.assertFalse(evidence.exists())

    def test_main_writes_the_evidence_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lines, evidence = root / "out.log", root / "function_evidence.tsv"
            lines.write_text("backend-pass: 00ef1000\nbackend-fail: 00ef2000 hang\n")
            evidence.write_text(HEADER)
            self.assertEqual(be.main(["--lines", str(lines), "--evidence", str(evidence), "--require"]), 0)
            body = evidence.read_text()
            self.assertIn("00ef1000\t1\t\t0\t0\t0", body)
            self.assertIn("00ef2000\t0\thang\t0\t0\t0", body)
            # A second run with no new lines leaves the file alone.
            lines.write_text("backend-pass: 00ef1000\n")
            be.main(["--lines", str(lines), "--evidence", str(evidence)])
            self.assertEqual(evidence.read_text(), body)


if __name__ == "__main__":
    unittest.main()
