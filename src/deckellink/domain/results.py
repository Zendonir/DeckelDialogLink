"""Result value objects shared between layers."""

from __future__ import annotations

import bisect
from dataclasses import dataclass, field
from datetime import datetime
from enum import StrEnum
from typing import Any

from deckellink.domain.errors import UserFacingError
from deckellink.domain.states import EndReason, ReceiveState, SendState


class Severity(StrEnum):
    INFO = "info"
    WARNING = "warning"
    ERROR = "error"

    @property
    def label(self) -> str:
        return {"info": "Info", "warning": "Warnung", "error": "Fehler"}[self.value]

    @property
    def rank(self) -> int:
        return {"info": 0, "warning": 1, "error": 2}[self.value]


@dataclass(frozen=True)
class Finding:
    code: str
    severity: Severity
    message: str
    lines: tuple[int, ...] = ()
    count: int = 0

    @property
    def lines_text(self) -> str:
        if not self.lines:
            return ""
        shown = ", ".join(str(n) for n in self.lines[:10])
        more = len(self.lines) - 10 if self.count <= 0 else self.count - min(len(self.lines), 10)
        return shown + (f" (+{more} weitere)" if more > 0 else "")

    def to_dict(self) -> dict[str, Any]:
        return {
            "code": self.code,
            "severity": self.severity.value,
            "message": self.message,
            "lines": list(self.lines[:50]),
            "count": self.count,
        }


@dataclass(frozen=True)
class AnalysisResult:
    findings: tuple[Finding, ...]
    byte_count: int
    line_count: int
    line_ending_counts: dict[str, int]
    longest_line: int
    has_program_end: bool
    sha256: str

    @property
    def errors(self) -> tuple[Finding, ...]:
        return tuple(f for f in self.findings if f.severity is Severity.ERROR)

    @property
    def warnings(self) -> tuple[Finding, ...]:
        return tuple(f for f in self.findings if f.severity is Severity.WARNING)

    @property
    def infos(self) -> tuple[Finding, ...]:
        return tuple(f for f in self.findings if f.severity is Severity.INFO)

    @property
    def has_errors(self) -> bool:
        return bool(self.errors)

    @property
    def has_warnings(self) -> bool:
        return bool(self.warnings)

    def has(self, code: str) -> bool:
        return any(f.code == code for f in self.findings)

    def get(self, code: str) -> Finding | None:
        return next((f for f in self.findings if f.code == code), None)


@dataclass(frozen=True)
class TransformResult:
    """Prepared transfer bytes. Never written over the original file."""

    data: bytes
    line_count: int
    line_end_offsets: tuple[int, ...]
    applied: tuple[str, ...]
    appended_program_end: bool = False

    def line_for_offset(self, offset: int) -> int:
        """1-based line number containing byte ``offset`` (bytes already sent)."""
        if not self.line_end_offsets:
            return 0
        index = bisect.bisect_left(self.line_end_offsets, offset)
        return min(index + 1, len(self.line_end_offsets))


class Direction(StrEnum):
    TX = "TX"
    RX = "RX"


@dataclass(frozen=True)
class TrafficEvent:
    direction: Direction
    timestamp: datetime
    data: bytes


@dataclass(frozen=True)
class TransferProgress:
    bytes_done: int
    bytes_total: int
    current_line: int
    total_lines: int
    elapsed_s: float
    started_at: datetime | None = None

    @property
    def percent(self) -> float:
        if self.bytes_total <= 0:
            return 0.0
        return min(100.0, 100.0 * self.bytes_done / self.bytes_total)

    @property
    def rate_bps(self) -> float:
        if self.elapsed_s <= 0:
            return 0.0
        return self.bytes_done / self.elapsed_s


@dataclass(frozen=True)
class SendResult:
    state: SendState
    bytes_total: int
    bytes_sent: int
    started_at: datetime
    duration_s: float
    sha256: str
    profile_id: str
    port: str
    simulated: bool
    error: UserFacingError | None = None
    port_left_open: bool = False

    @property
    def rate_bps(self) -> float:
        return self.bytes_sent / self.duration_s if self.duration_s > 0 else 0.0

    def to_dict(self) -> dict[str, Any]:
        return {
            "type": "send",
            "state": self.state.value,
            "bytes_total": self.bytes_total,
            "bytes_sent": self.bytes_sent,
            "started_at": self.started_at.isoformat(),
            "duration_s": round(self.duration_s, 3),
            "sha256": self.sha256,
            "profile_id": self.profile_id,
            "port": self.port,
            "simulated": self.simulated,
            "error": self.error.title if self.error else None,
        }


@dataclass(frozen=True)
class ReceiveProgress:
    state: ReceiveState
    bytes_received: int
    elapsed_s: float
    idle_s: float


@dataclass(frozen=True)
class ReceiveResult:
    state: ReceiveState
    raw: bytes
    end_reason: EndReason
    started_at: datetime
    duration_s: float
    profile_id: str
    port: str
    simulated: bool
    first_byte_at: datetime | None = None
    last_byte_at: datetime | None = None
    error: UserFacingError | None = None

    def to_dict(self) -> dict[str, Any]:
        return {
            "type": "receive",
            "state": self.state.value,
            "bytes_received": len(self.raw),
            "end_reason": self.end_reason.value,
            "started_at": self.started_at.isoformat(),
            "duration_s": round(self.duration_s, 3),
            "profile_id": self.profile_id,
            "port": self.port,
            "simulated": self.simulated,
            "error": self.error.title if self.error else None,
        }


@dataclass(frozen=True)
class ReceivedProgram:
    """Post-processed receive data."""

    raw: bytes
    normalized: bytes
    analysis: AnalysisResult
    complete: bool
    decode_errors: int
    notes: tuple[str, ...] = field(default=())
