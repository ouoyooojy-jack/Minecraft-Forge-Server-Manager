"""
Tests for ui/components.
Only the throttle logic is unit-testable without launching Flet.
"""
from __future__ import annotations

import time

from ui.throttle import ProgressThrottle


def test_throttle_first_emit_returns_true():
    """First call always emits (regardless of interval)."""
    t = ProgressThrottle(hz=10)
    assert t.should_emit() is True


def test_throttle_suppresses_rapid_calls():
    """Second call within the throttle window emits False."""
    t = ProgressThrottle(hz=10)  # 100ms interval
    t.should_emit()
    # Immediately after, second call should be suppressed
    assert t.should_emit() is False


def test_throttle_releases_after_interval():
    """After interval passes, emit returns True again."""
    t = ProgressThrottle(hz=20)  # 50ms interval
    t.should_emit()
    # Sleep well past the interval to account for Windows scheduler jitter
    time.sleep(0.15)
    assert t.should_emit() is True


def test_throttle_force_bypasses():
    """force=True emits regardless of interval."""
    t = ProgressThrottle(hz=10)
    t.should_emit()  # resets timer
    # Immediate force should still emit
    assert t.should_emit(force=True) is True


def test_throttle_initial_state_is_zero():
    """New throttle has last_emit=0 so first call always passes."""
    t = ProgressThrottle(hz=10)
    # Reading the attribute directly to confirm
    assert t._last_emit == 0.0
