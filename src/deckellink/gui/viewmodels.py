"""GUI-independent presentation logic (unit-testable without widgets)."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

from deckellink.domain.models import Profile
from deckellink.domain.results import AnalysisResult, TransformResult


def format_bytes(count: int) -> str:
    if count < 1024:
        return f"{count} B"
    if count < 1024 * 1024:
        return f"{count / 1024:.1f} KB".replace(".", ",")
    return f"{count / (1024 * 1024):.2f} MB".replace(".", ",")


def format_duration(seconds: float) -> str:
    seconds = max(0, int(seconds))
    return f"{seconds // 3600:02d}:{(seconds % 3600) // 60:02d}:{seconds % 60:02d}"


def format_rate(bytes_per_second: float) -> str:
    return f"{bytes_per_second:.0f} B/s"


@dataclass
class LoadedProgram:
    """A loaded NC file: original bytes (never modified), analysis and transfer data."""

    path: Path
    original: bytes
    analysis: AnalysisResult
    payload: TransformResult | None = None
    transform_error: str | None = None

    @property
    def line_count(self) -> int:
        return self.analysis.line_count


@dataclass
class SendGate:
    """Decides whether "An Maschine senden" may be enabled, with a reason."""

    allowed: bool
    reasons: list[str] = field(default_factory=list)

    @property
    def reason_text(self) -> str:
        return "; ".join(self.reasons)


def evaluate_send_gate(
    program: LoadedProgram | None,
    *,
    simulation: bool,
    port: str,
    busy: bool,
    warnings_acknowledged: bool,
) -> SendGate:
    reasons: list[str] = []
    if program is None:
        reasons.append("Keine Datei geladen")
    else:
        if program.analysis.has_errors:
            reasons.append("Prüfung enthält Fehler")
        if program.payload is None:
            reasons.append("Transferdaten konnten nicht erzeugt werden")
        if program.analysis.has_warnings and not warnings_acknowledged:
            reasons.append("Warnungen noch nicht bestätigt")
    if not simulation and not port:
        reasons.append("Kein COM-Port gewählt und Simulator nicht aktiv")
    if busy:
        reasons.append("Ein anderer Vorgang läuft")
    return SendGate(not reasons, reasons)


def evaluate_receive_gate(*, simulation: bool, port: str, busy: bool) -> SendGate:
    reasons: list[str] = []
    if not simulation and not port:
        reasons.append("Kein COM-Port gewählt und Simulator nicht aktiv")
    if busy:
        reasons.append("Ein anderer Vorgang läuft")
    return SendGate(not reasons, reasons)


def preview_text(data: bytes, max_lines: int = 400) -> str:
    """Readable preview of transfer bytes with visible control characters."""
    text = data.decode("ascii", errors="replace")
    lines = text.splitlines()
    shown = lines[:max_lines]
    out = "\n".join(shown)
    if len(lines) > max_lines:
        out += f"\n... ({len(lines) - max_lines} weitere Zeilen)"
    return out


def profile_summary(profile: Profile) -> str:
    return profile.serial.summary
