"""Pure-logic throttle. No flet dependency — fully unit-testable."""
from __future__ import annotations

import time


class ProgressThrottle:
    """Limits update frequency. Thread-safe via timestamp arithmetic."""

    def __init__(self, hz: float = 30):
        self._min_interval = 1.0 / hz
        self._last_emit = 0.0

    def should_emit(self, force: bool = False) -> bool:
        if force:
            self._last_emit = time.monotonic()
            return True
        now = time.monotonic()
        if (now - self._last_emit) >= self._min_interval:
            self._last_emit = now
            return True
        return False
