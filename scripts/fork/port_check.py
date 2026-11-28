#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The fast-path gate of one porting attempt: one command, one JSON verdict.

    python3 -I scripts/fork/port_check.py <family> [--static | --full] [--base REF] [--metadata FILE] [--review FILE]
                                             [--out FILE]

The checks are the fork's own tools, run as they are (components.py check, private_integrity.py
--check, public_text.py, verify-sync.sh), two source scans (a plain call of a registered typed
function, the family ids), rustfmt on the changed files, clippy -D warnings on the touched crates
and the family's differential cases. Cargo runs only through the build daemon; `--static` skips
every check that needs cargo or the network, so a worker can run the rest without compiling. `--full`, once per
candidate before it joins a batch, adds the changed-line coverage gate (every changed executable line run by a test,
measured with cargo-llvm-cov through the build daemon), the quality scans and the review verdict (`--review FILE`).

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
sys.path.insert(0, str(FORK))
import coverage_report

FAMILY_NAME = re.compile(r"^[A-Za-z0-9_]+$")
MARKER = re.compile(r"^\s*// Typed from ([0-9a-fA-F]{8})\b")
FN_DEF = re.compile(r"\bfn\s+([A-Za-z_]\w*)")
QUALIFIED_CALL = re.compile(r"\b([A-Za-z_]\w*)::([A-Za-z_]\w*)\s*(?:::\s*<[^>]*>\s*)?\(")
TYPED_CALL = re.compile(r"typed_call!\s*[(\[{]\s*([A-Za-z_][\w:]*)\s*,\s*([^,\s)\]}]+)")
FAMILY_CONST = re.compile(r"^\s*pub const FAMILY: FamilyId = FamilyId::new\((\d+)\)")
LOCATION = re.compile(r"([A-Za-z0-9_./-]+\.[A-Za-z]+):(\d+)")
DIAGNOSTIC = re.compile(r"^(?:error|warning)(?:\[[A-Za-z0-9]+\])?: .+$", re.MULTILINE)
SUMMARY = re.compile(r"could not compile|aborting due to|build failed|waiting for other jobs|generated \d+ warning|emitted")
ARROW = re.compile(r"^\s*--> ([^\s:]+):(\d+)", re.MULTILINE)
FMT_DIFF = re.compile(r"^Diff in (\S+?):(\d+):?", re.MULTILINE)
CRATE_PATH = re.compile(r"^crates/([^/]+)/")
HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")
STRING = re.compile(r'"(?:[^"\\]|\\.)*"|\'(?:[^\'\\]|\\.)\'')
UNSAFE = re.compile(r"\bunsafe\b")
UNWRAP = re.compile(r"\.unwrap\(\)|\.expect\(")
ALLOW = "port-check: allow"
# cargo-llvm-cov leaves out files outside the workspace by default, which hides the typed crate when its lab crate
# is measured; naming the ignore pattern ourselves keeps every crate source and drops only the toolchain, the
# registry and test files.
COVERAGE_IGNORE = r"/rustc/|\.cargo/registry|/tests/"
FN_SIZE_LIMIT = 150
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
    diagnostic = next((d for d in DIAGNOSTIC.finditer(output) if not SUMMARY.search(d.group(0))), None)
    if diagnostic is not None:  # the compiler's own error, with its place: the last line only says how many there were
        arrow = ARROW.search(output, diagnostic.end())
        file, line = (arrow.group(1), arrow.group(2)) if arrow else (default_file, 1)
        return [finding(rule, file, line, diagnostic.group(0)[:300], fix)]
    where = LOCATION.search(output)
    file, line = (where.group(1), where.group(2)) if where else (default_file, 1)
    lines = [x for x in output.strip().splitlines() if x.strip()]
    return [finding(rule, file, line, (lines[-1] if lines else f"exit status {code}")[:300], fix)]


def from_result(rule, result, fix, default_file):
    """`from_tool` for a runner's (status, output) pair."""
    code, output = result
    return from_tool(rule, code, output, fix, default_file)


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
    found = from_tool("diff", code, output, "read the lab output above the first error", suite)
    for item in found:  # the lab crate's `tests/typed_X.rs` is the suite, line for line
        if item["file"] == f"tests/typed_{family}.rs":
            item["file"] = suite
    return found


