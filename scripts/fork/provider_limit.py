# SPDX-License-Identifier: GPL-3.0-only
"""Per-provider concurrency limit for porting runs (standard library only)."""
import contextlib
import threading

ENV_LIMIT = "NV_PORT_PROVIDER_LIMIT"


def provider_of(model):
    """The provider part of ``provider/model``; a bare name is its own provider."""
    if not model:
        return "unknown"
    provider = model.split("/", 1)[0]
    return provider or "unknown"


class ProviderLimiter:
    """Hold at most ``limit`` concurrent runs per provider, recording the peak."""

    def __init__(self, limit):
        self.limit = max(1, int(limit))
        self.peak = {}
        self._lock = threading.Lock()
        self._semaphores = {}
        self._held = {}

    def _semaphore(self, provider):
        """Create the provider's bounded semaphore only once, under the lock."""
        with self._lock:
            if provider not in self._semaphores:
                self._semaphores[provider] = threading.BoundedSemaphore(self.limit)
                self._held[provider] = 0
                self.peak[provider] = 0
            return self._semaphores[provider]

    @contextlib.contextmanager
    def hold(self, model):
        """Block until this provider has room, then count the run until exit."""
        provider = provider_of(model)
        semaphore = self._semaphore(provider)
        semaphore.acquire()
        with self._lock:
            self._held[provider] += 1
            if self._held[provider] > self.peak[provider]:
                self.peak[provider] = self._held[provider]
        try:
            yield
        finally:
            with self._lock:
                self._held[provider] -= 1
            semaphore.release()


def limit_from_env(env, default=4):
    """Read ``NV_PORT_PROVIDER_LIMIT``; unusable values fall back to ``default``."""
    try:
        value = int(env.get(ENV_LIMIT))
    except (AttributeError, TypeError, ValueError):
        return default
    return value if value >= 1 else default
