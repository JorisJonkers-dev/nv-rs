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
cache when the archives are hash-pinned. When fetching fails, `build` reports why
and stops; `--cranelift` explicitly takes the native fallback, which compiles the
same units with the host C compiler and keeps the library's ABI, report and
manifest but not the wasm address-indexed table. `--offline` blocks fetching.

Nothing this tool writes is added to the repository: the default output directory
is git-ignored. Stdlib only; Python 3.11 or newer.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

TABLE_ENTRIES = 0x01000000  # the guest's .text VAs all fit below 16 MiB
GUEST_SIZE = 1 << 32
MAX_ARITY = 12  # widest export a wrapper is made for; `LibBackend::call` has the same ceiling
BANNED_SUFFIXES = {".so", ".a", ".lib", ".o", ".obj", ".dylib", ".dll", ".exe", ".pdb", ".wasm", ".cwasm"}
GENERATED_NAMES = {"mod.c", "mod.h", "wasm-rt-impl.c", "wasm-rt-mem-impl.c"}
TOOLCHAIN_VERSION = "v1"
TOOLCHAIN_URL = "https://github.com/JorisJonkers-dev/nv-rs-backend-toolchain/releases/download/v1"
TOOLCHAIN_ASSETS = ("clang", "wabt")
REQUIRED_TOOLS = ("clang", "wasm_ld", "wasm2c", "wasm2wat", "wat2wasm")


def toolchain_hashes():
    """SHA-256 pins for the fetched archives.

    No archive is unpacked without a pin: a release the launcher has not published
    yet must not put an unverified archive into the build's compiler path.
    """
    raw = os.environ.get("NV_TOOLCHAIN_SHA256", "")
    if raw:
        try:
            pins = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise ToolchainUnavailable("NV_TOOLCHAIN_SHA256 is not JSON: %s" % exc) from exc
        if isinstance(pins, dict):
            return {asset: pins.get(asset) for asset in TOOLCHAIN_ASSETS}
    return {asset: os.environ.get("NV_TOOLCHAIN_SHA256_" + asset.upper()) for asset in TOOLCHAIN_ASSETS}


class ToolchainUnavailable(RuntimeError):
    """The clang/wasm2c toolchain is neither installed nor fetchable on this host."""


SIZE_T_OPS = re.compile(r"(operator\s+(?:new|delete)(?:\[\])?\s*\((?:[^(),]*, ?)?)(?:unsigned int|unsigned(?!\s+(?:int|long|short|char|__int64)\b))")
SIZE_T_FIRST = re.compile(r"(operator\s+(?:new|delete)(?:\[\])?\s*\()(?:unsigned int|unsigned(?!\s+(?:int|long|short|char|__int64)\b))(?=\s+\w+\s*,)")
NAMED_ENUM = re.compile(r"\benum(\s+)(?!class\b|struct\b)([A-Za-z_]\w*)(\s*)([;{])")
IMPORT_RE = re.compile(r"/\* import: '([^']+)' '([^']+)' \*/\s*\n\s*([^\n;]+);")
EXPORT_RE = re.compile(r"/\* export: '([^']+)' \*/\s*\n\s*([^\n;]+);")
ELEM_RE = re.compile(r"\(elem\s+\(;(\d+);\)\s+\(([^)]*)\)\s+func\s+([^)]*)\)")
WAT_EXPORT_RE = re.compile(r'\(export\s+"([^"]+)"\s+\(func\s+\$?([\w.]+)\)\)')
TABLE_BASE_LOCAL_RE = re.compile(r"global\.get\s+\$__table_base\s*\n\s*local\.set\s+(\d+)")


# Declarations of MSVC intrinsics that clang already provides as macros or built-ins, and an
# `extern "C"` operator delete that clang's implicit C++ declaration contradicts.
INTRINSIC_DECLS = re.compile(
    r'^[ \t]*extern "C"[ \t]+(?:void\*?[ \t]+(?:__cdecl[ \t]+)?(?:_alloca\(unsigned int\)|_ReadWriteBarrier\((?:void)?\)));[ \t]*$',
    re.M)
EXTERN_C_DELETE = re.compile(r'extern "C"([ \t]+void[ \t]+(?:__cdecl[ \t]+)?operator[ \t]+delete)')
# `__if_not_exists` on a member of an incomplete template is an error in clang; the name it tests
# is never there, so the guarded include is the intent.
INCOMPLETE_MEMBER_TEST = re.compile(r"__if_not_exists\(TESDataHandlerArrayType::m_pBase\)")


# The decomp's non-MSVC `InterlockedIncrement`/`Decrement` definitions collide with the macros
# winnt.h makes of them; the macros stay and the definitions are renamed out of the way.
INTERLOCKED_DEF = re.compile(r"^(FNV_FORCEINLINE long )(Interlocked(?:Increment|Decrement))\(", re.M)
SIZE_T_TYPEDEF = re.compile(r"typedef unsigned int (\w*size_t);")


# A naked function is all assembly; wasm has no such attribute, and its body becomes the stub.
NAKED = re.compile(r"__declspec\(naked\)\s*")


CRITICAL_SECTION_CALL = re.compile(r"\b(?:Enter|Leave|TryEnter|Initialize|Delete)CriticalSection\(")


def cast_critical_sections(text):
    """Cast the argument of each `EnterCriticalSection(...)` and `LeaveCriticalSection(...)` call.

    The decomp passes an `int[6]`, a `void*` or its own struct; the Win32 prototype takes the
    struct pointer, which MSVC's own declaration (`void*`) hid. A declaration is left alone.
    """
    out, at = [], 0
    for match in CRITICAL_SECTION_CALL.finditer(text):
        if match.start() < at:
            continue
        line = text[text.rfind("\n", 0, match.start()) + 1:match.start()]
        if "__stdcall" in line or "WINAPI" in line:
            continue
        depth, end = 1, match.end()
        while end < len(text) and depth:
            depth += (text[end] == "(") - (text[end] == ")")
            end += 1
        if depth:
            continue
        out.append(text[at:match.end()])
        out.append("(struct _RTL_CRITICAL_SECTION*)(" + text[match.end():end - 1] + ")")
        out.append(")")
        at = end
    out.append(text[at:])
    return "".join(out)


def rewrite_source(text):
    """Apply the build-copy-only rewrites: CRT size_t spelling, MSVC enum width and clang gaps."""
    text = SIZE_T_OPS.sub(lambda m: m.group(1) + "__SIZE_TYPE__", text)
    # A placement form's first parameter is the size too (the pattern above stops at the second).
    text = SIZE_T_FIRST.sub(lambda m: m.group(1) + "__SIZE_TYPE__", text)
    text = INTRINSIC_DECLS.sub("", text)
    text = cast_critical_sections(text)
    text = NAKED.sub("", text)
    text = INTERLOCKED_DEF.sub(r"\1Fnv_\2(", text)
    text = SIZE_T_TYPEDEF.sub(r"typedef __SIZE_TYPE__ \1;", text)
    text = EXTERN_C_DELETE.sub(r"\1", text)
    text = INCOMPLETE_MEMBER_TEST.sub("__if_not_exists(TESDataHandlerArrayType_never_defined)", text)
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
            if path.suffix in {".c", ".cpp"} or "__asm" in rewritten or "_asm" in rewritten:
                rewritten, _ = rewrite_inline_asm(rewritten)
            if path.suffix in {".c", ".cpp"}:
                rewritten, _ = annotate_function_exports(rewritten)
            if rewritten != original:
                path.write_text(rewritten, errors="surrogateescape")
                changed += 1
    return changed


# x87 helpers the mingw math.h inlines with `fabs`, `fxam` and `fucom`; wasm has no x87 stack.
FXAM_HELPER = (
    "static __inline__ unsigned short __nv_fxam(long double v) {\n"
    "  unsigned short s = __builtin_signbit(v) ? 0x0200 : 0;\n"
    "  if (__builtin_isnan(v)) return s | 0x0100;\n"
    "  if (__builtin_isinf(v)) return s | 0x0500;\n"
    "  if (v == 0) return s | 0x4000;\n"
    "  return s | (__builtin_isnormal(v) ? 0x0400 : 0x4400);\n"
    "}\n"
    "static __inline__ unsigned short __nv_fucom(long double x, long double y) {\n"
    "  if (__builtin_isnan(x) || __builtin_isnan(y)) return 0x4500;\n"
    "  return x > y ? 0 : (x < y ? 0x0100 : 0x4000);\n"
    "}\n"
)
FABS_ASM = re.compile(r'__asm__ __volatile__ \("fabs;" : "=t" \(res\) : "0" \(x\)\);')
FXAM_ASM = re.compile(r'__asm__ __volatile__ \(\s*"fxam;[^:]*:\s*"=a" \((\w+)\)\s*:\s*"t" \((\w+)\)\s*\);')
FUCOM_ASM = re.compile(r'__asm__ __volatile__ \("fucom %%st\(1\);"\s*"fnstsw;": "=a" \(retval\) : "t" \(x\), "u" \(y\)\);')


