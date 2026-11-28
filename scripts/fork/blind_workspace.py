#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Build the blind workspace of one benchmark function (the set is `port_benchmark.py`'s).

    NV_BENCH_HIDDEN=DIR python3 -I scripts/fork/blind_workspace.py build SET ADDRESS WORKSPACE
    NV_BENCH_HIDDEN=DIR python3 -I scripts/fork/blind_workspace.py hidden ADDRESS

WORKSPACE is a checkout (a git worktree) of the commit the set was frozen on. `build` makes it blind for ADDRESS:

- the function's accepted code is replaced by a stub: its `// Typed from ADDRESS` marker, its signature and a body of
  `todo!()`. The signature stays because the rest of the file calls the function by name, and the file's other
  functions are needed to build; the documentation above it, which says what the function does, is removed;
- the suite that exercises the address keeps everything but the tests that exercise it: a test is a top-level
  `#[test]` item that names the address (as a `0x` literal) or calls the function by name. Those tests, and only those,
  are cut out of the checkout's copy of the suite, so the family's other cases, helpers and call sites stay and the
  family lab and `port_check.py` still run. The hidden copy, DIR/ADDRESS/typed_<family>.rs (the family being the suite's
  name without `typed_`), holds the suite's other items (its imports, constants and helpers) and the cut-out tests, and
  stands alone: DIR comes from NV_BENCH_HIDDEN and must be outside the workspace, so the hidden cases are never in a
  repository an attempt can read. `factory_queue.py verify --hidden DIR/ADDRESS` scores the attempt on them. Tests in
  more than one suite are refused: the queue scores one hidden suite per family.

`build` prints the manifest (address, source, family, signature, hidden files) as JSON and writes it to
DIR/ADDRESS/manifest.json. The attempt gets only the address, the signature and the rules.
"""
import argparse
import json
import os
import re
import sys
from pathlib import Path
from typing import Any

FORK = Path(__file__).resolve().parent
sys.path.insert(0, str(FORK))
import port_benchmark as pb

ENV = "NV_BENCH_HIDDEN"
FN = re.compile(r"^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const\s+)?(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?fn\s+\w+")
STRING = re.compile(r'"(?:[^"\\]|\\.)*"')
CHAR = re.compile(r"'(?:[^'\\]|\\.)'")

TEST = re.compile(r"^\s*#\[test\]\s*$")
NAME = re.compile(r"\bfn\s+(\w+)")
ENDS = ("}", ";")

Manifest = dict[str, Any]


def hidden_root(env: dict[str, str] | None = None) -> Path:
    """The hidden directory named by NV_BENCH_HIDDEN."""
    value = (os.environ if env is None else env).get(ENV, "").strip()
    if not value:
        raise ValueError(f"{ENV} names the directory for the hidden cases")
    return Path(value).resolve()


def code_of(line: str) -> str:
    """A line's code: no strings, characters or trailing comment."""
    return CHAR.sub("", STRING.sub('""', line)).split("//", 1)[0]


def braces(line: str) -> int:
    """The change in brace depth a line makes, leaving out strings, characters and a trailing comment."""
    code = code_of(line)
    return code.count("{") - code.count("}")


def items(text: str) -> list[str]:
    """The top-level items of Rust source, each with the comments, attributes and blank lines above it.

    Their concatenation is the text. ValueError when the brackets do not balance (a construct this splitter does not
    read, such as a multi-line string): the suite is then not touched.
    """
    chunks: list[str] = []
    current: list[str] = []
    depth = 0
    for line in text.splitlines(keepends=True):
        current.append(line)
        code = code_of(line)
        depth += sum(code.count(o) - code.count(c) for o, c in ("{}", "()", "[]"))
        if depth < 0:
            raise ValueError("the brackets of the suite do not balance")
        if depth == 0 and code.rstrip().endswith(ENDS):
            chunks.append("".join(current))
            current = []
    if depth != 0:
        raise ValueError("the brackets of the suite do not balance")
    if current:
        chunks.append("".join(current))
    return chunks


def exercises(chunk: str, address: str, function: str) -> bool:
    """Whether a test item names the address as a literal or calls the function by name."""
    literals = {pb.normal(found) for found in pb.LITERAL.findall(chunk)}
    return address in literals or re.search(rf"\b{re.escape(function)}\b", chunk) is not None


def split_suite(text: str, address: str, function: str) -> tuple[str, str, list[str]]:
    """(the suite without the tests of the function, the hidden suite, the names of the tests cut out)."""
    kept: list[str] = []
    hidden: list[str] = []
    names: list[str] = []
    for chunk in items(text):
        if not any(TEST.match(line) for line in chunk.splitlines()):
            kept.append(chunk)
            hidden.append(chunk)
        elif exercises(chunk, address, function):
            hidden.append(chunk)
            found = NAME.search(chunk)
            names.append(found.group(1) if found else "?")
        else:
            kept.append(chunk)
    return "".join(kept), "".join(hidden), names


