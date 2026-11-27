#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Inventory of built-in extender commands that read fields of families scheduled to move (ADR-0014).

  extender_move_inventory.py [--check | --out FILE]

Writes the Markdown table per family (default ``docs/fork/EXTENDER-MOVE-INVENTORY.md``), grouped by
the family each field belongs to, with the command, field, offset, access kind and source line.
``--check`` writes nothing and fails when the committed inventory differs, when a pinned opcode
base or co-save id differs, or when a command accesses a moved field directly. A family that has
moved has its fields marked ``moved``: a direct read or write of one of them is a problem, while a
call to the family's typed function at its exe address is not (ADR-0014, "Not decided here"). Tests
plant both cases in a temporary crate.

The reader looks at the built-in crates' source, not at game data, recordings or decompilations. It
follows a field's source spelling and any local name bound to it, reads statements rather than
single lines (an access can be split), and ignores line comments. A helper's read is reported
against every registered command handler that reaches it, so a command that reads a field through
`invref.rs` is inventoried as that command, not as the helper. The enclosing function is kept in
the message for provenance.

Stdlib only; Python 3.11 or newer.
"""
from __future__ import annotations

import argparse
import re
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
OUT = REPO / "docs" / "fork" / "EXTENDER-MOVE-INVENTORY.md"

# ADR-0014 names LockFreeMap as the pilot of the families that can really move; it goes first.
FAMILY_ORDER = ("lock-free maps", "Havok", "forms", "cell references", "extra data", "geometry", "animation keys")


@dataclass(frozen=True)
class Field:
    """A field of a family a built-in command may read.

    `token` is its source spelling in the built-in crates or in the typed layout that names the
    class; `offset` is its PC class offset. `aliases` are other spellings of the same class field.
    """

    family: str
    name: str
    token: str
    offset: int
    moved: bool = False
    aliases: tuple[str, ...] = ()

    @property
    def tokens(self) -> tuple[str, ...]:
        return (self.token,) + self.aliases


# The pilot's moved fields are those of MAP_STUB.moved in lockfree/store.rs, at the offsets of
# LockFreeMap in engine-typed/src/lockfree/layout.rs. The vtable, bucket count, count and lock do
# not move (ADR-0014, "Not decided here"), so they are not listed here.
FIELDS = (
    Field("lock-free maps", "node slots", "pReferencedNodes", 0x04, moved=True),
    Field("lock-free maps", "buckets", "pBuckets", 0x0c, moved=True),
    Field("lock-free maps", "delete batch size", "iDeleteBatchSize", 0x10, moved=True),
    Field("lock-free maps", "interface manager", "pInterfaceManager", 0x14, moved=True),
    # Havok: bhkNiCollisionObject and hkpRigidBody fields (jip's havok_refs.rs).
    Field("Havok", "collision object", "COLLISION_OBJECT", 0x1c, aliases=("KEY_COLLISION_OBJECT",)),
    Field("Havok", "havok object", "HAVOK_OBJECT", 0x08),
    Field("Havok", "world object", "WORLD_OBJECT", 0x10),
    Field("Havok", "contact bodies", "CONTACT_BODIES", 0x3b4),
    Field("Havok", "mass inverse", "MASS_INV", 0x1ac),
    # Forms: a reference's form fields, read by JIP and xNVSE's built-ins.
    Field("forms", "base form", "REFR_BASE_FORM", 0x20),
    Field("forms", "actor base flags", "ACTOR_BASE_FLAGS", 0x34),
    Field("forms", "form type", "TESForm::cFormType", 0x04),
    Field("forms", "form id", "TESForm::iFormID", 0x0c),
    # Cell references: a cell's extras and a reference's loaded data.
    Field("cell references", "cell extra list", "CELL_EXTRA_LIST", 0x28),
    Field("cell references", "cell flags", "CELL_FLAGS", 0x24),
    Field("cell references", "noise texture path", "NOISE_TEXTURE_PATH", 0x5c),
    Field("cell references", "loaded data", "LOADED_DATA", 0x64),
    # Extra data: the list's chain head and type bitmap, and a node's next and first payload word
    # (nvse's invref.rs and misc_ref.rs, jip's extra data commands). Not moved yet (nv-rs#952).
    Field("extra data", "chain head", "EXTRA_DATA", 0x04),
    Field("extra data", "type bitmap", "EXTRA_PRESENCE", 0x08),
    Field("extra data", "node next", "EXTRA_NEXT", 0x08),
    Field("extra data", "node payload word", "EXTRA_VALUE", 0x0c),
    # Geometry: the scene graph's node fields.
    Field("geometry", "first-person node", "NODE_1ST_PERSON", 0x694),
    # Animation keys: the actor's animation process fields.
    Field("animation keys", "process level", "PROCESS_LEVEL", 0x28),
    Field("animation keys", "character controller", "CHAR_CONTROLLER", 0x138),
)

CRATES = (
    "nvse",
    "jip",
    "johnnyguitar",
    "jg",
    "knvse",
    "showoff",
    "ttw-nvse",
    "anh-nvse",
    "mcm",
    "stewie",
    "uio",
)

# Opcode bases and co-save ids stay fixed so mods and saves keep working. Each pin reads the
# binding from the source that declares it; a changed source or a changed pin is a problem.
@dataclass(frozen=True)
class Pin:
    extender: str
    kind: str
    file: str
    pattern: str
    expected: str


PINS = (
    Pin("xNVSE", "opcode base", "crates/nvse/src/names.rs", r"FIRST_OPCODE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x1400"),
    Pin("xNVSE", "co-save id", "crates/nvse/src/cosave.rs", r"kind:\s*tag\(b\"(MODS)\"\)", "MODS"),
    Pin("JIP LN", "opcode base", "crates/jip/src/table.rs", r"FIRST_OPCODE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x2200"),
    Pin("JIP LN", "co-save id", "crates/jip/src/cosave.rs", r"SCRIPT_VARS:\s*u32\s*=\s*tag\(b\"(VSPJ)\"\)", "VSPJ"),
    Pin("JohnnyGuitar", "opcode base", "crates/jg/src/identity.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x3100"),
    Pin("JohnnyGuitar", "co-save id", "crates/jg/src/cosave.rs", r"STATS_KIND:\s*u32\s*=\s*tag\(b\"(NVXS)\"\)", "NVXS"),
    Pin("ShowOff", "opcode base", "crates/showoff/src/identity.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x3c93"),
    Pin("kNVSE", "opcode base", "crates/knvse/src/identity.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x3920"),
    Pin("TTW", "opcode base", "crates/ttw-nvse/src/lib.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x3800"),
    Pin("AnhNVSE", "opcode base", "crates/anh-nvse/src/lib.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x3600"),
    Pin("lStewieAl", "opcode base", "crates/stewie/src/lib.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x2132"),
    Pin("lStewieAl", "co-save id", "crates/stewie/src/cosave.rs", r"AMMO_ID:\s*u32\s*=\s*tag\(b\"(AMID)\"\)", "AMID"),
    Pin("MCM", "opcode base", "crates/mcm/src/lib.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x21c0"),
    Pin("UIO", "opcode base", "crates/uio/src/lib.rs", r"opcode_base:\s*(\d+)", "0"),
)

# A field used in an expression, not where its constant or import is declared.
MARKS = ("+", "wrapping_add(", ".u8(", ".u16(", ".u32(", ".f32(", ".get(", "rd32(", "ptr(")
WRITES = (".set_u8(", ".set_u16(", ".set_u32(", ".set_f32(", ".store(")
DECLARATIONS = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?(?:const|static|use)\b|^#|^//!?")
FUNCTION = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|unsafe\s+)*fn\s+([A-Za-z_]\w*)")
HANDLER = re.compile(r"\(\s*(0x[0-9a-f_]+)\s*,\s*([A-Za-z_]\w*)\s*\)")
EXECUTE = re.compile(r"\bexecute:\s*([A-Za-z_]\w*)")
CALL_PATH = re.compile(r"(?<![\w.])((?:[A-Za-z_]\w*::)*[A-Za-z_]\w*)\s*\(")
IDENT = re.compile(r"\b([A-Za-z_]\w*)\b")


@dataclass(frozen=True)
class Finding:
    family: str
    field: str
    token: str
    offset: int
    moved: bool
    access: str
    crate: str
    file: str
    line: int
    function: str
    command: str

    @property
    def source(self) -> str:
        return f"crates/{self.crate}/src/{self.file}:{self.line}"


def without_comment(line: str) -> str:
    """Source text before a trailing `//` note; comments are not code."""
    return line.split("//", 1)[0]


def functions(lines: list[str]) -> list[tuple[str, int, int]]:
    """(name, first line, last line) of each function, by brace depth, not a backwards `fn` search."""
    spans: list[tuple[str, int, int]] = []
    depth = 0
    current: tuple[str, int, int] | None = None
    for at, raw in enumerate(lines, 1):
        line = without_comment(raw)
        match = FUNCTION.match(line) if current is None else None
        if match:
            current = (match.group(1), at, depth)
        depth += line.count("{") - line.count("}")
        if current and depth <= current[2]:
            spans.append((current[0], current[1], at))
            current = None
            depth = max(depth, 0)
    return spans


def statements(lines: list[str]):
    """(first line, joined text) of each statement; a read split over lines stays one statement."""
    parts: list[str] = []
    first = 0
    for at, raw in enumerate(lines, 1):
        line = without_comment(raw).strip()
        if not line:
            if parts:
                yield first, " ".join(parts)
                parts = []
            continue
        if not parts:
            first = at
        parts.append(line)
        if line.endswith((";", "{", "}", ",")):
            yield first, " ".join(parts)
            parts = []
    if parts:
        yield first, " ".join(parts)


def aliases(lines: list[str], tokens: set[str]) -> dict[str, str]:
    """Local names bound to a tracked token by an import or an assignment."""
    out: dict[str, str] = {}
    for _, text in statements(lines):
        if DECLARATIONS.match(text) and "use " in text:
            for name in IDENT.findall(text):
                if name in tokens and name not in ("use", "crate", "super", "self"):
                    out[name] = name
                elif " as " in text and name in tokens:
                    out[name] = name
            for old, new in re.findall(r"\b([A-Za-z_]\w*)\s+as\s+([A-Za-z_]\w*)", text):
                if old in tokens:
                    out[new] = old
            continue
        match = re.match(r"^(?:const|let)\s+([A-Za-z_]\w*)\b[^=;]*=\s*([^;]*)", text)
        if match:
            right = match.group(2).strip()
            # Only a plain binding (`let at = HEAD;`), not a walk that happens to pass through
            # a field: a name from a chain is not an alias of the field it reads.
            for token in tokens:
                if right == token or right.endswith("::" + token):
                    out[match.group(1)] = token
                    break
    return out


def sources(root: Path, crates):
    """The built-in crates' source files, excluding their own test-only sources."""
    out: dict[str, dict[str, list[str]]] = {}
    for crate in crates:
        src = root / "crates" / crate / "src"
        if not src.is_dir():
            continue
        files: dict[str, list[str]] = {}
        for path in sorted(src.rglob("*.rs")):
            relative = str(path.relative_to(src))
            if relative.endswith("_tests.rs") or relative == "slots.rs" or path.name == "tests.rs":
                continue
            files[relative] = path.read_text().splitlines()
        out[crate] = files
    return out