def parse_ranges(diff):
    """{file: set of added or changed line numbers} from `git diff -U0` text (the new side of each hunk)."""
    out, current = {}, None
    for line in diff.splitlines():
        if line.startswith("+++ "):
            current = line[6:].rstrip("\t") if line.startswith("+++ b/") else None  # git tabs a name with spaces
        elif current and (hunk := HUNK.match(line)):
            start, count = int(hunk.group(1)), int(hunk.group(2) or "1")
            out.setdefault(current, set()).update(range(start, start + count))
    return out


def changed_ranges(root, base, runner, pattern="*.rs"):
    """Changed lines per file against `base`, or None when git cannot say."""
    code, output = runner(["git", "diff", "-U0", "--diff-filter=ACMR", f"{base}...HEAD", "--", pattern], root)
    return None if code else parse_ranges(output)


def library_ranges(ranges):
    """The changed lines of crate sources: the code a test run measures (tests directories are not)."""
    return {f: n for f, n in ranges.items() if f.startswith("crates/") and "/src/" in f}


def looks_like_code(text):
    """False for a blank line, a comment or a line of closing brackets only."""
    return bool(re.search(r"[A-Za-z0-9_]", text.split("//", 1)[0]))


def merge_lcov(paths):
    """{file: {line: hits}} over several lcov files (the most hits wins); None when none could be read."""
    merged = None
    for path in paths:
        part = coverage_report.parse_lcov(path)
        if part is None:
            continue
        merged = merged or {}
        for file, lines in part.items():
            target = merged.setdefault(file, {})
            for number, hits in lines.items():
                target[number] = max(target.get(number, 0), hits)
    return merged


def coverage_gaps(root, ranges, measured):
    """Findings for changed lines the tests did not run, and for changed code with no coverage data at all."""
    out = []
    for file, numbers in sorted(ranges.items()):
        data = measured.get(file)
        if not data:  # no record, or a record with no executable line at all
            text = (root / file).read_text(encoding="utf-8", errors="replace").splitlines()
            code = [n for n in sorted(numbers) if n <= len(text) and looks_like_code(text[n - 1])]
            if code:
                out.append(finding("coverage-missing", file, code[0],
                                   "changed code in a file the coverage run did not measure",
                                   "run the file's tests in the coverage run (name its crate or family)"))
            continue
        out += [finding("coverage", file, n, "changed executable line no test ran",
                        "add a differential case or unit test that runs this line")
                for n in sorted(numbers) if data.get(n) == 0]
    return out


def coverage_commands(root, family, packages, out_dir):
    """[([argv, ...], lcov path)]: the family's lab crate, and every other touched crate on its own tests.

    The lab crate is its own workspace, so cargo-llvm-cov instruments a crate it depends on only when that crate
    is named with --dep-coverage, and a report made in the same command as --dep-coverage came out empty in
    every run. The lab is therefore measured in two steps: run (no report), then report with the ignore pattern.
    """
    daemon = [sys.executable, FORK / "build_daemon.py", "run", "--", "llvm-cov"]
    ignore = ["--ignore-filename-regex", COVERAGE_IGNORE]
    lab = root / "target" / "family-lab" / family / "Cargo.toml"
    named = [arg for package in packages for arg in ("--dep-coverage", package)]
    steps = [daemon + ["test", "--no-report", "--manifest-path", lab, "--test", f"typed_{family}", *named],
             daemon + ["report", "--manifest-path", lab, *ignore, "--lcov", "--output-path", out_dir / "lab.info"]]
    commands = [(steps, out_dir / "lab.info")]
    for package in packages:
        if package != "engine-typed":
            commands.append(([daemon + [*ignore, "--lcov", "--output-path", out_dir / f"{package}.info", "-p", package]],
                             out_dir / f"{package}.info"))
    return commands


