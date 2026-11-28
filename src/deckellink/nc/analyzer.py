"""Read-only analysis of NC data. Never modifies its input."""

from __future__ import annotations

import re

from deckellink.domain.models import SerialSettings, TabHandling, TransferSettings
from deckellink.domain.results import AnalysisResult, Finding, Severity
from deckellink.nc.checks import (
    NUL,
    TAB,
    RawLine,
    count_line_endings,
    is_blank,
    is_disallowed_control,
    is_semicolon_line,
    max_value_for_data_bits,
    program_end_pattern,
    remove_comments,
    split_eot_trailer,
    split_lines,
)
from deckellink.storage.checksums import sha256_bytes

MAX_REPORTED_LINES = 200


def _thousands(value: int) -> str:
    """German thousands separator: 1234567 -> 1.234.567."""
    return f"{value:,}".replace(",", ".")


class _Collector:
    def __init__(self) -> None:
        self.findings: list[Finding] = []

    def add(
        self,
        code: str,
        severity: Severity,
        message: str,
        lines: list[int] | None = None,
        count: int = 0,
    ) -> None:
        line_tuple = tuple((lines or [])[:MAX_REPORTED_LINES])
        total = count or len(lines or [])
        self.findings.append(Finding(code, severity, message, line_tuple, total))


def analyze(
    data: bytes,
    settings: TransferSettings,
    serial: SerialSettings | None = None,
) -> AnalysisResult:
    """Analyse ``data`` according to ``settings`` and return all findings.

    ``serial`` is optional; when given, the character range is checked against
    its number of data bits.
    """
    col = _Collector()
    full = data
    data, trailer = split_eot_trailer(full)
    lines = split_lines(data)
    endings = count_line_endings(lines)
    longest = max((len(line.content) for line in lines), default=0)
    end_re = program_end_pattern(settings.program_end)

    if not data:
        col.add("EMPTY_FILE", Severity.ERROR, "Die Datei ist leer.")
        return AnalysisResult(tuple(col.findings), len(full), 0, endings, 0, False, sha256_bytes(full))
    if trailer:
        col.add(
            "TRANSMISSION_END",
            Severity.INFO,
            "Die Datei endet mit dem Übertragungsende-Zeichen EOT (0x04), wie es die DIALOG 11 ausgibt."
            + (" Es wird mitgesendet." if settings.send_eot_trailer else " Es wird laut Profil nicht gesendet."),
            [len(lines) + 1],
        )

    if len(data) > settings.max_program_bytes > 0:
        col.add(
            "FILE_TOO_LARGE",
            Severity.WARNING,
            f"Sehr große Datei ({_thousands(len(data))} Bytes, Grenze laut Profil "
            f"{_thousands(settings.max_program_bytes)} Bytes). Die Speicherkapazität der "
            "Steuerung ist nicht verifiziert.",
        )

    _check_bytes(data, lines, settings, serial, col)
    _check_line_endings(lines, endings, settings, col)
    _check_lines(lines, settings, col)
    has_end = _check_program_end(lines, settings, end_re, col)

    if settings.program_start.strip():
        col.add(
            "UNVERIFIED_START_SEQUENCE",
            Severity.WARNING,
            f"Profil fügt die Startsequenz '{settings.program_start.strip()}' ein. "
            "Diese Sequenz ist für die DIALOG 11 nicht verifiziert.",
        )

    findings = sorted(col.findings, key=lambda f: -f.severity.rank)
    return AnalysisResult(
        tuple(findings),
        len(full),
        len(lines),
        endings,
        longest,
        has_end,
        sha256_bytes(full),
    )


def _check_bytes(
    data: bytes,
    lines: list[RawLine],
    settings: TransferSettings,
    serial: SerialSettings | None,
    col: _Collector,
) -> None:
    max_value = max_value_for_data_bits(serial.data_bits if serial else 7)
    null_lines: list[int] = []
    high_lines: list[int] = []
    ctrl_lines: list[int] = []
    tab_lines: list[int] = []
    null_count = high_count = ctrl_count = 0
    for line in lines:
        content = line.content
        has_null = has_high = has_ctrl = False
        for b in content:
            if b == NUL:
                null_count += 1
                has_null = True
            elif b > max_value:
                high_count += 1
                has_high = True
            elif is_disallowed_control(b):
                ctrl_count += 1
                has_ctrl = True
        if has_null:
            null_lines.append(line.number)
        if has_high:
            high_lines.append(line.number)
        if has_ctrl:
            ctrl_lines.append(line.number)
        if TAB in content:
            tab_lines.append(line.number)

    if null_count:
        if settings.strip_null_bytes:
            col.add(
                "NULL_BYTES",
                Severity.WARNING,
                f"{null_count} Nullbyte(s) gefunden. Sie werden laut Profil vor dem Senden entfernt.",
                null_lines,
                null_count,
            )
        else:
            col.add(
                "NULL_BYTES",
                Severity.ERROR,
                f"{null_count} Nullbyte(s) gefunden und Entfernen ist im Profil deaktiviert.",
                null_lines,
                null_count,
            )
    if high_count:
        bits = serial.data_bits if serial else 7
        col.add(
            "NON_ASCII",
            Severity.ERROR,
            f"{high_count} Zeichen sind mit {bits} Datenbits bzw. 7-Bit-ASCII nicht "
            f"darstellbar (Bytewert über 0x{max_value:02X}), z. B. Umlaute.",
            high_lines,
            high_count,
        )
    if ctrl_count:
        col.add(
            "CONTROL_CHARS",
            Severity.ERROR,
            f"{ctrl_count} unzulässige Steuerzeichen gefunden (z. B. ESC, DEL, XON/XOFF).",
            ctrl_lines,
            ctrl_count,
        )
    if tab_lines:
        if settings.tab_handling is TabHandling.REJECT:
            col.add(
                "TABS",
                Severity.ERROR,
                "Tabulatoren gefunden. Laut Profil werden Tabs abgelehnt.",
                tab_lines,
            )
        elif settings.tab_handling is TabHandling.REPLACE:
            col.add(
                "TABS",
                Severity.INFO,
                "Tabulatoren gefunden. Sie werden durch Leerzeichen ersetzt.",
                tab_lines,
            )
        else:
            col.add(
                "TABS",
                Severity.WARNING,
                "Tabulatoren werden unverändert gesendet. Ob die Steuerung Tabs akzeptiert, ist nicht verifiziert.",
                tab_lines,
            )


