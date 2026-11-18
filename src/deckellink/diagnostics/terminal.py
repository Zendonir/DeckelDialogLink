"""Terminal model: bounded TX/RX history with ASCII/HEX formatting."""

from __future__ import annotations

import logging
from collections import deque
from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from pathlib import Path
from typing import TextIO

from deckellink.domain.results import Direction, TrafficEvent
from deckellink.nc.checks import format_ascii, format_hex

log = logging.getLogger(__name__)

MAX_ENTRY_BYTES = 256


class TerminalMode(StrEnum):
    ASCII = "ascii"
    HEX = "hex"
    COMBINED = "combined"

    @property
    def label(self) -> str:
        return {"ascii": "ASCII", "hex": "HEX", "combined": "Kombiniert"}[self.value]


@dataclass(frozen=True)
class TerminalEntry:
    direction: Direction
    timestamp: datetime
    data: bytes

    def format(self, mode: TerminalMode) -> str:
        stamp = self.timestamp.strftime("%H:%M:%S.") + f"{self.timestamp.microsecond // 1000:03d}"
        header = f"{self.direction.value} {stamp}"
        if mode is TerminalMode.ASCII:
            return f"{header}  {format_ascii(self.data)}"
        if mode is TerminalMode.HEX:
            return f"{header}  {format_hex(self.data)}"
        return f"{header}\nASCII: {format_ascii(self.data)}\nHEX:   {format_hex(self.data)}\n"


class TerminalBuffer:
    """Collects traffic into line-oriented entries with a bounded history.

    Consecutive bytes of the same direction are merged until LF, until
    ``MAX_ENTRY_BYTES`` is reached or the direction changes.
    """

    def __init__(self, max_entries: int = 2000) -> None:
        self._entries: deque[TerminalEntry] = deque(maxlen=max(10, max_entries))
        self._pending: dict[Direction, tuple[datetime, bytearray]] = {}
        self.tx_bytes = 0
        self.rx_bytes = 0

    @property
    def max_entries(self) -> int:
        return self._entries.maxlen or 0

    def resize(self, max_entries: int) -> None:
        self._entries = deque(self._entries, maxlen=max(10, max_entries))

    @property
    def entries(self) -> list[TerminalEntry]:
        return list(self._entries)

    def clear(self) -> None:
        self._entries.clear()
        self._pending.clear()

    def reset_counters(self) -> None:
        self.tx_bytes = 0
        self.rx_bytes = 0

    def add(self, event: TrafficEvent) -> list[TerminalEntry]:
        """Add traffic; returns entries that were completed by this call."""
        if event.direction is Direction.TX:
            self.tx_bytes += len(event.data)
        else:
            self.rx_bytes += len(event.data)
        completed: list[TerminalEntry] = []
        other = Direction.RX if event.direction is Direction.TX else Direction.TX
        if other in self._pending:
            completed.extend(self._flush_direction(other))
        start, buf = self._pending.get(event.direction, (event.timestamp, bytearray()))
        for byte in event.data:
            buf.append(byte)
            if byte == 0x0A or len(buf) >= MAX_ENTRY_BYTES:
                completed.append(self._commit(event.direction, start, bytes(buf)))
                buf = bytearray()
                start = event.timestamp
        if buf:
            self._pending[event.direction] = (start, buf)
        else:
            self._pending.pop(event.direction, None)
        return completed

    def flush(self) -> list[TerminalEntry]:
        completed: list[TerminalEntry] = []
        for direction in list(self._pending):
            completed.extend(self._flush_direction(direction))
        return completed

    def _flush_direction(self, direction: Direction) -> list[TerminalEntry]:
        pending = self._pending.pop(direction, None)
        if not pending or not pending[1]:
            return []
        return [self._commit(direction, pending[0], bytes(pending[1]))]

    def _commit(self, direction: Direction, when: datetime, data: bytes) -> TerminalEntry:
        entry = TerminalEntry(direction, when, data)
        self._entries.append(entry)
        return entry

    def render(self, mode: TerminalMode) -> str:
        return "\n".join(entry.format(mode) for entry in self._entries)


class TrafficFileLogger:
    """Optional full raw-traffic log. Must be enabled explicitly by the user."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self._handle: TextIO | None = None

    def open(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._handle = self.path.open("a", encoding="utf-8")
        log.info("Raw traffic logging enabled: %s", self.path.name)

    @property
    def is_open(self) -> bool:
        return self._handle is not None

    def write(self, entry: TerminalEntry) -> None:
        if self._handle is None:
            return
        self._handle.write(entry.format(TerminalMode.COMBINED) + "\n")
        self._handle.flush()

    def close(self) -> None:
        if self._handle is not None:
            self._handle.close()
            self._handle = None
            log.info("Raw traffic logging disabled")
