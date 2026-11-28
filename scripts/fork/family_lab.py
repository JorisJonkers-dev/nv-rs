#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Build a typed family's differential cases in a small crate of their own.

The typed cases live in crates/engine/tests, where `use engine as _;` assembles every
translation in the workspace. That table costs minutes to build even for one case.
This tool copies one suite into a throwaway crate under target/family-lab and
assembles only the part crates that hold the translations the suite's cases actually
call; cargo then builds engine-core, engine-typed and those parts, not the engine
facade. The copy is generated from the suite, so the case list is the suite's own and
stays in step with the hygiene rule (scripts/fork/tests/test_family_lab.py checks
both).

Usage:
    python3 scripts/fork/family_lab.py run <family> [libtest filter ...]
    python3 scripts/fork/family_lab.py families
    python3 scripts/fork/family_lab.py generate <family> <out-dir>

`run` is the portable entry point. It sends the cargo arguments to the build
daemon's client command (scripts/fork/build_daemon.py), which starts the daemon if
needed and owns cargo under the shared limits. The limits are configuration, so
nothing about one machine is built into this tool. Set NV_FAMILY_LAB_DIRECT=1 to
run plain cargo with the optional NV_FAMILY_LAB_CARGO wrapper and NV_FAMILY_LAB_LOCK
instead (see docs/fork/FAMILY-LAB.md).
"""
import os
import re
import shlex
import subprocess
import sys
from functools import lru_cache
from pathlib import Path

FORK = Path(__file__).resolve().parent
REPO = FORK.parent.parent
TESTS = REPO / "crates" / "engine" / "tests"
PARTS = REPO / "crates"
MARKER = re.compile(r"// Translated from ([0-9a-fA-F]{8})")
TYPED_MARKER = re.compile(r"// Typed from ([0-9a-fA-F]{8})")
ENTRY = re.compile(r"entry!\s*\(\s*0x([0-9a-fA-F_]+)")
CALL = re.compile(r"\.call\(\s*([A-Za-z_][A-Za-z0-9_]*|0x[0-9a-fA-F_]+)")
TYPED_CALL = re.compile(r"typed_call!\(\s*[A-Za-z_][A-Za-z0-9_:]*\s*,\s*(0x[0-9a-fA-F_]+)")
CONST = re.compile(r"const\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*u32\s*=\s*0x([0-9a-fA-F_]+)")
FN = re.compile(r"^\s*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(", re.MULTILINE)
FN_NAME = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(")
INCLUDE = re.compile(r'include!\(\s*"([^"]+)"')
CRATE = re.compile(r"crates/(engine-[^/]+)/")
PART_PATH = re.compile(r"\b(engine_[a-z0-9]+(?:_[a-z0-9]+)*)::")
NOT_PARTS = {"engine-core", "engine-typed"}
SOURCE_CACHE = {}


def normal(address):
    """An exe address as eight lower-case hex digits, without separators."""
    return address.replace("_", "").lower()


@lru_cache(maxsize=1)
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


@lru_cache(maxsize=1)
def typed_files():
    """Exe address to the typed source file that owns it."""
    by_address = {}
    for source in (REPO / "crates" / "engine-typed" / "src").rglob("*.rs"):
        for address in TYPED_MARKER.findall(source.read_text(encoding="utf-8")):
            by_address.setdefault(address.lower(), source)
    return by_address


def source_info(path):
    """A source file's text, its u32 constants and its top-level functions.

    Kept small and local: the lab needs the call graph of the functions under test,
    not a parse of Rust. A function ends where the next one starts, which is enough
    to keep calls from unrelated functions in the same file out of the assembly.
    """
    if path not in SOURCE_CACHE:
        text = path.read_text(encoding="utf-8", errors="replace")
        functions = [(m.group(1), m.start(), m.end()) for m in FN.finditer(text)]
        constants = {name: normal(value) for name, value in CONST.findall(text)}
        SOURCE_CACHE[path] = (text, functions, constants)
    return SOURCE_CACHE[path]


@lru_cache(maxsize=None)
def callees_of(path, address):
    """Addresses a marked function calls, including the helpers beside it.

    A translation reaches other translations with `e.call(ADDRESS_OR_CONSTANT, ...)`;
    typed code reaches them with `typed_call!(FAMILY, ADDRESS, ...)`. Data addresses
    and float bit patterns are not calls, so they never drag in another crate. Local
    Rust calls stay in the same file and are followed only to collect their calls.
    """
    text, functions, constants = source_info(path)
    marks = [m for m in MARKER.finditer(text) if m.group(1).lower() == address]
    marks += [m for m in TYPED_MARKER.finditer(text) if m.group(1).lower() == address]
    if not marks:
        return set()
    after = min(m.end() for m in marks)
    start = min((s for name, s, _ in functions if s >= after), default=None)
    if start is None:
        return set()
    by_name = {name: (s, e) for name, s, e in functions}
    positions = sorted((s, name) for name, s, _ in functions)
    pending = [min((s, name) for s, name in positions if s >= start)[1]]
    seen = set()
    addresses = set()
    while pending:
        name = pending.pop()
        if name in seen or name not in by_name:
            continue
        seen.add(name)
        begin, end = by_name[name]
        body_end = min((s for s, _ in positions if s > begin), default=len(text))
        body = text[end:body_end]
        for call in CALL.findall(body) + TYPED_CALL.findall(body):
            value = call[2:] if call.startswith("0x") else constants.get(call)
            if value:
                addresses.add(normal(value))
        for name2 in FN_NAME.findall(body):
            if name2 in by_name and name2 not in seen:
                pending.append(name2)
    return addresses


def suite(family):
    """The typed test file of a family (its name after `typed_`)."""
    path = TESTS / f"typed_{family}.rs"
    if not path.is_file():
        raise ValueError(f"no typed suite named {family!r} in {TESTS}")
    return path


def assembly_for(path):
    """Part crates the suite's cases reach, in a stable order.

    Start at every `entry!` address in the suite, then add the addresses the marked
    translation and typed function at each one calls (`e.call` and `typed_call!`).
    Those are the translations the two sides can reach at run time; following their
    callees as well would pull in most of the engine through call chains the cases
    double, ignore or never take. A case that does reach a deeper callee names its
    crate in NV_FAMILY_LAB_PARTS.
    """
    text = path.read_text(encoding="utf-8")
    by_address = translation_crates()
    typed = typed_files()
    addresses = {normal(a) for a in ENTRY.findall(text)}
    for address in list(addresses):
        found = by_address.get(address)
        if found:
            addresses.update(callees_of(found[1], address))
        source = typed.get(address)
        if source:
            addresses.update(callees_of(source, address))
    crates = set()
    for address in addresses:
        found = by_address.get(address)
        if found and found[0] != "engine-core":
            crates.add(found[0])
    for named in PART_PATH.findall(text):
        # A suite that calls a part crate by path needs it linked whether or not an entry reaches it.
        crate = named.replace("_", "-")
        if crate not in NOT_PARTS and (PARTS / crate).is_dir():
            crates.add(crate)
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
    replacements = (
        (
            "use crate::units::bsshader::run_00bb4520::fn_00bb4740;",
            "use engine_bs::units::bsshader::run_00bb4520::fn_00bb4740;",
        ),
        (
            "use crate::units::unplaced::run_00c048b0::fn_00c058f0;",
            "use engine_fallout_unplaced_b::units::unplaced::run_00c048b0::fn_00c058f0;",
        ),
    )
    for old, new in replacements:
        if old not in hook:
            raise ValueError(f"dead_lights.rs no longer says {old!r}; update the lab's inliner")
        hook = hook.replace(old, new)
    module = "mod engine {\n    pub mod dead_lights {\n"
    module += "\n".join("        " + line for line in hook.splitlines()) + "\n"
    module += "    }\n}\n\n"
    # Inner docs (`//!`) may only lead a file; they come before the local module.
    # The suite calls the hook from inside its own modules, so use a crate-root path.
    lines = text.splitlines(keepends=True)
    first = 0
    while first < len(lines) and (
        lines[first].lstrip().startswith("//!") or not lines[first].strip()
    ):
        first += 1
    text = "".join(lines[first:]).replace(
        "engine::dead_lights::install", "crate::engine::dead_lights::install"
    )
    return "".join(lines[:first]) + module + text


def rewrite_source(text, parts):
    """Assemble only this family's parts in place of the facade's whole table.

    `engine_typed::units::funcs` registers the typed families with the core: without
    it `Engine::new()` applies no family, and a Diff case that calls a typed function
    by address silently runs the translation instead. It costs no part crate.
    """
    functions = ", ".join(
        ["engine_typed::units::funcs"]
        + [f"{crate.replace('-', '_')}::units::funcs" for crate in parts]
    )
    assemble = f"engine_core::assemble!({functions});"
    if text.count("use engine as _;") != 1:
        raise ValueError("the suite does not link the engine facade in the expected way")
    text = text.replace("use engine as _;", assemble)
    return with_guard_helper(text)


PATCH_ENTRY = re.compile(r'^([A-Za-z0-9_-]+)\s*=\s*\{\s*path\s*=\s*"([^"]+)"\s*\}\s*(?:#.*)?$')


def patch_lines(root_manifest, out):
    """The root manifest's `[patch.crates-io]` path entries, re-pointed at `out`: a crate outside the workspace gets none.

    Without them the lab links the published wgpu-hal, which lacks what d3d9-wgpu calls, so no suite that reaches
    `engine-platform` (draw, shader) can build in the lab.
    """
    lines = []
    inside = False
    for line in root_manifest.splitlines():
        if line.startswith("["):
            inside = line.strip() == "[patch.crates-io]"
            continue
        found = PATCH_ENTRY.match(line.strip()) if inside else None
        if found:
            path = Path(os.path.relpath(REPO / found.group(2), out)).as_posix()
            lines.append(f'{found.group(1)} = {{ path = "{path}" }}')
    return ["", "[patch.crates-io]", *lines] if lines else []


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
        path = Path(os.path.relpath(REPO / "crates" / name, out)).as_posix()
        lines.append(f'{name} = {{ path = "{path}"{", " + options if options else ""} }}')
    lines += patch_lines((REPO / "Cargo.toml").read_text(encoding="utf-8"), out)
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
    # The workspace's lock file pins the versions the crates build with; the lab crate has none of its own.
    files = [(target, text), (manifest_path, manifest(family, parts, out)),
             (out / "Cargo.lock", (REPO / "Cargo.lock").read_text(encoding="utf-8"))]
    # A suite can include a table beside it; the generated crate needs the same file.
    for name in INCLUDE.findall(text):
        included = path.parent / name
        if not included.is_file():
            raise ValueError(f"{path.name} includes {name!r}, which is not beside it")
        dest = out / "tests" / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        files.append((dest, included.read_text(encoding="utf-8", errors="replace")))
    for file, content in files:
        # Leave the timestamp alone when nothing changed, so a warm run does not rebuild.
        if not file.exists() or file.read_text(encoding="utf-8") != content:
            file.write_text(content, encoding="utf-8")
    return manifest_path


def parse_run_args(argv):
    """The family and the libtest arguments, with go-task's literal `--` removed."""
    args = list(argv)
    if args and args[0] == "--":
        args.pop(0)
    if not args:
        raise ValueError("a typed family name is required")
    family, *rest = args
    if rest and rest[0] == "--":
        rest.pop(0)
    return family, rest


