#!/usr/bin/env python3
"""Fork-only. Turn the backend peer's harness lines into ledger evidence (nv-rs#928).

`Diff` prints one `backend-pass: <address>` or `backend-fail: <address> <class>` line for every
function it ran against the backend in the private run. This tool writes those results into the
ledger's `function_evidence.tsv`: a pass sets `backend=1`, so the ledger raises the function to
`translation and backend`; a mismatch records one of the five failure classes. It edits only the
rows it has evidence for and keeps the file's comments, header and other rows.

Run: python3 -I scripts/fork/backend_evidence.py --lines <log> --evidence scripts/ledger/function_evidence.tsv
"""
import argparse
import re
import sys
from pathlib import Path

CLASSES = ("real", "float-only", "uninitialised read", "hang", "fault")
PASS = re.compile(r"^backend-pass: ([0-9a-fA-F]{8})\s*$")
FAIL = re.compile(r"^backend-fail: ([0-9a-fA-F]{8}) (" + "|".join(CLASSES) + r")\s*$")
HEADER = "address\tbackend\tfailure_class\tfamily_moved\ttranslation_deleted\tby_address_callers"


def parse(lines):
    """Map an address to (passed, failure class). A mismatch wins over a pass on the same address."""
    results = {}
    for line in lines:
        line = line.strip()
        passed = PASS.match(line)
        failed = FAIL.match(line)
        if passed:
            results.setdefault(passed.group(1).lower(), (True, ""))
        elif failed:
            results[failed.group(1).lower()] = (False, failed.group(2))
    return results


def evidence_rows(text):
    """Split an evidence file into its comments/header and its rows keyed by address."""
    preamble, rows = [], {}
    for line in text.splitlines():
        if not line.strip() or line.startswith("#") or line.startswith("address\t"):
            preamble.append(line)
            continue
        cells = line.split("\t")
        if len(cells) == 6:
            rows[cells[0].lower()] = cells
    if not any(line.startswith("address\t") for line in preamble):
        preamble.append(HEADER)
    return preamble, rows


def merge(results, text):
    """Return the updated evidence text, with the same header and every untouched row kept."""
    preamble, rows = evidence_rows(text)
    for address, (passed, failure_class) in results.items():
        old = rows.get(address, [address, "0", "", "0", "0", "0"])
        rows[address] = [address, "1" if passed else "0", "" if passed else failure_class, *old[3:6]]
    lines = preamble + ["\t".join(rows[key]) for key in sorted(rows)]
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--lines", type=Path, required=True, help="test output with the backend lines")
    parser.add_argument("--evidence", type=Path, required=True, help="ledger evidence TSV to update")
    parser.add_argument("--require", action="store_true", help="fail when no backend line was printed")
    args = parser.parse_args(argv)
    results = parse(args.lines.read_text(encoding="utf-8", errors="replace").splitlines())
    if not results and args.require:
        print("failed: no backend case ran: every typed family must run against the backend", file=sys.stderr)
        return 1
    if results:
        text = args.evidence.read_text(encoding="utf-8") if args.evidence.is_file() else HEADER + "\n"
        updated = merge(results, text)
        if updated != text:
            args.evidence.write_text(updated, encoding="utf-8")
    print(f"backend evidence: {len(results)} function(s), "
          f"{sum(1 for passed, _ in results.values() if not passed)} mismatch(es)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