def standin_math(text):
    """The mingw math.h with its x87 inline assembly replaced by the same results in C."""
    text = FABS_ASM.sub("res = __builtin_fabs(x);", text)
    text = FXAM_ASM.sub(r"\1 = __nv_fxam(\2);", text)
    text = FUCOM_ASM.sub("retval = __nv_fucom(x, y);", text)
    marker = "#ifndef _MATH_H_"
    at = text.find("#define _MATH_H_")
    if at >= 0:
        end = text.index("\n", at) + 1
        text = text[:end] + FXAM_HELPER + text[end:]
    return text


# MSVC inline assembly (`__asm { ... }` or `__asm mov ...`) has no meaning in wasm. The build copy
# replaces each block with a call to an in-module stub that traps; a function that still calls the
# stub after inlining is left out of the exports with the reason recorded (nv-rs#1000).
ASM_KEYWORD = re.compile(r"(?<![\w])_{1,2}asm(?![\w])")
ASM_STUB = "__nv_untranslated_asm"


def _skip_to_block_end(text, start):
    """The index just after the `}` that closes the `{` at `start`, or -1 when it never closes."""
    depth, index, size = 0, start, len(text)
    while index < size:
        char = text[index]
        if text.startswith("//", index):
            index = text.find("\n", index)
            if index < 0:
                return -1
            continue
        if text.startswith("/*", index):
            index = text.find("*/", index)
            if index < 0:
                return -1
            index += 2
            continue
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return index + 1
        index += 1
    return -1


def rewrite_inline_asm(text):
    """Replace every MSVC `__asm` statement or block with a call to the trapping stub."""
    out, last, count = [], 0, 0
    for match in ASM_KEYWORD.finditer(text):
        if match.start() < last:
            continue
        line_start = text.rfind("\n", 0, match.start()) + 1
        prefix = text[line_start:match.start()]
        if "//" in prefix or prefix.count('"') % 2:
            continue
        index = match.end()
        while index < len(text) and text[index] in " \t":
            index += 1
        if index < len(text) and text[index] == "{":
            end = _skip_to_block_end(text, index)
            if end < 0:
                continue
        else:
            end = text.find("\n", index)
            end = len(text) if end < 0 else end
            following = ASM_KEYWORD.search(text, index, end)
            if following:
                end = following.start()
            brace = text.find("}", index, end)
            if brace >= 0:
                end = brace
        out.append(text[last:match.start()])
        out.append("%s();" % ASM_STUB)
        last = end
        count += 1
    out.append(text[last:])
    return "".join(out), count


DIAGNOSTIC = re.compile(r"^(?P<file>[A-Za-z]:[^:\n]+|[^\s:][^:\n]*):(?P<line>\d+):\d+: (?P<kind>error|note): (?P<msg>.*)$")
CONFLICT_MESSAGES = ("conflicting types for", "has a different language linkage",
                     "functions that differ only in their return type cannot be overloaded",
                     "with C language linkage conflicts with declaration in global scope")
DECLARATION_LINE = re.compile(r"^\s*(?:extern\s+\"C\"\s+)?[^{};=]*(?:\([^{};]*\)|\b\w+)\s*;\s*$")


def _inside(file, copy):
    try:
        return str(Path(file).resolve()).startswith(str(Path(copy).resolve()) + os.sep)
    except OSError:
        return False


def drop_conflicting_declarations(stderr, copy):
    """Blank the decomp's own redeclaration of a system function or variable.

    The decomp spells its Win32 and CRT declarations for MSVC, which accepts a different
    parameter type under C linkage; clang refuses it. When the diagnostic's own line or its note
    is a one-line declaration in the build copy, that line is blanked (line numbers stay). Only the
    private copy changes. Returns how many lines were blanked.
    """
    groups, current = [], None
    for line in stderr.splitlines():
        found = DIAGNOSTIC.match(line)
        if not found:
            continue
        if found["kind"] == "error":
            current = {"msg": found["msg"], "where": [(found["file"], int(found["line"]))]}
            groups.append(current)
        elif current is not None:
            current["where"].append((found["file"], int(found["line"])))
    blanked = 0
    for group in groups:
        if not any(message in group["msg"] for message in CONFLICT_MESSAGES):
            continue
        # When the system header is the one that came second, the decomp's earlier declaration is
        # replaced by the include, so the code between the two still sees the system's.
        system_second = not _inside(group["where"][0][0], copy)
        for file, number in group["where"]:
            if not _inside(file, copy) or not Path(file).is_file():
                continue
            path = Path(file)
            lines = path.read_text(errors="surrogateescape").split("\n")
            if not (0 < number <= len(lines)):
                continue
            if not DECLARATION_LINE.match(lines[number - 1]):
                # A declaration after other statements on the line (`typedef X Vec;extern "C" T N[4];`):
                # blank only the `extern "C"` declaration that names the symbol.
                name = re.search(r"'(\w+)'", group["msg"])
                if not name or system_second:
                    continue
                pattern = re.compile(r'extern\s+"C"[^;{}]*\b%s\b[^;{}]*;' % re.escape(name.group(1)))
                changed = pattern.sub("", lines[number - 1], count=1)
                if changed == lines[number - 1]:
                    continue
                lines[number - 1] = changed
                path.write_text("\n".join(lines), errors="surrogateescape")
                blanked += 1
                break
            lines[number - 1] = "#include <windows.h>" if system_second else ""
            path.write_text("\n".join(lines), errors="surrogateescape")
            blanked += 1
            break
    return blanked


FNV_MARKER = re.compile(r"^// @fnv 0x([0-9a-fA-F]{1,8})\b")
EXPORT_PREFIX = "fnv_"
SKIPPED_HEADS = ("template", "extern", "struct", "class", "namespace", "union", "enum", "#", "}",
                 "typedef", "using", "__declspec(thread)", "__declspec(dllimport)")


def annotate_function_exports(text):
    """Give each function the decomp marks with `// @fnv 0xADDR` an export named for its address.

    Clang mangles for Itanium on wasm and the decomp's symbol table holds MSVC names, so a name
    cannot say which function is which; the marker above each definition does. The line that
    follows the marker, when it starts a function definition, gets
    `__attribute__((export_name("fnv_<addr>")))`. A marker over anything else (a template, a
    declaration, a variable) is left alone. Returns the text and the number of exports named.
    """
    lines = text.split("\n")
    count = 0
    for index, line in enumerate(lines):
        found = FNV_MARKER.match(line)
        if not found:
            continue
        head = index + 1
        while head < len(lines) and (not lines[head].strip() or lines[head].lstrip().startswith("//")):
            head += 1
        if head >= len(lines) or lines[head].lstrip().startswith(SKIPPED_HEADS):
            continue
        signature = []
        for probe in lines[head:head + 40]:
            signature.append(probe)
            joined = "\n".join(signature)
            if "{" in joined or ";" in joined:
                break
        joined = "\n".join(signature)
        brace, semi = joined.find("{"), joined.find(";")
        paren = joined.find("(")
        if brace < 0 or (0 <= semi < brace) or paren < 0 or paren > brace:
            continue
        if "=" in joined[:paren]:
            continue
        # A constructor or destructor is emitted twice (complete and base, or deleting), and both
        # would claim the name.
        head_text = joined[:brace]
        if re.search(r"(\w+)::~?\1\s*\(", head_text) or "::~" in head_text:
            continue
        address = int(found.group(1), 16)
        lines[head] = '__attribute__((export_name("%s%08x"))) %s' % (EXPORT_PREFIX, address, lines[head])
        count += 1
    return "\n".join(lines), count


EXPORT_NAME_ATTRIBUTE = re.compile(r'__attribute__\(\(export_name\("%s([0-9a-f]{8})"\)\)\) ' % EXPORT_PREFIX)


def keep_first_export_of_each_address(copy, sources):
    """Strip the export name from a function whose address an earlier unit already exports.

    Variants of one source (`_o2_variant`, `_rtm_variant`) hold the same function at the same
    address; a module cannot export one name twice. The first unit named keeps it; the later
    function is still compiled, and anything inside the module that calls it still reaches it.
    Returns how many export names were removed.
    """
    seen, removed = set(), 0
    for source in sources:
        path = Path(copy) / source
        if not path.is_file():
            continue
        text = path.read_text(errors="surrogateescape")

        def decide(match):
            nonlocal removed
            if match.group(1) in seen:
                removed += 1
                return ""
            seen.add(match.group(1))
            return match.group(0)

        updated = EXPORT_NAME_ATTRIBUTE.sub(decide, text)
        if updated != text:
            path.write_text(updated, errors="surrogateescape")
    return removed


def wat_function_bodies(wat):
    """Map each function's index to its WAT text; the module's functions are `(func (;N;) ...)`."""
    bodies = {}
    for part in re.split(r"\n  \(func (?=\(;)", wat)[1:]:
        found = re.match(r"\(;(\d+);\)", part)
        if found:
            bodies[int(found.group(1))] = part
    return bodies


def wat_exports(wat):
    """Export field to function index."""
    return {field: int(index) for field, index in
            re.findall(r'\(export "([^"]+)" \(func (\d+)\)\)', wat)}


