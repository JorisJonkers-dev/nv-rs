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

Nothing this tool writes is added to the repository: the default output directory
is git-ignored. Stdlib only; Python 3.11 or newer.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

TABLE_ENTRIES = 0x01000000  # the guest's .text VAs all fit below 16 MiB
GUEST_SIZE = 1 << 32
BANNED_SUFFIXES = {".so", ".a", ".lib", ".o", ".obj", ".dylib", ".dll", ".exe", ".pdb", ".wasm", ".cwasm"}
GENERATED_NAMES = {"mod.c", "mod.h", "shim.c", "wasm-rt-impl.c", "wasm-rt-mem-impl.c",
                   "report.json", "report.md", "manifest.json", "layout.json"}

def wasm2c_module_name(module):
    """wasm2c's mangling of an import module name (GOT.mem -> GOT0x2Emem)."""
    return "".join(c if c.isalnum() or c == "_" else "0x%02X" % ord(c) for c in module)


GOT_MODULE = wasm2c_module_name("GOT.mem")

SIZE_T_OPS = re.compile(r"(operator\s+(?:new|delete)(?:\[\])?\s*\((?:[^(),]*, ?)?)unsigned int")
NAMED_ENUM = re.compile(r"\benum(\s+)(?!class\b|struct\b)([A-Za-z_]\w*)(\s*)([;{])")
IMPORT_RE = re.compile(r"/\* import: '([^']+)' '([^']+)' \*/\s*\n\s*([^\n;]+);")
EXPORT_RE = re.compile(r"/\* export: '([^']+)' \*/\s*\n\s*([^\n;]+);")


def rewrite_source(text):
    """Apply the build-copy-only rewrites: size_t/operator new width and MSVC enum width."""
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


