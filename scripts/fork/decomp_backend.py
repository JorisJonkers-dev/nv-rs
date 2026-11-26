#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Build fnv_decomp units into a native library on the user's own machine (nv-rs#926).

  decomp_backend.py build --decomp DIR [--unit SOURCE ...] [--limit N]
                          [--out DIR] [--report FILE]
  decomp_backend.py layout --decomp DIR [--unit SOURCE ...] [--limit N] [--out DIR]
  decomp_backend.py verify --library SO --manifest FILE --vectors FILE
  decomp_backend.py hygiene [--root DIR]

`build` rewrites a private build copy, compiles the selected units to wasm32 with
clang's MSVC-compatible layout flags and LTO, links with imported memory and table,
converts the module with wasm2c and compiles it into a shared library. The library's
linear memory is the guest reservation, its table is host-owned and indexed by the
original addresses, and its GOT.mem globals can be bound to original addresses.
Units that do not compile are reported and left to their translations.

`layout` dumps the records each unit lays out for i686-MSVC and for wasm32 with the
same flags, and fails on any difference. `verify` calls exports through the library
and compares them with results supplied by the translations. `hygiene` refuses built
artefacts that are visible to git in the repository tree.

Linux and macOS on x86-64 and aarch64 and Windows are supported. clang and wabt are
found on PATH or in a per-user cache; a missing toolchain is fetched once into that
cache. When fetching fails, `build` compiles the same selected units through the
native fallback, which keeps the library's ABI, report and manifest. `--offline`
blocks fetching and `--cranelift` forces the fallback, for hosts without a network.

Nothing this tool writes is added to the repository: the default output directory
is git-ignored. Stdlib only; Python 3.11 or newer.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request

TABLE_ENTRIES = 0x01000000  # the guest's .text VAs all fit below 16 MiB
GUEST_SIZE = 1 << 32
BANNED_SUFFIXES = {".so", ".a", ".lib", ".o", ".obj", ".dylib", ".dll", ".exe", ".pdb", ".wasm", ".cwasm"}
GENERATED_NAMES = {"mod.c", "mod.h", "wasm-rt-impl.c", "wasm-rt-mem-impl.c"}
TOOLCHAIN_VERSION = "v1"
TOOLCHAIN_URL = "https://github.com/JorisJonkers-dev/nv-rs-backend-toolchain/releases/download/v1"
TOOLCHAIN_ASSETS = ("clang", "wabt")
REQUIRED_TOOLS = ("clang", "wasm_ld", "wasm2c")


class ToolchainUnavailable(RuntimeError):
    """The clang/wasm2c toolchain is neither installed nor fetchable on this host."""


SIZE_T_OPS = re.compile(r"(operator\s+(?:new|delete)(?:\[\])?\s*\((?:[^(),]*, ?)?)unsigned int")
NAMED_ENUM = re.compile(r"\benum(\s+)(?!class\b|struct\b)([A-Za-z_]\w*)(\s*)([;{])")
IMPORT_RE = re.compile(r"/\* import: '([^']+)' '([^']+)' \*/\s*\n\s*([^\n;]+);")
EXPORT_RE = re.compile(r"/\* export: '([^']+)' \*/\s*\n\s*([^\n;]+);")
ELEM_RE = re.compile(r"\(elem\s+\(;(\d+);\)\s+\(([^)]*)\)\s+func\s+([^)]*)\)")
WAT_EXPORT_RE = re.compile(r'\(export\s+"([^"]+)"\s+\(func\s+\$?([\w.]+)\)\)')
TABLE_BASE_LOCAL_RE = re.compile(r"global\.get\s+\$__table_base\s*\n\s*local\.set\s+(\d+)")


def rewrite_source(text):
    """Apply the two build-copy-only rewrites: CRT size_t spelling and MSVC enum width."""
    text = SIZE_T_OPS.sub(lambda m: m.group(1) + "__SIZE_TYPE__", text)
    text = text.replace("typedef unsigned int size_t;", "typedef __SIZE_TYPE__ size_t;")
    return NAMED_ENUM.sub(r"enum\1\2 : int\3\4", text)


def prepare_build_copy(decomp, out):
    """Copy the decomp's sources into the build copy and rewrite them there."""
    decomp, out = Path(decomp), Path(out)
    copy = out / "copy"
    if copy.exists():
        shutil.rmtree(copy)
    copy.mkdir(parents=True)
    for name in ("include", "src", "third_party"):
        if (decomp / name).is_dir():
            shutil.copytree(decomp / name, copy / name)
    changed = 0
    for path in copy.rglob("*"):
        if path.is_file() and path.suffix in {".h", ".c", ".cpp", ".hpp"}:
            original = path.read_text(errors="surrogateescape")
            rewritten = rewrite_source(original)
            if rewritten != original:
                path.write_text(rewritten, errors="surrogateescape")
                changed += 1
    return changed


def make_standins(out, tools):
    """Write the small mingw-w64/SSE stand-ins the wasm units compile against."""
    standins = Path(out) / "standins"
    (standins / "psdk_inc").mkdir(parents=True, exist_ok=True)
    mingw = tools.get("mingw")
    if mingw:
        core = (Path(mingw) / "corecrt.h").read_text(errors="surrogateescape")
        for old, new in (
            ("typedef unsigned int size_t;", "typedef __SIZE_TYPE__ size_t;"),
            ("typedef int ssize_t;", "typedef __INTPTR_TYPE__ ssize_t;"),
            ("typedef int intptr_t;", "typedef __INTPTR_TYPE__ intptr_t;"),
            ("typedef unsigned int uintptr_t;", "typedef __UINTPTR_TYPE__ uintptr_t;"),
            ("typedef int ptrdiff_t;", "typedef __PTRDIFF_TYPE__ ptrdiff_t;"),
        ):
            core = core.replace(old, new)
        (standins / "corecrt.h").write_text(core)
        intrin = (Path(mingw) / "psdk_inc" / "intrin-impl.h").read_text(errors="surrogateescape")
        intrin = intrin.replace('#define __buildint(a) __asm__ __volatile__("int {$}" #a :)',
                                "#define __buildint(a) __builtin_trap()")
        intrin = "\n".join(line for line in intrin.split("\n")
                           if not re.match(r"\s*(?:__INTRINSICS_USEINLINE|__build\w*\(.*\)\s*;?\s*$)", line))
        (standins / "psdk_inc" / "intrin-impl.h").write_text(intrin)
    vector = ("#pragma once\n"
              "typedef float __m128 __attribute__((__vector_size__(16), __aligned__(16)));\n"
              "typedef long long __m128i __attribute__((__vector_size__(16), __aligned__(16)));\n"
              "typedef double __m128d __attribute__((__vector_size__(16), __aligned__(16)));\n"
              "typedef long long __m64 __attribute__((__vector_size__(8), __aligned__(8)));\n")
    for name in ("xmmintrin.h", "emmintrin.h", "pmmintrin.h", "immintrin.h", "mmintrin.h",
                 "intrin.h", "x86intrin.h"):
        (standins / name).write_text(vector)
    (standins / "wasm_prelude.h").write_text(
        "#pragma once\nextern \"C\" unsigned long __readfsdword(unsigned long);\n")
    return standins


