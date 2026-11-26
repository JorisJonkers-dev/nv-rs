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

The reader looks at the built-in crates' command source, not at game data, recordings or
decompilations. A field is named by its token in that source and by the offset the token is
defined with; a line counts when it uses the token in an expression (a member access, a pointer
walk or a memory read or write), not where the constant is declared or imported. The command is
the enclosing function.

Stdlib only; Python 3.11 or newer.
"""
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
OUT = REPO / "docs" / "fork" / "EXTENDER-MOVE-INVENTORY.md"

# The move order from ADR-0014 and issue #916: Havok first, then the heap-heavy families. The
# lock-free maps are the pilot; their fields move under `lockfree:moved` and `:freed`.
FAMILY_ORDER = ("Havok", "forms", "cell references", "geometry", "animation keys", "lock-free maps")


@dataclass(frozen=True)
class Field:
    """A field of a family that a built-in command may read; `token` is its source spelling."""

    family: str
    name: str
    token: str
    offset: int
    moved: bool = False


# Offsets are those the constants are declared with in the source files the token comes from.
FIELDS = (
    # Havok: bhkNiCollisionObject and hkpRigidBody fields (jip's havok_refs.rs).
    Field("Havok", "collision object", "COLLISION_OBJECT", 0x1c),
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
# value from the source that declares it; a changed source or a changed pin is a problem.
@dataclass(frozen=True)
class Pin:
    extender: str
    kind: str
    file: str
    pattern: str
    expected: str


PINS = (
    Pin("xNVSE", "opcode base", "crates/nvse/src/names.rs", r"FIRST_OPCODE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x1400"),
    Pin("xNVSE", "co-save id", "crates/nvse/src/cosave.rs", r'tag\(b"(MODS)"\)', "MODS"),
    Pin("JIP LN", "opcode base", "crates/jip/src/table.rs", r"FIRST_OPCODE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x2200"),
    Pin("JIP LN", "co-save id", "crates/jip/src/cosave.rs", r'tag\(b"(VSPJ)"\)', "VSPJ"),
    Pin("JohnnyGuitar", "opcode base", "crates/jg/src/identity.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x3100"),
    Pin("JohnnyGuitar", "co-save id", "crates/jg/src/cosave.rs", r'tag\(b"(NVXS)"\)', "NVXS"),
    Pin("ShowOff", "opcode base", "crates/showoff/src/identity.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x3c93"),
    Pin("kNVSE", "opcode base", "crates/knvse/src/identity.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x3920"),
    Pin("TTW", "opcode base", "crates/ttw-nvse/src/lib.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x3800"),
    Pin("AnhNVSE", "opcode base", "crates/anh-nvse/src/lib.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x3600"),
    Pin("lStewieAl", "opcode base", "crates/stewie/src/lib.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x2132"),
    Pin("lStewieAl", "co-save id", "crates/stewie/src/cosave.rs", r'tag\(b"(AMID)"\)', "AMID"),
    Pin("MCM", "opcode base", "crates/mcm/src/lib.rs", r"OPCODE_BASE:\s*u32\s*=\s*(0x[0-9a-f_]+)", "0x21c0"),
    Pin("UIO", "opcode base", "crates/uio/src/lib.rs", r"opcode_base:\s*(\d+)", "0"),
)

# A token in an expression, not a declaration or an import.
MARKS = ("+", "wrapping_add(", ".u8(", ".u16(", ".u32(", ".f32(", ".get(", "rd32(", "ptr(")
WRITES = (".set_u8(", ".set_u16(", ".set_u32(", ".set_f32(", ".store(")
DECLARATIONS = ("const ", "pub const ", "use ", "pub use ", "//", "*", "#")
FUNCTION = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+([A-Za-z_]\w*)")


@dataclass(frozen=True)
class Finding:
    family: str
    field: str
    token: str
    offset: int
    access: str
    crate: str
    file: str
    line: int
    function: str

    @property
    def source(self) -> str:
        return f"crates/{self.crate}/src/{self.file}:{self.line}"


def enclosing(lines: list[str], at: int) -> str:
    """The name of the function the line is in, or `<module>` outside one."""
    for line in reversed(lines[:at]):
        match = FUNCTION.match(line)
        if match:
            return match.group(1)
    return "<module>"


def scan(root: Path, fields=FIELDS, crates=CRATES) -> list[Finding]:
    """Every use of a listed field in a built-in command's source, in file and line order."""
    out: list[Finding] = []
    for crate in crates:
        src = root / "crates" / crate / "src"
        if not src.is_dir():
            continue
        for path in sorted(src.rglob("*.rs")):
            relative = path.relative_to(src)
            if not any("command" in part for part in relative.parts):
                continue
            if path.name.endswith("_tests.rs") or path.name == "slots.rs":
                continue
            lines = path.read_text().splitlines()
            for at, line in enumerate(lines, 1):
                stripped = line.strip()
                if not stripped or stripped.startswith(DECLARATIONS):
                    continue
                for field in fields:
                    if not re.search(r"\b" + re.escape(field.token) + r"\b", line):
                        continue
                    if not any(mark in line for mark in MARKS):
                        continue
                    out.append(
                        Finding(
                            field.family,
                            field.name,
                            field.token,
                            field.offset,
                            "write" if any(mark in line for mark in WRITES) else "read",
                            crate,
                            str(path.relative_to(src)),
                            at,
                            enclosing(lines, at),
                        )
                    )
    return out


def problems(root: Path, fields=FIELDS, crates=CRATES) -> list[str]:
    """Direct accesses to moved fields: the guard ADR-0014 puts on built-in commands."""
    return [
        f"{f.source}: {f.family}'s moved field {f.field} (+0x{f.offset:x}) is {f.access} "
        f"directly in `{f.function}`; reach it through the family's typed function at its exe address"
        for f in scan(root, fields, crates)
        if any(field.moved and field.token == f.token for field in fields)
    ]


def pin_problems(root: Path, pins=PINS) -> list[str]:
    """Pins whose source value has changed, or whose source no longer declares the id."""
    out = []
    for pin in pins:
        text = (root / pin.file).read_text()
        found = re.search(pin.pattern, text)
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
        "The field offsets and the command names come from the built-in crates' command source "
        "(issue #916, ADR-0014). A family that has moved holds poison in these fields: a command "
        "must reach the object through its virtual methods or the family's typed functions at "
        "their exe addresses, not by reading or writing them. The tracked fields are the `FIELDS` "
        "table in the generator; add a token there when a family's move is planned. Fields marked "
        "`moved` fail the guard when a command accesses one directly; none is marked yet.",
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
        for f in sorted(rows, key=lambda r: (r.crate, r.file, r.line, r.token)):
            lines.append(
                f"| `{f.function}` | `{f.field}` | `+0x{f.offset:x}` | {f.access} | `{f.source}` |"
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
