# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the blind workspace builder (scripts/fork/blind_workspace.py) on a fixture checkout."""
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE))

from test_port_benchmark import frozen_set

import blind_workspace as bw
import port_benchmark as pb

SOURCE = """\
use x;

// Typed from 00401000
/// What it does: a doc the blind attempt must not read.
#[inline]
pub fn first(e: &mut Engine, a: u32) -> u32 {
    let s = "}{"; // a brace in a string and a comment }
    let c = '{';
    if a > 0 {
        return 1;
    }
    a
}

// Typed from 00402000
pub fn wide(
    e: &mut Engine,
    b: u32,
) -> u32 {
    b
}

// Typed from 00403000
pub fn one() -> u32 { 7 }

// Typed from 00404000
    fn indented() {
        helper();
    }
"""


class Braces(unittest.TestCase):
    def test_counts_code_braces_only(self) -> None:
        self.assertEqual(bw.braces("fn f() {"), 1)
        self.assertEqual(bw.braces("}"), -1)
        self.assertEqual(bw.braces('let s = "}{"; // }'), 0)
        self.assertEqual(bw.braces("let c = '{';"), 0)
        self.assertEqual(bw.braces("let c = '\\'';  if x {"), 1)


class Stubbing(unittest.TestCase):
    def test_a_function_with_docs_and_attributes(self) -> None:
        text, head = bw.stub(SOURCE, "00401000")
        self.assertEqual(head, "pub fn first(e: &mut Engine, a: u32) -> u32")
        self.assertIn("// Typed from 00401000\npub fn first(e: &mut Engine, a: u32) -> u32 {\n    todo!(\"port 00401000\")\n}\n", text)
        self.assertNotIn("must not read", text)
        self.assertNotIn("#[inline]", text)
        self.assertNotIn("return 1", text)
        self.assertIn("pub fn wide(", text)  # the neighbours stay
        self.assertTrue(text.endswith("}\n"))

    def test_a_signature_over_several_lines(self) -> None:
        text, head = bw.stub(SOURCE, "00402000")
        self.assertEqual(head, "pub fn wide( e: &mut Engine, b: u32, ) -> u32")
        self.assertIn("    b: u32,\n) -> u32 {\n    todo!(\"port 00402000\")\n}\n", text)
        self.assertNotIn("\n    b\n}", text)

    def test_a_one_line_function_and_an_indented_one(self) -> None:
        text, head = bw.stub(SOURCE, "00403000")
        self.assertEqual(head, "pub fn one() -> u32")
        self.assertIn("pub fn one() -> u32 {\n    todo!(\"port 00403000\")\n}\n", text)
        text, head = bw.stub(SOURCE, "00404000")
        self.assertEqual(head, "fn indented()")
        self.assertIn("        todo!(\"port 00404000\")\n    }", text)

    def test_text_without_a_final_newline_keeps_none(self) -> None:
        text, _ = bw.stub("// Typed from 00401000\nfn a() {}", "00401000")
        self.assertFalse(text.endswith("\n"))

    def test_what_cannot_be_stubbed(self) -> None:
        with self.assertRaisesRegex(ValueError, "no `// Typed from"):
            bw.stub(SOURCE, "00409999")
        with self.assertRaisesRegex(ValueError, "not followed by a function"):
            bw.stub("// Typed from 00401000\n// Typed from 00402000\nfn a() {}\n", "00401000")
        with self.assertRaisesRegex(ValueError, "not followed by a function"):
            bw.stub("// Typed from 00401000\nconst X: u32 = 1;\n", "00401000")
        with self.assertRaisesRegex(ValueError, "no closing brace"):
            bw.stub("// Typed from 00401000\nfn a() {\n    x\n", "00401000")
        with self.assertRaisesRegex(ValueError, "no closing brace"):
            bw.stub("// Typed from 00401000\nfn a();\n", "00401000")


