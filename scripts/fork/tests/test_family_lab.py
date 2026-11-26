"""Tests of the one-family typed test lab (scripts/fork/family_lab.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
No cargo, no network, no files outside a temporary directory.
"""
import re
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
FORK = HERE.parent
REPO = FORK.parent.parent
sys.path.insert(0, str(FORK))

import family_lab  # noqa: E402

TESTS = REPO / "crates" / "engine" / "tests"


def cases(text):
    """Every registered differential case, in source order, as its exe address."""
    return re.findall(r"entry!\s*\(\s*0x([0-9a-fA-F_]+)", text)


def without_shim(text):
    """Case addresses, leaving out the two entries the lights guard hook adds."""
    out = cases(text)
    if "pub mod dead_lights" in text:
        out = [a for a in out if a.replace("_", "").lower() not in ("00bb4740", "00c058f0")]
    return out


class FamilyLab(unittest.TestCase):
    def test_a_family_assembles_only_its_translation_crates(self):
        all_parts = [p.name for p in (REPO / "crates").glob("engine-*") if p.is_dir()]
        for name in ("fade", "crt", "lights"):
            parts = family_lab.assembly_for(TESTS / f"typed_{name}.rs")
            self.assertTrue(parts)
            self.assertLess(len(parts), len(all_parts), name)
            self.assertNotIn("engine", parts)
        self.assertIn("engine-bs", family_lab.assembly_for(TESTS / "typed_fade.rs"))

    def test_a_generated_target_keeps_every_diff_case_and_links_no_facade(self):
        source = TESTS / "typed_fade.rs"
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "lab"
            family_lab.generate(source, out)
            manifest = (out / "Cargo.toml").read_text(encoding="utf-8")
            target = (out / "tests" / "typed_fade.rs").read_text(encoding="utf-8")
            self.assertEqual(cases(source.read_text(encoding="utf-8")), cases(target))
            self.assertNotIn("use engine as _;", target)
            self.assertTrue(
                target.startswith("//!") and "engine_core::assemble!(engine_bs::units::funcs," in target
            )
            self.assertNotIn("\nengine =", manifest)
            self.assertIn('features = ["test-support"]', manifest)
            self.assertIn('name = "typed_fade"', manifest)

    def test_every_typed_suite_gets_one_target_with_its_own_cases(self):
        suites = sorted(TESTS.glob("typed_*.rs"))
        self.assertGreater(len(suites), 30)
        total = 0
        for source in suites:
            with tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp) / "lab"
                family_lab.generate(source, out)
                target = out / "tests" / f"{source.stem}.rs"
                text = target.read_text(encoding="utf-8")
                self.assertNotIn("use engine as _;", text)
                self.assertEqual(
                    cases(source.read_text(encoding="utf-8")), without_shim(text)
                )
                total += len(cases(text))
        self.assertGreater(total, 900)

    def test_the_lights_target_keeps_its_guard_helper_without_the_facade(self):
        source = TESTS / "typed_lights.rs"
        self.assertIn("engine::dead_lights::install", source.read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "lab"
            family_lab.generate(source, out)
            target = (out / "tests" / "typed_lights.rs").read_text(encoding="utf-8")
            self.assertIn("mod engine", target)
            self.assertIn("pub fn install", target)
            self.assertIn("engine_bs::units::bsshader::run_00bb4520::fn_00bb4740", target)
            manifest = (out / "Cargo.toml").read_text(encoding="utf-8")
            self.assertNotIn("\nengine =", manifest)
            self.assertIn("engine-fallout-unplaced-b =", manifest)

    def test_the_hygiene_rule_survives_the_copy(self):
        """Every registered Diff case and its assertion reach the generated target."""
        for source in TESTS.glob("typed_*.rs"):
            with tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp) / "lab"
                family_lab.generate(source, out)
                before = source.read_text(encoding="utf-8")
                after = (out / "tests" / f"{source.stem}.rs").read_text(encoding="utf-8")
                self.assertEqual(before.count("Diff::new("), after.count("Diff::new("))
                self.assertEqual(before.count(".assert_same()"), after.count(".assert_same()"))


if __name__ == "__main__":
    unittest.main()
