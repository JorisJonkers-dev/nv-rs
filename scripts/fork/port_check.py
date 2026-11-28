#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The fast-path gate of one porting attempt: one command, one JSON verdict.

    python3 -I scripts/fork/port_check.py <family> [--static] [--base REF] [--metadata FILE] [--out FILE]

The checks are the fork's own tools, run as they are (components.py check, private_integrity.py
--check, public_text.py, verify-sync.sh), two source scans (a plain call of a registered typed
function, the family ids), rustfmt on the changed files, clippy -D warnings on the touched crates
and the family's differential cases. Cargo runs only through the build daemon; `--static` skips
every check that needs cargo or the network, so a worker can run the rest without compiling.

The verdict is JSON on stdout (and in --out): `pass`, the checks that ran and, for every failure,
the rule id, file:line, what is wrong and the fix. A tool that is missing or prints nothing
readable fails its check: silence is never a pass. Exit status 0 only on a pass.
"""
import argparse
import json
import re
import subprocess
import sys
import time
from pathlib import Path

FORK = Path(__file__).resolve().parent
REPO = FORK.parent.parent
FAMILY_NAME = re.compile(r"^[A-Za-z0-9_]+$")
MARKER = re.compile(r"^\s*// Typed from ([0-9a-fA-F]{8})\b")
FN_DEF = re.compile(r"\bfn\s+([A-Za-z_]\w*)")
QUALIFIED_CALL = re.compile(r"\b([A-Za-z_]\w*)::([A-Za-z_]\w*)\s*(?:::\s*<[^>]*>\s*)?\(")
TYPED_CALL = re.compile(r"typed_call!\s*[(\[{]\s*([A-Za-z_][\w:]*)\s*,\s*([^,\s)\]}]+)")
FAMILY_CONST = re.compile(r"^\s*pub const FAMILY: FamilyId = FamilyId::new\((\d+)\)")
LOCATION = re.compile(r"([A-Za-z0-9_./-]+\.[A-Za-z]+):(\d+)")
FMT_DIFF = re.compile(r"^Diff in (\S+?):(\d+):?", re.MULTILINE)
CRATE_PATH = re.compile(r"^crates/([^/]+)/")
FAMILY_LIMIT = 64
TYPED_SOURCE = Path("crates") / "engine-typed" / "src"


def finding(rule, file, line, message, fix):
    """One failure: the rule, where, what and the fix."""
    return {"rule": rule, "file": str(file), "line": int(line), "message": message, "fix": fix}


def run(argv, cwd):
    """Run a command; (exit status, combined output). A missing program is status 127."""
    try:
        done = subprocess.run([str(a) for a in argv], cwd=str(cwd), text=True, errors="replace",
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    except OSError as exc:
        return 127, f"{argv[0]}: {exc}"
    return done.returncode, done.stdout


def code_lines(path):
    """The (number, text) of a Rust file's code before its test module, comments cut off."""
    out = []
    for number, text in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        if text.strip().startswith("#[cfg(test)]"):
            break
        out.append((number, text.split("//", 1)[0]))
    return out


def module_of(path):
    """The module a source file is: its folder for a mod.rs, else its stem."""
    return path.parent.name if path.stem == "mod" else path.stem


def registered_functions(root):
    """{(module, function name): address} for every `// Typed from` marker in the typed crate."""
    found = {}
    for path in sorted((root / TYPED_SOURCE).rglob("*.rs")):
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        lines = lines[:next((i for i, t in enumerate(lines) if t.strip().startswith("#[cfg(test)]")), len(lines))]
        for index, text in enumerate(lines):
            marker = MARKER.match(text)
            if not marker:
                continue
            for following in lines[index + 1:index + 12]:
                if MARKER.match(following):
                    break  # the next marker's function, not this one's
                definition = FN_DEF.search(following)
                if definition:
                    found[(module_of(path), definition.group(1))] = marker.group(1).lower()
                    break
    return found