def wat_got_globals(wat):
    """The GOT imports as {global index: (kind, field)}."""
    return {int(index): (kind, field) for kind, field, index in
            re.findall(r'\(import "GOT\.(mem|func)" "([^"]+)" \(global \(;(\d+);\)', wat)}


def exports_left_out(wat, unresolved=()):
    """Why a function the module exports cannot be called, keyed by the export's field.

    A function that calls the asm stub holds x86 inline assembly (the stub traps). One that reads
    a GOT global the decomp's symbol table has no address for would read a zero binding. Either is
    left out of the exports, with the reason.
    """
    bodies, exports = wat_function_bodies(wat), wat_exports(wat)
    reasons = {}
    stub = exports.get(ASM_STUB)
    unresolved_index = {index: field for index, (_, field) in wat_got_globals(wat).items()
                        if field in set(unresolved)}
    for field, function in exports.items():
        body = bodies.get(function, "")
        if stub is not None and function != stub and re.search(r"call %d(?!\d)" % stub, body):
            reasons[field] = "contains x86 inline assembly that has no wasm translation"
            continue
        for index, name in unresolved_index.items():
            if re.search(r"global\.get %d(?!\d)" % index, body):
                reasons[field] = "uses %s, which has no address in the decomp's symbol table" % name
                break
    return reasons


# The SSE intrinsics the decomp declares and calls (`extern "C" LocalM128 _mm_add_ps(...)`), as plain
# functions over MSVC's `__m128` layout, linked into the module. The arithmetic is IEEE single
# precision, so add, sub, mul, div, min, max, sqrt and the compares are exact; `rcp` and `rsqrt`
# return the processor's 12-bit estimate from the tables of `engine-core`'s `estimate` module
# (`ESTIMATE_TABLES`, read from that file at build time), and `getcsr` reports the default control
# state.

# The estimate model of `crates/engine-core/src/estimate.rs` in C, over the tables of
# `estimate/tables.rs`: the result depends on the exponent and the top 12 mantissa bits only.
ESTIMATE_TABLES = Path(__file__).resolve().parents[2] / "crates/engine-core/src/estimate/tables.rs"
ESTIMATE_MODEL = r"""
#define NV_SIGN 0x80000000u
#define NV_EXP 0x7f800000u
#define NV_MAN 0x007fffffu
static unsigned int nv_from_entry(unsigned short entry, unsigned int sign, int shift) {
  int exponent = (int)(entry >> 12) + 120 + shift;
  if (exponent < 1) return sign;
  return sign | ((unsigned int)exponent << 23) | ((unsigned int)(entry & 0xfff) << 11);
}
static unsigned int nv_rcp_bits(unsigned int x) {
  unsigned int sign = x & NV_SIGN, exponent = x & NV_EXP;
  if (exponent == NV_EXP) return (x & NV_MAN) ? (x | 0x00400000u) : sign;
  if (exponent == 0) return sign | NV_EXP;
  int field = (int)(exponent >> 23);
  return nv_from_entry(nv_rcp_table[(x & NV_MAN) >> 11], sign, 127 - field);
}
static unsigned int nv_rsqrt_bits(unsigned int x) {
  unsigned int sign = x & NV_SIGN, exponent = x & NV_EXP;
  if (exponent == NV_EXP) return (x & NV_MAN) ? (x | 0x00400000u) : (sign ? 0xffc00000u : 0);
  if (exponent == 0) return sign | NV_EXP;
  if (sign) return 0xffc00000u;
  int field = (int)(exponent >> 23);
  int odd = field & 1;
  int half = odd ? 0 : 4096, reference = odd ? 127 : 128;
  return nv_from_entry(nv_rsqrt_table[half + ((x & NV_MAN) >> 11)], 0, (reference - field) / 2);
}
"""


def estimate_source(tables=None):
    """The C that answers `_mm_rcp_*` and `_mm_rsqrt_*` as the measured processor does."""
    text = Path(tables or ESTIMATE_TABLES).read_text()
    out = []
    for name, count in (("RCP", 4096), ("RSQRT", 8192)):
        match = re.search(r"const %s: \[u16; (\d+)\] = \[(.*?)\];" % name, text, re.S)
        if not match or int(match.group(1)) != count:
            raise ValueError("estimate tables: %s table not found with %d entries" % (name, count))
        values = re.findall(r"0x[0-9a-fA-F]+", match.group(2))
        if len(values) != count:
            raise ValueError("estimate tables: %s has %d entries, not %d" % (name, len(values), count))
        out.append("static const unsigned short nv_%s_table[%d] = {%s};\n"
                   % (name.lower(), count, ",".join(values)))
    return "".join(out) + ESTIMATE_MODEL


SSE_SOURCE = r"""
typedef union __attribute__((aligned(16))) { float f[4]; unsigned int u[4]; unsigned long long q[2]; } m128;
#define OP(name, expr) m128 name(m128 a, m128 b) { m128 r; \
  for (int i = 0; i < 4; i++) r.f[i] = (expr); return r; }
#define OPS(name, expr) m128 name(m128 a, m128 b) { m128 r = a; \
  { int i = 0; r.f[0] = (expr); } return r; }
#define BITS(name, expr) m128 name(m128 a, m128 b) { m128 r; \
  for (int i = 0; i < 4; i++) r.u[i] = (expr); return r; }
#define CMP(name, test) m128 name(m128 a, m128 b) { m128 r; \
  for (int i = 0; i < 4; i++) r.u[i] = (test) ? 0xffffffffu : 0; return r; }
#define CMPS(name, test) m128 name(m128 a, m128 b) { m128 r = a; \
  { int i = 0; r.u[0] = (test) ? 0xffffffffu : 0; } return r; }
OP(_mm_add_ps, a.f[i] + b.f[i])  OPS(_mm_add_ss, a.f[i] + b.f[i])
OP(_mm_sub_ps, a.f[i] - b.f[i])  OPS(_mm_sub_ss, a.f[i] - b.f[i])
OP(_mm_mul_ps, a.f[i] * b.f[i])  OPS(_mm_mul_ss, a.f[i] * b.f[i])
OP(_mm_div_ps, a.f[i] / b.f[i])  OPS(_mm_div_ss, a.f[i] / b.f[i])
OP(_mm_min_ps, a.f[i] < b.f[i] ? a.f[i] : b.f[i])
OP(_mm_max_ps, a.f[i] > b.f[i] ? a.f[i] : b.f[i])
OPS(_mm_min_ss, a.f[i] < b.f[i] ? a.f[i] : b.f[i])
OPS(_mm_max_ss, a.f[i] > b.f[i] ? a.f[i] : b.f[i])
BITS(_mm_and_ps, a.u[i] & b.u[i])  BITS(_mm_andnot_ps, ~a.u[i] & b.u[i])
BITS(_mm_or_ps, a.u[i] | b.u[i])   BITS(_mm_xor_ps, a.u[i] ^ b.u[i])
CMP(_mm_cmpeq_ps, a.f[i] == b.f[i])  CMP(_mm_cmplt_ps, a.f[i] < b.f[i])
CMP(_mm_cmple_ps, a.f[i] <= b.f[i])  CMP(_mm_cmpgt_ps, a.f[i] > b.f[i])
CMP(_mm_cmpge_ps, a.f[i] >= b.f[i])  CMP(_mm_cmpneq_ps, !(a.f[i] == b.f[i]))
CMPS(_mm_cmpeq_ss, a.f[i] == b.f[i]) CMPS(_mm_cmplt_ss, a.f[i] < b.f[i])
CMPS(_mm_cmple_ss, a.f[i] <= b.f[i]) CMPS(_mm_cmpgt_ss, a.f[i] > b.f[i])
CMPS(_mm_cmpge_ss, a.f[i] >= b.f[i]) CMPS(_mm_cmpneq_ss, !(a.f[i] == b.f[i]))
m128 _mm_sqrt_ps(m128 a) { m128 r; for (int i = 0; i < 4; i++) r.f[i] = __builtin_sqrtf(a.f[i]); return r; }
m128 _mm_sqrt_ss(m128 a) { m128 r = a; r.f[0] = __builtin_sqrtf(a.f[0]); return r; }
m128 _mm_rcp_ps(m128 a) { m128 r; for (int i = 0; i < 4; i++) r.u[i] = nv_rcp_bits(a.u[i]); return r; }
m128 _mm_rcp_ss(m128 a) { m128 r = a; r.u[0] = nv_rcp_bits(a.u[0]); return r; }
m128 _mm_rsqrt_ps(m128 a) { m128 r; for (int i = 0; i < 4; i++) r.u[i] = nv_rsqrt_bits(a.u[i]); return r; }
m128 _mm_rsqrt_ss(m128 a) { m128 r = a; r.u[0] = nv_rsqrt_bits(a.u[0]); return r; }
void _mm_stream_ps(float* p, m128 a) { p[0] = a.f[0]; p[1] = a.f[1]; p[2] = a.f[2]; p[3] = a.f[3]; }
void _mm_prefetch(const char* p, int hint) { (void)p; (void)hint; }
m128 _mm_setzero_ps(void) { m128 r = {{0, 0, 0, 0}}; return r; }
m128 _mm_set_ps1(float v) { m128 r = {{v, v, v, v}}; return r; }
m128 _mm_set1_ps(float v) { m128 r = {{v, v, v, v}}; return r; }
m128 _mm_set_ps(float w, float z, float y, float x) { m128 r = {{x, y, z, w}}; return r; }
m128 _mm_setr_ps(float x, float y, float z, float w) { m128 r = {{x, y, z, w}}; return r; }
m128 _mm_set_ss(float v) { m128 r = {{v, 0, 0, 0}}; return r; }
m128 _mm_load_ss(const float* p) { m128 r = {{*p, 0, 0, 0}}; return r; }
m128 _mm_load_ps(const float* p) { m128 r = {{p[0], p[1], p[2], p[3]}}; return r; }
m128 _mm_loadu_ps(const float* p) { m128 r = {{p[0], p[1], p[2], p[3]}}; return r; }
m128 _mm_load_ps1(const float* p) { m128 r = {{*p, *p, *p, *p}}; return r; }
m128 _mm_load1_ps(const float* p) { m128 r = {{*p, *p, *p, *p}}; return r; }
void _mm_store_ss(float* p, m128 a) { *p = a.f[0]; }
void _mm_store_ps(float* p, m128 a) { p[0] = a.f[0]; p[1] = a.f[1]; p[2] = a.f[2]; p[3] = a.f[3]; }
void _mm_storeu_ps(float* p, m128 a) { p[0] = a.f[0]; p[1] = a.f[1]; p[2] = a.f[2]; p[3] = a.f[3]; }
float _mm_cvtss_f32(m128 a) { return a.f[0]; }
int _mm_movemask_ps(m128 a) { return (a.u[0] >> 31) | ((a.u[1] >> 31) << 1) | ((a.u[2] >> 31) << 2) | ((a.u[3] >> 31) << 3); }
m128 _mm_move_ss(m128 a, m128 b) { m128 r = a; r.f[0] = b.f[0]; return r; }
m128 _mm_unpacklo_ps(m128 a, m128 b) { m128 r = {{a.f[0], b.f[0], a.f[1], b.f[1]}}; return r; }
m128 _mm_unpackhi_ps(m128 a, m128 b) { m128 r = {{a.f[2], b.f[2], a.f[3], b.f[3]}}; return r; }
m128 _mm_movelh_ps(m128 a, m128 b) { m128 r = {{a.f[0], a.f[1], b.f[0], b.f[1]}}; return r; }
m128 _mm_movehl_ps(m128 a, m128 b) { m128 r = {{b.f[2], b.f[3], a.f[2], a.f[3]}}; return r; }
m128 _mm_shuffle_ps(m128 a, m128 b, int m) {
  m128 r = {{a.f[m & 3], a.f[(m >> 2) & 3], b.f[(m >> 4) & 3], b.f[(m >> 6) & 3]}}; return r; }
unsigned int _mm_getcsr(void) { return 0x1f80; }
void _mm_setcsr(unsigned int v) { (void)v; }
"""