def stub(text: str, address: str) -> tuple[str, str]:
    """(the file's text with the function of `address` stubbed, its signature); ValueError when it is not there."""
    lines = text.splitlines()
    marker = None
    for i, line in enumerate(lines):
        found = pb.MARKER.match(line)
        if found and found.group(1).lower() == address:
            marker = i
    if marker is None:
        raise ValueError(f"no `// Typed from {address}` marker in the source")
    first = next((i for i in range(marker + 1, len(lines)) if FN.match(lines[i]) or pb.MARKER.match(lines[i])), None)
    if first is None or not FN.match(lines[first]):
        raise ValueError(f"the marker of {address} is not followed by a function")
    depth, opened, last = 0, False, first
    signature: list[str] = []
    for last in range(first, len(lines)):
        line = lines[last]
        if not opened:
            cut = line.find("{")
            signature.append(line if cut < 0 else line[:cut + 1])
        depth += braces(line)
        opened = opened or "{" in line
        if opened and depth <= 0:
            break
    else:
        raise ValueError(f"the function of {address} has no closing brace")
    indent = lines[first][:len(lines[first]) - len(lines[first].lstrip())]
    head = " ".join(part.strip() for part in signature).removesuffix("{").rstrip()
    body = [*signature, f'{indent}    todo!("port {address}")', f"{indent}}}"]
    out = [*lines[:marker + 1], *body, *lines[last + 1:]]
    return "\n".join(out) + ("\n" if text.endswith("\n") else ""), head


def build(frozen: dict[str, Any], address: str, workspace: Path, hidden: Path) -> Manifest:
    """Make `workspace` blind for `address`; the manifest of what was done."""
    wanted = pb.normal(address)
    pb.hide(frozen, wanted)  # a set that does not check, or an address outside it, stops here
    entry = next(e for e in frozen["entries"] if e["address"] == wanted)
    suites = list(dict.fromkeys(entry["suites"]))  # the set lists a suite twice when it spells the literal two ways
    names = [Path(suite).name for suite in suites]
    if not re.fullmatch(r"[0-9a-f]{8}", wanted) or not names or len(set(names)) != len(names):
        raise ValueError(f"{wanted}: the entry needs an eight-digit address and suites with distinct file names")
    workspace = Path(workspace).resolve()
    hidden = hidden.resolve()
    destination = hidden / wanted
    if hidden == workspace or workspace in hidden.parents or hidden in workspace.parents:
        raise ValueError("the hidden directory must be outside the workspace")
    source = workspace / entry["source"]
    new, signature = stub(source.read_text(encoding="utf-8"), wanted)
    function = NAME.search(signature)
    assert function is not None  # `stub` only returns a signature that has `fn`
    cuts: dict[str, tuple[str, str, list[str]]] = {}
    for suite in suites:
        split = split_suite((workspace / suite).read_text(encoding="utf-8"), wanted, function.group(1))
        if split[2]:
            cuts[suite] = split
    if not cuts:
        raise ValueError(f"{wanted}: no test in {', '.join(names)} exercises the function, so nothing could score it")
    if len(cuts) > 1:
        raise ValueError(f"{wanted}: tests in {', '.join(Path(s).name for s in cuts)}; one hidden suite per family is scored")
    suite, (kept, hidden_text, tests) = next(iter(cuts.items()))
    destination.mkdir(parents=True, exist_ok=True)
    (destination / Path(suite).name).write_text(hidden_text, encoding="utf-8")
    (workspace / suite).write_text(kept, encoding="utf-8")
    source.write_text(new, encoding="utf-8")
    left = sum(wanted in {pb.normal(x) for x in pb.LITERAL.findall(line)} for line in kept.splitlines())
    manifest: Manifest = {"address": wanted, "source": entry["source"], "signature": signature,
                          "function": function.group(1), "suite": suite, "hidden": [Path(suite).name],
                          "tests": tests, "mentions_left": left,
                          "family": Path(suite).stem.removeprefix("typed_")}
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    return manifest


def main(argv: list[str] | None = None, env: dict[str, str] | None = None, out: Any = sys.stdout) -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    make = sub.add_parser("build")
    make.add_argument("set")
    make.add_argument("address")
    make.add_argument("workspace")
    sub.add_parser("hidden").add_argument("address")
    args = parser.parse_args(argv)
    try:
        root = hidden_root(env)
        if args.command == "hidden":
            print(root / pb.normal(args.address), file=out)
            return 0
        frozen = json.loads(Path(args.set).read_text(encoding="utf-8"))
        print(json.dumps(build(frozen, args.address, Path(args.workspace), root), sort_keys=True), file=out)
    except (ValueError, OSError, KeyError, TypeError) as exc:
        print(f"blind_workspace: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
