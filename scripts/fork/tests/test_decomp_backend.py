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
import subprocess
import sys
import tempfile
import textwrap
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import decomp_backend as backend  # noqa: E402

ROOT = Path(__file__).resolve().parents[3]

# The exact layout clang 19 prints for a source written by this test:
#   struct S { char c; double d; }; struct T { int a; float b; };
# Kept as text so the parser is checked against the real format without clang.
CLANG_DUMP = """\
*** Dumping AST Record Layout
         0 | struct S
         0 |   char c
         8 |   double d
           | [sizeof=16, dsize=16, align=8,
           |  nvsize=16, nvalign=8]

*** Dumping AST Record Layout
         0 | struct T
         0 |   int a
         4 |   float b
           | [sizeof=8, dsize=8, align=4,
           |  nvsize=8, nvalign=4]
"""


def synthetic_dump(records):
    """A clang record-layout dump made from scratch, in clang's real format."""
    parts = []
    for name, fields in records:
        parts.append("*** Dumping AST Record Layout")
        parts.append("         0 | struct %s" % name)
        for offset, field in fields:
            parts.append("%7d | %s" % (offset, field))
        size = 4 + 4 * len(fields)
        parts.append("           | [sizeof=%d, dsize=%d, align=4," % (size, size))
        parts.append("           |  nvsize=%d, nvalign=4]" % size)
    return "\n".join(parts) + "\n"


class Result:
    def __init__(self, returncode=0, stderr=""):
        self.returncode, self.stderr = returncode, stderr


class Callable:
    """A stand-in for a ctypes function, which accepts .argtypes/.restype."""

    def __init__(self, fn):
        self.fn = fn

    def __call__(self, *args):
        return self.fn(*args)


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

    def test_rewrite_keeps_parameter_names_and_existing_enum_bases(self):
        source = textwrap.dedent("""\
            void* operator new(unsigned int size);
            enum Bar : unsigned char { A };
            typedef enum { B } Alias;
        """)
        out = backend.rewrite_source(source)
        self.assertIn("operator new(__SIZE_TYPE__ size)", out)
        self.assertIn("enum Bar : unsigned char { A };", out)
        self.assertIn("typedef enum { B } Alias;", out)

    def test_rewrite_leaves_unrelated_text_alone(self):
        source = "unsigned int count; enum Plain : int { X }; // unsigned int note\n"
        self.assertEqual(backend.rewrite_source(source), source)

    def test_wasm2c_module_mangling_is_derived(self):
        self.assertEqual(backend.wasm2c_module_name("GOT.mem"), "GOT0x2Emem")

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


