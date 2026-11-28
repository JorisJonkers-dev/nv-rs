#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Tests for the local fnv_decomp backend build tool (nv-rs#926).

Every input here is written by the test into a fresh temporary directory. No
source from the decomp checkout, no game file and no built object is read from
or written into the repository.
"""
import ctypes
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
import zipfile
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import decomp_backend as backend  # noqa: E402

ROOT = Path(__file__).resolve().parents[3]

# clang 19's own wording: the size line continues on the next line. Kept as
# text so the parser is checked against the real format without clang.
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


class Callable:
    """A stand-in for a ctypes function, which accepts .argtypes/.restype."""

    def __init__(self, fn):
        self.fn = fn

    def __call__(self, *args):
        return self.fn(*args)


def synthetic_dump(records):
    """A clang record-layout dump made from scratch, in clang's real format."""
    parts = []
    for name, fields in records:
        parts.append("*** Dumping AST Record Layout")
        parts.append("         0 | struct %s" % name)
        for offset, field in fields:
            parts.append("%7d | %s" % (offset, field))
        size = 4 + 4 * len([f for _, f in fields if "__vptr" not in f])
        parts.append("           | [sizeof=%d, dsize=%d, align=4," % (size, size))
        parts.append("           |  nvsize=%d, nvalign=4]" % size)
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

    def test_a_bare_unsigned_size_parameter_of_operator_new_is_widened(self):
        text = "static void* operator new[](unsigned size)\n{ return 0; }\n"
        self.assertIn("operator new[](__SIZE_TYPE__ size)", backend.rewrite_source(text))
        kept = "void* operator new(unsigned long long size);"
        self.assertEqual(kept, backend.rewrite_source(kept))

    def test_critical_section_arguments_are_cast_and_declarations_left(self):
        text = ("extern void __stdcall EnterCriticalSection(void* cs);\n"
                "void f() { EnterCriticalSection(*(void**)((char*)this + 0x14)); }\n")
        out = backend.rewrite_source(text)
        self.assertIn("EnterCriticalSection(void* cs);", out)
        self.assertIn("EnterCriticalSection((struct _RTL_CRITICAL_SECTION*)(*(void**)((char*)this + 0x14)));", out)

    def test_a_placement_operator_news_first_size_is_widened(self):
        out = backend.rewrite_source("static void* operator new(unsigned int size, unsigned int r) {}")
        self.assertIn("operator new(__SIZE_TYPE__ size, __SIZE_TYPE__ r)", out)

    def test_a_linkage_conflict_after_other_statements_blanks_only_that_declaration(self):
        with tempfile.TemporaryDirectory() as temp:
            copy = Path(temp) / "copy"
            copy.mkdir()
            unit = copy / "a.cpp"
            unit.write_text('typedef int Vec;extern "C" unsigned char DAT_1[118];\n')
            stderr = ("%s:1:30: error: declaration of 'DAT_1' with C language linkage conflicts with "
                      "declaration in global scope\n" % unit)
            self.assertEqual(1, backend.drop_conflicting_declarations(stderr, copy))
            self.assertEqual("typedef int Vec;\n", unit.read_text())

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
    def test_clangs_two_line_size_block_is_parsed(self):
        records = backend.parse_layout_dump(CLANG_DUMP)
        self.assertEqual(set(records), {"S", "T"})
        self.assertEqual(records["S"]["size"], (16, 8))
        self.assertEqual(records["T"]["size"], (8, 4))
        self.assertEqual(backend.compare_layouts(CLANG_DUMP, CLANG_DUMP), [])

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
        report = {"compared": [{"source": "src/a.cpp", "records": 1}], "skipped": [],
                  "differences": ["Thing.x: i686=0 wasm=4"]}
        with mock.patch.object(backend, "layout", return_value=report):
            self.assertTrue(backend.main(["layout", "--decomp", "unused"]))

    def test_the_layout_command_fails_when_nothing_was_compared(self):
        report = {"compared": [], "skipped": [{"source": "src/a.cpp", "reason": "no clang"}],
                  "differences": []}
        with mock.patch.object(backend, "layout", return_value=report):
            self.assertEqual(backend.main(["layout", "--decomp", "unused"]), 1)


class OutputPathTests(unittest.TestCase):
    def test_the_output_may_not_be_inside_the_checkout_or_an_unignored_folder(self):
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

    def test_build_refuses_an_output_inside_the_checkout(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp = Path(temp) / "decomp"
            decomp.mkdir()
            with self.assertRaisesRegex(ValueError, "inside the decomp checkout"):
                backend.build(decomp, decomp / "out", toolchain={})


class HygieneTests(unittest.TestCase):
    def test_built_artefacts_are_rejected(self):
        paths = ["README.md", "scripts/fork/decomp_backend.py", "target/decomp-backend/mod.o",
                 "build/libfnv.so", "notes.wasm", "src/a.cpp", "lib.a", "out/mod.c"]
        self.assertEqual(backend.built_artefacts(paths),
                         ["target/decomp-backend/mod.o", "build/libfnv.so", "notes.wasm", "lib.a", "out/mod.c"])

    def test_the_output_directory_is_ignored(self):
        self.assertIn("/.decomp-backend/", (ROOT / ".gitignore").read_text().splitlines())


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
        lib.decomp_last_trap_code = Callable(lambda: 0)
        lib.decomp_clear_trap = Callable(lambda: None)
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp)
            (out / "lib.so").write_text("")
            (out / "manifest.json").write_text(json.dumps({
                "table_entries": 16, "global_values": ["0x400000", 7],
                "exports": [{"name": "f", "arity": 0, "wrapper": "decomp_call_0"}]}))
            (out / "vectors.json").write_text(json.dumps(
                [{"export": "f", "args": [], "expected": "0x1234"}]))
            with mock.patch.object(backend.ctypes, "CDLL", return_value=lib):
                self.assertEqual(backend.verify(out / "lib.so", out / "manifest.json",
                                                out / "vectors.json"), [])
        self.assertEqual(seen["values"], [0x400000, 7])
        self.assertEqual(seen["count"], 2)

    def test_a_failed_init_is_reported(self):
        lib = mock.Mock()
        lib.decomp_init = Callable(lambda *args: -2)
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp)
            (out / "lib.so").write_text("")
            (out / "manifest.json").write_text(json.dumps({"table_entries": 16, "exports": []}))
            (out / "vectors.json").write_text("[]")
            with mock.patch.object(backend.ctypes, "CDLL", return_value=lib):
                with self.assertRaisesRegex(RuntimeError, "decomp_init failed: -2"):
                    backend.verify(out / "lib.so", out / "manifest.json", out / "vectors.json")


