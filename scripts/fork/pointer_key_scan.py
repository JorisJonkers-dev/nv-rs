# SPDX-License-Identifier: GPL-3.0-only
"""Read-only, re-runnable scan for code whose result depends on the width or layout of a pointer.

Guest pointers are 32-bit `u32` values in `Ptr<T>` wrappers today. A move to wider pointers, or a change of
their layout, changes anything computed from a pointer's value: hash buckets, orderings and iteration orders
that follow from them. This script walks `crates/**/*.rs` (skipping `target`) and reports four kinds of
suspicious pattern with regular expressions. Strings, chars and both `//` and `/* ... */` comments are
blanked before braces or semicolons are counted:

  hash      NiTPointerMap hash-table fields, a key reduced by `%` against a size/count, a `>> 2` bucket shift,
            or a virtual bucket slot (e.g. `SLOT_BUCKET`, `KeyToHashIndex`)
  order     sort / sort_by / sort_by_key / cmp / spaced `<` `>` on pointer-like values (Ptr, ptr, addr, *_addr)
  map-key   BTreeMap / BTreeSet / HashMap / HashSet keyed by Ptr, u32, usize or isize
  cast-mix  a key-like value cast with `as usize` / `as u32` then mixed with `%`, `>>`, `^` or `wrapping_mul`

A row's **reach** is the number of distinct *other* `.rs` files whose text contains the enclosing function's
name as a word: a cheap call-graph proxy, not a call graph. A name that is a word in more than
`GENERIC_FILE_THRESHOLD` (40) `.rs` files is generic: its reach column reads `generic` and it ranks after every
row with a numeric reach, then by kind and path. Test code is skipped as findings (files named `*_tests.rs`,
`tests.rs` or `test_*.rs`, anything under `tests/`, modules declared by `#[cfg(test)] mod name;` in a sibling
file, and code after `#[cfg(test)]`) but stays in the word index. The word index is built once (one regex pass
per file, a set of words per file) and reach is read from an inverted index.
Output is deterministic (sorted, no timestamps). Stdlib only.

  python3 scripts/fork/pointer_key_scan.py [ROOT]
  python3 scripts/fork/pointer_key_scan.py --markdown docs/fork/POINTER-KEYS.md
  python3 scripts/fork/pointer_key_scan.py --check docs/fork/POINTER-KEYS.md
  python3 scripts/fork/pointer_key_scan.py --self-test
"""
import argparse
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_ROOT = os.path.dirname(os.path.dirname(HERE))  # two levels above this script
MAX_ROWS = 150
WIDTH = 120
GENERIC_FILE_THRESHOLD = 40

CFG_ANY_TEST = re.compile(r"#\[cfg\([^\]]*\btest\b")
CFG_NOT_TEST = re.compile(r"\bnot\s*\(\s*test\b")


class _CfgTest:
    """`#[cfg(test)]`, `#[cfg(all(unix, test))]`, `#[cfg(any(test, ..))]`; not `cfg(not(test))`."""

    @staticmethod
    def search(text):
        for m in CFG_ANY_TEST.finditer(text):
            end = text.find("]", m.start())
            if not CFG_NOT_TEST.search(text[m.start():end if end >= 0 else len(text)]):
                return m
        return None


