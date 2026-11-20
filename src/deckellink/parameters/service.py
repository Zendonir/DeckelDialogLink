"""Machine parameter files: analysis, comparison and the write gate.

DeckelLink treats machine parameters as data files:

* reading = receiving the parameter output the operator starts at the CNC,
* writing = sending a parameter file into a parameter input that the authorised
  operator has enabled at the CNC (key switch / code number).

No remote parameter commands are sent. The parameter file structure is checked
against the format observed on a real FP2A (see :mod:`.fileformat`); it is not
an official specification. Parameter data is transferred with minimal changes
(only NUL removal and line-ending normalisation).
"""

from __future__ import annotations

import difflib
import re
from dataclasses import dataclass, field, replace
from datetime import datetime

from deckellink.domain.models import (
    CommentHandling,
    EndDetection,
    Profile,
    TabHandling,
    TransferSettings,
    WhitespaceHandling,
)
from deckellink.domain.results import AnalysisResult, Finding, Severity, TransformResult
from deckellink.nc.analyzer import analyze
from deckellink.nc.transformer import transform
from deckellink.parameters.fileformat import ParameterFileInfo, compatibility_problems, parse_parameter_file
from deckellink.storage.checksums import sha256_bytes

WRITE_CONFIRMATION_PHRASE = "PARAMETER SCHREIBEN"
MAX_DIFF_LINES = 400


def parameter_transfer_settings(profile: Profile) -> TransferSettings:
    """Transfer settings for parameter data: no content changes except NUL/line endings."""
    return replace(
        profile.transfer,
        program_start="",
        program_end="",
        append_program_end=False,
        remove_blank_lines=False,
        skip_semicolon_lines=False,
        normalize_line_endings=True,
        strip_null_bytes=True,
        tab_handling=TabHandling.KEEP,
        whitespace=WhitespaceHandling.NONE,
        comment_handling=CommentHandling.KEEP,
        ensure_final_newline=False,
        max_line_length=0,
        max_program_bytes=profile.parameters.max_bytes,
    )


def parameter_profile(profile: Profile) -> Profile:
    """Profile variant used for parameter transfers (inactivity end, no M30 logic)."""
    p = profile.parameters
    return replace(
        profile,
        transfer=parameter_transfer_settings(profile),
        receive=replace(
            profile.receive,
            end_detection=EndDetection.INACTIVITY,
            inactivity_timeout_ms=p.inactivity_timeout_ms,
            first_byte_timeout_ms=p.first_byte_timeout_ms,
            max_receive_bytes=p.max_bytes,
            program_name_pattern="",
        ),
    )


def analyze_parameters(data: bytes, profile: Profile) -> AnalysisResult:
    """Analysis of a parameter file (character checks plus optional identification pattern)."""
    result = analyze(data, parameter_transfer_settings(profile), profile.serial)
    # Tabs and blank lines are kept unchanged in parameter files - informational only.
    findings = [
        replace(f, severity=Severity.INFO) if f.code in ("TABS", "SEMICOLON_LINES") else f for f in result.findings
    ]
    pattern = profile.parameters.identification_pattern
    if data and pattern:
        text = data.decode("ascii", errors="replace")
        if not re.search(pattern, text):
            findings.append(
                Finding(
                    "NOT_A_PARAMETER_FILE",
                    Severity.ERROR,
                    f"Die Datei entspricht nicht dem im Profil hinterlegten Muster für Parameterdateien ({pattern!r}).",
                )
            )
    info = parse_parameter_file(data) if data else None
    if info is not None:
        findings.extend(info.findings)
    findings.sort(key=lambda f: -f.severity.rank)
    return replace(result, findings=tuple(findings))


def prepare_parameter_payload(data: bytes, profile: Profile) -> TransformResult:
    return transform(data, parameter_transfer_settings(profile))


@dataclass(frozen=True)
class ParameterDiff:
    changed_lines: int
    added_lines: int
    removed_lines: int
    diff_text: str
    truncated: bool

    @property
    def identical(self) -> bool:
        return self.changed_lines == self.added_lines == self.removed_lines == 0

    @property
    def summary(self) -> str:
        if self.identical:
            return "Keine Unterschiede"
        return (
            f"{self.changed_lines} geänderte, {self.added_lines} hinzugefügte, {self.removed_lines} entfernte Zeile(n)"
        )


def _lines(data: bytes) -> list[str]:
    return data.decode("ascii", errors="replace").replace("\r\n", "\n").replace("\r", "\n").splitlines()


