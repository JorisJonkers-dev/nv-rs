# SPDX-License-Identifier: GPL-3.0-only
"""Tests of the built-in extender move inventory (scripts/fork/extender_move_inventory.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v      (no network, no cargo, no writes)
"""
import sys
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import extender_move_inventory as inventory  # noqa: E402


class InventoryTests(unittest.TestCase):
    def test_the_committed_inventory_matches_a_fresh_generation(self):
        self.assertEqual(inventory.check(inventory.REPO), [])

    def test_the_inventory_is_generated_for_every_family(self):
        doc = inventory.generate(inventory.REPO)
        for family in inventory.FAMILY_ORDER:
            self.assertIn(f"## {family}", doc)
        findings = inventory.scan(inventory.REPO)
        self.assertTrue(findings, "the reader found no field read in any built-in command")
        families = {f.family for f in findings}
        self.assertTrue(families & set(inventory.FAMILY_ORDER))
        for finding in findings:
            self.assertIn(finding.family, inventory.FAMILY_ORDER)
            self.assertTrue(finding.function)
            self.assertIn(f"crates/{finding.crate}/src/{finding.file}", doc)

    def test_a_planted_direct_read_of_a_moved_family_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "crates" / "jg" / "src" / "commands.rs"
            source.parent.mkdir(parents=True)
            source.write_text(
                "fn stolen(e: &mut Engine, obj: u32) {\n"
                "    let head = e.mem.u32(obj + HEAD);\n"
                "    let _ = head;\n"
                "}\n"
            )
            head = inventory.Field("lockfree", "head", "HEAD", 8, moved=True)
            found = inventory.problems(root, fields=(head,), crates=("jg",))
            self.assertEqual(len(found), 1)
            for part in ("lockfree", "head", "stolen", "commands.rs:2"):
                self.assertIn(part, found[0])

            # A call to the family's typed function at its exe address stays callable.
            source.write_text(
                "fn allowed(e: &mut Engine, obj: u32) {\n"
                "    e.call(TYPED_HEAD, &[obj]);\n"
                "}\n"
            )
            self.assertEqual(inventory.problems(root, fields=(head,), crates=("jg",)), [])

    def test_a_scheduled_family_is_inventoried_without_failing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "crates" / "jg" / "src" / "commands.rs"
            source.parent.mkdir(parents=True)
            source.write_text("fn reads(e: &Engine, obj: u32) -> u32 { e.mem.u32(obj + HEAD) }\n")
            head = inventory.Field("forms", "head", "HEAD", 8)
            self.assertEqual(inventory.problems(root, fields=(head,), crates=("jg",)), [])
            self.assertEqual([f.function for f in inventory.scan(root, fields=(head,), crates=("jg",))], ["reads"])

    def test_opcode_bases_and_cosave_ids_are_unchanged(self):
        doc = inventory.generate(inventory.REPO)
        self.assertEqual(inventory.pin_problems(inventory.REPO), [])
        for pin in inventory.PINS:
            self.assertIn(f"`{pin.expected}`", doc)
            changed = replace(pin, expected="0xdead")
            self.assertTrue(any(pin.extender in p for p in inventory.pin_problems(inventory.REPO, pins=(changed,))))

    def test_the_inventory_names_writes_as_well_as_reads(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "crates" / "jg" / "src" / "commands.rs"
            source.parent.mkdir(parents=True)
            source.write_text("fn writes(e: &mut Engine, obj: u32) { e.mem.set_u32(obj + HEAD, 1); }\n")
            head = inventory.Field("forms", "head", "HEAD", 8)
            self.assertEqual([f.access for f in inventory.scan(root, fields=(head,), crates=("jg",))], ["write"])


if __name__ == "__main__":
    unittest.main()
