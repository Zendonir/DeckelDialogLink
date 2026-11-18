"""Cooperative cancellation."""

from __future__ import annotations

import logging
import threading
from collections.abc import Callable

from deckellink.domain.errors import DeckelLinkError

log = logging.getLogger(__name__)


class OperationCancelled(DeckelLinkError):
    """Raised inside a transfer when the user cancelled it."""


class CancellationToken:
    """Thread-safe cancellation flag with callbacks (e.g. ``transport.cancel``)."""

    def __init__(self) -> None:
        self._event = threading.Event()
        self._callbacks: list[Callable[[], None]] = []
        self._lock = threading.Lock()

    @property
    def event(self) -> threading.Event:
        return self._event

    @property
    def is_cancelled(self) -> bool:
        return self._event.is_set()

    def register(self, callback: Callable[[], None]) -> None:
        with self._lock:
            self._callbacks.append(callback)
            already = self._event.is_set()
        if already:
            self._run(callback)

    def cancel(self) -> None:
        with self._lock:
            if self._event.is_set():
                return
            self._event.set()
            callbacks = list(self._callbacks)
        for callback in callbacks:
            self._run(callback)

    def raise_if_cancelled(self) -> None:
        if self._event.is_set():
            raise OperationCancelled("Vorgang wurde abgebrochen.")

    @staticmethod
    def _run(callback: Callable[[], None]) -> None:
        try:
            callback()
        except Exception as exc:  # cancellation must never fail
            log.debug("Cancellation callback failed: %s", exc)