def diff_parameters(old: bytes, new: bytes, old_name: str = "Sicherung", new_name: str = "Neu") -> ParameterDiff:
    """Line based comparison (line endings and NUL bytes are ignored)."""
    a = _lines(old.replace(b"\x00", b""))
    b = _lines(new.replace(b"\x00", b""))
    changed = added = removed = 0
    for tag, i1, i2, j1, j2 in difflib.SequenceMatcher(a=a, b=b, autojunk=False).get_opcodes():
        if tag == "replace":
            common = min(i2 - i1, j2 - j1)
            changed += common
            removed += (i2 - i1) - common
            added += (j2 - j1) - common
        elif tag == "delete":
            removed += i2 - i1
        elif tag == "insert":
            added += j2 - j1
    diff = list(difflib.unified_diff(a, b, old_name, new_name, lineterm="", n=1))
    truncated = len(diff) > MAX_DIFF_LINES
    text = "\n".join(diff[:MAX_DIFF_LINES])
    if truncated:
        text += f"\n... ({len(diff) - MAX_DIFF_LINES} weitere Diff-Zeilen)"
    return ParameterDiff(changed, added, removed, text, truncated)


@dataclass(frozen=True)
class ParameterBackupInfo:
    """A parameter set read from the machine in the current session."""

    path_name: str
    data: bytes
    read_at: datetime
    profile_id: str
    port: str
    complete: bool

    @property
    def sha256(self) -> str:
        return sha256_bytes(self.data)

    @property
    def structure(self) -> ParameterFileInfo | None:
        return parse_parameter_file(self.data)


@dataclass
class ParameterCandidate:
    """A parameter file selected for writing."""

    name: str
    data: bytes
    analysis: AnalysisResult
    payload: TransformResult | None
    transform_error: str | None = None

    @property
    def structure(self) -> ParameterFileInfo | None:
        return parse_parameter_file(self.data)


@dataclass
class ParameterWriteGate:
    allowed: bool
    reasons: list[str] = field(default_factory=list)
    diff: ParameterDiff | None = None

    @property
    def reason_text(self) -> str:
        return "; ".join(self.reasons)


def evaluate_parameter_write_gate(
    *,
    profile: Profile,
    session_unlocked: bool,
    candidate: ParameterCandidate | None,
    backup: ParameterBackupInfo | None,
    now: datetime,
    port_label: str,
    simulation: bool,
    port: str,
    busy: bool,
) -> ParameterWriteGate:
    """All conditions that must hold before a parameter write may even be offered."""
    reasons: list[str] = []
    diff: ParameterDiff | None = None
    if not profile.parameters.allow_write:
        reasons.append("Profil erlaubt kein Schreiben von Maschinenparametern")
    if not session_unlocked:
        reasons.append("Schreibfunktion für diese Sitzung nicht freigeschaltet")
    max_age = profile.parameters.max_backup_age_min
    if backup is None:
        reasons.append(f"Keine aktuelle Sicherung - Parameter zuerst auslesen (max. {max_age} min alt)")
    else:
        age_min = (now - backup.read_at).total_seconds() / 60.0
        if backup.profile_id != profile.id or backup.port != port_label:
            reasons.append("Sicherung stammt von anderem Profil oder Anschluss - Parameter erneut auslesen")
        elif age_min > max_age:
            reasons.append(f"Sicherung ist {age_min:.0f} min alt (max. {max_age} min) - erneut auslesen")
        elif not backup.complete:
            reasons.append("Letzte Sicherung ist unvollständig - erneut auslesen")
    if candidate is None:
        reasons.append("Keine Parameterdatei zum Schreiben gewählt")
    else:
        if candidate.analysis.has_errors:
            reasons.append("Parameterdatei enthält Prüffehler")
        if candidate.payload is None:
            reasons.append("Transferdaten konnten nicht erzeugt werden")
        if backup is not None:
            diff = diff_parameters(backup.data, candidate.data, backup.path_name, candidate.name)
            if diff.identical:
                reasons.append("Datei ist identisch mit der aktuellen Sicherung - nichts zu schreiben")
            else:
                reasons.extend(_structure_reasons(backup, candidate))
    if not simulation and not port:
        reasons.append("Kein COM-Port gewählt und Simulator nicht aktiv")
    if busy:
        reasons.append("Ein anderer Vorgang läuft")
    return ParameterWriteGate(not reasons, reasons, diff)


def _structure_reasons(backup: ParameterBackupInfo, candidate: ParameterCandidate) -> list[str]:
    """Format, software version, machine number and checksum consistency against the backup."""
    backup_info = backup.structure
    candidate_info = candidate.structure
    if candidate_info is None:
        return ["Parameterdatei hat keine erkennbare Gruppenstruktur (%MPGnn/%MDGnn)"]
    if backup_info is None:
        return ["Sicherung hat keine erkennbare Gruppenstruktur - Softwarestand nicht prüfbar"]
    return compatibility_problems(backup_info, candidate_info)