def check_coverage(root, family, base, runner):
    """100% of the changed executable lines of crate sources run by a test; no measurement is a failure."""
    ranges = changed_ranges(root, base, runner)
    if ranges is None:
        return [finding("git", ".", 1, f"cannot diff against {base}", "pass --base with a ref that exists")]
    ranges = library_ranges(ranges)
    if not ranges:
        return []
    out_dir = root / "target" / "port-check" / family
    out_dir.mkdir(parents=True, exist_ok=True)
    code, output = runner([sys.executable, FORK / "family_lab.py", "generate", family, root / "target" / "family-lab" / family], root)
    if code:
        return from_tool("coverage", code, output, "generate the family's lab crate first", f"crates/engine/tests/typed_{family}.rs")
    packages = touched_packages(root, list(ranges))
    paths = []
    for steps, lcov in coverage_commands(root, family, packages, out_dir):
        lcov.unlink(missing_ok=True)
        for argv in steps:
            code, output = runner(argv, root)
            if code:
                return from_tool("coverage", code, output, "make the covered tests pass; coverage needs a green run", "Cargo.toml")
        paths.append(lcov)
    measured = merge_lcov(paths)
    if not measured:
        return [finding("coverage-missing-baseline", ".", 1, "the coverage run produced no lcov data",
                        "run cargo-llvm-cov on the touched crates; a gate with no measurement fails")]
    return coverage_gaps(root, ranges, measured)


def function_spans(lines):
    """[(first line, last line)] of each `fn` in `lines` (1-based), by brace depth."""
    spans = []
    for index, text in enumerate(lines):
        if not FN_DEF.search(text.split("//", 1)[0]):
            continue
        depth, seen = 0, False
        for end in range(index, len(lines)):
            code = STRING.sub('""', lines[end].split("//", 1)[0])
            depth += code.count("{") - code.count("}")
            seen = seen or "{" in code
            if seen and depth <= 0:
                spans.append((index + 1, end + 1))
                break
            if not seen and code.rstrip().endswith(";"):
                break
    return spans


def scan_quality(root, ranges):
    """unsafe without a SAFETY comment, unwrap or expect, and over-long functions, on changed non-test lines."""
    out = []
    for file, numbers in sorted(library_ranges(ranges).items()):
        raw = (root / file).read_text(encoding="utf-8", errors="replace").splitlines()
        limit = next((i for i, t in enumerate(raw) if t.strip().startswith("#[cfg(test)]")), len(raw))
        for number in sorted(n for n in numbers if n <= limit):
            text = raw[number - 1]
            code = text.split("//", 1)[0]
            if ALLOW in text:
                continue
            if UNSAFE.search(code) and not any("SAFETY:" in t for t in raw[max(0, number - 4):number]):
                out.append(finding("quality-unsafe", file, number, "unsafe without a SAFETY comment",
                                   "add `// SAFETY: why this is sound` on or above the line"))
            if UNWRAP.search(code):
                out.append(finding("quality-unwrap", file, number, "unwrap or expect in non-test code",
                                   f"return the error, or allow it with `// {ALLOW} unwrap (reason)`"))
        for first, last in function_spans(raw[:limit]):
            if last - first + 1 > FN_SIZE_LIMIT and any(first <= n <= last for n in numbers):
                out.append(finding("quality-size", file, first, f"function of {last - first + 1} lines (limit {FN_SIZE_LIMIT})",
                                   "split it into smaller functions"))
    return out


def check_quality(root, base, runner):
    """The quality scans on the diff, and ruff on the changed Python."""
    ranges = changed_ranges(root, base, runner)
    if ranges is None:
        return [finding("git", ".", 1, f"cannot diff against {base}", "pass --base with a ref that exists")]
    out = scan_quality(root, ranges)
    python = changed_ranges(root, base, runner, "*.py")
    if python is None:
        return [finding("git", ".", 1, f"cannot diff against {base}", "pass --base with a ref that exists")]
    files = sorted(f for f in python if f.startswith("scripts/fork/") and (root / f).is_file())
    if files:
        out += from_result("ruff", runner(["ruff", "check", "--output-format", "concise", *files], root),
                         "fix what ruff names; do not silence it", files[0])
        out += from_result("pyright", runner(["pyright", *files], root),
                           "fix the type error (for example guard a value that may be None); do not silence it", files[0])
    return out


