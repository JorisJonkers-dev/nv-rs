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

import blind_workspace as bw
import port_benchmark as pb
from test_port_benchmark import frozen_set

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


def suite_text(address: str, function: str) -> str:
    """A suite shaped like the real ones: imports, a helper, a table, a macro, tests of other functions and of this one."""
    literal = f"0x{address[:4]}_{address[4:]}"
    return f"""//! The suite.
use engine_core::Engine;

const TABLE: &[u32] = &[
    1,
    2, // a bracket in a comment ]
];

fn helper(e: &mut Engine) -> u32 {{
    let s = "}}{{";
    e.mem.u32(0)
}}

macro_rules! twice {{
    ($e:expr) => {{ ($e, $e) }};
}}

// ---- the others ----

#[test]
fn other() {{
    Diff::new(entry!(0x0040_9990, fam::other() -> u32))
        .run()
        .assert_same();
}}

/// By literal.
#[test]
fn by_literal() {{
    Diff::new(entry!({literal}, fam::{function}() -> u32))
        .run()
        .assert_same();
}}

#[test]
fn by_name() {{
    let _ = {function}(e);
}}

#[test]
fn unrelated_{function}x() {{ helper(e); }}

const AFTER: u32 = {literal}; // a table row naming the address
const AGAIN: [u32; 2] = [{literal}, {literal}]; // two on one line count as two
const NAMED: &str = "{function}"; // a use of the name that is no test
"""


class Items(unittest.TestCase):
    def test_the_items_are_the_text_and_attributes_and_comments_go_with_the_item_below(self) -> None:
        text = suite_text("00400000", "f0")
        chunks = bw.items(text)
        self.assertEqual("".join(chunks), text)
        self.assertEqual(len(chunks), 11)
        self.assertTrue(chunks[5].startswith("\n/// By literal.\n#[test]\nfn by_literal"))
        self.assertTrue(chunks[0].endswith("use engine_core::Engine;\n"))

    def test_text_that_does_not_end_in_an_item_is_one_last_chunk(self) -> None:
        self.assertEqual(bw.items("fn a() {}\n// a note\n"), ["fn a() {}\n", "// a note\n"])
        self.assertEqual(bw.items(""), [])

    def test_unbalanced_brackets_are_refused(self) -> None:
        for text in ("fn a() {\n", "fn a() {}\n}\n", "const A: [u32; 1 = [1];\n"):
            with self.assertRaisesRegex(ValueError, "do not balance"):
                bw.items(text)


class Splitting(unittest.TestCase):
    def test_only_the_tests_that_name_the_address_or_the_function_are_cut_out(self) -> None:
        text = suite_text("00400000", "f0")
        kept, hidden, names = bw.split_suite(text, "00400000", "f0")
        self.assertEqual(names, ["by_literal", "by_name"])
        for needle in ("fn other()", "fn helper", "macro_rules! twice", "const TABLE", "unrelated_f0x", "const AFTER"):
            self.assertIn(needle, kept)
        for needle in ("fn helper", "macro_rules! twice", "const TABLE", "const AFTER", "use engine_core::Engine"):
            self.assertIn(needle, hidden)
        self.assertNotIn("fn by_literal", kept)
        self.assertNotIn("fn by_name", kept)
        self.assertIn("fn by_literal", hidden)
        self.assertIn("fn by_name", hidden)
        self.assertNotIn("fn other()", hidden)  # the hidden suite holds the items and only the tests of the function
        self.assertNotIn("unrelated_f0x", hidden)
        self.assertEqual(kept.count("#[test]"), 2)

    def test_a_test_with_no_function_name_is_listed_by_a_question_mark(self) -> None:
        _, _, names = bw.split_suite("#[test]\nconst X: u32 = 0x0040_0000;\n", "00400000", "f0")
        self.assertEqual(names, ["?"])

    def test_a_suite_without_such_tests_is_returned_whole(self) -> None:
        text = suite_text("00400000", "f0")
        kept, hidden, names = bw.split_suite(text, "00400004", "g")
        self.assertEqual((kept, names), (text, []))
        self.assertNotIn("fn other", hidden)