SUPPORTED_HOSTS = {
    ("Linux", "x86-64"), ("Linux", "aarch64"),
    ("Darwin", "x86-64"), ("Darwin", "aarch64"),
    ("Windows", "x86-64"), ("Windows", "aarch64"),
}


def detect_host(system=None, machine=None):
    """Describe the machine running the build, not the 32-bit guest it targets."""
    system = system or platform.system()
    machine = machine or platform.machine()
    arch = machine.lower()
    if arch in ("x86_64", "amd64", "x64"):
        arch = "x86-64"
    elif arch in ("aarch64", "arm64"):
        arch = "aarch64"
    if (system, arch) not in SUPPORTED_HOSTS:
        raise ValueError("unsupported backend host: %s %s" % (system, machine))
    host = {
        "system": system,
        "machine": arch,
        "slug": "%s-%s" % (system.lower(), arch),
        "library_name": ("fnv_decomp.dll" if system == "Windows" else
                         "libfnv_decomp.dylib" if system == "Darwin" else "libfnv_decomp.so"),
        "native_compiler": "clang" if system == "Windows" else "cc",
        "native_cxx": "clang++" if system == "Windows" else "c++",
        "native_flags": ["-O2", "-fPIC", "-dynamiclib" if system == "Darwin" else "-shared"]
                        if system != "Windows" else ["-O2", "-shared"],
        "native_libs": [] if system == "Windows" else ["-lm"],
    }
    return host


def guest_memory_source(host):
    """C that reserves the guest mapping and its guard pages on this host."""
    if host["system"] == "Windows":
        return """#include <windows.h>
static int decomp_reserve(unsigned char** guest, uint64_t size) {
  void* p = VirtualAlloc(NULL, (size_t)size * 2, MEM_RESERVE, PAGE_NOACCESS);
  if (!p) return -1;
  if (!VirtualAlloc(p, (size_t)size, MEM_COMMIT, PAGE_READWRITE)) return -2;
  *guest = (unsigned char*)p;
  return 0;
}
"""
    return """#include <sys/mman.h>
#ifndef MAP_ANONYMOUS
#define MAP_ANONYMOUS MAP_ANON
#endif
#ifndef MAP_NORESERVE
#define MAP_NORESERVE 0
#endif
static int decomp_reserve(unsigned char** guest, uint64_t size) {
  void* p = mmap(NULL, (size_t)size * 2, PROT_NONE,
                 MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
  if (p == MAP_FAILED) return -1;
  if (mprotect(p, (size_t)size, PROT_READ | PROT_WRITE) != 0) return -2;
  *guest = (unsigned char*)p;
  return 0;
}
"""


def toolchain_cache_dir(root=None):
    """Per-user cache for the fetched clang and wabt, outside the repository."""
    if os.environ.get("NV_TOOLCHAIN_CACHE"):
        return Path(os.environ["NV_TOOLCHAIN_CACHE"])
    if root is None:
        if os.name == "nt" and os.environ.get("LOCALAPPDATA"):
            root = Path(os.environ["LOCALAPPDATA"]) / "nv-rs" / "cache"
        else:
            root = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "nv-rs"
    return Path(root) / "backend" / "toolchain"


def toolchain_paths(cache, host):
    """Locations of one host's toolchain inside the cache."""
    target = Path(cache) / ("%s-%s" % (host["slug"], TOOLCHAIN_VERSION))
    suffix = ".exe" if host["system"] == "Windows" else ""
    binary = lambda name: str(target / "bin" / (name + suffix))
    return target, {
        "clang": binary("clang"),
        "wasm_ld": binary("wasm-ld"),
        "wasm2c": binary("wasm2c"),
        "wabt_include": str(target / "include"),
        "wabt_runtime": str(target / "share" / "wabt" / "wasm2c"),
        "wabt_share": str(target),
    }


def cached_toolchain(cache, host):
    """Return the cached tools, or None when this host has no complete copy."""
    target, paths = toolchain_paths(cache, host)
    stamp = target / "stamp.json"
    if not stamp.is_file():
        return None
    try:
        record = json.loads(stamp.read_text())
    except (OSError, json.JSONDecodeError):
        return None
    if record.get("version") != TOOLCHAIN_VERSION or record.get("host") != host["slug"]:
        return None
    needed = [paths[key] for key in REQUIRED_TOOLS] + [
        str(Path(paths["wabt_runtime"]) / name)
        for name in ("wasm-rt-impl.c", "wasm-rt-mem-impl.c")]
    if not all(Path(path).is_file() for path in needed):
        return None
    return paths


def download_archive(url, path):
    with urllib.request.urlopen(url, timeout=30) as response, open(path, "wb") as archive:
        shutil.copyfileobj(response, archive)


def blocked_download(url, path):
    raise ToolchainUnavailable("toolchain fetching is disabled")


def fetch_toolchain(cache, host, download=None):
    """Fetch and unpack clang and wabt once, then stamp the cache as complete."""
    cached = cached_toolchain(cache, host)
    if cached:
        return cached
    target, paths = toolchain_paths(cache, host)
    target.mkdir(parents=True, exist_ok=True)
    fetch = download or download_archive
    base = os.environ.get("NV_TOOLCHAIN_URL", TOOLCHAIN_URL)
    for asset in TOOLCHAIN_ASSETS:
        url = "%s/%s-%s.zip" % (base, asset, host["slug"])
        archive = Path(cache) / ("%s-%s.zip" % (asset, host["slug"]))
        try:
            fetch(url, archive)
            shutil.unpack_archive(str(archive), str(target))
        except (OSError, ValueError, shutil.ReadError) as exc:
            raise ToolchainUnavailable("could not fetch %s: %s" % (asset, exc)) from exc
        finally:
            archive.unlink(missing_ok=True)
    for key in REQUIRED_TOOLS:
        binary = Path(paths[key])
        if binary.is_file() and host["system"] != "Windows":
            binary.chmod(binary.stat().st_mode | 0o111)
    needed = [paths[key] for key in REQUIRED_TOOLS] + [
        str(Path(paths["wabt_runtime"]) / name)
        for name in ("wasm-rt-impl.c", "wasm-rt-mem-impl.c")]
    if not all(Path(path).is_file() for path in needed):
        raise ToolchainUnavailable("fetched toolchain is incomplete for %s" % host["slug"])
    (target / "stamp.json").write_text(json.dumps(
        {"version": TOOLCHAIN_VERSION, "host": host["slug"]}) + "\n")
    return paths


