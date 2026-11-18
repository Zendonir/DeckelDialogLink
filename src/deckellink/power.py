"""Prevent system standby while a drip-feed transfer is running (Windows only)."""

from __future__ import annotations

import logging
import sys
from collections.abc import Iterator
from contextlib import contextmanager

log = logging.getLogger(__name__)

ES_CONTINUOUS = 0x80000000
ES_SYSTEM_REQUIRED = 0x00000001


def _set_execution_state(flags: int) -> bool:
    if sys.platform != "win32":
        return False
    try:
        import ctypes

        windll = getattr(ctypes, "windll", None)
        if windll is None:
            return False
        return bool(windll.kernel32.SetThreadExecutionState(flags))
    except (OSError, AttributeError) as exc:
        log.warning("SetThreadExecutionState failed: %s", exc)
        return False


@contextmanager
def keep_system_awake(enabled: bool = True) -> Iterator[bool]:
    """While active, Windows does not enter standby. Yields whether it is effective."""
    active = enabled and _set_execution_state(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)
    if active:
        log.info("System standby suppressed during drip-feed")
    try:
        yield active
    finally:
        if active:
            _set_execution_state(ES_CONTINUOUS)