# MSVC's built-in `__m128` and the intrinsics a unit calls without declaring them, spelled as
# the compiler's own header does: `<xmmintrin.h>` and the fallback header a unit gets when clang
# does not know the type (`sse_types.h`). `_INCLUDED_MM2` is the guard the decomp's own copies of
# the union test, so the type is defined once either way. The functions are those of `nv_sse.c`.
SSE_DECLS = (
    "#ifndef _INCLUDED_MM2\n#define _INCLUDED_MM2\n"
    "#define _MM_SHUFFLE(z, y, x, w) (((z) << 6) | ((y) << 4) | ((x) << 2) | (w))\n"
    "union __declspec(intrin_type) __declspec(align(16)) __m128 {\n"
    "  float m128_f32[4]; unsigned __int64 m128_u64[2]; };\n"
    "#ifdef __cplusplus\nextern \"C\" {\n#endif\n"
    + "".join("__m128 %s(__m128, __m128);\n" % name for name in (
        "_mm_add_ps _mm_sub_ps _mm_mul_ps _mm_div_ps _mm_min_ps _mm_max_ps _mm_add_ss _mm_sub_ss "
        "_mm_mul_ss _mm_div_ss _mm_min_ss _mm_max_ss _mm_and_ps _mm_andnot_ps _mm_or_ps _mm_xor_ps "
        "_mm_cmpeq_ps _mm_cmplt_ps _mm_cmple_ps _mm_cmpgt_ps _mm_cmpge_ps _mm_cmpneq_ps "
        "_mm_cmpeq_ss _mm_cmplt_ss _mm_cmple_ss _mm_cmpgt_ss _mm_cmpge_ss _mm_cmpneq_ss "
        "_mm_move_ss _mm_unpacklo_ps _mm_unpackhi_ps _mm_movelh_ps _mm_movehl_ps").split())
    + "".join("__m128 %s(__m128);\n" % name for name in (
        "_mm_sqrt_ps _mm_sqrt_ss _mm_rcp_ps _mm_rcp_ss _mm_rsqrt_ps _mm_rsqrt_ss").split())
    + "__m128 _mm_setzero_ps(void);\n__m128 _mm_set_ps1(float);\n__m128 _mm_set1_ps(float);\n"
    "__m128 _mm_set_ps(float, float, float, float);\n__m128 _mm_setr_ps(float, float, float, float);\n"
    "__m128 _mm_set_ss(float);\n__m128 _mm_load_ss(const float*);\n__m128 _mm_load_ps(const float*);\n"
    "__m128 _mm_loadu_ps(const float*);\n__m128 _mm_load_ps1(const float*);\n"
    "__m128 _mm_load1_ps(const float*);\nvoid _mm_store_ss(float*, __m128);\n"
    "void _mm_store_ps(float*, __m128);\nvoid _mm_storeu_ps(float*, __m128);\n"
    "void _mm_stream_ps(float*, __m128);\nfloat _mm_cvtss_f32(__m128);\nint _mm_movemask_ps(__m128);\n"
    "__m128 _mm_shuffle_ps(__m128, __m128, int);\nvoid _mm_prefetch(const char*, int);\n"
    "#ifdef __cplusplus\n}\n#endif\n#endif\n"
)


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
        # The x86 barrier asm names `eax`, which wasm has no register for; a fence is the same ordering.
        # A unit calls `::_ReadWriteBarrier()`; the macro's asm cannot take the scope operator, so
        # the name is the function `nv_runtime.c` defines (a fence).
        intrin = intrin.replace(
            '#define _ReadWriteBarrier() __asm__ __volatile__ ("" ::: "memory")\n', "")
        intrin = re.sub(r"#define __buildmemorybarrier\(\) \\\n\{ \\\n.*?\n\}\n",
                        "#define __buildmemorybarrier() { __atomic_thread_fence(__ATOMIC_SEQ_CST); }\n",
                        intrin, flags=re.S)
        # A definition the header marks for inlining is emitted by every unit that includes it; a
        # weak one lets the units link together instead of dropping all but the first. A marker
        # that precedes an asm wrapper (`__build...`) has nothing to attach to and goes.
        lines, kept = intrin.split("\n"), []
        for number, line in enumerate(lines):
            if re.match(r"\s*(?:__MINGW_EXTENSION\s+)?__INTRINSICS_USEINLINE\s*$", line):
                follows_wrapper = number + 1 < len(lines) and re.match(r"\s*__build\w*\(", lines[number + 1])
                if not follows_wrapper:
                    kept.append("__attribute__((__weak__))")
            elif not re.match(r"\s*(?:__INTRINSICS_USEINLINE|__build\w*\(.*\)\s*;?\s*$)", line):
                kept.append(line)
        intrin = "\n".join(kept)
        (standins / "psdk_inc" / "intrin-impl.h").write_text(intrin)
        math = Path(mingw) / "math.h"
        if math.is_file():
            (standins / "math.h").write_text(standin_math(math.read_text(errors="surrogateescape")))
    # The decomp spells its own `__m128` union and declares the `_mm_*` functions it calls; the
    # headers stay empty so clang's x86-only ones are not used, and nv_sse.c defines the functions.
    # `intrin.h` stays empty: windows.h includes it, and a unit that declares its own
    # `extern "C" LocalM128 _mm_add_ps(...)` would then see two spellings. The headers a unit
    # includes to get the intrinsics define them.
    (standins / "intrin.h").write_text("#pragma once\n")
    for name in ("xmmintrin.h", "emmintrin.h", "pmmintrin.h", "immintrin.h", "mmintrin.h",
                 "x86intrin.h"):
        (standins / name).write_text("#pragma once\n" + SSE_DECLS)
    (standins / "wasm_prelude.h").write_text(
        "#pragma once\nextern \"C\" unsigned long __readfsdword(unsigned long);\n")
    # Included in every unit: the stub the rewritten `__asm` blocks call, and the barrier clang
    # reads as an intrinsic that mingw's empty intrin.h stand-in no longer provides.
    (standins / "nv_asm.h").write_text(
        "#pragma once\n"
        + "".join("#ifndef %s\n#define %s\n#endif\n" % (n, n) for n in (
            "_In_", "_In_opt_", "_Out_", "_Out_opt_", "_Inout_", "_Inout_opt_", "_Ret_maybenull_",
            "_Success_(x)", "_Check_return_"))
        + "#ifdef __cplusplus\nextern \"C\" {\n#endif\n"
        "__attribute__((noreturn, noinline)) void %s(void);\n"
        "#ifdef __cplusplus\n}\n#endif\n"
        "#ifdef __cplusplus\nextern \"C\" {\n#endif\n"
        "void _ReadWriteBarrier(void);\nunsigned int _mm_getcsr(void);\nvoid _mm_setcsr(unsigned int);\n"
        "#ifdef __cplusplus\n}\n#endif\n"
        % ASM_STUB)
    # MSVC's built-in `__m128`, for units that use it without declaring it.
    (standins / "sse_types.h").write_text("#pragma once\n" + SSE_DECLS)
    (standins / "nv_runtime.c").write_text(
        "__attribute__((noreturn, noinline)) void %s(void) { __builtin_trap(); }\n"
        "void _ReadWriteBarrier(void) { __atomic_signal_fence(__ATOMIC_SEQ_CST); }\n%s" % (ASM_STUB, estimate_source() + SSE_SOURCE))
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
                        if system != "Windows" else ["-O2", "-shared", "-fuse-ld=lld"],
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
        "wasm2wat": binary("wasm2wat"),
        "wat2wasm": binary("wat2wasm"),
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
    return paths if toolchain_files_ready(paths) else None