def find_toolchain():
    """Find the local toolchain; callers may override any entry with NV_* variables."""
    def which(name, env):
        return os.environ.get(env) or shutil.which(name) or name

    tools = {
        "clang": which("clang", "NV_CLANG"),
        "wasm_ld": which("wasm-ld", "NV_WASM_LD"),
        "wasm2c": which("wasm2c", "NV_WASM2C"),
        "cc": which("cc", "NV_CC"),
        "cxx": which("clang++", "NV_CXX"),
    }
    try:
        tools["resource"] = subprocess.run([tools["clang"], "-print-resource-dir"],
                                           capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        tools["resource"] = ""
    for key, pattern, env in (
        ("mingw", "/nix/store/*mingw_w64-headers*/include", "NV_MINGW_INCLUDE"),
        ("libcxx", "/nix/store/*libcxx*dev*/include/c++/v1", "NV_LIBCXX_INCLUDE"),
    ):
        import glob
        found = os.environ.get(env) or next(iter(sorted(glob.glob(pattern))), "")
        tools[key] = found or None
    wasm2c = tools["wasm2c"]
    if Path(wasm2c).is_file():
        root = Path(wasm2c).resolve().parent.parent
        tools["wabt_share"] = os.environ.get("NV_WABT_SHARE") or str(root)
    else:
        tools["wabt_share"] = os.environ.get("NV_WABT_SHARE") or ""
    return tools


def run(command, **kwargs):
    return subprocess.run([str(c) for c in command], capture_output=True, text=True,
                          errors="replace", **kwargs)


def check_out_path(decomp, out):
    """Refuse an output folder inside the checkout or an unignored git work tree."""
    decomp, out = Path(decomp).resolve(), Path(out).resolve()
    if out == decomp or decomp in out.parents:
        raise ValueError("--out must not be inside the decomp checkout")
    parent = out if out.exists() else out.parent
    top = run(["git", "-C", str(parent), "rev-parse", "--show-toplevel"])
    if top.returncode:
        return
    root = Path(top.stdout.strip()).resolve()
    if out == root:
        raise ValueError("--out must not be the git work tree root")
    if root in out.parents:
        relative = out.relative_to(root).as_posix() + "/"
        ignored = run(["git", "-C", str(root), "check-ignore", "--no-index", "-q", "--", relative])
        if ignored.returncode:
            raise ValueError("--out must be an ignored folder inside a git work tree")


def wasm_type(spelling):
    """Use the runtime's function-type constant for a wasm2c parameter spelling."""
    return "WASM_RT_F32" if spelling.strip() == "f32" else "WASM_RT_I32"


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
        if isinstance(va, str):
            va = int(va, 0)
        if va is not None and not 0 <= va < TABLE_ENTRIES:
            raise ValueError("export %s: address %#x is outside the %#x-entry table"
                             % (entry["name"], va, TABLE_ENTRIES))
        arity = len(entry["params"])
        result.append({**entry, "address": va, "wrapper": "decomp_call_%d" % index,
                       "arity": arity if arity <= 5 and all(p in {"u32", "f32", "int", "unsigned int"}
                                                            for p in entry["params"]) else None})
    return result


def generate_shim(path, imports, exports, globals_, instantiate):
    """Write the host side: guest memory, original-address table, GOT.mem and import stubs."""
    lines = ['#include <stdint.h>', '#include <stdlib.h>', '#include <string.h>',
             '#include <sys/mman.h>', '#include "mod.h"', '',
             'struct w2c_env { wasm_rt_memory_t mem; wasm_rt_funcref_table_t table;',
             '  u32 sp, membase, tablebase; };',
             'struct w2c_%s { %s };' % (GOT_MODULE,
                                        " ".join("u32 f%d;" % i for i in range(len(globals_)))),
             'static struct w2c_env env; static w2c_mod inst; static struct w2c_%s got;' % GOT_MODULE,
             'static unsigned char* guest_base;', '']
    for index, name in enumerate(globals_):
        lines.append('u32* w2c_%s_%s(struct w2c_%s* g) { return &g->f%d; }'
                     % (GOT_MODULE, name.replace(".", "_"), GOT_MODULE, index))
    lines += [
        'static uint32_t (*import_hook)(const char*, uint32_t*, uint32_t);',
        'void decomp_set_import_hook(uint32_t (*hook)(const char*, uint32_t*, uint32_t)) { import_hook = hook; }',
        'static uint32_t decomp_import(const char* name, uint32_t* args, uint32_t n) {',
        '  return import_hook ? import_hook(name, args, n) : 0; }', '']
    for item in imports:
        if not item["function"]:
            name = item["name"]
            if item["module"] == "GOT.mem":
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
            # Use the type from mod.h's declaration, so the stub can never
            # disagree with the prototype the generated module calls through.
            ret = item["decl"].replace(name, "", 1).strip() or "u32*"
            lines.append('%s %s(struct w2c_env* e) { return (%s)%s; }'
                         % (ret, name, ret, target))
            continue
        args = ", ".join("u32 a%d" % i for i in range(len(item["params"])))
        array = "{" + ", ".join("a%d" % i for i in range(len(item["params"]))) + "}" if item["params"] else "NULL"
        lines.append('%s(struct w2c_env* e, %s) { (void)e; u32 a[] = %s; return decomp_import("%s", a, %d); }'
                     % (item["decl"].split("(")[0].strip(), args, array, item["field"], len(item["params"])))
    lines += ['', 'int decomp_init(unsigned char* guest, uint64_t size, uint32_t entries,',
              '                 uint32_t* globals, uint32_t nglobals) {',
              '  wasm_rt_init();',
              '  if (!guest) {',
              '    /* Reserve the guest range plus an unmapped guard half. */',
              '    guest = mmap(NULL, (size_t)size * 2, PROT_NONE,',
              '                 MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);',
              '    if (guest == MAP_FAILED) return -1;',
              '    if (mprotect(guest, (size_t)size, PROT_READ | PROT_WRITE) != 0) return -2;',
              '  }',
              '  guest_base = guest;',
              '  env.mem = (wasm_rt_memory_t){guest, guest + size, (u32)(size / 65536),',
              '    (u32)(size / 65536), (u32)(size / 65536), size, false};',
              '  env.table.data = calloc(entries, sizeof(wasm_rt_funcref_t));',
              '  env.table.size = env.table.max_size = entries;',
              '  env.sp = 0x7ff00000; env.membase = 0x30000000; env.tablebase = 0x00ff0000;']
    if globals_:
        lines.append('  for (uint32_t i = 0; i < nglobals && i < %d && globals; i++) (&got.f0)[i] = globals[i];'
                     % len(globals_))
    instantiate_args = "&inst"
    if len(instantiate.split(",")) >= 3:
        instantiate_args += ", &got, &env"
    elif len(instantiate.split(",")) == 2:
        instantiate_args += ", &env"
    lines += ['  wasm2c_mod_instantiate(%s);' % instantiate_args,
              '  w2c_mod_0x5F_wasm_apply_data_relocs(&inst);',
              '  w2c_mod_0x5F_wasm_call_ctors(&inst);']
    for entry in exports:
        if entry["address"] is not None and entry["arity"] is not None:
            types = ", ".join(wasm_type(p) for p in entry["params"] + [entry["result"]])
            lines.append('  env.table.data[%d] = (wasm_rt_funcref_t){' % entry["address"])
            lines.append('    wasm2c_mod_get_func_type(%d, 1, %s), (wasm_rt_function_ptr_t)%s, {NULL}, &inst };'
                         % (entry["arity"], types, entry["cname"]))
    lines += ['  return 0; }',
              'unsigned char* decomp_memory_base(void) { return guest_base; }',
              'int decomp_table_set(uint32_t va, void* fn, void* instance, uint32_t params) {',
              '  wasm_rt_func_type_t t = wasm2c_mod_get_func_type(params, 1, WASM_RT_I32, WASM_RT_I32,',
              '                                                   WASM_RT_I32, WASM_RT_I32, WASM_RT_I32);',
              '  env.table.data[va] = (wasm_rt_funcref_t){t, (wasm_rt_function_ptr_t)fn, {NULL}, instance};',
              '  return 0; }', '']
    for index, entry in enumerate(exports):
        if entry["arity"] is None:
            continue
        def cast(value, spelling):
            return "((union {u32 u; f32 f;}){.u=%s}).f" % value if spelling == "f32" else value

        # The ABI hands the host raw 32-bit values; cast f32 arguments inside
        # the wrapper rather than declaring a float parameter to ctypes.
        args = ", ".join("u32 a%d" % i for i in range(len(entry["params"])))
        values = ", ".join(cast("a%d" % i, p) for i, p in enumerate(entry["params"]))
        call = "%s(&inst%s%s)" % (entry["cname"], ", " if values else "", values)
        body = (call + "; return 0;") if entry["result"] == "void" else (
            "return ((union {u32 u; f32 f;}){.f=(f32)(%s)}).u;" % call if entry["result"] == "f32"
            else "return (u32)(%s);" % call)
        lines.append('u32 decomp_call_%d(%s) { %s }' % (index, args, body))
    Path(path).write_text("\n".join(lines) + "\n")


def select_units(units, sources=None, limit=None):
    if sources:
        wanted = set(sources)
        missing = sorted(wanted - {u.get("source") for u in units})
        if missing:
            raise ValueError("unit(s) not found in units.json: " + ", ".join(missing))
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


def link_wasm(tools, objects, wasm):
    """Link the compiled wasm objects into one shared module."""
    return run([tools["wasm_ld"], "--experimental-pic", "-shared", "--unresolved-symbols=import-dynamic",
                "--export-all", "--import-memory", "--max-memory=%d" % GUEST_SIZE, "--lto-O2",
                "-o", wasm, *objects])


def isolate_link_failures(tools, out, objects, outcomes, indices, linked):
    """Drop an object solely responsible for a link failure and skip its unit."""
    wasm, probe = out / "mod.wasm", out / "link-probe.wasm"
    if linked.returncode == 0:
        return objects, linked
    while linked.returncode and len(objects) > 1:
        failure = linked.stderr.strip()
        for position, candidate in enumerate(objects):
            trial = [o for o in objects if o != candidate]
            if link_wasm(tools, trial, probe).returncode == 0:
                outcomes[indices[position]] = {
                    "ok": False, "reason": "wasm-ld: " + failure[-240:], "object": candidate.name}
                objects = trial
                indices = indices[:position] + indices[position + 1:]
                linked = link_wasm(tools, objects, wasm)
                break
        else:
            break
    return objects, linked


def build(decomp, out, units=None, limit=None, toolchain=None):
    """Build a native library and write report.json/report.md into the output folder."""
    decomp, out = Path(decomp), Path(out)
    check_out_path(decomp, out)
    out.mkdir(parents=True, exist_ok=True)
    tools = toolchain or find_toolchain()
    manifest = json.loads((decomp / "build" / "units.json").read_text())
    all_units = manifest.get("units", [])
    chosen = select_units(all_units, units, limit)
    rewrites = prepare_build_copy(decomp, out)
    standins = make_standins(out, tools)
    objects, outcomes, indices = [], [], []
    for index, unit in enumerate(chosen):
        obj = out / ("unit_%04d.o" % index)
        ok, reason = compile_unit(tools, out / "copy", standins, unit["source"], obj,
                                  cxx=unit["source"].endswith((".cpp", ".cc", ".cxx")))
        outcomes.append({"ok": ok, "reason": reason, "object": obj.name})
        if ok:
            objects.append(obj)
            indices.append(index)
    if not objects:
        raise RuntimeError("no unit compiled; nothing to link")
    linked = link_wasm(tools, objects, out / "mod.wasm")
    objects, linked = isolate_link_failures(tools, out, objects, outcomes, indices, linked)
    if linked.returncode:
        raise RuntimeError("wasm-ld: " + linked.stderr.strip()[-500:])
    generated = out / "mod.c"
    converted = run([tools["wasm2c"], out / "mod.wasm", "-n", "mod", "-o", generated])
    if converted.returncode:
        raise RuntimeError("wasm2c: " + converted.stderr.strip()[-500:])
    header = (out / "mod.h").read_text()
    imports, exports = parse_header(header)
    globals_ = []
    for item in imports:
        if item["module"] == "GOT.mem":
            match = re.search(r"w2c_%s_(\w+)" % GOT_MODULE, item["name"])
            if match:
                globals_.append(match.group(1))
    exports = export_manifest(exports, chosen)
    symbols = {}
    for table in [manifest.get("symbols", {})] + [u.get("symbols", {}) for u in all_units]:
        for name, address in table.items():
            plain = name.lstrip("?_").split("@@", 1)[0]
            symbols.setdefault(plain, address)
            symbols.setdefault(name.lstrip("?"), address)
    def address_of(name):
        for key in (name, name.lstrip("?_"), name.upper()):
            if key in symbols:
                value = symbols[key]
                return int(value, 0) if isinstance(value, str) else int(value)
        return None

    global_values = [address_of(name) for name in globals_]
    unresolved = [name for name, value in zip(globals_, global_values) if value is None]
    if unresolved:
        raise RuntimeError("unresolved GOT.mem symbol(s): " + ", ".join(unresolved))
    shim = out / "shim.c"
    instantiate = re.search(r"void wasm2c_mod_instantiate\(([^;]+)\);", header).group(1)
    generate_shim(shim, imports, exports, globals_, instantiate)
    library = out / "libfnv_decomp.so"
    share = tools.get("wabt_share") or ""
    runtime = [str(Path(share) / "share" / "wabt" / "wasm2c" / n) for n in (
        "wasm-rt-impl.c", "wasm-rt-mem-impl.c")] if share else []
    compiled = run([tools["cc"], "-O2", "-fPIC", "-shared", "-I", str(Path(share) / "include") if share else share,
                    "-I", str(Path(share) / "share" / "wabt" / "wasm2c") if share else share,
                    "-I", out, generated, shim, *runtime, "-o", library, "-lm"])
    if compiled.returncode or not library.is_file():
        raise RuntimeError("native compiler: " + compiled.stderr.strip()[-500:])
    report = make_report(chosen, outcomes, library, exports,
                         dict(zip(globals_, global_values)), rewrites)
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (out / "report.md").write_text(render_report(report))
    (out / "manifest.json").write_text(json.dumps(
        {"library": str(library), "exports": exports, "globals": globals_,
         "global_values": global_values, "table_entries": TABLE_ENTRIES}, indent=2) + "\n")
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


def layout(decomp, out, units=None, limit=None, tools=None):
    """Dump both targets for every unit and fail when a shared record differs."""
    decomp, out = Path(decomp), Path(out)
    check_out_path(decomp, out)
    out.mkdir(parents=True, exist_ok=True)
    tools = tools or find_toolchain()
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
    # Include ignored files and files in untracked folders: a built artefact is
    # still in the tree when git was told not to show it.
    paths = []
    for extra in ([], ["--ignored"]):
        result = run(["git", "-C", str(root), "ls-files", "--cached", "--others", *extra,
                      "--exclude-standard", "-z"])
        paths += [p for p in result.stdout.split("\0") if p]
    # .decomp-backend is the sanctioned output folder and target/ is the
    # workspace's own build folder; neither is a stray artefact from this tool.
    sanctioned = {".git", ".decomp-backend", "target"}
    paths = [p for p in dict.fromkeys(paths) if Path(p).parts[0] not in sanctioned]
    offenders = built_artefacts(paths)
    ignored = run(["git", "-C", str(root), "check-ignore", "--no-index", "-q", "--", ".decomp-backend/"])
    if ignored.returncode == 0:
        return offenders
    return offenders + [".decomp-backend is not git-ignored"]


def verify(library, manifest, vectors):
    """Call exports through the library and compare with the translations' results."""
    data = json.loads(Path(manifest).read_text())
    lib = ctypes.CDLL(str(library))
    lib.decomp_init.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32,
                                ctypes.POINTER(ctypes.c_uint32), ctypes.c_uint32]
    values = [int(v, 0) if isinstance(v, str) else int(v) for v in data.get("global_values", [])]
    globals_array = (ctypes.c_uint32 * len(values))(*[v & 0xffffffff for v in values]) if values else None
    status = lib.decomp_init(None, GUEST_SIZE, data["table_entries"], globals_array, len(values))
    if status:
        raise RuntimeError("decomp_init failed: %d" % status)
    exports = {e["name"]: e for e in data["exports"]}
    lib.decomp_memory_base.restype = ctypes.c_void_p
    base = lib.decomp_memory_base()
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
        got = wrapper(*args)
        want = int(vector["expected"], 0) & 0xffffffff
        if got != want:
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
    verify_cmd = commands.add_parser("verify")
    verify_cmd.add_argument("--library", required=True)
    verify_cmd.add_argument("--manifest", required=True)
    verify_cmd.add_argument("--vectors", required=True)
    hygiene_cmd = commands.add_parser("hygiene")
    hygiene_cmd.add_argument("--root", default=".")
    args = parser.parse_args(argv)
    if args.command == "build":
        report = build(args.decomp, args.out, args.unit, args.limit)
        print("compiled %d, skipped %d; library %s" % (
            report["counts"]["compiled"], report["counts"]["skipped"], report["library"]))
    elif args.command == "layout":
        report = layout(args.decomp, args.out, args.unit, args.limit)
        print("compared %d units, %d differences" % (len(report["compared"]), len(report["differences"])))
        for difference in report["differences"]:
            print(difference)
        if not report["compared"]:
            print("no unit was compared; the layout check did not run")
            return 1
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