def ensure_toolchain(cache, host, download=None):
    """Return cached tool paths and whether they had to be fetched."""
    paths = cached_toolchain(cache, host)
    if paths:
        return {**paths, "fetched": False}
    return {**fetch_toolchain(cache, host, download), "fetched": True}


def find_toolchain(cache_dir=None, download=None, host=None):
    """Find the toolchain: overrides, then the cache, then PATH, then a first fetch."""
    host = host or detect_host()
    tools = {"host": host, "wabt_include": None, "wabt_runtime": None}
    for key, env, name in (
        ("clang", "NV_CLANG", "clang"),
        ("wasm_ld", "NV_WASM_LD", "wasm-ld"),
        ("wasm2c", "NV_WASM2C", "wasm2c"),
        ("cc", "NV_CC", host["native_compiler"]),
        ("cxx", "NV_CXX", host["native_cxx"]),
        ("wabt_share", "NV_WABT_SHARE", None),
    ):
        tools[key] = os.environ.get(env) or (shutil.which(name) if name else None)
    if not all(tools[key] for key in REQUIRED_TOOLS):
        cache = Path(cache_dir) if cache_dir is not None else toolchain_cache_dir()
        cached = cached_toolchain(cache, host)
        if cached is None:
            try:
                cached = fetch_toolchain(cache, host, download)
            except ToolchainUnavailable as exc:
                tools["toolchain_error"] = str(exc)
        if cached:
            for key in (*REQUIRED_TOOLS, "wabt_include", "wabt_runtime", "wabt_share"):
                tools[key] = tools[key] or cached[key]
    if tools["clang"]:
        try:
            tools["resource"] = run([tools["clang"], "-print-resource-dir"]).stdout.strip()
        except OSError:
            tools["resource"] = ""
    else:
        tools["resource"] = ""
    wasm2c = tools["wasm2c"]
    if wasm2c and Path(wasm2c).is_file() and not tools["wabt_share"]:
        tools["wabt_share"] = str(Path(wasm2c).resolve().parent.parent)
    if tools["wabt_share"]:
        share = Path(tools["wabt_share"])
        tools["wabt_include"] = tools["wabt_include"] or str(share / "include")
        tools["wabt_runtime"] = tools["wabt_runtime"] or str(share / "share" / "wabt" / "wasm2c")
    import glob
    for key, pattern, env in (
        ("mingw", "/nix/store/*mingw_w64-headers*/include", "NV_MINGW_INCLUDE"),
        ("libcxx", "/nix/store/*libcxx*dev*/include/c++/v1", "NV_LIBCXX_INCLUDE"),
    ):
        tools[key] = os.environ.get(env) or next(iter(sorted(glob.glob(pattern))), None)
    return tools


def toolchain_ready(tools):
    return all(tools.get(key) for key in REQUIRED_TOOLS)


def wasm_runtime_ready(tools):
    runtime = tools.get("wabt_runtime")
    if not runtime:
        return False
    return all((Path(runtime) / name).is_file()
               for name in ("wasm-rt-impl.c", "wasm-rt-mem-impl.c"))


def run(command, **kwargs):
    return subprocess.run([str(c) for c in command], capture_output=True, text=True,
                          errors="replace", **kwargs)


def wabt_tool(tools, name):
    """Find a sibling of wasm2c in the same wabt install."""
    override = os.environ.get("NV_WABT_" + name.upper().replace("2", "_").replace("-", "_"))
    if override:
        return override
    wasm2c = tools.get("wasm2c")
    sibling = Path(wasm2c).with_name(name) if wasm2c else None
    return str(sibling) if sibling and sibling.is_file() else None


def rewrite_addresses(wat, addresses):
    """Bind addresses taken in the module to the original addresses.

    wasm-ld lays the functions whose address is taken into one element segment
    at ``__table_base``. Taking ``&foo`` then compiles to ``__table_base + i``,
    where ``i`` is the function's position in that segment. The WAT is rewritten
    before wasm2c so that the value is the symbol's original address instead.
    The host table is already indexed by original address, so a call through
    the stored value then needs no second mapping.

    ``addresses`` maps an exported symbol name to its original address. Returns
    the rewritten WAT and the number of addresses bound.
    """
    names = {func: name for name, func in WAT_EXPORT_RE.findall(wat)}
    by_index = {}
    for _, offset, functions in ELEM_RE.findall(wat):
        if "__table_base" not in offset:
            continue
        for index, func in enumerate(functions.split()):
            name = names.get(func.lstrip("$"), func.lstrip("$"))
            address = addresses.get(name)
            if address is None:
                continue
            by_index[index] = int(address, 0) if isinstance(address, str) else int(address)
    if not by_index:
        return wat, 0
    base_locals = {int(match.group(1)) for match in TABLE_BASE_LOCAL_RE.finditer(wat)}
    if not base_locals:
        return wat, 0
    lines = wat.splitlines(keepends=True)
    out, count, index = [], 0, 0
    while index < len(lines):
        match = re.fullmatch(r"\s*local\.get\s+(\d+)\s*\n", lines[index])
        if match and int(match.group(1)) in base_locals and index + 2 < len(lines):
            constant = re.fullmatch(r"\s*i32\.const\s+(\d+)\s*\n", lines[index + 1])
            add = re.fullmatch(r"\s*i32\.add\s*\n", lines[index + 2])
            if constant and add and int(constant.group(1)) in by_index:
                indent = lines[index][:len(lines[index]) - len(lines[index].lstrip())]
                out.append("%si32.const %d\n" % (indent, by_index[int(constant.group(1))]))
                index += 3
                count += 1
                continue
        out.append(lines[index])
        index += 1
    return "".join(out), count


def wasm_flags(copy, standins, tools, cxx=False):
    flags = ["--target=wasm32-unknown-unknown", "-mms-bitfields", "-fshort-wchar",
             "-fms-extensions", "-fms-compatibility", "-fms-compatibility-version=19",
             "-nostdlib", "-Wno-everything", "-O2", "-fPIC", "-flto", "-D_WIN32",
             "-DWIN32", "-D__i386__", "-D_M_IX86=600", "-D_X86_", "-D__GNUC__=4",
             "-D__GNUC_MINOR__=2", "-isystem", str(standins)]
    if tools.get("mingw"):
        flags += ["-isystem", tools["mingw"]]
    if cxx and tools.get("libcxx"):
        flags += ["-isystem", tools["libcxx"], "-D_LIBCPP_HAS_NO_THREADS",
                  "-include", str(standins / "wasm_prelude.h")]
    flags += ["-I", str(copy / "include"), "-I", str(copy / "src")]
    if tools.get("resource"):
        flags += ["-resource-dir", tools["resource"]]
    return flags