class Building(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name) / "work"
        self.frozen = frozen_set(self.root)
        self.hidden = Path(self.tmp.name) / "hidden"
        self.address = self.frozen["entries"][0]["address"]
        self.entry = self.frozen["entries"][0]

    def test_build_stubs_the_function_and_moves_the_suites_out(self) -> None:
        manifest = bw.build(self.frozen, self.address, self.root, self.hidden)
        source = (self.root / self.entry["source"]).read_text(encoding="utf-8")
        self.assertIn(f'todo!("port {self.address}")', source)
        for suite in self.entry["suites"]:
            self.assertFalse((self.root / suite).exists())
            self.assertTrue((self.hidden / self.address / Path(suite).name).is_file())
        self.assertEqual(manifest["address"], self.address)
        self.assertEqual(manifest["hidden"], [Path(s).name for s in self.entry["suites"]])
        self.assertEqual(manifest["family"], Path(self.entry["suites"][0]).stem.removeprefix("typed_"))
        saved = json.loads((self.hidden / self.address / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(saved, manifest)

    def test_the_address_may_be_written_with_a_prefix_or_capitals(self) -> None:
        manifest = bw.build(self.frozen, "0x" + self.address.upper(), self.root, self.hidden)
        self.assertEqual(manifest["address"], self.address)

    def test_the_hidden_directory_must_be_outside_the_workspace(self) -> None:
        for hidden in (self.root / "hidden", self.root, self.root.parent):
            with self.assertRaisesRegex(ValueError, "outside the workspace"):
                bw.build(self.frozen, self.address, self.root, hidden)
        self.assertTrue((self.root / self.entry["suites"][0]).exists())  # nothing was touched

    def test_a_bad_set_or_address_is_refused_before_anything_moves(self) -> None:
        with self.assertRaises(ValueError):
            bw.build(self.frozen, "00000001", self.root, self.hidden)
        broken = dict(self.frozen, hash="0" * 64)
        with self.assertRaises(ValueError):
            bw.build(broken, self.address, self.root, self.hidden)
        self.assertTrue((self.root / self.entry["suites"][0]).exists())

    def test_entries_that_cannot_be_hidden_safely_are_refused(self) -> None:
        changes: list[dict[str, Any]] = [{"suites": []}, {"suites": ["a/typed_x.rs", "b/typed_x.rs"]},
                                         {"address": "../../etc/x"}]
        for change in changes:
            entry = dict(self.entry, **change)
            frozen = {"entries": [entry] * 1, "hash": ""}
            with mock.patch.object(pb, "hide", return_value=[]), self.assertRaisesRegex(ValueError, "eight-digit address"):
                bw.build(frozen, str(entry["address"]), self.root, self.hidden)
        self.assertTrue((self.root / self.entry["suites"][0]).exists())

    def test_a_source_that_cannot_be_stubbed_moves_nothing(self) -> None:
        (self.root / self.entry["source"]).write_text("nothing here\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            bw.build(self.frozen, self.address, self.root, self.hidden)
        self.assertTrue((self.root / self.entry["suites"][0]).exists())


class Command(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name) / "work"
        self.frozen = frozen_set(self.root)
        self.set = Path(self.tmp.name) / "set.json"
        self.set.write_text(json.dumps(self.frozen), encoding="utf-8")
        self.env = {bw.ENV: str(Path(self.tmp.name) / "hidden")}
        self.address = self.frozen["entries"][0]["address"]

    def test_build_and_hidden(self) -> None:
        out = io.StringIO()
        code = bw.main(["build", str(self.set), self.address, str(self.root)], env=self.env, out=out)
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(out.getvalue())["address"], self.address)
        out = io.StringIO()
        self.assertEqual(bw.main(["hidden", "0x" + self.address], env=self.env, out=out), 0)
        self.assertEqual(out.getvalue().strip(), str(Path(self.env[bw.ENV]).resolve() / self.address))

    def test_errors_exit_two(self) -> None:
        err = io.StringIO()
        with mock.patch.object(sys, "stderr", err):
            self.assertEqual(bw.main(["hidden", self.address], env={}), 2)
            self.assertEqual(bw.main(["build", str(self.set), "00000001", str(self.root)], env=self.env), 2)
            self.assertEqual(bw.main(["build", str(self.set) + "x", self.address, str(self.root)], env=self.env), 2)
        self.assertIn(bw.ENV, err.getvalue())

    def test_the_environment_is_read_when_none_is_given(self) -> None:
        with mock.patch.dict("os.environ", {bw.ENV: self.env[bw.ENV]}):
            self.assertEqual(bw.hidden_root(), Path(self.env[bw.ENV]).resolve())

    def test_the_set_check_is_the_benchmarks(self) -> None:
        self.assertEqual(pb.check(self.frozen), [])


if __name__ == "__main__":
    unittest.main()
