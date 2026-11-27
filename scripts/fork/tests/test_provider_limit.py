# SPDX-License-Identifier: GPL-3.0-only
"""Tests of provider_limit.py: provider names, per-provider caps and the env knob."""
import sys
import threading
import time
import unittest
from pathlib import Path

FORK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FORK))

import provider_limit as pl  # noqa: E402


class ProviderLimitTests(unittest.TestCase):
    def test_provider_of(self):
        self.assertEqual(pl.provider_of("free/m"), "free")
        self.assertEqual(pl.provider_of("hosted/team/m"), "hosted")
        self.assertEqual(pl.provider_of("bare-model"), "bare-model")
        self.assertEqual(pl.provider_of(""), "unknown")
        self.assertEqual(pl.provider_of(None), "unknown")
        self.assertEqual(pl.provider_of("/m"), "unknown")

    def test_a_limit_below_one_is_one(self):
        limiter = pl.ProviderLimiter(0)
        with limiter.hold("free/m"):
            self.assertEqual(limiter.peak["free"], 1)

    def test_hold_caps_a_provider_and_records_the_peak(self):
        limiter = pl.ProviderLimiter(2)
        gate = threading.Barrier(2)
        errors = []

        def worker():
            try:
                with limiter.hold("free/m"):
                    # Two admitted runs meet here, so the peak is deterministic.
                    gate.wait(0.05)
                    time.sleep(0.02)
            except Exception as exc:
                errors.append(exc)

        threads = [threading.Thread(target=worker) for _ in range(20)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()

        self.assertEqual(errors, [])
        self.assertLessEqual(limiter.peak["free"], 2)
        self.assertEqual(limiter.peak["free"], 2)

    def test_two_providers_do_not_block_each_other(self):
        limiter = pl.ProviderLimiter(1)
        a_held = threading.Event()
        b_entered = threading.Event()
        release_a = threading.Event()
        errors = []

        def hold_a():
            try:
                with limiter.hold("a/x"):
                    a_held.set()
                    release_a.wait(0.05)
            except Exception as exc:
                errors.append(exc)

        def hold_b():
            try:
                if a_held.wait(0.05):
                    with limiter.hold("b/x"):
                        b_entered.set()
            except Exception as exc:
                errors.append(exc)

        threads = [threading.Thread(target=hold_a), threading.Thread(target=hold_b)]
        for thread in threads:
            thread.start()
        try:
            self.assertTrue(b_entered.wait(0.05), "provider b was blocked by provider a")
        finally:
            release_a.set()
            for thread in threads:
                thread.join()

        self.assertEqual(errors, [])

    def test_limit_from_env(self):
        self.assertEqual(pl.limit_from_env({}), 4)
        self.assertEqual(pl.limit_from_env({"NV_PORT_PROVIDER_LIMIT": "3"}), 3)
        self.assertEqual(pl.limit_from_env({"NV_PORT_PROVIDER_LIMIT": "x"}), 4)
        self.assertEqual(pl.limit_from_env({"NV_PORT_PROVIDER_LIMIT": "0"}), 4)
        self.assertEqual(pl.limit_from_env({"NV_PORT_PROVIDER_LIMIT": "-2"}, default=7), 7)


if __name__ == "__main__":
    unittest.main()