class SelectionTests(unittest.TestCase):
    def test_an_unknown_unit_is_named(self):
        units = [{"source": "src/a.c"}]
        with self.assertRaisesRegex(ValueError, "src/b\\.c"):
            backend.select_units(units, ["src/b.c"])

    def test_output_path_rules(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            decomp = base / "decomp"
            decomp.mkdir()
            repo = base / "repo"
            repo.mkdir()
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            (repo / ".gitignore").write_text("/.decomp-backend/\n")
            with self.assertRaisesRegex(ValueError, "inside the decomp checkout"):
                backend.check_out_path(decomp, decomp / "copy")
            with self.assertRaisesRegex(ValueError, "ignored folder"):
                backend.check_out_path(decomp, repo / "out")
            with self.assertRaisesRegex(ValueError, "git work tree root"):
                backend.check_out_path(decomp, repo)
            backend.check_out_path(decomp, repo / ".decomp-backend")

    def test_a_unit_that_only_fails_to_link_is_skipped(self):
        objects = [Path("unit_%04d.o" % i) for i in range(3)]
        outcomes = [{"ok": True, "reason": "", "object": o.name} for o in objects]
        def link(tools, trial, wasm):
            if any(o.name == "unit_0001.o" for o in trial):
                return Result(1, "duplicate symbol: bad")
            return Result(0, "")
        with mock.patch.object(backend, "link_wasm", side_effect=link):
            kept, linked = backend.isolate_link_failures(
                {}, Path("out"), objects, outcomes, [0, 1, 2], Result(1, "duplicate symbol: bad"))
        self.assertEqual([o.name for o in kept], ["unit_0000.o", "unit_0002.o"])
        self.assertEqual(linked.returncode, 0)
        self.assertFalse(outcomes[1]["ok"])
        self.assertIn("duplicate symbol", outcomes[1]["reason"])
        self.assertTrue(outcomes[0]["ok"])


class LayoutTests(unittest.TestCase):
    def test_real_clang_dump_is_parsed(self):
        records = backend.parse_layout_dump(CLANG_DUMP)
        self.assertEqual(set(records), {"S", "T"})
        self.assertEqual(records["S"]["size"], (16, 8))
        self.assertEqual(records["S"]["fields"][(3, "double d", 1)], 8)
        self.assertEqual(backend.compare_layouts(CLANG_DUMP, CLANG_DUMP), [])

    def test_identical_layouts_are_accepted(self):
        dump = synthetic_dump([("Thing", [(0, "float x"), (4, "int y")])])
        self.assertEqual(backend.compare_layouts(dump, dump), [])

    def test_a_moved_offset_is_reported_by_record_and_field(self):
        left = CLANG_DUMP
        right = left.replace("         8 |   double d", "         4 |   double d")
        right = right.replace("sizeof=16", "sizeof=12").replace("dsize=16", "dsize=12")
        diffs = backend.compare_layouts(left, right)
        self.assertEqual(len(diffs), 2)
        self.assertTrue(all("S" in d for d in diffs))
        self.assertTrue(any("double d: i686=8 wasm=4" in d for d in diffs))

    def test_an_extra_field_is_reported(self):
        left = synthetic_dump([("Thing", [(0, "float x")])])
        right = synthetic_dump([("Thing", [(0, "float x"), (4, "int y")])])
        self.assertTrue(backend.compare_layouts(left, right))

    def test_field_order_and_vptr_spelling_do_not_matter(self):
        left = synthetic_dump([("Thing", [(0, "vftable pointer"), (4, "float x"), (8, "int y")])])
        right = synthetic_dump([("Thing", [(8, "int y"), (0, "vptr"), (4, "float x")])])
        self.assertEqual(backend.compare_layouts(left, right), [])

    def test_the_layout_command_fails_when_a_record_differs(self):
        report = {"compared": [{"source": "src/a.cpp", "records": 1}], "skipped": [],
                  "differences": ["Thing.x: i686=0 wasm=4"]}
        with mock.patch.object(backend, "layout", return_value=report):
            self.assertTrue(backend.main(["layout", "--decomp", "unused"]))

    def test_the_layout_command_fails_when_nothing_was_compared(self):
        report = {"compared": [], "skipped": [{"source": "src/a.cpp", "reason": "no clang"}],
                  "differences": []}
        with mock.patch.object(backend, "layout", return_value=report):
            self.assertEqual(backend.main(["layout", "--decomp", "unused"]), 1)


class HygieneTests(unittest.TestCase):
    def test_built_artefacts_are_rejected(self):
        paths = ["README.md", "scripts/fork/decomp_backend.py", "target/decomp-backend/mod.o",
                 "build/libfnv.so", "notes.wasm", "src/a.cpp", "lib.a", "out/mod.c",
                 "out/shim.c", "out/report.json", "out/layout.json"]
        self.assertEqual(backend.built_artefacts(paths),
                         ["target/decomp-backend/mod.o", "build/libfnv.so", "notes.wasm", "lib.a",
                          "out/mod.c", "out/shim.c", "out/report.json", "out/layout.json"])

    def test_the_output_directory_is_ignored(self):
        self.assertIn("/.decomp-backend/", (ROOT / ".gitignore").read_text().splitlines())

    def test_hygiene_sees_ignored_artefacts_and_accepts_a_fresh_checkout(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            (root / ".gitignore").write_text("/.decomp-backend/\nignored/\n")
            self.assertEqual(backend.hygiene(root), [])
            (root / "ignored").mkdir()
            (root / "ignored" / "mod.o").write_text("built")
            (root / "out").mkdir()
            (root / "out" / "mod.c").write_text("generated")
            (root / "target" / "debug").mkdir(parents=True)
            (root / "target" / "debug" / "unit.o").write_text("cargo build output")
            offenders = backend.hygiene(root)
            self.assertIn("ignored/mod.o", offenders)
            self.assertIn("out/mod.c", offenders)
            self.assertNotIn("target/debug/unit.o", offenders)


class VerifyTests(unittest.TestCase):
    def test_globals_are_passed_to_the_library(self):
        seen = {}

        def init(guest, size, entries, values, count):
            seen["values"] = [values[i] for i in range(count)] if values else []
            seen["count"] = count
            return 0

        lib = mock.Mock()
        lib.decomp_init = Callable(init)
        lib.decomp_memory_base = Callable(lambda: 0)
        lib.decomp_call_0 = Callable(lambda: 0x1234)
        with tempfile.TemporaryDirectory() as temp:
            manifest = Path(temp) / "manifest.json"
            vectors = Path(temp) / "vectors.json"
            manifest.write_text(json.dumps({"table_entries": 16, "global_values": ["0x400000", 7],
                                            "exports": [{"name": "f", "arity": 0,
                                                         "wrapper": "decomp_call_0"}]}))
            vectors.write_text(json.dumps([{"export": "f", "args": [], "expected": "0x1234"}]))
            with mock.patch.object(backend.ctypes, "CDLL", return_value=lib):
                self.assertEqual(backend.verify(manifest.parent / "lib.so", manifest, vectors), [])
        self.assertEqual(seen["values"], [0x400000, 7])
        self.assertEqual(seen["count"], 2)

    def test_a_failed_init_is_reported(self):
        lib = mock.Mock()
        lib.decomp_init = Callable(lambda *args: -2)
        with tempfile.TemporaryDirectory() as temp:
            manifest = Path(temp) / "manifest.json"
            vectors = Path(temp) / "vectors.json"
            manifest.write_text(json.dumps({"table_entries": 16, "exports": []}))
            vectors.write_text("[]")
            with mock.patch.object(backend.ctypes, "CDLL", return_value=lib):
                with self.assertRaisesRegex(RuntimeError, "decomp_init failed: -2"):
                    backend.verify(manifest.parent / "lib.so", manifest, vectors)


@unittest.skipUnless(os.environ.get("NV_BACKEND_TOOLCHAIN_TEST"), "needs clang, lld and wabt")
class PipelineTests(unittest.TestCase):
    """Builds a whole library from a synthetic checkout and calls it."""

    PURE = textwrap.dedent("""\
        extern unsigned extern_add(unsigned a, unsigned b);
        extern unsigned ext_global;
        unsigned add2(unsigned a, unsigned b) { return a + b; }
        unsigned scale(unsigned a, unsigned b) { return a * b + 1; }
        float half(float x) { return x * 0.5f; }
        unsigned call_import(unsigned a, unsigned b) { return extern_add(a, b); }
        unsigned read_global(void) { return ext_global; }
    """)

    def test_library_and_report_from_scratch(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp = Path(temp) / "decomp"
            (decomp / "include").mkdir(parents=True)
            (decomp / "src").mkdir()
            (decomp / "build").mkdir()
            functions = [{"address": address, "symbol": "_" + name} for name, address in (
                ("add2", "0x1000"), ("scale", "0x2000"), ("half", "0x2400"),
                ("call_import", "0x2800"), ("read_global", "0x2c00"))]
            (decomp / "build" / "units.json").write_text(json.dumps({"units": [
                {"source": "src/pure.c", "functions": functions},
                {"source": "src/bad.c", "functions": [
                    {"address": "0x3000", "symbol": "_broken"}]},
            ], "symbols": {"ext_global": "0x00400000"}}))
            (decomp / "src" / "pure.c").write_text(self.PURE)
            (decomp / "src" / "bad.c").write_text("unsigned broken( { }\n")
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

            # The import is answered by a host hook; the expected values are the
            # translations: plain functions written from scratch with the same
            # inputs as the library calls.
            hook_type = ctypes.CFUNCTYPE(ctypes.c_uint32, ctypes.c_char_p,
                                         ctypes.POINTER(ctypes.c_uint32), ctypes.c_uint32)
            @hook_type
            def hook(name, args, count):
                return args[0] + args[1] if name == b"extern_add" else 0
            lib.decomp_set_import_hook.argtypes = [hook_type]
            lib.decomp_set_import_hook(hook)
            vectors = out / "vectors.json"
            vectors.write_text(json.dumps([
                {"export": "add2", "args": [7, 9], "expected": hex((7 + 9) & 0xffffffff)},
                {"export": "scale", "args": [7, 9], "expected": hex((7 * 9 + 1) & 0xffffffff)},
                {"export": "half", "args": [hex(0x40c00000)], "expected": hex(0x40400000)},
                {"export": "call_import", "args": [7, 9], "expected": hex(16)},
                {"export": "read_global", "input": "0x400000", "data": "34120000",
                 "args": [], "expected": hex(0x1234)},
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
