#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Build a typed family's differential cases in a small crate of their own.

The typed cases live in crates/engine/tests, where `use engine as _;` assembles every
translation in the workspace. That table costs minutes to build even for one case.
This tool copies one suite into a throwaway crate under target/family-lab and
assembles only the part crates that hold the translations the suite names; cargo then
builds engine-core, engine-typed and those parts, not the engine facade. The copy is
generated from the suite, so the case list is the suite's own and stays in step with
the hygiene rule (scripts/fork/tests/test_family_lab.py checks both).

Usage:
    python3 scripts/fork/family_lab.py generate <family> <out-dir>
    python3 scripts/fork/family_lab.py families
"""
import os
import re
import sys
from pathlib import Path

FORK = Path(__file__).resolve().parent
REPO = FORK.parent.parent
TESTS = REPO / "crates" / "engine" / "tests"
PARTS = REPO / "crates"
MARKER = re.compile(r"// Translated from ([0-9a-fA-F]{8})")
ENTRY = re.compile(r"entry!\s*\(\s*0x([0-9a-fA-F_]+)")
HEX = re.compile(r"0x([0-9a-fA-F][0-9a-fA-F_]{7,})")
CRATE = re.compile(r"crates/(engine-[^/]+)/")


def translation_crates():
    """Exe address to (part crate, source file), from the translations' own markers."""
    by_address = {}
    for source in sorted(PARTS.glob("engine-*/src/**/*.rs")):
        match = CRATE.match(source.relative_to(REPO).as_posix())
        if not match:
            continue
        for address in MARKER.findall(source.read_text(encoding="utf-8", errors="replace")):
            by_address.setdefault(address.lower(), (match.group(1), source))
    return by_address


def typed_files():
    """Exe address to the typed source file that owns it."""
    by_address = {}
    for source in (REPO / "crates" / "engine-typed" / "src").rglob("*.rs"):
        for address in re.findall(
            r"// Typed from ([0-9a-fA-F]{8})", source.read_text(encoding="utf-8")
        ):
            by_address.setdefault(address.lower(), source)
    return by_address


def suite(family):
    """The typed test file of a family (its name after `typed_`)."""
    path = TESTS / f"typed_{family}.rs"
    if not path.is_file():
        raise ValueError(f"no typed suite named {family!r} in {TESTS}")
    return path


def assembly_for(path):
    """Part crates the suite's cases need, in a stable order.

    A case's translation reaches callees by address from its own unit; the typed
    version makes `typed_call!`s from the file holding its marker, including helpers
    beside the marked function. Reading those two files, and the suite itself, finds
    the addresses the lab must register: a missing one stops one side at an open
    function while the other runs on.
    """
    text = path.read_text(encoding="utf-8")
    by_address = translation_crates()
    typed = typed_files()
    addresses = set(HEX.findall(text))
    for case in ENTRY.findall(text):
        address = case.replace("_", "").lower()
        found = by_address.get(address)
        if found:
            addresses.update(HEX.findall(found[1].read_text(encoding="utf-8")))
        source = typed.get(address)
        if source:
            addresses.update(HEX.findall(source.read_text(encoding="utf-8")))
    crates = set()
    for address in addresses:
        found = by_address.get(address.replace("_", "").lower())
        if found and found[0] != "engine-core":
            crates.add(found[0])
    if "engine::dead_lights::install" in text:
        # The guard hook calls these two translations directly, without an address in the suite.
        crates.update(["engine-bs", "engine-fallout-unplaced-b"])
    # A callee reached through a vtable has no address in the text. When a case stops at
    # an open function, name the crate here and rerun; the full CI suite is still the proof.
    extra = os.environ.get("NV_FAMILY_LAB_PARTS", "")
    crates.update(name.strip() for name in extra.split(",") if name.strip())
    return sorted(crates)


def with_guard_helper(text):
    """Replace the lights suite's call into the engine facade with a local copy.

    `engine::dead_lights` is a hook in the facade, not a translation crate: it sweeps a
    dying light and then calls two translations. Copying that small hook into the lab
    keeps the suite's behaviour without linking the whole facade.
    """
    if "engine::dead_lights::install" not in text:
        return text
    hook = (REPO / "crates" / "engine" / "src" / "dead_lights.rs").read_text(encoding="utf-8")
    hook = hook.replace(
        "use crate::units::bsshader::run_00bb4520::fn_00bb4740;",
        "use engine_bs::units::bsshader::run_00bb4520::fn_00bb4740;",
    ).replace(
        "use crate::units::unplaced::run_00c048b0::fn_00c058f0;",
        "use engine_fallout_unplaced_b::units::unplaced::run_00c048b0::fn_00c058f0;",
    )
    module = "mod engine {\n    pub mod dead_lights {\n"
    module += "\n".join("        " + line for line in hook.splitlines()) + "\n"
    module += "    }\n}\n\n"
    return module + text


def rewrite_source(text, parts):
    """Assemble only this family's parts in place of the facade's whole table."""
    functions = ", ".join(f"{crate.replace('-', '_')}::units::funcs" for crate in parts)
    assemble = f"engine_core::assemble!({functions});"
    if text.count("use engine as _;") != 1:
        raise ValueError("the suite does not link the engine facade in the expected way")
    text = text.replace("use engine as _;", assemble)
    return with_guard_helper(text)


def manifest(family, parts, out):
    """A standalone package: path dependencies, no workspace inheritance."""
    lines = [
        "[package]",
        f'name = "typed-lab-{family}"',
        'version = "0.0.0"',
        'edition = "2021"',
        'license = "GPL-3.0-only"',
        "publish = false",
        "",
        "# Kept out of the workspace: this crate is generated, not a member.",
        "[workspace]",
        "",
        "[dependencies]",
    ]
    deps = [("engine-core", 'features = ["test-support"]')] + [
        ("engine-typed", "")
    ] + [(crate, "") for crate in parts]
    for name, options in deps:
        path = os.path.relpath(REPO / "crates" / name, out)
        lines.append(f'{name} = {{ path = "{path}"{", " + options if options else ""} }}')
    lines += [
        "",
        "[[test]]",
        f'name = "typed_{family}"',
        f'path = "tests/typed_{family}.rs"',
        "",
    ]
    return "\n".join(lines)


def generate(path, out):
    """Write a lab crate for one typed suite into `out`; return its manifest path."""
    path = Path(path)
    out = Path(out)
    family = path.stem.removeprefix("typed_")
    parts = assembly_for(path)
    text = rewrite_source(path.read_text(encoding="utf-8"), parts)
    (out / "tests").mkdir(parents=True, exist_ok=True)
    target = out / "tests" / path.name
    manifest_path = out / "Cargo.toml"
    files = ((target, text), (manifest_path, manifest(family, parts, out)))
    for file, content in files:
        # Leave the timestamp alone when nothing changed, so a warm run does not rebuild.
        if not file.exists() or file.read_text(encoding="utf-8") != content:
            file.write_text(content, encoding="utf-8")
    return manifest_path


def main(argv):
    if len(argv) == 2 and argv[1] == "families":
        for path in sorted(TESTS.glob("typed_*.rs")):
            print(f"{path.stem.removeprefix('typed_'):20} {', '.join(assembly_for(path))}")
        return 0
    if len(argv) == 4 and argv[1] == "generate":
        path = suite(argv[2])
        print(generate(path, Path(argv[3])))
        return 0
    print(__doc__.strip(), file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