class Building(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name) / "work"
        self.frozen = frozen_set(self.root)
        self.hidden = Path(self.tmp.name) / "hidden"
        self.entry = next(e for e in self.frozen["entries"] if len(e["suites"]) == 1)
        self.address = self.entry["address"]
        source = (self.root / self.entry["source"]).read_text(encoding="utf-8")
        self.function = source.split(f"// Typed from {self.address}\npub fn ", 1)[1].split("(", 1)[0]
        self.suite = self.root / self.entry["suites"][0]
        self.suite.write_text(suite_text(self.address, self.function), encoding="utf-8")

    def test_build_stubs_the_function_and_cuts_only_its_tests_out(self) -> None:
        manifest = bw.build(self.frozen, self.address, self.root, self.hidden)
        source = (self.root / self.entry["source"]).read_text(encoding="utf-8")
        self.assertIn(f'todo!("port {self.address}")', source)
        kept = self.suite.read_text(encoding="utf-8")
        self.assertIn("fn other()", kept)
        self.assertIn("fn helper", kept)
        self.assertNotIn("fn by_literal", kept)
        hidden = (self.hidden / self.address / self.suite.name).read_text(encoding="utf-8")
        self.assertIn("fn by_literal", hidden)
        self.assertNotIn("fn other()", hidden)
        self.assertEqual(manifest["address"], self.address)
        self.assertEqual(manifest["function"], self.function)
        self.assertEqual(manifest["tests"], ["by_literal", "by_name"])
        self.assertEqual(manifest["hidden"], [self.suite.name])
        self.assertEqual(manifest["suite"], self.entry["suites"][0])
        self.assertEqual(manifest["mentions_left"], 3)  # the table row and the two on one line
        self.assertEqual(manifest["calls_left"], 1)  # the name in a constant: a nested test or helper would show here
        self.assertEqual(manifest["family"], self.suite.stem.removeprefix("typed_"))
        saved = json.loads((self.hidden / self.address / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(saved, manifest)

    def test_the_workspace_allows_the_divergence_a_re_port_makes_from_upstream(self) -> None:
        manifest = bw.build(self.frozen, self.address, self.root, self.hidden)
        entry = self.root / ".fork" / "allowed-divergence.d" / f"bench-{self.address}"
        lines = entry.read_text(encoding="utf-8").splitlines()
        self.assertEqual([line.split("#")[0].strip() for line in lines], [self.entry["source"], manifest["suite"]])
        self.assertTrue(all(f"re-port of {self.address}" in line for line in lines))
        self.assertEqual(manifest["allowed_divergence"], f".fork/allowed-divergence.d/bench-{self.address}")

    def test_a_suite_listed_twice_is_cut_once(self) -> None:
        entry = dict(self.entry, suites=[self.entry["suites"][0]] * 2)
        frozen = dict(self.frozen, entries=[entry])
        with mock.patch.object(pb, "hide", return_value=[]):
            manifest = bw.build(frozen, self.address, self.root, self.hidden)
        self.assertEqual(manifest["tests"], ["by_literal", "by_name"])

    def test_the_address_may_be_written_with_a_prefix_or_capitals(self) -> None:
        manifest = bw.build(self.frozen, "0x" + self.address.upper(), self.root, self.hidden)
        self.assertEqual(manifest["address"], self.address)

    def test_the_hidden_directory_must_be_outside_the_workspace(self) -> None:
        before = self.suite.read_text(encoding="utf-8")
        for hidden in (self.root / "hidden", self.root, self.root.parent):
            with self.assertRaisesRegex(ValueError, "outside the workspace"):
                bw.build(self.frozen, self.address, self.root, hidden)
        self.assertEqual(self.suite.read_text(encoding="utf-8"), before)  # nothing was touched

    def test_a_bad_set_or_address_is_refused_before_anything_moves(self) -> None:
        before = self.suite.read_text(encoding="utf-8")
        with self.assertRaises(ValueError):
            bw.build(self.frozen, "00000001", self.root, self.hidden)
        broken = dict(self.frozen, hash="0" * 64)
        with self.assertRaises(ValueError):
            bw.build(broken, self.address, self.root, self.hidden)
        self.assertEqual(self.suite.read_text(encoding="utf-8"), before)

    def test_entries_that_cannot_be_hidden_safely_are_refused(self) -> None:
        before = self.suite.read_text(encoding="utf-8")
        changes: list[dict[str, Any]] = [{"suites": []}, {"suites": ["a/typed_x.rs", "b/typed_x.rs"]},
                                         {"address": "../../etc/x"}]
        for change in changes:
            entry = dict(self.entry, **change)
            frozen = {"entries": [entry] * 1, "hash": ""}
            with mock.patch.object(pb, "hide", return_value=[]), self.assertRaisesRegex(ValueError, "eight-digit address"):
                bw.build(frozen, str(entry["address"]), self.root, self.hidden)
        self.assertEqual(self.suite.read_text(encoding="utf-8"), before)

    def test_a_source_that_cannot_be_stubbed_changes_nothing(self) -> None:
        before = self.suite.read_text(encoding="utf-8")
        (self.root / self.entry["source"]).write_text("nothing here\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            bw.build(self.frozen, self.address, self.root, self.hidden)
        self.assertEqual(self.suite.read_text(encoding="utf-8"), before)

    def test_a_suite_with_no_test_of_the_function_is_refused(self) -> None:
        self.suite.write_text("const X: u32 = 1;\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "nothing could score it"):
            bw.build(self.frozen, self.address, self.root, self.hidden)
        self.assertFalse(self.hidden.exists())

    def test_tests_in_two_suites_are_refused(self) -> None:
        other = self.root / "crates/engine/tests/typed_second.rs"
        other.write_text(suite_text(self.address, self.function), encoding="utf-8")
        entry = dict(self.entry, suites=[self.entry["suites"][0], "crates/engine/tests/typed_second.rs"])
        frozen = dict(self.frozen, entries=[entry])
        before = self.suite.read_text(encoding="utf-8")
        with mock.patch.object(pb, "hide", return_value=[]), self.assertRaisesRegex(ValueError, "one hidden suite"):
            bw.build(frozen, self.address, self.root, self.hidden)
        self.assertEqual(self.suite.read_text(encoding="utf-8"), before)
        self.assertFalse(self.hidden.exists())

    def test_a_suite_that_cannot_be_split_changes_nothing(self) -> None:
        self.suite.write_text("fn a() {\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "do not balance"):
            bw.build(self.frozen, self.address, self.root, self.hidden)
        self.assertFalse(self.hidden.exists())


class Command(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name) / "work"
        self.frozen = frozen_set(self.root)
        self.set = Path(self.tmp.name) / "set.json"
        self.set.write_text(json.dumps(self.frozen), encoding="utf-8")
        self.env = {bw.ENV: str(Path(self.tmp.name) / "hidden")}
        entry = next(e for e in self.frozen["entries"] if len(e["suites"]) == 1)
        self.address = entry["address"]
        source = (self.root / entry["source"]).read_text(encoding="utf-8")
        function = source.split(f"// Typed from {self.address}\npub fn ", 1)[1].split("(", 1)[0]
        (self.root / entry["suites"][0]).write_text(suite_text(self.address, function), encoding="utf-8")

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
