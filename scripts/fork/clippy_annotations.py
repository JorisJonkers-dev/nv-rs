#!/usr/bin/env python3
"""Turn `cargo clippy --message-format json` into short log lines and annotations.

Cargo prints compiler messages as JSON lines on stdout. This script reads such a file, prints each diagnostic
once as one short line inside a ::group::, writes the first --max-annotations as ::error / ::warning
annotations (GitHub shows ten per step), and appends a table of lint names and counts to $GITHUB_STEP_SUMMARY
if that variable is set. It never decides the result: the workflow keeps cargo's own exit code.

  cargo clippy --locked --workspace --all-targets --message-format json -- -D warnings > clippy.json
  rc=$?; python3 -I scripts/fork/clippy_annotations.py clippy.json; exit $rc
"""
import argparse
import collections
import json
import os
import sys


def esc(text: str) -> str:
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("file")
    ap.add_argument("--max-annotations", type=int, default=10)
    a = ap.parse_args()
    seen = set()
    diags = []
    with open(a.file, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            if msg.get("reason") != "compiler-message":
                continue
            m = msg["message"]
            if m.get("level") not in ("warning", "error") or not m.get("spans"):
                continue
            span = next((s for s in m["spans"] if s.get("is_primary")), m["spans"][0])
            code = (m.get("code") or {}).get("code") or ""
            key = (span["file_name"], span["line_start"], span["column_start"], code, m["message"])
            if key in seen:  # a lib and its test target report the same lint twice
                continue
            seen.add(key)
            diags.append((m["level"], span["file_name"].replace("\\", "/"), span["line_start"],
                          span["column_start"], code, m["message"], m.get("rendered", "")))
    if not diags:
        print("clippy: no diagnostics")
        return 0
    print(f"::group::clippy: {len(diags)} diagnostic(s)")
    for level, f, ln, col, code, text, _ in diags:
        print(f"{level}: {f}:{ln}:{col}: {text.splitlines()[0] if text else ''} [{code or 'rustc'}]")
    print("::endgroup::")
    print("::group::clippy: the first messages in full")
    for *_, rendered in diags[:20]:
        print(rendered.rstrip())
    print("::endgroup::")
    for level, f, ln, col, code, text, _ in diags[: a.max_annotations]:
        kind = "error" if level == "error" else "warning"
        print(f"::{kind} file={f},line={ln},col={col},title={esc(code or 'clippy')}::{esc(text)}")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        counts = collections.Counter(d[4] or "(rustc)" for d in diags)
        with open(summary, "a", encoding="utf-8") as out:
            out.write("### Clippy\n\n| Lint | Count |\n| --- | ---: |\n")
            for name, n in counts.most_common(15):
                out.write(f"| `{name}` | {n} |\n")
            out.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
