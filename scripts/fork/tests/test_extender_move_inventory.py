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


def crate_file(root: Path, crate: str, name: str, text: str) -> None:
    path = root / "crates" / crate / "src" / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


COMMANDS = "pub static COMMANDS: Handlers = &[(0x01, stolen),];\n"


class InventoryTests(unittest.TestCase):
    def test_the_committed_inventory_matches_a_fresh_generation(self):
        self.assertEqual(inventory.check(inventory.REPO), [])

    def test_the_inventory_is_generated_for_every_family(self):
        doc = inventory.generate(inventory.REPO)
        for family in inventory.FAMILY_ORDER:
            self.assertIn(f"## {family}", doc)
        findings = inventory.scan(inventory.REPO)
        self.assertTrue(findings, "the reader found no field read in any built-in command")
        for finding in findings:
            self.assertIn(finding.family, inventory.FAMILY_ORDER)
            self.assertTrue(finding.function)
            self.assertTrue(finding.command)
            self.assertIn(f"crates/{finding.crate}/src/{finding.file}", doc)

    def test_known_reads_are_attributed_to_the_handler_that_reaches_the_helper(self):
        findings = inventory.scan(inventory.REPO)
        self.assertTrue(
            any(
                f.file == "commands/havok_refs.rs" and f.line == 401 and f.command == "is_mobile"
                for f in findings
            ),
            "the helper's collision-object read must be listed under is_mobile",
        )
        self.assertTrue(
            any(
                f.file == "invref.rs" and f.line == 859 and f.command == "copy_ir"
                for f in findings
            ),
            "a read outside commands/ must be listed under the command that calls invref",
        )
        self.assertIn(
            "| `copy_ir` | `base form` | `+0x20` | read | `crates/nvse/src/invref.rs:859` |",
            inventory.generate(inventory.REPO),
        )

    def test_a_planted_direct_read_of_a_moved_family_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            crate_file(
                root,
                "jg",
                "commands.rs",
                COMMANDS + "fn stolen(e: &mut Engine, obj: u32) {\n"
                "    let head = e.mem.u32(obj + HEAD);\n"
                "    let _ = head;\n"
                "}\n",
            )
            head = inventory.Field("lockfree", "head", "HEAD", 8, moved=True)
            found = inventory.problems(root, fields=(head,), crates=("jg",))
            self.assertEqual(len(found), 1)
            for part in ("lockfree", "head", "`stolen`", "commands.rs:3", "typed function"):
                self.assertIn(part, found[0])

            # A call to the family's typed function at its exe address stays callable.
            crate_file(
                root,
                "jg",
                "commands.rs",
                COMMANDS + "fn stolen(e: &mut Engine, obj: u32) {\n"
                "    e.call(TYPED_HEAD, &[obj]);\n"
                "}\n",
            )
            self.assertEqual(inventory.problems(root, fields=(head,), crates=("jg",)), [])

    def test_a_planted_read_through_an_imported_alias_in_a_helper_fails_at_the_handler(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            crate_file(root, "jg", "fields.rs", "pub const HEAD: u32 = 0x14;\n")
            crate_file(
                root,
                "jg",
                "invref.rs",
                "use crate::fields::HEAD;\n"
                "pub fn helper(e: &Engine, obj: u32) -> u32 {\n"
                "    e.mem.u32(obj + HEAD)\n"
                "}\n",
            )
            crate_file(
                root,
                "jg",
                "commands.rs",
                COMMANDS + "use crate::invref::{self};\n"
                "fn stolen(e: &mut Engine, obj: u32) {\n"
                "    let _ = invref::helper(e, obj);\n"
                "}\n",
            )
            head = inventory.Field("lockfree", "head", "HEAD", 8, moved=True)
            found = inventory.problems(root, fields=(head,), crates=("jg",))
            self.assertEqual(len(found), 1)
            for part in ("invref.rs:3", "`helper`", "`stolen`", "lockfree"):
                self.assertIn(part, found[0])

    def test_a_planted_read_split_over_lines_and_bound_to_a_name_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            crate_file(
                root,
                "jg",
                "commands.rs",
                COMMANDS + "use crate::fields::HEAD;\n"
                "fn stolen(e: &mut Engine, obj: u32) -> u32 {\n"
                "    let at = HEAD;\n"
                "    e.mem.u32(\n"
                "        obj + at,\n"
                "    )\n"
                "}\n",
            )
            head = inventory.Field("lockfree", "head", "HEAD", 8, moved=True)
            found = inventory.problems(root, fields=(head,), crates=("jg",))
            self.assertEqual(len(found), 1)
            self.assertIn("commands.rs:5", found[0])
            self.assertIn("`stolen`", found[0])

    def test_the_committed_moved_fields_guard_a_planted_read(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            crate_file(
                root,
                "jg",
                "commands.rs",
                COMMANDS + "fn stolen(e: &mut Engine, map: u32) -> u32 {\n"
                "    e.mem.u32(map + pInterfaceManager)\n"
                "}\n",
            )
            found = inventory.problems(root, crates=("jg",))
            self.assertEqual(len(found), 1)
            for part in ("lock-free maps", "interface manager", "+0x14", "`stolen`"):
                self.assertIn(part, found[0])

    def test_a_scheduled_family_is_inventoried_without_failing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            crate_file(
                root,
                "jg",
                "commands.rs",
                COMMANDS + "fn stolen(e: &Engine, obj: u32) -> u32 { e.mem.u32(obj + HEAD) }\n",
            )
            head = inventory.Field("forms", "head", "HEAD", 8)
            self.assertEqual(inventory.problems(root, fields=(head,), crates=("jg",)), [])
            self.assertEqual(
                [f.function for f in inventory.scan(root, fields=(head,), crates=("jg",))],
                ["stolen"],
            )

    def test_a_note_in_a_comment_is_not_a_read(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            crate_file(
                root,
                "jg",
                "commands.rs",
                COMMANDS + "fn stolen(e: &Engine, obj: u32) -> u32 {\n"
                "    let x = 1; // HEAD + 4 is read here\n"
                "    x\n"
                "}\n",
            )
            head = inventory.Field("lockfree", "head", "HEAD", 8, moved=True)
            self.assertEqual(inventory.problems(root, fields=(head,), crates=("jg",)), [])

    def test_opcode_bases_and_cosave_ids_are_unchanged(self):
        doc = inventory.generate(inventory.REPO)
        self.assertEqual(inventory.pin_problems(inventory.REPO), [])
        for pin in inventory.PINS:
            row = f"| {pin.extender} | {pin.kind} | `{pin.expected}` | `{pin.file}` |"
            self.assertIn(row, doc)
            changed = replace(pin, expected="0xdead")
            self.assertTrue(any(pin.extender in p for p in inventory.pin_problems(inventory.REPO, pins=(changed,))))

    def test_a_cosave_id_rebound_to_another_record_fails(self):
        pin = next(p for p in inventory.PINS if p.file == "crates/jip/src/cosave.rs")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = (inventory.REPO / pin.file).read_text()
            rebound = source.replace(
                'SCRIPT_VARS: u32 = tag(b"VSPJ")', 'SCRIPT_VARS: u32 = tag(b"VAPJ")'
            ).replace('AUX_VARS: u32 = tag(b"VAPJ")', 'AUX_VARS: u32 = tag(b"VSPJ")')
            self.assertNotEqual(rebound, source)
            self.assertIn('tag(b"VSPJ")', rebound)  # the bytes survive on another record
            path = root / pin.file
            path.parent.mkdir(parents=True)
            path.write_text(rebound)
            found = inventory.pin_problems(root, pins=(pin,))
            self.assertEqual(len(found), 1)
            self.assertIn("expected `VSPJ`", found[0])

    def test_a_missing_pin_source_is_reported_not_raised(self):
        with tempfile.TemporaryDirectory() as tmp:
            found = inventory.pin_problems(Path(tmp), pins=(inventory.PINS[0],))
            self.assertEqual(len(found), 1)
            self.assertIn("missing", found[0])

    def test_the_inventory_names_writes_as_well_as_reads(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            crate_file(
                root,
                "jg",
                "commands.rs",
                COMMANDS + "fn stolen(e: &mut Engine, obj: u32) { e.mem.set_u32(obj + HEAD, 1); }\n",
            )
            head = inventory.Field("forms", "head", "HEAD", 8)
            self.assertEqual([f.access for f in inventory.scan(root, fields=(head,), crates=("jg",))], ["write"])


if __name__ == "__main__":
    unittest.main()