def compile_unit(tools, copy, standins, source, obj, cxx=False):
    path = copy / source
    if not path.is_file():
        return False, "source not found in build copy"
    compiler = tools["cxx"] if cxx else tools["clang"]
    result = run([compiler, *wasm_flags(copy, standins, tools, cxx), "-c", path, "-o", obj])
    if result.returncode:
        for line in result.stderr.splitlines():
            if "error:" in line:
                return False, line.split("error:", 1)[1].strip()[:240]
        return False, (result.stderr.strip().splitlines() or ["clang failed"])[-1][:240]
    return True, ""


def declaration(declaration_text):
    """Split a wasm2c declaration into its name, parameter types and result type."""
    match = re.search(r"(\w+)\s*\(([^)]*)\)\s*;?", declaration_text)
    if not match:
        return "", [], ""
    name = match.group(1)
    params = [p.strip() for p in match.group(2).split(",") if p.strip() and p.strip() != "void"]
    params = params[1:] if params and re.match(r"(?:struct\s+)?(?:w2c_mod|w2c_env)\s*\*", params[0]) else params
    result = declaration_text.split(name, 1)[0].strip()
    return name, params, result


def parse_header(text):
    """Read the wasm2c header's imports and exports."""
    imports, exports = [], []
    for module, field, decl in IMPORT_RE.findall(text):
        name, params, result = declaration(decl)
        member = re.search(r"(\w+)\s*(?:\(|;|\*?\s*$)", decl)
        name = name or (member.group(1) if member else "")
        imports.append({"module": module, "field": field, "decl": decl.strip(),
                        "name": name, "params": params, "result": result,
                        "function": "(" in decl})
    for field, decl in EXPORT_RE.findall(text):
        name, params, result = declaration(decl)
        if name.startswith("w2c_mod_"):
            exports.append({"name": field, "cname": name, "params": params, "result": result})
    return imports, exports


def export_manifest(exports, units):
    """Attach each export's original address from the decomp's unit table."""
    symbols = {}
    for unit in units:
        for function in unit.get("functions", []):
            symbol = function.get("symbol", "")
            symbols.setdefault(symbol.lstrip("_"), function.get("address"))
            symbols.setdefault(symbol, function.get("address"))
    result = []
    for index, entry in enumerate(exports):
        va = symbols.get(entry["name"]) or symbols.get("_" + entry["name"])
        arity = len(entry["params"])
        result.append({**entry, "address": va, "wrapper": "decomp_call_%d" % index,
                       "arity": arity if arity <= 5 and all(p in {"u32", "f32", "int", "unsigned int"}
                                                            for p in entry["params"]) else None})
    return result


def exports_from_units(units):
    """Derive exports from the unit table when there is no wasm2c header to read."""
    exports, skipped = [], []
    for unit in units:
        for function in unit.get("functions", []):
            symbol = function.get("symbol", "")
            name = symbol.lstrip("_?")
            if "params" not in function and "arity" not in function:
                skipped.append({"source": unit.get("source", ""),
                                "reason": "no signature for the native path"})
                continue
            params = function.get("params")
            if params is None:
                params = ["u32"] * int(function.get("arity", 0))
            if not re.fullmatch(r"[A-Za-z_]\w*", name):
                skipped.append({"source": unit.get("source", ""),
                                "reason": "no signature for the native path"})
                continue
            exports.append({"source": unit.get("source", ""), "name": name, "cname": symbol,
                            "params": params, "result": function.get("result", "u32"),
                            "address": function.get("address"), "arity": len(params)})
    for index, entry in enumerate(exports):
        entry["wrapper"] = "decomp_call_%d" % index
    return exports, skipped