@unittest.skipUnless(os.environ.get("NV_BACKEND_TOOLCHAIN_TEST"), "needs clang, lld and wabt")
class PipelineTests(unittest.TestCase):
    """Builds a whole library from a synthetic checkout and calls it."""

    PURE = textwrap.dedent("""\
        unsigned add2(unsigned a, unsigned b) { volatile unsigned z = a ^ b; return a + b + z - z; }
        unsigned scale(unsigned a, unsigned b) { volatile unsigned z = a | b; return a * b + 1 + z - z; }
        unsigned (*volatile slot)(unsigned, unsigned);
        unsigned store(void) { slot = &add2; return (unsigned)slot; }
        unsigned callslot(unsigned a, unsigned b) { return slot(a, b); }
        unsigned broken( { }
    """)

    def test_the_rebuilt_module_is_converted_only_once(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp = Path(temp) / "decomp"
            (decomp / "src").mkdir(parents=True)
            (decomp / "build").mkdir()
            (decomp / "build" / "units.json").write_text(json.dumps({"units": [
                {"source": "src/pure.c", "functions": [
                    {"address": "0x1000", "symbol": "_add2"}]}]}))
            (decomp / "src" / "pure.c").write_text(
                "unsigned add2(unsigned a, unsigned b) { return a + b; }\n")
            calls, original = [], backend.run

            def record(command, **kwargs):
                calls.append([str(part) for part in command])
                return original(command, **kwargs)

            with mock.patch.object(backend, "run", side_effect=record):
                backend.build(decomp, Path(temp) / "out", toolchain=backend.find_toolchain())
            self.assertEqual(sum(Path(call[0]).name == "wasm2c" for call in calls), 1)

    def test_library_and_report_from_scratch(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp = Path(temp) / "decomp"
            (decomp / "include").mkdir(parents=True)
            (decomp / "src").mkdir()
            (decomp / "build").mkdir()
            (decomp / "build" / "units.json").write_text(json.dumps({"units": [
                {"source": "src/pure.c", "functions": [
                    {"address": "0x00001000", "symbol": "_add2"},
                    {"address": "0x00002000", "symbol": "_scale"},
                    {"address": "0x00003000", "symbol": "_slot"},
                    {"address": "0x00004000", "symbol": "_store"},
                    {"address": "0x00005000", "symbol": "_callslot"}]},
                {"source": "src/bad.c", "functions": [
                    {"address": "0x00006000", "symbol": "_broken"}]},
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
            lib.decomp_init.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32,
                                        ctypes.c_void_p, ctypes.c_uint32]
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
            # Taking the address of a function inside the library stores its
            # original address, and a call through that stored address reaches
            # the function. An empty address traps and the trap is reported.
            store = by_name["store"]
            callslot = by_name["callslot"]
            getattr(lib, store["wrapper"]).restype = ctypes.c_uint32
            getattr(lib, callslot["wrapper"]).restype = ctypes.c_uint32
            self.assertEqual(getattr(lib, store["wrapper"])(), 0x1000)
            self.assertEqual(getattr(lib, callslot["wrapper"])(7, 9), 16)
            lib.decomp_last_trap_code.restype = ctypes.c_uint32
            lib.decomp_last_trap_reason.restype = ctypes.c_char_p
            lib.decomp_clear_trap.argtypes = []
            lib.decomp_memory_base.restype = ctypes.c_void_p
            base = lib.decomp_memory_base()
            self.assertEqual(lib.decomp_last_trap_code(), 0)
            ctypes.memmove(base + 0x30000000, ctypes.byref(ctypes.c_uint32(0)), 4)
            lib.decomp_clear_trap()
            self.assertEqual(getattr(lib, callslot["wrapper"])(7, 9), 0)
            self.assertNotEqual(lib.decomp_last_trap_code(), 0)
            self.assertIn(b"indirect", lib.decomp_last_trap_reason())
            # A wrong expected value must fail, in a fresh process: the runtime's
            # alternate signal stack cannot be installed twice in one process.
            bad = out / "bad.json"
            bad.write_text(json.dumps([
                {"export": "add2", "args": [7, 9], "expected": "0x10"},
                {"export": "scale", "args": [7, 9], "expected": "0x41"},
                {"export": "callslot", "input": "0x30000000", "data": "00000000",
                 "args": [7, 9], "expected": "0x10"},
            ]))
            tool = Path(backend.__file__)
            result = subprocess.run([sys.executable, str(tool), "verify", "--library", str(library),
                                     "--manifest", str(out / "manifest.json"), "--vectors", str(bad)],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn("got", result.stdout)
            self.assertIn("trap", result.stdout)


class HostTests(unittest.TestCase):
    def test_every_supported_host_has_its_native_library_name(self):
        expected = {
            ("Linux", "x86_64"): "libfnv_decomp.so",
            ("Linux", "aarch64"): "libfnv_decomp.so",
            ("Darwin", "x86_64"): "libfnv_decomp.dylib",
            ("Darwin", "arm64"): "libfnv_decomp.dylib",
            ("Windows", "AMD64"): "fnv_decomp.dll",
            ("Windows", "arm64"): "fnv_decomp.dll",
        }
        for (system, machine), name in expected.items():
            host = backend.detect_host(system, machine)
            self.assertEqual(host["system"], system)
            self.assertEqual(host["library_name"], name)
            self.assertIn(host["machine"], ("x86-64", "aarch64"))

    def test_windows_reserves_guest_memory_with_virtual_alloc(self):
        source = backend.guest_memory_source(backend.detect_host("Windows", "AMD64"))
        self.assertIn("VirtualAlloc", source)
        self.assertNotIn("sys/mman.h", source)

    def test_posix_reserves_guest_memory_with_mmap(self):
        for system in ("Linux", "Darwin"):
            source = backend.guest_memory_source(backend.detect_host(system, "aarch64"))
            self.assertIn("mmap(", source)
            self.assertIn("mprotect", source)

    def test_native_link_flags_follow_the_host(self):
        linux = backend.detect_host("Linux", "aarch64")
        darwin = backend.detect_host("Darwin", "x86_64")
        windows = backend.detect_host("Windows", "AMD64")
        self.assertIn("-shared", linux["native_flags"])
        self.assertIn("-fPIC", linux["native_flags"])
        self.assertIn("-dynamiclib", darwin["native_flags"])
        self.assertIn("-shared", windows["native_flags"])
        self.assertIn("-lm", linux["native_libs"])
        self.assertEqual(windows["native_libs"], [])

    def test_the_generated_shim_carries_the_host_reservation(self):
        with tempfile.TemporaryDirectory() as temp:
            shim = Path(temp) / "shim.c"
            host = backend.detect_host("Windows", "AMD64")
            backend.generate_shim(shim, [], [], [], "", host=host)
            text = shim.read_text()
            self.assertIn("VirtualAlloc", text)
            self.assertNotIn("sys/mman.h", text)

    def test_an_unsupported_host_is_named_in_the_error(self):
        with self.assertRaises(ValueError) as raised:
            backend.detect_host("Plan9", "mips")
        self.assertIn("Plan9", str(raised.exception))


class ToolchainTests(unittest.TestCase):
    def archives(self, temp, suffix="", tag=""):
        clang = Path(temp) / ("clang%s.zip" % tag)
        wabt = Path(temp) / ("wabt%s.zip" % tag)
        with zipfile.ZipFile(clang, "w") as archive:
            archive.writestr("bin/clang" + suffix, "#!/bin/sh\nexit 0\n")
        with zipfile.ZipFile(wabt, "w") as archive:
            for name in ("bin/wasm-ld", "bin/wasm2c", "bin/wasm2wat", "bin/wat2wasm",
                         "share/wabt/wasm2c/wasm-rt-impl.c",
                         "share/wabt/wasm2c/wasm-rt-mem-impl.c", "include/wasm-rt.h"):
                archive.writestr(name + suffix if name.startswith("bin/") else name, "")
        return clang, wabt

    def pins(self, clang, wabt):
        digest = lambda path: hashlib.sha256(Path(path).read_bytes()).hexdigest()
        return {"NV_TOOLCHAIN_SHA256": json.dumps({"clang": digest(clang), "wabt": digest(wabt)})}

    def copier(self, clang, wabt, calls=None):
        def download(url, path):
            if calls is not None:
                calls.append(url)
            shutil.copyfile(clang if "clang" in url else wabt, path)
        return download

    def test_a_missing_toolchain_is_fetched_once_and_then_cached(self):
        with tempfile.TemporaryDirectory() as temp:
            clang, wabt = self.archives(temp)
            cache = Path(temp) / "cache"
            calls = []
            host = backend.detect_host("Linux", "x86_64")
            with mock.patch.dict(os.environ, self.pins(clang, wabt), clear=True):
                first = backend.ensure_toolchain(cache, host, download=self.copier(clang, wabt, calls))
                second = backend.ensure_toolchain(cache, host, download=self.copier(clang, wabt, calls))
            self.assertTrue(first["fetched"])
            self.assertFalse(second["fetched"])
            self.assertEqual(len(calls), 2)
            for key in backend.REQUIRED_TOOLS:
                self.assertTrue(Path(second[key]).is_file())

    def test_a_second_find_run_uses_the_cache(self):
        with tempfile.TemporaryDirectory() as temp:
            clang, wabt = self.archives(temp)
            cache = Path(temp) / "cache"
            calls = []
            with mock.patch.dict(os.environ, self.pins(clang, wabt), clear=True), \
                    mock.patch.object(backend.shutil, "which", return_value=None):
                first = backend.find_toolchain(cache_dir=cache, download=self.copier(clang, wabt, calls))
                second = backend.find_toolchain(cache_dir=cache, download=self.copier(clang, wabt, calls))
            self.assertEqual(len(calls), 2)
            self.assertEqual(first["clang"], second["clang"])
            self.assertTrue(str(second["clang"]).startswith(str(cache)))
            self.assertTrue(backend.toolchain_ready(second))
            self.assertTrue(backend.wasm_runtime_ready(second))

    def test_a_cached_host_uses_one_consistent_wabt_set(self):
        with tempfile.TemporaryDirectory() as temp:
            clang, wabt = self.archives(temp)
            cache = Path(temp) / "cache"
            with mock.patch.dict(os.environ, self.pins(clang, wabt), clear=True), \
                    mock.patch.object(backend.shutil, "which", return_value=None):
                backend.ensure_toolchain(cache, backend.detect_host("Linux", "x86_64"),
                                         download=self.copier(clang, wabt))
                with mock.patch.object(backend.shutil, "which",
                                       side_effect=lambda name: "/usr/bin/" + name):
                    tools = backend.find_toolchain(cache_dir=cache, download=self.copier(clang, wabt))
            for key in backend.REQUIRED_TOOLS:
                self.assertTrue(str(tools[key]).startswith(str(cache)))
            self.assertEqual(backend.wabt_tool(tools, "wasm2wat"), tools["wasm2wat"])
            self.assertEqual(backend.wabt_tool(tools, "wat2wasm"), tools["wat2wasm"])

    def test_the_cache_keeps_one_toolchain_per_host(self):
        with tempfile.TemporaryDirectory() as temp:
            linux = self.archives(temp, tag="-linux")
            windows = self.archives(temp, suffix=".exe", tag="-windows")
            cache = Path(temp) / "cache"
            calls = []

            def download(url, path):
                calls.append(url)
                pair = windows if "windows" in url else linux
                shutil.copyfile(pair[0] if "clang" in url else pair[1], path)

            with mock.patch.dict(os.environ, self.pins(*linux), clear=True):
                backend.ensure_toolchain(cache, backend.detect_host("Linux", "x86_64"), download=download)
                with mock.patch.dict(os.environ, self.pins(*windows), clear=True):
                    backend.ensure_toolchain(cache, backend.detect_host("Windows", "AMD64"), download=download)
            self.assertEqual(len(calls), 4)

    def test_an_unpinned_or_mismatched_archive_is_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            clang, wabt = self.archives(temp)
            cache = Path(temp) / "cache"
            with mock.patch.dict(os.environ, {}, clear=True):
                with self.assertRaisesRegex(backend.ToolchainUnavailable, "SHA-256 pin"):
                    backend.ensure_toolchain(cache, backend.detect_host("Linux", "x86_64"),
                                             download=self.copier(clang, wabt))
            with mock.patch.dict(os.environ, self.pins(clang, wabt), clear=True):
                with self.assertRaisesRegex(backend.ToolchainUnavailable, "SHA-256 mismatch"):
                    backend.ensure_toolchain(Path(temp) / "other", backend.detect_host("Linux", "x86_64"),
                                             download=self.copier(wabt, wabt))

    def test_an_archive_may_not_write_outside_the_cache(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / "evil.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("../escaped.txt", "no")
            with self.assertRaisesRegex(ValueError, "unsafe toolchain archive member"):
                backend.safe_unpack(archive, Path(temp) / "target")
            self.assertFalse((Path(temp) / "escaped.txt").exists())

    def test_a_blocked_fetch_is_reported(self):
        with tempfile.TemporaryDirectory() as temp:
            clang, wabt = self.archives(temp)

            def blocked(url, path):
                raise OSError("network is disabled")

            with mock.patch.dict(os.environ, self.pins(clang, wabt), clear=True):
                with self.assertRaises(backend.ToolchainUnavailable) as raised:
                    backend.ensure_toolchain(Path(temp) / "cache",
                                             backend.detect_host("Linux", "x86_64"), download=blocked)
            self.assertIn("network is disabled", str(raised.exception))

    def test_env_overrides_are_used_without_fetching(self):
        with tempfile.TemporaryDirectory() as temp:
            calls = []

            def download(url, path):
                calls.append(url)

            overrides = {"NV_CLANG": "/tools/clang", "NV_WASM_LD": "/tools/wasm-ld",
                         "NV_WASM2C": "/tools/wasm2c", "NV_WABT_WASM_WAT": "/tools/wasm2wat",
                         "NV_WABT_WAT_WASM": "/tools/wat2wasm"}
            with mock.patch.dict(os.environ, overrides, clear=True), \
                    mock.patch.object(backend.shutil, "which", return_value=None):
                tools = backend.find_toolchain(cache_dir=Path(temp) / "cache", download=download)
            self.assertEqual(calls, [])
            self.assertEqual(tools["clang"], "/tools/clang")
            self.assertTrue(backend.toolchain_ready(tools))

    def test_a_failed_fetch_leaves_the_toolchain_unready(self):
        with tempfile.TemporaryDirectory() as temp:
            clang, wabt = self.archives(temp)

            def blocked(url, path):
                raise OSError("blocked")

            with mock.patch.dict(os.environ, self.pins(clang, wabt), clear=True):
                tools = backend.find_toolchain(cache_dir=Path(temp) / "cache", download=blocked)
            self.assertFalse(backend.toolchain_ready(tools))
            self.assertIn("toolchain_error", tools)

    def test_a_broken_cache_is_fetched_again(self):
        with tempfile.TemporaryDirectory() as temp:
            clang, wabt = self.archives(temp)
            cache = Path(temp) / "cache"
            host = backend.detect_host("Linux", "x86_64")
            with mock.patch.dict(os.environ, self.pins(clang, wabt), clear=True), \
                    mock.patch.object(backend.shutil, "which", return_value=None):
                backend.ensure_toolchain(cache, host, download=self.copier(clang, wabt))
                paths = backend.toolchain_paths(cache, host)[1]
                Path(paths["wasm2wat"]).unlink()
                self.assertIsNone(backend.cached_toolchain(cache, host))
                calls = []
                again = backend.ensure_toolchain(cache, host, download=self.copier(clang, wabt, calls))
                self.assertTrue(again["fetched"])
                self.assertEqual(len(calls), 2)


@unittest.skipUnless(shutil.which("cc"), "needs a local C compiler")
class FallbackTests(unittest.TestCase):
    """The native fallback path, driven by an injected compiler instead of clang."""

    def checkout(self, temp):
        decomp = Path(temp) / "decomp"
        (decomp / "include").mkdir(parents=True)
        (decomp / "src").mkdir()
        (decomp / "build").mkdir()
        (decomp / "build" / "units.json").write_text(json.dumps({"units": [
            {"source": "src/pure.c", "functions": [
                {"address": "0x00001000", "symbol": "_add2", "params": ["u32", "u32"], "result": "u32"},
                {"address": "0x00002000", "symbol": "_scale", "params": ["u32", "u32"], "result": "u32"}]},
            {"source": "src/bad.c", "functions": [
                {"address": "0x00003000", "symbol": "_broken", "params": ["u32"], "result": "u32"}]},
        ]}))
        (decomp / "src" / "pure.c").write_text(
            "unsigned add2(unsigned a, unsigned b) { return a + b; }\n"
            "unsigned scale(unsigned a, unsigned b) { return a * b + 1; }\n")
        (decomp / "src" / "bad.c").write_text("unsigned broken( { }\n")
        return decomp

    def compiler(self, calls):
        def compile_(command):
            calls.append([str(part) for part in command])
            return backend.run(command)
        return compile_

    def test_the_fallback_produces_a_module_with_the_same_abi(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp, out = self.checkout(temp), Path(temp) / "out"
            tools = backend.find_toolchain(cache_dir=Path(temp) / "cache",
                                           download=lambda url, path: (_ for _ in ()).throw(OSError("blocked")))
            calls = []
            report = backend.build(decomp, out, toolchain=tools, compiler=self.compiler(calls),
                                   force_fallback=True)
            self.assertEqual(report["counts"], {"compiled": 1, "skipped": 1})
            library = Path(report["library"])
            self.assertTrue(library.is_file())
            self.assertTrue(any("native_shim.c" in part for call in calls for part in call))
            vectors = out / "vectors.json"
            vectors.write_text(json.dumps([
                {"export": "add2", "args": [7, 9], "expected": hex((7 + 9) & 0xffffffff)},
                {"export": "scale", "args": [7, 9], "expected": hex((7 * 9 + 1) & 0xffffffff)},
            ]))
            self.assertEqual(backend.verify(library, out / "manifest.json", vectors), [])

    def test_the_fallback_writes_the_same_manifest_shape(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp, out = self.checkout(temp), Path(temp) / "out"
            tools = backend.find_toolchain(cache_dir=Path(temp) / "cache",
                                           download=lambda url, path: (_ for _ in ()).throw(OSError("blocked")))
            backend.build(decomp, out, toolchain=tools, compiler=self.compiler([]),
                          force_fallback=True)
            manifest = json.loads((out / "manifest.json").read_text())
            self.assertEqual(set(manifest), {"library", "exports", "globals", "global_values", "table_entries"})
            self.assertEqual(manifest["table_entries"], backend.TABLE_ENTRIES)

    def test_the_offline_command_forces_a_working_build(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp, out = self.checkout(temp), Path(temp) / "out"
            result = backend.main(["build", "--decomp", str(decomp), "--out", str(out),
                                   "--offline", "--cranelift"])
            self.assertEqual(result, 0)
            self.assertTrue((out / backend.detect_host()["library_name"]).is_file())
            self.assertEqual(json.loads((out / "report.json").read_text())["counts"],
                             {"compiled": 1, "skipped": 1})

    def test_a_missing_toolchain_is_reported_instead_of_silently_swapping_paths(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp, out = self.checkout(temp), Path(temp) / "out"
            with self.assertRaises(backend.ToolchainUnavailable):
                backend.build(decomp, out, toolchain={"host": backend.detect_host()},
                              compiler=self.compiler([]))

    def test_the_fallback_manifest_lists_the_symbol_values(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp, out = self.checkout(temp), Path(temp) / "out"
            tools = backend.find_toolchain(cache_dir=Path(temp) / "cache",
                                           download=lambda url, path: (_ for _ in ()).throw(OSError("blocked")))
            backend.build(decomp, out, toolchain=tools, compiler=self.compiler([]),
                          force_fallback=True)
            manifest = json.loads((out / "manifest.json").read_text())
            self.assertIn("add2", manifest["globals"])
            self.assertEqual(manifest["global_values"][manifest["globals"].index("add2")], 0x1000)
            self.assertEqual([e["address"] for e in manifest["exports"]], [0x1000, 0x2000])

    def test_the_fallback_shim_declares_the_raw_symbol_and_registers_it(self):
        with tempfile.TemporaryDirectory() as temp:
            shim = Path(temp) / "native_shim.c"
            exports = [{"name": "f", "cname": "?f@@YAHH@Z", "address": "0x1000",
                        "params": ["u32"], "result": "u32", "arity": 1, "wrapper": "decomp_call_0"}]
            backend.fallback_shim(shim, exports, host=backend.detect_host("Linux", "x86_64"))
            text = shim.read_text()
            self.assertIn('__asm__("?f@@YAHH@Z")', text)
            self.assertIn("decomp_native_0(a0)", text)
            self.assertIn("decomp_table_set(0x00001000U, (void*)decomp_native_0", text)
            self.assertIn("process-global", text)

    def test_windows_links_with_lld(self):
        self.assertIn("-fuse-ld=lld", backend.detect_host("Windows", "AMD64")["native_flags"])

    def test_exports_without_a_signature_are_skipped(self):
        units = [{"source": "src/pure.c", "functions": [
            {"address": "0x1000", "symbol": "_plain"}]}]
        exports, skipped = backend.exports_from_units(units)
        self.assertEqual(exports, [])
        self.assertIn("signature", skipped[0]["reason"])
        units[0]["functions"].append({"address": "0x2000", "symbol": "_noop", "arity": 0})
        exports, _ = backend.exports_from_units(units)
        self.assertEqual(exports[0]["name"], "noop")
        self.assertEqual(exports[0]["arity"], 0)


class FunctionAddressTests(unittest.TestCase):
    """Function addresses taken in the module must be the original addresses."""

    WAT = textwrap.dedent("""\
        (module
          (import "env" "__table_base" (global $__table_base i32))
          (func $add2 (param i32 i32) (result i32)
            i32.const 0))
          (func $scale (param i32 i32) (result i32)
            i32.const 0))
          (func $helper (param i32) (result i32)
            i32.const 0))
          (func $store0 (param i32)
            (local i32)
            global.get $__table_base
            local.set 1
            local.get 0
            i32.const 12
            i32.add
            local.get 1
            i32.const 0
            i32.add
            i32.store)
          (func $store1 (param i32)
            (local i32)
            global.get $__table_base
            local.set 1
            local.get 0
            i32.const 8
            i32.add
            local.get 1
            i32.const 1
            i32.add
            i32.store)
          (func $store2 (param i32)
            (local i32)
            global.get $__table_base
            local.set 1
            local.get 0
            i32.const 16
            i32.add
            local.get 1
            i32.const 2
            i32.add
            i32.store)
          (export "add2" (func $add2))
          (export "scale" (func $scale))
          (elem (;0;) (global.get $__table_base) func $add2 $scale $helper))
    """)

    def test_addresses_taken_in_the_module_become_the_original_addresses(self):
        wat, count = backend.rewrite_addresses(
            self.WAT, {"add2": 0x1000, "scale": 0x2000, "helper": 0x3000})
        self.assertEqual(count, 3)
        self.assertIn("i32.const 4096", wat)
        self.assertIn("i32.const 8192", wat)
        self.assertIn("i32.const 12288", wat)
        self.assertNotIn("local.get 1\n    i32.const 1\n    i32.add", wat)

    def test_an_unknown_element_index_is_left_alone_beside_a_known_one(self):
        wat, count = backend.rewrite_addresses(self.WAT, {"add2": 0x1000})
        self.assertEqual(count, 1)
        self.assertIn("i32.const 4096", wat)
        self.assertIn("local.get 1\n    i32.const 1\n    i32.add", wat)
        self.assertIn("local.get 1\n    i32.const 2\n    i32.add", wat)

    def test_a_local_function_that_is_not_exported_is_bound_too(self):
        wat, count = backend.rewrite_addresses(self.WAT, {"helper": 0x3000})
        self.assertEqual(count, 1)
        self.assertIn("i32.const 12288", wat)
        self.assertIn("local.get 1\n    i32.const 1\n    i32.add", wat)

    def test_the_shim_exposes_the_trap_and_the_table_check(self):
        with tempfile.TemporaryDirectory() as temp:
            shim = Path(temp) / "shim.c"
            entry = {"name": "add2", "cname": "w2c_mod_add2", "address": "0x1000",
                     "params": ["u32", "u32"], "result": "u32", "arity": 2,
                     "wrapper": "decomp_call_0"}
            backend.generate_shim(shim, [], [entry], [], "&inst", host=backend.detect_host())
            text = shim.read_text()
            self.assertIn("decomp_last_trap_code", text)
            self.assertIn("decomp_last_trap_reason", text)
            self.assertIn("decomp_table_has", text)
            self.assertIn("WASM_RT_SETJMP(g_wasm_rt_jmp_buf)", text)
            self.assertIn("process-global: one caller at a time", text)

    def test_got_func_accessors_hold_original_addresses(self):
        with tempfile.TemporaryDirectory() as temp:
            shim = Path(temp) / "shim.c"
            imports = [{"module": "GOT.func", "field": "missing",
                        "name": "w2c_GOT0x2Efunc_missing",
                        "decl": "u32 *w2c_GOT0x2Efunc_missing;",
                        "params": [], "function": False}]
            backend.generate_shim(shim, imports, [], [], "&inst",
                                  func_globals=["missing"],
                                  func_values={"missing": 0x4000}, host=backend.detect_host())
            text = shim.read_text()
            self.assertIn("struct w2c_GOT0x2Efunc", text)
            self.assertIn("u32* w2c_GOT0x2Efunc_missing(struct w2c_GOT0x2Efunc* g)", text)
            self.assertIn("fgot.f0 = 0x00004000U;", text)

    def test_an_export_at_address_zero_still_gets_a_table_slot(self):
        with tempfile.TemporaryDirectory() as temp:
            shim = Path(temp) / "shim.c"
            entry = {"name": "zero", "cname": "w2c_mod_zero", "address": 0,
                     "params": [], "result": "u32", "arity": 0, "wrapper": "decomp_call_0"}
            backend.generate_shim(shim, [], [entry], [], "&inst", host=backend.detect_host())
            self.assertIn("env.table.data[0x00000000U]", shim.read_text())

    def test_address_literals_are_unsigned_and_within_32_bits(self):
        self.assertEqual(backend.address_literal("0xdeadbeef"), "0xdeadbeefU")
        self.assertEqual(backend.address_literal(0x100000000), "0x00000000U")

    def test_an_export_with_only_a_literal_underscore_symbol_is_resolved(self):
        exports = [{"name": "f", "cname": "w2c_mod_f", "params": [], "result": "u32"}]
        manifest = {"units": [], "symbols": {"_f": "0x1000"}}
        result = backend.export_manifest(exports, [], backend.collect_symbols(manifest, []))
        self.assertEqual(result[0]["address"], 0x1000)

    def test_an_unresolved_got_symbol_is_reported_instead_of_bound_to_zero(self):
        with self.assertRaisesRegex(RuntimeError, "unresolved GOT symbol\\(s\\): missing"):
            backend.got_values(["known", "missing"], ["also_missing"], {"known": 0x1000})


class GapUnitTests(unittest.TestCase):
    """What the build does for the units that hold the real translation gaps (nv-rs#1000).

    Every source here is written by the test; nothing is read from a decomp checkout.
    """

    def test_an_asm_block_becomes_the_trapping_stub(self):
        text = "void f() { __asm { mov eax, 1 ; }\n  g(); }\nvoid h() { __asm mov eax, 2\n  k(); }\n"
        rewritten, count = backend.rewrite_inline_asm(text)
        self.assertEqual(count, 2)
        self.assertNotIn("mov", rewritten)
        self.assertIn("g(); }", rewritten)
        self.assertIn("k(); }", rewritten)
        self.assertEqual(rewritten.count(backend.ASM_STUB + "();"), 2)

    def test_asm_in_a_comment_or_a_string_or_a_longer_word_is_left_alone(self):
        text = '// __asm { x }\nconst char* s = "__asm";\nint __asm__x; void __asm__(int);\n'
        self.assertEqual(backend.rewrite_inline_asm(text), (text, 0))

    def test_the_marker_names_the_export_by_address(self):
        text = "// @fnv 0x00a7fc00\nint* A::B(int x,\n   int y)\n{\n}\n"
        rewritten, count = backend.annotate_function_exports(text)
        self.assertEqual(count, 1)
        self.assertIn('__attribute__((export_name("fnv_00a7fc00"))) int* A::B(', rewritten)

    def test_markers_over_anything_but_a_definition_are_left_alone(self):
        for head in ("template <class T> void f() {}", "extern int DAT_01;", "int f(int);",
                     "int g_x = 3;", "struct S { int x; };", "A::A() {}", "A::~A() {}",
                     "__declspec(noinline) A::~A() {}"):
            text = "// @fnv 0x00a7fc00\n" + head + "\n"
            self.assertEqual(backend.annotate_function_exports(text), (text, 0), head)

    def test_a_variant_function_at_an_address_an_earlier_unit_exports_loses_the_name(self):
        with tempfile.TemporaryDirectory() as temp:
            copy = Path(temp)
            attribute = '__attribute__((export_name("fnv_00a7fc00"))) '
            (copy / "a.cpp").write_text(attribute + "int f() { return 1; }\n")
            (copy / "b.cpp").write_text(attribute + "int f() { return 2; }\n")
            removed = backend.keep_first_export_of_each_address(copy, ["a.cpp", "b.cpp"])
            self.assertEqual(removed, 1)
            self.assertIn("export_name", (copy / "a.cpp").read_text())
            self.assertNotIn("export_name", (copy / "b.cpp").read_text())

    def test_a_decomp_declaration_that_contradicts_the_system_header_is_dropped(self):
        with tempfile.TemporaryDirectory() as temp:
            copy = Path(temp) / "copy"
            copy.mkdir()
            header = copy / "x.h"
            header.write_text('extern "C" void* __stdcall TlsGetValue(unsigned int);\nint keep();\n')
            system = Path(temp) / "sys.h"
            system.write_text("")
            stderr = (
                "%s:3:1: error: conflicting types for 'TlsGetValue'\n"
                "%s:1:50: note: previous declaration is here\n" % (system, header))
            self.assertEqual(backend.drop_conflicting_declarations(stderr, copy), 1)
            lines = header.read_text().split("\n")
            self.assertEqual(lines[0], "#include <windows.h>")  # the system header came second
            self.assertEqual(lines[1], "int keep();")
            stderr = "%s:2:1: error: conflicting types for 'keep'\n" % header
            self.assertEqual(backend.drop_conflicting_declarations(stderr, copy), 1)
            self.assertEqual(header.read_text().split("\n")[1], "")

    def test_a_second_local_m128_definition_is_dropped(self):
        with tempfile.TemporaryDirectory() as temp:
            copy = Path(temp)
            source = copy / "x.cpp"
            source.write_text(
                "typedef union { float a[4]; } __m128, LocalM128;\n"
                "extern \"C\" LocalM128 f();\n"
                "namespace n {\n"
                "typedef union {\n"
                "    float a[4];\n"
                "} __m128, LocalM128;\n"
                "extern \"C\" LocalM128 f();\n"
                "}\n")
            stderr = ("%s:7:22: error: functions that differ only in their return type cannot be "
                      "overloaded\n" % source)
            self.assertEqual(backend.drop_duplicate_local_m128(stderr, copy), 1)
            lines = source.read_text().split("\n")
            self.assertIn("LocalM128", lines[0])  # the first definition stays
            self.assertEqual(lines[3:6], ["", "", ""])
            self.assertEqual(len(lines), 9)  # line numbers stay

    def test_a_definition_is_never_dropped_as_a_declaration(self):
        with tempfile.TemporaryDirectory() as temp:
            copy = Path(temp)
            source = copy / "x.cpp"
            source.write_text("int f() { return 1; }\n")
            stderr = "%s:1:1: error: conflicting types for 'f'\n" % source
            self.assertEqual(backend.drop_conflicting_declarations(stderr, copy), 0)
            self.assertIn("return 1", source.read_text())

    WAT = textwrap.dedent("""\
        (module
          (import "GOT.mem" "known" (global (;0;) (mut i32)))
          (import "GOT.mem" "_ZTVmissing" (global (;1;) (mut i32)))
          (func (;3;) (type 0)
            call 4)
          (func (;4;) (type 0))
          (func (;5;) (type 0)
            global.get 1)
          (func (;6;) (type 0)
            global.get 0)
          (export "__nv_untranslated_asm" (func 4))
          (export "fnv_00000003" (func 3))
          (export "fnv_00000005" (func 5))
          (export "fnv_00000006" (func 6))
        )
        """)

    def test_exports_that_cannot_run_are_left_out_with_the_reason(self):
        reasons = backend.exports_left_out(self.WAT, ["_ZTVmissing"])
        self.assertEqual(sorted(reasons), ["fnv_00000003", "fnv_00000005"])
        self.assertIn("inline assembly", reasons["fnv_00000003"])
        self.assertIn("_ZTVmissing", reasons["fnv_00000005"])
        self.assertNotIn("fnv_00000006", reasons)

    def test_an_address_in_a_global_name_is_read_from_the_name(self):
        for name in ("DAT_01017130", "_ZN12fnv_00b13c1012DAT_01017130E", "fnv_01017130"):
            self.assertEqual(backend.symbol_address({}, name), 0x01017130, name)
        self.assertIsNone(backend.symbol_address({}, "g_memoryManager"))

    def test_a_lenient_binding_zeroes_the_unresolved_and_names_them(self):
        mem, funcs, unresolved = backend.got_values(
            ["known", "missing"], ["also_missing"], {"known": 0x1000}, lenient=True)
        self.assertEqual((mem, funcs, unresolved), ([0x1000, 0], {"also_missing": 0}, ["missing", "also_missing"]))

    def test_wasm2c_names_are_unmangled(self):
        self.assertEqual(backend.unmangle("0x5FZN12fnv_00c8954015g_tlsE"), "_ZN12fnv_00c8954015g_tlsE")

    def test_the_gap_list_selects_the_units_that_hold_its_addresses(self):
        units = [{"source": "a.cpp", "functions": [{"address": "0x00c86060"}]},
                 {"source": "b.cpp", "functions": [{"address": "0x00000001"}]},
                 {"source": "c.cpp", "functions": [{"address": "0x00c86940"}, {"address": "0x00c9de40"}]}]
        with tempfile.TemporaryDirectory() as temp:
            gaps = Path(temp) / "gaps.txt"
            gaps.write_text("# header\n00c86940 x.rs eax\n00c86060 y.rs registers\n00dead00 z.rs eax\n")
            addresses = backend.read_gap_addresses(gaps)
        self.assertEqual(addresses, [0x00c86940, 0x00c86060, 0x00dead00])
        self.assertEqual(backend.gap_units(units, addresses), ["a.cpp", "c.cpp"])

    def test_units_are_built_in_the_order_they_were_named(self):
        units = [{"source": "a.cpp"}, {"source": "b.cpp"}, {"source": "c.cpp"}]
        chosen = backend.select_units(units, ["c.cpp", "a.cpp", "c.cpp", "missing.cpp"])
        self.assertEqual([u["source"] for u in chosen], ["c.cpp", "a.cpp"])

    def test_the_manifest_carries_wide_exports_and_the_tsv_says_so(self):
        exports = [
            {"name": "w", "cname": "w2c_mod_w", "params": ["u32"] * 9, "result": "u64"},
            {"name": "d", "cname": "w2c_mod_d", "params": ["f32"], "result": "f64"},
            {"name": "x", "cname": "w2c_mod_x", "params": ["u32"] * 13, "result": "u32"},
        ]
        symbols = {"w": 0x1000, "d": 0x2000, "x": 0x3000}
        result = backend.export_manifest(exports, [], symbols)
        for index, entry in enumerate(result):
            entry["wrapper"] = "decomp_call_%d" % index
        self.assertEqual([e["arity"] for e in result], [9, 1, None])
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp)
            report = {"library": "x", "counts": {"compiled": 0, "skipped": 0}, "compiled": [], "skipped": []}
            backend.write_build_outputs(out, report, result, [], [])
            tsv = (out / "exports.tsv").read_text().splitlines()
        self.assertIn("export 00001000 decomp_call_0 9 u64", tsv)
        self.assertIn("export 00002000 decomp_call_1 1 f64", tsv)
        self.assertFalse([line for line in tsv if "00003000" in line])

    def test_the_wrapper_hands_back_edx_and_st0_and_takes_words(self):
        with tempfile.TemporaryDirectory() as temp:
            shim = Path(temp) / "shim.c"
            exports = [
                {"name": "w", "cname": "w2c_mod_w", "params": ["u32", "f32"], "result": "u64",
                 "address": 0x1000, "arity": 2},
                {"name": "d", "cname": "w2c_mod_d", "params": [], "result": "f64",
                 "address": 0x2000, "arity": 0},
            ]
            for index, entry in enumerate(exports):
                entry["wrapper"] = "decomp_call_%d" % index
            backend.generate_shim(shim, [], exports, [], "w2c_mod* inst", host=backend.detect_host("Linux", "x86-64"))
            text = shim.read_text()
        self.assertIn("uint32_t decomp_last_edx(void)", text)
        self.assertIn("double decomp_last_st0(void)", text)
        self.assertIn("u32 decomp_call_0(u32 a0, u32 a1)", text)
        self.assertIn(".u=a1}).f", text)  # the f32 parameter is the word's bits
        self.assertIn("decomp_edx = (u32)(v >> 32)", text)
        self.assertIn("decomp_st0 = v", text)
        self.assertIn("decomp_edx = 0; decomp_st0 = 0.0;", text)

    def test_an_import_with_no_parameters_makes_a_valid_stub(self):
        imports = [{"module": "env", "field": "_Z3foov", "decl": "void w2c_env_0x5FZ3foov(struct w2c_env*)",
                    "name": "w2c_env_0x5FZ3foov", "params": [], "result": "void", "function": True}]
        with tempfile.TemporaryDirectory() as temp:
            shim = Path(temp) / "shim.c"
            backend.generate_shim(shim, imports, [], [], "w2c_mod* inst", host=backend.detect_host("Linux", "x86-64"))
            text = shim.read_text()
        self.assertIn("w2c_env_0x5FZ3foov(struct w2c_env* e) {", text)
        self.assertIn("u32 a[] = {0};", text)
        self.assertNotIn("a[] = NULL", text)

    def test_a_module_without_data_relocations_does_not_call_them(self):
        with tempfile.TemporaryDirectory() as temp:
            shim = Path(temp) / "shim.c"
            backend.generate_shim(shim, [], [], [], "w2c_mod* inst",
                                  host=backend.detect_host("Linux", "x86-64"), startup=[])
            text = shim.read_text()
        self.assertNotIn("apply_data_relocs", text)
        self.assertNotIn("call_ctors", text)

    def test_the_mingw_math_header_loses_its_x87_assembly(self):
        text = ("#define _MATH_H_\nfloat fabsf (float x) { float res = 0.0F;\n"
                '__asm__ __volatile__ ("fabs;" : "=t" (res) : "0" (x));\nreturn res; }\n'
                'int c(double _x) { unsigned short sw;\n'
                '__asm__ __volatile__ ("fxam;"\n"fstsw %%ax": "=a" (sw) : "t" (_x));\nreturn sw; }\n')
        rewritten = backend.standin_math(text)
        self.assertNotIn("__asm__", rewritten)
        self.assertIn("res = __builtin_fabs(x);", rewritten)
        self.assertIn("sw = __nv_fxam(_x);", rewritten)
        self.assertIn("__nv_fxam(long double", rewritten)

    def test_the_decomp_declarations_clang_refuses_are_removed_from_the_copy(self):
        text = ('extern "C" void* __cdecl _alloca(unsigned int);\n'
                'extern "C" void _ReadWriteBarrier();\n'
                'extern "C" void __cdecl operator delete(void*, unsigned int);\n'
                "typedef unsigned int C8size_t;\n"
                "__declspec(naked) void __fastcall F(int);\n"
                "FNV_FORCEINLINE long InterlockedDecrement(volatile long* value)\n")
        rewritten = backend.rewrite_source(text)
        self.assertNotIn("_alloca", rewritten)
        self.assertNotIn("_ReadWriteBarrier", rewritten)
        self.assertIn("void __cdecl operator delete(void*, __SIZE_TYPE__);", rewritten)
        self.assertNotIn('extern "C" void __cdecl operator delete', rewritten)
        self.assertIn("typedef __SIZE_TYPE__ C8size_t;", rewritten)
        self.assertNotIn("naked", rewritten)
        self.assertIn("Fnv_InterlockedDecrement(", rewritten)


class GapUnitToolchainTests(unittest.TestCase):
    """Builds two synthetic units that hold an asm block and a variant, when the toolchain is there."""

    @unittest.skipUnless(os.environ.get("NV_BACKEND_TOOLCHAIN_TEST") == "1",
                         "set NV_BACKEND_TOOLCHAIN_TEST=1 inside the nix shell")
    def test_an_asm_function_is_left_out_and_a_plain_one_is_exported_by_address(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp = Path(temp) / "decomp"
            (decomp / "build").mkdir(parents=True)
            (decomp / "include").mkdir()
            (decomp / "src").mkdir()
            (decomp / "src" / "gap.cpp").write_text(textwrap.dedent("""\
                // @fnv 0x00401000
                int Plain(int a) { return a + 1; }

                // @fnv 0x00401010
                int __declspec(naked) WithAsm(int a) { __asm { mov eax, a ; ret } }
                """))
            units = {"units": [{"source": "src/gap.cpp", "functions": [
                {"address": "0x00401000", "symbol": "?Plain@@YAHH@Z", "size": 3},
                {"address": "0x00401010", "symbol": "?WithAsm@@YAHH@Z", "size": 3}]}]}
            (decomp / "build" / "units.json").write_text(json.dumps(units))
            out = Path(temp) / "out"
            report = backend.build(decomp, out)
            exported = {e["address"] for e in report["exports"]}
            self.assertIn(0x00401000, exported)
            self.assertNotIn(0x00401010, exported)
            self.assertEqual([e["address"] for e in report["left_out_of_exports"]], [0x00401010])
            tsv = (out / "exports.tsv").read_text()
            self.assertIn("export 00401000 ", tsv)
            self.assertNotIn("export 00401010 ", tsv)


class NoBackendTests(unittest.TestCase):
    def test_a_missing_library_loads_as_none(self):
        with tempfile.TemporaryDirectory() as temp:
            self.assertIsNone(backend.load_library(Path(temp) / "libfnv_decomp.so"))

    def test_verify_reports_a_missing_backend_without_raising(self):
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp)
            (out / "manifest.json").write_text(json.dumps({"exports": [], "table_entries": 1}))
            (out / "vectors.json").write_text("[]")
            result = backend.verify(out / "libfnv_decomp.so", out / "manifest.json", out / "vectors.json")
            self.assertEqual(result[0]["reason"], "backend is not built")


class DuplicateExportTests(unittest.TestCase):
    def test_the_later_unit_holding_the_address_is_dropped_and_the_rest_relinked(self):
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp)
            (out / "copy").mkdir()
            (out / "copy" / "a.cpp").write_text("// @fnv 0x00c711d0\nvoid f();\n")
            (out / "copy" / "b.cpp").write_text("void g();\n")
            chosen = [{"source": "a.cpp"}, {"source": "b.cpp"}]
            outcomes = [{"ok": True, "reason": "", "object": "a.o"}, {"ok": True, "reason": "", "object": "b.o"}]
            objects = [Path("a.o"), Path("b.o")]
            replies = [mock.Mock(returncode=1, stderr='x: error: duplicate export "fnv_00c711d0"'),
                       mock.Mock(returncode=0, stderr="")]
            with mock.patch.object(backend, "wabt_tool", return_value="wasm2wat"), \
                    mock.patch.object(backend, "run", side_effect=replies), \
                    mock.patch.object(backend, "link_without_duplicates",
                                      side_effect=lambda t, o, r, w, c, oc: o):
                left = backend.drop_duplicate_exports({}, out, objects, "rt", "m.wasm", chosen, outcomes)
            self.assertEqual([Path("b.o")], left)
            self.assertFalse(outcomes[0]["ok"])
            self.assertIn("duplicate export fnv_00c711d0", outcomes[0]["reason"])


class SseHeaderTests(unittest.TestCase):
    def test_the_intrinsic_headers_define_the_type_once_and_declare_the_functions(self):
        with tempfile.TemporaryDirectory() as temp:
            standins = backend.make_standins(Path(temp), {})
            for name in ("xmmintrin.h", "sse_types.h"):
                text = (standins / name).read_text()
                self.assertIn("#ifndef _INCLUDED_MM2", text)
                self.assertIn("__m128 _mm_shuffle_ps(__m128, __m128, int);", text)
                self.assertIn("#define _MM_SHUFFLE(", text)
            # windows.h includes intrin.h: it stays empty so a unit's own spelling is the only one.
            self.assertEqual("#pragma once\n", (standins / "intrin.h").read_text())

    def test_every_declared_intrinsic_is_defined_by_the_runtime(self):
        declared = set(re.findall(r"\b(_mm_\w+)\(", backend.SSE_DECLS))
        defined = set(re.findall(r"\b(_mm_\w+)\b", backend.SSE_SOURCE))
        self.assertEqual(set(), declared - defined)


class EstimateStandinTests(unittest.TestCase):
    """The `_mm_rcp_*` and `_mm_rsqrt_*` stand-ins answer as the 12-bit estimate does."""

    def test_the_tables_are_read_from_the_engines_estimate_module(self):
        source = backend.estimate_source()
        self.assertIn("nv_rcp_table[4096]", source)
        self.assertIn("nv_rsqrt_table[8192]", source)
        self.assertIn("0x6ffe,0x6ffc", source)  # the first two entries of estimate/tables.rs

    def test_a_table_of_the_wrong_size_is_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "tables.rs"
            path.write_text("const RCP: [u16; 4096] = [0x1];\nconst RSQRT: [u16; 8192] = [0x1];\n")
            with self.assertRaises(ValueError):
                backend.estimate_source(path)

    def test_the_standins_call_the_estimate_not_the_exact_reciprocal(self):
        self.assertIn("nv_rcp_bits(a.u[i])", backend.SSE_SOURCE)
        self.assertNotIn("1.0f / a.f[i]", backend.SSE_SOURCE)

    @unittest.skipUnless(shutil.which("cc") and os.uname().machine == "x86_64", "needs cc on x86-64")
    def test_the_c_model_is_the_instruction_on_the_measured_processor(self):
        program = backend.estimate_source() + textwrap.dedent("""
            #include <stdio.h>
            #include <string.h>
            #include <xmmintrin.h>
            static unsigned host_rcp(unsigned x) { float f; memcpy(&f, &x, 4);
              __m128 r = _mm_rcp_ss(_mm_set_ss(f)); unsigned h; memcpy(&h, &r, 4); return h; }
            static unsigned host_rsqrt(unsigned x) { float f; memcpy(&f, &x, 4);
              __m128 r = _mm_rsqrt_ss(_mm_set_ss(f)); unsigned h; memcpy(&h, &r, 4); return h; }
            int main(void) {
              unsigned probe[] = {0x40000000u, 0x40400000u, 0x3f333333u, 0x4640e6b6u, 0x3f800000u, 0x3ffff000u};
              for (int i = 0; i < 6; i++)
                if (host_rcp(probe[i]) != nv_rcp_bits(probe[i]) || host_rsqrt(probe[i]) != nv_rsqrt_bits(probe[i])) {
                  puts("other"); return 0; }
              unsigned bad = 0;
              for (unsigned long long k = 0; k < (1ull << 32); k += 4099) {
                unsigned x = (unsigned)k;
                if (host_rcp(x) != nv_rcp_bits(x) || host_rsqrt(x) != nv_rsqrt_bits(x)) bad++; }
              puts(bad ? "differ" : "same"); return 0; }
        """)
        with tempfile.TemporaryDirectory() as temp:
            (Path(temp) / "t.c").write_text(program)
            subprocess.run(["cc", "-O1", "-o", str(Path(temp) / "t"), str(Path(temp) / "t.c")], check=True)
            answer = subprocess.run([str(Path(temp) / "t")], capture_output=True, text=True,
                                    check=True).stdout.strip()
        if answer == "other":
            self.skipTest("not the processor the estimate tables were measured on")
        self.assertEqual(answer, "same")


if __name__ == "__main__":
    unittest.main()