def scan_typed_calls(root):
    """Plain calls of registered functions and typed_call! sites with a non-literal address."""
    registry, out = registered_functions(root), []
    for path in sorted((root / TYPED_SOURCE).rglob("*.rs")):
        rel = path.relative_to(root).as_posix()
        lines = code_lines(path)
        for position, (number, text) in enumerate(lines):
            for site in TYPED_CALL.finditer(text):
                if not re.fullmatch(r"0x[0-9a-fA-F_]+", site.group(2)):
                    out.append(finding(
                        "typed-call-address", rel, number,
                        f"typed_call! names its address as `{site.group(2)}`",
                        "write the address as a hexadecimal literal, e.g. 0x0012_3450"))
            for call in QUALIFIED_CALL.finditer(text):
                module, name = call.groups()
                if (module, name) not in registry or module == module_of(path):
                    continue
                statement = "\n".join(t for _, t in lines[max(0, position - 8):position]) + text[:call.start()]
                statement = re.split(r"[;{}]", statement)[-1]
                if any(ok in statement for ok in ("typed_call!", "entry!", ".call(")):
                    continue
                out.append(finding(
                    "typed-call-plain", rel, number,
                    f"plain call of registered `{module}::{name}` (address {registry[(module, name)]})",
                    f"call it as typed_call!(FAMILY, 0x{registry[(module, name)]}, {module}::{name}(e, ...))"))
    return out


def scan_family_ids(root):
    """Family ids that are 64 or more, and ids used by two families."""
    seen, out = {}, []
    for path in sorted((root / "crates").glob("*/src/**/*.rs")):
        rel = path.relative_to(root).as_posix()
        for number, text in code_lines(path):
            declared = FAMILY_CONST.match(text)
            if not declared:
                continue
            value = int(declared.group(1))
            if value >= FAMILY_LIMIT:
                out.append(finding("family-id-cap", rel, number,
                                   f"FamilyId {value} does not fit the 64-bit flag word",
                                   f"use an unused id below {FAMILY_LIMIT}"))
            if value in seen:
                out.append(finding("family-id-unique", rel, number,
                                   f"FamilyId {value} is already used at {seen[value]}",
                                   "take the next unused id (see the FAMILY constants)"))
            seen.setdefault(value, f"{rel}:{number}")
    return out


def from_tool(rule, code, output, fix, default_file):
    """Findings of one tool run: none on exit 0, else the first file:line it printed (or the default)."""
    if code == 0:
        return []
    if code == 127:
        return [finding("tool-missing", default_file, 1, output.strip()[:300] or "tool not found",
                        "install the tool or run the gate where it exists")]
    where = LOCATION.search(output)
    file, line = (where.group(1), where.group(2)) if where else (default_file, 1)
    lines = [x for x in output.strip().splitlines() if x.strip()]
    return [finding(rule, file, line, (lines[-1] if lines else f"exit status {code}")[:300], fix)]


def changed_files(root, base, runner):
    """Paths changed against `base` (the tip of the branch is HEAD), or None when git cannot say."""
    code, output = runner(["git", "diff", "--name-only", f"{base}...HEAD"], root)
    return None if code else output.splitlines()


def touched_packages(root, files):
    """Package names of the crates the changed files belong to."""
    names = []
    for crate in sorted({m.group(1) for f in files if (m := CRATE_PATH.match(f))}):
        manifest = root / "crates" / crate / "Cargo.toml"
        try:
            text = manifest.read_text(encoding="utf-8")
        except (OSError, UnicodeError):
            continue  # no readable manifest: the crate has no package to name
        found = re.search(r'^name\s*=\s*"([^"]+)"', text, re.MULTILINE)
        if found:
            names.append(found.group(1))
    return names


def check_format(root, base, runner):
    """rustfmt --check on the changed Rust files; no cargo."""
    files = changed_files(root, base, runner)
    if files is None:
        return [finding("git", ".", 1, f"cannot diff against {base}", "pass --base with a ref that exists")]
    rust = [f for f in files if f.endswith(".rs") and (root / f).is_file()]
    if not rust:
        return []
    code, output = runner(["rustfmt", "--check", "--edition", "2021", "--", *rust], root)
    if code == 0:
        return []
    marks = FMT_DIFF.findall(output)
    if code == 127 or not marks:
        return from_tool("fmt", code, output, "run rustfmt on the changed files", rust[0])
    return [finding("fmt", file, line, "not formatted as rustfmt writes it", f"run rustfmt on {file}")
            for file, line in dict.fromkeys(marks)]


