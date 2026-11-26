"""Tests of the one-family typed test lab (scripts/fork/family_lab.py).

Run: python3 -I -m unittest discover -s scripts/fork/tests -v
No cargo, no network, no files outside a temporary directory.
"""
import re
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
FORK = HERE.parent
REPO = FORK.parent.parent
sys.path.insert(0, str(FORK))

import family_lab  # noqa: E402

TESTS = REPO / "crates" / "engine" / "tests"
CASES = re.compile(
    r"entry!\s*\(\s*0x([0-9a-fA-F_]+)|Diff::new\(\s*\(\s*0x([0-9a-fA-F_]+)"
)


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
        expected = {
            "anim_keys": ["engine-gamebryo"],
            "crt": ["engine-fallout-unplaced-b"],
            "fade": [
                "engine-bs",
                "engine-fallout-cell",
                "engine-fallout-forms",
                "engine-fallout-magic",
                "engine-gamebryo",
            ],
            "lights": [
                "engine-bs",
                "engine-fallout-cell",
                "engine-fallout-facegen",
                "engine-fallout-forms",
                "engine-fallout-unplaced-b",
                "engine-gamebryo",
            ],
        }
        for name, wanted in expected.items():
            parts = family_lab.assembly_for(TESTS / f"typed_{name}.rs")
            self.assertEqual(parts, wanted, name)
            self.assertTrue(parts)
            self.assertLess(len(parts), len(all_parts), name)
            self.assertNotIn("engine", parts)
            for part in parts:
                self.assertIn(part, all_parts)
        # Crates the earlier whole-file hex sweep pulled in, but these cases never call.
        fade = family_lab.assembly_for(TESTS / "typed_fade.rs")
        for unrelated in ("engine-havok", "engine-fallout-pathfinding", "engine-fallout-script"):
            self.assertNotIn(unrelated, fade)

    def test_a_generated_target_keeps_every_diff_case_and_links_no_facade(self):
        source = TESTS / "typed_fade.rs"
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "lab"
            family_lab.generate(source, out)
            manifest = (out / "Cargo.toml").read_text(encoding="utf-8")
            target = (out / "tests" / "typed_fade.rs").read_text(encoding="utf-8")
            self.assertEqual(cases(source.read_text(encoding="utf-8")), cases(target))
            self.assertNotIn("use engine as _;", target)
            parts = family_lab.assembly_for(source)
            assembled = ", ".join(
                ["engine_typed::units::funcs"]
                + [f"{p.replace('-', '_')}::units::funcs" for p in parts]
            )
            self.assertTrue(target.startswith("//!"))
            self.assertIn(f"engine_core::assemble!({assembled});", target)
            self.assertNotIn("\nengine =", manifest)
            self.assertIn("engine-core =", manifest)
            self.assertIn("engine-typed =", manifest)
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
                manifest = (out / "Cargo.toml").read_text(encoding="utf-8")
                self.assertNotIn("use engine as _;", text)
                self.assertEqual(
                    cases(source.read_text(encoding="utf-8")), without_shim(text)
                )
                self.assertEqual(manifest.count("[[test]]"), 1)
                self.assertIn(f'name = "{source.stem}"', manifest)
                self.assertIn(f'path = "tests/{source.stem}.rs"', manifest)
                total += len(without_shim(text))
        self.assertGreater(total, 900)
        self.assertEqual(total, sum(len(cases(p.read_text(encoding="utf-8"))) for p in suites))

    def test_the_lights_target_keeps_its_guard_helper_without_the_facade(self):
        source = TESTS / "typed_lights.rs"
        self.assertIn("engine::dead_lights::install", source.read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "lab"
            family_lab.generate(source, out)
            target = (out / "tests" / "typed_lights.rs").read_text(encoding="utf-8")
            self.assertTrue(target.startswith("//!"))
            self.assertIn("mod engine", target)
            self.assertIn("pub fn install", target)
            self.assertIn("engine_bs::units::bsshader::run_00bb4520::fn_00bb4740", target)
            self.assertIn(
                "engine_fallout_unplaced_b::units::unplaced::run_00c048b0::fn_00c058f0",
                target,
            )
            self.assertNotIn("crate::units::", target)
            self.assertIn("crate::engine::dead_lights::install", target)
            self.assertIn("engine_typed::units::funcs", target)
            manifest = (out / "Cargo.toml").read_text(encoding="utf-8")
            self.assertNotIn("\nengine =", manifest)
            self.assertIn("engine-fallout-unplaced-b =", manifest)

    def test_the_hygiene_rule_survives_the_copy(self):
        """Every `// Typed from` has a registered Diff case, and the copy keeps its cases.

        The repository's nvinspect test is the authority on the full rule (it also
        checks registration and the assertion chain); this is the address-level part
        the lab depends on, plus a check that generation does not drop a case.
        """
        marked = set()
        registered = set()
        for source in REPO.glob("crates/**/*.rs"):
            text = source.read_text(encoding="utf-8", errors="replace")
            marked.update(a.lower() for a in re.findall(r"// Typed from ([0-9a-fA-F]{8})", text))
            for match in CASES.finditer(text):
                address = match.group(1) or match.group(2)
                registered.add(address.replace("_", "").lower())
        self.assertTrue(marked)
        self.assertEqual(marked - registered, set())
        for source in TESTS.glob("typed_*.rs"):
            with tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp) / "lab"
                family_lab.generate(source, out)
                before = source.read_text(encoding="utf-8")
                after = (out / "tests" / f"{source.stem}.rs").read_text(encoding="utf-8")
                self.assertEqual(before.count("Diff::new("), after.count("Diff::new("))
                self.assertEqual(before.count(".assert_same()"), after.count(".assert_same()"))

    def test_included_tables_reach_the_generated_crate(self):
        source = TESTS / "typed_shader.rs"
        names = family_lab.INCLUDE.findall(source.read_text(encoding="utf-8"))
        self.assertTrue(names)
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "lab"
            family_lab.generate(source, out)
            for name in names:
                self.assertEqual(
                    (source.parent / name).read_text(encoding="utf-8"),
                    (out / "tests" / name).read_text(encoding="utf-8"),
                )

    def test_the_go_task_separator_is_removed_before_the_family_name(self):
        self.assertEqual(
            family_lab.parse_run_args(["--", "fade", "culling_fade_on_visible"]),
            ("fade", ["culling_fade_on_visible"]),
        )
        self.assertEqual(
            family_lab.parse_run_args(["fade", "--", "--exact", "a"]),
            ("fade", ["--exact", "a"]),
        )
        self.assertEqual(family_lab.parse_run_args(["fade"]), ("fade", []))
        with self.assertRaises(ValueError):
            family_lab.parse_run_args([])

    def test_the_cargo_wrapper_is_an_environment_override(self):
        with mock.patch.dict("os.environ", {}, clear=True):
            self.assertEqual(family_lab.cargo_command(), ["cargo"])
        wrapped = "flock ~/.cache/nv-workers/cargo.lock ~/.local/bin/capped 10G cargo"
        with mock.patch.dict("os.environ", {"NV_FAMILY_LAB_CARGO": wrapped}):
            command = family_lab.cargo_command()
        self.assertEqual(
            command,
            [
                "flock",
                str(Path.home() / ".cache/nv-workers/cargo.lock"),
                str(Path.home() / ".local/bin/capped"),
                "10G",
                "cargo",
            ],
        )

    def test_a_run_goes_through_the_daemon_client(self):
        with mock.patch.object(family_lab, "daemon_command", return_value=["client"]), \
                mock.patch.object(family_lab, "suite", return_value=Path("typed_fade.rs")), \
                mock.patch.object(family_lab, "generate", return_value=Path("out/Cargo.toml")), \
                mock.patch.object(family_lab.subprocess, "call", return_value=7) as call:
            self.assertEqual(family_lab.run(["fade", "--", "--exact", "x"]), 7)
        call.assert_called_once_with([
            "client", "test", "--manifest-path", "out/Cargo.toml", "--test", "typed_fade",
            "--", "--exact", "x",
        ])

    def test_the_client_command_is_the_real_build_daemon_entry_point(self):
        command = family_lab.daemon_command()
        self.assertEqual(command[-2:], ["run", "--"])
        self.assertEqual(Path(command[1]).name, "build_daemon.py")
        self.assertTrue(Path(command[1]).is_file())


if __name__ == "__main__":
    unittest.main()
