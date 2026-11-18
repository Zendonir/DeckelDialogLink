"""Profile schema validation (no external dependency).

``parse_profile`` validates raw JSON data and returns a :class:`Profile`
together with non-fatal warnings. Fatal problems raise
:class:`ProfileValidationError` listing every offending field.
"""

from __future__ import annotations

import re
from dataclasses import MISSING, dataclass, fields
from enum import StrEnum
from typing import Any

from deckellink.domain.errors import FieldError, ProfileValidationError
from deckellink.domain.models import (
    DATA_BITS,
    STANDARD_BAUD_RATES,
    STOP_BITS,
    BccMode,
    CommentHandling,
    EndDetection,
    LineEnding,
    MachineInfo,
    Parity,
    Profile,
    ProtocolMode,
    ProtocolSettings,
    ReceiveSettings,
    SerialSettings,
    TabHandling,
    TransferSettings,
    VerificationStatus,
    WhitespaceHandling,
)

SCHEMA_VERSION = 1
PROFILE_ID_RE = re.compile(r"^[a-z0-9][a-z0-9_\-]{0,63}$")
_PRINTABLE_RE = re.compile(r"^[\x20-\x7e]*$")
MAX_BYTES_LIMIT = 100 * 1024 * 1024


@dataclass(frozen=True)
class Rule:
    """Constraint for one field."""

    kind: type
    minimum: float | None = None
    maximum: float | None = None
    choices: tuple[Any, ...] | None = None
    max_length: int | None = None
    printable: bool = False


SERIAL_RULES: dict[str, Rule] = {
    "baud_rate": Rule(int, choices=STANDARD_BAUD_RATES),
    "data_bits": Rule(int, choices=DATA_BITS),
    "parity": Rule(Parity),
    "stop_bits": Rule(float, choices=STOP_BITS),
    "rts_cts": Rule(bool),
    "xon_xoff": Rule(bool),
    "dsr_dtr": Rule(bool),
    "initial_rts": Rule(bool),
    "initial_dtr": Rule(bool),
    "require_cts_before_send": Rule(bool),
    "require_dsr_before_send": Rule(bool),
    "read_timeout_ms": Rule(int, 1, 600_000),
    "write_timeout_ms": Rule(int, 1, 600_000),
    "inter_byte_timeout_ms": Rule(int, 0, 60_000),
    "handshake_timeout_ms": Rule(int, 0, 3_600_000),
}

TRANSFER_RULES: dict[str, Rule] = {
    "encoding": Rule(str, choices=("ascii",)),
    "line_ending": Rule(LineEnding),
    "program_start": Rule(str, max_length=64, printable=True),
    "program_end": Rule(str, max_length=32, printable=True),
    "append_program_end": Rule(bool),
    "start_delay_ms": Rule(int, 0, 600_000),
    "end_delay_ms": Rule(int, 0, 600_000),
    "character_delay_ms": Rule(int, 0, 10_000),
    "packet_size": Rule(int, 0, 65_536),
    "packet_delay_ms": Rule(int, 0, 600_000),
    "remove_blank_lines": Rule(bool),
    "strip_null_bytes": Rule(bool),
    "skip_semicolon_lines": Rule(bool),
    "normalize_line_endings": Rule(bool),
    "tab_handling": Rule(TabHandling),
    "whitespace": Rule(WhitespaceHandling),
    "comment_handling": Rule(CommentHandling),
    "comment_open": Rule(str, max_length=2, printable=True),
    "comment_close": Rule(str, max_length=2, printable=True),
    "ensure_final_newline": Rule(bool),
    "max_line_length": Rule(int, 0, 100_000),
    "max_program_bytes": Rule(int, 0, MAX_BYTES_LIMIT),
    "close_port_after_send": Rule(bool),
}

RECEIVE_RULES: dict[str, Rule] = {
    "end_detection": Rule(EndDetection),
    "end_sequence": Rule(str, max_length=32, printable=True),
    "inactivity_timeout_ms": Rule(int, 50, 3_600_000),
    "first_byte_timeout_ms": Rule(int, 0, 86_400_000),
    "end_grace_ms": Rule(int, 0, 60_000),
    "max_receive_bytes": Rule(int, 0, MAX_BYTES_LIMIT),
    "strip_null_bytes": Rule(bool),
    "normalize_line_endings": Rule(bool),
    "save_raw": Rule(bool),
    "program_name_pattern": Rule(str, max_length=200),
}

