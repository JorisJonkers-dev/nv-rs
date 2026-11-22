# SPDX-License-Identifier: GPL-3.0-only
"""The guard of nv-rs#273: an extender answers "installed" in the general answer only when its whole scanned command
set is built.

Turning an installed answer on too early makes every script that calls a missing command stop at that line. The general
answer is the row of `.fork/capabilities.json` with a `reported_installed` text (xNVSE itself always answers, so it has
none and is held to the same rule): such an extender must have `implemented == total` and as many implemented names as
commands. The one named exception is the Tale of Two Wastelands dependency answers (nv-rs#399, #403, #404): TTW 3.4's
scripts check each plugin's version and ask for a handful of its commands, and without an "installed" answer the start
script stops at five message boxes. Those extenders answer "installed" with only the commands TTW calls built (listed
below with the reason); every other command of them stays missing and a mod that calls one fails at that line.

An exception pins the number of commands TTW needs: the extender must have at least that many built (a command lost
would bring the five message boxes back), and when the set is whole the exception must be removed (the test below fails
on an exception that no longer applies). Building more commands for other mods (the commands the owner's modded profile
calls, built group by group) needs no change here: the answer is only more true with them.

Run: python3 -I -m unittest discover -s scripts/fork/tests -v        (no network, no cargo, nothing created)
"""
import json
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent.parent
CAPABILITIES = REPO / ".fork" / "capabilities.json"

# id -> (commands built when the exception was written, why the installed answer is still right)
TTW_DEPENDENCY_ANSWERS = {
    "johnnyguitar": (
        5,
        "TTW 3.4 asks for version 5.01 or newer and calls five commands (IsDLLLoaded, InitExtraMiscStat, ModExtraMiscStat, "
        "GetExtraMiscStat, GetQuestFailed); all five are built (nv-rs#399, #403)",
    ),
    "showoff": (
        1,
        "TTW 3.4 asks for version 1.80 or newer and calls one command (GetActorHasBaseFlag); it is built (nv-rs#399, #404)",
    ),
    "stewie-tweaks": (
        0,
        "TTW 3.4 asks for version 9.41 or newer and calls none of its script commands; the answer stands for the engine "
        "fixes built so far, behind the NV_STEWIE row (nv-rs#399)",
    ),
}


def answers_installed(extender):
    """Whether the engine answers "installed" for the extender in the general answer: a row with a `reported_installed`
    text, or xNVSE, which is always there."""
    return bool(extender.get("reported_installed")) or extender["id"] == "xnvse"


def problems(doc, exceptions=TTW_DEPENDENCY_ANSWERS):
    """What is wrong with a capabilities document, as text lines; none when the rule holds."""
    out = []
    for e in doc["extenders"]:
        ident, c = e["id"], e["commands"]
        built, total, names = c["implemented"], c["total"], len(c["implemented_names"])
        whole = built == total and names == total
        if not answers_installed(e):
            if ident in exceptions:
                out.append(f"{ident}: listed as an exception but does not answer installed")
            continue
        if ident in exceptions:
            pinned, why = exceptions[ident]
            if not why.strip():
                out.append(f"{ident}: the exception has no reason")
            if whole:
                out.append(f"{ident}: its whole set ({total}) is built: remove it from the exceptions")
            elif built < pinned:
                out.append(f"{ident}: {built} of {total} commands built, the exception was written for {pinned}: check that the installed answer still holds for TTW, then update it")
        elif not whole:
            out.append(f"{ident}: answers installed with {built} of {total} scanned commands built (names listed: {names})")
    return out


def missing_rows(doc, exceptions=TTW_DEPENDENCY_ANSWERS):
    """Exceptions for an extender that capabilities.json no longer has."""
    seen = {e["id"] for e in doc["extenders"]}
    return [f"{ident}: listed as an exception but not in capabilities.json" for ident in sorted(set(exceptions) - seen)]


def doc(*extenders):
    return {"schema_version": 1, "extenders": list(extenders)}


def ext(ident, built, total, installed=True, names=None):
    e = {"id": ident, "name": ident, "commands": {"implemented": built, "total": total, "implemented_names": ["c"] * (built if names is None else names)}}
    if installed:
        e["reported_installed"] = "when the data folder holds TaleOfTwoWastelands.esm"
    return e


