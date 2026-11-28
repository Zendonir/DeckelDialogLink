"""Structure of DIALOG 11 machine parameter files (as output by the CNC).

Observed format (output of a real FP2A, not an official specification):

* group header ``%MPGnn (NAME)`` (machine parameters) or ``%MDGnn (NAME)``,
* parameter lines ``Mnnnnn value`` (value may be empty),
* group end ``?`` followed by a line with a 4-digit hexadecimal checksum,
* group 00 ``(KOPF)`` holds software versions, machine type, number and date,
* the file ends with EOT (0x04).

The checksum algorithm is unknown. DeckelLink therefore never computes
checksums; it only checks that each group has one and detects groups whose
content changed while the checksum stayed the same.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from deckellink.domain.results import Finding, Severity
from deckellink.nc.checks import split_eot_trailer, split_lines

GROUP_RE = re.compile(r"^%M([DP])G(\d{2}) \((.*)\)\s*$")
PARAM_RE = re.compile(r"^M(\d{5})(?: (.*))?$")
CHECKSUM_RE = re.compile(r"^[0-9A-Fa-f]{4}$")
GROUP_END = "?"
MAX_REPORTED = 200

# Header parameters by kind: (software versions range, machine type, machine number, date)
_HEADER_KEYS = {
    "P": (range(65001, 65031), "65033", "65034", "65035"),
    "D": (range(1, 31), "00033", "00034", "00035"),
}


@dataclass(frozen=True)
class ParameterGroup:
    kind: str  # "P" (MPG) or "D" (MDG)
    number: int
    name: str
    line: int
    lines: tuple[str, ...]
    checksum: str | None

    @property
    def key(self) -> str:
        return f"%M{self.kind}G{self.number:02d}"

    @property
    def label(self) -> str:
        return f"{self.key} ({self.name})"


@dataclass(frozen=True)
class ParameterFileInfo:
    kind: str | None
    groups: tuple[ParameterGroup, ...]
    header: dict[str, str]
    eot: bool
    findings: tuple[Finding, ...] = field(default_factory=tuple)

    @property
    def kind_label(self) -> str:
        return {"P": "MPG", "D": "MDG"}.get(self.kind or "", "unbekannt")

    def _header_value(self, index: int) -> str:
        if self.kind is None:
            return ""
        return self.header.get(str(_HEADER_KEYS[self.kind][index]), "").strip()

    @property
    def software_versions(self) -> tuple[str, ...]:
        """All non-empty software version entries of the header group, in order."""
        if self.kind is None:
            return ()
        values = (self.header.get(f"{n:05d}", "").strip() for n in _HEADER_KEYS[self.kind][0])
        return tuple(v for v in values if v)

    @property
    def software_version(self) -> str:
        versions = self.software_versions
        return versions[0] if versions else ""

    @property
    def machine_type(self) -> str:
        return self._header_value(1)

    @property
    def machine_number(self) -> str:
        return self._header_value(2)

    @property
    def date(self) -> str:
        return self._header_value(3)

    @property
    def has_header(self) -> bool:
        return any(g.number == 0 for g in self.groups)

    def group(self, key: str) -> ParameterGroup | None:
        return next((g for g in self.groups if g.key == key), None)

    @property
    def summary(self) -> str:
        if not self.groups:
            return "Keine Parametergruppen erkannt"
        parts = [f"{self.kind_label}-Format, {len(self.groups)} Gruppe(n)"]
        if self.software_version:
            parts.append(f"Software {self.software_version}")
        if self.machine_type:
            parts.append(f"Maschine {self.machine_type}")
        if self.machine_number:
            parts.append(f"Nr. {self.machine_number}")
        return ", ".join(parts)


def _finding(code: str, severity: Severity, message: str, lines: list[int] | None = None) -> Finding:
    lines = lines or []
    return Finding(code, severity, message, tuple(lines[:MAX_REPORTED]), len(lines))


def parse_parameter_file(data: bytes) -> ParameterFileInfo | None:
    """Parse ``data``; ``None`` if it contains no parameter group header at all."""
    body, trailer = split_eot_trailer(data.replace(b"\x00", b""))
    raw_lines = split_lines(body)
    texts = [(line.number, line.content.decode("ascii", errors="replace").rstrip(" \t")) for line in raw_lines]
    if not any(GROUP_RE.match(text) for _, text in texts):
        return None

    groups: list[ParameterGroup] = []
    findings: list[Finding] = []
    outside: list[int] = []
    bad_lines: list[int] = []
    unterminated: list[int] = []
    missing_checksum: list[int] = []
    kinds: set[str] = set()
    header: dict[str, str] = {}

    current: tuple[str, int, str, int] | None = None
    content: list[str] = []
    awaiting_checksum: tuple[str, int, str, int, list[str]] | None = None

    def close(checksum: str | None) -> None:
        nonlocal awaiting_checksum
        assert awaiting_checksum is not None
        kind, number, name, line, lines = awaiting_checksum
        groups.append(ParameterGroup(kind, number, name, line, tuple(lines), checksum))
        awaiting_checksum = None

    for number, text in texts:
        if awaiting_checksum is not None:
            if CHECKSUM_RE.match(text):
                close(text.upper())
                continue
            missing_checksum.append(awaiting_checksum[3])
            close(None)
        if not text:
            continue
        match = GROUP_RE.match(text)
        if match:
            if current is not None:
                unterminated.append(current[3])
                groups.append(ParameterGroup(current[0], current[1], current[2], current[3], tuple(content), None))
            kind, group_no, name = match.group(1), int(match.group(2)), match.group(3)
            kinds.add(kind)
            current = (kind, group_no, name, number)
            content = []
            continue
        if current is None:
            outside.append(number)
            continue
        if text == GROUP_END:
            awaiting_checksum = (*current, content)
            current = None
            continue
        param = PARAM_RE.match(text)
        if param is None:
            bad_lines.append(number)
            continue
        content.append(text)
        # Parameter numbers may repeat within a group (e.g. compensation tables).
        if current[1] == 0:
            header.setdefault(param.group(1), param.group(2) or "")
    if awaiting_checksum is not None:
        missing_checksum.append(awaiting_checksum[3])
        close(None)
    if current is not None:
        unterminated.append(current[3])
        groups.append(ParameterGroup(current[0], current[1], current[2], current[3], tuple(content), None))

    if outside:
        findings.append(
            _finding(
                "PARAM_DATA_OUTSIDE_GROUP",
                Severity.ERROR,
                f"{len(outside)} Zeile(n) stehen außerhalb einer Parametergruppe (%MPGnn/%MDGnn).",
                outside,
            )
        )
    if bad_lines:
        findings.append(
            _finding(
                "PARAM_LINE_FORMAT",
                Severity.ERROR,
                f"{len(bad_lines)} Zeile(n) entsprechen nicht dem Format 'Mnnnnn Wert'.",
                bad_lines,
            )
        )
    if unterminated:
        findings.append(
            _finding(
                "PARAM_GROUP_UNTERMINATED",
                Severity.ERROR,
                f"{len(unterminated)} Gruppe(n) ohne Gruppenende '?'.",
                unterminated,
            )
        )
    if missing_checksum:
        findings.append(
            _finding(
                "PARAM_CHECKSUM_MISSING",
                Severity.ERROR,
                f"{len(missing_checksum)} Gruppe(n) ohne Prüfsummenzeile (4 Hex-Ziffern nach '?').",
                missing_checksum,
            )
        )
    keys = [g.key for g in groups]
    duplicates = sorted({k for k in keys if keys.count(k) > 1})
    if duplicates:
        findings.append(
            _finding(
                "PARAM_DUPLICATE_GROUP",
                Severity.WARNING,
                f"Gruppen mehrfach vorhanden: {', '.join(duplicates)}.",
            )
        )
    if len(kinds) > 1:
        findings.append(
            _finding(
                "PARAM_MIXED_KINDS",
                Severity.ERROR,
                "Die Datei mischt %MPG- und %MDG-Gruppen.",
            )
        )
    if not any(g.number == 0 for g in groups):
        findings.append(
            _finding(
                "PARAM_NO_HEADER",
                Severity.WARNING,
                "Kopfgruppe 00 (KOPF) mit Softwarestand fehlt - der Softwarestand kann nicht geprüft werden.",
            )
        )
    if not trailer:
        findings.append(
            _finding(
                "PARAM_NO_EOT",
                Severity.INFO,
                "Die Datei endet nicht mit EOT (0x04), wie es bei Ausgaben der Steuerung beobachtet wurde.",
            )
        )
    kind = next(iter(kinds)) if len(kinds) == 1 else None
    return ParameterFileInfo(kind, tuple(groups), header, bool(trailer), tuple(findings))


@dataclass(frozen=True)
class GroupChange:
    key: str
    label: str
    checksum_changed: bool


def changed_groups(old: ParameterFileInfo, new: ParameterFileInfo) -> list[GroupChange]:
    """Groups of ``new`` whose parameter lines differ from the same group in ``old``."""
    result: list[GroupChange] = []
    for group in new.groups:
        before = old.group(group.key)
        if before is None or before.lines == group.lines:
            continue
        result.append(GroupChange(group.key, group.label, before.checksum != group.checksum))
    return result


def compatibility_problems(backup: ParameterFileInfo, candidate: ParameterFileInfo) -> list[str]:
    """Reasons why ``candidate`` must not be written to the machine ``backup`` was read from."""
    reasons: list[str] = []
    if backup.kind != candidate.kind:
        reasons.append(f"Dateiformat {candidate.kind_label} passt nicht zur Sicherung ({backup.kind_label})")
    if not candidate.has_header:
        reasons.append("Parameterdatei hat keine Kopfgruppe - Softwarestand nicht prüfbar")
    elif not backup.has_header:
        reasons.append("Sicherung hat keine Kopfgruppe - Softwarestand nicht prüfbar")
    else:
        if candidate.software_versions != backup.software_versions:
            reasons.append(
                f"Softwarestand der Datei ({candidate.software_version or '-'}) passt nicht zur "
                f"Maschine laut Sicherung ({backup.software_version or '-'})"
            )
        if candidate.machine_number != backup.machine_number:
            reasons.append(
                f"Maschinennummer der Datei ({candidate.machine_number or '-'}) passt nicht zur "
                f"Sicherung ({backup.machine_number or '-'})"
            )
    for change in changed_groups(backup, candidate):
        if not change.checksum_changed:
            reasons.append(
                f"Gruppe {change.label} wurde geändert, die Prüfsumme aber nicht (Prüfsummenverfahren "
                "unbekannt - die Steuerung würde die Gruppe verwerfen)"
            )
    return reasons