PROTOCOL_RULES: dict[str, Rule] = {
    "mode": Rule(ProtocolMode),
    "transparent": Rule(bool),
    "bcc_mode": Rule(BccMode),
    "enq_interval_ms": Rule(int, 100, 60_000),
    "link_timeout_ms": Rule(int, 100, 3_600_000),
    "ack_timeout_ms": Rule(int, 50, 600_000),
    "drip_feed_ack_timeout_ms": Rule(int, 0, 86_400_000),
    "max_retries": Rule(int, 0, 20),
    "wack_poll_ms": Rule(int, 10, 60_000),
    "lines_per_telegram": Rule(int, 1, 1000),
    "max_telegram_length": Rule(int, 8, 4096),
    "include_line_ending": Rule(bool),
    "start_telegrams": Rule(tuple, maximum=10, max_length=128, printable=True),
    "end_telegrams": Rule(tuple, maximum=10, max_length=128, printable=True),
    "send_eot_on_cancel": Rule(bool),
}

KNOWN_TOP_LEVEL = {
    "schema_version",
    "id",
    "display_name",
    "description",
    "machine",
    "verification_status",
    "serial",
    "transfer",
    "receive",
    "protocol",
}


def _check_value(path: str, value: Any, rule: Rule, errors: list[FieldError]) -> Any:
    kind = rule.kind
    if kind is tuple:
        if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
            errors.append(FieldError(path, "Liste von Texten erwartet"))
            return None
        if rule.maximum is not None and len(value) > rule.maximum:
            errors.append(FieldError(path, f"höchstens {rule.maximum:g} Einträge erlaubt"))
            return None
        for item in value:
            if (rule.max_length is not None and len(item) > rule.max_length) or not _PRINTABLE_RE.match(item):
                errors.append(
                    FieldError(path, f"Eintrag {item!r}: nur druckbare ASCII-Zeichen, max. {rule.max_length}")
                )
                return None
        return tuple(value)
    if issubclass(kind, StrEnum):
        allowed = [m.value for m in kind]
        if not isinstance(value, str) or value.lower() not in allowed:
            errors.append(FieldError(path, f"ungültiger Wert {value!r}, erlaubt: {', '.join(allowed)}"))
            return None
        return kind(value.lower())
    if kind is bool:
        if not isinstance(value, bool):
            errors.append(FieldError(path, f"Wahrheitswert (true/false) erwartet, erhalten {value!r}"))
            return None
        return value
    if kind is int:
        if isinstance(value, bool) or not isinstance(value, int):
            errors.append(FieldError(path, f"Ganzzahl erwartet, erhalten {value!r}"))
            return None
    elif kind is float:
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            errors.append(FieldError(path, f"Zahl erwartet, erhalten {value!r}"))
            return None
        value = float(value)
    elif kind is str:
        if not isinstance(value, str):
            errors.append(FieldError(path, f"Text erwartet, erhalten {value!r}"))
            return None
        if rule.max_length is not None and len(value) > rule.max_length:
            errors.append(FieldError(path, f"Text zu lang (max. {rule.max_length} Zeichen)"))
            return None
        if rule.printable and not _PRINTABLE_RE.match(value):
            errors.append(FieldError(path, "nur druckbare ASCII-Zeichen erlaubt"))
            return None
    if rule.choices is not None and value not in rule.choices:
        shown = ", ".join(str(c) for c in rule.choices)
        errors.append(FieldError(path, f"ungültiger Wert {value!r}, erlaubt: {shown}"))
        return None
    if rule.minimum is not None and value < rule.minimum:
        errors.append(FieldError(path, f"Wert {value} kleiner als Minimum {rule.minimum:g}"))
        return None
    if rule.maximum is not None and value > rule.maximum:
        errors.append(FieldError(path, f"Wert {value} größer als Maximum {rule.maximum:g}"))
        return None
    return value