def daemon_command():
    """The build daemon's client command, with -- before the cargo arguments."""
    return [sys.executable, str(FORK / "build_daemon.py"), "run", "--"]


def cargo_command():
    """The optional shared-machine wrapper, from the environment, or plain cargo.

    The value is a command prefix, not a fixed path: on one Linux machine it may be
    `flock ~/.cache/nv-workers/cargo.lock ~/.local/bin/capped 10G env ... cargo`.
    Without the variable the tool works anywhere cargo and Python are installed.
    """
    value = os.environ.get("NV_FAMILY_LAB_CARGO", "cargo")
    return [os.path.expandvars(os.path.expanduser(part))
            for part in shlex.split(value, posix=os.name != "nt")]


class _NoLock:
    def __enter__(self):
        return self

    def __exit__(self, *_):
        return False


def cargo_lock():
    """Hold the optional shared lock for the whole cargo run, on POSIX and Windows."""
    name = os.environ.get("NV_FAMILY_LAB_LOCK")
    if not name:
        return _NoLock()
    path = Path(os.path.expandvars(os.path.expanduser(name)))
    path.parent.mkdir(parents=True, exist_ok=True)
    handle = path.open("a+b")
    try:
        if os.name == "nt":
            import msvcrt
            if handle.seek(0, os.SEEK_END) == 0:
                handle.write(b"0")
                handle.flush()
            handle.seek(0)
            msvcrt.locking(handle.fileno(), msvcrt.LK_LOCK, 1)
        else:
            import fcntl
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
    except Exception:
        handle.close()
        raise
    return handle