def generate_shim(path, imports, exports, globals_, instantiate, host=None,
                  func_globals=None, func_values=None):
    """Write the host side: guest memory, original-address table, GOT and import stubs."""
    host = host or detect_host()
    func_globals = func_globals or []
    func_values = func_values or {}
    lines = ['#include <stdint.h>', '#include <stdlib.h>', '#include <string.h>']
    lines += guest_memory_source(host).splitlines()
    lines += ['#include "mod.h"', '#include "wasm-rt-impl.h"', '',
             'struct w2c_env { wasm_rt_memory_t mem; wasm_rt_funcref_table_t table;',
             '  u32 sp, membase, tablebase; };',
             'struct w2c_GOT0x2Emem { %s };' % " ".join("u32 f%d;" % i for i in range(len(globals_))),
             'struct w2c_GOT0x2Efunc { %s };' % " ".join("u32 f%d;" % i for i in range(len(func_globals))),
             'static struct w2c_env env; static w2c_mod inst; static struct w2c_GOT0x2Emem got;',
             'static struct w2c_GOT0x2Efunc fgot; static unsigned char* guest_base;',
             'static uint32_t decomp_trap_code; static const char* decomp_trap_reason;', '']
    for index, name in enumerate(globals_):
        lines.append('u32* w2c_GOT0x2Emem_%s(struct w2c_GOT0x2Emem* g) { return &g->f%d; }'
                     % (name.replace(".", "_"), index))
    for index, name in enumerate(func_globals):
        lines.append('u32* w2c_GOT0x2Efunc_%s(struct w2c_GOT0x2Efunc* g) { return &g->f%d; }'
                     % (name.replace(".", "_"), index))
    lines += [
        'static uint32_t (*import_hook)(const char*, uint32_t*, uint32_t);',
        'void decomp_set_import_hook(uint32_t (*hook)(const char*, uint32_t*, uint32_t)) { import_hook = hook; }',
        'static uint32_t decomp_import(const char* name, uint32_t* args, uint32_t n) {',
        '  return import_hook ? import_hook(name, args, n) : 0; }', '']
    for item in imports:
        if not item["function"]:
            name = item["name"]
            if item["module"] in ("GOT.mem", "GOT.func"):
                continue
            if "memory_base" in name:
                target = "&e->membase"
            elif "table_base" in name:
                target = "&e->tablebase"
            elif "stack_pointer" in name:
                target = "&e->sp"
            elif "indirect_function_table" in name:
                target = "&e->table"
            else:
                target = "&e->mem"
            ret = "wasm_rt_funcref_table_t*" if "indirect_function_table" in name else (
                "wasm_rt_memory_t*" if target == "&e->mem" else "u32*")
            lines.append('%s %s(struct w2c_env* e) { return %s; }' % (ret, name, target))
            continue
        args = ", ".join("u32 a%d" % i for i in range(len(item["params"])))
        array = "{" + ", ".join("a%d" % i for i in range(len(item["params"]))) + "}" if item["params"] else "NULL"
        lines.append('%s(struct w2c_env* e, %s) { (void)e; u32 a[] = %s; return decomp_import("%s", a, %d); }'
                     % (item["decl"].split("(")[0].strip(), args, array, item["field"], len(item["params"])))
    lines += ['', 'int decomp_init(unsigned char* guest, uint64_t size, uint32_t entries,',
              '                 uint32_t* globals, uint32_t nglobals) {',
              '  wasm_rt_init();',
              '  if (!guest) { int rc = decomp_reserve(&guest, size); if (rc) return rc; }',
              '  guest_base = guest;',
              '  env.mem = (wasm_rt_memory_t){guest, guest + size, (u32)(size / 65536),',
              '    (u32)(size / 65536), (u32)(size / 65536), size, false};',
              '  env.table.data = calloc(entries, sizeof(wasm_rt_funcref_t));',
              '  env.table.size = env.table.max_size = entries;',
              '  env.sp = 0x7ff00000; env.membase = 0x30000000; env.tablebase = 0x00ff0000;']
    if globals_:
        lines.append('  for (uint32_t i = 0; i < nglobals && i < %d && globals; i++) (&got.f0)[i] = globals[i];'
                     % len(globals_))
    for index, name in enumerate(func_globals):
        lines.append('  fgot.f%d = %s;' % (index, func_values.get(name, "0")))
    instantiate_args = "&inst"
    if len(instantiate.split(",")) >= 4:
        instantiate_args += ", &fgot, &got, &env"
    elif len(instantiate.split(",")) == 3:
        instantiate_args += ", &fgot, &env" if func_globals else ", &got, &env"
    elif len(instantiate.split(",")) == 2:
        instantiate_args += ", &env"
    lines += ['  wasm2c_mod_instantiate(%s);' % instantiate_args,
              '  w2c_mod_0x5F_wasm_apply_data_relocs(&inst);',
              '  w2c_mod_0x5F_wasm_call_ctors(&inst);']
    for entry in exports:
        if entry["address"] and entry["arity"] is not None:
            args = "WASM_RT_I32, " * (entry["arity"] + 1)
            lines.append('  env.table.data[%s] = (wasm_rt_funcref_t){' % entry["address"])
            lines.append('    wasm2c_mod_get_func_type(%d, 1, %s), (wasm_rt_function_ptr_t)%s, {NULL}, &inst };'
                         % (entry["arity"], args.rstrip(", "), entry["cname"]))
    lines += ['  return 0; }',
              'unsigned char* decomp_memory_base(void) { return guest_base; }',
              'int decomp_table_set(uint32_t va, void* fn, void* instance, uint32_t params) {',
              '  wasm_rt_func_type_t t = wasm2c_mod_get_func_type(params, 1, WASM_RT_I32, WASM_RT_I32,',
              '                                                   WASM_RT_I32, WASM_RT_I32, WASM_RT_I32);',
              '  env.table.data[va] = (wasm_rt_funcref_t){t, (wasm_rt_function_ptr_t)fn, {NULL}, instance};',
              '  return 0; }',
              'uint32_t decomp_last_trap_code(void) { return decomp_trap_code; }',
              'const char* decomp_last_trap_reason(void) { return decomp_trap_reason; }',
              'void decomp_clear_trap(void) { decomp_trap_code = 0; decomp_trap_reason = NULL; }',
              'int decomp_table_has(uint32_t va) {',
              '  return va < env.table.size && env.table.data[va].func != NULL; }', '']
    for index, entry in enumerate(exports):
        if entry["arity"] is None:
            continue
        cast = lambda p: "((union {u32 u; f32 f;}){.u=%s}).f" % p if p == "f32" else p
        args = ", ".join("%s a%d" % (p, i) for i, p in enumerate(entry["params"]))
        values = ", ".join(cast("a%d" % i) for i in range(len(entry["params"])))
        call = "%s(&inst%s%s)" % (entry["cname"], ", " if values else "", values)
        result = (call + "; return 0;" if entry["result"] == "void" else
                  "return ((union {u32 u; f32 f;}){.f=(f32)(%s)}).u;" % call if entry["result"] == "f32"
                  else "return (u32)(%s);" % call)
        lines += ['u32 decomp_call_%d(%s) {' % (index, args),
                  '  wasm_rt_trap_t trap = WASM_RT_SETJMP(g_wasm_rt_jmp_buf);',
                  '  if (trap != WASM_RT_TRAP_NONE) { decomp_trap_code = trap;',
                  '    decomp_trap_reason = wasm_rt_strerror(trap); return 0; }',
                  '  decomp_trap_code = 0; decomp_trap_reason = NULL;',
                  '  %s }' % result]
    Path(path).write_text("\n".join(lines) + "\n")


def fallback_shim(path, exports, host=None):
    """Write the host side of the native path with the same public ABI as the wasm one."""
    host = host or detect_host()
    lines = ['#include <stdint.h>', '#include <stdlib.h>', '#include <string.h>']
    lines += guest_memory_source(host).splitlines()
    lines += [
        'typedef uint32_t u32; typedef float f32;',
        'static unsigned char* guest_base;',
        'static void** table; static uint32_t table_size;',
        'static uint32_t (*import_hook)(const char*, u32*, u32);',
        'void decomp_set_import_hook(uint32_t (*hook)(const char*, u32*, u32)) { import_hook = hook; }',
        'static uint32_t decomp_import(const char* name, u32* args, u32 n) {',
        '  (void)name; (void)args; (void)n; return import_hook ? import_hook(name, args, n) : 0; }',
        '',
        'int decomp_init(unsigned char* guest, uint64_t size, uint32_t entries,',
        '                uint32_t* globals, uint32_t nglobals) {',
        '  (void)globals; (void)nglobals;',
        '  if (!guest) { int rc = decomp_reserve(&guest, size); if (rc) return rc; }',
        '  guest_base = guest;',
        '  table = calloc(entries, sizeof(void*)); table_size = entries;',
        '  return 0; }',
        'unsigned char* decomp_memory_base(void) { return guest_base; }',
        'int decomp_table_set(uint32_t va, void* fn, void* instance, uint32_t params) {',
        '  (void)instance; (void)params;',
        '  if (!table || va >= table_size) return -1;',
        '  table[va] = fn; return 0; }',
        'static uint32_t decomp_trap_code; static const char* decomp_trap_reason;',
        'uint32_t decomp_last_trap_code(void) { return decomp_trap_code; }',
        'const char* decomp_last_trap_reason(void) { return decomp_trap_reason; }',
        'void decomp_clear_trap(void) { decomp_trap_code = 0; decomp_trap_reason = NULL; }',
        'int decomp_table_has(uint32_t va) {',
        '  return table && va < table_size && table[va] != NULL; }',
        '']
    for entry in exports:
        params = ", ".join("%s a%d" % (p, i) for i, p in enumerate(entry["params"]))
        values = ", ".join("((union {u32 u; f32 f;}){.u=a%d}).f" % i if p == "f32" else "a%d" % i
                           for i, p in enumerate(entry["params"]))
        lines.append("%s %s(%s);" % (entry["result"], entry["name"], params))
        call = "%s(%s)" % (entry["name"], values)
        body = ("%s; return 0;" % call if entry["result"] == "void" else
                "return ((union {u32 u; f32 f;}){.f=(f32)(%s)}).u;" % call
                if entry["result"] == "f32" else "return (u32)(%s);" % call)
        lines.append('u32 %s(%s) { %s }' % (entry["wrapper"], params, body))
    Path(path).write_text("\n".join(lines) + "\n")