CFG_TEST = _CfgTest()
WORD = re.compile(r"[A-Za-z_]\w*")
# A necessary condition for every matcher below. One cheap pass keeps the full regex battery off the
# ~98% of lines that cannot match anything (the scans cover millions of lines).
LINE_ANCHOR = re.compile(
    r"m_uiHashSize|m_ppkHashTable|KeyToHashIndex|SLOT_BUCKET|%|>>|sort|cmp|[<>] |"
    r"BTreeMap|BTreeSet|HashMap|HashSet|AddrMap|AddrSet|as\s+(?:usize|u32)|bucket"
)
FN_DECL = re.compile(r"\bfn\s+([A-Za-z_]\w*)")
DELIMS = re.compile(r"[{};()\[\]]")  # delimiters tracked by the brace/semicolon scanners
BLOCK_MARK = re.compile(r"/\*|\*/")  # block comments nest in Rust, so open and close marks are counted
MOD_DECL = re.compile(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+([A-Za-z_]\w*)\s*;\s*$")
PTRISH = r"(?:\bPtr\b|\bptr\b|\baddr(?:ess)?\b|\b\w+_addr\b|\.addr\(\)|\bpointer\b)"
SORT = r"\.\s*sort(?:_unstable)?(?:_by|_by_key)?\s*\("

# Each kind is a tuple of independent regexes; a line is one finding per kind if any regex matches.
PATTERNS = {
    "hash": tuple(re.compile(p) for p in (
        r"m_uiHashSize|m_ppkHashTable|KeyToHashIndex|SLOT_BUCKET",
        r"(?:\b(?:\w+_)?(?:key|hash|id|low|ptr|addr)(?:_\w+)?\b|\ba\[\d+\])[^\n%;]{0,60}%\s*"
        r"(?:e\.mem\.u32\([^\n)]*\)|\w*(?:size|count|bucket|hash)\w*|\d+)",
        r"\b(?:\w+_)?(?:key|hash|addr|ptr|low)(?:_\w+)?\b[^\n]{0,24}>>\s*2\b",
        r"\bbucket\w*\b[^\n]{0,40}(?:vcall|slot)|(?:vcall|slot)[^\n]{0,40}\bbucket\w*\b",
    )),
    "order": tuple(re.compile(p) for p in (
        SORT + r"[^\n]*(?:" + PTRISH + r")|(?:" + PTRISH + r")[^\n]*" + SORT,
        r"\.cmp\([^\n]*(?:" + PTRISH + r")|(?:" + PTRISH + r")[^\n]*\.cmp\(",
        r"(?:" + PTRISH + r")[^\n]{0,40}\s[<>] [^\n]{0,40}|[^\n]{0,40}\s[<>] [^\n]{0,40}(?:" + PTRISH + r")",
    )),
    "map-key": tuple(re.compile(p) for p in (
        r"\b(?:BTreeMap|BTreeSet|HashMap|HashSet|AddrMap|AddrSet)\s*<\s*(?:Ptr\b|u32\b|usize\b|isize\b)",
    )),
    "cast-mix": tuple(re.compile(p) for p in (
        # The last identifier segment must end in a key-ish word at a word start: `key`, `m_key`,
        # `self.addr`, `base_ptr`, but not `valid` (the `id` there is a substring, not a segment).
        r"\b(?:\w*_)?(?:key|addr|ptr|hash|id|base)\b\s+as\s+(?:usize|u32)\b"
        r"[^\n]{0,48}(?:%|>>|\^|wrapping_mul)",
        r"\b(?:Ptr|address)\b\)?\s+as\s+(?:usize|u32)\b[^\n]{0,48}(?:%|>>|\^|wrapping_mul)",
    )),
}
KINDS = tuple(PATTERNS)


def strip_code(line, state=None):
    """Copy of `line` with comments and string/char literals blanked, for brace counting.

    `state`, when given, is a one-element list holding the block-comment nesting depth on entry; it is
    updated with the depth on exit so a `/* ... */` spanning lines blanks braces on every line it covers.
    """
    out = []
    i, n = 0, len(line)
    depth = state[0] if state is not None else 0
    while i < n:
        if depth:
            for m in BLOCK_MARK.finditer(line, i):
                depth += 1 if m.group() == "/*" else -1
                if depth <= 0:
                    depth = 0
                    i = m.end()
                    break
            else:
                i = n
            continue
        c = line[i]
        if c == "/" and i + 1 < n and line[i + 1] == "/":
            break
        if c == "/" and i + 1 < n and line[i + 1] == "*":
            depth = 1
            i += 2
            continue
        if c == '"':
            i += 1
            while i < n:
                if line[i] == "\\":
                    i += 2
                    continue
                if line[i] == '"':
                    i += 1
                    break
                i += 1
            continue
        if c == "'" and i + 2 < n:
            if line[i + 2] == "'" or (line[i + 1] == "\\" and i + 3 < n and line[i + 3] == "'"):
                i += 4 if line[i + 1] == "\\" else 3
                continue
        out.append(c)
        i += 1
    if state is not None:
        state[0] = depth
    return "".join(out)


def enclosing_names(lines):
    """For each line, the name of the function whose body contains it ("" at module level)."""
    names = []
    depth = 0
    nesting = 0  # parenthesis/bracket depth: `;` inside it is part of a signature, not an item end
    frames = []  # (depth of body, fn name)
    pending = None  # a fn name whose `{` has not been seen yet (multi-line signature, trait decl)
    block = [0]
    for line in lines:
        names.append(frames[-1][1] if frames else "")
        code = strip_code(line, block)
        events = [(m.start(), "decl", m.group(1)) for m in FN_DECL.finditer(code)]
        events += [(m.start(), m.group(), None) for m in DELIMS.finditer(code)]
        events.sort(key=lambda e: e[0])
        active = pending
        pending = None
        braced = False  # no `{` has opened a body since the active fn was declared
        for _, kind, value in events:
            if kind == "decl":
                if active is None:
                    active = value
                    braced = False
            elif kind in ("(", "["):
                nesting += 1
            elif kind in (")", "]"):
                nesting = max(0, nesting - 1)
            elif kind == "{":
                depth += 1
                if active is not None:
                    frames.append((depth, active))
                    active = None
                braced = True
            elif kind == "}":
                depth -= 1
                while frames and frames[-1][0] > depth:
                    frames.pop()
            elif kind == ";" and active is not None and not braced and nesting == 0:
                active = None  # a trait method declaration, not a definition
        pending = active
    return names


def kinds_for(line):
    """The kinds matched by one source line (comment-only and blank lines are skipped)."""
    stripped = line.strip()
    if not stripped or stripped.startswith("//") or not LINE_ANCHOR.search(line):
        return ()
    found = tuple(k for k, regexes in PATTERNS.items() if any(r.search(line) for r in regexes))
    if found and ("//" in line or '"' in line):
        # Look again at the code alone: a match inside a trailing comment or a string literal is not code.
        code = strip_code(line)
        found = tuple(k for k in found if any(r.search(code) for r in PATTERNS[k]))
    return found


def is_test_path(rel):
    """True for Rust test files by name or directory: test_*.rs, *_tests.rs, tests.rs, under tests/."""
    parts = rel.split("/")
    if "tests" in parts[:-1]:
        return True
    base = parts[-1]
    return base == "tests.rs" or base.endswith("_tests.rs") or base.startswith("test_")


def test_paths(texts):
    """Test-only paths: the name rules plus modules declared by `#[cfg(test)] mod name;` in a sibling.

    Both `mod name;` and `pub(crate) mod name;` resolve to `<dir>/name.rs` or `<dir>/name/mod.rs`.
    """
    found = {rel for rel in texts if is_test_path(rel)}
    for rel, text in texts.items():
        if not CFG_TEST.search(text):
            continue
        lines = text.splitlines()
        directory = os.path.dirname(rel)
        depth = 0
        for i, line in enumerate(lines[:-1]):
            before = depth
            if "/*" in line or "*/" in line:
                state = [before]
                strip_code(line, state)  # blanks strings and // comments before counting block marks
                depth = state[0]
            if before or not CFG_TEST.search(line):
                continue
            if not CFG_TEST.search(strip_code(line)):
                continue
            decl = MOD_DECL.match(strip_code(lines[i + 1]))
            if decl is None:
                continue
            name = decl.group(1)
            for candidate in (directory + "/" + name + ".rs", directory + "/" + name + "/mod.rs"):
                candidate = candidate.lstrip("/")
                if candidate in texts:
                    found.add(candidate)
    return found


def _guarded_step(code, depth, opened, nesting):
    """Advance brace/nesting state over one cfg'd line; report a top-level `;`."""
    terminator = False
    for m in DELIMS.finditer(code):
        ch = m.group()
        if ch in "([":
            nesting += 1
        elif ch in ")]":
            nesting = max(0, nesting - 1)
        elif ch == "{":
            depth += 1
            opened = True
        elif ch == "}":
            depth -= 1
        elif ch == ";" and nesting == 0:
            terminator = True
    return depth, opened, nesting, terminator


def cfg_test_lines(lines):
    """1-based line numbers of code guarded by a #[cfg(test)] attribute."""
    skip = set()
    block = 0
    depth = opened = nesting = 0
    guarding = False
    for i, line in enumerate(lines):
        before = block
        if guarding:
            state = [before]
            code = strip_code(line, state)
            block = state[0]
            skip.add(i + 1)
            depth, opened, nesting, terminated = _guarded_step(code, depth, opened, nesting)
            field_end = not opened and nesting == 0 and code.rstrip().endswith(",")
            if (terminated and not opened) or field_end or (opened and depth <= 0):
                guarding = False
            continue
        if "/*" in line or "*/" in line:  # strip_code only where a block mark could change the depth
            state = [before]
            strip_code(line, state)
            block = state[0]
        if before or not CFG_TEST.search(line):
            continue
        if not CFG_TEST.search(strip_code(line)):
            continue
        guarding = True
        depth, opened, nesting = 0, False, 0
        skip.add(i + 1)
        depth, opened, nesting, terminated = _guarded_step(strip_code(line), depth, opened, nesting)
        if (terminated and not opened) or (opened and depth <= 0):
            guarding = False
    return skip


def is_generic(files):
    """True when a name is a word in more than GENERIC_FILE_THRESHOLD .rs files."""
    return len(files) > GENERIC_FILE_THRESHOLD


def collect(root):
    """Read every .rs file under root once; findings are (kind, path, line, text, fn, reach, generic)."""
    texts = {}
    for base, dirs, files in os.walk(os.path.join(root, "crates")):
        dirs[:] = sorted(d for d in dirs if d != "target")
        for name in sorted(files):
            if not name.endswith(".rs"):
                continue
            path = os.path.join(base, name)
            rel = os.path.relpath(path, root).replace(os.sep, "/")
            with open(path, "r", encoding="utf-8", errors="replace") as fh:
                texts[rel] = fh.read()

    matches = {}
    excluded = test_paths(texts)
    for rel in sorted(texts):
        if rel in excluded:
            continue
        text = texts[rel]
        lines = text.splitlines()
        hits = []
        for number, line in enumerate(lines, 1):
            kinds = kinds_for(line)
            if kinds:
                hits.append((number, line, kinds))
        if not hits:
            continue
        skip = cfg_test_lines(lines) if CFG_TEST.search(text) else ()
        hits = [h for h in hits if h[0] not in skip]
        if not hits:
            continue
        names = enclosing_names(lines)
        matches[rel] = [(number, line, kind, names[number - 1]) for number, line, kinds in hits for kind in kinds]

    # One pass builds an inverted word index from every file, tests included (excluded from findings only):
    # a fn name is a word in a file iff it is an identifier in it. All reach counts are read from this
    # index, never by rescanning.
    needed = {fn for hits in matches.values() for _, _, _, fn in hits if fn}
    word_files = {}
    for rel, text in texts.items():
        for word in set(WORD.findall(text)) & needed:
            word_files.setdefault(word, set()).add(rel)

    findings = []
    for rel in sorted(matches):
        for number, line, kind, fn in matches[rel]:
            files = word_files.get(fn, ())
            reach = len(files) - (1 if rel in files else 0) if fn else 0
            findings.append((kind, rel, number, line.strip(), fn, reach, is_generic(files)))
    findings.sort(key=lambda f: (f[6], -f[5] if not f[6] else 0, f[0], f[1], f[2]))
    return findings


def cell(text):
    return text.replace("|", "\\|").replace("\n", " ")[:WIDTH]


def render(findings):
    counts = {k: sum(1 for f in findings if f[0] == k) for k in KINDS}
    total = len(findings)
    out = [
        "# Pointer-key scan",
        "",
        "A read-only, re-runnable regex scan for code whose result can depend on the width or layout of a guest",
        "pointer (a 32-bit `u32` in a `Ptr<T>` wrapper today). A wider pointer or a different layout changes",
        "anything computed from a pointer's value: hash buckets, orderings and the iteration orders that follow.",
        "It walks `crates/**/*.rs` (skipping `target`) and recognises four kinds of pattern:",
        "",
        "- `hash` - `m_uiHashSize` / `m_ppkHashTable` reads, a key reduced by `%` against a size or count,",
        "  a `>> 2` bucket shift, or a virtual bucket slot (`SLOT_BUCKET`, `KeyToHashIndex`).",
        "- `order` - `sort`, `sort_by`, `sort_by_key`, `cmp` or a spaced `<` / `>` on pointer-like values",
        "  (`Ptr`, `ptr`, `addr`, `*_addr`, `.addr()`).",
        "- `map-key` - `BTreeMap`, `BTreeSet`, `HashMap`, `HashSet` keyed by `Ptr`, `u32`, `usize` or `isize`.",
        "- `cast-mix` - a key-like value cast with `as usize` / `as u32` then mixed with `%`, `>>`, `^` or",
        "  `wrapping_mul`.",
        "",
        "**Reach** is the number of distinct *other* `.rs` files whose text contains the enclosing function's",
        "name as a word: a cheap call-graph proxy, not a call graph. A name that is a word in more than 40",
        "`.rs` files (`GENERIC_FILE_THRESHOLD`) is **generic**: its reach reads `generic` and it ranks after",
        "every row with a numeric reach. Rows are then ranked by kind and path. Test code is not reported:",
        "files named `*_tests.rs`, `tests.rs` or `test_*.rs` (including `test_support.rs` and `test_game.rs`),",
        "anything under `tests/`, modules declared by `#[cfg(test)] mod name;` in a sibling file, and code",
        "after `#[cfg(test)]` are skipped as findings, but still count towards the word index and reach. The",
        "table shows the top 150 rows by reach; the counts above cover every finding. Findings are candidates",
        "for triage: a regex hit is not on its own proof of pointer dependence, and a miss is not proof of "
        "independence. Known limits: an address used as an index with `>> 2` is found only next to a key-like "
        "name, a module declared with `#[path]` is not followed, a cfg(test) struct field whose comma is on a later line hides the lines up to it, and raw strings are not tokenised.",
        "",
        "Re-run from the repository root with",
        "`python3 scripts/fork/pointer_key_scan.py --markdown docs/fork/POINTER-KEYS.md`; check the committed copy",
        "without writing anything with",
        "`python3 scripts/fork/pointer_key_scan.py --check docs/fork/POINTER-KEYS.md`; verify the matchers on",
        "synthetic snippets with `python3 scripts/fork/pointer_key_scan.py --self-test`.",
        "",
        "Total findings: {} (hash: {}, order: {}, map-key: {}, cast-mix: {}).".format(
            total, counts["hash"], counts["order"], counts["map-key"], counts["cast-mix"]),
        "",
        "| kind | file:line | enclosing fn | matched line (<= 120 chars) | reach |",
        "| --- | --- | --- | --- | ---: |",
    ]
    for kind, rel, number, text, fn, reach, generic in findings[:MAX_ROWS]:
        out.append("| {} | {}:{} | {} | {} | {} |".format(
            kind, rel, number, cell(fn or "(module)"), cell(text), "generic" if generic else reach))
    return "\n".join(out) + "\n"


SELF_TESTS = (
    ("hash", "let bucket = key as u32 % hash_size;"),
    ("hash", "let size = e.get(map, NiTPointerMap::m_uiHashSize);"),
    ("order", "items.sort_by(|a, b| a.addr().cmp(&b.addr()));"),
    ("order", "if this.addr() < other.addr() { return; }"),
    ("map-key", "let mut seen: BTreeMap<Ptr, usize> = BTreeMap::new();"),
    ("cast-mix", "let h = (key as usize) ^ (key as usize >> 5);"),
    # A whole key-ish identifier ending in a key-ish word, not an arbitrary substring.
    ("cast-mix", "let h = key as u32 % hash_size;"),
    ("cast-mix", "let h = self.addr as usize ^ 1;"),
    ("cast-mix", "let h = base_ptr as u32 >> 2;"),
    ("cast-mix", "let h = m_id as usize % 4;"),
    ("cast-mix", "let h = valid_id as u32 % 4;"),
    ("cast-mix", "let h = _key as u32 % 4;"),
    ("hash", "let slot = buckets + 4 * (key_low % count);"),
    ("hash", "let i = key_low >> 2;"),
    ("map-key", "let funcs: AddrMap<u32, Callable> = AddrMap::default();"),  # a single leading underscore is a key-ish prefix too
)


def self_test():
    for kind, line in SELF_TESTS:
        assert kind in kinds_for(line), (kind, line, kinds_for(line))
    non_matches = (
        "let total = names.len();",
        "/// the hash uses m_uiHashSize and key % hash_size",
        "pub fn make() -> Ptr<u32> { Ptr::new(0) }",
        "let n = valid as u32 % 4;",  # `id` inside `valid` is not a key-ish identifier
        "let n = grid as u32 % 4;",
        "let i = index as u32 % 4 + addr;",  # `addr` elsewhere on the line does not make `index` a key
    )
    for line in non_matches:
        assert kinds_for(line) == (), (line, kinds_for(line))

    # A `;` in an array length must not drop the enclosing function; a top-level `;` still does.
    assert enclosing_names(["fn outer() {", "    let x = 1;", "}", "let y = 2;"]) == [
        "", "outer", "outer", "",
    ]
    assert enclosing_names([
        "fn foo(x: [u8; 4]) {", "    let x = 1;", "}", "let y = 2;",
    ]) == ["", "foo", "foo", ""]
    assert enclosing_names([
        "fn outer() { fn inner(x: [u8; 4]) {", "    let x = 1;", "}", "}",
    ]) == ["", "inner", "inner", "outer"]
    assert enclosing_names(["fn decl();", "fn def() {", "    let x = 1;", "}"]) == [
        "", "", "def", "def",
    ]

    # Block comments, including ones spanning lines, are blanked before braces are counted.
    assert strip_code("/* { */ let x = 1;") == " let x = 1;"
    block = [0]
    assert strip_code("let a = 1; /* open {", block) == "let a = 1; "
    assert strip_code("   } still a comment; */ let b = 2;", block) == " let b = 2;"
    assert strip_code("let c = 3;", block) == "let c = 3;"
    assert enclosing_names([
        "/* {", "} */", "fn real() {", "    let x = 1;", "}",
    ]) == ["", "", "", "real", "real"]
    assert enclosing_names([
        "fn f() {", "    /* } */ let x = 1;", "}", "let y = 2;",
    ]) == ["", "f", "f", ""]

    # Test files by name/path, plus modules declared cfg(test) in a sibling file.
    for rel in (
        "crates/foo/src/bar_tests.rs", "crates/foo/src/tests.rs", "crates/foo/tests/it.rs",
        "crates/foo/src/test_support.rs", "crates/foo/src/test_game.rs", "crates/foo/src/test_util.rs",
    ):
        assert is_test_path(rel), rel
    assert not is_test_path("crates/foo/src/bar.rs")
    assert test_paths({
        "crates/foo/src/lib.rs": "// top\nlet s = \"/*\";\n#[cfg(test)]\npub(crate) mod helper;\n"
                                "#[cfg(test)]\nmod inner;\npub mod real;\n",
        "crates/foo/src/helper.rs": "fn f() {}",
        "crates/foo/src/inner/mod.rs": "fn f() {}",
        "crates/foo/src/real.rs": "fn f() {}",
        "crates/foo/src/test_util.rs": "fn f() {}",
        "crates/foo/src/bar_tests.rs": "fn f() {}",
    }) == {
        "crates/foo/src/helper.rs", "crates/foo/src/inner/mod.rs",
        "crates/foo/src/test_util.rs", "crates/foo/src/bar_tests.rs",
    }

    assert is_generic(set(range(GENERIC_FILE_THRESHOLD + 1)))
    assert not is_generic(set(range(GENERIC_FILE_THRESHOLD)))
    assert cfg_test_lines([
        "fn real() {}",
        "#[cfg(test)]",
        "mod tests {",
        "    let bucket = key % count;",
        "}",
        "fn real_after() {}",
    ]) == {2, 3, 4, 5}
    # A `;` inside array brackets does not end the cfg'd item early.
    assert cfg_test_lines([
        "#[cfg(test)]",
        "fn t(x: [u8; 4]) {",
        "    let bucket = key % count;",
        "}",
        "fn real() {}",
    ]) == {1, 2, 3, 4}
    assert cfg_test_lines([
        "#[cfg(test)]",
        "use crate::foo::bar;",
        "fn real() {}",
    ]) == {1, 2}
    # A cfg(test) struct field ends at its comma; the next production line is not skipped.
    assert cfg_test_lines([
        "struct S {",
        "    #[cfg(test)]",
        "    host_calls: u32,",
        "    asked: HashMap<u32, u32>,",
        "}",
    ]) == {2, 3}
    assert cfg_test_lines(["#[cfg(all(test, unix))]", "fn t() {}", "fn real() {}"]) == {1, 2}
    assert cfg_test_lines(["#[cfg(not(test))]", "fn real() {}"]) == set()
    assert cfg_test_lines(["#[cfg(all(unix, test))]", "fn t() {}", "fn real() {}"]) == {1, 2}
    assert cfg_test_lines(["#[cfg(any(test, debug_assertions))]", "fn t() {}", "fn real() {}"]) == {1, 2}
    assert cfg_test_lines(["#[cfg(all(unix, not(test)))]", "fn real() {}"]) == set()
    assert kinds_for("let x = 1; // key % hash_size") == ()
    assert kinds_for('let s = "key % hash_size";') == ()
    # A `/*` inside a string or a // comment is not a block-comment open.
    for prefix in ("let s = \"/*\";", "// note: /* opens"):
        assert cfg_test_lines([
            prefix, "#[cfg(test)]", "mod t { let x = 1; }", "fn real() {}",
        ]) == {2, 3}, prefix
    print("self-test passed: {} patterns, {} non-matches, generic cutoff, block comments, test files "
          "and cfg(test) skips".format(len(SELF_TESTS), len(non_matches)))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("root", nargs="?", default=DEFAULT_ROOT, help="repository root (default: two levels above this script)")
    group = ap.add_mutually_exclusive_group()
    group.add_argument("--markdown", metavar="FILE", help="write the markdown table to FILE (the only file written)")
    group.add_argument("--check", metavar="FILE", help="exit 1 if FILE differs from a fresh markdown run")
    group.add_argument("--self-test", action="store_true", help="run the matcher on synthetic snippets and exit")
    args = ap.parse_args(argv)

    if args.self_test:
        self_test()
        return 0

    findings = collect(args.root)
    text = render(findings)
    if args.check:
        try:
            with open(args.check, "r", encoding="utf-8") as fh:
                actual = fh.read()
        except OSError as exc:
            print("check failed: cannot read {}: {}".format(args.check, exc))
            return 1
        if actual != text:
            print("check failed: {} differs from a fresh run".format(args.check))
            return 1
        print("check passed: {} matches a fresh run".format(args.check))
        return 0
    if args.markdown:
        with open(args.markdown, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(text)
        print("wrote {} findings to {}".format(len(findings), args.markdown))
        return 0
    sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