def _check_line_endings(
    lines: list[RawLine],
    endings: dict[str, int],
    settings: TransferSettings,
    col: _Collector,
) -> None:
    used = [name for name, count in endings.items() if count]
    if len(used) > 1:
        detail = ", ".join(f"{name.upper()}: {endings[name]}" for name in used)
        if settings.normalize_line_endings:
            col.add(
                "MIXED_LINE_ENDINGS",
                Severity.INFO,
                f"Gemischte Zeilenenden ({detail}). Sie werden auf "
                f"{settings.line_ending.value.upper()} vereinheitlicht.",
            )
        else:
            col.add(
                "MIXED_LINE_ENDINGS",
                Severity.WARNING,
                f"Gemischte Zeilenenden ({detail}) und Normalisierung ist deaktiviert.",
            )
    elif used and used[0] != settings.line_ending.value:
        col.add(
            "FOREIGN_LINE_ENDINGS",
            Severity.INFO if settings.normalize_line_endings else Severity.WARNING,
            f"Datei verwendet {used[0].upper()}, Profil erwartet "
            f"{settings.line_ending.value.upper()}."
            + (" Wird umgewandelt." if settings.normalize_line_endings else ""),
        )
    if lines and lines[-1].ending == b"":
        col.add(
            "MISSING_FINAL_NEWLINE",
            Severity.INFO if settings.ensure_final_newline else Severity.WARNING,
            "Die letzte Zeile hat keinen abschließenden Zeilenumbruch."
            + (" Er wird ergänzt." if settings.ensure_final_newline else ""),
            [lines[-1].number],
        )


def _check_lines(lines: list[RawLine], settings: TransferSettings, col: _Collector) -> None:
    blank: list[int] = []
    semicolon: list[int] = []
    long_lines: list[int] = []
    whitespace: list[int] = []
    for line in lines:
        if is_blank(line.content):
            # A final empty fragment without content is not a line in the editor sense.
            if not (line.ending == b"" and line.content == b""):
                blank.append(line.number)
            continue
        if is_semicolon_line(line.content):
            semicolon.append(line.number)
        if settings.max_line_length > 0 and len(line.content) > settings.max_line_length:
            long_lines.append(line.number)
        if line.content != line.content.strip(b" \t"):
            whitespace.append(line.number)
    if blank:
        col.add(
            "BLANK_LINES",
            Severity.INFO,
            f"{len(blank)} Leerzeile(n)." + (" Sie werden entfernt." if settings.remove_blank_lines else ""),
            blank,
        )
    if semicolon:
        col.add(
            "SEMICOLON_LINES",
            Severity.INFO if settings.skip_semicolon_lines else Severity.WARNING,
            f"{len(semicolon)} Zeile(n) beginnen mit Semikolon."
            + (
                " Sie werden laut Profil entfernt."
                if settings.skip_semicolon_lines
                else " Ob die Steuerung diese Zeilen akzeptiert, ist nicht verifiziert."
            ),
            semicolon,
        )
    if long_lines:
        col.add(
            "LONG_LINES",
            Severity.WARNING,
            f"{len(long_lines)} Zeile(n) länger als {settings.max_line_length} Zeichen "
            "(maximale Zeilenlänge der Steuerung nicht verifiziert).",
            long_lines,
        )
    if whitespace:
        col.add(
            "SURROUNDING_WHITESPACE",
            Severity.INFO,
            f"{len(whitespace)} Zeile(n) mit führenden oder nachgestellten Leerzeichen "
            f"(Behandlung laut Profil: {settings.whitespace.value}).",
            whitespace,
        )


def _check_program_end(
    lines: list[RawLine],
    settings: TransferSettings,
    end_re: re.Pattern[bytes] | None,
    col: _Collector,
) -> bool:
    if end_re is None:
        return False
    end_line: int | None = None
    after: list[int] = []
    for line in lines:
        content = remove_comments(line.content, settings.comment_open, settings.comment_close)
        if end_line is None:
            if end_re.search(content):
                end_line = line.number
        elif not is_blank(content) and content.strip(b" \t%") != b"":
            after.append(line.number)
    if end_line is None:
        if settings.append_program_end:
            col.add(
                "MISSING_PROGRAM_END",
                Severity.WARNING,
                f"Programmende '{settings.program_end}' fehlt. Das Profil erlaubt das "
                "Ergänzen - dies erfolgt nur nach ausdrücklicher Bestätigung.",
            )
        else:
            col.add(
                "MISSING_PROGRAM_END",
                Severity.WARNING,
                f"Programmende '{settings.program_end}' wurde nicht gefunden.",
            )
        return False
    if after:
        col.add(
            "DATA_AFTER_PROGRAM_END",
            Severity.WARNING,
            f"Nach '{settings.program_end}' (Zeile {end_line}) folgen weitere Daten.",
            after,
        )
    return True
