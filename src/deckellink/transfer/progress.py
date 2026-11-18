"""Progress calculation."""

from __future__ import annotations

from datetime import datetime

from deckellink.clock import Clock
from deckellink.domain.results import TransferProgress, TransformResult


class ProgressTracker:
    def __init__(self, payload: TransformResult, clock: Clock, started_at: datetime) -> None:
        self._payload = payload
        self._clock = clock
        self._started_at = started_at
        self._t0: float | None = None
        self.bytes_done = 0

    def start(self) -> None:
        self._t0 = self._clock.monotonic()

    def advance(self, count: int) -> TransferProgress:
        self.bytes_done += count
        return self.snapshot()

    def snapshot(self) -> TransferProgress:
        elapsed = 0.0 if self._t0 is None else self._clock.monotonic() - self._t0
        total = len(self._payload.data)
        line = self._payload.line_for_offset(self.bytes_done) if self.bytes_done else 0
        if total and self.bytes_done >= total:
            line = self._payload.line_count
        return TransferProgress(
            bytes_done=self.bytes_done,
            bytes_total=total,
            current_line=line,
            total_lines=self._payload.line_count,
            elapsed_s=elapsed,
            started_at=self._started_at,
        )
