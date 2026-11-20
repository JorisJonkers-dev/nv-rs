# SPDX-License-Identifier: GPL-3.0-only
"""The public-text check finds machine details in a synthetic example and passes role words and placeholders."""
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("public_text", Path(__file__).resolve().parents[1] / "public_text.py")
public_text = importlib.util.module_from_spec(spec)
spec.loader.exec_module(public_text)


class PublicText(unittest.TestCase):
    def test_a_card_a_device_id_a_home_path_and_a_wrapper_are_found(self):
        text = "\n".join([
            "numbers from an RX " + "6800",
            "adapter 10de:" + "2684",
            "saves under /home/" + "carol/Games",
            "run it through nv-rs-" + "run",
            "on the owner's " + "session",
        ])
        kinds = [what for _, what, _ in public_text.findings(text)]
        self.assertEqual(len(kinds), 5, kinds)

    def test_role_words_and_placeholders_pass(self):
        text = "a recent AMD card on RADV\n/home/someone/x and /home/runner/work\nnv-rs-run-summary/1\nthe owner's call\n"
        self.assertEqual(public_text.findings(text), [])

    def test_the_tree_is_clean(self):
        self.assertEqual(public_text.check(), 0)


if __name__ == "__main__":
    unittest.main()