class GuardTests(unittest.TestCase):
    def test_the_committed_capabilities_obey_the_rule(self):
        committed = json.loads(CAPABILITIES.read_text())
        self.assertEqual(problems(committed), [])
        self.assertEqual(missing_rows(committed), [])

    def test_flipping_a_committed_extender_on_early_fails(self):
        committed = json.loads(CAPABILITIES.read_text())
        for e in committed["extenders"]:
            if e["id"] == "jip-ln":
                e["commands"]["implemented"] -= 1  # one scanned command lost its handler while the answer stays on
        self.assertTrue(any(p.startswith("jip-ln:") for p in problems(committed)))

    def test_the_exceptions_are_the_ttw_dependency_answers_and_each_has_a_reason(self):
        self.assertEqual(sorted(TTW_DEPENDENCY_ANSWERS), ["johnnyguitar", "showoff", "stewie-tweaks"])
        for ident, (built, why) in TTW_DEPENDENCY_ANSWERS.items():
            self.assertGreaterEqual(built, 0)
            self.assertGreater(len(why), 40, ident)

    def test_a_whole_extender_may_answer_installed(self):
        self.assertEqual(problems(doc(ext("xnvse", 654, 654, installed=False), ext("jip-ln", 1118, 1118), ext("ttw-nvse", 3, 3))), [])

    def test_flipping_an_extender_on_early_fails(self):
        early = doc(ext("jip-ln", 1118, 1118), ext("extender-b", 10, 38))
        self.assertEqual(problems(early), ["extender-b: answers installed with 10 of 38 scanned commands built (names listed: 10)"])

    def test_an_extender_that_does_not_answer_installed_may_be_incomplete(self):
        self.assertEqual(problems(doc(ext("extender-c", 0, 28, installed=False))), [])

    def test_xnvse_always_answers_so_it_must_be_whole(self):
        self.assertEqual(len(problems(doc(ext("xnvse", 600, 654, installed=False)))), 1)

    def test_names_that_do_not_cover_the_commands_fail(self):
        self.assertEqual(len(problems(doc(ext("jip-ln", 1118, 1118, names=1000)))), 1)

    def test_an_exception_holds_while_the_built_set_is_what_it_was_written_for(self):
        d = doc(ext("johnnyguitar", 5, 412), ext("showoff", 1, 215), ext("stewie-tweaks", 0, 8))
        self.assertEqual(problems(d), [])

    def test_building_fewer_than_the_exception_was_written_for_asks_for_it_to_be_looked_at(self):
        d = doc(ext("johnnyguitar", 4, 412), ext("showoff", 1, 215), ext("stewie-tweaks", 0, 8))
        self.assertEqual(len(problems(d)), 1)
        self.assertIn("update it", problems(d)[0])

    def test_building_more_than_the_exception_was_written_for_needs_no_change(self):
        d = doc(ext("johnnyguitar", 9, 412), ext("showoff", 30, 215), ext("stewie-tweaks", 0, 8))
        self.assertEqual(problems(d), [])

    def test_an_exception_that_became_whole_must_be_removed(self):
        d = doc(ext("johnnyguitar", 412, 412), ext("showoff", 1, 215), ext("stewie-tweaks", 0, 8))
        self.assertIn("remove it from the exceptions", problems(d)[0])

    def test_an_exception_needs_a_reason_an_installed_answer_and_a_row(self):
        self.assertIn("no reason", problems(doc(ext("x", 1, 5)), {"x": (1, " ")})[0])
        self.assertIn("does not answer installed", problems(doc(ext("x", 1, 5, installed=False)), {"x": (1, "because")})[0])
        self.assertIn("not in capabilities.json", missing_rows(doc(ext("y", 1, 1)), {"x": (1, "because")})[0])

    def test_an_exception_does_not_cover_another_extender(self):
        d = doc(ext("johnnyguitar", 5, 412), ext("extender-b", 10, 38))
        self.assertEqual(len(problems(d)), 1)
        self.assertTrue(problems(d)[0].startswith("extender-b:"))


if __name__ == "__main__":
    unittest.main()
