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
import zipfile
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
        unsigned add2(unsigned a, unsigned b) { volatile unsigned z = a ^ b; return a + b + z - z; }
        unsigned scale(unsigned a, unsigned b) { volatile unsigned z = a | b; return a * b + 1 + z - z; }
        unsigned (*volatile slot)(unsigned, unsigned);
        unsigned store(void) { slot = &add2; return (unsigned)slot; }
        unsigned callslot(unsigned a, unsigned b) { return slot(a, b); }
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
            for name in ("bin/wasm-ld", "bin/wasm2c",
                         "share/wabt/wasm2c/wasm-rt-impl.c",
                         "share/wabt/wasm2c/wasm-rt-mem-impl.c", "include/wasm-rt.h"):
                archive.writestr(name + suffix if name.startswith("bin/") else name, "")
        return clang, wabt

    def test_a_missing_toolchain_is_fetched_once_and_then_cached(self):
        with tempfile.TemporaryDirectory() as temp:
            clang, wabt = self.archives(temp)
            cache = Path(temp) / "cache"
            calls = []

            def download(url, path):
                calls.append(url)
                shutil.copyfile(clang if "clang" in url else wabt, path)

            host = backend.detect_host("Linux", "x86_64")
            first = backend.ensure_toolchain(cache, host, download=download)
            second = backend.ensure_toolchain(cache, host, download=download)
            self.assertTrue(first["fetched"])
            self.assertFalse(second["fetched"])
            self.assertEqual(len(calls), 2)
            for key in ("clang", "wasm_ld", "wasm2c"):
                self.assertTrue(Path(second[key]).is_file())

    def test_a_second_find_run_uses_the_cache(self):
        with tempfile.TemporaryDirectory() as temp:
            clang, wabt = self.archives(temp)
            cache = Path(temp) / "cache"
            calls = []

            def download(url, path):
                calls.append(url)
                shutil.copyfile(clang if "clang" in url else wabt, path)

            with mock.patch.dict(os.environ, {}, clear=True):
                first = backend.find_toolchain(cache_dir=cache, download=download)
                second = backend.find_toolchain(cache_dir=cache, download=download)
            self.assertEqual(len(calls), 2)
            self.assertEqual(first["clang"], second["clang"])
            self.assertTrue(backend.toolchain_ready(second))
            self.assertTrue(backend.wasm_runtime_ready(second))

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

            backend.ensure_toolchain(cache, backend.detect_host("Linux", "x86_64"), download=download)
            backend.ensure_toolchain(cache, backend.detect_host("Windows", "AMD64"), download=download)
            self.assertEqual(len(calls), 4)

    def test_a_blocked_fetch_is_reported(self):
        with tempfile.TemporaryDirectory() as temp:
            def blocked(url, path):
                raise OSError("network is disabled")

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
                         "NV_WASM2C": "/tools/wasm2c"}
            with mock.patch.dict(os.environ, overrides):
                tools = backend.find_toolchain(cache_dir=Path(temp) / "cache", download=download)
            self.assertEqual(calls, [])
            self.assertEqual(tools["clang"], "/tools/clang")
            self.assertTrue(backend.toolchain_ready(tools))

    def test_a_failed_fetch_leaves_the_toolchain_unready(self):
        with tempfile.TemporaryDirectory() as temp:
            def blocked(url, path):
                raise OSError("blocked")

            tools = backend.find_toolchain(cache_dir=Path(temp) / "cache", download=blocked)
            self.assertFalse(backend.toolchain_ready(tools))
            self.assertIn("toolchain_error", tools)


@unittest.skipUnless(shutil.which("cc"), "needs a local C compiler")
class CraneliftTests(unittest.TestCase):
    """The alternate build path, driven by an injected compiler instead of clang."""

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
            report = backend.build(decomp, out, toolchain=tools, compiler=self.compiler(calls))
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
            backend.build(decomp, out, toolchain=tools, compiler=self.compiler([]))
            manifest = json.loads((out / "manifest.json").read_text())
            self.assertEqual(set(manifest), {"library", "exports", "globals", "global_values", "table_entries"})
            self.assertEqual(manifest["table_entries"], backend.TABLE_ENTRIES)

    def test_the_offline_command_forces_a_working_build(self):
        with tempfile.TemporaryDirectory() as temp:
            decomp, out = self.checkout(temp), Path(temp) / "out"
            result = backend.main(["build", "--decomp", str(decomp), "--out", str(out),
                                   "--offline", "--cranelift"])
            self.assertEqual(result, 0)
            self.assertTrue((out / "libfnv_decomp.so").is_file())
            self.assertEqual(json.loads((out / "report.json").read_text())["counts"],
                             {"compiled": 1, "skipped": 1})

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
          (func $store (param i32)
            (local i32)
            global.get $__table_base
            local.set 1
            local.get 0
            i32.const 4
            i32.add
            local.get 1
            i32.const 1
            i32.add
            i32.store)
          (export "add2" (func $add2))
          (export "scale" (func $scale))
          (elem (;0;) (global.get $__table_base) func $add2 $scale))
    """)

    def test_an_address_taken_in_the_module_becomes_the_original_address(self):
        wat, count = backend.rewrite_addresses(
            self.WAT, {"add2": 0x1000, "scale": 0x2000})
        self.assertEqual(count, 1)
        self.assertIn("i32.const 8192", wat)
        self.assertNotIn("local.get 1\n    i32.const 1\n    i32.add", wat)

    def test_an_unknown_element_index_is_left_alone(self):
        wat, count = backend.rewrite_addresses(self.WAT, {"add2": 0x1000})
        self.assertEqual(count, 0)
        self.assertEqual(wat, self.WAT)

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

    def test_got_func_accessors_hold_original_addresses(self):
        with tempfile.TemporaryDirectory() as temp:
            shim = Path(temp) / "shim.c"
            imports = [{"module": "GOT.func", "field": "missing",
                        "name": "w2c_GOT0x2Efunc_missing",
                        "decl": "u32 *w2c_GOT0x2Efunc_missing;",
                        "params": [], "function": False}]
            backend.generate_shim(shim, imports, [], [], "&inst",
                                  func_globals=["missing"], host=backend.detect_host())
            text = shim.read_text()
            self.assertIn("struct w2c_GOT0x2Efunc", text)
            self.assertIn("u32* w2c_GOT0x2Efunc_missing(struct w2c_GOT0x2Efunc* g)", text)


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


if __name__ == "__main__":
    unittest.main()