def _parse_section(
    name: str,
    raw: Any,
    rules: dict[str, Rule],
    cls: type,
    errors: list[FieldError],
    warnings: list[str],
    *,
    required: bool,
) -> Any:
    if raw is None:
        if required:
            errors.append(FieldError(name, "Abschnitt fehlt"))
            return None
        return cls()
    if not isinstance(raw, dict):
        errors.append(FieldError(name, "Objekt erwartet"))
        return None
    values: dict[str, Any] = {}
    for key in raw:
        if key not in rules:
            warnings.append(f"Unbekanntes Feld '{name}.{key}' wird ignoriert.")
    for f in fields(cls):
        path = f"{name}.{f.name}"
        if f.name not in raw:
            default = f.default if f.default is not MISSING else None
            warnings.append(f"Feld '{path}' fehlt - Standardwert {default!r} wird verwendet.")
            continue
        value = _check_value(path, raw[f.name], rules[f.name], errors)
        if value is not None:
            values[f.name] = value
    return cls(**values)


def parse_profile(data: Any, source: str = "", *, builtin: bool = False) -> tuple[Profile, list[str]]:
    """Validate ``data`` and build a :class:`Profile`.

    Returns the profile and a list of non-fatal warnings (structural notes and
    safety warnings). Raises :class:`ProfileValidationError` on fatal errors.
    """
    errors: list[FieldError] = []
    warnings: list[str] = []
    if not isinstance(data, dict):
        raise ProfileValidationError([FieldError("(Datei)", "JSON-Objekt erwartet")], source)

    for key in data:
        if key not in KNOWN_TOP_LEVEL:
            warnings.append(f"Unbekanntes Feld '{key}' wird ignoriert.")

    version = data.get("schema_version")
    if version != SCHEMA_VERSION:
        errors.append(FieldError("schema_version", f"Version {SCHEMA_VERSION} erwartet, erhalten {version!r}"))

    profile_id = data.get("id")
    if not isinstance(profile_id, str) or not PROFILE_ID_RE.match(profile_id):
        errors.append(FieldError("id", "Kleinbuchstaben, Ziffern, '_' oder '-' (max. 64 Zeichen) erwartet"))
    display_name = data.get("display_name")
    if not isinstance(display_name, str) or not display_name.strip() or len(display_name) > 120:
        errors.append(FieldError("display_name", "nicht-leerer Text (max. 120 Zeichen) erwartet"))
    description = data.get("description", "")
    if not isinstance(description, str):
        errors.append(FieldError("description", "Text erwartet"))
        description = ""

    machine_raw = data.get("machine", {})
    machine = MachineInfo()
    if isinstance(machine_raw, dict):
        machine = MachineInfo(
            manufacturer=str(machine_raw.get("manufacturer", machine.manufacturer)),
            model=str(machine_raw.get("model", machine.model)),
            control=str(machine_raw.get("control", machine.control)),
        )
    else:
        errors.append(FieldError("machine", "Objekt erwartet"))

    status_raw = data.get("verification_status", "unverified")
    status = VerificationStatus.UNVERIFIED
    if status_raw in [s.value for s in VerificationStatus]:
        status = VerificationStatus(status_raw)
    else:
        errors.append(FieldError("verification_status", "erwartet 'unverified' oder 'verified'"))

    serial = _parse_section("serial", data.get("serial"), SERIAL_RULES, SerialSettings, errors, warnings, required=True)
    transfer = _parse_section(
        "transfer", data.get("transfer"), TRANSFER_RULES, TransferSettings, errors, warnings, required=True
    )
    receive = _parse_section(
        "receive", data.get("receive"), RECEIVE_RULES, ReceiveSettings, errors, warnings, required=False
    )

    protocol = _parse_section(
        "protocol", data.get("protocol"), PROTOCOL_RULES, ProtocolSettings, errors, warnings, required=False
    )

    if isinstance(receive, ReceiveSettings):
        _check_receive_semantics(receive, errors)

    if errors:
        raise ProfileValidationError(errors, source)

    profile = Profile(
        id=str(profile_id),
        display_name=str(display_name).strip(),
        machine=machine,
        verification_status=status,
        serial=serial,
        transfer=transfer,
        receive=receive,
        protocol=protocol,
        description=description,
        schema_version=SCHEMA_VERSION,
        builtin=builtin,
    )
    warnings.extend(safety_warnings(profile))
    return profile, warnings