def select_units(units, sources=None, limit=None):
    if sources:
        wanted = set(sources)
        return [u for u in units if u.get("source") in wanted]
    if limit is None:
        return units
    return units[:limit]


def make_report(units, outcomes, library, exports=None, globals_=None, rewrites=0):
    compiled, skipped = [], []
    for unit, outcome in zip(units, outcomes):
        address = (unit.get("functions") or [{}])[0].get("address")
        if outcome["ok"]:
            compiled.append({"source": unit["source"], "address": address, "object": outcome.get("object")})
        else:
            skipped.append({"source": unit["source"], "address": address, "reason": outcome["reason"]})
    return {"library": str(library), "compiled": compiled, "skipped": skipped,
            "counts": {"compiled": len(compiled), "skipped": len(skipped)},
            "exports": exports or [], "globals": globals_ or [], "rewritten_files": rewrites}


def write_build_outputs(out, report, exports, globals_, global_values):
    """Write the report and manifest shared by both build paths."""
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (out / "report.md").write_text(render_report(report))
    (out / "manifest.json").write_text(json.dumps(
        {"library": str(report["library"]), "exports": exports, "globals": globals_,
         "global_values": global_values, "table_entries": TABLE_ENTRIES}, indent=2) + "\n")


def build(decomp, out, units=None, limit=None, toolchain=None, download=None,
          compiler=None, allow_fallback=True, force_fallback=False):
    """Build a native library and write report.json/report.md into the output folder."""
    decomp, out = Path(decomp), Path(out)
    out.mkdir(parents=True, exist_ok=True)
    tools = toolchain if toolchain is not None else find_toolchain(download=download)
    host = tools.get("host") or detect_host()
    if force_fallback or (allow_fallback and not (toolchain_ready(tools) and wasm_runtime_ready(tools))):
        return build_cranelift(decomp, out, units, limit, host=host,
                               toolchain=tools, compiler=compiler)
    manifest = json.loads((decomp / "build" / "units.json").read_text())
    all_units = manifest.get("units", [])
    chosen = select_units(all_units, units, limit)
    rewrites = prepare_build_copy(decomp, out)
    standins = make_standins(out, tools)
    objects, outcomes = [], []
    for index, unit in enumerate(chosen):
        obj = out / ("unit_%04d.o" % index)
        ok, reason = compile_unit(tools, out / "copy", standins, unit["source"], obj,
                                  cxx=unit["source"].endswith((".cpp", ".cc", ".cxx")))
        outcomes.append({"ok": ok, "reason": reason, "object": obj.name})
        if ok:
            objects.append(obj)
    if not objects:
        raise RuntimeError("no unit compiled; nothing to link")
    wasm = out / "mod.wasm"
    linked = run([tools["wasm_ld"], "--experimental-pic", "-shared", "--unresolved-symbols=import-dynamic",
                  "--export-all", "--import-memory", "--max-memory=%d" % GUEST_SIZE, "--lto-O2",
                  "-o", wasm, *objects])
    if linked.returncode:
        raise RuntimeError("wasm-ld: " + linked.stderr.strip()[-500:])
    generated = out / "mod.c"
    converted = run([tools["wasm2c"], wasm, "-n", "mod", "-o", generated])
    if converted.returncode:
        raise RuntimeError("wasm2c: " + converted.stderr.strip()[-500:])
    header = (out / "mod.h").read_text()
    imports, exports = parse_header(header)
    addresses = {}
    for unit in chosen:
        for function in unit.get("functions", []):
            symbol = function.get("symbol", "").lstrip("_?")
            if function.get("address"):
                addresses[symbol] = function["address"]
    wat2wat, wat2wasm = wabt_tool(tools, "wasm2wat"), wabt_tool(tools, "wat2wasm")
    if not wat2wat or not wat2wasm:
        raise RuntimeError("wabt needs wasm2wat and wat2wasm to bind function addresses")
    text = run([wat2wat, wasm]).stdout
    text, bound = rewrite_addresses(text, addresses)
    addressed = out / "addressed.wat"
    addressed.write_text(text)
    rebuilt = run([wat2wasm, addressed, "-o", wasm])
    if rebuilt.returncode:
        raise RuntimeError("wat2wasm: " + rebuilt.stderr.strip()[-500:])
    generated = out / "mod.c"
    converted = run([tools["wasm2c"], wasm, "-n", "mod", "-o", generated])
    if converted.returncode:
        raise RuntimeError("wasm2c: " + converted.stderr.strip()[-500:])
    globals_, func_globals = [], []
    for item in imports:
        match = re.search(r"w2c_GOT0x2E(mem|func)_(\w+)", item["name"])
        if item["module"] == "GOT.mem" and match:
            globals_.append(match.group(2))
        elif item["module"] == "GOT.func" and match:
            func_globals.append(match.group(2))
    exports = export_manifest(exports, chosen)
    symbols = {}
    for unit in all_units:
        for name, address in unit.get("symbols", {}).items():
            plain = name.lstrip("?_").split("@@", 1)[0]
            symbols.setdefault(plain, address)
            symbols.setdefault(name.lstrip("?"), address)
    global_values = [symbols.get(name, symbols.get(name.upper(), "0x0")) for name in globals_]
    func_values = {name: addresses.get(name, symbols.get(name, "0x0")) for name in func_globals}
    shim = out / "shim.c"
    instantiate = re.search(r"void wasm2c_mod_instantiate\(([^;]+)\);", header).group(1)
    generate_shim(shim, imports, exports, globals_, instantiate, host=host,
                  func_globals=func_globals, func_values=func_values)
    library = out / host["library_name"]
    runtime_dir = tools["wabt_runtime"]
    includes = []
    for path in (tools.get("wabt_include"), runtime_dir, str(out)):
        if path:
            includes += ["-I", path]
    runtime = [str(Path(runtime_dir) / name) for name in ("wasm-rt-impl.c", "wasm-rt-mem-impl.c")]
    compiled = run([tools["cc"], *host["native_flags"], *includes, generated, shim,
                    *runtime, "-o", library, *host["native_libs"]])
    if compiled.returncode or not library.is_file():
        raise RuntimeError("native compiler: " + compiled.stderr.strip()[-500:])
    report = make_report(chosen, outcomes, library, exports,
                         dict(zip(globals_, global_values)), rewrites)
    report["bound_function_addresses"] = bound
    report["got_func_values"] = func_values
    write_build_outputs(out, report, exports, globals_, global_values)
    return report