def module_of(file: str) -> str:
    """A source file's module path, so `invref::entry_list` resolves to `invref.rs`."""
    parts = file.split("/")
    if parts[-1] == "mod.rs":
        parts.pop()
    else:
        parts[-1] = parts[-1][: -len(".rs")]
    return "::".join(parts)


def imports(lines: list[str]) -> dict[str, str]:
    """Imported name to the module it came from, for calls written without a path."""
    out: dict[str, str] = {}
    for _, text in statements(lines):
        if not text.startswith("use "):
            continue
        body = text[4:].strip().rstrip(";").strip()
        group = re.match(r"([A-Za-z_]\w*(?:::\w+)*)::\{([^}]*)\}$", body)
        if group:
            prefix = group.group(1)
            for item in group.group(2).split(","):
                parts = [p.strip() for p in item.split(" as ")]
                if not parts[0]:
                    continue
                if parts[0] == "self":
                    # `use crate::invref::{self, …}` makes `invref` a local module name.
                    out[prefix.split("::")[-1]] = prefix
                    continue
                out[parts[-1]] = prefix
            continue
        plain = re.match(r"([A-Za-z_]\w*(?:::\w+)*)\s*(?:as\s+(\w+))?$", body)
        if plain:
            path = plain.group(1)
            last = path.split("::")[-1]
            if plain.group(2) and last[:1].islower():
                # `use super::havok_refs as h;` aliases the module, not a function in it.
                out[plain.group(2)] = path
            else:
                out[plain.group(2) or last] = "::".join(path.split("::")[:-1])
    return out


