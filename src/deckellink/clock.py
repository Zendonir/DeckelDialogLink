"""Time abstraction.

All transfer, receive and simulation code obtains time through a :class:`Clock`.
This keeps timeouts and delays deterministic in tests (:class:`FakeClock`) and
allows the GUI simulator to run faster than real time (:class:`ScaledClock`).
"""

from __future__ import annotations

import threading
import time
from datetime import UTC, datetime, timedelta
from typing import Protocol


class Clock(Protocol):
    """Source of monotonic time, wall-clock time and interruptible sleeps."""

    def monotonic(self) -> float:
        """Monotonic time in seconds."""
        ...

    def now(self) -> datetime:
        """Timezone-aware wall-clock time."""
        ...

    def sleep(self, seconds: float, interrupt: threading.Event | None = None) -> bool:
        """Sleep for ``seconds``. Return ``True`` if ``interrupt`` was set."""
        ...


class SystemClock:
    """Real time."""

    def monotonic(self) -> float:
        return time.monotonic()

    def now(self) -> datetime:
        return datetime.now().astimezone()

    def sleep(self, seconds: float, interrupt: threading.Event | None = None) -> bool:
        if interrupt is not None and interrupt.is_set():
            return True
        if seconds <= 0:
            return False
        if interrupt is None:
            time.sleep(seconds)
            return False
        return interrupt.wait(seconds)


class ScaledClock:
    """Real time running ``scale`` times faster (used for the GUI simulator in tests)."""

    def __init__(self, scale: float = 1.0) -> None:
        if scale <= 0:
            raise ValueError("scale must be positive")
        self._scale = scale
        self._origin = time.monotonic()

    @property
    def scale(self) -> float:
        return self._scale

    def monotonic(self) -> float:
        return (time.monotonic() - self._origin) * self._scale

    def now(self) -> datetime:
        return datetime.now().astimezone()

    def sleep(self, seconds: float, interrupt: threading.Event | None = None) -> bool:
        return SystemClock().sleep(seconds / self._scale, interrupt)


class FakeClock:
    """Virtual time that only advances when ``sleep`` or ``advance`` is called."""

    def __init__(self, start: datetime | None = None) -> None:
        self._start = start or datetime(2026, 1, 1, 8, 0, 0, tzinfo=UTC)
        self._elapsed = 0.0
        self._lock = threading.Lock()

    def monotonic(self) -> float:
        with self._lock:
            return self._elapsed

    def now(self) -> datetime:
        with self._lock:
            return self._start + timedelta(seconds=self._elapsed)

    def advance(self, seconds: float) -> None:
        with self._lock:
            self._elapsed += max(0.0, seconds)

    def sleep(self, seconds: float, interrupt: threading.Event | None = None) -> bool:
        if interrupt is not None and interrupt.is_set():
            return True
        self.advance(seconds)
        return interrupt is not None and interrupt.is_set()