def toolchain_files_ready(paths):
    """The binaries and wabt runtime are really on disk, not just named in a stamp."""
    needed = [paths[key] for key in REQUIRED_TOOLS] + [
        str(Path(paths["wabt_runtime"]) / name)
        for name in ("wasm-rt-impl.c", "wasm-rt-mem-impl.c")]
    binaries = [paths[key] for key in REQUIRED_TOOLS]
    return (all(Path(path).is_file() for path in needed)
            and all(path.endswith(".exe") or os.access(path, os.X_OK) for path in binaries))


def download_archive(url, path):
    """Download with a configurable socket timeout and a few retries."""
    timeout = float(os.environ.get("NV_TOOLCHAIN_TIMEOUT", "120"))
    last = None
    for _ in range(3):
        try:
            with urllib.request.urlopen(url, timeout=timeout) as response, open(path, "wb") as archive:
                shutil.copyfileobj(response, archive)
            return
        except (OSError, ValueError) as exc:
            last = exc
    raise ToolchainUnavailable("download failed after 3 attempts: %s" % last)


def blocked_download(url, path):
    raise ToolchainUnavailable("toolchain fetching is disabled")


def safe_unpack(archive, target):
    """Unpack a toolchain zip, refusing names that would escape its folder."""
    root = Path(target).resolve()
    with zipfile.ZipFile(archive) as bundle:
        for info in bundle.infolist():
            name = info.filename.replace("\\", "/")
            if (name.startswith("/") or re.match(r"^[A-Za-z]:", name)
                    or ".." in PurePosixPath(name).parts):
                raise ValueError("unsafe toolchain archive member: %s" % info.filename)
        bundle.extractall(root)


def fetch_toolchain(cache, host, download=None):
    """Fetch and unpack clang and wabt once, then stamp the cache as complete."""
    cached = cached_toolchain(cache, host)
    if cached:
        return cached
    target, paths = toolchain_paths(cache, host)
    target.mkdir(parents=True, exist_ok=True)
    fetch = download or download_archive
    pins = toolchain_hashes()
    base = os.environ.get("NV_TOOLCHAIN_URL", TOOLCHAIN_URL)
    for asset in TOOLCHAIN_ASSETS:
        if not pins.get(asset):
            raise ToolchainUnavailable(
                "no SHA-256 pin for %s; set NV_TOOLCHAIN_SHA256 before fetching" % asset)
        url = "%s/%s-%s.zip" % (base, asset, host["slug"])
        archive = Path(cache) / ("%s-%s.zip" % (asset, host["slug"]))
        try:
            fetch(url, archive)
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            if digest.lower() != pins[asset].lower():
                raise ValueError("SHA-256 mismatch: %s" % digest)
            safe_unpack(archive, target)
        except (OSError, ValueError, shutil.ReadError, zipfile.BadZipFile) as exc:
            raise ToolchainUnavailable("could not fetch %s: %s" % (asset, exc)) from exc
        finally:
            archive.unlink(missing_ok=True)
    for key in REQUIRED_TOOLS:
        binary = Path(paths[key])
        if binary.is_file() and host["system"] != "Windows":
            binary.chmod(binary.stat().st_mode | 0o111)
    if not toolchain_files_ready(paths):
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
    """Find one consistent tool set: explicit overrides, then cache, then PATH."""
    host = host or detect_host()
    tools = {"host": host, "wabt_include": None, "wabt_runtime": None}
    overrides = {}
    for key, env, name in (
        ("clang", "NV_CLANG", "clang"),
        ("wasm_ld", "NV_WASM_LD", "wasm-ld"),
        ("wasm2c", "NV_WASM2C", "wasm2c"),
        ("wasm2wat", "NV_WABT_WASM_WAT", "wasm2wat"),
        ("wat2wasm", "NV_WABT_WAT_WASM", "wat2wasm"),
        ("cc", "NV_CC", host["native_compiler"]),
        ("cxx", "NV_CXX", host["native_cxx"]),
        ("wabt_share", "NV_WABT_SHARE", None),
    ):
        overrides[key] = os.environ.get(env)
        tools[key] = overrides[key] or (shutil.which(name) if name else None)
    cache = Path(cache_dir) if cache_dir is not None else toolchain_cache_dir()
    cached = cached_toolchain(cache, host)
    if cached:
        # A cached host is one install: do not mix its wasm-ld with PATH's wabt.
        for key in (*REQUIRED_TOOLS, "wabt_include", "wabt_runtime", "wabt_share"):
            if not overrides.get(key):
                tools[key] = cached[key]
    elif not all(tools[key] for key in REQUIRED_TOOLS):
        try:
            cached = fetch_toolchain(cache, host, download)
        except ToolchainUnavailable as exc:
            tools["toolchain_error"] = str(exc)
        if cached:
            for key in (*REQUIRED_TOOLS, "wabt_include", "wabt_runtime", "wabt_share"):
                if not overrides.get(key):
                    tools[key] = cached[key]
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


def wabt_tool(tools, name):
    """Use the tool from the chosen set, then an override, then wasm2c's sibling."""
    return (tools.get(name) or
            os.environ.get("NV_WABT_" + name.upper().replace("2", "_").replace("-", "_")) or
            (str(Path(tools["wasm2c"]).with_name(name))
             if tools.get("wasm2c") and Path(tools["wasm2c"]).with_name(name).is_file() else None))


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
    names = {func.lstrip("$"): func.lstrip("$")
             for func in re.findall(r"\(func\s+\$([\w.]+)", wat)}
    names.update({func.lstrip("$"): name for name, func in WAT_EXPORT_RE.findall(wat)})
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


def wasm_flags(copy, standins, tools, cxx=False, sse_types=False):
    flags = ["--target=wasm32-unknown-unknown", "-mms-bitfields", "-fshort-wchar",
             "-fms-extensions", "-fms-compatibility", "-fms-compatibility-version=19",
             "-nostdlib", "-Wno-everything", "-O2", "-fPIC", "-flto", "-D_WIN32",
             "-DWIN32", "-D__i386__", "-D_M_IX86=600", "-D_X86_", "-D__GNUC__=4",
             "-D__GNUC_MINOR__=2", "-fno-builtin-_InterlockedIncrement",
             "-fno-builtin-_InterlockedDecrement", "-include", str(standins / "nv_asm.h"),
             "-isystem", str(standins)]
    # libc++ first: its stddef.h and friends wrap the next header on the path, mingw's included.
    if cxx and tools.get("libcxx"):
        flags += ["-isystem", tools["libcxx"], "-D_LIBCPP_HAS_NO_THREADS",
                  "-include", str(standins / "wasm_prelude.h")]
    if tools.get("mingw"):
        flags += ["-isystem", tools["mingw"]]
    if sse_types:
        flags += ["-include", str(standins / "sse_types.h")]
    flags += ["-I", str(copy / "include"), "-I", str(copy / "src")]
    if tools.get("resource"):
        flags += ["-resource-dir", tools["resource"]]
    return flags