def call_graph(files: dict[str, list[str]], spans, defs: set[tuple[str, str]]):
    """Call edges between (file, function) pairs, resolved by module rather than by bare name."""
    modules = {file: module_of(file) for file in files}
    imported = {file: imports(lines) for file, lines in files.items()}
    once: dict[str, str] = {}
    for file, name in defs:
        if name in once:
            once[name] = ""  # ambiguous: a bare call needs a path or an import
        else:
            once[name] = file
    once = {name: file for name, file in once.items() if file}

    def module_files(path: str):
        suffix = "::".join(p for p in path.split("::") if p not in ("super", "crate", "self"))
        return [f for f, m in modules.items() if m == suffix or m.endswith("::" + suffix)]

    def resolve(path: str, file: str):
        parts = path.split("::")
        name = parts[-1]
        if len(parts) > 1:
            prefix = "::".join(parts[:-1])
            if parts[0] in imported[file]:
                alias = imported[file][parts[0]]
                prefix = alias + ("::" + "::".join(parts[1:-1]) if len(parts) > 2 else "")
            candidates = module_files(prefix)
        elif (file, name) in defs:
            return (file, name)
        elif name in imported[file]:
            candidates = module_files(imported[file][name])
        elif name in once:
            return (once[name], name)
        else:
            return None
        return next(((f, name) for f in candidates if (f, name) in defs), None)

    edges: dict[tuple[str, str], set[tuple[str, str]]] = defaultdict(set)
    reverse: dict[tuple[str, str], set[tuple[str, str]]] = defaultdict(set)
    for file, lines in files.items():
        for name, first, last in spans[file]:
            caller = (file, name)
            for line in lines[first - 1 : last]:
                for path in CALL_PATH.findall(without_comment(line)):
                    callee = resolve(path, file)
                    if callee and callee != caller:
                        edges[caller].add(callee)
                        reverse[callee].add(caller)
    return edges, reverse