def run(argv):
    """Generate one family's lab crate and run its Diff cases in one cargo process."""
    family, filters = parse_run_args(argv)
    path = suite(family)
    out = REPO / "target" / "family-lab" / family
    manifest_path = generate(path, out)
    print(f"# family lab: {manifest_path}", flush=True)
    command = [
        "test", "--manifest-path", str(manifest_path), "--test", f"typed_{family}"
    ]
    if filters:
        command += ["--"] + filters
    if os.environ.get("NV_FAMILY_LAB_DIRECT") == "1":
        with cargo_lock():
            return subprocess.call(cargo_command() + command)
    return subprocess.call(daemon_command() + command)


def main(argv):
    if len(argv) == 2 and argv[1] == "families":
        for path in sorted(TESTS.glob("typed_*.rs")):
            print(f"{path.stem.removeprefix('typed_'):20} {', '.join(assembly_for(path))}")
        return 0
    if len(argv) == 4 and argv[1] == "generate":
        path = suite(argv[2])
        print(generate(path, Path(argv[3])))
        return 0
    if len(argv) == 2 and argv[1] == "--list":
        return main(["family_lab.py", "families"])
    if len(argv) >= 2 and argv[1] == "run":
        if len(argv) == 3 and argv[2] == "--list":
            return main(["family_lab.py", "families"])
        return run(argv[2:])
    print(__doc__.strip(), file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