def compile_unit(tools, copy, standins, source, obj, cxx=False):
    """Compile one unit to wasm, repairing what clang refuses in the private copy and trying again.

    Two repairs: a declaration of the decomp's that contradicts the system header's is blanked
    (`drop_conflicting_declarations`), and a unit that uses MSVC's built-in `__m128` without
    declaring it gets the stand-in type. The loop stops when a round changes nothing.
    """
    path = copy / source
    if not path.is_file():
        return False, "source not found in build copy"
    compiler = tools["cxx"] if cxx else tools["clang"]
    sse_types, result, attempts = False, None, []
    for _ in range(12):
        result = run([compiler, *wasm_flags(copy, standins, tools, cxx, sse_types),
                      "-c", path, "-o", obj])
        if not result.returncode:
            return True, ""
        attempts.append(result)
        blanked = drop_conflicting_declarations(result.stderr, copy)
        # The stand-in type is the last repair: a blanked declaration can clear the same error.
        wants_sse = (not blanked and not sse_types
                     and ("unknown type name '__m128'" in result.stderr
                          or "no type named '__m128'" in result.stderr
                          or "use of undeclared identifier '_mm_" in result.stderr))
        sse_types = sse_types or wants_sse
        if not blanked and not wants_sse:
            break
    # Report the attempt that got furthest: the stand-in type can make a unit worse.
    result = min(attempts, key=lambda r: r.stderr.count("error:"))
    for line in result.stderr.splitlines():
        if "error:" in line:
            return False, line.split("error:", 1)[1].strip()[:240]
    return False, (result.stderr.strip().splitlines() or ["clang failed"])[-1][:240]


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


def address_value(value):
    """An address as a number, or None when it is not recorded."""
    return None if value is None else (int(value, 0) if isinstance(value, str) else int(value))


def address_literal(value):
    """A 32-bit address literal that has the same value on every host's C compiler."""
    return "0x%08xU" % ((address_value(value) or 0) & 0xffffffff)


def collect_symbols(manifest, units):
    """Every spelling of the decomp's symbols, from the manifest and the units."""
    symbols = {}
    tables = [manifest.get("symbols", {})] + [unit.get("symbols", {}) for unit in units]
    for table in tables:
        for name, address in table.items():
            for key in (name, name.lstrip("?_"), name.lstrip("?").split("@@", 1)[0]):
                symbols.setdefault(key, address)
    for unit in units:
        for function in unit.get("functions", []):
            name, address = function.get("symbol", ""), function.get("address")
            if not name or address is None:
                continue
            for key in (name, name.lstrip("?"), name.lstrip("_"), name.lstrip("_?")):
                symbols.setdefault(key, address)
    return symbols


ADDRESS_NAME = re.compile(r"(?:^|[^0-9A-Za-z])(?:DAT|PTR|FUN|str|g|s|u|LAB|BYTE|DWORD|WORD|PTR_DAT|PTR_FUN)_(?:[A-Za-z]*_)?"
                          r"([0-9a-f]{8})(?![0-9A-Za-z])")


def itanium_identifiers(name):
    """The length-prefixed identifiers of an Itanium name, or the name itself when it has none."""
    found, index = [], 0
    while True:
        match = re.compile(r"(\d+)").search(name, index)
        if not match:
            break
        size = int(match.group(1))
        start = match.end()
        if size == 0 or start + size > len(name):
            index = match.end()
            continue
        found.append(name[start:start + size])
        index = start + size
    return found or [name]


def symbol_address(symbols, name):
    """Resolve an export or GOT name to a number, including its literal _name key.

    Besides the symbol tables: `fnv_<addr>` is an export the build named for its address, and the
    decomp spells its globals and functions with the address (`DAT_01017130`, `FUN_00c877b0`),
    also inside an Itanium name (`_ZN12fnv_00b13c1012DAT_01017130E`).
    """
    found = re.fullmatch(EXPORT_PREFIX + r"([0-9a-f]{8})", name)
    if found:
        return int(found.group(1), 16)
    for key in (name, name.lstrip("?_"), name.lstrip("?"), "_" + name, name.upper()):
        if key in symbols:
            value = symbols[key]
            return int(value, 0) if isinstance(value, str) else int(value)
    for identifier in reversed(itanium_identifiers(name)):
        found = ADDRESS_NAME.fullmatch(identifier) or ADDRESS_NAME.search(identifier)
        if found and re.fullmatch(r"[0-9a-f]{8}", found.group(1)):
            return int(found.group(1), 16)
    return None


def export_manifest(exports, units, symbols=None):
    """Attach each export's original address from the decomp's symbol tables."""
    symbols = symbols if symbols is not None else collect_symbols({}, units)
    result = []
    for index, entry in enumerate(exports):
        va = symbol_address(symbols, entry["name"])
        arity = len(entry["params"])
        result.append({**entry, "address": va, "wrapper": "decomp_call_%d" % index,
                       "arity": arity if arity <= MAX_ARITY and all(p in {"u32", "f32", "int", "unsigned int"}
                                                            for p in entry["params"]) else None})
    return result


def unmangle(name):
    """A wasm2c identifier back to the module's field name: `0x5F` is `_`, `0x2E` is `.`."""
    return re.sub(r"0x([0-9A-Fa-f]{2})", lambda m: chr(int(m.group(1), 16)), name)


def got_values(globals_, func_globals, symbols, lenient=False):
    """Bind every GOT.mem and GOT.func name, refusing a silent zero binding.

    With `lenient` an unresolved name is bound to zero and returned in a third list, so the build
    can leave out the exports that read it instead of stopping.
    """
    mem = [symbol_address(symbols, name) for name in globals_]
    funcs = {name: symbol_address(symbols, name) for name in func_globals}
    unresolved = ([name for name, value in zip(globals_, mem) if value is None]
                  + [name for name, value in funcs.items() if value is None])
    if lenient:
        return ([value or 0 for value in mem], {k: v or 0 for k, v in funcs.items()}, unresolved)
    if unresolved:
        raise RuntimeError("unresolved GOT symbol(s): " + ", ".join(unresolved))
    return mem, funcs


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
                            "address": address_value(function.get("address")), "arity": len(params)})
    for index, entry in enumerate(exports):
        entry["wrapper"] = "decomp_call_%d" % index
    return exports, skipped