def handlers(files: dict[str, list[str]], defs: set[tuple[str, str]]) -> set[tuple[str, str]]:
    """Functions registered in a command table as `(0x…, handler)` or `cmd(0x…, …, handler)`."""
    out: set[tuple[str, str]] = set()
    for file, lines in files.items():
        text = "\n".join(lines)
        # A command table uses a small opcode; an exe-address hook site does not count as one.
        out.update(
            (file, name)
            for opcode, name in HANDLER.findall(text)
            if int(opcode.replace("_", ""), 16) < 0x10000 and (file, name) in defs
        )
        out.update((file, name) for name in EXECUTE.findall(text) if (file, name) in defs)
        for call in re.findall(r"\bcmd\s*\((.*?)\)", text, re.S):
            for name in IDENT.findall(call):
                if (file, name) in defs and name not in ("cmd", "Some", "None"):
                    out.add((file, name))
                    break
    return out


def scan(root: Path, fields=FIELDS, crates=CRATES) -> list[Finding]:
    """Every tracked field read of a registered built-in command, in crate and source order."""
    by_token = {token: field for field in fields for token in field.tokens}
    tokens = set(by_token)
    files = sources(root, crates)
    out: list[Finding] = []
    for crate, crate_files in files.items():
        spans = {f: functions(lines) for f, lines in crate_files.items()}
        defs = {(file, name) for file, fns in spans.items() for name, _, _ in fns}
        commands = handlers(crate_files, defs)
        _, reverse = call_graph(crate_files, spans, defs)
        reachers: dict[tuple[str, str], set[tuple[str, str]]] = {}

        def reaching(key: tuple[str, str]) -> set[tuple[str, str]]:
            if key not in reachers:
                seen: set[tuple[str, str]] = set()
                stack = [key]
                while stack:
                    for caller in reverse.get(stack.pop(), ()):
                        if caller not in seen:
                            seen.add(caller)
                            stack.append(caller)
                reachers[key] = seen
            return reachers[key]

        for file, lines in crate_files.items():
            local = aliases(lines, tokens)
            for first, text in statements(lines):
                if DECLARATIONS.match(text):
                    continue
                found = {token for token in tokens if re.search(r"\b" + re.escape(token) + r"\b", text)}
                for name, token in local.items():
                    if re.search(r"\b" + re.escape(name) + r"\b", text):
                        found.add(token)
                if not found:
                    continue
                direct = any(re.search(r"\.\s*" + re.escape(token) + r"\b", text) for token in found)
                if not direct and not any(mark in text for mark in MARKS):
                    continue
                at = next(((name, a, b) for name, a, b in spans[file] if a <= first <= b), None)
                if at is None:
                    continue
                key = (file, at[0])
                for command in [key] if key in commands else sorted(commands & reaching(key)):
                    for token in sorted(found):
                        field = by_token[token]
                        out.append(
                            Finding(
                                field.family,
                                field.name,
                                token,
                                field.offset,
                                field.moved,
                                "write" if any(mark in text for mark in WRITES) else "read",
                                crate,
                                file,
                                first,
                                at[0],
                                command[1],
                            )
                        )
    return sorted(out, key=lambda f: (f.crate, f.file, f.line, f.token, f.command))