def build_cranelift(decomp, out, units=None, limit=None, host=None, compiler=None, toolchain=None):
    """Build the same module when clang and wasm2c are unavailable.

    The engine supplies a Cranelift-backed compiler through `compiler`; the default
    is the host C compiler. Both compile the same selected units and feed the shim
    the same exports, ABI, report and manifest as the wasm path.
    """
    decomp, out = Path(decomp), Path(out)
    out.mkdir(parents=True, exist_ok=True)
    tools = toolchain or {}
    host = host or tools.get("host") or detect_host()
    compile_ = compiler or run
    cc = tools.get("cc") or shutil.which(host["native_compiler"]) or host["native_compiler"]
    cxx = tools.get("cxx") or shutil.which(host["native_cxx"]) or host["native_cxx"]
    manifest = json.loads((decomp / "build" / "units.json").read_text())
    chosen = select_units(manifest.get("units", []), units, limit)
    rewrites = prepare_build_copy(decomp, out)
    exports, _ = exports_from_units(chosen)
    by_source = {}
    for entry in exports:
        by_source.setdefault(entry["source"], []).append(entry)
    copy = out / "copy"
    includes = ["-I", str(copy / "include"), "-I", str(copy / "src")]
    objects, outcomes, compiled_sources = [], [], set()
    for index, unit in enumerate(chosen):
        usable = by_source.get(unit["source"], [])
        if not usable:
            outcomes.append({"ok": False, "reason": "no signature for the native path"})
            continue
        source = copy / unit["source"]
        if not source.is_file():
            outcomes.append({"ok": False, "reason": "source not found in build copy"})
            continue
        binary = cxx if source.suffix in (".cpp", ".cc", ".cxx") else cc
        obj = out / ("native_%04d.o" % index)
        flags = [binary, "-O2"] + (["-fPIC"] if host["system"] != "Windows" else [])
        result = compile_([str(part) for part in (*flags, "-c", source, "-o", obj, *includes)])
        if result.returncode:
            lines = [line for line in result.stderr.splitlines() if "error:" in line]
            reason = (lines[-1] if lines else (result.stderr.strip().splitlines() or ["compiler failed"])[-1])
            outcomes.append({"ok": False, "reason": reason[:240]})
            continue
        outcomes.append({"ok": True, "object": obj.name})
        objects.append(obj)
        compiled_sources.add(unit["source"])
    if not objects:
        raise RuntimeError("no unit compiled; nothing to link")
    exports = [{key: value for key, value in entry.items() if key != "source"}
               for entry in exports if entry["source"] in compiled_sources]
    for index, entry in enumerate(exports):
        entry["wrapper"] = "decomp_call_%d" % index
    shim = out / "native_shim.c"
    fallback_shim(shim, exports, host=host)
    library = out / host["library_name"]
    link = [cc, *host["native_flags"], "-I", str(out), shim, *objects,
            "-o", library, *host["native_libs"]]
    result = compile_([str(part) for part in link])
    if result.returncode or not library.is_file():
        raise RuntimeError("native compiler: " + result.stderr.strip()[-500:])
    report = make_report(chosen, outcomes, library, exports, {}, rewrites)
    write_build_outputs(out, report, exports, [], [])
    return report


def render_report(report):
    lines = ["# fnv_decomp backend build", "",
             "Compiled %d units; skipped %d." % (report["counts"]["compiled"], report["counts"]["skipped"]),
             "", "## Compiled", ""]
    lines += ["- `%s` at %s" % (u["source"], u["address"]) for u in report["compiled"]] or ["(none)"]
    lines += ["", "## Skipped", ""]
    lines += ["- `%s`: %s" % (u["source"], u["reason"]) for u in report["skipped"]] or ["(none)"]
    lines += ["", "## Library", "", "`%s`" % report["library"]]
    return "\n".join(lines) + "\n"


def parse_layout_dump(text):
    """Read clang's record-layout dump into records of (depth, field, nth) -> offset.

    The two targets spell a few implicit rows differently (vtable pointers,
    anonymous members), so those are normalised the same way before comparing.
    """
    records = {}
    for block in text.split("*** Dumping AST Record Layout")[1:]:
        lines = [line for line in block.splitlines() if line.strip()]
        if not lines:
            continue
        header = re.match(r"\s*0\s*\|\s*(?:class|struct|union)\s+(.+?)(?: \(empty\))?\s*$", lines[0])
        if not header:
            continue
        name = header.group(1)
        fields, size, seen = {}, None, {}
        for line in lines[1:]:
            row = re.match(r"\s*(\d+)(?::(\d+)-(\d+))?\s*\|(\s*)(.*)$", line)
            if row:
                field = row.group(5).strip()
                if field.endswith("(empty)"):
                    continue
                field = re.sub(r"\(anonymous at [^)]*\)|\(unnamed[^)]*\)", "(anon)", field)
                field = field.replace("vftable pointer", "vptr").replace("vtable pointer", "vptr")
                field = field.replace("vbtable pointer", "vbptr")
                key = (len(row.group(4)), field)
                seen[key] = seen.get(key, 0) + 1
                fields[(*key, seen[key])] = int(row.group(1))
            found = re.search(r"sizeof=(\d+), (?:dsize=\d+, )?align=(\d+)", line)
            if found:
                size = (int(found.group(1)), int(found.group(2)))
                break
        records.setdefault(name, {"fields": fields, "size": size})
    return records


def compare_layouts(i686, wasm):
    """Compare every record both targets lay out; report name, field and both offsets."""
    left, right = parse_layout_dump(i686), parse_layout_dump(wasm)
    diffs = []
    for name in sorted(left.keys() & right.keys()):
        a, b = left[name], right[name]
        if a["size"] != b["size"]:
            diffs.append("%s: sizeof/align i686=%s wasm=%s" % (name, a["size"], b["size"]))
        for key in sorted(a["fields"].keys() & b["fields"].keys()):
            if key[1].startswith(("(", "class ", "struct ")):
                continue
            if a["fields"][key] != b["fields"][key]:
                diffs.append("%s.%s: i686=%d wasm=%d" % (name, key[1], a["fields"][key], b["fields"][key]))
    return diffs