def check_clippy(root, base, runner):
    """clippy -D warnings on the touched crates, through the build daemon."""
    files = changed_files(root, base, runner)
    if files is None:
        return [finding("git", ".", 1, f"cannot diff against {base}", "pass --base with a ref that exists")]
    packages = touched_packages(root, files)
    if not packages:
        return []
    argv = [sys.executable, FORK / "build_daemon.py", "run", "--", "clippy"]
    for package in packages:
        argv += ["-p", package]
    code, output = runner(argv + ["--all-targets", "--", "-D", "warnings"], root)
    return from_tool("clippy", code, output, "fix the warning at that line; do not allow it", "Cargo.toml")


def check_differential(root, family, runner):
    """The family's differential cases through the family lab."""
    code, output = runner([sys.executable, FORK / "family_lab.py", "run", family], root)
    suite = f"crates/engine/tests/typed_{family}.rs"
    failed = re.findall(r"^\s*test\s+(.+?)\s+\.\.\.\s+FAILED\b", output, re.MULTILINE)
    if code == 0 and not failed:
        return []
    if failed:
        return [finding("diff", suite, 1, f"case {name} differs from the original", "make the typed code return what the original does")
                for name in failed]
    return from_tool("diff", code, output, "read the lab output above the first error", suite)


def run_checks(root, family, base, static, runner, metadata=None):
    """[(check id, findings, seconds)] for the fast path, or for the cargo-free subset."""
    suite = root / "crates" / "engine" / "tests" / f"typed_{family}.rs"
    plan = [
        ("family", lambda: [] if FAMILY_NAME.match(family) and suite.is_file() else [finding(
            "family-suite", suite.relative_to(root).as_posix() if FAMILY_NAME.match(family) else ".", 1,
            f"no differential suite for family `{family}`", "name an existing family (family_lab.py families)")]),
        ("components", lambda: from_tool("components", *runner(
            [sys.executable, FORK / "components.py", "check", *(["--metadata", metadata] if metadata else [])], root),
                                         "assign the crate or test file to a component in .fork/components.toml",
                                         ".fork/components.toml")),
        ("private-integrity", lambda: from_tool(
            "private-integrity", *runner([sys.executable, FORK / "private_integrity.py", "--check"], root),
            "restore the checker files; workers may not change them", "scripts/fork/private_integrity.py")),
        ("public-text", lambda: from_tool("public-text", *runner([sys.executable, FORK / "public_text.py"], root),
                                          "remove the machine detail from the text", "docs/fork")),
        ("typed-calls", lambda: scan_typed_calls(root)),
        ("family-ids", lambda: scan_family_ids(root)),
        ("fmt", lambda: check_format(root, base, runner)),
    ]
    if not static:
        plan += [
            ("verify-sync", lambda: from_tool("verify-sync", *runner(["bash", FORK / "verify-sync.sh"], root),
                                              "list a new fork-only file in .fork/overlay-paths; for an edit to a file upstream has too, "
                                              "add a file under .fork/allowed-divergence.d/ (`path  # reason`)",
                                              ".fork/overlay-paths")),
            ("clippy", lambda: check_clippy(root, base, runner)),
            ("differential", lambda: check_differential(root, family, runner)),
        ]
    results = []
    for name, check in plan:
        started = time.time()
        results.append((name, check(), time.time() - started))
        if name == "family" and results[-1][1]:
            break  # nothing else can be judged without the family
    return results


def verdict(family, static, results):
    """The JSON object for one run."""
    failures = [f for _, found, _ in results for f in found]
    return {
        "family": family, "mode": "static" if static else "fast", "pass": not failures,
        "checks": [{"id": name, "ok": not found, "seconds": round(seconds, 2)} for name, found, seconds in results],
        "failures": failures,
    }


def main(argv=None, root=REPO, runner=run, out=sys.stdout):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("family")
    parser.add_argument("--static", action="store_true", help="skip every check that needs cargo or the network")
    parser.add_argument("--base", default="origin/fork", help="the ref the attempt is measured against")
    parser.add_argument("--metadata", help="a saved `cargo metadata --no-deps` for components.py, so no cargo is run")
    parser.add_argument("--out", help="also write the verdict to this file")
    args = parser.parse_args(argv)
    result = verdict(args.family, args.static, run_checks(Path(root), args.family, args.base, args.static, runner, args.metadata))
    text = json.dumps(result, indent=2, sort_keys=True)
    if args.out:
        Path(args.out).write_text(text + "\n", encoding="utf-8")
    print(text, file=out)
    return 0 if result["pass"] else 1


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