def problems(root: Path, fields=FIELDS, crates=CRATES) -> list[str]:
    """Direct accesses to moved fields: the guard ADR-0014 puts on built-in commands."""
    return [
        f"{f.source}: {f.family}'s moved field {f.field} (+0x{f.offset:x}) is {f.access} "
        f"directly in `{f.function}` (command `{f.command}`); reach it through the family's typed "
        "function at its exe address"
        for f in scan(root, fields, crates)
        if f.moved
    ]


def pin_problems(root: Path, pins=PINS) -> list[str]:
    """Pins whose binding changed, was re-tagged, or whose source no longer declares the id."""
    out = []
    for pin in pins:
        path = root / pin.file
        if not path.is_file():
            out.append(f"{pin.extender}'s {pin.kind} source {pin.file} is missing, expected `{pin.expected}`")
            continue
        found = re.search(pin.pattern, path.read_text())
        value = found.group(1) if found else None
        if value != pin.expected:
            out.append(
                f"{pin.extender}'s {pin.kind} is `{value}` in {pin.file}, expected `{pin.expected}`"
            )
    return out


def generate(root: Path, fields=FIELDS, crates=CRATES, pins=PINS) -> str:
    """The inventory, generated from source; a changed source or pin shows up as a diff."""
    found = scan(root, fields, crates)
    lines = [
        "<!-- SPDX-License-Identifier: GPL-3.0-only -->",
        "# Built-in extender reads of families scheduled to move",
        "",
        "Generated by `python3 scripts/fork/extender_move_inventory.py --out "
        "docs/fork/EXTENDER-MOVE-INVENTORY.md`. Do not edit by hand.",
        "",
        "The field offsets and the command names come from the built-in crates' source and "
        "engine-typed's class layout (issue #916, ADR-0014). A family that has moved holds poison "
        "in these fields: a command must reach the object through its virtual methods or the "
        "family's typed functions at their exe addresses, not by reading or writing them. The "
        "tracked fields are the `FIELDS` table in the generator; add a token there when a family's "
        "move is planned. Fields marked `moved` fail the guard when a command accesses one "
        "directly: the lock-free maps are ADR-0014's pilot and are marked. A read through a helper "
        "is listed against the registered command that calls it.",
        "",
        "## Opcode bases and co-save ids",
        "",
        "Pinned so mods and saves keep working; a change fails `--check`.",
        "",
        "| Extender | Kind | Id | Declared in |",
        "| --- | --- | --- | --- |",
    ]
    for pin in pins:
        lines.append(f"| {pin.extender} | {pin.kind} | `{pin.expected}` | `{pin.file}` |")
    for family in FAMILY_ORDER:
        lines += ["", f"## {family}", "", "| Command | Field | Offset | Access | Source |", "| --- | --- | --- | --- | --- |"]
        rows = [f for f in found if f.family == family]
        if not rows:
            lines.append("| _none found_ | | | | |")
        for f in rows:
            lines.append(
                f"| `{f.command}` | `{f.field}` | `+0x{f.offset:x}` | {f.access} | `{f.source}` |"
            )
    return "\n".join(lines) + "\n"


def check(root: Path) -> list[str]:
    """What is wrong with the committed inventory, as text lines; empty when it is current."""
    out = pin_problems(root) + problems(root)
    text = generate(root)
    if not OUT.is_file():
        out.append(f"{OUT.relative_to(root)} is missing: run the generator with --out")
    elif OUT.read_text() != text:
        out.append(f"{OUT.relative_to(root)} is stale: run the generator with --out")
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--check", action="store_true", help="fail on a stale inventory, a changed pin or a direct read of a moved field")
    group.add_argument("--out", type=Path, help="write the inventory to this file")
    args = parser.parse_args()
    if args.check:
        problems_found = check(REPO)
        for line in problems_found:
            print(line)
        return 1 if problems_found else 0
    text = generate(REPO)
    if args.out:
        args.out.write_text(text)
        print(f"wrote {args.out}")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