def generate_shim(path, imports, exports, globals_, instantiate, host=None,
                  func_globals=None, func_values=None, startup=None):
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
             'static uint32_t decomp_trap_code; static const char* decomp_trap_reason;',
             '/* The rest of the last call\'s result: EDX (the high word of a u64) and ST0 (a float). */',
             'static uint32_t decomp_edx; static double decomp_st0;',
             'uint32_t decomp_last_edx(void) { return decomp_edx; }',
             'double decomp_last_st0(void) { return decomp_st0; }',
             "/* Trap state and wasm2c's jump buffer are process-global: one caller at a time. */", '']
    for index, name in enumerate(globals_):
        lines.append('u32* w2c_GOT0x2Emem_%s(struct w2c_GOT0x2Emem* g) { return &g->f%d; }'
                     % (name.replace(".", "_"), index))
    for index, name in enumerate(func_globals):
        lines.append('u32* w2c_GOT0x2Efunc_%s(struct w2c_GOT0x2Efunc* g) { return &g->f%d; }'
                     % (name.replace(".", "_"), index))
    lines += [
        'static uint32_t (*import_hook)(const char*, uint32_t*, uint32_t);',
        'void decomp_set_import_hook(uint32_t (*hook)(const char*, uint32_t*, uint32_t)) { import_hook = hook; }',
        'static uint32_t decomp_call_import(const char* name, uint32_t* args, uint32_t n) {',
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
        args = ", ".join("%s a%d" % (kind, i) for i, kind in enumerate(item["params"]))
        values = ", ".join("(u32)a%d" % i for i in range(len(item["params"])))
        array = "{" + values + "}" if values else "{0}"
        call = 'decomp_call_import("%s", a, %d)' % (item["field"], len(item["params"]))
        result = item.get("result") or "u32"
        body = ("%s; return;" % call if result == "void"
                else "return (%s)%s;" % (result, call))
        lines.append('%s(struct w2c_env* e%s) { (void)e; u32 a[] = %s; %s }'
                     % (item["decl"].split("(")[0].strip(), ", " + args if args else "", array, body))
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
        lines.append('  fgot.f%d = %s;' % (index, address_literal(func_values.get(name))))
    instantiate_args = "&inst"
    if len(instantiate.split(",")) >= 4:
        instantiate_args += ", &fgot, &got, &env"
    elif len(instantiate.split(",")) == 3:
        instantiate_args += ", &fgot, &env" if func_globals else ", &got, &env"
    elif len(instantiate.split(",")) == 2:
        instantiate_args += ", &env"
    # A module with no data relocations or constructors does not export these.
    startup = ("__wasm_apply_data_relocs", "__wasm_call_ctors") if startup is None else startup
    lines += ['  wasm2c_mod_instantiate(%s);' % instantiate_args]
    lines += ['  w2c_mod_0x5F%s(&inst);' % name[1:] for name in startup]
    for entry in exports:
        if entry["address"] is not None and entry["arity"] is not None:
            args = "WASM_RT_I32, " * (entry["arity"] + 1)
            lines.append('  env.table.data[%s] = (wasm_rt_funcref_t){' % address_literal(entry["address"]))
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
        # Every wrapper takes guest words; an f32 parameter is the word's bits.
        args = ", ".join("u32 a%d" % i for i in range(len(entry["params"])))
        values = ", ".join(
            "((union {u32 u; f32 f;}){.u=a%d}).f" % i if kind == "f32" else "a%d" % i
            for i, kind in enumerate(entry["params"]))
        call = "%s(&inst%s%s)" % (entry["cname"], ", " if values else "", values)
        kind = entry["result"]
        if kind == "void":
            body = "%s; return 0;" % call
        elif kind == "f32":
            body = ("f32 v = %s; decomp_st0 = (double)v; "
                    "return ((union {u32 u; f32 f;}){.f=v}).u;" % call)
        elif kind == "f64":
            body = "double v = %s; decomp_st0 = v; return 0;" % call
        elif kind == "u64":
            body = "uint64_t v = %s; decomp_edx = (u32)(v >> 32); return (u32)v;" % call
        else:
            body = "return (u32)(%s);" % call
        lines += ['u32 decomp_call_%d(%s) {' % (index, args),
                  '  wasm_rt_trap_t trap = WASM_RT_SETJMP(g_wasm_rt_jmp_buf);',
                  '  if (trap != WASM_RT_TRAP_NONE) { decomp_trap_code = trap;',
                  '    decomp_trap_reason = wasm_rt_strerror(trap); return 0; }',
                  '  decomp_trap_code = 0; decomp_trap_reason = NULL;',
                  '  decomp_edx = 0; decomp_st0 = 0.0;',
                  '  %s }' % body]
    Path(path).write_text("\n".join(lines) + "\n")


def fallback_shim(path, exports, host=None):
    """Write the host side of the native path with the same public ABI as the wasm one."""
    host = host or detect_host()
    declarations, registrations, wrappers = [], [], []
    for index, entry in enumerate(exports):
        params = ", ".join("%s a%d" % (p, i) for i, p in enumerate(entry["params"]))
        name, raw = entry["name"], entry.get("cname") or entry["name"]
        if re.fullmatch(r"[A-Za-z_]\w*", name) and (
                raw == name or re.fullmatch(r"_[A-Za-z_]\w*", raw)):
            # Host C compilers emit undecorated C names on every supported host.
            alias = name
            declarations.append("%s %s(%s);" % (entry["result"], alias, params))
        elif re.fullmatch(r"[A-Za-z_]\w*", raw):
            alias = raw
            declarations.append("%s %s(%s);" % (entry["result"], alias, params))
        else:
            # A decomp symbol such as ?f@@YAHH@Z must be declared exactly; the
            # wrapper forwards to it rather than to a name the linker cannot find.
            alias = "decomp_native_%d" % index
            declarations.append('extern %s %s(%s) __asm__("%s");'
                                % (entry["result"], alias, params, raw.replace('"', '\\"')))
        if entry["address"] is not None:
            registrations.append('  decomp_table_set(%s, (void*)%s, NULL, 0);'
                                 % (address_literal(entry["address"]), alias))
        values = ", ".join("((union {u32 u; f32 f;}){.u=a%d}).f" % i if p == "f32" else "a%d" % i
                           for i, p in enumerate(entry["params"]))
        call = "%s(%s)" % (alias, values)
        body = ("%s; return 0;" % call if entry["result"] == "void" else
                "return ((union {u32 u; f32 f;}){.f=(f32)(%s)}).u;" % call
                if entry["result"] == "f32" else "return (u32)(%s);" % call)
        words = ", ".join("u32 a%d" % i for i in range(len(entry["params"])))
        wrappers.append('u32 %s(%s) { %s }' % (entry["wrapper"], words, body))
    lines = ['#include <stdint.h>', '#include <stdlib.h>', '#include <string.h>']
    lines += guest_memory_source(host).splitlines()
    lines += [
        'typedef uint32_t u32; typedef float f32;',
        'static unsigned char* guest_base;',
        'static void** table; static uint32_t table_size;',
        'static uint32_t (*import_hook)(const char*, u32*, u32);',
        'void decomp_set_import_hook(uint32_t (*hook)(const char*, u32*, u32)) { import_hook = hook; }',
        'static uint32_t decomp_call_import(const char* name, u32* args, u32 n) {',
        '  (void)name; (void)args; (void)n; return import_hook ? import_hook(name, args, n) : 0; }',
        'static uint32_t decomp_trap_code; static const char* decomp_trap_reason;',
        "/* Trap state is process-global: call the library from one thread at a time. */",
        'int decomp_table_set(uint32_t va, void* fn, void* instance, uint32_t params);',
        '']
    lines += declarations
    lines += [
        'int decomp_init(unsigned char* guest, uint64_t size, uint32_t entries,',
        '                uint32_t* globals, uint32_t nglobals) {',
        '  (void)globals; (void)nglobals;',
        '  if (!guest) { int rc = decomp_reserve(&guest, size); if (rc) return rc; }',
        '  guest_base = guest;',
        '  table = calloc(entries, sizeof(void*)); table_size = entries;']
    lines += registrations
    lines += [
        '  return 0; }',
        'unsigned char* decomp_memory_base(void) { return guest_base; }',
        'int decomp_table_set(uint32_t va, void* fn, void* instance, uint32_t params) {',
        '  (void)instance; (void)params;',
        '  if (!table || va >= table_size) return -1;',
        '  table[va] = fn; return 0; }',
        'uint32_t decomp_last_trap_code(void) { return decomp_trap_code; }',
        'const char* decomp_last_trap_reason(void) { return decomp_trap_reason; }',
        'void decomp_clear_trap(void) { decomp_trap_code = 0; decomp_trap_reason = NULL; }',
        'uint32_t decomp_last_edx(void) { return 0; }',
        'double decomp_last_st0(void) { return 0.0; }',
        'int decomp_table_has(uint32_t va) {',
        '  return table && va < table_size && table[va] != NULL; }',
        '']
    lines += wrappers
    Path(path).write_text("\n".join(lines) + "\n")


def read_gap_addresses(path):
    """The addresses of a `real_gaps.txt`-style list: `<8 hex digits> <file> <result>` lines."""
    found = []
    for line in Path(path).read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and re.match(r"[0-9a-fA-F]{8}\b", line):
            found.append(int(line.split()[0], 16))
    return found


def gap_units(units, addresses):
    """The sources of the units that define a function at one of `addresses`, in manifest order."""
    wanted, sources = set(addresses), []
    for unit in units:
        held = {address_value(f.get("address")) for f in unit.get("functions", [])}
        if wanted & held and unit["source"] not in sources:
            sources.append(unit["source"])
    return sources


def select_units(units, sources=None, limit=None):
    if sources:
        by_source = {}
        for unit in units:
            by_source.setdefault(unit.get("source"), unit)
        # In the order asked for: when two units define one symbol, the first named keeps it.
        return [by_source[source] for source in dict.fromkeys(sources) if source in by_source]
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


def write_build_outputs(out, report, exports, globals_, global_values, imports=None, symbols=None):
    """Write the report, manifest and the peer's plain-text address table (nv-rs#928)."""
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (out / "report.md").write_text(render_report(report))
    (out / "manifest.json").write_text(json.dumps(
        {"library": str(report["library"]), "exports": exports, "globals": globals_,
         "global_values": global_values, "table_entries": TABLE_ENTRIES}, indent=2) + "\n")
    # The Rust peer reads this table without a JSON dependency. It carries no build output, only
    # the original addresses the shim already binds.
    lines = ["# original address, wrapper, arity; generated by decomp_backend.py"]
    for entry in exports:
        if entry.get("address") is None or entry.get("arity") is None:
            continue
        kind = entry.get("result")
        lines.append("export %08x %s %d%s" % (entry["address"], entry["wrapper"], entry["arity"],
                                              " " + kind if kind in ("f32", "f64", "u64") else ""))
    for item in imports or []:
        if not item.get("function"):
            continue
        address = symbol_address(symbols or {}, item["field"])
        if address is not None:
            lines.append("import %s %08x" % (item["field"], address))
    for address in global_values:
        lines.append("global %08x" % address)
    (out / "exports.tsv").write_text("\n".join(lines) + "\n")


def link_without_duplicates(tools, objects, runtime, wasm, chosen, outcomes):
    """Link the units, dropping a later unit that defines a symbol an earlier one already does.

    Variants of one source define the same symbol at different addresses; one module holds one
    definition. The first unit named keeps it, and each dropped unit is reported with the symbol
    and the unit that kept it. Returns the objects that were linked.
    """
    by_object = {}
    for unit, outcome in zip(chosen, outcomes):
        if outcome["ok"]:
            by_object[outcome["object"]] = (unit, outcome)
    objects = list(objects)
    for _ in range(len(objects) + 1):
        linked = run([tools["wasm_ld"], "--experimental-pic", "-shared",
                      "--unresolved-symbols=import-dynamic", "--export-all", "--import-memory",
                      "--max-memory=%d" % GUEST_SIZE, "--lto-O2", "-o", wasm, *objects, runtime])
        if not linked.returncode:
            return objects
        match = re.search(r"duplicate symbol: (.+)\n>>> defined in (\S+)\n>>> defined in (\S+)",
                          linked.stderr)
        if not match:
            raise RuntimeError("wasm-ld: " + linked.stderr.strip()[-500:])
        symbol, first, second = match.groups()
        dropped = Path(second).name
        if dropped not in by_object or dropped == Path(first).name:
            raise RuntimeError("wasm-ld: " + linked.stderr.strip()[-500:])
        kept = by_object.get(Path(first).name, ({"source": first}, None))[0]["source"]
        unit, outcome = by_object[dropped]
        outcome["ok"] = False
        outcome["reason"] = "duplicate symbol %s, kept from %s" % (symbol, kept)
        objects = [o for o in objects if o.name != dropped]
        if not objects:
            break
    raise RuntimeError("wasm-ld: no unit is left after dropping duplicates")


def build(decomp, out, units=None, limit=None, toolchain=None, download=None,
          compiler=None, allow_fallback=False, force_fallback=False):
    """Build a native library and write report.json/report.md into the output folder."""
    decomp, out = Path(decomp), Path(out).resolve()
    check_out_path(decomp, out)
    out.mkdir(parents=True, exist_ok=True)
    tools = toolchain if toolchain is not None else find_toolchain(download=download)
    host = tools.get("host") or detect_host()
    if force_fallback or (allow_fallback and not (toolchain_ready(tools) and wasm_runtime_ready(tools))):
        return build_cranelift(decomp, out, units, limit, host=host,
                               toolchain=tools, compiler=compiler)
    if not (toolchain_ready(tools) and wasm_runtime_ready(tools)):
        # The native fallback is a different module: it cannot be substituted
        # silently, because it does not use the address-indexed guest table.
        raise ToolchainUnavailable(tools.get("toolchain_error") or
                                   "wasm toolchain is incomplete; use --cranelift for the native fallback")
    manifest = json.loads((decomp / "build" / "units.json").read_text())
    all_units = manifest.get("units", [])
    chosen = select_units(all_units, units, limit)
    rewrites = prepare_build_copy(decomp, out)
    standins = make_standins(out, tools)
    objects, outcomes = [], []
    keep_first_export_of_each_address(out / "copy", [unit["source"] for unit in chosen])
    # Repairs to shared headers help the units after them and can break others; each unit is built
    # in a working copy first, and one that fails is tried again alone in a fresh copy.
    work, retry = out / "work", out / "retry"
    for folder in (work, retry):
        if folder.exists():
            shutil.rmtree(folder)
    shutil.copytree(out / "copy", work)
    for index, unit in enumerate(chosen):
        obj = out / ("unit_%04d.o" % index)
        cxx = unit["source"].endswith((".cpp", ".cc", ".cxx"))
        ok, reason = compile_unit(tools, work, standins, unit["source"], obj, cxx=cxx)
        if not ok:
            if retry.exists():
                shutil.rmtree(retry)
            shutil.copytree(out / "copy", retry)
            ok, reason = compile_unit(tools, retry, standins, unit["source"], obj, cxx=cxx)
        outcomes.append({"ok": ok, "reason": reason, "object": obj.name})
        if ok:
            objects.append(obj)
    for folder in (work, retry):
        shutil.rmtree(folder, ignore_errors=True)
    if not objects:
        raise RuntimeError("no unit compiled; nothing to link")
    runtime = out / "nv_runtime.o"
    stub = run([tools["clang"], "--target=wasm32-unknown-unknown", "-O2", "-fPIC", "-nostdlib",
                "-Wno-everything", "-c", standins / "nv_runtime.c", "-o", runtime])
    if stub.returncode:
        raise RuntimeError("asm stub: " + stub.stderr.strip()[-500:])
    wasm = out / "mod.wasm"
    objects = link_without_duplicates(tools, objects, runtime, wasm, chosen, outcomes)
    symbols = collect_symbols({}, chosen)
    for key, value in collect_symbols(manifest, all_units).items():
        symbols.setdefault(key, value)
    addresses = dict(symbols)
    for unit in chosen:
        for function in unit.get("functions", []):
            symbol = function.get("symbol", "")
            if symbol and function.get("address") is not None:
                addresses.setdefault(symbol.lstrip("_?"), function["address"])
    wat2wat, wat2wasm = wabt_tool(tools, "wasm2wat"), wabt_tool(tools, "wat2wasm")
    if not wat2wat or not wat2wasm:
        raise RuntimeError("wabt needs wasm2wat and wat2wasm to bind function addresses")
    disassembled = run([wat2wat, wasm])
    if disassembled.returncode:
        raise RuntimeError("wasm2wat: " + disassembled.stderr.strip()[-500:])
    text = disassembled.stdout
    # The analysis reads functions and globals by index; the rewrite below reads them by name.
    indexed = run([wat2wat, "--no-debug-names", wasm])
    analysed = indexed.stdout if not indexed.returncode else ""
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
    header = (out / "mod.h").read_text()
    imports, exports = parse_header(header)
    globals_, func_globals = [], []
    for item in imports:
        match = re.search(r"w2c_GOT0x2E(mem|func)_(\w+)", item["name"])
        if item["module"] == "GOT.mem" and match:
            globals_.append(match.group(2))
        elif item["module"] == "GOT.func" and match:
            func_globals.append(match.group(2))
    exports = export_manifest(exports, chosen, symbols)
    global_values, raw_funcs, unresolved = got_values(
        [unmangle(name) for name in globals_], [unmangle(name) for name in func_globals],
        symbols, lenient=True)
    func_values = {name: raw_funcs[unmangle(name)] for name in func_globals}
    # A function that holds x86 `__asm`, or reads a global with no known address, is not exported.
    reasons = exports_left_out(analysed, unresolved)
    left_out = [{"name": e["name"], "address": e["address"], "reason": reasons[e["name"]]}
                for e in exports if e["name"] in reasons and e["address"] is not None]
    exports = [e for e in exports if e["name"] not in reasons and e["name"] != ASM_STUB]
    for index, entry in enumerate(exports):
        entry["wrapper"] = "decomp_call_%d" % index  # the shim numbers the wrappers it writes
    shim = out / "shim.c"
    instantiate = re.search(r"void wasm2c_mod_instantiate\(([^;]+)\);", header).group(1)
    startup = [name for name in ("__wasm_apply_data_relocs", "__wasm_call_ctors")
               if "w2c_mod_0x5F%s(" % name[1:] in header]
    generate_shim(shim, imports, exports, globals_, instantiate, host=host,
                  func_globals=func_globals, func_values=func_values, startup=startup)
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
    report["left_out_of_exports"] = left_out
    report["unresolved_globals"] = unresolved
    write_build_outputs(out, report, exports, globals_, global_values, imports, symbols)
    return report


def build_cranelift(decomp, out, units=None, limit=None, host=None, compiler=None, toolchain=None):
    """Native fallback build, used only when explicitly asked for.

    `compiler` can inject another compiler; the default is the host C compiler.
    No Cranelift compiler is part of this tool. The native module keeps the
    public ABI, exports, report and manifest, but it is not the wasm module with
    its address-indexed table, so a build never swaps to it on its own.
    """
    decomp, out = Path(decomp), Path(out)
    check_out_path(decomp, out)
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
    # The native path has no wasm GOT slots: native code binds symbols at link
    # time. The manifest still lists the symbol values so an engine reading
    # `globals` sees the same addresses whichever path built the library.
    symbols = collect_symbols(manifest, chosen)
    globals_ = sorted(symbols)
    global_values = [symbol_address(symbols, name) for name in globals_]
    report = make_report(chosen, outcomes, library, exports,
                         dict(zip(globals_, global_values)), rewrites)
    write_build_outputs(out, report, exports, globals_, global_values)
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
        # clang 19 closes the size line on the next one with nvsize/nvalign.
        found = re.search(r"sizeof=(\d+),\s*(?:dsize=\d+,\s*)?align=(\d+)", "\n".join(lines[1:]))
        if found:
            size = (int(found.group(1)), int(found.group(2)))
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
    values = [int(v, 0) if isinstance(v, str) else int(v)
              for v in data.get("global_values", [])]
    globals_array = (ctypes.c_uint32 * len(values))(*[v & 0xffffffff for v in values]) if values else None
    status = lib.decomp_init(None, GUEST_SIZE, data["table_entries"], globals_array, len(values))
    if status:
        raise RuntimeError("decomp_init failed: %d" % status)
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
        sub.add_argument("--gaps", help="a real_gaps.txt: also build the units that hold its addresses")
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
        if args.gaps:
            manifest = json.loads((Path(args.decomp) / "build" / "units.json").read_text())
            args.unit = list(args.unit) + gap_units(manifest.get("units", []),
                                                    read_gap_addresses(args.gaps))
        report = build(args.decomp, args.out, args.unit, args.limit,
                       download=download, force_fallback=args.cranelift)
        print("compiled %d, skipped %d; library %s" % (
            report["counts"]["compiled"], report["counts"]["skipped"], report["library"]))
    elif args.command == "layout":
        report = layout(args.decomp, args.out, args.unit, args.limit, download=download)
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