def layout(decomp, out, units=None, limit=None, tools=None, download=None):
    """Dump both targets for every unit and fail when a shared record differs."""
    decomp, out = Path(decomp), Path(out)
    out.mkdir(parents=True, exist_ok=True)
    tools = tools or find_toolchain(download=download)
    if not toolchain_ready(tools):
        raise ToolchainUnavailable("clang is required for the layout check")
    manifest = json.loads((decomp / "build" / "units.json").read_text())
    chosen = select_units(manifest.get("units", []), units, limit)
    prepare_build_copy(decomp, out)
    standins = make_standins(out, tools)
    copy = out / "copy"
    msvc = ["--target=i686-pc-windows-msvc", "-fms-extensions", "-fms-compatibility",
            "-fms-compatibility-version=19", "-fdelayed-template-parsing", "-Wno-everything",
            "-fsyntax-only", "-Xclang", "-fdump-record-layouts", "-D_X86_",
            "-D__GNUC__=4", "-D__GNUC_MINOR__=2"]
    if tools.get("mingw"):
        msvc += ["-isystem", tools["mingw"]]
    if tools.get("libcxx"):
        msvc += ["-isystem", tools["libcxx"]]
    compared, skipped, diffs = [], [], []
    for unit in chosen:
        source = copy / unit["source"]
        if not source.is_file():
            skipped.append({"source": unit["source"], "reason": "source not found"})
            continue
        extra = ["-resource-dir", tools["resource"]] if tools.get("resource") else []
        left = run([tools["cxx"], *msvc, *extra, "-I", str(copy / "include"), source])
        right = run([tools["cxx"], *wasm_flags(copy, standins, tools, cxx=True),
                     "-fsyntax-only", "-Xclang", "-fdump-record-layouts", source])
        if left.returncode or right.returncode:
            reason = (left.stderr or right.stderr).strip().splitlines()
            skipped.append({"source": unit["source"], "reason": (reason[-1] if reason else "layout dump failed")[:240]})
            continue
        found = compare_layouts(left.stdout, right.stdout)
        compared.append({"source": unit["source"], "records": len(parse_layout_dump(left.stdout))})
        diffs += [("%s: %s" % (unit["source"], d)) for d in found]
    report = {"compared": compared, "skipped": skipped, "differences": diffs}
    (out / "layout.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def built_artefacts(paths):
    """List paths that are build output, never source that belongs in the tree."""
    result = []
    for path in paths:
        name = Path(path).name
        if Path(path).suffix in BANNED_SUFFIXES or name in GENERATED_NAMES:
            result.append(path)
    return result


def hygiene(root="."):
    root = Path(root)
    result = run(["git", "-C", str(root), "ls-files", "--cached", "--others", "--exclude-standard", "-z"])
    paths = [p for p in result.stdout.split("\0") if p]
    offenders = built_artefacts(paths)
    ignored = run(["git", "-C", str(root), "check-ignore", ".decomp-backend"])
    if ignored.returncode == 0:
        return offenders
    return offenders + [".decomp-backend is not git-ignored"]


def load_library(path):
    """Load a built module, or return None when no backend has been built."""
    if not path or not Path(path).is_file():
        return None
    return ctypes.CDLL(str(path))


def verify(library, manifest, vectors):
    """Call exports through the library and compare with the translations' results."""
    data = json.loads(Path(manifest).read_text())
    lib = load_library(library)
    if lib is None:
        return [{"reason": "backend is not built", "library": str(library)}]
    lib.decomp_init.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32,
                                ctypes.c_void_p, ctypes.c_uint32]
    lib.decomp_init(None, GUEST_SIZE, data["table_entries"], None, 0)
    exports = {e["name"]: e for e in data["exports"]}
    lib.decomp_memory_base.restype = ctypes.c_void_p
    base = lib.decomp_memory_base()
    trap_code = getattr(lib, "decomp_last_trap_code", None)
    trap_reason = getattr(lib, "decomp_last_trap_reason", None)
    clear_trap = getattr(lib, "decomp_clear_trap", None)
    if trap_code:
        trap_code.restype = ctypes.c_uint32
    if trap_reason:
        trap_reason.restype = ctypes.c_char_p
    raw = Path(vectors).read_text()
    try:
        cases = json.loads(raw)
    except json.JSONDecodeError:
        cases = [json.loads(line) for line in raw.splitlines() if line.strip()]
    mismatches = []
    for vector in cases:
        entry = exports[vector["export"]]
        if entry["arity"] is None:
            mismatches.append({"export": vector["export"], "reason": "no callable wrapper"})
            continue
        args = [int(a, 0) if isinstance(a, str) else int(a) for a in vector["args"]]
        if vector.get("input"):
            address = int(vector["input"], 0)
            ctypes.memmove(base + address, bytes.fromhex(vector["data"]), len(vector["data"]) // 2)
        wrapper = getattr(lib, entry["wrapper"])
        wrapper.argtypes = [ctypes.c_uint32] * entry["arity"]
        wrapper.restype = ctypes.c_uint32
        if clear_trap:
            clear_trap()
        got = wrapper(*args)
        want = int(vector["expected"], 0) & 0xffffffff
        code = trap_code() if trap_code else 0
        if code:
            reason = trap_reason().decode("utf-8", "replace") if trap_reason else "trap"
            mismatches.append({"export": vector["export"], "args": args,
                               "trap": code, "reason": reason, "expected": hex(want)})
        elif got != want:
            mismatches.append({"export": vector["export"], "args": args, "got": hex(got), "expected": hex(want)})
    return mismatches


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("build", "layout"):
        sub = commands.add_parser(name)
        sub.add_argument("--decomp", required=True, help="local fnv_decomp checkout")
        sub.add_argument("--unit", action="append", default=[], help="source path to build (repeatable)")
        sub.add_argument("--limit", type=int, help="build only the first N units")
        sub.add_argument("--out", default=".decomp-backend", help="git-ignored output folder")
        sub.add_argument("--offline", action="store_true", help="do not fetch a missing toolchain")
        if name == "build":
            sub.add_argument("--cranelift", action="store_true", help="force the native fallback build")
    verify_cmd = commands.add_parser("verify")
    verify_cmd.add_argument("--library", required=True)
    verify_cmd.add_argument("--manifest", required=True)
    verify_cmd.add_argument("--vectors", required=True)
    hygiene_cmd = commands.add_parser("hygiene")
    hygiene_cmd.add_argument("--root", default=".")
    args = parser.parse_args(argv)
    download = blocked_download if args.command in ("build", "layout") and args.offline else None
    if args.command == "build":
        report = build(args.decomp, args.out, args.unit, args.limit,
                       download=download, force_fallback=args.cranelift)
        print("compiled %d, skipped %d; library %s" % (
            report["counts"]["compiled"], report["counts"]["skipped"], report["library"]))
    elif args.command == "layout":
        report = layout(args.decomp, args.out, args.unit, args.limit, download=download)
        print("compared %d units, %d differences" % (len(report["compared"]), len(report["differences"])))
        for difference in report["differences"]:
            print(difference)
        return bool(report["differences"])
    elif args.command == "verify":
        mismatches = verify(args.library, args.manifest, args.vectors)
        for mismatch in mismatches:
            print(mismatch)
        return bool(mismatches)
    else:
        offenders = hygiene(args.root)
        for offender in offenders:
            print(offender)
        return bool(offenders)
    return 0


if __name__ == "__main__":
    sys.exit(main())
