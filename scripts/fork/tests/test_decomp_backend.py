#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Tests for the local fnv_decomp backend build tool (nv-rs#926).

Every input here is written by the test into a fresh temporary directory. No
source from the decomp checkout, no game file and no built object is read from
or written into the repository.
"""
import ctypes
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import decomp_backend as backend  # noqa: E402

ROOT = Path(__file__).resolve().parents[3]


def synthetic_dump(records):
    """A clang record-layout dump made from scratch."""
    parts = []
    for name, fields in records:
        parts.append("*** Dumping AST Record Layout")
        parts.append("         0 | struct %s" % name)
        for offset, field in fields:
            parts.append("%7d | %s" % (offset, field))
        real = [f for _, f in fields if "__vptr" not in f]
        parts.append(" [sizeof=%d, align=4]" % (4 + 4 * len(real)))
    return "\n".join(parts) + "\n"


class RewriteTests(unittest.TestCase):
    def test_mechanical_rewrites_change_both_spellings(self):
        source = textwrap.dedent("""\
            static void* operator new(unsigned int size);
            static void operator delete(void* p, unsigned int size);
            typedef unsigned int size_t;
            enum Named { A = 1, B = 2 };
            enum { ANONYMOUS = 3 };
            enum class Scoped { C = 4 };
        """)
        out = backend.rewrite_source(source)
        self.assertIn("operator new(__SIZE_TYPE__ size)", out)
        self.assertIn("operator delete(void* p, __SIZE_TYPE__ size)", out)
        self.assertIn("typedef __SIZE_TYPE__ size_t;", out)
        self.assertIn("enum Named : int {", out)
        self.assertIn("enum { ANONYMOUS = 3 };", out)
        self.assertIn("enum class Scoped {", out)
        self.assertNotIn("unsigned int size", out)

    def test_rewrite_leaves_unrelated_text_alone(self):
        source = "unsigned int count; enum Plain : int { X }; // unsigned int note\n"
        self.assertEqual(backend.rewrite_source(source), source)

    def test_build_copy_does_not_touch_the_checkout(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp = Path(temp) / "decomp"
            (decomp / "include").mkdir(parents=True)
            (decomp / "src").mkdir()
            header = decomp / "include" / "tiny.h"
            original = "enum E { A }; void* operator new(unsigned int);\n"
            header.write_text(original)
            (decomp / "build").mkdir()
            (decomp / "build" / "units.json").write_text('{"units": []}')
            out = Path(temp) / "out"
            changed = backend.prepare_build_copy(decomp, out)
            self.assertEqual(changed, 1)
            self.assertEqual(header.read_text(), original)
            self.assertIn("enum E : int", (out / "copy" / "include" / "tiny.h").read_text())


class ReportTests(unittest.TestCase):
    def test_report_names_every_skip_reason(self):
        units = [
            {"source": "src/a.cpp", "functions": [{"address": "0x1000", "symbol": "_a"}]},
            {"source": "src/b.cpp", "functions": [{"address": "0x2000", "symbol": "_b"}]},
        ]
        outcomes = [
            {"source": "src/a.cpp", "ok": True, "object": "a.o"},
            {"source": "src/b.cpp", "ok": False, "reason": "__asm is not supported on wasm"},
        ]
        report = backend.make_report(units, outcomes, library="libfnv.so")
        self.assertEqual(report["library"], "libfnv.so")
        self.assertEqual(len(report["compiled"]), 1)
        self.assertEqual(report["compiled"][0]["address"], "0x1000")
        self.assertEqual(len(report["skipped"]), 1)
        self.assertEqual(report["skipped"][0]["reason"], "__asm is not supported on wasm")
        self.assertEqual(report["counts"], {"compiled": 1, "skipped": 1})


class LayoutTests(unittest.TestCase):
    def test_identical_layouts_are_accepted(self):
        dump = synthetic_dump([("Thing", [(0, "float x"), (4, "int y")])])
        self.assertEqual(backend.compare_layouts(dump, dump), [])

    def test_a_moved_offset_is_reported_by_record_and_field(self):
        left = synthetic_dump([("Thing", [(0, "float x"), (4, "int y")])])
        right = synthetic_dump([("Thing", [(0, "float x"), (8, "int y")])])
        diffs = backend.compare_layouts(left, right)
        self.assertEqual(len(diffs), 1)
        self.assertIn("Thing", diffs[0])
        self.assertIn("int y", diffs[0])

    def test_an_extra_field_is_reported(self):
        left = synthetic_dump([("Thing", [(0, "float x")])])
        right = synthetic_dump([("Thing", [(0, "float x"), (4, "int y")])])
        self.assertTrue(backend.compare_layouts(left, right))

    def test_field_order_and_vptr_spelling_do_not_matter(self):
        left = synthetic_dump([("Thing", [(4, "int y"), (0, "float x")])])
        right = synthetic_dump([("Thing", [(0, "__vptr"), (0, "float x"), (4, "int y")])])
        self.assertEqual(backend.compare_layouts(left, right), [])

    def test_the_layout_command_fails_when_a_record_differs(self):
        report = {"compared": [], "skipped": [], "differences": ["Thing.x: i686=0 wasm=4"]}
        with mock.patch.object(backend, "layout", return_value=report):
            self.assertTrue(backend.main(["layout", "--decomp", "unused"]))


class HygieneTests(unittest.TestCase):
    def test_built_artefacts_are_rejected(self):
        paths = ["README.md", "scripts/fork/decomp_backend.py", "target/decomp-backend/mod.o",
                 "build/libfnv.so", "notes.wasm", "src/a.cpp", "lib.a", "out/mod.c"]
        self.assertEqual(backend.built_artefacts(paths),
                         ["target/decomp-backend/mod.o", "build/libfnv.so", "notes.wasm", "lib.a", "out/mod.c"])

    def test_the_output_directory_is_ignored(self):
        self.assertIn("/.decomp-backend/", (ROOT / ".gitignore").read_text().splitlines())


@unittest.skipUnless(os.environ.get("NV_BACKEND_TOOLCHAIN_TEST"), "needs clang, lld and wabt")
class PipelineTests(unittest.TestCase):
    """Builds a whole library from a synthetic checkout and calls it."""

    PURE = textwrap.dedent("""\
        unsigned add2(unsigned a, unsigned b) { return a + b; }
        unsigned scale(unsigned a, unsigned b) { return a * b + 1; }
        unsigned broken( { }
    """)

    def test_library_and_report_from_scratch(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp = Path(temp) / "decomp"
            (decomp / "include").mkdir(parents=True)
            (decomp / "src").mkdir()
            (decomp / "build").mkdir()
            (decomp / "build" / "units.json").write_text(json.dumps({"units": [
                {"source": "src/pure.c", "functions": [
                    {"address": "0x00001000", "symbol": "_add2"},
                    {"address": "0x00002000", "symbol": "_scale"}]},
                {"source": "src/bad.c", "functions": [
                    {"address": "0x00003000", "symbol": "_broken"}]},
            ]}))
            good = decomp / "src" / "pure.c"
            bad = decomp / "src" / "bad.c"
            good.write_text(self.PURE.split("unsigned broken")[0])
            bad.write_text("unsigned broken( { }\n")
            out = Path(temp) / "out"
            report = backend.build(decomp, out, toolchain=backend.find_toolchain())
            self.assertEqual(report["counts"], {"compiled": 1, "skipped": 1})
            self.assertEqual(report["skipped"][0]["source"], "src/bad.c")
            self.assertTrue(report["skipped"][0]["reason"])
            library = Path(report["library"])
            self.assertTrue(library.is_file())
            lib = ctypes.CDLL(str(library))
            lib.decomp_init.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32]
            for entry in report["exports"]:
                wrapper = getattr(lib, entry["wrapper"])
                wrapper.argtypes = [ctypes.c_uint32] * entry["arity"]
                wrapper.restype = ctypes.c_uint32
            by_name = {e["name"]: e for e in report["exports"]}
            # The expected values are the translations: plain functions written in
            # Python from scratch, with the same arguments as the library calls.
            vectors = out / "vectors.json"
            vectors.write_text(json.dumps([
                {"export": "add2", "args": [7, 9], "expected": hex((7 + 9) & 0xffffffff)},
                {"export": "scale", "args": [7, 9], "expected": hex((7 * 9 + 1) & 0xffffffff)},
            ]))
            self.assertEqual(backend.verify(library, out / "manifest.json", vectors), [])
            add, scl = by_name["add2"], by_name["scale"]
            self.assertEqual(getattr(lib, add["wrapper"])(7, 9), 16)
            self.assertEqual(getattr(lib, scl["wrapper"])(7, 9), 64)
            # A wrong expected value must fail, in a fresh process: the runtime's
            # alternate signal stack cannot be installed twice in one process.
            bad = out / "bad.json"
            bad.write_text(json.dumps([
                {"export": "add2", "args": [7, 9], "expected": "0x10"},
                {"export": "scale", "args": [7, 9], "expected": "0x41"},
            ]))
            tool = Path(backend.__file__)
            result = subprocess.run([sys.executable, str(tool), "verify", "--library", str(library),
                                     "--manifest", str(out / "manifest.json"), "--vectors", str(bad)],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn("got", result.stdout)


if __name__ == "__main__":
    unittest.main()