def _check_receive_semantics(receive: ReceiveSettings, errors: list[FieldError]) -> None:
    if receive.end_detection.uses_end_sequence and not receive.end_sequence.strip():
        errors.append(FieldError("receive.end_sequence", "darf nicht leer sein, wenn auf Endsequenz geprüft wird"))
    if receive.program_name_pattern:
        try:
            compiled = re.compile(receive.program_name_pattern)
        except re.error as exc:
            errors.append(FieldError("receive.program_name_pattern", f"ungültiger regulärer Ausdruck: {exc}"))
        else:
            if compiled.groups < 1:
                errors.append(FieldError("receive.program_name_pattern", "benötigt eine Gruppe (...) für den Namen"))


def safety_warnings(profile: Profile) -> list[str]:
    """Warnings about unsafe or questionable combinations (never silently accepted)."""
    s = profile.serial
    t = profile.transfer
    r = profile.receive
    out: list[str] = []
    if s.data_bits < 7:
        out.append(f"{s.data_bits} Datenbits: 7-Bit-ASCII-Zeichen sind damit nicht vollständig darstellbar.")
    if s.rts_cts and not s.initial_rts:
        out.append(
            "RTS/CTS ist aktiv, gleichzeitig ist RTS manuell auf 'aus' gesetzt. Bei Hardware-"
            "Handshake steuert der Treiber RTS; ein erzwungener Zustand kann die Gegenstelle blockieren."
        )
    if sum((s.rts_cts, s.xon_xoff, s.dsr_dtr)) > 1:
        out.append("Mehrere Flusssteuerungen gleichzeitig aktiv - ungewöhnliche Kombination.")
    if s.require_cts_before_send and not s.rts_cts:
        out.append("CTS wird vor Sendebeginn geprüft, aber nicht während der Übertragung (kein RTS/CTS).")
    if r.max_receive_bytes == 0:
        out.append("Unbegrenzter Empfang: Es ist keine maximale Empfangsgröße gesetzt.")
    short = []
    if s.read_timeout_ms < 100:
        short.append(f"Read Timeout {s.read_timeout_ms} ms")
    if s.write_timeout_ms < 500:
        short.append(f"Write Timeout {s.write_timeout_ms} ms")
    if 0 < s.handshake_timeout_ms < 1000:
        short.append(f"Handshake-Wartezeit {s.handshake_timeout_ms} ms")
    if r.inactivity_timeout_ms < 1000:
        short.append(f"Empfangs-Inaktivität {r.inactivity_timeout_ms} ms")
    if short:
        out.append("Extrem kurze Timeouts: " + ", ".join(short) + ".")
    if t.append_program_end:
        out.append(
            f"Automatisches Ergänzen von '{t.program_end}' ist aktiviert. Es erfolgt nur nach "
            "ausdrücklicher Bestätigung pro Übertragung."
        )
    if t.program_start.strip():
        out.append(
            f"Startsequenz '{t.program_start.strip()}' ist gesetzt. Ihre Bedeutung für die "
            "DIALOG 11 ist nicht verifiziert (z. B. M82)."
        )
    if t.tab_handling is TabHandling.KEEP:
        out.append("Tabulatoren werden unverändert gesendet - Verhalten der Steuerung unbekannt.")
    if not t.program_end.strip():
        out.append("Kein Programmende definiert - fehlendes Programmende wird nicht erkannt.")
    p = profile.protocol
    if p.is_lsv2:
        out.append(
            "LSV2-Protokoll aktiv: Telegrammaufbau (DLE-Transparenz, BCC, Quittungen) ist für die "
            "DIALOG 11 nicht verifiziert."
        )
        if p.drip_feed_ack_timeout_ms == 0:
            out.append("Nachladebetrieb ohne Zeitlimit für Quittungen der CNC (wartet unbegrenzt).")
        if p.start_telegrams or p.end_telegrams:
            out.append("Start-/Endtelegramme sind gesetzt - ihr Inhalt ist für die DIALOG 11 nicht verifiziert.")
        if p.max_retries == 0:
            out.append("LSV2 ohne Wiederholungen: jeder Übertragungsfehler bricht ab.")
        if not p.include_line_ending:
            out.append("Telegramme ohne Zeilenende - Zeilentrennung erfolgt nur über die Telegrammgrenzen.")
    return out