def check_review(path):
    """The review verdict file: pass, or fail with findings; a missing or unreadable one is `review-absent`."""
    absent = [finding("review-absent", str(path or "-"), 1, "no readable review verdict",
                      "requeue the candidate for review with backoff")]
    try:
        data = json.loads(Path(path).read_text(encoding="utf-8"))
        kind, found = data["verdict"], data.get("findings", [])
        if kind not in ("pass", "fail") or not isinstance(found, list):
            return absent
    except (OSError, TypeError, ValueError, KeyError):
        return absent
    if kind == "pass":
        return []
    return [finding("review", str(f.get("file", "?")), f.get("line", 1) or 1, str(f.get("issue", "")), str(f.get("fix", "")))
            for f in found if isinstance(f, dict)] or [finding("review", "-", 1, "review failed without findings", "ask for findings")]


def run_checks(root, family, base, mode, runner, review=None, metadata=None):
    """[(check id, findings, seconds)] for the fast path, or for the cargo-free subset."""
    suite = root / "crates" / "engine" / "tests" / f"typed_{family}.rs"
    plan = [
        ("family", lambda: [] if FAMILY_NAME.match(family) and suite.is_file() else [finding(
            "family-suite", suite.relative_to(root).as_posix() if FAMILY_NAME.match(family) else ".", 1,
            f"no differential suite for family `{family}`", "name an existing family (family_lab.py families)")]),
        ("components", lambda: from_result("components", runner(
            [sys.executable, FORK / "components.py", "check", *(["--metadata", metadata] if metadata else [])], root),
                                         "assign the crate or test file to a component in .fork/components.toml",
                                         ".fork/components.toml")),
        ("private-integrity", lambda: from_result(
            "private-integrity", runner([sys.executable, FORK / "private_integrity.py", "--check"], root),
            "restore the checker files; workers may not change them", "scripts/fork/private_integrity.py")),
        ("public-text", lambda: from_result("public-text", runner([sys.executable, FORK / "public_text.py"], root),
                                          "remove the machine detail from the text", "docs/fork")),
        ("typed-calls", lambda: scan_typed_calls(root)),
        ("family-ids", lambda: scan_family_ids(root)),
        ("fmt", lambda: check_format(root, base, runner)),
    ]
    if mode != "static":
        plan += [
            ("verify-sync", lambda: from_result("verify-sync", runner(["bash", FORK / "verify-sync.sh"], root),
                                              "list a new fork-only file in .fork/overlay-paths; for an edit to a file upstream has too, "
                                              "add a file under .fork/allowed-divergence.d/ (`path  # reason`)",
                                              ".fork/overlay-paths")),
            ("clippy", lambda: check_clippy(root, base, runner)),
            ("differential", lambda: check_differential(root, family, runner)),
        ]
    if mode == "full":
        plan += [
            ("coverage", lambda: check_coverage(root, family, base, runner)),
            ("quality", lambda: check_quality(root, base, runner)),
            ("review", lambda: check_review(review)),
        ]
    results = []
    for name, check in plan:
        started = time.time()
        results.append((name, check(), time.time() - started))
        if name == "family" and results[-1][1]:
            break  # nothing else can be judged without the family
    return results


def verdict(family, mode, results):
    """The JSON object for one run."""
    failures = [f for _, found, _ in results for f in found]
    return {
        "family": family, "mode": mode, "pass": not failures,
        "checks": [{"id": name, "ok": not found, "seconds": round(seconds, 2)} for name, found, seconds in results],
        "failures": failures,
    }


def main(argv=None, root=REPO, runner=run, out=sys.stdout):
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    parser.add_argument("family")
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--static", action="store_true", help="skip every check that needs cargo or the network")
    modes.add_argument("--full", action="store_true", help="add coverage, quality and the review verdict")
    parser.add_argument("--review", help="the review verdict JSON (--full)")
    parser.add_argument("--base", default="origin/fork", help="the ref the attempt is measured against")
    parser.add_argument("--metadata", help="a saved `cargo metadata --no-deps` for components.py, so no cargo is run")
    parser.add_argument("--out", help="also write the verdict to this file")
    args = parser.parse_args(argv)
    mode = "static" if args.static else "full" if args.full else "fast"
    result = verdict(args.family, mode, run_checks(Path(root), args.family, args.base, mode, runner, args.review,
                                                   args.metadata))
    text = json.dumps(result, indent=2, sort_keys=True)
    if args.out:
        Path(args.out).write_text(text + "\n", encoding="utf-8")
    print(text, file=out)
    return 0 if result["pass"] else 1


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
